use core::cmp;

extern crate alloc;
use alloc::boxed::Box;
use xhci::accessor::Mapper;

use crate::{
    alloc::alloc::{alloc_array_with_boundary, alloc_with_boundary},
    memory::PAGE_SIZE,
    xhci::command_ring::CommandRing,
};

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
        let interrupter_register_set_array = &registers.interrupter_register_set;
        const COMMAND_RING_BUF_SIZE: usize = 32;
        let command_ring = CommandRing::new(COMMAND_RING_BUF_SIZE);
        Self::register_command_ring(&mut registers, &command_ring);
        log::debug!("{:?}", &registers.operational.crcr.read_volatile());
        Self {
            registers,
            device_manager,
        }
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
        let mut device_manager = DeviceManager::new(max_slots);

        // Allocate scratchpad_buffers on first pointer of DeviceContextArray
        let hcsparams2 = registers.capability.hcsparams2.read_volatile();
        let max_scratchpad_buffers = hcsparams2.max_scratchpad_buffers();
        if max_scratchpad_buffers > 0 {
            const ALIGNMENT: usize = 64;
            let mut scratchpad_buffer_array = alloc_array_with_boundary::<*mut [u8; PAGE_SIZE]>(
                max_scratchpad_buffers as usize,
                ALIGNMENT,
                PAGE_SIZE,
            )
            .expect("scratchpad buffer array allocation failed");
            for scratchpad_buffer in scratchpad_buffer_array.iter_mut() {
                let mut allocated_array =
                    alloc_with_boundary::<[u8; PAGE_SIZE]>(PAGE_SIZE, PAGE_SIZE).unwrap();
                unsafe { allocated_array.as_mut_ptr().write([0; PAGE_SIZE]) };
                let allocated_array = unsafe { allocated_array.assume_init() };
                unsafe {
                    scratchpad_buffer
                        .as_mut_ptr()
                        .write(Box::leak(allocated_array) as *mut _)
                };
            }

            let scratchpad_buffer_array = unsafe { scratchpad_buffer_array.assume_init() };
            device_manager.set_scratchpad_buffer_array(scratchpad_buffer_array);
        }

        let ptr_head = device_manager.get_device_contexts_head_ptr();
        log::debug!("DeviceContextBaseAddressArrayPointer: {0:p}", ptr_head);
        operational.dcbaap.update_volatile(|dcbaap| {
            dcbaap.set(device_manager.get_device_contexts_head_ptr() as u64)
        });
        while operational.dcbaap.read_volatile().get() != ptr_head as u64 {}

        device_manager
    }

    fn register_command_ring(registers: &mut xhci::Registers<M>, ring: &CommandRing) {
        let command_ring_controller_register = &mut registers.operational.crcr.read_volatile();
        command_ring_controller_register.clear_ring_cycle_state();
        command_ring_controller_register.set_command_ring_pointer(ring.buffer_ptr() as u64);
    }
}
