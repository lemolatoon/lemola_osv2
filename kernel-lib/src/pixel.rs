extern crate alloc;
use alloc::boxed::Box;
use common::types::{GraphicsInfo, PixcelFormat};

use crate::Color;

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
pub trait RenderedPixel {
    fn pixel(&self, c: Color) -> [u8; 4];
}

impl RenderedPixel for Rgb {
    fn pixel(&self, c: Color) -> [u8; 4] {
        return [c.r, c.g, c.b, 0xff];
    }
}
impl RenderedPixel for Bgr {
    fn pixel(&self, c: Color) -> [u8; 4] {
        return [c.b, c.g, c.r, 0xff];
    }
}
pub fn new_rendering_handler(graphics_info: GraphicsInfo) -> Box<dyn RenderedPixel + Send + Sync> {
    match graphics_info.pixcel_format() {
        PixcelFormat::Rgb => Box::new(Rgb),
        PixcelFormat::Bgr => Box::new(Bgr),
    }
}
