use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::ptr::null_mut;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use limine::memory_map::EntryType;
use limine::request::MemoryMapRequest;
use x86_64::VirtAddr;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::OffsetPageTable;
use x86_64::structures::paging::PageTable;
use x86_64::structures::paging::Translate;

use crate::serial_println;

static MEMMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();

pub fn _is_mapped(vaddr: u64) -> bool {
    let (frame, _) = Cr3::read();
    let table_addr = frame.start_address();
    let table_ptr = table_addr.as_u64() as *mut PageTable;

    // SAFETY: assume PML4 table is identity‑mapped
    // let phys_to_virt = |phys: PhysAddr| VirtAddr::new(phys.as_u64());

    let mapper = unsafe { OffsetPageTable::new(&mut *table_ptr, VirtAddr::new(0)) };

    let va = VirtAddr::new(vaddr);
    mapper.translate_addr(va).is_some()
}

pub fn is_mapped(vaddr: u64) -> bool {
    let (frame, _) = Cr3::read();
    let table_addr = frame.start_address().as_u64() as *mut PageTable;

    // SAFETY: assumes the physical memory containing the PML4 is
    // identity‑mapped or mapped at offset 0
    let pml4 = unsafe { &mut *table_addr };

    let mapper = unsafe { OffsetPageTable::new(pml4, VirtAddr::new(0)) };

    mapper.translate_addr(VirtAddr::new(vaddr)).is_some()
}

pub struct HeapArea {
    pub base: usize,
    pub size: usize,
}

pub fn find_heap_area() -> Option<HeapArea> {
    let resp = MEMMAP_REQ.get_response()?;

    for entry in resp.entries() {
        if entry.entry_type == EntryType::USABLE && entry.base >= 0x100000 {
            let base = entry.base as usize;
            let size = entry.length as usize;

            // check if first page of region is mapped
            if is_mapped(base as u64) {
                // require at least 16 MiB
                if size >= 32 * 1024_usize.pow(2) {
                    return Some(HeapArea { base, size });
                }
            }
        }
    }
    serial_println!("not found");
    None
}

static mut HEAP_BASE: usize = 0;
static mut HEAP_SIZE: usize = 0;
static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn init_heap() {
    if let Some(area) = find_heap_area() {
        unsafe {
            HEAP_BASE = area.base;
            HEAP_SIZE = area.size;
            NEXT.store(HEAP_BASE, Ordering::Relaxed);
        }
        serial_println!("heap at {:#x}, size {:#x}", area.base, area.size);
    } else {
        panic!("No suitable mapped heap region found");
    }
}

pub struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();

        let mut current = NEXT.load(Ordering::Relaxed);

        if current % align != 0 {
            current += align - (current % align);
        }

        let next = current.saturating_add(size);
        unsafe {
            if next > HEAP_BASE + HEAP_SIZE {
                null_mut()
            } else {
                NEXT.store(next, Ordering::Relaxed);
                current as *mut u8
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // bump allocator can't free
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: BumpAllocator = BumpAllocator;

#[alloc_error_handler]
fn oom(_layout: core::alloc::Layout) -> ! {
    panic!("Out of memory in bump allocator");
}
