extern crate alloc;
use core::fmt::{self};
use core::mem::ManuallyDrop;

use alloc::vec::Vec;
use alloc::vec;
use bit_field::BitField;
use common::types::{GraphicsInfo, PixcelFormat};
use kernel_lib::mutex::Mutex;
use kernel_lib::{
    logger::{CharWriter, DecoratedLog},
    AsciiWriter, Color, PixcelInfo, PixcelWritable, Writer,
};
use once_cell::unsync::OnceCell;

use crate::{serial_print, serial_println};

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

#[derive(Debug, Clone)]
pub struct PixcelWriter<T: MarkerColor> {
    frame_buffer_base: *mut u8,
    pixcels_per_scan_line: usize,
    horizontal_resolution: usize,
    vertical_resolution: usize,
    layers: Vec<Vec<u32>>,
    _pixcel_format: T,
}

impl PixcelWriter<Rgb> {
    pub fn new_raw(
        frame_buffer_base: *mut u8,
        pixcels_per_scan_line: usize,
        horizontal_resolution: usize,
        vertical_resolution: usize,
    ) -> Self {
        serial_println!("bytes: {}", horizontal_resolution * pixcels_per_scan_line * 4);
        let layers = vec![vec![u32::MAX; horizontal_resolution * pixcels_per_scan_line]; 3];
        Self {
            frame_buffer_base,
            pixcels_per_scan_line,
            horizontal_resolution,
            vertical_resolution,
            layers,
            _pixcel_format: Rgb,
        }
    }

    pub fn write_pixcel_at_offset_at(&self, offset: usize, color: Color, layer_id: usize) {

        let mut pixcel: u32 = 0;
        pixcel.set_bits(0..8, color.r as u32);
        pixcel.set_bits(8..16, color.g as u32);
        pixcel.set_bits(16..24, color.b as u32);
        // TODO: make this method &mut self
        unsafe { (&self.layers[layer_id][offset] as *const u32 as *mut u32).write_volatile(pixcel) };
    }
}

impl PixcelWritable for PixcelWriter<Rgb> {
    fn write_at(&self, x: usize, y: usize, color: Color, layer_id: usize) {
        let offset = self.get_offset(x, y);
        self.write_pixcel_at_offset_at(offset, color, layer_id);
    }

    fn flush(&self) {
        self.flush_inner();
    }
}

impl PixcelWritable for PixcelWriter<Bgr> {
    fn write_at(&self, x: usize, y: usize, color: Color, layer_id: usize) {
        let offset = self.get_offset(x, y);
        self.write_pixcel_at_offset_at(offset, color, layer_id);
    }

    fn flush(&self) {
        self.flush_inner();
    }
}

impl PixcelWriter<Bgr> {
    pub fn new_raw(
        frame_buffer_base: *mut u8,
        pixcels_per_scan_line: usize,
        horizontal_resolution: usize,
        vertical_resolution: usize,
    ) -> Self {
        serial_println!("bytes: {}", horizontal_resolution * pixcels_per_scan_line * 4);
        let layers = vec![vec![u32::MAX; horizontal_resolution * pixcels_per_scan_line ]; 3];
        Self {
            frame_buffer_base,
            pixcels_per_scan_line,
            horizontal_resolution,
            vertical_resolution,
            layers,
            _pixcel_format: Bgr,
        }
    }

    pub fn write_pixcel_at_offset_at(&self, offset: usize, color: Color, layer_id: usize) {
        let mut pixcel: u32 = 0;
        pixcel.set_bits(0..8, color.b as u32);
        pixcel.set_bits(8..16, color.g as u32);
        pixcel.set_bits(16..24, color.r as u32);
        // TODO: make this method &mut self
        unsafe { (&self.layers[layer_id][offset] as *const u32 as *mut u32).write_volatile(pixcel) };
    }
}

pub struct PixcelWriterBuilder;

