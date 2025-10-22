use spin::Once;
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTableFlags, PhysFrame,
        Size4KiB,
    },
};

use crate::allocator::{
    allocator::HHDM_REQ,
    tree_alloc::{RBTreeAlloc, TREE_ALLOCATOR_DATA},
};

unsafe impl FrameAllocator<Size4KiB> for RBTreeAlloc {
    fn allocate_frame(&mut self) -> Option<x86_64::structures::paging::PhysFrame<Size4KiB>> {
        let hhdm_offset = HHDM_REQ.get_response().unwrap().offset();
        let virt = self
            .alloc(0x1000, 0x1000)
            .map(|x| x as u64)
            .unwrap_or_else(|| panic!("failed to allocate for FrameAllocator"));
        Some(PhysFrame::containing_address(
            PhysAddr::try_new(virt - hhdm_offset).unwrap_or_else(|_| panic!("here is the problem")),
        ))
    }
}
pub static mut PAGE_TABLE_MAPPER: Once<OffsetPageTable> = Once::new();
pub unsafe fn init_mapper() -> OffsetPageTable<'static> {
    let hhdm_offset = HHDM_REQ.get_response().unwrap().offset();
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address().as_u64();
    let virt = VirtAddr::new(phys + hhdm_offset); // convert phys addr into virtual via HHDM
    let l4_table: &mut x86_64::structures::paging::PageTable = unsafe { &mut *virt.as_mut_ptr() };
    unsafe { OffsetPageTable::new(l4_table, VirtAddr::new(hhdm_offset)) }
}
pub fn map_mmio_region(phys: u64, size: u64) -> *mut u8 {
    // We'll place MMIO windows here (pick a safe hole in higher half).
    // You can also make a simple "bump" for MMIO mappings.
    const MMIO_BASE: u64 = 0xFFFF_A000_0000_0000;

    // Align start/end to 4KiB pages
    let phys_start = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(phys));
    let phys_end = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(phys + size - 1));

    // First free virtual address for this mapping (could track with a global bump ptr)
    static mut NEXT_MMIO_VIRT: u64 = MMIO_BASE;
    let virt_start = unsafe {
        let v = NEXT_MMIO_VIRT;
        NEXT_MMIO_VIRT += ((phys_end.start_address().as_u64()
            - phys_start.start_address().as_u64())
            + Size4KiB::SIZE as u64)
            .next_multiple_of(Size4KiB::SIZE as u64);
        v
    };
    let mut virt = VirtAddr::new(virt_start);

    // Get mapper + allocator
    let mapper = unsafe { PAGE_TABLE_MAPPER.get_mut().unwrap() };
    let frame_alloc = unsafe { &mut TREE_ALLOCATOR_DATA };

    for frame in PhysFrame::range_inclusive(phys_start, phys_end) {
        let page: Page<Size4KiB> = Page::containing_address(virt);
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE; // Strongly recommended for MMIO

        unsafe {
            // Map device frame to kernel virtual page
            mapper
                .map_to(page, frame, flags, frame_alloc)
                .expect("map_to failed for MMIO")
                .flush();
        }

        virt += Size4KiB::SIZE;
    }

    virt_start as *mut u8
}
