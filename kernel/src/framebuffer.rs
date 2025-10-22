use core::ptr::copy_nonoverlapping;

use alloc::boxed::Box;
use limine::framebuffer::Framebuffer as LimlineFramebuffer;

pub struct Framebuffer<'a> {
    inner: LimlineFramebuffer<'a>,
    backbuffer: Box<[u32]>,
}
impl<'a> Framebuffer<'a> {
    pub fn new(fb: LimlineFramebuffer<'a>) -> Framebuffer<'a> {
        if fb.bpp() != 32 {
            panic!("only 4 byte pixel data supported");
        }
        let fb_heigh = fb.height();
        let fb_pitch = fb.pitch();
        Framebuffer {
            inner: fb,
            backbuffer: unsafe {
                Box::new_zeroed_slice(fb_heigh as usize * fb_pitch as usize / size_of::<u32>())
                    .assume_init()
            },
        }
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
    pub fn get_element_ptr(&mut self, y: usize, x: usize) -> *mut u32 {
        unsafe { self.backbuffer.as_mut_ptr().add(y * self.pitch_u32() + x) }
    }
    pub fn get_element_ref(&self, y: usize, x: usize) -> &u32 {
        debug_assert!(y < self.height());
        debug_assert!(x < self.width());
        let offset = y * self.pitch_u32() + x;
        &self.backbuffer[offset]
    }
    pub fn get_element_mut(&mut self, y: usize, x: usize) -> &mut u32 {
        debug_assert!(y < self.height());
        debug_assert!(x < self.width());
        let offset = y * self.pitch_u32() + x;
        &mut self.backbuffer[offset]
    }
    pub fn get(&self, y: usize, x: usize) -> u32 {
        *self.get_element_ref(y, x)
    }
    pub fn set(&mut self, y: usize, x: usize, pixel: u32) {
        *self.get_element_mut(y, x) = pixel
    }

    pub fn clear(&mut self, color: u32) {
        let total = self.pitch_u32() * self.height(); // stride × height

        unsafe {
            core::arch::asm!(
                "rep stosd",
                inout("rcx") total as u64 => _,       // pitch, not width
                inout("rdi") (self.backbuffer.as_mut_ptr()) as u64=> _,
                in("eax") color,
                options(nostack, preserves_flags)
            );
        }
    }
    pub fn flip(&mut self) {
        unsafe {
            copy_nonoverlapping(
                self.backbuffer.as_ptr(),
                self.inner.addr() as *mut u32,
                self.height() * self.pitch_u32(),
            );
        }
    }
}
/*





















*/
