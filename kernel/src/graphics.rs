use common::types::{GraphicsInfo, PixcelFormat};

use crate::font::AsciiWriter;

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

#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
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
            *self.frame_buffer_base.add(offset + 0) = color.r;
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
            *self.frame_buffer_base.add(offset + 0) = color.b;
            *self.frame_buffer_base.add(offset + 1) = color.g;
            *self.frame_buffer_base.add(offset + 2) = color.r;
        }
    }
}

pub struct PixcelWriterBuilder;

impl PixcelWriterBuilder {
    pub const PIXCEL_WRITER_NECESSARY_BUF_SIZE: usize = core::cmp::max(
        core::mem::size_of::<PixcelWriter<Rgb>>(),
        core::mem::size_of::<PixcelWriter<Bgr>>(),
    );
    pub fn get_writer<'buf>(
        graphics_info: &GraphicsInfo,
        buf: &'buf mut [u8; Self::PIXCEL_WRITER_NECESSARY_BUF_SIZE],
    ) -> &'buf dyn PixcelWriterTrait {
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
                let pixcel_writer = unsafe { &*(buf.as_ptr() as *const PixcelWriter<Rgb>) };
                pixcel_writer
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
                let pixcel_writer = unsafe { &*(buf.as_ptr() as *const PixcelWriter<Bgr>) };
                pixcel_writer
            }
        }
    }
}

impl<T: MarkerColor> PixcelWriter<T> {}
pub trait PixcelWritable {
    fn write(&self, x: usize, y: usize, color: Color);
}

pub trait PixcelInfo {
    fn get_pixcel_format(&self) -> PixcelFormat;
    fn get_num_pixcels(&self) -> usize;
    fn horizontal_resolution(&self) -> usize;
    fn vertical_resolution(&self) -> usize;
}

pub trait PixcelWriterTrait: PixcelWritable + PixcelInfo + AsciiWriter {}
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
}

impl<T: MarkerColor> PixcelWriter<T> {
    fn get_offset(&self, x: usize, y: usize) -> usize {
        y * self.pixcels_per_scan_line + x
    }
}
