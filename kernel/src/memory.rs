#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryMapper;

impl MemoryMapper {
    pub fn new() -> Self {
        Self {}
    }
}

impl xhci::accessor::Mapper for MemoryMapper {
    unsafe fn map(&mut self, phys_start: usize, _bytes: usize) -> core::num::NonZeroUsize {
        // currently virtual address is same as physical address
        return core::num::NonZeroUsize::new_unchecked(phys_start);
    }

    fn unmap(&mut self, _virt_start: usize, _bytes: usize) {
        // currently virtual address is same as physical address
        return;
    }
}
