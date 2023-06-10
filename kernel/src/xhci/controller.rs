use xhci::accessor::Mapper;

#[derive(Debug)]
pub struct XhciController<M>
where
    M: Mapper + Clone,
{
    registers: xhci::Registers<M>,
}

impl<M> XhciController<M>
where
    M: Mapper + Clone,
{
    /// # Safety
    /// The caller must ensure that the xHCI registers are accessed only through this struct.
    ///
    /// # Panics
    /// This method panics if `mmio_base` is not aligned correctly.
    ///
    pub unsafe fn new(xhci_memory_mapped_io_base_address: usize, mapper: M) -> Self {
        let xhc = xhci::Registers::new(xhci_memory_mapped_io_base_address, mapper);
        Self { registers: xhc }
    }
}
