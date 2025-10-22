// ===== FILE: src/allocator.rs
                                       =====



// ===== FILE: src/cpu_panic.rs
                                       =====

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


// ===== FILE: src/font_renderer.rs
                                       =====

use crate::framebuffer::Framebuffer;

pub static FONT8X16: [[u8; 16]; 256] = include!("../font/vga8x16_font.in");

impl Framebuffer<'_> {
    pub fn draw_char(&self, x: usize, y: usize, ch: u8, fg: u32, bg: Option<u32>, scale: usize) {
        let glyph = &FONT8X16[ch as usize];

        for (row, row_bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                let bit = (row_bits >> (7 - col)) & 1;
                let color = if bit == 1 {
                    fg
                } else if let Some(bgcol) = bg {
                    bgcol
                } else {
                    continue; // transparent background
                };

                // handle scaling
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = x + col * scale + dx;
                        let py = y + row * scale + dy;

                        if px < self.width() && py < self.height() {
                            self.set(py, px, color);
                        }
                    }
                }
            }
        }
    }
    pub fn draw_string(
        &self,
        mut x: usize,
        y: usize,
        text: &str,
        fg: u32,
        bg: Option<u32>,
        scale: usize,
    ) {
        for ch in text.bytes() {
            self.draw_char(x, y, ch, fg, bg, scale);
            x += 8 * scale;
        }
    }
}


// ===== FILE: src/framebuffer.rs
                                       =====

use limine::framebuffer::Framebuffer as LimlineFramebuffer;

pub struct Framebuffer<'a> {
    inner: LimlineFramebuffer<'a>,
}
impl<'a> Framebuffer<'a> {
    pub fn new(fb: LimlineFramebuffer<'a>) -> Framebuffer<'a> {
        if fb.bpp() != 32 {
            panic!("only 4 byte pixel data supported");
        }
        Framebuffer { inner: fb }
    }
    pub fn width(&self) -> usize {
        self.inner.width() as usize
    }
    pub fn height(&self) -> usize {
        self.inner.height() as usize
    }
    pub fn pitch_u32(&self) -> usize {
        self.inner.pitch() as usize / size_of::<u32>()
    }
    pub fn rgb_color(&self, r: u8, g: u8, b: u8) -> u32 {
        let rm = self.inner.red_mask_shift();
        let gm = self.inner.green_mask_shift();
        let bm = self.inner.blue_mask_shift();
        ((r as u32) << rm) | ((g as u32) << gm) | ((b as u32) << bm)
    }
    pub fn get_element_ptr(&self, y: usize, x: usize) -> *mut u32 {
        debug_assert!(y < self.height());
        debug_assert!(x < self.width());
        let offset = y * self.pitch_u32() + x;
        unsafe { (self.inner.addr() as *mut u32).add(offset) }
    }
    pub fn get(&self, y: usize, x: usize) -> u32 {
        unsafe { *self.get_element_ptr(y, x) }
    }
    pub fn set(&self, y: usize, x: usize, pixel: u32) {
        unsafe { *self.get_element_ptr(y, x) = pixel }
    }
    // pub fn clear(&self, color: u32) {
    //     let width = self.width();
    //     let height = self.height();

    //     for y in 0..height {
    //         let row_ptr = self.get_element_ptr(y, 0);
    //         unsafe {
    //             core::arch::asm!(
    //                 "cld",
    //                 "rep stosd",
    //                 in("rcx") width as u64,
    //                 in("rdi") row_ptr as u64,
    //                 in("eax") color,
    //                 options(nostack)
    //             );
    //         }
    //     }
    // }

    pub fn clear(&self, color: u32) {
        let total = self.pitch_u32() * self.height(); // stride × height

        unsafe {
            core::arch::asm!(
                "rep stosd",
                inout("rcx") total as u64 => _,       // pitch, not width
                inout("rdi") self.get_element_ptr(0, 0) as u64=> _,
                in("eax") color,
                options(nostack, preserves_flags)
            );
        }
    }
}
/*





















*/


// ===== FILE: src/main.rs
                                       =====

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(new_zeroed_alloc)]
#![allow(static_mut_refs)]

pub mod allocator;
pub mod cpu_panic;
pub mod font_renderer;
pub mod framebuffer;
pub mod serial;
pub mod time;

extern crate alloc;

use alloc::boxed::Box;
use core::{
    arch::{asm, naked_asm},
    sync::atomic::AtomicUsize,
};
pub use limine::framebuffer::Framebuffer as LimlineFramebuffer;
use limine::{BaseRevision, request::FramebufferRequest};
use spin::{Lazy, once::Once};
use x86_64::structures::idt::InterruptDescriptorTable;

use crate::{
    allocator::bump_alloc::init_bump_alloc,
    framebuffer::Framebuffer,
    serial::{inb, outb, serial_init, serial_write_str},
    time::{irq0_handler, pit_init},
};

static FB_REQ: FramebufferRequest = FramebufferRequest::new();
static BASE_REVISION: BaseRevision = BaseRevision::new();

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    idt[0x20].set_handler_fn(irq0_handler);
    idt.breakpoint.set_handler_fn(cpu_panic::breakpoint_handler);
    idt.general_protection_fault
        .set_handler_fn(cpu_panic::general_protection_fault_handler);
    idt.double_fault
        .set_handler_fn(cpu_panic::double_fault_handler);
    idt.invalid_opcode
        .set_handler_fn(cpu_panic::invalid_opcode_handler);
    idt
});

