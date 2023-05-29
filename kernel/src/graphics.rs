use core::fmt::{self};

use common::types::{GraphicsInfo, PixcelFormat};
use kernel_lib::{
    logger::CharWriter, AsciiWriter, Color, PixcelInfo, PixcelWritable, PixcelWriterTrait, Writer,
};
use once_cell::unsync::OnceCell;
use spin::Mutex;

pub struct Rgb;
pub struct Bgr;

pub trait MarkerColor {
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
            *self.frame_buffer_base.add(offset) = color.r;
            *self.frame_buffer_base.add(offset + 1) = color.g;
            *self.frame_buffer_base.add(offset + 2) = color.b;
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
            *self.frame_buffer_base.add(offset) = color.b;
            *self.frame_buffer_base.add(offset + 1) = color.g;
            *self.frame_buffer_base.add(offset + 2) = color.r;
        }
    }
}

pub struct PixcelWriterBuilder;

/// Safety: frame_buffer_base is write only.
unsafe impl<T: MarkerColor> Sync for PixcelWriter<T> {}
unsafe impl<T: MarkerColor> Send for PixcelWriter<T> {}

impl PixcelWriterBuilder {
    const fn cmp_max(a: usize, b: usize) -> usize {
        if a > b {
            a
        } else {
            b
        }
    }
    pub const PIXCEL_WRITER_NECESSARY_BUF_SIZE: usize = Self::cmp_max(
        core::mem::size_of::<PixcelWriter<Rgb>>(),
        core::mem::size_of::<PixcelWriter<Bgr>>(),
    );
    pub fn get_writer<'buf>(
        graphics_info: &GraphicsInfo,
        buf: &'buf mut [u8; Self::PIXCEL_WRITER_NECESSARY_BUF_SIZE],
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
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &pixcel_writer,
                        buf.as_mut_ptr() as *mut PixcelWriter<Rgb>,
                        1,
                    );
                };
                unsafe { &*(buf.as_ptr() as *const PixcelWriter<Rgb>) }
            }
            PixcelFormat::Bgr => {
                let pixcel_writer = PixcelWriter::<Bgr>::new_raw(
                    frame_buffer_base,
                    pixcels_per_scan_line,
                    graphics_info.horizontal_resolution(),
                    graphics_info.vertical_resolution(),
                );
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &pixcel_writer,
                        buf.as_mut_ptr() as *mut PixcelWriter<Bgr>,
                        1,
                    );
                };
                unsafe { &*(buf.as_ptr() as *const PixcelWriter<Bgr>) }
            }
        }
    }
}

impl PixcelWriterTrait for PixcelWriter<Rgb> {}
impl PixcelWriterTrait for PixcelWriter<Bgr> {}

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
static mut _WRITER_BUF: [u8; PixcelWriterBuilder::PIXCEL_WRITER_NECESSARY_BUF_SIZE] =
    [0; PixcelWriterBuilder::PIXCEL_WRITER_NECESSARY_BUF_SIZE];
pub static WRITER: CharWriter<N_CHAR_PER_LINE, N_WRITEABLE_LINE> =
    CharWriter(Mutex::new(OnceCell::new()));

pub fn init_graphics(graphics_info: GraphicsInfo) {
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
    writer.get_or_init(|| {
        let pixcel_writer =
            PixcelWriterBuilder::get_writer(&graphics_info, unsafe { &mut _WRITER_BUF });
        let writer = Writer::new(pixcel_writer);
        writer
    });
}

pub fn init_logger() {
    log::set_logger(&WRITER)
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
