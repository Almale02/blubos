use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::{NonNull, null_mut},
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::alloc::Allocator;
use bytesize::ByteSize;

use crate::{
    allocator::allocator::{KERNEL_ALLOC_MODE, KernelAllocMode, Region, find_heap_areas},
    serial_println,
};

pub static mut BUMP_HEAP_BASE: usize = 0;
pub static mut BUMP_HEAP_SIZE: usize = 0;
static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn init_bump_alloc() {
    let mut areas = [Region::default(); 256];
    if let Some(area_count) = find_heap_areas(&mut areas) {
        let areas = &areas[..area_count];
        let area = areas
            .iter()
            .find(|x| x.size >= 3 * 1024_usize.pow(2))
            .unwrap_or_else(|| panic!("No suitable heap region found"));

        serial_println!(
            "found heap area at {:#x}, size {}",
            area.base,
            ByteSize(area.size as u64)
        );
        unsafe {
            BUMP_HEAP_BASE = area.base;
            BUMP_HEAP_SIZE = 2 * 1024_usize.pow(2);
            NEXT.store(BUMP_HEAP_BASE, Ordering::Relaxed);
        }
        unsafe {
            KERNEL_ALLOC_MODE = KernelAllocMode::Bump;
        }
    } else {
        panic!("No suitable heap region found");
    }
}

pub struct BumpAlloc;
pub static BUMP_ALLOCATOR: BumpAlloc = BumpAlloc;

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();

        let mut current = NEXT.load(Ordering::Relaxed);

        if current % align != 0 {
            current += align - (current % align);
        }

        let next = current.saturating_add(size);
        unsafe {
            if next > BUMP_HEAP_BASE + BUMP_HEAP_SIZE {
                null_mut()
            } else {
                NEXT.store(next, Ordering::Relaxed);
                // serial_println!("bump allocated at {:x}", current);
                current as *mut u8
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // bump allocator can't free
    }
}

unsafe impl Allocator for BumpAlloc {
    fn allocate(
        &self,
        layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        let ptr = unsafe { <BumpAlloc as GlobalAlloc>::alloc(self, layout) };
        if ptr.is_null() {
            Err(core::alloc::AllocError)
        } else {
            // Construct slice pointer
            unsafe {
                Ok(core::ptr::NonNull::new_unchecked(
                    core::ptr::slice_from_raw_parts_mut(ptr, layout.size()),
                ))
            }
        }
    }

    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {
        // bump can't free; leak
    }
}
