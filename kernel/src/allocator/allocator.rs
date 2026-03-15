use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::fmt::Display;

use alloc::alloc::Allocator;
use alloc::boxed::Box;
use bytesize::ByteSize;
use limine::memory_map::EntryType;
use limine::request::HhdmRequest;
use limine::request::MemoryMapRequest;
use x86_64::VirtAddr;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::OffsetPageTable;
use x86_64::structures::paging::PageTable;
use x86_64::structures::paging::Translate;

use crate::allocator::bump_alloc::BUMP_ALLOCATOR;
use crate::allocator::paging::PAGE_TABLE_MAPPER;
use crate::allocator::tree_alloc::TREE_ALLOCATOR;

static MEMMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();
pub static HHDM_REQ: HhdmRequest = HhdmRequest::new();

pub fn is_mapped(vaddr: u64) -> bool {
    let (frame, _) = Cr3::read();
    let table_addr = frame.start_address().as_u64() as *mut PageTable;

    let pml4 = unsafe { &mut *table_addr };

    let mapper = unsafe { OffsetPageTable::new(pml4, VirtAddr::new(0)) };

    mapper.translate_addr(VirtAddr::new(vaddr)).is_some()
}
pub fn to_physical_addr(virtual_addr: usize) -> usize {
    match unsafe { PAGE_TABLE_MAPPER.get() } {
        Some(mapper) => mapper
            .translate_addr(VirtAddr::new(virtual_addr as u64))
            .unwrap()
            .as_u64() as usize,
        None => {
            let hhdm_offset = HHDM_REQ.get_response().unwrap().offset();
            virtual_addr - hhdm_offset as usize
        }
    }
}
pub trait GetPhysicalAddr {
    fn to_physical_addr(&self) -> usize;
}
impl<T: ?Sized, A: Allocator> GetPhysicalAddr for Box<T, A> {
    fn to_physical_addr(&self) -> usize {
        let ptr = self.as_ref() as *const T;
        let (data_ptr, _meta) = ptr.to_raw_parts();
        to_physical_addr(data_ptr as usize)
    }
}
impl<T> GetPhysicalAddr for &T {
    fn to_physical_addr(&self) -> usize {
        to_physical_addr((*self as *const _) as usize)
    }
}
impl<T> GetPhysicalAddr for &mut T {
    fn to_physical_addr(&self) -> usize {
        to_physical_addr((*self as *const _) as usize)
    }
}
impl GetPhysicalAddr for usize {
    fn to_physical_addr(&self) -> usize {
        to_physical_addr(*self)
    }
}

#[derive(Copy, Clone, Default)]
pub struct Region {
    /// virtual base
    pub base: usize,
    pub size: usize,
}

impl Region {
    pub fn new(base: usize, size: usize) -> Self {
        Self { base, size }
    }
}
impl Display for Region {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "region at {:#0x} with size: {:?}",
            self.base,
            ByteSize(self.size as u64)
        )
    }
}

pub fn find_heap_areas(areas: &mut [Region]) -> Option<usize> {
    let resp = MEMMAP_REQ.get_response()?;
    let hhdm = HHDM_REQ.get_response()?.offset() as usize;
    let mut len = 0;

    for entry in resp.entries() {
        // serial_println!(
        //     "Entry: base={:#x}, length={:#x}, type={}",
        //     entry.base,
        //     entry.length,
        //     match entry.entry_type {
        //         EntryType::USABLE => "USABLE",
        //         EntryType::RESERVED => "RESERVED",
        //         EntryType::ACPI_RECLAIMABLE => "ACPI_RECLAIMABLE",
        //         EntryType::ACPI_NVS => "ACPI_NVS",
        //         EntryType::BAD_MEMORY => "BAD_MEMORY",
        //         EntryType::BOOTLOADER_RECLAIMABLE => "BOOTLOADER_RECLAIMABLE",
        //         EntryType::EXECUTABLE_AND_MODULES => "EXECUTABLE_AND_MODULES",
        //         EntryType::FRAMEBUFFER => "FRAMEBUFFER",
        //         _ => unreachable!(),
        //     }
        // );
        if (entry.entry_type == EntryType::USABLE
            || entry.entry_type == EntryType::BOOTLOADER_RECLAIMABLE)
            && entry.base >= 0x100000
        {
            let base_phys = entry.base as usize;
            let size = entry.length as usize;
            areas[len] = Region {
                base: hhdm + base_phys,
                size,
            };
            len += 1;
        }
    }
    Some(len)
}
pub struct KernelAlloc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelAllocMode {
    Uninit,
    Bump,
    Tree,
}
pub static mut KERNEL_ALLOC_MODE: KernelAllocMode = KernelAllocMode::Uninit;

unsafe impl GlobalAlloc for KernelAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            match KERNEL_ALLOC_MODE {
                KernelAllocMode::Uninit => {
                    panic!("tried to allocate without a allocator initialized")
                }
                KernelAllocMode::Bump => BUMP_ALLOCATOR.alloc(layout),
                KernelAllocMode::Tree => TREE_ALLOCATOR.alloc(layout),
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            match KERNEL_ALLOC_MODE {
                KernelAllocMode::Uninit => {
                    panic!("tried to free without a allocator initialized")
                }
                KernelAllocMode::Bump => BUMP_ALLOCATOR.dealloc(ptr, layout),
                KernelAllocMode::Tree => TREE_ALLOCATOR.dealloc(ptr, layout),
            }
        }
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: KernelAlloc = KernelAlloc;

#[alloc_error_handler]
fn oom(_layout: core::alloc::Layout) -> ! {
    match unsafe { KERNEL_ALLOC_MODE } {
        KernelAllocMode::Uninit => todo!(),
        KernelAllocMode::Bump => panic!("out of memory in bump allocator"),
        KernelAllocMode::Tree => panic!("out of memory in tree allocator"),
    }
}
pub trait AlignedBoxAlloc<T, A: Allocator> {
    fn new_in_aligned(x: T, alloc: A, align: usize) -> Box<T, A>;
}
impl<T, A: Allocator> AlignedBoxAlloc<T, A> for Box<T, A> {
    fn new_in_aligned(x: T, alloc: A, align: usize) -> Box<T, A> {
        let layout = Layout::from_size_align(size_of::<T>(), align).unwrap();
        let base_ptr = alloc
            .allocate(layout)
            .expect("out of memory")
            .as_non_null_ptr()
            .cast()
            .as_ptr();

        unsafe {
            *base_ptr = x;
            Box::from_raw_in(base_ptr, alloc)
        }
    }
}
