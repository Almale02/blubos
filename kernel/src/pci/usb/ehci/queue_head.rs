use core::fmt::{self, Display};

use bit_field::BitField as _;

use crate::pci::usb::ehci::qtd::{EhciQtd, EhciQtdToken};

#[repr(C, align(32))]
pub struct EhciQueueHead {
    pub horiz_link: u32,       // Next QH in async/periodic list
    pub ep_char: EhciQhEpChar, // Endpoint characteristics
    pub ep_caps: EhciQhEpCaps, // Endpoint capabilities
    pub curr_qtd: u32,         // Pointer to current qTD
    pub next_qtd: u32,
    pub alt_next_qtd: u32,
    pub token: EhciQtdToken,
    pub buffer_pages: [u32; 5],
}
impl Display for EhciQueueHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "horz link: {:#x}", self.horiz_link);
        writeln!(f, "ep chars {}", self.ep_char);
        writeln!(f, "ep caps {}", self.ep_caps);
        writeln!(f, "overlay \n{}", self.get_overlay())
    }
}
impl EhciQueueHead {
    pub fn overlay_set_next_qtd(&mut self, addr: u32) {
        self.next_qtd = (addr & !0x1F) | 0x0; // lower bits hold type/terminate
    }

    pub fn overlay_set_next_qtd_term(&mut self) {
        self.next_qtd = 1; // Terminate (T=1)
    }

    pub fn overlay_set_alt_next_qtd(&mut self, addr: u32) {
        self.alt_next_qtd = (addr & !0x1F) | 0x0;
    }

    pub fn overlay_set_alt_next_qtd_term(&mut self) {
        self.alt_next_qtd = 1;
    }

    pub fn overlay_next_qtd(&self) -> u32 {
        self.next_qtd
    }

    pub fn overlay_alt_next_qtd(&self) -> u32 {
        self.alt_next_qtd
    }
    pub fn overlay_set_buffer_page(&mut self, index: usize, addr: u32) {
        assert!(index < 5);
        self.buffer_pages[index] = addr;
    }
    pub fn overlay_buffer_page(&self, index: usize) -> u32 {
        assert!(index < 5);
        self.buffer_pages[index]
    }

    pub fn overlay_buffer_pages(&self) -> &[u32; 5] {
        &self.buffer_pages
    }

    pub fn overlay_buffer_pages_mut(&mut self) -> &mut [u32; 5] {
        &mut self.buffer_pages
    }
    pub fn get_overlay(&self) -> EhciQtd {
        EhciQtd {
            next_qtd: self.next_qtd,
            alt_next_qtd: self.alt_next_qtd,
            token: self.token,
            buffer_pages: self.buffer_pages,
        }
    }
}

#[repr(C)]
pub struct EhciQhEpChar(u32);

impl EhciQhEpChar {
    pub fn new() -> Self {
        Self(0)
    }

    // 31:28 - Nak Count Reload (RL)
    pub fn rl(&self) -> u8 {
        self.0.get_bits(28..32) as u8
    }
    pub fn set_rl(&mut self, val: u8) {
        self.0.set_bits(28..32, val as u32);
    }

    // 27 - Control Endpoint Flag (C)
    pub fn is_control(&self) -> bool {
        self.0.get_bit(27)
    }
    pub fn set_control(&mut self, val: bool) {
        self.0.set_bit(27, val);
    }

    // 26:16 - Max Packet Size (11 bits)
    pub fn max_packet_size(&self) -> u16 {
        self.0.get_bits(16..27) as u16
    }
    pub fn set_max_packet_size(&mut self, val: u16) {
        self.0.set_bits(16..27, val as u32);
    }

    // 15 - Head of Reclamation List Flag (H)
    pub fn is_head(&self) -> bool {
        self.0.get_bit(15)
    }
    pub fn set_head(&mut self, val: bool) {
        self.0.set_bit(15, val);
    }

    // 14 - Data Toggle Control (DTC)
    pub fn is_dtc(&self) -> bool {
        self.0.get_bit(14)
    }
    pub fn set_dtc(&mut self, val: bool) {
        self.0.set_bit(14, val);
    }

    // 13:12 - Endpoint Speed
    pub fn eps(&self) -> u8 {
        self.0.get_bits(12..14) as u8
    }
    pub fn set_eps(&mut self, val: u8) {
        self.0.set_bits(12..14, val as u32);
    }
    // 00=FS, 01=LS, 10=HS

    // 11:8 - Endpoint Number
    pub fn endpt(&self) -> u8 {
        self.0.get_bits(8..12) as u8
    }
    pub fn set_endpt(&mut self, val: u8) {
        self.0.set_bits(8..12, val as u32);
    }

    // 7 - Inactivate on Next Transaction (I)
    pub fn inactivate(&self) -> bool {
        self.0.get_bit(7)
    }
    pub fn set_inactivate(&mut self, val: bool) {
        self.0.set_bit(7, val);
    }

    // 6:0 - Device Address
    pub fn dev_addr(&self) -> u8 {
        self.0.get_bits(0..7) as u8
    }
    pub fn set_dev_addr(&mut self, val: u8) {
        self.0.set_bits(0..7, val as u32);
    }

    pub fn raw(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for EhciQhEpChar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EhciQhEpChar {{ raw: 0x{:08X}, rl: {}, control: {}, max_packet_size: {}, head: {}, dtc: {}, eps: {}, endpt: {}, inactivate: {}, dev_addr: {} }}",
            self.raw(),
            self.rl(),
            self.is_control(),
            self.max_packet_size(),
            self.is_head(),
            self.is_dtc(),
            self.eps(),
            self.endpt(),
            self.inactivate(),
            self.dev_addr(),
        )
    }
}
/// DW2: Endpoint Capabilities
#[repr(C)]
pub struct EhciQhEpCaps(u32);

impl EhciQhEpCaps {
    pub fn new() -> Self {
        Self(0)
    }

    // 31:30 - High Bandwidth Multiplier
    pub fn mult(&self) -> u8 {
        self.0.get_bits(30..32) as u8
    }
    pub fn set_mult(&mut self, val: u8) {
        self.0.set_bits(30..32, val as u32);
    }

    // 29:23 - Port Number
    pub fn port_num(&self) -> u8 {
        self.0.get_bits(23..30) as u8
    }
    pub fn set_port_num(&mut self, val: u8) {
        self.0.set_bits(23..30, val as u32);
    }

    // 22:16 - Hub Addr
    pub fn hub_addr(&self) -> u8 {
        self.0.get_bits(16..23) as u8
    }
    pub fn set_hub_addr(&mut self, val: u8) {
        self.0.set_bits(16..23, val as u32);
    }

    // 15:8 - Split Completion Mask
    pub fn c_mask(&self) -> u8 {
        self.0.get_bits(8..16) as u8
    }
    pub fn set_c_mask(&mut self, val: u8) {
        self.0.set_bits(8..16, val as u32);
    }

    // 7:0 - Interrupt Schedule Mask
    pub fn s_mask(&self) -> u8 {
        self.0.get_bits(0..8) as u8
    }
    pub fn set_s_mask(&mut self, val: u8) {
        self.0.set_bits(0..8, val as u32);
    }

    pub fn raw(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for EhciQhEpCaps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EhciQhEpCaps {{ raw: 0x{:08X}, mult: {}, port_num: {}, hub_addr: {}, c_mask: 0x{:02X}, s_mask: 0x{:02X} }}",
            self.raw(),
            self.mult(),
            self.port_num(),
            self.hub_addr(),
            self.c_mask(),
            self.s_mask(),
        )
    }
}