pub union PixcelWriterUnion {
    rgb: ManuallyDrop<PixcelWriter<Rgb>>,
    bgr: ManuallyDrop<PixcelWriter<Bgr>>,
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
                buf.rgb = ManuallyDrop::new(pixcel_writer);
                // Safety: buf.rgb is initialized at previous line.
                unsafe { core::mem::transmute::<_, &'static PixcelWriter<Rgb>>(&buf.rgb) }
            }
            PixcelFormat::Bgr => {
                let pixcel_writer = PixcelWriter::<Bgr>::new_raw(
                    frame_buffer_base,
                    pixcels_per_scan_line,
                    graphics_info.horizontal_resolution(),
                    graphics_info.vertical_resolution(),
                );
                buf.bgr = ManuallyDrop::new(pixcel_writer);
                // Safety: buf.bgr is initialized at previous line.
                unsafe { core::mem::transmute::<_, &'static PixcelWriter<Bgr>>(&buf.bgr) }
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

    fn flush_inner(&self) {
        const BUF_LEN: usize = 10000 * 10000;
        let mut applied_layers = [0u32; BUF_LEN];
        for layer in self.layers.iter() {
            for (i, pixcel) in layer.iter().enumerate() {
                if *pixcel == u32::MAX {
                    applied_layers[i] = *pixcel;
                }
            }
        }
        unsafe {
            core::ptr::copy_nonoverlapping::<u32>(applied_layers.as_ptr(), self.frame_buffer_base as *mut u32, self.layers[0].len());
        }
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
    let writer = kernel_lib::lock!(WRITER.0);
    let pixcel_writer =
        PixcelWriterBuilder::get_writer(&graphics_info, unsafe { &mut UNSAFE_WRITER_BUF });
    writer.get_or_init(|| {
        let writer = Writer::new(pixcel_writer);
        writer
    });
    pixcel_writer
}

pub struct SerialAndVgaCharWriter;

impl SerialAndVgaCharWriter {
    pub const fn new() -> Self {
        Self {}
    }
}
static SERIAL_VGA_WRITER: SerialAndVgaCharWriter = SerialAndVgaCharWriter::new();
pub struct InstantWriter<F: Fn(&str)> {
    f: F,
}
impl<F: Fn(&str)> InstantWriter<F> {
    pub fn new(f: F) -> Self {
        Self { f }
    }
}
impl<F: Fn(&str)> fmt::Write for InstantWriter<F> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        (self.f)(s);
        Ok(())
    }
}
impl log::Log for SerialAndVgaCharWriter {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        WRITER.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            if record.level() <= log::Level::Info {
                let mut serial_vga_writer = InstantWriter::new(|s| {
                    serial_print!("{}", s);
                    crate::print!("{}", s)
                });
                DecoratedLog::write(
                    &mut serial_vga_writer,
                    record.level(),
                    record.args(),
                    record.file().unwrap_or("<unknown>"),
                    record.line().unwrap_or(0),
                )
                .unwrap();
            } else {
                // let mut serial_writer = InstantWriter::new(|s| serial_print!("{}", s));
                let mut serial_writer = InstantWriter::new(|s| {
                    serial_print!("{}", s);
                });
                DecoratedLog::write(
                    &mut serial_writer,
                    record.level(),
                    record.args(),
                    record.file().unwrap_or("<unknown>"),
                    record.line().unwrap_or(0),
                )
                .unwrap();
            }
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

pub async fn render() {
    loop {
        if let Some(pixcel_writer) = get_pixcel_writer() {
            pixcel_writer.flush();
        }
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::graphics::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! print_and_flush {
    ($($arg:tt)*) => {{
        $crate::graphics::_print_and_flush(format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        kernel_lib::lock!(WRITER.0)
            .get_mut()
            .expect("WRITER NOT INITIALIZED")
            .write_fmt(args)
            .unwrap();
    });
}

#[doc(hidden)]
pub fn _print_and_flush(args: fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut writer = kernel_lib::lock!(WRITER.0);
        let writer = writer.get_mut().expect("WRITER NOT INITIALIZED");
        writer.write_fmt(args).unwrap();
        writer.flush();
    });
}
