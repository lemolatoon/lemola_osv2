#[repr(C)]
#[derive(Debug, Clone)]
pub struct KernelMainArg {
    pub graphics_info: GraphicsInfo,
}

pub type KernelMain = extern "C" fn(arg: *const KernelMainArg) -> !;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum PixcelFormat {
    Rgb,
    Bgr,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GraphicsInfo {
    horizontal_resolution: usize,
    vertical_resolution: usize,
    pixels_per_scan_line: usize,
    frame_buffer_base: *mut u8,
    pixcel_format: PixcelFormat,
}

impl GraphicsInfo {
    pub fn new(
        horizontal_resolution: usize,
        vertical_resolution: usize,
        pixels_per_scan_line: usize,
        frame_buffer_base: *mut u8,
        pixcel_format: PixcelFormat,
    ) -> Self {
        Self {
            horizontal_resolution,
            vertical_resolution,
            pixels_per_scan_line,
            frame_buffer_base,
            pixcel_format,
        }
    }

    pub fn base(&self) -> *mut u8 {
        self.frame_buffer_base
    }

    pub fn size(&self) -> usize {
        self.pixels_per_scan_line * self.vertical_resolution
    }

    pub fn stride(&self) -> usize {
        self.pixels_per_scan_line
    }

    pub fn horizontal_resolution(&self) -> usize {
        self.horizontal_resolution
    }

    pub fn vertical_resolution(&self) -> usize {
        self.vertical_resolution
    }

    pub fn pixcel_format(&self) -> PixcelFormat {
        self.pixcel_format
    }
}
