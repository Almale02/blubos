use crate::allocator::allocator::GetPhysicalAddr;
use crate::pci::usb::ehci::qtd::{
    EhciQtdPid, EhciQtdStatus, EhciQtdToken, UsbRequestCode, UsbRequestType, UsbSetupPacket,
};
use crate::pci::usb::ehci::registers::*;
use crate::serial_println;
use crate::time::wait_ms;
use alloc::vec::Vec;
use alloc::{boxed::Box, collections::btree_map::BTreeMap};
use core::fmt::{Display, Write};
use core::hint::spin_loop;
use core::ptr::read_volatile;
use core::ptr::write_volatile;
use spin::Once;

use crate::{
    allocator::tree_alloc::TreeAllocLow,
    pci::usb::ehci::{qtd::EhciQtd, queue_head::*},
};

pub struct EndpointQtds {
    pub setup: Box<EhciQtd, TreeAllocLow>,
    pub data: Vec<Box<EhciQtd, TreeAllocLow>>,
    pub status: Box<EhciQtd, TreeAllocLow>,
    pub active_data_qtds: usize,
}
impl Display for EndpointQtds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(
            f,
            "setup qtd at addr: {:#x} is: \n{}",
            self.setup.to_physical_addr(),
            self.setup
        );
        for (i, x) in self.data.iter().enumerate() {
            writeln!(
                f,
                "data at {} and addr: {:#x} is: \n{}",
                i,
                x.to_physical_addr(),
                x
            );
        }
        writeln!(
            f,
            "status qtd at addr: {:#x} is: \n{}",
            self.status.to_physical_addr(),
            self.status
        )
    }
}
impl Default for EndpointQtds {
    fn default() -> Self {
        Self {
            setup: Box::new_in(EhciQtd::default(), TreeAllocLow),
            data: Default::default(),
            status: Box::new_in(EhciQtd::default(), TreeAllocLow),
            active_data_qtds: 0,
        }
    }
}
impl EndpointQtds {
    pub fn build_get_descriptor_req(
        &mut self,
        setup_buff: &mut UsbSetupPacket,
        data_buff: &mut UsbDeviceDescriptor,
        size: u16,
    ) {
        self.active_data_qtds = 1;
        if self.data.is_empty() {
            self.data.push(Box::new_in(EhciQtd::new(), TreeAllocLow));
        }

        let setup_buff_addr = setup_buff.to_physical_addr() as u32;
        let data_buff_addr = data_buff.to_physical_addr() as u32;

        *setup_buff = UsbSetupPacket {
            request_type: UsbRequestType::DEVICE_TO_HOST
                | UsbRequestType::STANDARD
                | UsbRequestType::TO_DEVICE,
            request: UsbRequestCode::GetDescriptor,
            value: 0x0100, // Device descriptor (type 1, index 0)
            index: 0,
            length: size,
        };
        self.setup.token.set_pid(EhciQtdPid::Setup);
        self.setup.token.set_data_toggle(false);
        self.setup.token.set_total_bytes(8);
        self.setup.token.set_cerr(3);
        self.setup.token.set_ioc(false);
        self.setup.set_buffer_page(0, setup_buff_addr);
        self.setup.set_alt_next_qtd_term();
        let data_qtd = &mut *self.data[0];

        data_qtd.token.set_pid(EhciQtdPid::In);
        data_qtd.token.set_data_toggle(true);
        data_qtd.token.set_total_bytes(size);
        data_qtd.token.set_cerr(3);
        data_qtd.token.set_ioc(false);
        data_qtd.set_buffer_page(0, data_buff_addr);
        data_qtd.set_alt_next_qtd_term();

        self.status.token.set_pid(EhciQtdPid::Out);
        self.status.token.set_data_toggle(true);
        self.status.token.set_total_bytes(0);
        self.status.token.set_cerr(3);
        self.status.token.set_ioc(true);
        self.status.set_buffer_page(0, 0); // no data
        self.status.set_alt_next_qtd_term();
        self.status.set_next_qtd(1);
    }
    pub fn build_set_address_req(&mut self, setup_buf: &mut UsbSetupPacket, address: u8) {
        self.active_data_qtds = 0;

        let setup_phys = setup_buf.to_physical_addr() as u32;

        // Fill the Setup packet (always 8 bytes on the wire)
        *setup_buf = UsbSetupPacket {
            request_type: UsbRequestType::TO_DEVICE
                | UsbRequestType::STANDARD
                | UsbRequestType::TO_DEVICE, // host→device, standard, device‑recipient
            request: UsbRequestCode::SetAddress,
            value: address as u16, // new device address
            index: 0,
            length: 0, // no data stage
        };

        // ---- SETUP qTD ----
        self.setup.token.set_pid(EhciQtdPid::Setup);
        self.setup.token.set_data_toggle(false); // DATA0
        self.setup.token.set_total_bytes(8);
        self.setup.token.set_cerr(3);
        self.setup.token.set_ioc(false);
        self.setup.set_buffer_page(0, setup_phys);
        self.setup.set_alt_next_qtd_term();

        // ---- STATUS qTD ----
        self.status.token.set_pid(EhciQtdPid::In); // status is IN for SET_ADDRESS
        self.status.token.set_data_toggle(true);
        self.status.token.set_total_bytes(0);
        self.status.token.set_cerr(3);
        self.status.token.set_ioc(true);
        self.status.set_buffer_page(0, 0);
        self.status.set_alt_next_qtd_term();
        self.status.set_next_qtd(1);
    }
    pub fn link_qtds(&mut self) {
        let n = self.active_data_qtds;
        if n == 0 {
            self.setup.next_qtd = self.status.to_physical_addr() as u32;
            return;
        }

        self.setup.next_qtd = self.data[0].to_physical_addr() as u32;

        for i in 0..(n - 1) {
            let next_phys = self.data[i + 1].to_physical_addr() as u32;
            self.data[i].next_qtd = next_phys;
        }

        self.data[n - 1].next_qtd = self.status.to_physical_addr() as u32;
        self.status.next_qtd = 1;
    }
    pub fn activate(&mut self) {
        self.data
            .iter_mut()
            .for_each(|x| x.token.set_status(EhciQtdStatus::ACTIVE));
        self.status.token.set_status(EhciQtdStatus::ACTIVE);
        self.setup.token.set_status(EhciQtdStatus::ACTIVE);
    }
    pub fn reset(&mut self) {
        *self.status = EhciQtd::new();
        *self.setup = EhciQtd::new();
        self.data.iter_mut().for_each(|x| **x = EhciQtd::new());
    }
}

