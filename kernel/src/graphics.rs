use core::{
    cell::Cell,
    fmt::{self},
};

use common::types::{GraphicsInfo, PixcelFormat};
use kernel_lib::{
    logger::{CharWriter, DecoratedLog},
    AsciiWriter, Color, PixcelInfo, PixcelWritable, Writer,
};
use once_cell::unsync::OnceCell;
use spin::Mutex;

use crate::serial_print;

#[derive(Debug, Clone, Copy)]
pub struct Rgb;
#[derive(Debug, Clone, Copy)]
pub struct Bgr;

pub trait MarkerColor: Copy {
    fn pixcel_format() -> PixcelFormat;
}
impl MarkerColor for Rgb {
    fn pixcel_format() -> PixcelFormat {
        PixcelFormat::Rgb
    }
}
impl MarkerColor for Bgr {
    fn pixcel_format() -> PixcelFormat {
        PixcelFormat::Bgr
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PixcelWriter<T: MarkerColor> {
    frame_buffer_base: *mut u8,
    pixcels_per_scan_line: usize,
    horizontal_resolution: usize,
    vertical_resolution: usize,
    _pixcel_format: T,
}

impl PixcelWriter<Rgb> {
    pub fn new_raw(
        frame_buffer_base: *mut u8,
        pixcels_per_scan_line: usize,
        horizontal_resolution: usize,
        vertical_resolution: usize,
    ) -> Self {
        Self {
            frame_buffer_base,
            pixcels_per_scan_line,
            horizontal_resolution,
            vertical_resolution,
            _pixcel_format: Rgb,
        }
    }

    pub fn write_pixcel_at_offset(&self, offset: usize, color: Color) {
        let offset = offset * 4;
        unsafe {
            self.frame_buffer_base.add(offset).write_volatile(color.r);
            self.frame_buffer_base
                .add(offset + 1)
                .write_volatile(color.g);
            self.frame_buffer_base
                .add(offset + 2)
                .write_volatile(color.b);
        }
    }
}

impl PixcelWritable for PixcelWriter<Rgb> {
    fn write(&self, x: usize, y: usize, color: Color) {
        let offset = self.get_offset(x, y);
        self.write_pixcel_at_offset(offset, color);
    }
}

impl PixcelWritable for PixcelWriter<Bgr> {
    fn write(&self, x: usize, y: usize, color: Color) {
        let offset = self.get_offset(x, y);
        self.write_pixcel_at_offset(offset, color);
    }
}

impl PixcelWriter<Bgr> {
    pub fn new_raw(
        frame_buffer_base: *mut u8,
        pixcels_per_scan_line: usize,
        horizontal_resolution: usize,
        vertical_resolution: usize,
    ) -> Self {
        Self {
            frame_buffer_base,
            pixcels_per_scan_line,
            horizontal_resolution,
            vertical_resolution,
            _pixcel_format: Bgr,
        }
    }

    pub fn write_pixcel_at_offset(&self, offset: usize, color: Color) {
        let offset = offset * 4;
        unsafe {
            self.frame_buffer_base.add(offset).write_volatile(color.b);
            self.frame_buffer_base
                .add(offset + 1)
                .write_volatile(color.g);
            self.frame_buffer_base
                .add(offset + 2)
                .write_volatile(color.r);
        }
    }
}

pub struct PixcelWriterBuilder;

pub union PixcelWriterUnion {
    rgb: PixcelWriter<Rgb>,
    bgr: PixcelWriter<Bgr>,
    none: (),
}

/// Safety: frame_buffer_base is write only.
unsafe impl<T: MarkerColor> Sync for PixcelWriter<T> {}
unsafe impl<T: MarkerColor> Send for PixcelWriter<T> {}

impl PixcelWriterBuilder {
    pub fn get_writer<'buf>(
        graphics_info: &GraphicsInfo,
        buf: &'buf mut PixcelWriterUnion,
    ) -> &'buf (dyn AsciiWriter + Send + Sync) {
        let frame_buffer_base = graphics_info.base();
        let pixcels_per_scan_line = graphics_info.stride();
        let pixcel_format = graphics_info.pixcel_format();
        match pixcel_format {
            PixcelFormat::Rgb => {
                let pixcel_writer = PixcelWriter::<Rgb>::new_raw(
                    frame_buffer_base,
                    pixcels_per_scan_line,
                    graphics_info.horizontal_resolution(),
                    graphics_info.vertical_resolution(),
                );
                buf.rgb = pixcel_writer;
                // Safety: buf.rgb is initialized at previous line.
                unsafe { &buf.rgb }
            }
            PixcelFormat::Bgr => {
                let pixcel_writer = PixcelWriter::<Bgr>::new_raw(
                    frame_buffer_base,
                    pixcels_per_scan_line,
                    graphics_info.horizontal_resolution(),
                    graphics_info.vertical_resolution(),
                );
                buf.bgr = pixcel_writer;
                // Safety: buf.bgr is initialized at previous line.
                unsafe { &buf.bgr }
            }
        }
    }
}

