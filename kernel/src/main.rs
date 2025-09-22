#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(new_zeroed_alloc)]

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
    allocator::init_heap,
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
        "sub rsp, 8",         // Adjust so ABI holds at call entry
        "call {main}",        // jump into real Rust entrypoint
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
    init_heap();

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
