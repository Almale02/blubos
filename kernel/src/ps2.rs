use spin::once::Once;
use x86_64::structures::idt::InterruptStackFrame;

use crate::{
    serial::{inb, outb},
    serial_println,
};

pub static mut KEYPRSS_COUNT: Once<u128> = Once::new();

pub extern "x86-interrupt" fn keyboard_handler(_stack_frame: InterruptStackFrame) {
    let status = unsafe { inb(0x64) };

    if (status & 0x01) != 0 {
        let _ = unsafe { inb(0x60) };
        unsafe { *KEYPRSS_COUNT.get_mut().unwrap() += 1 };
    }

    unsafe {
        outb(0xA0, 0x20);
        outb(0x20, 0x20);
    }
}
