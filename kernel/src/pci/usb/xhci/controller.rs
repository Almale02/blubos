use core::hint::spin_loop;

use alloc::{boxed::Box, vec::Vec};
use modular_bitfield::{bitfield, prelude::*};
use spin::Once;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Translate as _},
};

use crate::{
    allocator::{
        allocator::{AlignedBoxAlloc, GetPhysicalAddr},
        paging::PAGE_TABLE_MAPPER,
        xhci::{XHCI_ADDRESS_MODE, XhciAllocator},
    },
    byte_ext::ByteExt as _,
    pci::{
        pci::PciValue,
        usb::xhci::{
            registers::{
                XhciHostControllerCaps, XhciOpRegisters, XhciPortRegisters, XhciRuntimeRegisters,
            },
            ring::{XHCI_COMMAND_RING, XhciCommandRing},
        },
    },
    serial_println,
    time::wait_ms,
};

pub static mut XHCI_CONTROLLER: Once<XhciController> = Once::new();
pub struct XhciController {
    mmio_base: *mut u8,
    pub capabilities: PciValue<XhciHostControllerCaps>,
    pub op_registers: PciValue<XhciOpRegisters>,
    pub runtime_registers: PciValue<XhciRuntimeRegisters>,
    pub dcbaa: XhciDcbaa,
}
impl XhciController {
    pub fn new(mmio_base: *mut u8) -> XhciController {
        let caps = PciValue::<XhciHostControllerCaps>::new(mmio_base).read();
        let op_base = unsafe { mmio_base.add(caps.caplen as usize) };
        let runtime_base = unsafe { mmio_base.add(caps.runtime_regs_offset as usize) };
        unsafe { XHCI_ADDRESS_MODE.call_once(|| caps.cap_params_1.addressing_mode()) };
        XhciController {
            mmio_base,
            capabilities: PciValue::new(mmio_base),
            op_registers: PciValue::new(op_base),
            runtime_registers: PciValue::new(runtime_base),
            dcbaa: XhciDcbaa::new(),
        }
    }
    pub fn get() -> &'static XhciController {
        unsafe { XHCI_CONTROLLER.get().unwrap() }
    }
    pub fn get_mut() -> &'static mut XhciController {
        unsafe { XHCI_CONTROLLER.get_mut().unwrap() }
    }
    /// first port is 1
    pub fn get_port_reg(&self, port: usize) -> PciValue<XhciPortRegisters> {
        let op_base = self.get_op_base();
        let port_reg_base = unsafe { op_base.add(0x400 + (0x10 * (port - 1))) };
        PciValue::new(port_reg_base)
    }
    pub fn get_op_base(&self) -> *mut u8 {
        unsafe { self.mmio_base.add(self.capabilities.read().caplen as usize) }
    }
    pub fn reset(&mut self) {
        self.op_registers.modify(|x| x.usb_cmd.set_run(false));
        while !self.op_registers.read().usb_status.host_controller_halted() {
            spin_loop();
        }
        self.op_registers.modify(|x| x.usb_cmd.set_reset(true));
        serial_println!("reset bit: {}", self.op_registers.read().usb_cmd.reset());
        while self.op_registers.read().usb_cmd.reset()
            || self.op_registers.read().usb_status.controller_not_ready()
        {
            spin_loop();
        }
        wait_ms(50);
    }
    pub fn start(&mut self) -> Result<(), ()> {
        self.op_registers.modify(|op| {
            op.usb_cmd.set_run(true);
            op.usb_cmd.set_interrupt_enable(true);
        });
        while self.op_registers.read().usb_status.host_controller_halted() {
            wait_ms(1);
        }
        if self.op_registers.read().usb_status.controller_not_ready() {
            return Err(());
        }
        Ok(())
    }
    pub fn setup_op_regs(&mut self) {
        let mapper = unsafe { PAGE_TABLE_MAPPER.get_mut().unwrap() };
        let command_ring = XhciCommandRing::get();

        self.op_registers.modify(|x| {
            x.configure.set_device_slots_enabled(
                self.capabilities.read().struct_param_1.max_device_slots(),
            );
            x.notification_enable = 0xffff;
            let physical_dcbaa = mapper
                .translate_addr(VirtAddr::new(self.dcbaa.dcbaa.as_ptr() as usize as u64))
                .unwrap()
                .as_u64();
            x.device_context_base_address_array = physical_dcbaa;
            serial_println!("dcbaa: {}", physical_dcbaa);
        });
        let max_device_slots = self.capabilities.read().struct_param_1.max_device_slots() as usize;
        let scratchpad_count = self
            .capabilities
            .read()
            .struct_param_2
            .get_max_scratchpad_buffer() as usize;

        self.dcbaa
            .device_contexes
            .reserve_exact(max_device_slots + 1);
        for i in 1..=max_device_slots {
            let device_context = unsafe {
                Box::new_in(
                    core::mem::zeroed::<XhciDeviceContext>(),
                    XhciAllocator::default(),
                )
            };
            let physical = mapper
                .translate_addr(VirtAddr::new(device_context.as_ref() as *const _ as u64))
                .unwrap()
                .as_u64();
            assert!(physical & 0x3f == 0);
            self.dcbaa.dcbaa[i] = physical;
            self.dcbaa.device_contexes.push(device_context);
        }
        if scratchpad_count > 0 {
            self.dcbaa.scratchpads.reserve_exact(scratchpad_count);
            for i in 0..scratchpad_count {
                let frame = Box::new_in_aligned([0u8; 4.kb()], XhciAllocator::default(), 4.kb());
                let addr = frame.to_physical_addr();
                self.dcbaa.scratchpads.push(frame);
                self.dcbaa.scratchpad_pointers[i] = addr as u64;
            }
            self.dcbaa.dcbaa[0] = self.dcbaa.scratchpad_pointers.to_physical_addr() as u64;
        }
        self.op_registers.modify(|x| {
            x.command_ring_control
                .set_command_ring_pointer((command_ring.trb_physical_base >> 6) as u64);
            x.command_ring_control
                .set_ring_cycle_state(command_ring.cycle_bit);
        });
    }
    pub fn setup_runtime_regs(&mut self) {
        self.runtime_registers.modify(|runtime| {
            runtime.interrupters[0].iman.set_interrupt_enable(true);
        });
        self.ack_interrupt(0);
    }
    pub fn ack_interrupt(&mut self, interrupter: usize) {
        self.op_registers.modify(|op| {
            op.usb_status.set_event_interrupt(true);
        });
        self.runtime_registers.modify(|runtime| {
            runtime.interrupters[interrupter]
                .iman
                .zero_w1c()
                .set_interrupt_pending(true);
        });
    }
}
pub struct XhciDcbaa {
    /// first pointer points to scratchpad, the rest points to DeviceContext
    ///  all pointers including the Box itself needs to be 64B aligned
    pub dcbaa: Box<[u64], XhciAllocator>,
    pub device_contexes: Vec<Box<XhciDeviceContext, XhciAllocator>>,
    pub scratchpads: Vec<Box<[u8; 4096], XhciAllocator>>,
    pub scratchpad_pointers: Box<[u64], XhciAllocator>,
}
impl XhciDcbaa {
    pub fn new() -> Self {
        Self {
            dcbaa: Box::new_in_aligned([0; 256], XhciAllocator::default(), 64),
            device_contexes: Vec::new(),
            scratchpads: Vec::new(),
            scratchpad_pointers: Box::new_in_aligned([0; 256], XhciAllocator::default(), 64),
        }
    }
}
#[repr(C, align(64))]
pub struct XhciDeviceContext {
    pub slot_context: XhciSlotContext,
    pub endpoint_context: [XhciEndpointContext; 31],
}
#[bitfield]
pub struct XhciSlotContext {
    pub route_string: B20,
    pub speed: B4,
    #[skip]
    __: B1,
    pub mtt: bool,
    pub hub: bool,
    pub context_entries: B5,
    pub max_exit_latency: u16,
    pub root_hub_port_number: u8,
    pub number_of_ports: u8,
    pub parent_hub_slot_id: u8,
    pub parent_port_number: u8,
    pub tt_think_time: B2,
    #[skip]
    __: B4,
    pub interrupt_target: B10,
    pub usb_device_address: u8,
    #[skip]
    __: B19,
    pub slot_state: XhciSlotState,
    #[skip]
    __: B128,
}
const _: () = assert!(size_of::<XhciDeviceContext>() == 1024);
const _: () = assert!(size_of::<XhciSlotContext>() == 32);

