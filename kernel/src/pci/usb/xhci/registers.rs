use core::{fmt::Display, mem};

use bit_field::BitField;
use memoffset::offset_of;
use modular_bitfield::prelude::*;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XhciHostControllerCaps {
    pub caplen: u8,
    _r1: u8,
    pub version: u16,
    pub struct_param_1: XhciStructuralParam1,
    pub struct_param_2: XhciStructuralParam2,
    pub struct_param_3: XhciStructuralParam3,
    pub cap_params_1: XhciCapabilityParams1,
    pub doorbell_offset: XhciDoorbellArrayOffset,
    pub runtime_regs_offset: u32,
    pub cap_params_2: XhciCapabilityParams2,
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciStructuralParam1 {
    pub max_device_slots: u8,
    pub max_interrupts: B11,
    #[skip]
    __: B5,
    pub max_ports: u8,
}

#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciStructuralParam2 {
    pub ist: B4,
    pub event_ring_seg_table_max: B4,
    #[skip]
    __: B13,
    pub max_scratchpad_buffer_hi: B5,
    pub scratchpad_restore: bool,
    pub max_scratchpad_buffer_low: B5,
}
impl XhciStructuralParam2 {
    pub fn get_ist(&self) -> XhciIst {
        match self.ist().get_bit(3) {
            true => XhciIst::Frame(self.ist().get_bits(0..=2) as u8),
            false => XhciIst::MicroFrame(self.ist().get_bits(0..=2) as u8),
        }
    }
    pub fn get_max_scratchpad_buffer(&self) -> u16 {
        ((self.max_scratchpad_buffer_hi() as u16) << 5) | self.max_scratchpad_buffer_low() as u16
    }
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciStructuralParam3 {
    pub u1_device_exit_latency: u8,
    #[skip]
    __: u8,
    pub u2_device_exit_latency: u16,
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciCapabilityParams1 {
    pub addressing_mode: AddressMode,
    pub bw_negotation: bool,
    /// if true 64bit, else 32bit
    pub context_size_mode: AddressMode,
    pub port_power_control: bool,
    pub port_indicators: bool,
    pub light_reset_cap: bool,
    pub latency_tolarance_messaging: bool,
    pub no_secondary_sid: bool,
    pub parse_all_events: bool,
    pub stopped_short_packet: bool,
    pub stopped_edtla: bool,
    pub contigous_frame_id: bool,
    pub max_primary_stream_array_size: B4,
    pub extended_caps_pointer: u16,
}
#[derive(Specifier, Debug, Copy, Clone)]
#[repr(u8)]
#[bits = 1]
pub enum AddressMode {
    Bit32 = 0,
    Bit64 = 1,
}
#[allow(non_snake_case)]
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciDoorbellArrayOffset {
    #[skip]
    __: B1,
    pub offset: B31,
}
#[allow(non_snake_case)]
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciCapabilityParams2 {
    pub u3_entry: bool,
    pub cmc: bool,
    pub force_save_context: bool,
    pub compliance_transition: bool,
    pub large_esit_payload: bool,
    pub configuration_information: bool,
    pub extended_tbc: bool,
    pub extended_tbc_tbr_status: bool,
    pub get_set_extended_property_commands: bool,
    pub virtualization_based_trusted_io: bool,
    #[skip]
    __: B22,
}

pub enum XhciIst {
    MicroFrame(u8),
    Frame(u8),
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XhciOpRegisters {
    pub usb_cmd: XhciUsbCmd,
    pub usb_status: XhciUsbStatus,
    pub page_size: u32,
    _r1: [u8; 8],
    pub notification_enable: u16,
    _r2: [u8; 2],
    pub command_ring_control: XhciCommandRingControl,
    _r3: [u8; 16],
    pub device_context_base_address_array: u64,
    pub configure: XhciConfigure,
}
impl Default for XhciOpRegisters {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}
const _: () = assert!(size_of::<XhciUsbCmd>() == 4);
const _: () = assert!(size_of::<XhciUsbStatus>() == 4);
const _: () = assert!(size_of::<XhciCommandRingControl>() == 8);
const _: () = assert!(size_of::<XhciConfigure>() == 4);
const _: () = assert!(offset_of!(XhciOpRegisters, configure) == 0x38);
impl Display for XhciOpRegisters {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("op regs:\n");
        f.write_str("\tusb cmd:\n");
        f.write_fmt(format_args!("\t\t{:?}\n", self.usb_cmd));
        f.write_str("\tusb status: \n");
        f.write_fmt(format_args!("\t\t{:?}\n", self.usb_status));
        f.write_fmt(format_args!("\t page size: {}\n", self.page_size));
        f.write_fmt(format_args!(
            "\tnotification enable: {:#x}\n",
            self.notification_enable
        ));
        f.write_str("\tcommand ring control\n");
        f.write_fmt(format_args!("\t\t{:?}\n", self.command_ring_control));
        f.write_fmt(format_args!(
            "\t\t{:#x}\n",
            self.command_ring_control.command_ring_pointer() << 6
        ));
        f.write_fmt(format_args!(
            "\tdevice context base address array: {:#x}\n",
            self.device_context_base_address_array
        ));
        f.write_str("\tconfigure:\n");
        f.write_fmt(format_args!("\t\t{:?}\n", self.configure))
    }
}

#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciUsbCmd {
    pub run: bool,
    pub reset: bool,
    pub interrupt_enable: bool,
    pub host_system_error_enable: bool,
    #[skip]
    __: B3,
    pub light_host_controller_reset: bool,
    pub controller_save_state: bool,
    pub controller_restore_state: bool,
    pub enable_wrap_event: bool,
    pub enable_u3_mfindex_stop: bool,
    #[skip]
    __: B1,
    pub enable_cem: bool,
    pub extended_tbc_enable: bool,
    pub extended_tbc_trb_status_enable: bool,
    pub vtio_enable: bool,
    #[skip]
    __: B15,
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciUsbStatus {
    #[skip(setters)]
    pub host_controller_halted: bool,
    #[skip]
    __: B1,
    pub host_system_error: bool,
    pub event_interrupt: bool,
    pub port_changed: bool,
    #[skip]
    __: B3,
    #[skip(setters)]
    pub save_state_status: bool,
    #[skip(setters)]
    pub restore_state_status: bool,
    pub save_restore_error: bool,
    #[skip(setters)]
    pub controller_not_ready: bool,
    #[skip(setters)]
    pub host_controller_error: bool,
    #[skip]
    __: B19,
}
impl XhciUsbStatus {
    pub fn zero_w1c(&mut self) -> &mut Self {
        self.set_host_system_error(false);
        self.set_event_interrupt(false);
        self.set_port_changed(false);
        self.set_save_restore_error(false);
        self
    }
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciCommandRingControl {
    pub ring_cycle_state: bool,
    pub command_stop: bool,
    pub command_abort: bool,
    #[skip(setters)]
    pub command_ring_running: bool,
    #[skip]
    __: B2,
    pub command_ring_pointer: B58,
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciConfigure {
    pub device_slots_enabled: u8,
    pub u3_entry_enable: bool,
    pub config_info_enable: bool,
    #[skip]
    __: B22,
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciPortControl {
    pub current_connect_status: bool,
    pub port_enabled: bool,
    #[skip]
    __: B1,
    #[skip(setters)]
    pub over_current_active: bool,
    pub port_reset: bool,
    pub port_link_state: B4,
    pub port_power: bool,
    pub port_speed: B4,
    pub port_indicator: B2,
    pub port_link_state_write_strobe: bool,
    pub connected_status_change: bool,
    pub port_enabled_chnage: bool,
    pub warm_port_reset_change: bool,
    pub over_current_change: bool,
    pub port_reset_change: bool,
    pub port_link_state_change: bool,
    pub port_config_error_change: bool,
    pub cold_attach_status: bool,
    pub wake_on_connect_enable: bool,
    pub wake_on_disconnect_enable: bool,
    pub wake_on_over_current_enable: bool,
    #[skip]
    __: B2,
    pub device_removable: bool,
    pub warm_port_reset: bool,
}
impl XhciPortControl {
    pub fn zero_w1c(&mut self) -> &mut Self {
        self.set_connected_status_change(false);
        self.set_port_enabled_chnage(false);
        self.set_warm_port_reset_change(false);
        self.set_over_current_change(false);
        self.set_port_reset_change(false);
        self.set_port_link_state_change(false);
        self.set_port_config_error_change(false);
        self
    }
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciPortPowerControl {
    pub u1_timeout: u8,
    pub u2_timeout: u8,
    pub force_link_pm_accept: bool,
    #[skip]
    __: B15,
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciPortLinkInfo {
    pub link_error_count: u16,
    pub rx_lane_count: B4,
    pub tx_lane_count: B4,
    #[skip]
    __: B8,
}
#[modular_bitfield::bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciPortHardwareLPMControl {
    pub host_inited_resume_mode: B2,
    pub l1_timeout: B8,
    pub best_effort_service_latency_deep: B4,
    #[skip]
    __: B18,
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XhciPortRegisters {
    pub port_control: XhciPortControl,
    pub port_power_control: XhciPortPowerControl,
    pub port_link_info: XhciPortLinkInfo,
    pub port_lpm_control: XhciPortHardwareLPMControl,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XhciInterrupterRegister {
    pub iman: XhciInterrupterManagement,
    pub i_mod: XhciInterrupterModeration,
    pub erst_size: u32,
    _rsvd: u32,
    pub event_ring_segment_table_address: u64,
    pub event_ring_dequeue_ptr: XhciEventRingDequeuePtr,
}
#[bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciEventRingDequeuePtr {
    pub segment_idx: B3,
    pub handler_busy: bool,
    pub dequeue_ptr: B60,
}
impl XhciEventRingDequeuePtr {
    pub fn from_ptr(ptr: u64) -> Self {
        Self::new().with_dequeue_ptr(ptr >> 4)
    }
    pub fn zero_w1c(&mut self) -> &mut Self {
        self.set_handler_busy(false);
        self
    }
}

#[bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciInterrupterManagement {
    pub interrupt_pending: bool,
    pub interrupt_enable: bool,
    #[skip]
    __: B30,
}
impl XhciInterrupterManagement {
    pub fn zero_w1c(&mut self) -> &mut Self {
        self.set_interrupt_pending(false);
        self
    }
}

#[bitfield]
#[derive(Debug, Copy, Clone)]
pub struct XhciInterrupterModeration {
    pub interval: u16,
    pub counter: u16,
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XhciRuntimeRegisters {
    pub mf_index: u32,
    _r1: [u32; 7],
    pub interrupters: [XhciInterrupterRegister; 1],
}
impl Default for XhciRuntimeRegisters {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

impl Display for XhciRuntimeRegisters {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("mf index: {}\n", self.mf_index));
        f.write_str("interrupter:\n");
        f.write_fmt(format_args!(
            "\t segment table address: {:#x}\n",
            self.interrupters[0].event_ring_segment_table_address
        ));
        f.write_fmt(format_args!(
            "\t event ring dequeue ptr: {:?}\n",
            self.interrupters[0].event_ring_dequeue_ptr
        ));
        f.write_fmt(format_args!(
            "\t event ring dequeue ptr address: {:#x}\n",
            self.interrupters[0].event_ring_dequeue_ptr.dequeue_ptr()
        ));
        f.write_fmt(format_args!(
            "\t event ring size: {:#x}\n",
            self.interrupters[0].erst_size
        ))
    }
}