static RENDER_BUFFER: Once<Option<Framebuffer>> = Once::new();

static TICKS: AtomicUsize = AtomicUsize::new(0);

#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text._start")]
pub extern "C" fn _start() -> ! {
    naked_asm!(
        "and rsp, -16",
        "sub rsp, 8",
        "call {main}",
        main = sym kernel_main,
    );
}

#[allow(unreachable_code)]
#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    unsafe {
        asm!("cli");
    }
    enable_sse();
    serial_init();

    pic_remap();
    IDT.load();
    init_bump_alloc();

    pit_init(1000);
    serial_write_str("kernel start\n");

    unsafe { asm!("sti") }

    let mut buffer = unsafe { Box::<[u32]>::new_zeroed_slice(30).assume_init() };

    buffer
        .iter_mut()
        .enumerate()
        .for_each(|(i, x)| *x = (i as u32).pow(2));

    serial_println!("value at 12 = {}", buffer[12]);

    if let Some(resp) = FB_REQ.get_response() {
        if let Some(fb) = resp.framebuffers().next() {
            RENDER_BUFFER.call_once(|| Some(Framebuffer::new(fb)));
        }
    }
    loop {
        // serial_println!("before tick: {}", TICKS.load(Ordering::Relaxed));
        if let Some(framebuffer) = RENDER_BUFFER.get().unwrap() {
            framebuffer.clear(framebuffer.rgb_color(0, 0, 0));
            framebuffer.draw_string(
                50,
                50,
                "test string",
                framebuffer.rgb_color(255, 255, 255),
                None,
                2,
            );
        }
        // serial_println!("after tick: {}", TICKS.load(Ordering::Relaxed));
    }
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    if let Some(loc) = info.location() {
        serial_println!("panic at {}:{}:{}", loc.file(), loc.line(), loc.column());
    } else {
        serial_println!("paniced without location info");
    }
    if let Some(info) = info.message().as_str() {
        serial_print!("{}", info);
    }
    unsafe {
        asm!("cli");
    }
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

fn fill_screen_solid(fb: &LimlineFramebuffer, color: u32) {
    let addr = fb.addr() as *mut u32;
    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let pitch_bytes = fb.pitch() as usize;
    let bpp = fb.bpp() as usize;

    serial_println!("bpp is: {}", bpp);
    serial_println!("pitch bytes: {}", pitch_bytes);

    if bpp != 32 {
        return;
    }

    let pitch_u32 = pitch_bytes / core::mem::size_of::<u32>();
    serial_println!("pitch_u32: {}", pitch_u32);
    serial_println!("{}x{}", width, height);
    unsafe {
        for y in 0..height {
            let row_ptr = addr.add(y * pitch_u32);
            for x in 0..width {
                row_ptr.add(x).write_volatile(color);
            }
        }
    }
}

fn pack_color(fb: &LimlineFramebuffer, r: u8, g: u8, b: u8) -> u32 {
    let rm = fb.red_mask_shift();
    let gm = fb.green_mask_shift();
    let bm = fb.blue_mask_shift();
    ((r as u32) << rm) | ((g as u32) << gm) | ((b as u32) << bm)
}

#[inline(always)]
fn halt() {
    unsafe {
        asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

pub fn qemu_exit(code: u32) -> ! {
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") 0xF4u16,   // I/O port
            in("eax") code,     // exit code
            options(noreturn)
        );
    }
}

pub fn pic_remap() {
    // Initialization Command Words
    const ICW1_INIT: u8 = 0x10;
    const ICW1_ICW4: u8 = 0x01;
    const ICW4_8086: u8 = 0x01;

    unsafe {
        // Save masks
        let a1 = inb(0xA1);
        let a2 = inb(0x21);

        // Starts the initialization sequence in cascade mode
        outb(0x20, ICW1_INIT | ICW1_ICW4);
        outb(0xA0, ICW1_INIT | ICW1_ICW4);

        // ICW2: Master PIC vector offset
        outb(0x21, 0x20);
        // ICW2: Slave PIC vector offset
        outb(0xA1, 0x28);

        // ICW3: tell Master PIC that there is a slave PIC at IRQ2 (0000 0100)
        outb(0x21, 4);
        // ICW3: tell Slave PIC its cascade identity (0000 0010)
        outb(0xA1, 2);

        // ICW4: 8086 mode
        outb(0x21, ICW4_8086);
        outb(0xA1, ICW4_8086);

        // Restore saved masks
        outb(0x21, a2 & !0x01); // unmask IRQ0 by clearing bit 0
        outb(0xA1, a1);
    }
}

pub fn enable_sse() {
    unsafe {
        let mut cr0: u64;
        let mut cr4: u64;

        core::arch::asm!("mov {}, cr0", out(reg) cr0);
        core::arch::asm!("mov {}, cr4", out(reg) cr4);

        // CR0.MP = 1, CR0.EM = 0
        cr0 |= 1 << 1;
        cr0 &= !(1 << 2);

        // CR4.OSFXSR (bit 9), CR4.OSXMMEXCPT (bit 10)
        cr4 |= (1 << 9) | (1 << 10);

        core::arch::asm!("mov cr0, {}", in(reg) cr0);
        core::arch::asm!("mov cr4, {}", in(reg) cr4);
    }
}


// ===== FILE: src/serial.rs
                                       =====

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


// ===== FILE: src/time.rs
                                       =====

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


