pub use uefi_raw::table::boot::MemoryDescriptor;
pub use uefi_raw::table::boot::MemoryType;

#[repr(C)]
#[derive(Debug, Clone)]
pub struct KernelMainArg {
    pub graphics_info: GraphicsInfo,
    pub memory_map_entry: *const MemMapEntry,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct MemMapEntry {
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct MemMapIter<'a> {
    index: usize,
    size: usize,
    current: *const MemoryDescriptor,
    _lifetime: core::marker::PhantomData<&'a MemoryDescriptor>,
}

impl MemMapEntry {
    /// # Safety
    /// The caller must ensure that entry has enough size to store the given iterator.
    /// And header(=MemMapEntry) can be placed at the beginning of the entry.
    pub unsafe fn new_inplace<'a, 'b>(
        entry: &'a mut [u8],
        size: u64,
        iter: impl ExactSizeIterator<Item = &'b MemoryDescriptor> + Clone,
    ) {
        let header = entry.as_mut_ptr() as *mut MemMapEntry;
        (*header).size = size;
        let desc_head = unsafe { (header).add(1) } as *mut MemoryDescriptor;
        for (i, desc) in iter.enumerate() {
            unsafe { *desc_head.add(i) = *desc };
        }
    }

    /// # Safety
    /// The caller must ensure that an array(len = self.size) of MemoryDescriptor are
    /// placed after this struct.
    pub unsafe fn into_iter(&self) -> MemMapIter<'_> {
        let current = unsafe { (self as *const MemMapEntry).add(1) } as *const MemoryDescriptor;
        MemMapIter {
            index: 0,
            current,
            size: self.size as usize,
            _lifetime: core::marker::PhantomData,
        }
    }
}

impl<'a> Iterator for MemMapIter<'a> {
    type Item = MemoryDescriptor;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.size {
            return None;
        }
        let current = unsafe { self.current.add(self.index) };
        self.index += 1;
        Some(unsafe { *current })
    }
}

impl<'a> ExactSizeIterator for MemMapIter<'a> {
    fn len(&self) -> usize {
        self.size
    }
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
    pub const fn uninitialized() -> Self {
        Self {
            horizontal_resolution: 0,
            vertical_resolution: 0,
            pixels_per_scan_line: 0,
            frame_buffer_base: core::ptr::null_mut(),
            pixcel_format: PixcelFormat::Rgb,
        }
    }

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