impl<T: MarkerColor> PixcelInfo for PixcelWriter<T> {
    fn get_pixcel_format(&self) -> PixcelFormat {
        T::pixcel_format()
    }

    fn get_num_pixcels(&self) -> usize {
        self.pixcels_per_scan_line * self.vertical_resolution
    }

    fn horizontal_resolution(&self) -> usize {
        self.horizontal_resolution
    }

    fn vertical_resolution(&self) -> usize {
        self.vertical_resolution
    }

    fn pixcels_per_scan_line(&self) -> usize {
        self.pixcels_per_scan_line
    }
}

impl<T: MarkerColor> PixcelWriter<T>
where
    Self: PixcelWritable,
{
    fn get_offset(&self, x: usize, y: usize) -> usize {
        y * self.pixcels_per_scan_line + x
    }
}

pub const N_CHAR_PER_LINE: usize = 80;
pub const N_WRITEABLE_LINE: usize = 25;
static mut UNSAFE_WRITER_BUF: PixcelWriterUnion = PixcelWriterUnion { none: () };
pub static WRITER: CharWriter<N_CHAR_PER_LINE, N_WRITEABLE_LINE> =
    CharWriter(Mutex::new(OnceCell::new()));

pub fn get_pixcel_writer() -> Option<&'static (dyn AsciiWriter + Send + Sync)> {
    Some(WRITER.lock().get()?.pixcel_writer())
}

/// init graphics and return pixcel_writer
pub fn init_graphics(graphics_info: GraphicsInfo) -> &'static (dyn AsciiWriter + Send + Sync) {
    // clear
    for y in 0..graphics_info.vertical_resolution() {
        for x in 0..graphics_info.horizontal_resolution() {
            let offset = y * graphics_info.stride() + x;
            unsafe {
                *graphics_info.base().add(offset * 4) = 0;
                *graphics_info.base().add(offset * 4 + 1) = 0;
                *graphics_info.base().add(offset * 4 + 2) = 0;
            }
        }
    }
    let writer = WRITER.0.lock();
    let pixcel_writer =
        PixcelWriterBuilder::get_writer(&graphics_info, unsafe { &mut UNSAFE_WRITER_BUF });
    writer.get_or_init(|| {
        let writer = Writer::new(pixcel_writer);
        writer
    });
    pixcel_writer
}

pub struct SerialAndVgaCharWriterInner;
pub struct SerialAndVgaCharWriter {
    inner: Cell<SerialAndVgaCharWriterInner>,
}
impl SerialAndVgaCharWriter {
    pub const fn new() -> Self {
        Self {
            inner: Cell::new(SerialAndVgaCharWriterInner),
        }
    }
}
// Safety: SerialAndVgaCharWriterInner is not actually mutable. It is just call outer `println!` and `serial_println!`.
//         and in `println!`, `serial_println!` static variables are wrapped by `Mutex`
unsafe impl Sync for SerialAndVgaCharWriter {}
unsafe impl Send for SerialAndVgaCharWriter {}
static SERIAL_VGA_WRITER: SerialAndVgaCharWriter = SerialAndVgaCharWriter::new();
impl fmt::Write for SerialAndVgaCharWriterInner {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        use crate::print;
        // print!("{}", s);
        serial_print!("{}", s);
        Ok(())
    }
}
impl log::Log for SerialAndVgaCharWriter {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        WRITER.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let writer = self.inner.as_ptr();
            DecoratedLog::write(
                unsafe { writer.as_mut().unwrap_unchecked() },
                record.level(),
                record.args(),
                record.file().unwrap_or("<unknown>"),
                record.line().unwrap_or(0),
            )
            .unwrap();
        }
    }

    fn flush(&self) {}
}

pub fn init_logger() {
    log::set_logger(&SERIAL_VGA_WRITER)
        .map(|()| {
            log::set_max_level(log::LevelFilter::Trace);
            log::info!("logger initialized");
        })
        .unwrap();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::graphics::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    WRITER
        .0
        .lock()
        .get_mut()
        .expect("WRITER NOT INITIALIZED")
        .write_fmt(args)
        .unwrap();
}
