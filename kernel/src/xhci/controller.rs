use core::{cmp, mem::MaybeUninit};

extern crate alloc;
use alloc::boxed::Box;
use xhci::{
    accessor::{array, Mapper},
    context::{Endpoint64Byte, Slot64Byte},
    extended_capabilities::{self, usb_legacy_support_capability},
    registers::PortRegisterSet,
    ring::trb::{self, event},
    ExtendedCapability,
};

use crate::{
    alloc::alloc::{alloc_array_with_boundary, alloc_with_boundary},
    memory::PAGE_SIZE,
    usb::device::EndpointId,
    xhci::{command_ring::CommandRing, event_ring::EventRing, port, trb::TrbRaw},
};

use super::{
    device_manager::DeviceManager,
    port::{PortConfigPhase, PortConfigureState},
    transfer_ring::TransferRing,
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
    transfer_rings: [Option<Box<TransferRing>>; 31],
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
        let mut registers =
            xhci::Registers::new(xhci_memory_mapped_io_base_address, mapper.clone());
        let hccparam1 = registers.capability.hccparams1.read_volatile();
        let extended_capabilities_list = unsafe {
            extended_capabilities::List::new(xhci_memory_mapped_io_base_address, hccparam1, mapper)
        };
        if let Some(mut extended_capabilities_list) = extended_capabilities_list {
            for extended_capability in extended_capabilities_list.into_iter() {
                match extended_capability {
                    Err(_) => continue,
                    Ok(extended_capability) => match extended_capability {
                        ExtendedCapability::UsbLegacySupport(mut usb_legacy_support) => {
                            Self::request_hc_ownership(&mut usb_legacy_support)
                        }
                        ExtendedCapability::XhciSupportedProtocol(_) => {
                            log::debug!("xhci supported protocol")
                        }
                        ExtendedCapability::HciExtendedPowerManagementCapability(_) => {
                            log::debug!("hci extended power management capability")
                        }
                        ExtendedCapability::XhciMessageInterrupt(_) => {
                            log::debug!("xhci message interrupt")
                        }
                        ExtendedCapability::XhciLocalMemory(_) => log::debug!("xhci local memory"),
                        ExtendedCapability::Debug(_) => log::debug!("debug"),
                        ExtendedCapability::XhciExtendedMessageInterrupt(_) => {
                            log::debug!("xhci extended message interrupt")
                        }
                    },
                }
            }
        }
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

        // enable interrupt for the primary interrupter
        primary_interrupter
            .iman
            .update_volatile(|interrupter_management_register| {
                interrupter_management_register.set_0_interrupt_pending();
                interrupter_management_register.set_interrupt_enable();
            });

        // enable interrupt for the controller
        registers.operational.usbcmd.update_volatile(|usbcmd| {
            usbcmd.set_interrupter_enable();
        });

        let port_configure_state = PortConfigureState::new();

        const TRANSFER_RING_LEN: usize = 31;
        let mut transfer_rings =
            MaybeUninit::<[Option<Box<TransferRing>>; TRANSFER_RING_LEN]>::uninit();
        let ptr = transfer_rings.as_mut_ptr() as *mut Option<Box<TransferRing>>;
        let transfer_rings = unsafe {
            for i in 0..TRANSFER_RING_LEN {
                ptr.add(i).write(None);
            }
            transfer_rings.assume_init()
        };

        Self {
            registers,
            device_manager,
            command_ring,
            event_ring,
            number_of_ports,
            port_configure_state,
            transfer_rings,
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
        let mut primary_interrupter = primary_interrupter;
        let event_trb = self.event_ring.pop(&mut primary_interrupter);
        match event_trb {
            event::Allowed::TransferEvent(_) => todo!(),
            event::Allowed::CommandCompletion(command_completion) => {
                self.process_command_completion_event(command_completion)
            }
            event::Allowed::PortStatusChange(port_status_change) => {
                self.process_port_status_change_event(port_status_change)
            }
            event::Allowed::BandwidthRequest(_) => todo!(),
            event::Allowed::Doorbell(_) => todo!(),
            event::Allowed::HostController(_) => todo!(),
            event::Allowed::DeviceNotification(_) => todo!(),
            event::Allowed::MfindexWrap(_) => todo!(),
        }
    }

    pub fn port_register_sets(&mut self) -> &mut array::ReadWrite<PortRegisterSet, M> {
        &mut self.registers.port_register_set
    }

    pub fn number_of_ports(&self) -> u8 {
        self.number_of_ports
    }

    pub fn configure_port_at(&mut self, port_idx: usize) {
        log::debug!("configure port at: {}", port_idx);
        if !self.port_configure_state.is_connected(port_idx) {
            self.reset_port_at(port_idx);
        }
    }

    pub fn reset_port_at(&mut self, port_idx: usize) {
        log::debug!("reset port at: {}", port_idx);
        let is_connected = self.is_port_connected_at(port_idx);
        if !is_connected {
            return;
        }

        match self.port_configure_state.addressing_port_index {
            Some(_) => {
                // This branch is fallen, when another port is currently trying to be configured
                self.port_configure_state.port_config_phase[port_idx] =
                    PortConfigPhase::WaitingAddressed;
            }
            None => {
                let port_phase = self.port_configure_state.port_phase_at(port_idx);
                if !matches!(
                    port_phase,
                    PortConfigPhase::NotConnected | PortConfigPhase::WaitingAddressed
                ) {
                    panic!("INVALID PortConfigPhase state.");
                }

                self.port_configure_state.start_configuration_at(port_idx);
                log::debug!(
                    "start clear connect status change and port reset port at: {}",
                    port_idx
                );
                self.port_register_sets()
                    .update_volatile_at(port_idx, |port| {
                        // actual reset operation of port
                        port.portsc.clear_connect_status_change();
                        port.portsc.set_port_reset();
                    });
                while self
                    .port_register_sets()
                    .read_volatile_at(port_idx)
                    .portsc
                    .port_reset()
                {}
                log::debug!("[XHCI] port at {} is now reset!", port_idx);
            }
        }
    }

    pub fn enable_slot_at(&mut self, port_idx: usize) {
        let port_reg_set = self.port_register_sets().read_volatile_at(port_idx);
        let is_enabled = port_reg_set.portsc.port_enabled_disabled();
        let reset_completed = port_reg_set.portsc.connect_status_change();

        log::debug!(
            "enable slot: is enabled: {}, is port connect status change: {}",
            is_enabled,
            reset_completed
        );

        if is_enabled && reset_completed {
            self.port_register_sets()
                .update_volatile_at(port_idx, |port_reg_set| {
                    // clear port reset change
                    port_reg_set.portsc.clear_port_reset_change();
                    port_reg_set.portsc.set_0_port_reset_change();
                });

            self.port_configure_state.port_config_phase[port_idx] = PortConfigPhase::EnablingSlot;

            let enable_slot_cmd =
                trb::command::Allowed::EnableSlot(trb::command::EnableSlot::new());
            self.command_ring.push(enable_slot_cmd);
            self.registers.doorbell.update_volatile_at(0, |doorbell| {
                doorbell.set_doorbell_target(0);
                doorbell.set_doorbell_stream_id(0);
            })
        }
    }

    fn address_device_at(&mut self, port_index: usize, slot_id: usize) {
        log::debug!(
            "address device at: port_index: {}, slot_id: {}",
            port_index,
            slot_id
        );
        let endpoint_context_0_id = EndpointId::new(0, false);
        let device = self.device_manager.allocate_device(slot_id);
        device.enable_slot_context();
        device.enable_endpoint(endpoint_context_0_id);
        let porttsc = self
            .port_register_sets()
            .read_volatile_at(port_index)
            .portsc;
        let device = self.device_manager.device_by_slot_id_mut(slot_id).unwrap();
        device.initialize_slot_context(port_index as u8 + 1, porttsc.port_speed());

        let transfer_ring = TransferRing::alloc_new(32);
        let transfer_ring_dequeue_pointer = &*transfer_ring as *const _ as u64;
        debug_assert!(self.transfer_rings[endpoint_context_0_id.address() - 1].is_none());
        self.transfer_rings[endpoint_context_0_id.address() - 1] = Some(transfer_ring);

        log::debug!(
            "transfer ring dequeue pointer: {:#x}",
            transfer_ring_dequeue_pointer
        );
        let slot_context = device.slot_context();
        let max_packet_size = Self::max_packet_size_for_control_pipe(slot_context.speed());

        device.initialize_endpoint0_context(transfer_ring_dequeue_pointer, max_packet_size);

        self.device_manager.load_device_context(slot_id);
        let device = self.device_manager.device_by_slot_id_mut(slot_id).unwrap();

        let slot_context = device.slot_context();
        log::debug!("slot context: {:x?}", slot_context.as_ref());
        log::debug!("slot context at: {:p}", slot_context.as_ref().as_ptr());
        let endpoint0_context = device.endpoint_context(endpoint_context_0_id);
        log::debug!("ep0 context: {:x?}", endpoint0_context.as_ref());
        log::debug!("ep0 context: {:p}", endpoint0_context.as_ref().as_ptr());

        self.port_configure_state.port_config_phase[port_index] = PortConfigPhase::AddressingDevice;

        let mut address_device_command = trb::command::AddressDevice::new();
        let input_context_pointer = &device.input_context as *const _ as u64;
        let slot_context_pointer = (input_context_pointer + 32) as *const Slot64Byte;
        let ep0_context_pointer = (input_context_pointer + 64) as *const Endpoint64Byte;
        log::debug!("slot context pointer?: {:p}", slot_context_pointer);
        log::debug!("ep0 context pointer?: {:p}", ep0_context_pointer);
        unsafe {
            let slot_context = &*slot_context_pointer;
            let ep0_context = &*ep0_context_pointer;
            let slot_context_raw = slot_context.as_ref();
            let ep0_context_raw = ep0_context.as_ref();
            log::debug!("slot context: {:x?}", slot_context_raw);
            log::debug!("ep0 context: {:x?}", ep0_context_raw);
        }
        log::debug!("input context pointer: {:#x}", input_context_pointer);
        address_device_command.set_input_context_pointer(input_context_pointer);
        address_device_command.set_slot_id(slot_id as u8);
        log::debug!("address device command: {:#x?}", address_device_command);
        let address_device_command = trb::command::Allowed::AddressDevice(address_device_command);
        self.command_ring.push(address_device_command);
        self.registers.doorbell.update_volatile_at(0, |doorbell| {
            doorbell.set_doorbell_target(0);
            doorbell.set_doorbell_stream_id(0);
        })
    }

    pub fn initialize_device_at(&mut self, port_idx: u8, slot_id: u8) {
        log::debug!(
            "initialize device at: port_id: {}, slot_id: {}",
            port_idx + 1,
            slot_id
        );

        let Some(device) = self
            .device_manager
            .device_by_slot_id_mut(slot_id as usize) else {
                log::error!("device not found for slot_id: {}", slot_id);
                panic!("Invalid slot_id!");
            };
        self.port_configure_state
            .set_port_phase_at(port_idx as usize, PortConfigPhase::InitializingDevice);
        device.start_initialization();
    }

    pub fn max_packet_size_for_control_pipe(slot_speed: u8) -> u16 {
        match slot_speed {
            4 => 512, // SuperSpeed
            3 => 64,  // HighSpeed
            _ => 8,
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
        let operational = &mut registers.operational;
        assert!(
            operational.usbsts.read_volatile().hc_halted(),
            "xHC is not halted."
        );
        log::debug!("xHC is halted.");

        operational.usbcmd.update_volatile(|usbcmd| {
            usbcmd.set_host_controller_reset();
        });
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
                        .write(Box::leak(allocated_array) as *mut [u8; PAGE_SIZE])
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
        registers
            .operational
            .crcr
            .update_volatile(|command_ring_controller_register| {
                command_ring_controller_register.set_ring_cycle_state();
                command_ring_controller_register.set_command_stop(); // TODO: 本当はfalseを入れたいが...
                command_ring_controller_register.set_command_abort();
                command_ring_controller_register
                    .set_command_ring_pointer(ring.buffer_ptr() as *const TrbRaw as u64);
            });
    }

    fn request_hc_ownership(
        usb_legacy_support: &mut usb_legacy_support_capability::UsbLegacySupport<M>,
    ) {
        if usb_legacy_support
            .usblegsup
            .read_volatile()
            .hc_os_owned_semaphore()
        {
            log::debug!("already os owned ownership");
            return;
        }

        usb_legacy_support
            .usblegsup
            .update_volatile(|usb_legacy_support_reg| {
                usb_legacy_support_reg.set_hc_os_owned_semaphore();
            });

        log::debug!("wating until OS has owned xHC...");
        let mut usb_legacy_support_reg = usb_legacy_support.usblegsup.read_volatile();
        while usb_legacy_support_reg.hc_bios_owned_semaphore()
            || !usb_legacy_support_reg.hc_os_owned_semaphore()
        {
            usb_legacy_support_reg = usb_legacy_support.usblegsup.read_volatile();
        }
        log::debug!("OS has owned xHC!!");
    }
}

impl<M> XhciController<M>
where
    M: Mapper + Clone,
{
    // process events

    fn process_port_status_change_event(&mut self, event: trb::event::PortStatusChange) {
        log::debug!("PortStatusChangeEvent: port_id: {}", event.port_id());
        let port_idx = event.port_id() as usize - 1;

        match self.port_configure_state.port_phase_at(port_idx) {
            PortConfigPhase::NotConnected => self.reset_port_at(port_idx),
            PortConfigPhase::ResettingPort => {
                // already called reset_port_at once
                self.enable_slot_at(port_idx);
                log::debug!("enable slot at {} done", port_idx);
            }
            state => {
                log::error!("InvalidPhase: {:?}", state);
                panic!("InvalidPhase")
            }
        }
    }

    fn process_command_completion_event(&mut self, event: trb::event::CommandCompletion) {
        let slot_id = event.slot_id();
        let Ok(completion_code) = event.completion_code() else { log::error!("Invalid CommandCompletionEvent: {:?}, slot_id: {}", event, slot_id);
            return;
        };

        if completion_code != trb::event::CompletionCode::Success {
            log::error!(
                "CommandCompletionEvent failed: {:?}, slot_id: {}",
                completion_code,
                slot_id
            );
            log::error!("{:?}", event);
            return;
        }

        let trb_raw =
            unsafe { TrbRaw::new_from_ptr(event.command_trb_pointer() as *const [u32; 4]) };
        let Ok(command_trb) = trb::command::Allowed::try_from(trb_raw) else {
            log::error!("Failed to parse CommandCompletionEvent: {:?}, slot_id: {}", event, slot_id);
            return;
        };

        log::debug!(
            "CommandCompletionEvent: {:?}, slot_id: {}",
            command_trb,
            slot_id
        );

        match command_trb {
            trb::command::Allowed::Link(_) => todo!(),
            trb::command::Allowed::EnableSlot(enable_slot) => {
                let Some(addressing_port_phase) = self.port_configure_state.addressing_port_phase() else {
                    log::error!("No addressing port: {:?}", self.port_configure_state.addressing_port_index);
                    panic!("InvalidPhase");
                };
                if addressing_port_phase != PortConfigPhase::EnablingSlot {
                    log::error!("InvalidPhase: {:?}", addressing_port_phase);
                    panic!("InvalidPhase")
                }

                let addressing_port_idx = self.port_configure_state.addressing_port_index.unwrap();
                self.address_device_at(addressing_port_idx, slot_id as usize);
            }
            trb::command::Allowed::DisableSlot(_) => todo!(),
            trb::command::Allowed::AddressDevice(_address_device) => {
                let Some(device) = self.device_manager.device_by_slot_id(slot_id as usize) else {
                    log::error!("InvalidSlotId: {}", slot_id);
                    panic!("InvalidSlotId")
                };

                let port_index = device.slot_context().root_hub_port_number() - 1;

                if self.port_configure_state.addressing_port_index != Some(port_index as usize) {
                    log::error!(
                        "InvalidPhase:\naddressing: {:?}, received: {}",
                        self.port_configure_state.addressing_port(),
                        port_index
                    );
                    panic!("InvalidPhase")
                }

                if self.port_configure_state.addressing_port_phase()
                    != Some(PortConfigPhase::AddressingDevice)
                {
                    log::error!(
                        "InvalidPhase: {:?}",
                        self.port_configure_state.addressing_port_phase()
                    );
                    panic!("InvalidPhase")
                }

                self.port_configure_state.clear_addressing_port_index();
                for port_idx in 0..self.port_configure_state.len() {
                    if self.port_configure_state.port_phase_at(port_idx)
                        == PortConfigPhase::WaitingAddressed
                    {
                        self.reset_port_at(port_idx);
                        break;
                    }
                }

                self.initialize_device_at(port_index, slot_id);
            }
            trb::command::Allowed::ConfigureEndpoint(_) => todo!(),
            trb::command::Allowed::EvaluateContext(_) => todo!(),
            trb::command::Allowed::ResetEndpoint(_) => todo!(),
            trb::command::Allowed::StopEndpoint(_) => todo!(),
            trb::command::Allowed::SetTrDequeuePointer(_) => todo!(),
            trb::command::Allowed::ResetDevice(_) => todo!(),
            trb::command::Allowed::ForceEvent(_) => todo!(),
            trb::command::Allowed::NegotiateBandwidth(_) => todo!(),
            trb::command::Allowed::SetLatencyToleranceValue(_) => todo!(),
            trb::command::Allowed::GetPortBandwidth(_) => todo!(),
            trb::command::Allowed::ForceHeader(_) => todo!(),
            trb::command::Allowed::Noop(_) => todo!(),
            trb::command::Allowed::GetExtendedProperty(_) => todo!(),
            trb::command::Allowed::SetExtendedProperty(_) => todo!(),
        }
    }
}
