#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(const_trait_impl)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(ptr_metadata)]
#![allow(static_mut_refs)]

pub mod allocator;
pub mod byte_ext;
pub mod cpu_panic;
pub mod font_renderer;
pub mod framebuffer;
pub mod pci;
pub mod ps2;
pub mod serial;
pub mod time;

extern crate alloc;

use alloc::string::String;
use bytesize::ByteSize;
use core::{
    alloc::{GlobalAlloc, Layout},
    arch::{asm, naked_asm},
    sync::atomic::{AtomicUsize, Ordering},
};
pub use limine::framebuffer::Framebuffer as LimlineFramebuffer;
use limine::{BaseRevision, request::FramebufferRequest};
use spin::{Lazy, once::Once};
use x86_64::{
    VirtAddr,
    structures::{
        idt::{InterruptDescriptorTable, InterruptStackFrame},
        paging::Translate,
    },
};

use crate::{
    allocator::{
        bump_alloc::{BUMP_ALLOCATOR, BUMP_HEAP_SIZE, init_bump_alloc},
        paging::{PAGE_TABLE_MAPPER, init_mapper, map_mmio_region},
        tree_alloc::{TREE_ALLOCATOR, TreeAlloc},
    },
    byte_ext::ByteExt as _,
    cpu_panic::{DOUBLE_FAULT_IST_INDEX, PAGE_FAULT_IST_INDEX, init_tss},
    font_renderer::{TEXT_OUT_BUFFER, TextBufferWriter},
    framebuffer::Framebuffer,
    pci::{
        pci::{PCI_SCANNER, PciScanner, enable_mmio_and_bus_master},
        usb::xhci::{
            controller::{XHCI_CONTROLLER, XhciController},
            ring::{XhciCommandRing, XhciEventRing},
        },
    },
    ps2::{KEYPRSS_COUNT, keyboard_handler},
    serial::{inb, outb, serial_init, serial_write_str},
    time::{irq0_handler, pit_init},
};

static FB_REQ: FramebufferRequest = FramebufferRequest::new();
static BASE_REVISION: BaseRevision = BaseRevision::new();

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();

    for i in 32..48 {
        idt[i].set_handler_fn(catch_all_irq);
    }
    idt[0x20].set_handler_fn(irq0_handler);
    idt[0x21].set_handler_fn(keyboard_handler);
    idt.breakpoint.set_handler_fn(cpu_panic::breakpoint_handler);
    idt.general_protection_fault
        .set_handler_fn(cpu_panic::general_protection_fault_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(cpu_panic::double_fault_handler)
            .set_stack_index(DOUBLE_FAULT_IST_INDEX);
        idt.page_fault
            .set_handler_fn(cpu_panic::page_fault_handler)
            .set_stack_index(PAGE_FAULT_IST_INDEX);
    }
    idt.invalid_opcode
        .set_handler_fn(cpu_panic::invalid_opcode_handler);
    idt
});

static mut RENDER_BUFFER: Once<Option<Framebuffer>> = Once::new();

static TICKS: AtomicUsize = AtomicUsize::new(0);

#[repr(C, packed)]
struct DTBuf([u8; 10]);

pub extern "x86-interrupt" fn catch_all_irq(_stack_frame: InterruptStackFrame) {
    unsafe {
        outb(0xA0, 0x20);
        outb(0x20, 0x20);
    }
}

