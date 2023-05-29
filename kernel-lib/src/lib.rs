#![cfg_attr(not(feature = "std"), no_std)]

pub mod logger;
use core::fmt;

use common::types::PixcelFormat;
use gen_font::gen_font;

gen_font!();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn black() -> Self {
        Self { r: 0, g: 0, b: 0 }
    }

    pub const fn white() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
        }
    }
}
pub trait PixcelInfo {
    fn get_pixcel_format(&self) -> PixcelFormat;
    fn get_num_pixcels(&self) -> usize;
    fn horizontal_resolution(&self) -> usize;
    fn vertical_resolution(&self) -> usize;
    fn pixcels_per_scan_line(&self) -> usize;
}
pub trait PixcelWritable {
    fn write(&self, x: usize, y: usize, color: Color);
}

pub trait PixcelWriterTrait: PixcelWritable + PixcelInfo + AsciiWriter {}

pub trait AsciiWriter: PixcelWritable + PixcelInfo {
    fn write_ascii(&self, x: usize, y: usize, c: char, bg_color: Color, fg_color: Color) {
        let Some(font) = FONT.get(c as usize) else {
            return;
        };
        for (dy, font) in font.iter().enumerate() {
            for dx in 0..8 {
                if font & (1 << (7 - dx)) != 0 {
                    self.write(x + dx, y + dy, fg_color);
                } else {
                    self.write(x + dx, y + dy, bg_color);
                }
            }
        }
    }

    fn write_string(&self, x: usize, y: usize, s: &str, color: Color) {
        for (idx, c) in s.chars().enumerate() {
            self.write_ascii(x + 8 * idx, y, c, Color::black(), color);
        }
    }
}
#[cfg(not(test))]
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
pub struct Writer<'a, const N_ROW: usize, const N_COLUMN: usize> {
    writer: &'a (dyn AsciiWriter + Send + Sync),
    position: CursorPosition,
    background_color: Color,
    foreground_color: Color,
    buffer: [[char; N_COLUMN]; N_ROW],
}

impl<const N_ROW: usize, const N_COLUMN: usize> fmt::Write for Writer<'_, N_ROW, N_COLUMN> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.put_string(s);
        Ok(())
    }
}

impl<'a, const N_ROW: usize, const N_COLUMN: usize> Writer<'a, N_ROW, N_COLUMN> {
    pub fn new(writer: &'a (dyn AsciiWriter + Send + Sync)) -> Self {
        Self {
            writer,
            position: CursorPosition { x: 0, y: 0 },
            background_color: Color::black(),
            foreground_color: Color::white(),
            buffer: [[' '; N_COLUMN]; N_ROW],
        }
    }

    pub fn store(&mut self, c: char) {
        self.buffer[self.position.y][self.position.x] = c;
    }

    pub fn put_char(&mut self, c: char) {
        if c == '\n' {
            self.new_line();
        } else if self.position.x < N_COLUMN && self.position.y < N_ROW {
            self.store(c);
            self.position.x += 1;
        } else {
            self.new_line();
            self.store(c);
            self.position.x += 1;
        }
    }

    pub fn put_string(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c);
        }
    }

    pub fn new_line(&mut self) {
        if self.position.y < N_ROW {
            self.position.just_new_line();
        } else {
            self.scroll(1);
        }
        self.flush();
    }

    pub fn scroll(&mut self, dy: usize) {
        for y in 0..(N_ROW - dy) {
            self.buffer[y] = self.buffer[y + dy];
        }
        for y in (N_ROW - dy)..N_ROW {
            self.buffer[y] = [' '; N_COLUMN];
        }
        self.position.y -= dy;
        self.position.x = 0;
    }

    pub fn flush(&mut self) {
        for y in 0..N_ROW {
            for x in 0..N_COLUMN {
                self.writer.write_ascii(
                    x * 8,
                    y * 16,
                    self.buffer[y][x],
                    self.background_color,
                    self.foreground_color,
                );
            }
        }
    }
}

