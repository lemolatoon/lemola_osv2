#[repr(C)]
#[derive(Debug, Clone)]
pub struct KernelMainArg {
    pub graphics_frame_buffer: GraphicsFrameBuffer,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GraphicsFrameBuffer {
    frame_buffer_base: *mut u8,
    frame_buffer_size: usize,
}

impl GraphicsFrameBuffer {
    pub fn new(frame_buffer_base: *mut u8, frame_buffer_size: usize) -> Self {
        Self {
            frame_buffer_base,
            frame_buffer_size,
        }
    }

    #[inline]
    pub fn base(&self) -> *mut u8 {
        self.frame_buffer_base
    }

    #[inline]
    pub fn size(&self) -> usize {
        self.frame_buffer_size
    }
}
