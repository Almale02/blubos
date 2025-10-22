use alloc::{
    alloc::{AllocError, Allocator},
    boxed::Box,
};
use bytesize::ByteSize;
use core::{
    alloc::{GlobalAlloc, Layout},
    fmt::Write,
    ptr::null_mut,
};

use crate::{
    allocator::{
        allocator::{GetPhysicalAddr, KERNEL_ALLOC_MODE, KernelAllocMode, Region, find_heap_areas},
        bump_alloc::{BUMP_ALLOCATOR, BUMP_HEAP_BASE, BUMP_HEAP_SIZE, BumpAlloc},
    },
    serial_print, serial_println,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Color {
    Red,
    Black,
}

type NodeBox = Box<TreeRegion, &'static BumpAlloc>;

pub struct TreeRegion {
    pub base: usize,
    pub size: usize,
    color: Color,
    max_size: usize,
    left: Option<NodeBox>,
    right: Option<NodeBox>,
}

impl TreeRegion {
    fn new(base: usize, size: usize) -> Self {
        Self {
            base,
            size,
            color: Color::Red,
            max_size: size,
            left: None,
            right: None,
        }
    }
    fn end(&self) -> usize {
        self.base + self.size
    }
}

pub struct RBTreeAlloc {
    root: Option<NodeBox>,
    freelist: Option<NodeBox>, // NEW
}

impl RBTreeAlloc {
    pub const fn new() -> Self {
        Self {
            root: None,
            freelist: None, // store reusable TreeRegion nodes here
        }
    }

    pub fn debug_dump(&self) {
        fn dfs(node: &Option<NodeBox>, depth: usize) {
            if let Some(n) = node {
                for _ in 0..depth {
                    serial_print!("  ");
                }
                serial_println!(
                    "region base={:#x} size={} max_size={}",
                    n.base,
                    ByteSize(n.size as u64),
                    ByteSize(n.max_size as u64)
                );
                dfs(&n.left, depth + 1);
                dfs(&n.right, depth + 1);
            }
        }
        serial_println!("---Tree dump---");
        dfs(&self.root, 0);

        // Optionally show how many nodes are sitting in the freelist
        let mut free_count = 0;
        let mut cur = &self.freelist;
        while let Some(node) = cur {
            free_count += 1;
            cur = &node.left;
        }
        serial_println!("(freelist nodes: {})", free_count);
    }
    // --------------------------
    // Node recycling
    // --------------------------

    fn alloc_node(&mut self, base: usize, size: usize) -> NodeBox {
        if let Some(mut node) = self.freelist.take() {
            // Pop from freelist (reuse old node)
            self.freelist = node.left.take(); // using .left as freelist "next"

            node.base = base;
            node.size = size;
            node.max_size = size;
            node.color = Color::Red;
            node.left = None;
            node.right = None;
            node
        } else {
            // Bootstrap via bump allocator
            Box::new_in(TreeRegion::new(base, size), &BUMP_ALLOCATOR)
        }
    }

    fn free_node(&mut self, mut node: NodeBox) {
        // Push node into freelist; thread .left as linked-list next
        node.left = self.freelist.take();
        node.right = None;
        self.freelist = Some(node);
    }

    // --------------------------
    // Helpers
    // --------------------------

    fn is_red(x: &Option<NodeBox>) -> bool {
        matches!(x, Some(r) if r.color == Color::Red)
    }

    fn update_max_size(n: &mut NodeBox) {
        let mut max = n.size;
        if let Some(ref l) = n.left {
            max = max.max(l.max_size);
        }
        if let Some(ref r) = n.right {
            max = max.max(r.max_size);
        }
        n.max_size = max;
    }

    fn rotate_left(mut h: NodeBox) -> NodeBox {
        let mut x = h.right.take().unwrap();
        h.right = x.left.take();
        RBTreeAlloc::update_max_size(&mut h);
        x.left = Some(h);
        x.color = x.left.as_ref().unwrap().color;
        x.left.as_mut().unwrap().color = Color::Red;
        RBTreeAlloc::update_max_size(&mut x);
        x
    }

    fn rotate_right(mut h: NodeBox) -> NodeBox {
        let mut x = h.left.take().unwrap();
        h.left = x.right.take();
        RBTreeAlloc::update_max_size(&mut h);
        x.right = Some(h);
        x.color = x.right.as_ref().unwrap().color;
        x.right.as_mut().unwrap().color = Color::Red;
        RBTreeAlloc::update_max_size(&mut x);
        x
    }

    fn flip_colors(h: &mut NodeBox) {
        h.color = match h.color {
            Color::Red => Color::Black,
            Color::Black => Color::Red,
        };
        if let Some(ref mut l) = h.left {
            l.color = match l.color {
                Color::Red => Color::Black,
                Color::Black => Color::Red,
            };
        }
        if let Some(ref mut r) = h.right {
            r.color = match r.color {
                Color::Red => Color::Black,
                Color::Black => Color::Red,
            };
        }
    }

    fn insert_node(h: Option<NodeBox>, node: NodeBox) -> NodeBox {
        let mut h = match h {
            None => return node,
            Some(h) => h,
        };

        if node.base < h.base {
            h.left = Some(Self::insert_node(h.left.take(), node));
        } else {
            h.right = Some(Self::insert_node(h.right.take(), node));
        }

        if Self::is_red(&h.right) && !Self::is_red(&h.left) {
            h = Self::rotate_left(h);
        }
        if Self::is_red(&h.left) && Self::is_red(&h.left.as_ref().unwrap().left) {
            h = Self::rotate_right(h);
        }
        if Self::is_red(&h.left) && Self::is_red(&h.right) {
            Self::flip_colors(&mut h);
        }
        Self::update_max_size(&mut h);
        h
    }

    pub fn insert(&mut self, region: Region) {
        if region.size == 0 {
            return;
        }

        let node = self.alloc_node(region.base, region.size); // recycled alloc
        self.root = Some(Self::insert_node(self.root.take(), node));

        if let Some(r) = self.root.as_mut() {
            r.color = Color::Black;
        }
    }

    pub fn alloc(&mut self, size: usize, align: usize) -> Option<usize> {
        let (root, out) = Self::alloc_rec(self.root.take(), size, align);
        self.root = root;
        if let Some(r) = self.root.as_mut() {
            r.color = Color::Black;
        }
        out
    }

    fn alloc_rec(
        node: Option<NodeBox>,
        size: usize,
        align: usize,
    ) -> (Option<NodeBox>, Option<usize>) {
        let mut n = match node {
            None => return (None, None),
            Some(n) => n,
        };

        if let Some(ref l) = n.left {
            if l.max_size >= size {
                let (new_left, out) = Self::alloc_rec(n.left.take(), size, align);
                if out.is_some() {
                    n.left = new_left;
                    Self::update_max_size(&mut n);
                    return (Some(n), out);
                }
                n.left = new_left;
                Self::update_max_size(&mut n);
            }
        }

        let aligned = (n.base + (align - 1)) & !(align - 1);
        let adj = aligned - n.base;

        if n.size >= adj + size {
            // changed > to >= to allow exact fits
            let alloc_base = aligned;
            let consumed = adj + size;

            n.base += consumed;
            n.size -= consumed;
            Self::update_max_size(&mut n);
            return (Some(n), Some(alloc_base));
        }

        if let Some(ref r) = n.right {
            if r.max_size >= size {
                let (new_right, out) = Self::alloc_rec(n.right.take(), size, align);
                if out.is_some() {
                    n.right = new_right;
                    Self::update_max_size(&mut n);
                    return (Some(n), out);
                }
                n.right = new_right;
                Self::update_max_size(&mut n);
            }
        }

        Self::update_max_size(&mut n);
        (Some(n), None)
    }

    pub fn free(&mut self, base: usize, size: usize) {
        let mut new_base = base;
        let mut new_size = size;

        let mut pred_key = None;
        let mut succ_key = None;

        if let Some(p) = self.predecessor(self.root.as_ref(), base) {
            if p.end() >= new_base {
                new_base = p.base;
                new_size = (new_base + new_size).max(p.end()) - new_base;
                pred_key = Some(p.base);
            }
        }

        if let Some(s) = self.successor(self.root.as_ref(), base) {
            if new_base + new_size >= s.base {
                new_size = (s.base + s.size) - new_base;
                succ_key = Some(s.base);
            }
        }

        if let Some(key) = pred_key {
            let root = self.root.take();
            self.root = self.delete(root, key);
        }
        if let Some(key) = succ_key {
            let root = self.root.take();
            self.root = self.delete(root, key);
        }

        self.insert(Region {
            base: new_base,
            size: new_size,
        });
    }

    fn predecessor<'a>(
        &self,
        mut node: Option<&'a NodeBox>,
        addr: usize,
    ) -> Option<&'a TreeRegion> {
        let mut pred = None;
        while let Some(n) = node {
            if n.base < addr {
                pred = Some(&**n);
                node = n.right.as_ref();
            } else {
                node = n.left.as_ref();
            }
        }
        pred
    }

    fn successor<'a>(&self, mut node: Option<&'a NodeBox>, addr: usize) -> Option<&'a TreeRegion> {
        let mut succ = None;
        while let Some(n) = node {
            if n.base >= addr {
                succ = Some(&**n);
                node = n.left.as_ref();
            } else {
                node = n.right.as_ref();
            }
        }
        succ
    }

    fn move_red_left(mut h: NodeBox) -> NodeBox {
        Self::flip_colors(&mut h);
        if Self::is_red(&h.right.as_ref().unwrap().left) {
            let mut r = h.right.take().unwrap();
            r = Self::rotate_right(r);
            h.right = Some(r);
            h = Self::rotate_left(h);
            Self::flip_colors(&mut h);
        }
        h
    }

    fn move_red_right(mut h: NodeBox) -> NodeBox {
        Self::flip_colors(&mut h);
        if Self::is_red(&h.left.as_ref().unwrap().left) {
            h = Self::rotate_right(h);
            Self::flip_colors(&mut h);
        }
        h
    }

    fn fix_up(mut h: NodeBox) -> NodeBox {
        if Self::is_red(&h.right) {
            h = Self::rotate_left(h);
        }
        if Self::is_red(&h.left) && Self::is_red(&h.left.as_ref().unwrap().left) {
            h = Self::rotate_right(h);
        }
        if Self::is_red(&h.left) && Self::is_red(&h.right) {
            Self::flip_colors(&mut h);
        }
        Self::update_max_size(&mut h);
        h
    }

    fn min_node<'a>(mut h: &'a NodeBox) -> &'a TreeRegion {
        while let Some(ref l) = h.left {
            h = l;
        }
        h
    }

    fn delete_min(&mut self, mut h: NodeBox) -> Option<NodeBox> {
        if h.left.is_none() {
            self.free_node(h); // recycle node
            return None;
        }
        if !Self::is_red(&h.left) && !Self::is_red(&h.left.as_ref().unwrap().left) {
            h = Self::move_red_left(h);
        }
        h.left = self.delete_min(h.left.take().unwrap());
        Some(Self::fix_up(h))
    }

    fn delete(&mut self, h: Option<NodeBox>, key: usize) -> Option<NodeBox> {
        let mut h = match h {
            None => return None,
            Some(h) => h,
        };

        if key < h.base {
            if h.left.is_some() {
                if !Self::is_red(&h.left) && !Self::is_red(&h.left.as_ref().unwrap().left) {
                    h = Self::move_red_left(h);
                }
                h.left = self.delete(h.left.take(), key);
            }
        } else {
            if Self::is_red(&h.left) {
                h = Self::rotate_right(h);
            }
            if key == h.base && h.right.is_none() {
                self.free_node(h);
                return None;
            }
            if h.right.is_some() {
                if !Self::is_red(&h.right) && !Self::is_red(&h.right.as_ref().unwrap().left) {
                    h = Self::move_red_right(h);
                }
                if key == h.base {
                    let min = Self::min_node(h.right.as_ref().unwrap());
                    h.base = min.base;
                    h.size = min.size;
                    h.right = self.delete_min(h.right.take().unwrap());
                } else {
                    h.right = self.delete(h.right.take(), key);
                }
            }
        }

        Some(Self::fix_up(h))
    }

    /// allocates under 4GB
    pub fn alloc_low(&mut self, size: usize, align: usize) -> Option<usize> {
        let (root, out) = Self::alloc_rec_low(self.root.take(), size, align);
        self.root = root;
        if let Some(r) = self.root.as_mut() {
            r.color = Color::Black;
        }
        out
    }

    fn alloc_rec_low(
        node: Option<NodeBox>,
        size: usize,
        align: usize,
    ) -> (Option<NodeBox>, Option<usize>) {
        let mut n = match node {
            None => return (None, None),
            Some(n) => n,
        };

        if let Some(ref l) = n.left {
            if l.max_size >= size {
                let (new_left, out) = Self::alloc_rec_low(n.left.take(), size, align);
                if out.is_some() {
                    n.left = new_left;
                    Self::update_max_size(&mut n);
                    return (Some(n), out);
                }
                n.left = new_left;
                Self::update_max_size(&mut n);
            }
        }
        let aligned = (n.base + (align - 1)) & !(align - 1);
        let adj = aligned - n.base;

        if n.size >= adj + size && aligned.to_physical_addr() + size <= 0x1_0000_0000 {
            let alloc_base = aligned;
            let consumed = adj + size;

            n.base += consumed;
            n.size -= consumed;
            Self::update_max_size(&mut n);
            return (Some(n), Some(alloc_base));
        }

        // Right subtree search
        if let Some(ref r) = n.right {
            if r.max_size >= size {
                let (new_right, out) = Self::alloc_rec_low(n.right.take(), size, align);
                if out.is_some() {
                    n.right = new_right;
                    Self::update_max_size(&mut n);
                    return (Some(n), out);
                }
                n.right = new_right;
                Self::update_max_size(&mut n);
            }
        }

        Self::update_max_size(&mut n);
        (Some(n), None)
    }
}
pub struct TreeAllocLow;
unsafe impl Allocator for TreeAllocLow {
    fn allocate(&self, layout: Layout) -> Result<core::ptr::NonNull<[u8]>, AllocError> {
        unsafe {
            if let Some(addr) = TREE_ALLOCATOR_DATA.alloc_low(layout.size(), layout.align()) {
                let ptr = addr as *mut u8;
                Ok(core::ptr::NonNull::new_unchecked(
                    core::ptr::slice_from_raw_parts_mut(ptr, layout.size()),
                ))
            } else {
                Err(AllocError)
            }
        }
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: Layout) {
        // free back into the tree allocator
        unsafe { TREE_ALLOCATOR_DATA.free(ptr.as_ptr() as usize, layout.size()) };
    }
}

