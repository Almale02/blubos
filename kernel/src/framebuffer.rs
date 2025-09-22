use core::{arch::asm, ptr};

use limine::framebuffer::Framebuffer as LimlineFramebuffer;

pub struct Framebuffer<'a> {
    inner: LimlineFramebuffer<'a>,
}
impl<'a> Framebuffer<'a> {
    pub fn new(fb: LimlineFramebuffer<'a>) -> Framebuffer<'a> {
        if fb.bpp() != 32 {
            panic!("only 4 byte pixel data supported");
        }
        Framebuffer { inner: fb }
    }
    pub fn width(&self) -> usize {
        self.inner.width() as usize
    }
    pub fn height(&self) -> usize {
        self.inner.height() as usize
    }
    pub fn pitch_u32(&self) -> usize {
        self.inner.pitch() as usize / size_of::<u32>()
    }
    pub fn rgb_color(&self, r: u8, g: u8, b: u8) -> u32 {
        let rm = self.inner.red_mask_shift();
        let gm = self.inner.green_mask_shift();
        let bm = self.inner.blue_mask_shift();
        ((r as u32) << rm) | ((g as u32) << gm) | ((b as u32) << bm)
    }
    pub fn get_element_ptr(&self, y: usize, x: usize) -> *mut u32 {
        debug_assert!(y < self.height());
        debug_assert!(x < self.width());
        let offset = y * self.pitch_u32() + x;
        unsafe { (self.inner.addr() as *mut u32).add(offset) }
    }
    pub fn get(&self, y: usize, x: usize) -> u32 {
        unsafe { *self.get_element_ptr(y, x) }
    }
    pub fn set(&self, y: usize, x: usize, pixel: u32) {
        unsafe { *self.get_element_ptr(y, x) = pixel }
    }
    // pub fn clear(&self, color: u32) {
    //     let width = self.width();
    //     let height = self.height();

    //     for y in 0..height {
    //         let row_ptr = self.get_element_ptr(y, 0);
    //         unsafe {
    //             core::arch::asm!(
    //                 "cld",
    //                 "rep stosd",
    //                 in("rcx") width as u64,
    //                 in("rdi") row_ptr as u64,
    //                 in("eax") color,
    //                 options(nostack)
    //             );
    //         }
    //     }
    // }

    pub fn clear(&self, color: u32) {
        let total = self.pitch_u32() * self.height(); // stride × height

        unsafe {
            core::arch::asm!(
                "rep stosd",
                inout("rcx") total as u64 => _,       // pitch, not width
                inout("rdi") self.get_element_ptr(0, 0) as u64=> _,
                in("eax") color,
                options(nostack, preserves_flags)
            );
        }
    }
}
/*





















*/
