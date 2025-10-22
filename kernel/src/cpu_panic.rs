use core::arch::asm;
use x86_64::instructions::tables::load_tss;
use x86_64::registers::control::Cr2;
use x86_64::registers::segmentation::{CS, DS, ES, SS, Segment};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};

use x86_64::{
    VirtAddr,
    structures::{
        idt::{InterruptStackFrame, PageFaultErrorCode},
        tss::TaskStateSegment,
    },
};

use crate::{serial::serial_write_str, serial_println};

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
    serial_write_str("double fault with code");

    loop {
        unsafe { asm!("cli; hlt") }
    }
}

pub extern "x86-interrupt" fn general_protection_fault_handler(
    stack: InterruptStackFrame,
    error_code: u64,
) {
    serial_println!("general prot fault: {}", error_code);
    serial_println!("{:?}", stack);
    panic!()
}
pub extern "x86-interrupt" fn page_fault_handler(
    stack: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    if let Some(addr) = Cr2::read().ok() {
        serial_println!(
            "page fault: {:?} at address {:#x}\n{:?}",
            error_code,
            addr,
            stack
        );
    } else {
        serial_println!(
            "page fault: {:?} at unkown address\n{:?}",
            error_code,
            stack
        );
    }
    panic!()
}

pub extern "x86-interrupt" fn invalid_opcode_handler(_stack: InterruptStackFrame) {
    serial_write_str("invalid opcode");
    loop {
        unsafe { asm!("cli; hlt") }
    }
}

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
pub const PAGE_FAULT_IST_INDEX: u16 = 1;

// static TSS
pub static mut TSS: TaskStateSegment = TaskStateSegment::new();

static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();

pub fn init_tss() {
    unsafe {
        // Allocate a stack for double fault
        const STACK_SIZE: usize = 4096 * 5;
        static mut DOUBLE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
        static mut PAGE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

        let df_stack_top = VirtAddr::from_ptr(&DOUBLE_FAULT_STACK) + STACK_SIZE as u64;
        let pf_stack_top = VirtAddr::from_ptr(&PAGE_FAULT_STACK) + STACK_SIZE as u64;
        TSS.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = df_stack_top;
        TSS.interrupt_stack_table[PAGE_FAULT_IST_INDEX as usize] = pf_stack_top;

        // Make GDT with code/data + TSS
        let gdt = &mut GDT;
        let code_segment = gdt.append(Descriptor::kernel_code_segment());
        let data_segment = gdt.append(Descriptor::kernel_data_segment());
        let tss_sel = gdt.append(Descriptor::tss_segment(&TSS));

        gdt.load();
        CS::set_reg(code_segment);
        DS::set_reg(data_segment);
        ES::set_reg(data_segment);
        SS::set_reg(data_segment);

        // Load selectors
        load_tss(tss_sel);
    }
}
