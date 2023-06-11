#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryMapper;

impl MemoryMapper {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MemoryMapper {
    fn default() -> Self {
        Self::new()
    }
}

pub const PAGE_SIZE: usize = 4096;

impl xhci::accessor::Mapper for MemoryMapper {
    unsafe fn map(&mut self, phys_start: usize, _bytes: usize) -> core::num::NonZeroUsize {
        // currently virtual address is same as physical address
        core::num::NonZeroUsize::new_unchecked(phys_start)
    }

    fn unmap(&mut self, _virt_start: usize, _bytes: usize) {
        // currently virtual address is same as physical address
    }
}
