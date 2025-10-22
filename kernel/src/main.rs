#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(new_zeroed_alloc)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![allow(static_mut_refs)]

pub mod allocator;
pub mod cpu_panic;
pub mod font_renderer;
pub mod framebuffer;
pub mod pci;
pub mod serial;
pub mod time;

extern crate alloc;

use alloc::{boxed::Box, string::String};
use bytesize::ByteSize;
use core::{
    alloc::{GlobalAlloc, Layout},
    arch::{asm, naked_asm},
    hint::spin_loop,
    sync::atomic::{AtomicUsize, Ordering},
};
pub use limine::framebuffer::Framebuffer as LimlineFramebuffer;
use limine::{BaseRevision, request::FramebufferRequest};
use memoffset::offset_of;
use spin::{Lazy, once::Once};
use x86_64::structures::idt::InterruptDescriptorTable;

use crate::{
    allocator::{
        allocator::{AlignedBoxAlloc, GetPhysicalAddr},
        bump_alloc::{BUMP_ALLOCATOR, BUMP_HEAP_SIZE, init_bump_alloc},
        paging::{PAGE_TABLE_MAPPER, init_mapper, map_mmio_region},
        tree_alloc::{TREE_ALLOCATOR, TreeAlloc, TreeAllocLow},
    },
    cpu_panic::{DOUBLE_FAULT_IST_INDEX, PAGE_FAULT_IST_INDEX, init_tss},
    font_renderer::{TEXT_OUT_BUFFER, TextBufferWriter},
    framebuffer::Framebuffer,
    pci::{
        pci::{PCI_SCANNER, PciScanner, enable_mmio_and_bus_master},
        usb::ehci::{
            controller::{DeviceEndpointRef, EHCI_CONTROLLER, EhciController, UsbDeviceDescriptor},
            qtd::{EhciQtd, EhciQtdStatus, UsbSetupPacket},
            queue_head::EhciQueueHead,
            registers::*,
        },
    },
    serial::{SerialWriter, inb, outb, serial_init, serial_write_str},
    time::{irq0_handler, pit_init, wait_ms},
};

