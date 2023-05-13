use gen_font::gen_font;

use crate::graphics::{self, PixcelWritable};

gen_font!();

pub trait AsciiWriter: PixcelWritable {
    fn write_ascii(&self, x: usize, y: usize, c: char, color: graphics::Color) {
        let Some(font) = FONT.get(c as usize) else {
            return;
        };
        for dy in 0..16 {
            for dx in 0..8 {
                if font[dy] & (1 << (7 - dx)) != 0 {
                    self.write(x + dx, y + dy, color);
                }
            }
        }
    }

    fn write_string(&self, x: usize, y: usize, s: &str, color: graphics::Color) {
        for (idx, c) in s.chars().enumerate() {
            self.write_ascii(x + 8 * idx, y, c, color);
        }
    }
}
impl<T> AsciiWriter for T where T: PixcelWritable {}
