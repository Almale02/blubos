use core::arch::asm;

use x86_64::structures::idt::InterruptStackFrame;

use crate::serial::serial_write_str;

pub extern "x86-interrupt" fn breakpoint_handler(_stack: InterruptStackFrame) {
    serial_write_str("breakpoint invoked, they are not support");

    loop {
        unsafe { asm!("cli; hlt") }
    }
}
pub extern "x86-interrupt" fn double_fault_handler(
    _stack: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    serial_write_str("double fault with code: {}");

    loop {
        unsafe { asm!("cli; hlt") }
    }
}

pub extern "x86-interrupt" fn general_protection_fault_handler(
    _stack: InterruptStackFrame,
    _error_code: u64,
) {
    serial_write_str("general protection fault");
    loop {
        unsafe { asm!("cli; hlt") }
    }
}

pub extern "x86-interrupt" fn invalid_opcode_handler(_stack: InterruptStackFrame) {
    serial_write_str("invalid opcode");
    loop {
        unsafe { asm!("cli; hlt") }
    }
}
