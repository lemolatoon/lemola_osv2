use core::cmp;

extern crate alloc;
use alloc::{boxed::Box, vec::Vec};
use xhci::accessor::Mapper;

use super::device_manager::DeviceManager;

#[derive(Debug)]
pub struct XhciController<M>
where
    M: Mapper + Clone,
{
    registers: xhci::Registers<M>,
    device_manager: DeviceManager,
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
        let mut registers = xhci::Registers::new(xhci_memory_mapped_io_base_address, mapper);
        Self::reset_controller(&mut registers);
        let device_manager = Self::configure_device_context(&mut registers);
        log::debug!("device_manager allocated: {:?}", &device_manager);
        let controller = Self {
            registers,
            device_manager,
        };
        controller
    }

    fn reset_controller(registers: &mut xhci::Registers<M>) {
        let operational = &registers.operational;
        assert!(
            operational.usbsts.read_volatile().hc_halted(),
            "xHC is not halted."
        );
        log::debug!("xHC is halted.");

        operational
            .usbcmd
            .read_volatile()
            .set_host_controller_reset(); // write 1
        log::debug!("write 1 to USBCMD.HCRST, set_host_controller_reset");

        // wait for the reset to complete
        while operational.usbcmd.read_volatile().host_controller_reset() {}
        log::debug!("xHC is now reset.");
        while operational.usbsts.read_volatile().controller_not_ready() {}
        log::debug!("xHC is now ready.");
    }

    fn configure_device_context(registers: &mut xhci::Registers<M>) -> DeviceManager {
        let capability = &registers.capability;
        let operational = &mut registers.operational;
        let max_slots = capability
            .hcsparams1
            .read_volatile()
            .number_of_device_slots();
        log::debug!("number_of_device_slots: {}", max_slots);
        const MAX_SLOTS: u8 = 10;
        let max_device_slots_enabled = cmp::min(max_slots, MAX_SLOTS);
        operational.config.update_volatile(|config| {
            config.set_max_device_slots_enabled(max_device_slots_enabled);
        });
        log::debug!("max_device_slots_enabled: {}", max_device_slots_enabled);
        let device_manager = DeviceManager::new(max_slots);

        device_manager
    }
}
