use core::sync::atomic::Ordering;

use x86_64::structures::idt::InterruptStackFrame;

use crate::{TICKS, serial::outb};

const PIT_CH0: u16 = 0x40;
const PIT_CMD: u16 = 0x43;
const PIT_FREQUENCY: u32 = 1_193_182;

pub fn pit_init(hz: u32) {
    let divisor: u16 = (PIT_FREQUENCY / hz) as u16;

    unsafe {
        // Binary mode, mode=3 (square wave), low+high byte, channel 0
        crate::serial::outb(PIT_CMD, 0x36);

        // Send divisor low byte then high byte
        crate::serial::outb(PIT_CH0, (divisor & 0xFF) as u8);
        crate::serial::outb(PIT_CH0, (divisor >> 8) as u8);
    }
}
pub extern "x86-interrupt" fn irq0_handler(_stack_frame: InterruptStackFrame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    unsafe {
        outb(0x20, 0x20);
    }
}