pub static mut TREE_ALLOCATOR_DATA: RBTreeAlloc = RBTreeAlloc::new();

pub struct TreeAlloc;
pub static TREE_ALLOCATOR: TreeAlloc = TreeAlloc;

impl TreeAlloc {
    pub fn init(&self) {
        unsafe {
            let mut areas = [Region::default(); 128];
            let count = find_heap_areas(&mut areas).unwrap();
            let areas = &mut areas[..count];
            for r in areas.iter_mut() {
                if r.base == BUMP_HEAP_BASE {
                    r.base += BUMP_HEAP_SIZE;
                    r.size -= BUMP_HEAP_SIZE;
                }
            }
            for r in areas.iter() {
                TREE_ALLOCATOR_DATA.insert(*r);
            }
            KERNEL_ALLOC_MODE = KernelAllocMode::Tree;
        }
    }
    pub fn write_init_areas(mut writer: impl Write) {
        unsafe {
            let mut areas = [Region::default(); 128];
            let count = find_heap_areas(&mut areas).unwrap();
            let areas = &mut areas[..count];
            for r in areas.iter_mut() {
                if r.base == BUMP_HEAP_BASE {
                    r.base += BUMP_HEAP_SIZE;
                    r.size -= BUMP_HEAP_SIZE;
                }
            }
            let mut total = 0;
            for (i, r) in areas.iter().enumerate() {
                writeln!(writer, "init area {}: {}", i, ByteSize(r.size as u64));
                total += r.size;
            }
            writeln!(writer, "tree initialized with {}", ByteSize(total as u64));
        }
    }
}

unsafe impl GlobalAlloc for TreeAlloc {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe {
            if let Some(addr) = TREE_ALLOCATOR_DATA.alloc(layout.size(), layout.align()) {
                addr as *mut u8
            } else {
                null_mut()
            }
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        unsafe {
            TREE_ALLOCATOR_DATA.free(ptr as usize, layout.size());
        }
    }
}