impl core::fmt::Debug for &(dyn AsciiWriter + Send + Sync) {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        format_args!("AsciiWriter: {:?}", self as *const _).fmt(f)
    }
}

#[cfg(test)]
mod test {
    use core::cell::RefCell;

    use super::*;
    const N_ROW: usize = 10;
    const N_COLUMN: usize = 10;
    const N_PIXCELS_ROW: usize = 16 * N_ROW;
    const N_PIXCELS_COLUMN: usize = 8 * N_COLUMN;
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MockWriter {
        pub buffer: RefCell<[[char; N_COLUMN]; N_ROW]>,
    }
    unsafe impl Sync for MockWriter {} // for test only
    impl MockWriter {
        pub const fn new() -> Self {
            Self {
                buffer: RefCell::new([[' '; N_ROW]; N_COLUMN]),
            }
        }
    }
    impl PixcelWritable for MockWriter {
        fn write(&self, _x: usize, _y: usize, _color: Color) {
            panic!("should not be called")
        }
    }

    impl PixcelInfo for MockWriter {
        fn get_pixcel_format(&self) -> PixcelFormat {
            PixcelFormat::Bgr
        }
        fn get_num_pixcels(&self) -> usize {
            self.pixcels_per_scan_line() * self.vertical_resolution()
        }
        fn horizontal_resolution(&self) -> usize {
            N_PIXCELS_COLUMN
        }
        fn vertical_resolution(&self) -> usize {
            N_PIXCELS_ROW
        }
        fn pixcels_per_scan_line(&self) -> usize {
            self.horizontal_resolution() / 4
        }
    }

    impl AsciiWriter for MockWriter {
        fn write_ascii(&self, x: usize, y: usize, c: char, _bg_color: Color, _fg_color: Color) {
            let mut buffer = self.buffer.borrow_mut();
            buffer[y / 16][x / 8] = c;
        }
    }

    fn downcast(any: &dyn AsciiWriter) -> &MockWriter {
        unsafe { &*(any as *const dyn AsciiWriter as *const MockWriter) }
    }

    #[test]
    fn test_writer() {
        let writer = MockWriter::new();
        let mut writer = Writer::<N_ROW, N_COLUMN>::new(&writer);
        // 10 * 10
        let string = "abcdefghij";
        assert_eq!(string.len(), 10);
        writer.put_string(string);
        let mock_writer = downcast(writer.writer);
        assert_eq!(mock_writer.buffer.borrow()[0], [' '; 10]);
        writer.put_char('\n');
        assert_eq!(
            mock_writer.buffer.borrow()[0],
            &string.chars().collect::<Vec<_>>()[..]
        );
        for i in 0..9 {
            writer.put_string(format!("{}\n", i).as_str());
        }
        let buffers = mock_writer.buffer.borrow();
        for (idx, buffer) in (&(buffers)[1..]).into_iter().enumerate() {
            let mut expected = [' '; 10];
            expected[0] = format!("{}", idx).chars().next().unwrap();
            assert_eq!(*buffer, expected);
        }
        drop(buffers);
        writer.put_char('\n');
        let buffers = mock_writer.buffer.borrow();
        for (idx, buffer) in (&(buffers)[0..9]).into_iter().enumerate() {
            let mut expected = [' '; 10];
            expected[0] = format!("{}", idx).chars().next().unwrap();
            assert_eq!(*buffer, expected);
        }
        assert_eq!(buffers[9], [' '; 10]);
    }

    #[test]
    fn test_writer2() {
        let writer = MockWriter::new();
        let mut writer = Writer::<N_ROW, N_COLUMN>::new(&writer);
        // 10 * 10
        let mock_writer = downcast(writer.writer);
        for i in 0..200usize {
            writer.put_char((('a' as u8) + (i % 26) as u8) as char);
            writer.put_char('\n')
        }

        for idx in 0..10 {
            let mut expected = [' '; 10];
            expected[0] = (('a' as u8) + ((190 + idx) % 26) as u8) as char;
            assert_eq!(mock_writer.buffer.borrow()[idx], expected);
        }
    }
}
