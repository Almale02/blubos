use core::{arch::asm, fmt::Write};

// COM1 base port
const COM1: u16 = 0x3F8;

pub unsafe fn outb(port: u16, val: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") val) };
}

pub unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    unsafe { asm!("in al, dx", out("al") val, in("dx") port) };
    val
}

pub fn serial_init() {
    unsafe {
        outb(COM1 + 1, 0x00); // Disable interrupts
        outb(COM1 + 3, 0x80); // Enable DLAB
        outb(COM1 + 0, 0x03); // Set divisor to 3 (38400 baud)
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03); // 8 bits, no parity, one stop bit
        outb(COM1 + 2, 0xC7); // Enable FIFO, clear, 14-byte threshold
        outb(COM1 + 4, 0x0B); // IRQs enabled, RTS/DSR set
    }
}

pub fn serial_write_byte(c: u8) {
    unsafe {
        while (inb(COM1 + 5) & 0x20) == 0 {} // Wait until empty
        outb(COM1, c);
    }
}

pub fn serial_write_str(s: &str) {
    for c in s.bytes() {
        serial_write_byte(c);
    }
}

pub struct SerialWriter;
impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        serial_write_str(s);
        Ok(())
    }
}
#[macro_export]
macro_rules! serial_print {
    ($($arg: tt)*) => {
        {
            use core::fmt::Write as _;
            let _ = write!(crate::serial::SerialWriter, $($arg)*);
        }
    };
}
#[macro_export]
macro_rules! serial_println {
    ($($arg: tt)*) => {
        {
            use core::fmt::Write as _;
            let _ = write!(crate::serial::SerialWriter, $($arg)*);
            let _ = write!(crate::serial::SerialWriter, "\n");
        }
    };
}