pub static mut EHCI_CONTROLLER: Once<EhciController> = Once::new();
pub struct EhciController {
    mmio_base: *mut u8,
    pub qh_map: BTreeMap<DeviceEndpointRef, Box<EhciQueueHead, TreeAllocLow>>,
    pub anchor_qh: Box<EhciQueueHead, TreeAllocLow>,
    pub qtd_map: BTreeMap<DeviceEndpointRef, EndpointQtds>,
    /// None if it is the anchor
    last_qh: Option<DeviceEndpointRef>,
}
unsafe impl Sync for EhciController {}
unsafe impl Send for EhciController {}

impl EhciController {
    pub fn dump(&self, mut writer: impl Write) {
        writeln!(&mut writer, "anchor qh: {}", self.anchor_qh);
        writeln!(
            &mut writer,
            "anchor addr: {:#x}",
            self.anchor_qh.to_physical_addr()
        );
        for (ep_ref, qh) in self.qh_map.iter() {
            let qtds = self.qtd_map.get(ep_ref).unwrap();
            writeln!(&mut writer, "at ep: {:?}, qtds: \n{}", ep_ref, qtds);
            writeln!(
                &mut writer,
                "at ep: {:?}, addr: {:#x}, qh: \n{}",
                ep_ref,
                qh.to_physical_addr(),
                qh
            );
        }
    }
    pub fn new(mmio_base: *mut u8) -> Self {
        Self {
            mmio_base,
            qh_map: Default::default(),
            qtd_map: Default::default(),
            anchor_qh: {
                let mut qh = Self::new_anchor_head();
                let mut phys = qh.to_physical_addr() as u32;
                phys &= !0x1F;
                phys |= 2; // qh type

                qh.horiz_link = phys;
                qh.ep_char.set_head(true); // Head of Reclamation

                qh
            },
            last_qh: None,
        }
    }
    pub fn new_queue_head(&mut self) -> Box<EhciQueueHead, TreeAllocLow> {
        Box::new_in(
            EhciQueueHead {
                horiz_link: 0,
                ep_char: EhciQhEpChar::new(),
                ep_caps: EhciQhEpCaps::new(),
                curr_qtd: 0,
                next_qtd: 1,     // terminate
                alt_next_qtd: 1, // terminate
                token: EhciQtdToken::new(),
                buffer_pages: [0; 5],
            },
            TreeAllocLow,
        )
    }
    pub fn set_queue_head_next(&mut self, qh: DeviceEndpointRef, next: Option<DeviceEndpointRef>) {
        if let Some(next) = next {
            let next = self.qh_map.get(&next).unwrap();
            let mut phys = next.to_physical_addr() as u32;

            let curr = self.qh_map.get_mut(&qh).unwrap();

            phys &= !0x1F;
            phys |= 2;
            curr.horiz_link = phys;
        } else {
            let anchor_ref = self.get_anchor_ref();
            let curr = self.qh_map.get_mut(&qh).unwrap();
            curr.horiz_link = anchor_ref;
        }
    }
    pub fn set_anchor_head_next(&mut self, next: Option<DeviceEndpointRef>) {
        if let Some(next) = next {
            let next = self.qh_map.get(&next).unwrap();
            let mut phys = next.to_physical_addr() as u32;

            phys &= !0x1F;
            phys |= 2;
            self.anchor_qh.horiz_link = phys;
        } else {
            let anchor_ref = self.get_anchor_ref();
            self.anchor_qh.horiz_link = anchor_ref;
        }
    }
    pub fn set_device_control_queue_head(&mut self, ep_ref: DeviceEndpointRef) {
        let qh = self.get_qh_mut(ep_ref);
        qh.ep_char.set_max_packet_size(8);
        qh.ep_char.set_eps(2);
        qh.ep_char.set_control(true);
        qh.ep_char.set_dtc(true);
    }
    pub fn new_anchor_head() -> Box<EhciQueueHead, TreeAllocLow> {
        let mut qh = Box::new_in(
            EhciQueueHead {
                horiz_link: 0,
                ep_char: EhciQhEpChar::new(),
                ep_caps: EhciQhEpCaps::new(),
                curr_qtd: 0,
                next_qtd: 1,     // terminate
                alt_next_qtd: 1, // terminate
                token: EhciQtdToken::new(),
                buffer_pages: [0; 5],
            },
            TreeAllocLow,
        );
        let mut phys = qh.to_physical_addr() as u32;
        phys &= !0x1F;
        phys |= 2; // qh type

        qh.horiz_link = phys;
        qh.ep_char.set_head(true);
        qh.ep_char.set_eps(2);

        qh
    }
    pub fn new_queue_head_for_ep(&mut self, ep_ref: DeviceEndpointRef) -> &mut EhciQueueHead {
        let mut qh = self.new_queue_head();
        qh.ep_char.set_dev_addr(ep_ref.device_addr);
        qh.ep_char.set_endpt(ep_ref.endpoint);
        if self.qh_map.contains_key(&ep_ref) {
            *self.qh_map.get_mut(&ep_ref).unwrap() = qh;
        } else {
            self.qh_map.insert(ep_ref, qh);
        }
        if let Some(last_qh) = self.last_qh {
            self.set_queue_head_next(last_qh, Some(ep_ref));
        } else {
            self.set_anchor_head_next(Some(ep_ref));
        }
        self.set_queue_head_next(ep_ref, None);
        self.last_qh = Some(ep_ref);
        self.add_qtds_to_endpoint(ep_ref);
        self.get_qh_mut(ep_ref)
    }
    pub fn activate_overlay(&mut self, ep_ref: DeviceEndpointRef) {
        let ehci_queue_head = self.qh_map.get_mut(&ep_ref).unwrap();
        ehci_queue_head.token = EhciQtdToken::new();
        ehci_queue_head.token.set_data_toggle(false);
        ehci_queue_head.token.set_cerr(3);
        ehci_queue_head.token.set_pid(EhciQtdPid::Setup);
        ehci_queue_head.token.set_status(EhciQtdStatus::ACTIVE);
        ehci_queue_head.next_qtd =
            self.qtd_map.get(&ep_ref).unwrap().setup.to_physical_addr() as u32;
    }
    pub fn get_qh_ref(&self, ep_ref: DeviceEndpointRef) -> &EhciQueueHead {
        self.qh_map
            .get(&ep_ref)
            .unwrap_or_else(|| panic!("did not found qh at endpoint: {:?}", ep_ref))
    }
    pub fn get_qh_mut(&mut self, ep_ref: DeviceEndpointRef) -> &mut EhciQueueHead {
        self.qh_map
            .get_mut(&ep_ref)
            .unwrap_or_else(|| panic!("did not found qh at endpoint: {:?}", ep_ref))
    }
    pub fn get_anchor_ref(&self) -> u32 {
        (self.anchor_qh.to_physical_addr() as u32) | 2
    }
    pub fn get_opreg_ptr(&self) -> *mut u8 {
        unsafe { self.mmio_base.add(self.get_caplen() as usize) }
    }
    pub fn get_interrupt_ptr(&self) -> *mut u32 {
        unsafe { self.get_opreg_ptr().add(0x8) as *mut u32 }
    }
    pub fn get_port_control_ptr(&self, port: usize) -> *mut u32 {
        unsafe { self.get_opreg_ptr().add(0x44 + port * 4) as *mut u32 }
    }
    pub fn get_cmd_ptr(&self) -> *mut u32 {
        self.get_opreg_ptr() as *mut u32
    }
    pub fn get_status_ptr(&self) -> *mut u32 {
        unsafe { self.get_opreg_ptr().add(0x4) as *mut u32 }
    }
    pub fn get_hcs_ptr(&self) -> *mut u32 {
        unsafe { self.mmio_base.add(0x4) as *mut _ }
    }
    pub fn get_caplen(&self) -> u8 {
        unsafe { read_volatile(self.mmio_base) }
    }
    pub fn get_port_count(&self) -> u32 {
        unsafe { read_volatile(self.get_hcs_ptr()) & 0xF }
    }
    pub fn set_config_flag(&mut self) {
        unsafe {
            write_volatile(self.get_opreg_ptr().add(0x40) as *mut u32, 1);
        }
    }
    pub fn set_asynclistaddr_ptr(&self, addr: usize) {
        unsafe {
            write_volatile(self.get_opreg_ptr().add(0x18) as *mut u32, addr as u32);
        }
    }
    pub fn get_asynclistaddr_register(&self) -> u32 {
        unsafe { read_volatile(self.get_opreg_ptr().add(0x18) as *mut u32) }
    }
    ///
    pub fn get_cmd_flags(&self) -> EhciCmd {
        unsafe { EhciCmd::from_bits_truncate(read_volatile(self.get_cmd_ptr())) }
    }
    pub fn write_cmd_flags(&mut self, flags: EhciCmd) {
        unsafe {
            write_volatile(self.get_cmd_ptr(), flags.bits());
        }
    }
    pub fn add_cmd_flags(&mut self, flags: EhciCmd) {
        self.write_cmd_flags(self.get_cmd_flags() | flags);
    }
    pub fn remove_cmd_flags(&mut self, flags: EhciCmd) {
        self.write_cmd_flags(self.get_cmd_flags() & !flags);
    }
    ///
    pub fn get_status_flags(&self) -> EhciStatus {
        unsafe { EhciStatus::from_bits_truncate(read_volatile(self.get_status_ptr())) }
    }
    pub fn write_status_flags(&mut self, flags: EhciStatus) {
        unsafe {
            write_volatile(self.get_status_ptr(), flags.bits());
        }
    }
    pub fn add_status_flags(&mut self, flags: EhciStatus) {
        self.write_status_flags(self.get_status_flags() | flags);
    }
    pub fn remove_status_flags(&mut self, flags: EhciStatus) {
        self.write_status_flags(self.get_status_flags() & !flags);
    }
    pub fn get_interrupt_flags(&self) -> EhciIntr {
        unsafe { EhciIntr::from_bits_truncate(read_volatile(self.get_interrupt_ptr())) }
    }
    pub fn write_interrupt_flags(&mut self, flags: EhciIntr) {
        unsafe {
            write_volatile(self.get_interrupt_ptr(), flags.bits());
        }
    }
    pub fn add_interrupt_flags(&mut self, flags: EhciIntr) {
        self.write_interrupt_flags(self.get_interrupt_flags() | flags);
    }
    pub fn remove_interrupt_flags(&mut self, flags: EhciIntr) {
        self.write_interrupt_flags(self.get_interrupt_flags() & !flags);
    }
    pub fn get_port_control_flags(&self, port: usize) -> EhciPortControl {
        unsafe {
            EhciPortControl::from_bits_truncate(read_volatile(self.get_port_control_ptr(port)))
        }
    }