static FB_REQ: FramebufferRequest = FramebufferRequest::new();
static BASE_REVISION: BaseRevision = BaseRevision::new();

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    idt[0x20].set_handler_fn(irq0_handler);
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

    let stack_ptr = alloc_stack(1024 * 256).unwrap_or_else(|| {
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

    serial_println!("size is: {}", size_of::<EhciQtd>());

    if let Some(resp) = FB_REQ.get_response() {
        if let Some(fb) = resp.framebuffers().next() {
            unsafe { RENDER_BUFFER.call_once(|| Some(Framebuffer::new(fb))) };
        }
    }
    unsafe { TEXT_OUT_BUFFER.call_once(|| String::new()) };
    unsafe { PAGE_TABLE_MAPPER.call_once(|| init_mapper()) };

    unsafe { PCI_SCANNER.call_once(|| PciScanner::new()) };
    unsafe { PCI_SCANNER.get_mut().unwrap().scan() };

    for func in unsafe { PCI_SCANNER.get().unwrap().iter() } {
        if func.class.class == 0x0c && func.class.subclass == 0x03 && func.class.prog_if == 0x20 {
            enable_mmio_and_bus_master(func.loc);
            if let Some(phys_addr) = func.bars[0] {
                unsafe {
                    EHCI_CONTROLLER
                        .call_once(|| EhciController::new(map_mmio_region(phys_addr, 0x1000)))
                };

                unsafe {
                    let ehci = EHCI_CONTROLLER.get_mut().unwrap();
                    let caplen = ehci.get_caplen();
                    let n_ports = ehci.get_port_count() as usize;

                    serial_println!("EHCI capability length = {:#x}", caplen);
                    serial_println!("EHCI reports {} ports", n_ports);

                    // Reset controller
                    ehci.reset_controller();
                    serial_println!("EHCI reset completed");

                    // Claim ownership (CONFIGFLAG = 1)
                    ehci.set_config_flag();
                    serial_println!("EHCI ownership flag set");

                    ehci.set_asynclistaddr_ptr(ehci.anchor_qh.to_physical_addr() | 2);

                    // Start controller (Run bit in USBCMD)
                    ehci.add_cmd_flags(EhciCmd::RUN);

                    while ehci.get_status_flags().contains(EhciStatus::HALTED) {
                        spin_loop();
                    }

                    ehci.new_queue_head_for_ep(DeviceEndpointRef {
                        device_addr: 0,
                        endpoint: 0,
                    });
                    ehci.set_device_control_queue_head(DeviceEndpointRef {
                        device_addr: 0,
                        endpoint: 0,
                    });

                    ehci.add_cmd_flags(EhciCmd::ASYNC_SCHEDULE_ENABLE);
                    let status = ehci.get_status_flags();
                    if status.contains(EhciStatus::PORT_CHANGE) {
                        ehci.add_status_flags(EhciStatus::PORT_CHANGE);
                    }

                    while !ehci.get_status_flags().contains(EhciStatus::ASYNC_STATUS) {
                        spin_loop();
                    }

                    // Handle ports
                    for port in 0..n_ports.min(1) {
                        let port_flags = ehci.get_port_control_flags(port);

                        let connected = port_flags.contains(EhciPortControl::CURR_CONNECT_STATUS);
                        let enabled = port_flags.contains(EhciPortControl::ENABLED);

                        serial_println!(
                            "Port {} before: {:?}, connected={}, enabled={}",
                            port + 1,
                            port_flags,
                            connected,
                            enabled
                        );

                        if connected && !enabled {
                            serial_println!("Port {}: resetting...", port + 1);
                            ehci.reset_port(port);
                            serial_println!("Port {} enabled", port + 1);
                        }

                        let final_flags = ehci.get_port_control_flags(port);

                        serial_println!(
                            "Port {} after: {:?}, connected={}, enabled={}",
                            port + 1,
                            final_flags,
                            final_flags.contains(EhciPortControl::CURR_CONNECT_STATUS),
                            final_flags.contains(EhciPortControl::ENABLED),
                        );
                        if final_flags.contains(EhciPortControl::CURR_CONNECT_STATUS) {
                            if !final_flags.contains(EhciPortControl::ENABLED) {
                                panic!("failed to enable connected port");
                            }
                        } else {
                            continue;
                        }
                        if final_flags.contains(EhciPortControl::PORT_OWNER) {
                            serial_println!("found device at port {}, but it was not ehci", port);
                            continue;
                        }
                        serial_println!(
                            "port speed at: {} is: {:?}",
                            port,
                            ehci.get_port_speed(port)
                        );

                        let init_endpoint = DeviceEndpointRef::new(0, 0);
                        let init_qh = ehci.qh_map.get_mut(&init_endpoint).unwrap();
                        let init_qtds = ehci.qtd_map.get_mut(&init_endpoint).unwrap();

                        let mut setup_buffer =
                            Box::new_in_aligned(UsbSetupPacket::default(), TreeAllocLow, 0x1000);
                        let mut data_buffer = Box::new_in_aligned(
                            UsbDeviceDescriptor::default(),
                            TreeAllocLow,
                            0x1000,
                        );
                        assert_eq!(setup_buffer.to_physical_addr() & 0xFFF, 0);
                        assert_eq!(data_buffer.to_physical_addr() & 0xFFF, 0);
                        init_qtds.build_get_descriptor_req(
                            setup_buffer.as_mut(),
                            data_buffer.as_mut(),
                            8,
                        );
                        init_qtds.link_qtds();
                        init_qtds.activate();
                        EHCI_CONTROLLER
                            .get_mut()
                            .unwrap()
                            .activate_overlay(init_endpoint);

                        while init_qh.token.status().contains(EhciQtdStatus::ACTIVE) {
                            spin_loop();
                        }

                        serial_println!("data header is: {:?}", data_buffer);
                        init_qtds.reset();

                        serial_println!("finished");
                        ehci.get_qh_mut(init_endpoint)
                            .ep_char
                            .set_max_packet_size(data_buffer.b_max_packet_size0 as u16);
                        let init_qtds = ehci.qtd_map.get_mut(&init_endpoint).unwrap();
                        init_qtds.build_set_address_req(&mut setup_buffer, 1);
                        init_qtds.link_qtds();
                        init_qtds.activate();
                        EHCI_CONTROLLER
                            .get_mut()
                            .unwrap()
                            .activate_overlay(init_endpoint);
                        // serial_println!("setup data: {:?}", setup_buffer);
                        if ehci.get_status_flags().contains(EhciStatus::INTERRUPT) {
                            ehci.add_status_flags(EhciStatus::INTERRUPT);
                        }
                        serial_println!("status: {:?}", ehci.get_status_flags());
                        serial_println!("setup data: {:?}", setup_buffer);
                        wait_ms(20);
                        ehci.dump(SerialWriter);
                        serial_println!("status: {:?}", ehci.get_status_flags());

                        // serial_println!("data is: {:?}", data_buffer);
                    }
                }
            }
        }
    }

    loop {
        // serial_println!("before tick: {}", TICKS.load(Ordering::Relaxed));
        if let Some(framebuffer) = unsafe { RENDER_BUFFER.get_mut().unwrap() } {
            framebuffer.clear(framebuffer.rgb_color(128, 32, 64));
            textbuff_println!("i love banana");
            textbuff_println!("tick: {}", TICKS.load(Ordering::Relaxed));
            textbuff_println!("res: {}x{}", framebuffer.width(), framebuffer.height());
            textbuff_println!("pitch: {}", framebuffer.pitch_u32());
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