pub fn dump_idt_ptr() {
    let mut raw = DTBuf([0; 10]);
    unsafe {
        asm!(
            "sidt [{}]",
            in(reg) &mut raw,
            options(nostack, preserves_flags)
        );
    }
    let limit = u16::from_le_bytes([raw.0[0], raw.0[1]]);
    let base = u64::from_le_bytes([
        raw.0[2], raw.0[3], raw.0[4], raw.0[5], raw.0[6], raw.0[7], raw.0[8], raw.0[9],
    ]);
    crate::serial_println!("CPU.IDTR base={:#x} limit={:#x}", base, limit);
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text._start")]
pub extern "C" fn _start() -> ! {
    naked_asm!(
        "and rsp, -16",
        // "sub rsp, 8",         // Adjust so ABI holds at call entry
        "call {main}",        // jump into real Rust entrypoint
        main = sym kernel_stack_init,
    );
}

#[allow(unreachable_code)]
#[unsafe(no_mangle)]
pub extern "C" fn kernel_stack_init() -> ! {
    unsafe {
        asm!("cli");
    }
    enable_sse();
    init_tss();
    serial_init();

    pic_remap();

    IDT.load();
    init_bump_alloc();

    let stack_ptr = alloc_stack(256.kb()).unwrap_or_else(|| {
        serial_println!("failed to alloc new stack");
        panic!();
    });
    switch_stack(stack_ptr);
}

#[unsafe(no_mangle)]
fn kernel_main() {
    serial_println!(
        "inited bump alloc with {}",
        ByteSize(unsafe { BUMP_HEAP_SIZE } as u64)
    );

    TREE_ALLOCATOR.init();

    pit_init(1000);
    serial_write_str("kernel start\n");
    unsafe { asm!("sti") }

    if let Some(resp) = FB_REQ.get_response() {
        if let Some(fb) = resp.framebuffers().next() {
            unsafe { RENDER_BUFFER.call_once(|| Some(Framebuffer::new(fb))) };
        }
    }
    unsafe { TEXT_OUT_BUFFER.call_once(|| String::new()) };
    unsafe { KEYPRSS_COUNT.call_once(|| 0) };
    unsafe { PAGE_TABLE_MAPPER.call_once(|| init_mapper()) };

    unsafe { PCI_SCANNER.call_once(|| PciScanner::new()) };
    unsafe { PCI_SCANNER.get_mut().unwrap().scan() };

    for func in unsafe { PCI_SCANNER.get().unwrap().iter() } {
        if func.class.class == 0xc && func.class.subclass == 0x03 && func.class.prog_if == 0x30 {
            enable_mmio_and_bus_master(func.loc);
            if let Some(phys_addr) = func.bars[0] {
                unsafe {
                    let bar = map_mmio_region(phys_addr, 16.kb() as u64);
                    let mapper = PAGE_TABLE_MAPPER.get_mut().unwrap();
                    let physical = mapper.translate_addr(VirtAddr::new(bar as u64)).unwrap();

                    serial_println!(
                        "found xhci, and mapped at: {:#x}, physical: {:#x}",
                        bar as usize,
                        physical.as_u64()
                    );
                    XHCI_CONTROLLER.call_once(|| XhciController::new(bar));
                    let controller = XhciController::get_mut();
                    let caps = controller.capabilities.read();

                    serial_println!("capabilities: {:?}", caps);

                    controller.reset();
                    serial_println!("controller reset");

                    XhciCommandRing::init_ring();

                    controller.setup_op_regs();
                    serial_println!("op regs: {}", controller.op_registers.read());
                    controller.setup_runtime_regs();
                    XhciEventRing::init_ring();
                    serial_println!("op regs: {}", controller.op_registers.read());
                    serial_println!("runtime regs: {}", controller.runtime_registers.read());

                    match controller.start() {
                        Ok(_) => serial_println!("controller started!"),
                        Err(_) => serial_println!("controller failed to start!"),
                    }
                    serial_println!("op regs: {}", controller.op_registers.read());

                    for i in 1..=caps.struct_param_1.max_ports() as usize {
                        let port_reg = controller.get_port_reg(i).read();
                        if port_reg.port_control.current_connect_status() {
                            serial_println!("port {} is connected", i);
                            serial_println!("{:?}\n", port_reg);
                        }
                    }
                }
            }
        }
    }
    let mut prev_frametime = 0usize;
    loop {
        let start_tick = TICKS.load(Ordering::Relaxed);
        if let Some(framebuffer) = unsafe { RENDER_BUFFER.get_mut().unwrap() } {
            framebuffer.clear(framebuffer.rgb_color(128, 32, 64));
            textbuff_println!("i love banana");
            textbuff_println!("tick: {}", TICKS.load(Ordering::Relaxed));
            textbuff_println!("res: {}x{}", framebuffer.width(), framebuffer.height());
            textbuff_println!("pitch: {}", framebuffer.pitch_u32());
            textbuff_println!("frame ticktime: {}", prev_frametime);
            textbuff_println!("keypress count: {}", unsafe {
                *KEYPRSS_COUNT.get_unchecked()
            });
            TreeAlloc::write_init_areas(TextBufferWriter);
            framebuffer.draw_string(
                5,
                5,
                unsafe { TEXT_OUT_BUFFER.get().unwrap() },
                framebuffer.rgb_color(255, 255, 255),
                1,
            );
            framebuffer.flip();
            unsafe {
                TEXT_OUT_BUFFER.get_mut().unwrap().clear();
            }
        }
        let end_tick = TICKS.load(Ordering::Relaxed);
        prev_frametime = end_tick - start_tick;
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

// pub fn pic_remap() {
//     // Initialization Command Words
//     const ICW1_INIT: u8 = 0x10;
//     const ICW1_ICW4: u8 = 0x01;
//     const ICW4_8086: u8 = 0x01;

//     unsafe {
//         // Save masks
//         let a1 = inb(0xA1);
//         let a2 = inb(0x21);

//         // Starts the initialization sequence in cascade mode
//         outb(0x20, ICW1_INIT | ICW1_ICW4);
//         outb(0xA0, ICW1_INIT | ICW1_ICW4);

//         // ICW2: Master PIC vector offset
//         outb(0x21, 0x20);
//         // ICW2: Slave PIC vector offset
//         outb(0xA1, 0x28);

//         // ICW3: tell Master PIC that there is a slave PIC at IRQ2 (0000 0100)
//         outb(0x21, 4);
//         // ICW3: tell Slave PIC its cascade identity (0000 0010)
//         outb(0xA1, 2);

//         // ICW4: 8086 mode
//         outb(0x21, ICW4_8086);
//         outb(0xA1, ICW4_8086);

//         // Restore saved masks
//         outb(0x21, a2 & !0x03); // unmask IRQ0 by clearing bit 0
//         outb(0xA1, a1);
//     }
// }
pub fn pic_remap() {
    // Initialization Command Words
    const ICW1_INIT: u8 = 0x10;
    const ICW1_ICW4: u8 = 0x01;
    const ICW4_8086: u8 = 0x01;

    // Port addresses
    const MASTER_CMD: u16 = 0x20;
    const MASTER_DATA: u16 = 0x21;
    const SLAVE_CMD: u16 = 0xA0;
    const SLAVE_DATA: u16 = 0xA1;

    unsafe {
        // 1. Start the initialization sequence (in cascade mode)
        // This tells both PICs to expect 3 more initialization bytes
        outb(MASTER_CMD, ICW1_INIT | ICW1_ICW4);
        outb(SLAVE_CMD, ICW1_INIT | ICW1_ICW4);

        // 2. ICW2: Set the Vector Offsets
        // We map Master IRQs to 32-39 and Slave IRQs to 40-47
        // (This moves them out of the way of CPU Exceptions like Page Faults)
        outb(MASTER_DATA, 0x20); // 32 in decimal
        outb(SLAVE_DATA, 0x28); // 40 in decimal

        // 3. ICW3: Tell the PICs how they are wired together
        // Tell Master PIC that there is a slave PIC at IRQ2 (0000 0100)
        outb(MASTER_DATA, 4);
        // Tell Slave PIC its cascade identity (0000 0010)
        outb(SLAVE_DATA, 2);

        // 4. ICW4: Set 8086 mode
        outb(MASTER_DATA, ICW4_8086);
        outb(SLAVE_DATA, ICW4_8086);

        // --- MASKING CONFIGURATION ---

        // Master PIC:
        // We must unmask:
        // Bit 0: Timer (IRQ 0)
        // Bit 1: Keyboard (IRQ 1)
        // Bit 2: Slave Cascade (IRQ 2) - REQUIRED to hear anything from Slave!
        // 0xF8 = 1111 1000 in binary
        outb(MASTER_DATA, 0xF8);

        // Slave PIC:
        // For debugging on real hardware, we unmask EVERYTHING on the slave.
        // This ensures that if the hardware fires a random interrupt (like the Mouse),
        // our 'catch_all_irq' can send the EOI and prevent the system from freezing.
        outb(SLAVE_DATA, 0x00);
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

pub fn alloc_stack(size: usize) -> Option<*mut u8> {
    let layout = Layout::from_size_align(size, 16).unwrap();
    unsafe {
        let ptr = BUMP_ALLOCATOR.alloc(layout);
        if ptr.is_null() {
            None
        } else {
            let top = ptr.add(size);
            Some((top as usize & !0xf) as *mut u8) // make sure it's 16‑byte aligned
        }
    }
}

pub fn switch_stack(new_stack_top: *mut u8) -> ! {
    unsafe {
        core::arch::asm!(
            "mov rsp, {0}",
            "jmp {1}",
            in(reg) (new_stack_top as usize & !0xf) - 8, // ensure 16n-8
            sym kernel_main,
            options(noreturn)
        );
    }
}

/*






















*/
