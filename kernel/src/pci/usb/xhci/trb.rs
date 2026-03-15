use modular_bitfield::prelude::*;

use crate::serial_println;

#[derive(Clone, Copy, Debug)]
pub enum XhciTrb {
    Normal(XhciNormalTrb),
    Link(XhciLinkTrb),
    Empty(XhciEmptyTrb),
}
impl XhciTrb {
    pub fn set_cycle_bit(&mut self, bit: bool) {
        match self {
            XhciTrb::Normal(xhci_normal_trb) => xhci_normal_trb.set_cycle_bit(bit),
            XhciTrb::Link(xhci_link_trb) => xhci_link_trb.set_cycle_bit(bit),
            XhciTrb::Empty(xhci_empy_trb) => xhci_empy_trb.set_cycle_bit(bit),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum XhciTrbTag {
    Normal,
    Link,
    Empty,
}
#[derive(Copy, Clone)]
pub union XhciTrbUnion {
    pub Normal: XhciNormalTrb,
    pub Link: XhciLinkTrb,
    pub Empty: XhciEmptyTrb,
}
impl XhciTrbUnion {
    #[inline(always)]
    fn control_dword(&self) -> u32 {
        unsafe {
            let dwords: &[u32; 4] = core::mem::transmute(self);
            dwords[3]
        }
    }

    /// Returns the Cycle Bit (Bit 0 of the 4th Dword)
    pub fn cycle_bit(&self) -> bool {
        (self.control_dword() & 0x1) != 0
    }

    /// Returns the TRB Type (Bits 10-15 of the 4th Dword)
    pub fn trb_type(&self) -> XhciTrbType {
        let raw_type = ((self.control_dword() >> 10) & 0x3F) as u8;
        XhciTrbType::from_bytes(raw_type).unwrap_or_else(|_| panic!("trb type decoding not good"))
    }
}

const _: () = assert!(size_of::<XhciTrbUnion>() == 16);
impl XhciTrb {
    pub fn to_union(&self) -> Option<XhciTrbUnion> {
        match *self {
            XhciTrb::Normal(x) => Some(XhciTrbUnion { Normal: x }),
            XhciTrb::Link(x) => Some(XhciTrbUnion { Link: x }),
            XhciTrb::Empty(x) => Some(XhciTrbUnion { Empty: x }),
        }
    }
    pub fn to_tag(&self) -> XhciTrbTag {
        match self {
            XhciTrb::Normal(_) => XhciTrbTag::Normal,
            XhciTrb::Link(_) => XhciTrbTag::Link,
            XhciTrb::Empty(_) => XhciTrbTag::Empty,
        }
    }
    pub fn from_raw(union: XhciTrbUnion, tag: XhciTrbTag) -> Self {
        unsafe {
            match tag {
                XhciTrbTag::Normal => Self::Normal(union.Normal),
                XhciTrbTag::Link => Self::Link(union.Link),
                XhciTrbTag::Empty => Self::Empty(union.Empty),
            }
        }
    }
}
#[modular_bitfield::bitfield(bits = 128)]
#[derive(Default, Copy, Clone, Debug)]
#[repr(C)]
pub struct XhciNormalTrb {
    pub param: u64,
    pub transfer_len: B17,
    pub td_size: B5,
    pub interrupt_target: B10,
    pub cycle_bit: bool,
    pub evaluate_next_trb: bool,
    pub interrupt_on_short: bool,
    pub no_snoop: bool,
    pub chain_bit: bool,
    pub interrupt_on_completion: bool,
    pub immediate_data: bool,
    #[skip]
    __: B2,
    pub block_event_interrupt: bool,
    pub trb_type: XhciTrbType,
    #[skip]
    __: B16,
}
#[modular_bitfield::bitfield]
#[derive(Default, Copy, Clone, Debug)]
#[repr(C)]
pub struct XhciLinkTrb {
    pub ring_segment_pointer: u64,
    #[skip]
    __: B22,
    pub interrupt_target: B10,
    pub cycle_bit: bool,
    pub toggle_cycle: bool,
    #[skip]
    __: B2,
    pub chain_bit: bool,
    pub interrupt_on_completion: bool,
    #[skip]
    __: B4,
    pub trb_type: XhciTrbType,
    #[skip]
    __: B16,
}
#[modular_bitfield::bitfield]
#[derive(Default, Copy, Clone, Debug)]
#[repr(C)]
pub struct XhciEmptyTrb {
    #[skip]
    __: B96,
    pub cycle_bit: bool,
    #[skip]
    __: B7,
}
impl XhciEmptyTrb {
    pub fn new_cycle_bit(cycle_bit: bool) -> Self {
        Self::new().with_cycle_bit(cycle_bit)
    }
}
#[derive(Default, Copy, Clone, Debug, Specifier)]
#[bits = 6]
pub enum XhciTrbType {
    #[default]
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Isoch = 5,
    Link = 6,
    EventData = 7,
    NoOp = 8,
    EnableSlotCommand = 9,
    DisableSlotCommand = 10,
    AddressDeviceCommand = 11,
    ConfigureEndpointCommand = 12,
    EvaluateContextCommand = 13,
    ResetEndpointCommand = 14,
    StopEndpointCommand = 15,
    SetTRDequeuePointerCommand = 16,
    ResetDeviceCommand = 17,
    ForceEventCommand = 18,
    NegotiateBandwidthCommand = 19,
    SetLatencyToleranceValueCommand = 20,
    GetPortBandwidthCommand = 21,
    ForceHeaderCommand = 22,
    NoOpCommand = 23,
    GetExtendedPropertyCommand = 24,
    SetExtendedPropertyCommand = 25,
    TransferEvent = 32,
    CommandCompletionEvent = 33,
    PortStatusChangeEvent = 34,
    BandwidthRequestEvent = 35,
    DoorbellEvent = 36,
    HostControllerEvent = 37,
    DeviceNotificationEvent = 38,
    MFINDEXWrapEvent = 39,
}
