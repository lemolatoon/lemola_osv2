use gen_font::gen_font;

use crate::graphics::{self, PixcelInfo, PixcelWritable};

gen_font!();

pub trait AsciiWriter: PixcelWritable + PixcelInfo {
    fn write_ascii(
        &self,
        x: usize,
        y: usize,
        c: char,
        bg_color: graphics::Color,
        fg_color: graphics::Color,
    ) {
        let Some(font) = FONT.get(c as usize) else {
            return;
        };
        for dy in 0..16 {
            for dx in 0..8 {
                if font[dy] & (1 << (7 - dx)) != 0 {
                    self.write(x + dx, y + dy, fg_color);
                } else {
                    self.write(x + dx, y + dy, bg_color);
                }
            }
        }
    }

    fn write_string(&self, x: usize, y: usize, s: &str, color: graphics::Color) {
        for (idx, c) in s.chars().enumerate() {
            self.write_ascii(x + 8 * idx, y, c, graphics::Color::black(), color);
        }
    }

    fn num_writeable_char_per_line(&self) -> usize {
        self.horizontal_resolution() / 8
    }

    fn num_writeable_line(&self) -> usize {
        self.vertical_resolution() / 16
    }
}
impl<T> AsciiWriter for T where T: PixcelWritable + PixcelInfo {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    x: usize,
    y: usize,
}

impl CursorPosition {
    pub fn just_new_line(&mut self) {
        self.x = 0;
        self.y += 1;
    }
}

#[derive(Debug, Clone)]
pub struct Writer<const ROW: usize, const COLUMN: usize> {
    writer: &'static dyn AsciiWriter,
    position: CursorPosition,
    background_color: graphics::Color,
    foreground_color: graphics::Color,
    buffer: [[char; COLUMN]; ROW],
}

impl<const ROW: usize, const COLUMN: usize> Writer<ROW, COLUMN> {
    pub fn new(writer: &'static dyn AsciiWriter) -> Self {
        Self {
            writer,
            position: CursorPosition { x: 0, y: 0 },
            background_color: graphics::Color::black(),
            foreground_color: graphics::Color::white(),
            buffer: [[' '; COLUMN]; ROW],
        }
    }

    pub fn write_ascii_by_position(&mut self, c: char) {
        self.writer.write_ascii(
            self.position.x * 8,
            self.position.y * 16,
            c,
            self.background_color,
            self.foreground_color,
        );
    }

    pub fn put_char(&mut self, c: char) {
        if c == '\n' {
            self.new_line();
        } else if self.position.x < self.writer.num_writeable_char_per_line() {
            self.write_ascii_by_position(c);
            self.position.x += 1;
        } else {
            self.new_line();
            self.write_ascii_by_position(c);
            self.position.x += 1;
        }
    }

    pub fn put_string(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c);
        }
    }

    pub fn new_line(&mut self) {
        if self.position.y > self.writer.num_writeable_line() {
            self.scroll(1);
        } else {
            self.position.just_new_line();
        }
    }

    pub fn scroll(&mut self, dy: usize) {
        self.writer.scroll(16 * dy, self.background_color);
        self.position.y -= dy - 1;
        self.position.x = 0;
    }
}

impl core::fmt::Debug for &dyn AsciiWriter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        format_args!("AsciiWriter: {:?}", self as *const _).fmt(f)
    }
}
