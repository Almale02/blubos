use core::{
    alloc::GlobalAlloc,
    ptr::{NonNull, slice_from_raw_parts_mut},
};

use alloc::alloc::Allocator;
use spin::Once;

use crate::{
    allocator::tree_alloc::{TREE_ALLOCATOR_DATA, TreeAlloc},
    byte_ext::ByteExt as _,
    pci::usb::xhci::registers::AddressMode,
};
pub static mut XHCI_ADDRESS_MODE: Once<AddressMode> = Once::new();
#[derive(Default)]
pub struct XhciAllocator {
    pub boundary: Option<usize>,
}
unsafe impl Allocator for XhciAllocator {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, alloc::alloc::AllocError> {
        unsafe {
            Ok(NonNull::new_unchecked(slice_from_raw_parts_mut(
                TreeAlloc {
                    max_address: {
                        match { XHCI_ADDRESS_MODE.get().unwrap() } {
                            AddressMode::Bit32 => Some(4.gb()),
                            AddressMode::Bit64 => None,
                        }
                    },
                    boundary: self.boundary,
                }
                .alloc(layout),
                layout.size(),
            )))
        }
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: core::alloc::Layout) {
        unsafe {
            TREE_ALLOCATOR_DATA.free(ptr.addr().get(), layout.size());
        }
    }
}
