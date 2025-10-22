use core::{alloc::GlobalAlloc, ptr::null_mut};

use alloc::boxed::Box;

#[derive(Copy, Clone, Debug, PartialEq)]
enum Color {
    Red,
    Black,
}

pub struct Region {
    base: usize,
    size: usize,
    color: Color,
    left: Option<Box<Region>>,
    right: Option<Box<Region>>,
}

impl Region {
    fn new(base: usize, size: usize) -> Self {
        Self {
            base,
            size,
            color: Color::Red,
            left: None,
            right: None,
        }
    }

    fn end(&self) -> usize {
        self.base + self.size
    }
}

pub struct RBTreeAlloc {
    root: Option<Box<Region>>,
}

impl RBTreeAlloc {
    pub const fn new() -> Self {
        Self { root: None }
    }

    // ----------------- Insert -----------------
    pub fn insert(&mut self, base: usize, size: usize) {
        if size == 0 {
            return;
        }
        let node = Box::new(Region::new(base, size));
        self.root = Self::insert_node(self.root.take(), node);
        if let Some(root) = self.root.as_mut() {
            root.color = Color::Black;
        }
    }

    fn insert_node(h: Option<Box<Region>>, node: Box<Region>) -> Option<Box<Region>> {
        if h.is_none() {
            return Some(node);
        }
        let mut hbox = h.unwrap();
        if node.base < hbox.base {
            hbox.left = Self::insert_node(hbox.left.take(), node);
        } else {
            hbox.right = Self::insert_node(hbox.right.take(), node);
        }

        // RB fixup
        if Self::is_red(&hbox.right) && !Self::is_red(&hbox.left) {
            hbox = Self::rotate_left(hbox);
        }
        if Self::is_red(&hbox.left) && Self::is_red(&hbox.left.as_ref().unwrap().left) {
            hbox = Self::rotate_right(hbox);
        }
        if Self::is_red(&hbox.left) && Self::is_red(&hbox.right) {
            Self::flip_colors(&mut hbox);
        }

        Some(hbox)
    }

    pub fn alloc(&mut self, size: usize, align: usize) -> Option<usize> {
        let (root, result) = Self::alloc_rec(self.root.take(), size, align);
        self.root = root;
        result
    }

    fn alloc_rec(
        node: Option<Box<Region>>,
        size: usize,
        align: usize,
    ) -> (Option<Box<Region>>, Option<usize>) {
        let mut node = match node {
            None => return (None, None),
            Some(n) => n,
        };

        // Search left subtree first
        let (left, addr) = Self::alloc_rec(node.left.take(), size, align);
        if addr.is_some() {
            node.left = left;
            return (Some(node), addr);
        }

        // Check this node
        let aligned = (node.base + (align - 1)) & !(align - 1);
        let adj = aligned - node.base;
        if node.size >= adj + size {
            let alloc_base = aligned;
            let consumed = (aligned + size) - node.base;
            if consumed < node.size {
                let leftover = Box::new(Region::new(node.base + consumed, node.size - consumed));
                node.right = Self::insert_node(node.right.take(), leftover);
            }
            return (node.right.take(), Some(alloc_base));
        }

        // Try right subtree
        let (right, addr) = Self::alloc_rec(node.right.take(), size, align);
        node.right = right;
        (Some(node), addr)
    }

    // ----------------- Free -----------------
    pub fn free(&mut self, base: usize, size: usize) {
        // New region we want to free
        let mut new_base = base;
        let mut new_size = size;

        // Merge with predecessor
        if let Some(pred) = self.find_predecessor(new_base) {
            if pred.end() == new_base {
                new_base = pred.base;
                new_size += pred.size;
                self.delete(pred.base);
            }
        }

        // Merge with successor
        if let Some(succ) = self.find_successor(new_base + new_size) {
            if new_base + new_size == succ.base {
                new_size += succ.size;
                self.delete(succ.base);
            }
        }

        // Insert merged region
        self.insert(new_base, new_size);
    }

    // ----------------- Helpers -----------------
    fn is_red(x: &Option<Box<Region>>) -> bool {
        matches!(x, Some(r) if r.color == Color::Red)
    }

    fn rotate_left(mut h: Box<Region>) -> Box<Region> {
        let mut x = h.right.take().unwrap();
        h.right = x.left.take();
        x.left = Some(h);
        x.color = x.left.as_ref().unwrap().color;
        x.left.as_mut().unwrap().color = Color::Red;
        x
    }

    fn rotate_right(mut h: Box<Region>) -> Box<Region> {
        let mut x = h.left.take().unwrap();
        h.left = x.right.take();
        x.right = Some(h);
        x.color = x.right.as_ref().unwrap().color;
        x.right.as_mut().unwrap().color = Color::Red;
        x
    }

    fn flip_colors(h: &mut Box<Region>) {
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

    // These are linear-time scans (not optimized)
    fn find_predecessor(&self, addr: usize) -> Option<&Region> {
        let mut node = self.root.as_ref();
        let mut pred = None;
        while let Some(n) = node {
            if n.base < addr {
                pred = Some(n.as_ref());
                node = n.right.as_ref();
            } else {
                node = n.left.as_ref();
            }
        }
        pred
    }

    fn find_successor(&self, addr: usize) -> Option<&Region> {
        let mut node = self.root.as_ref();
        let mut succ = None;
        while let Some(n) = node {
            if n.base >= addr {
                succ = Some(n.as_ref());
                node = n.left.as_ref();
            } else {
                node = n.right.as_ref();
            }
        }
        succ
    }

    fn delete(&mut self, _base: usize) {
        // TODO: implement proper RBTree deletion
        // For simplicity now: memory leak instead of removing nodes
        // (safe for bring-up; real kernel needs delete)
    }
}
static mut TREE_ALLOCATOR_DATA: RBTreeAlloc = RBTreeAlloc::new();
pub static mut TREE_ALLOCATOR: TreeAlloc = TreeAlloc;
pub struct TreeAlloc;
impl TreeAlloc {
    pub fn init(&self) {
        unsafe {
            if TREE_ALLOC_STATE.is_some() {
                panic!("TreeAlloc already initialized");
            }
            TREE_ALLOC_STATE = Some(RegionAllocator::new());
            let ra = TREE_ALLOC_STATE.as_mut().unwrap();

            let mut areas = [Region::default(); 128];
            let area_count = find_heap_areas(&mut areas).unwrap();
            let areas = &mut areas[..area_count];
            areas.iter_mut().for_each(|x| {
                if BUMP_HEAP_BASE == x.base {
                    x.base += BUMP_HEAP_SIZE;
                    x.size -= BUMP_HEAP_SIZE;
                }
            });
            for region in areas {
                ra.insert(region.base, region.size);
            }
            // KERNEL_ALLOC_MODE = KernelAllocMode::Tree;
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
