use core::fmt::Write;

use alloc::string::String;
use spin::Once;

use crate::framebuffer::Framebuffer;

// VGA 8×16 bitmap font (256 glyphs)
pub static FONT8X16: [[u8; 16]; 256] = include!("../font/vga8x16_font.in");

impl Framebuffer<'_> {
    pub fn draw_char(&mut self, x: usize, y: usize, ch: u8, fg: u32, scale: usize) {
        let glyph = &FONT8X16[ch as usize];

        for (row, row_bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                let bit = (row_bits >> (7 - col)) & 1;
                if bit == 0 {
                    continue; // skip background pixels
                }

                // handle scaling
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = x + col * scale + dx;
                        let py = y + row * scale + dy;

                        if px < self.width() && py < self.height() {
                            self.set(py, px, fg);
                        }
                    }
                }
            }
        }
    }

    pub fn draw_string(
        &mut self,
        start_x: usize,
        start_y: usize,
        text: &str,
        fg: u32,
        scale: usize,
    ) {
        let mut x = start_x;
        let mut y = start_y;
        for ch in text.bytes() {
            match ch {
                b'\n' => {
                    x = start_x; // carriage return
                    y += 16 * scale; // newline = move one font height down
                }
                _ => {
                    self.draw_char(x, y, ch, fg, scale);
                    x += 8 * scale; // advance cursor horizontally
                }
            }
        }
    }
}
pub static mut TEXT_OUT_BUFFER: Once<String> = Once::new();

pub struct TextBufferWriter;

impl Write for TextBufferWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        unsafe { TEXT_OUT_BUFFER.get_mut().unwrap() }.push_str(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! textbuff_print {
    ($($arg: tt)*) => {
        {
            use core::fmt::Write as _;
            let _ = write!(crate::font_renderere::TextBufferWriter, $($arg)*);
        }
    };
}
#[macro_export]
macro_rules! textbuff_println {
    ($($arg: tt)*) => {
        {
            use core::fmt::Write as _;
            let _ = write!(crate::font_renderer::TextBufferWriter, $($arg)*);
            let _ = write!(crate::font_renderer::TextBufferWriter, "\n");
        }
    };
}
