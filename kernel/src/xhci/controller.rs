use core::cmp;

extern crate alloc;
use alloc::boxed::Box;
use xhci::{
    accessor::{array, Mapper},
    registers::PortRegisterSet,
    ring::trb::{self, event},
};

use crate::{
    alloc::alloc::{alloc_array_with_boundary, alloc_with_boundary},
    memory::PAGE_SIZE,
    xhci::{command_ring::CommandRing, event_ring::EventRing},
};

use super::{
    device_manager::DeviceManager,
    port::{PortConfigPhase, PortConfigureState},
};

#[derive(Debug)]
pub struct XhciController<M>
where
    M: Mapper + Clone,
{
    registers: xhci::Registers<M>,
    device_manager: DeviceManager,
    command_ring: CommandRing,
    event_ring: EventRing,
    number_of_ports: u8,
    port_configure_state: PortConfigureState,
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
        let number_of_ports = registers
            .capability
            .hcsparams1
            .read_volatile()
            .number_of_ports();
        // TODO: この操作の意味を調べる
        registers.operational.usbcmd.update_volatile(|usbcmd| {
            usbcmd.clear_interrupter_enable();
            usbcmd.clear_host_system_error_enable();
            usbcmd.clear_enable_wrap_event();
        });
        Self::reset_controller(&mut registers);
        log::debug!("[XHCI] reset controller");
        let device_manager = Self::configure_device_context(&mut registers);
        log::debug!("[XHCI] configure device context");

        const COMMAND_RING_BUF_SIZE: usize = 32;
        let command_ring = CommandRing::new(COMMAND_RING_BUF_SIZE);
        Self::register_command_ring(&mut registers, &command_ring);
        log::debug!("[XHCI] register command ring");

        const EVENT_RING_BUF_SIZE: u16 = 32;
        let mut primary_interrupter = registers.interrupter_register_set.interrupter_mut(0);
        let event_ring = EventRing::new(EVENT_RING_BUF_SIZE, &mut primary_interrupter);
        log::debug!("[XHCI] initialize event ring");

        let port_configure_state = PortConfigureState::new();

        Self {
            registers,
            device_manager,
            command_ring,
            event_ring,
            number_of_ports,
            port_configure_state,
        }
    }

    pub fn run(&mut self) {
        let operational = &mut self.registers.operational;
        operational.usbcmd.update_volatile(|usbcmd| {
            usbcmd.set_run_stop();
        });

        while operational.usbsts.read_volatile().hc_halted() {}
        log::debug!("[XHCI] xhc controller starts running!!");
    }

    pub fn process_event(&mut self) {
        let primary_interrupter = self.registers.interrupter_register_set.interrupter_mut(0);
        let event_ring_trb = unsafe {
            (primary_interrupter
                .erdp
                .read_volatile()
                .event_ring_dequeue_pointer() as *const trb::Link)
                .read_volatile()
        };
        if event_ring_trb.cycle_bit() != self.event_ring.cycle_bit() {
            // EventRing does not have front
            return;
        }

        log::debug!("[XHCI] EventRing received trb: {:?}", event_ring_trb);
        let mut primary_interrupter = self.registers.interrupter_register_set.interrupter_mut(0);
        self.event_ring.pop(&mut primary_interrupter);
    }

    pub fn port_register_sets(&mut self) -> &mut array::ReadWrite<PortRegisterSet, M> {
        &mut self.registers.port_register_set
    }

    pub fn number_of_ports(&self) -> u8 {
        self.number_of_ports
    }

    pub fn configure_port_at(&mut self, port_index: usize) {
        if !self.port_configure_state.is_connected(port_index) {
            self.reset_port_at(port_index);
        }
    }

    pub fn reset_port_at(&mut self, port_index: usize) {
        let is_connected = self.is_port_connected_at(port_index);
        if !is_connected {
            return;
        }

        match self.port_configure_state.addressing_port_index {
            Some(_) => {
                // This branch is fallen, when another port is currently trying to be configured
                self.port_configure_state.port_config_phase[port_index] =
                    PortConfigPhase::WaitingAddressed;
            }
            None => {
                let port_phase = self.port_configure_state.port_phase_at(port_index);
                if !matches!(
                    port_phase,
                    PortConfigPhase::NotConnected | PortConfigPhase::WaitingAddressed
                ) {
                    panic!("INVALID PortConfigPhase state.");
                }

                self.port_configure_state.start_configuration_at(port_index);
                self.port_register_sets()
                    .update_volatile_at(port_index, |port| {
                        // actual reset operation of port
                        port.portsc.clear_connect_status_change();
                        port.portsc.set_port_reset();
                    });
                while self
                    .port_register_sets()
                    .read_volatile_at(port_index)
                    .portsc
                    .port_reset()
                {}
                log::debug!("[XHCI] port at {} is now reset!", port_index);
            }
        }
    }

    pub fn is_port_connected_at(&self, port_index: usize) -> bool {
        self.registers
            .port_register_set
            .read_volatile_at(port_index)
            .portsc
            .current_connect_status()
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
        let mut device_manager = DeviceManager::new(max_device_slots_enabled);

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
