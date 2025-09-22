use crate::framebuffer::Framebuffer;

pub static FONT8X16: [[u8; 16]; 256] = include!("../font/vga8x16_font.in");

impl Framebuffer<'_> {
    pub fn draw_char(&self, x: usize, y: usize, ch: u8, fg: u32, bg: Option<u32>, scale: usize) {
        let glyph = &FONT8X16[ch as usize];

        for (row, row_bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                let bit = (row_bits >> (7 - col)) & 1;
                let color = if bit == 1 {
                    fg
                } else if let Some(bgcol) = bg {
                    bgcol
                } else {
                    continue; // transparent background
                };

                // handle scaling
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = x + col * scale + dx;
                        let py = y + row * scale + dy;

                        if px < self.width() && py < self.height() {
                            self.set(py, px, color);
                        }
                    }
                }
            }
        }
    }
    pub fn draw_string(
        &self,
        mut x: usize,
        y: usize,
        text: &str,
        fg: u32,
        bg: Option<u32>,
        scale: usize,
    ) {
        for ch in text.bytes() {
            self.draw_char(x, y, ch, fg, bg, scale);
            x += 8 * scale;
        }
    }
}