#[bitfield]
pub struct XhciEndpointContext {
    pub endpoint_state: XhciEndpointState,
    #[skip]
    __: B5,
    pub mult: B2,
    pub max_primary_streams: B5,
    pub linear_stream_array: bool,
    pub interval: u8,
    pub max_enpoint_sti_payload_high: u8,
    #[skip]
    __: B1,
    pub error_count: B2,
    pub endpoint_type: B3,
    #[skip]
    __: B1,
    pub host_inited_disable: bool,
    pub max_burst_size: u8,
    pub max_packet_size: u16,
    pub dequeue_cycle_state: bool,
    #[skip]
    __: B3,
    pub transfer_ring_dequeue_ptr: B60,
    pub avarage_trb_length: u16,
    pub max_endpoint_sti_payload_low: u16,
    #[skip]
    __: B96,
}
const _: () = assert!(size_of::<XhciEndpointContext>() == 32);
#[derive(Specifier)]
#[bits = 5]
pub enum XhciSlotState {
    Disabled = 0,
    Default = 1,
    Addressed = 2,
    Configured = 3,
}
#[derive(Specifier)]
#[bits = 3]
pub enum XhciEndpointState {
    Disabled = 0,
    Running = 1,
    Halted = 2,
    Stopped = 3,
    Error = 4,
}
#[derive(Specifier)]
#[bits = 3]
pub enum XhciEndpointType {
    NotValid = 0,
    IsochOut = 1,
    BulkOut = 2,
    InterrupOut = 3,
    Control = 4,
    IsochIn = 5,
    BulkIn = 6,
    InterrupIn = 7,
}

/*

























*/