    pub fn get_port_speed(&self, port: usize) -> EhciPortSpeed {
        let port_reg = unsafe { core::ptr::read_volatile(self.get_port_control_ptr(port)) };

        let port_owner = self
            .get_port_control_flags(port)
            .contains(EhciPortControl::PORT_OWNER) as u8;
        let line_status = (port_reg >> 10) & 0b11;

        match (port_owner, line_status) {
            // EHCI owns the port → always high‑speed
            (0, _) => EhciPortSpeed::High,
            // Companion owns: check line state
            (1, 0b10) => EhciPortSpeed::Full,
            (1, 0b01) => EhciPortSpeed::Low,
            _ => {
                serial_println!("port owner: {}", port_owner);
                serial_println!("line status: {}", line_status);
                panic!("unknown port speed")
            }
        }
    }

    pub fn write_port_control_flags(&mut self, port: usize, flags: EhciPortControl) {
        unsafe {
            write_volatile(self.get_port_control_ptr(port), flags.bits());
        }
    }
    pub fn add_port_control_flags(&mut self, port: usize, flags: EhciPortControl) {
        self.write_port_control_flags(port, self.get_port_control_flags(port) | flags);
    }
    pub fn remove_port_control_flags(&mut self, port: usize, flags: EhciPortControl) {
        self.write_port_control_flags(port, self.get_port_control_flags(port) & !flags);
    }
    pub fn get_ehci_capability(&self) -> EhciControllerCapability {
        unsafe {
            EhciControllerCapability::from_bits_truncate(read_volatile(
                self.mmio_base.add(0x8) as *const u32
            ))
        }
    }
    pub fn reset_port(&mut self, port: usize) {
        self.add_port_control_flags(port, EhciPortControl::RESET);
        wait_ms(30);
        self.remove_port_control_flags(port, EhciPortControl::RESET);
        while !self
            .get_port_control_flags(port)
            .contains(EhciPortControl::ENABLED)
        {
            spin_loop();
        }
        self.add_port_control_flags(port, EhciPortControl::ENABLE_CHANGE);
    }
    pub fn reset_controller(&mut self) {
        self.add_cmd_flags(EhciCmd::RESET);
        while self.get_cmd_flags().contains(EhciCmd::RESET) {
            spin_loop();
        }
    }
    pub fn disable_ass(&mut self) {
        self.remove_cmd_flags(EhciCmd::ASYNC_SCHEDULE_ENABLE);
        while self.get_status_flags().contains(EhciStatus::ASYNC_STATUS) {
            spin_loop();
        }
    }
    pub fn enable_ass(&mut self) {
        self.add_cmd_flags(EhciCmd::ASYNC_SCHEDULE_ENABLE);
        while !self.get_status_flags().contains(EhciStatus::ASYNC_STATUS) {
            spin_loop();
        }
    }
}
impl EhciController {
    pub fn add_qtds_to_endpoint(&mut self, ep: DeviceEndpointRef) {
        if !self.qtd_map.contains_key(&ep) {
            self.qtd_map.insert(ep, EndpointQtds::default());
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceEndpointRef {
    pub device_addr: u8,
    pub endpoint: u8,
}

impl DeviceEndpointRef {
    pub fn new(device_addr: u8, endpoint: u8) -> Self {
        Self {
            device_addr,
            endpoint,
        }
    }
}
#[repr(C, packed)]
#[derive(Copy, Clone, Debug, Default)]
pub struct UsbDeviceDescriptorHeader {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub bcd_usb: u16,
    pub b_device_class: u8,
    pub b_device_sub_class: u8,
    pub b_device_protocol: u8,
    pub b_max_packet_size0: u8,
}
#[repr(C, packed)]
#[derive(Copy, Clone, Debug, Default)]
pub struct UsbDeviceDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub bcd_usb: u16,
    pub b_device_class: u8,
    pub b_device_sub_class: u8,
    pub b_device_protocol: u8,
    pub b_max_packet_size0: u8,
    pub id_vendor: u16,
    pub id_product: u16,
    pub bcd_device: u16,
    pub i_manufacturer: u8,
    pub i_product: u8,
    pub i_serial_number: u8,
    pub b_num_configurations: u8,
}
/*

























*/
