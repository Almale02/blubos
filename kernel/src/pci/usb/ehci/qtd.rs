use core::fmt::Display;

use bit_field::BitField as _;
use bitflags::bitflags;

#[repr(C, align(32))]
#[derive(Default)]
pub struct EhciQtd {
    pub next_qtd: u32,
    pub alt_next_qtd: u32,
    pub token: EhciQtdToken,
    pub buffer_pages: [u32; 5],
}
impl Display for EhciQtd {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "\tnext: {:#x}", self.next_qtd());
        // writeln!(f, "\tbuffer pages: {:#010x}", self.buffer_pages());

        write!(f, "\tbuffer pages: ");
        for (i, val) in self.buffer_pages().iter().enumerate() {
            if i > 0 {
                write!(f, ", ");
            }
            write!(f, "{:#010x}", val);
        }
        writeln!(f, "");

        writeln!(f, "\ttoken: {}", self.token)
    }
}
impl EhciQtd {
    pub fn new() -> Self {
        Self {
            next_qtd: 1,
            alt_next_qtd: 1,
            token: EhciQtdToken::new(),
            buffer_pages: [0; 5],
        }
    }
    pub fn set_next_qtd(&mut self, addr: u32) {
        self.next_qtd = (addr & !0x1F) | 0x0; // lower bits hold type/terminate
    }

    pub fn set_next_qtd_term(&mut self) {
        self.next_qtd = 1; // Terminate (T=1)
    }

    pub fn set_alt_next_qtd(&mut self, addr: u32) {
        self.alt_next_qtd = (addr & !0x1F) | 0x0;
    }

    pub fn set_alt_next_qtd_term(&mut self) {
        self.alt_next_qtd = 1;
    }

    pub fn next_qtd(&self) -> u32 {
        self.next_qtd
    }

    pub fn alt_next_qtd(&self) -> u32 {
        self.alt_next_qtd
    }
    pub fn set_buffer_page(&mut self, index: usize, addr: u32) {
        assert!(index < 5);
        self.buffer_pages[index] = addr;
    }
    pub fn buffer_page(&self, index: usize) -> u32 {
        assert!(index < 5);
        self.buffer_pages[index]
    }

    pub fn buffer_pages(&self) -> &[u32; 5] {
        &self.buffer_pages
    }

    pub fn buffer_pages_mut(&mut self) -> &mut [u32; 5] {
        &mut self.buffer_pages
    }
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct EhciQtdToken(u32);

impl Display for EhciQtdToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "\t\tdata toggle: {}", self.data_toggle());
        writeln!(f, "\t\ttotal bytes: {}", self.total_bytes());
        writeln!(f, "\t\tioc: {}", self.ioc());
        writeln!(f, "\t\tcurr page: {}", self.curr_page());
        writeln!(f, "\t\tcerr: {}", self.cerr());
        writeln!(f, "\t\tpid: {:?}", self.pid());
        writeln!(f, "\t\tstatus: {:?}", self.status())
    }
}
impl EhciQtdToken {
    pub fn new() -> Self {
        Self(0)
    }

    // [31] Data Toggle
    pub fn data_toggle(&self) -> bool {
        self.0.get_bit(31)
    }
    pub fn set_data_toggle(&mut self, val: bool) {
        self.0.set_bit(31, val);
    }

    // [30:16] Total Bytes to Transfer
    pub fn total_bytes(&self) -> u16 {
        self.0.get_bits(16..31) as u16
    }
    pub fn set_total_bytes(&mut self, val: u16) {
        self.0.set_bits(16..31, val as u32);
    }

    // [15] Interrupt On Complete
    pub fn ioc(&self) -> bool {
        self.0.get_bit(15)
    }
    pub fn set_ioc(&mut self, val: bool) {
        self.0.set_bit(15, val);
    }

    // [14:12] Current Page
    pub fn curr_page(&self) -> u8 {
        self.0.get_bits(12..15) as u8
    }
    pub fn set_curr_page(&mut self, val: u8) {
        self.0.set_bits(12..15, val as u32);
    }

    // [11:10] Error Counter (CERR)
    pub fn cerr(&self) -> u8 {
        self.0.get_bits(10..12) as u8
    }
    pub fn set_cerr(&mut self, val: u8) {
        self.0.set_bits(10..12, val as u32);
    }

    // [9:8] PID Code
    pub fn pid(&self) -> EhciQtdPid {
        let bits = self.0.get_bits(8..10);
        match bits {
            0x0 => EhciQtdPid::Out,
            0b01 => EhciQtdPid::In,
            0b10 => EhciQtdPid::Setup,
            _ => unreachable!(),
        }
    }
    pub fn set_pid(&mut self, val: EhciQtdPid) {
        self.0.set_bits(
            8..10,
            match val {
                EhciQtdPid::Out => 0b0,
                EhciQtdPid::In => 0b01,
                EhciQtdPid::Setup => 0b10,
            },
        );
    }
    // 00=OUT, 01=IN, 10=SETUP, 11=Reserved

    pub fn status(&self) -> EhciQtdStatus {
        EhciQtdStatus::from_bits_truncate(self.0.get_bits(0..=7) as u8)
    }
    pub fn set_status(&mut self, status: EhciQtdStatus) {
        self.0.set_bits(0..8, status.bits() as u32);
    }

    pub fn raw(&self) -> u32 {
        self.0
    }
    pub fn set_raw(&mut self, val: u32) {
        self.0 = val;
    }
}

bitflags! {
    /// Status bits [7:0] of qTD token
    #[derive(Copy, Clone, Debug)]
    pub struct EhciQtdStatus: u8 {
        const ACTIVE      = 1 << 7;
        const HALTED      = 1 << 6;
        const BUFFER_ERR  = 1 << 5;
        const BABBLE      = 1 << 4;
        const XACT_ERR    = 1 << 3;
        const MISSED_UF   = 1 << 2; // missed micro-frame
        const SPLITXSTATE = 1 << 1; // split transaction state
        const PING_STATE  = 1 << 0; // ping/ERR depending on speed
    }
}

#[derive(Debug)]
pub enum EhciQtdPid {
    Out,
    In,
    Setup,
}
bitflags! {
    #[derive(Copy, Clone, Debug, Default)]
    pub struct UsbRequestType: u8 {
        // Bit 7 → Direction
        const HOST_TO_DEVICE = 0 << 7; // OUT
        const DEVICE_TO_HOST = 1 << 7; // IN

        // Bits 6–5 → Type
        const STANDARD = 0b00 << 5;
        const CLASS    = 0b01 << 5;
        const VENDOR   = 0b10 << 5;
        const RESERVED = 0b11 << 5;

        // Bits 4–0 → Recipient
        const TO_DEVICE    = 0;
        const TO_INTERFACE = 1;
        const TO_ENDPOINT  = 2;
        const TO_OTHER     = 3;
    }
}
#[derive(Copy, Clone, Debug, Default)]
#[repr(u8)]
pub enum UsbRequestCode {
    #[default]
    GetStatus = 0x00,
    ClearFeature = 0x01,
    SetFeature = 0x03,
    SetAddress = 0x05,
    GetDescriptor = 0x06,
    SetDescriptor = 0x07,
    GetConfiguration = 0x08,
    SetConfiguration = 0x09,
    GetInterface = 0x0A,
    SetInterface = 0x0B,
    SynchFrame = 0x12,
}
#[repr(C, packed)]
#[derive(Copy, Clone, Debug, Default)]
pub struct UsbSetupPacket {
    pub request_type: UsbRequestType,
    pub request: UsbRequestCode,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}
