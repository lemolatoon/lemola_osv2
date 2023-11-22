use core::{alloc::Allocator, cmp};

extern crate alloc;
use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use kernel_lib::mutex::Mutex;
use xhci::{
    accessor::Mapper,
    extended_capabilities::{self, usb_legacy_support_capability},
    ring::trb::{
        self,
        event::{self, CompletionCode},
        transfer,
    },
    ExtendedCapability,
};

use crate::{
    alloc::alloc::{alloc_array_with_boundary, alloc_with_boundary, GlobalAllocator},
    memory::PAGE_SIZE,
    usb::{
        class_driver::{keyboard, mouse, ClassDriverManager, DriverKind},
        device::{DeviceContextIndex, DeviceContextInfo, InputContextWrapper},
    },
    xhci::{
        command_ring::CommandRing,
        event_ring::{CommandCompletionFuture, EventRing},
        trb::TrbRaw,
    },
};
use spin::MutexGuard;

use super::{
    device_manager::DeviceManager,
    port::{PortConfigPhase, PortConfigureState},
    user_event_ring::{InitPortDevice, UserEventRing},
};

#[derive(Debug)]
pub struct XhciController<M, A, MF, KF>
where
    M: Mapper + Clone + Send + Sync,
    A: Allocator,
    MF: Fn(u8, &[u8]) + 'static,
    KF: Fn(u8, &[u8]) + 'static,
{
    registers: Arc<Mutex<xhci::Registers<M>>>,
    device_manager: DeviceManager<M, A>,
    command_ring: Arc<Mutex<CommandRing>>,
    event_ring: Arc<Mutex<EventRing<A>>>,
    user_event_ring: Arc<Mutex<UserEventRing>>,
    class_driver_manager: &'static ClassDriverManager<MF, KF>,
    number_of_ports: u8,
    port_configure_state: Mutex<PortConfigureState>,
    // port_id -> vector of slot_id
    port_slot_id_map: Mutex<BTreeMap<usize, Vec<usize>>>,
}

impl<M, MF, KF> XhciController<M, &'static GlobalAllocator, MF, KF>
where
    M: Mapper + Clone + Send + Sync + core::fmt::Debug,
    MF: Fn(u8, &[u8]) + 'static,
    KF: Fn(u8, &[u8]) + 'static,
{
    /// # Safety
    /// The caller must ensure that the xHCI registers are accessed only through this struct.
    ///
    /// # Panics
    /// This method panics if `mmio_base` is not aligned correctly.
    ///
    pub unsafe fn new(
        xhci_memory_mapped_io_base_address: usize,
        mapper: M,
        class_driver_manager: &'static ClassDriverManager<MF, KF>,
    ) -> Self
    where
        MF: Fn(u8, &[u8]) + 'static,
        KF: Fn(u8, &[u8]) + 'static,
    {
        let mut registers =
            xhci::Registers::new(xhci_memory_mapped_io_base_address, mapper.clone());
        let hccparam1 = registers.capability.hccparams1.read_volatile();
        let extended_capabilities_list = unsafe {
            extended_capabilities::List::new(xhci_memory_mapped_io_base_address, hccparam1, mapper)
        };
        if let Some(mut extended_capabilities_list) = extended_capabilities_list {
            for extended_capability in extended_capabilities_list.into_iter() {
                log::debug!("extended_capability: {:?}", &extended_capability);
                match extended_capability {
                    Err(_) => continue,
                    Ok(extended_capability) => match extended_capability {
                        ExtendedCapability::UsbLegacySupport(mut usb_legacy_support) => {
                            Self::request_hc_ownership(&mut usb_legacy_support)
                            // todo: write volatile this register
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

        const EVENT_RING_BUF_SIZE: u16 = 32;
        let mut primary_interrupter = registers.interrupter_register_set.interrupter_mut(0);
        let event_ring = Arc::new(Mutex::new(EventRing::new(
            EVENT_RING_BUF_SIZE,
            &mut primary_interrupter,
        )));
        log::debug!("[XHCI] initialize event ring");

        const COMMAND_RING_BUF_SIZE: usize = 32;
        let command_ring = CommandRing::new(COMMAND_RING_BUF_SIZE);
        Self::register_command_ring(&mut registers, &command_ring);
        let command_ring = Arc::new(Mutex::new(command_ring));
        log::debug!("[XHCI] register command ring");

        // This is clippy's bug
        #[allow(clippy::arc_with_non_send_sync)]
        let arc_registers = Arc::new(Mutex::new(registers));

        let user_event_ring = Arc::new(Mutex::new(UserEventRing::new()));
        let device_manager = Self::configure_device_context(
            &arc_registers,
            Arc::clone(&event_ring),
            Arc::clone(&command_ring),
            Arc::clone(&user_event_ring),
        );
        log::debug!("[XHCI] configure device context");
        let mut registers = kernel_lib::lock!(arc_registers);

        // enable interrupt for the primary interrupter
        let mut primary_interrupter = registers.interrupter_register_set.interrupter_mut(0);
        primary_interrupter.imod.update_volatile(|imodi| {
            imodi.set_interrupt_moderation_interval(0);
        });
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

        let port_configure_state = Mutex::new(PortConfigureState::new());

        drop(registers);
        Self {
            registers: arc_registers,
            device_manager,
            command_ring,
            event_ring,
            user_event_ring,
            class_driver_manager,
            number_of_ports,
            port_configure_state,
            port_slot_id_map: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn run(&self) {
        let mut registers = kernel_lib::lock!(self.registers);
        let operational = &mut registers.operational;
        for _ in 0..1000 {
            let mut count = 0;
            unsafe {
                (&mut count as *mut i32).write_volatile(0);
            }
        }
        operational.usbcmd.update_volatile(|usbcmd| {
            usbcmd.set_run_stop();
        });

        while operational.usbsts.read_volatile().hc_halted() {}
        log::debug!("[XHCI] xhc controller starts running!!");
    }

    pub fn pending_already_popped_queue(&self) -> bool {
        let event_ring = kernel_lib::lock!(self.event_ring);
        event_ring.pending_already_popped_queue()
    }

    pub fn pending_event(&self) -> bool {
        let mut registers = kernel_lib::lock!(self.registers);
        let primary_interrupter = &mut registers.interrupter_register_set.interrupter_mut(0);
        let event_ring_trb = unsafe {
            (primary_interrupter
                .erdp
                .read_volatile()
                .event_ring_dequeue_pointer() as *const trb::Link)
                .read_volatile()
        };
        let event_ring = kernel_lib::lock!(self.event_ring);
        if event_ring_trb.cycle_bit() != event_ring.cycle_bit() {
            // EventRing does not have front
            return false;
        }

        true
    }

    pub async fn process_once_received(&self) {
        let trb = {
            let mut event_ring = kernel_lib::lock!(self.event_ring);
            event_ring.pop_already_popped()
        };
        if let Some(trb) = trb {
            self.process_event_ring_event(trb).await;
        }
    }

    pub fn pop_event_ring(&self) -> Option<Result<event::Allowed, TrbRaw>> {
        let mut registers = kernel_lib::lock!(self.registers);
        let primary_interrupter = &mut registers.interrupter_register_set.interrupter_mut(0);
        let event_ring_trb = unsafe {
            (primary_interrupter
                .erdp
                .read_volatile()
                .event_ring_dequeue_pointer() as *const trb::Link)
                .read_volatile()
        };
        let mut event_ring = kernel_lib::lock!(self.event_ring);
        if event_ring_trb.cycle_bit() != event_ring.cycle_bit() {
            // EventRing does not have front
            return None;
        }
        let primary_interrupter = primary_interrupter;
        Some(event_ring.pop(primary_interrupter))
    }

    pub async fn process_event(&self) {
        let Some(popped) = self.pop_event_ring() else {
            return;
        };
        let _trb = match popped {
            Ok(event_trb) => {
                self.process_event_ring_event(event_trb).await;
                return;
            }
            Err(raw) => raw,
        };

        todo!()
    }

    pub async fn process_event_ring_event(&self, event_trb: event::Allowed) {
        // log::debug!("event_trb: {:?}", event_trb);
        match event_trb {
            event::Allowed::TransferEvent(transfer_event) => {
                self.process_transfer_event(transfer_event);
            }
            event::Allowed::CommandCompletion(command_completion) => {
                self.process_command_completion_event(command_completion)
                    .await;
            }
            event::Allowed::PortStatusChange(port_status_change) => {
                self.process_port_status_change_event(port_status_change)
                    .await
            }
            event::Allowed::BandwidthRequest(_) => todo!(),
            event::Allowed::Doorbell(_) => todo!(),
            event::Allowed::HostController(host_controller) => {
                log::warn!("ignoring... {:?}", host_controller);
            }
            event::Allowed::DeviceNotification(_) => todo!(),
            event::Allowed::MfindexWrap(_) => todo!(),
        }
    }

    pub fn number_of_ports(&self) -> u8 {
        self.number_of_ports
    }

    pub fn registers(&self) -> MutexGuard<'_, xhci::Registers<M>> {
        kernel_lib::lock!(self.registers)
    }

    pub fn configure_port_at(&self, port_idx: usize) {
        log::debug!("configure port at: portsc[{}]", port_idx);

        let is_connected = {
            let port_configure_state = kernel_lib::lock!(self.port_configure_state);
            port_configure_state.is_connected(port_idx)
        };
        // TODO: ここで !is_connected とするのでなく、phase == PortConfigPhase::NotConnected とする。
        if !is_connected {
            self.reset_port_at(port_idx);
        }
    }

    pub fn reset_port_at(&self, port_idx: usize) {
        log::debug!("reset port at: portsc[{}]", port_idx);
        // current connect status (CCS)
        let is_connected = self.is_port_connected_at(port_idx);
        // connect status change (CSC)
        let connect_status_change = {
            let mut registers = kernel_lib::lock!(self.registers);
            let port_register_sets = &mut registers.port_register_set;
            port_register_sets
                .read_volatile_at(port_idx)
                .portsc
                .connect_status_change()
        };
        if !is_connected {
            log::debug!("connect status change is not set");
            return;
        }

        if !connect_status_change {
            log::debug!("connect status change is not set");
            return;
        }

        let is_enabled = {
            let mut registers = kernel_lib::lock!(self.registers);
            let port_register_sets = &mut registers.port_register_set;
            let portsc = port_register_sets.read_volatile_at(port_idx).portsc;
            // 4.19.1 Root Hub Port State Machines
            let flags = (
                portsc.port_power(),
                portsc.current_connect_status(),
                portsc.port_enabled_disabled(),
                portsc.port_reset(),
            );
            log::debug!("{:?}", flags);
            flags == (true, true, true, false)
        };

        if is_enabled {
            log::info!("might be USB3.0 device");
        } else {
            log::info!("might be USB2.0 device");
        }

        let can_process = {
            let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
            if port_configure_state.addressing_port_index == Some(port_idx) {
                true
            } else if port_configure_state.addressing_port_index.is_none() {
                port_configure_state.addressing_port_index = Some(port_idx);
                true
            } else {
                // This branch is fallen, when another port is currently trying to be configured
                port_configure_state.set_port_phase_at(port_idx, PortConfigPhase::WaitingAddressed);
                false
            }
        };
        if !can_process {
            return;
        }
        {
            let port_configure_state = kernel_lib::lock!(self.port_configure_state);
            let port_phase = port_configure_state.port_phase_at(port_idx);
            if !matches!(
                port_phase,
                PortConfigPhase::NotConnected | PortConfigPhase::WaitingAddressed
            ) {
                panic!("INVALID PortConfigPhase state.");
            }
        }

        log::debug!(
            "start clear connect status change and port reset port at: portsc[{}]",
            port_idx
        );
        let mut registers = kernel_lib::lock!(self.registers);
        let port_register_sets = &mut registers.port_register_set;
        port_register_sets.update_volatile_at(port_idx, |port| {
            // prevent clearing rw1c bits
            port.portsc.set_0_port_enabled_disabled();
            port.portsc.set_0_connect_status_change();
            port.portsc.set_0_port_enabled_disabled_change();
            port.portsc.set_0_warm_port_reset_change();
            port.portsc.set_0_over_current_change();
            port.portsc.set_0_port_reset_change();
            port.portsc.set_0_port_link_state_change();
            port.portsc.set_0_port_config_error_change();
            // actual reset operation of port
            port.portsc.set_port_power();
        });
        while !port_register_sets
            .read_volatile_at(port_idx)
            .portsc
            .port_power()
        {}
        port_register_sets.update_volatile_at(port_idx, |port| {
            // prevent clearing rw1c bits
            port.portsc.set_0_port_enabled_disabled();
            port.portsc.set_0_connect_status_change();
            port.portsc.set_0_port_enabled_disabled_change();
            port.portsc.set_0_warm_port_reset_change();
            port.portsc.set_0_over_current_change();
            port.portsc.set_0_port_reset_change();
            port.portsc.set_0_port_link_state_change();
            port.portsc.set_0_port_config_error_change();
            // actual reset operation of port
            port.portsc.set_port_reset();
        });
        while port_register_sets
            .read_volatile_at(port_idx)
            .portsc
            .port_reset()
        {}
        port_register_sets.update_volatile_at(port_idx, |port| {
            // prevent clearing rw1c bits
            port.portsc.set_0_port_enabled_disabled();
            port.portsc.set_0_connect_status_change();
            port.portsc.set_0_port_enabled_disabled_change();
            port.portsc.set_0_warm_port_reset_change();
            port.portsc.set_0_over_current_change();
            port.portsc.set_0_port_reset_change();
            port.portsc.set_0_port_link_state_change();
            port.portsc.set_0_port_config_error_change();
            // actual reset operation of port
            port.portsc.clear_connect_status_change();
        });
        while port_register_sets
            .read_volatile_at(port_idx)
            .portsc
            .connect_status_change()
        {}
        log::debug!("[XHCI] port at {} is now reset!", port_idx);
        log::debug!(
            "ports[{}].portsc: {:#x?}",
            port_idx,
            port_register_sets.read_volatile_at(port_idx).portsc
        );
        let is_enabled = {
            let portsc = port_register_sets.read_volatile_at(port_idx).portsc;
            // 4.19.1 Root Hub Port State Machines
            let flags = (
                portsc.port_power(),
                portsc.current_connect_status(),
                portsc.port_enabled_disabled(),
                portsc.port_reset(),
            );
            flags == (true, true, true, false)
        };
        assert!(is_enabled, "port is not enabled");
    }

    pub fn enable_slot_at(&self, port_idx: usize) -> u64 {
        let mut registers = kernel_lib::lock!(self.registers);
        let port_register_sets = &mut registers.port_register_set;
        let port_reg_set = port_register_sets.read_volatile_at(port_idx);
        let is_enabled = port_reg_set.portsc.port_enabled_disabled();
        let current_connect_status = port_reg_set.portsc.current_connect_status();
        // Port Reset をしたなら、少なくとも CSC bitは下がっていなければおかしい。
        let reset_completed = port_reg_set.portsc.connect_status_change();

        log::debug!(
            "portsc[{}]: enable slot: is enabled: {}, is port connect status change: {}, current_connect_status: {}",
            port_idx,
            is_enabled,
            reset_completed,
            current_connect_status
        );

        if is_enabled && !reset_completed {
            port_register_sets.update_volatile_at(port_idx, |port_reg_set| {
                // prevent clearing rw1c bits
                port_reg_set.portsc.set_0_port_enabled_disabled();
                port_reg_set.portsc.set_0_connect_status_change();
                port_reg_set.portsc.set_0_port_enabled_disabled_change();
                port_reg_set.portsc.set_0_warm_port_reset_change();
                port_reg_set.portsc.set_0_over_current_change();
                port_reg_set.portsc.set_0_port_reset_change();
                port_reg_set.portsc.set_0_port_link_state_change();
                port_reg_set.portsc.set_0_port_config_error_change();
                // clear port reset change
                port_reg_set.portsc.clear_port_reset_change();
            });

            {
                let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
                port_configure_state.port_config_phase[port_idx] = PortConfigPhase::EnablingSlot;
            }

            let enable_slot_cmd =
                trb::command::Allowed::EnableSlot(trb::command::EnableSlot::new());
            let trb_ptr = kernel_lib::lock!(self.command_ring).push(enable_slot_cmd) as u64;
            registers.doorbell.update_volatile_at(0, |doorbell| {
                doorbell.set_doorbell_target(0);
                doorbell.set_doorbell_stream_id(0);
            });

            trb_ptr
        } else {
            log::error!("this port[{}] is not ready to be enabled slot", port_idx);
            {
                let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
                port_configure_state.port_config_phase[port_idx] = PortConfigPhase::NotConnected;
            }
            self.reset_port_at(port_idx);
            self.enable_slot_at(port_idx)
        }
    }

    fn address_device_at(
        &self,
        port_index: usize,
        slot_id: usize,
        routing: u32,
        speed: u8,
        parent_hub_slot_id: Option<u8>,
        parent_port_index: Option<u8>,
    ) -> u64 {
        // 4.3.3 Device Slot Initialization
        log::debug!(
            "address device at: port_index: {}, slot_id: {}",
            port_index,
            slot_id
        );
        let ep0_dci = DeviceContextIndex::ep0();

        {
            let mut port_slot_id_map = kernel_lib::lock!(self.port_slot_id_map);
            port_slot_id_map
                .entry(port_index)
                .or_insert_with(Vec::new)
                .push(slot_id);
        }

        // 1. Allocate an Input Context ...
        // 4. Allocate and initialize the Transfer Ring for Default Control Endpoint...
        // 6. Allocate the Output Device Context data structure (6.2.1)...
        let device = self
            .device_manager
            .allocate_device(port_index, slot_id, routing);

        {
            let mut device = kernel_lib::lock!(device);
            let device = device.as_mut().unwrap();
            // 2. Initialize the Input Control Context(6.2.5.1)
            // setting the A0
            device.enable_slot_context();
            // and A1 flags to '1'
            device.enable_endpoint(ep0_dci);
        }

        let mut registers = kernel_lib::lock!(self.registers);
        let porttsc = registers
            .port_register_set
            .read_volatile_at(port_index)
            .portsc;
        {
            let mut device = kernel_lib::lock!(device);
            let device = device.as_mut().unwrap();
            let speed = if speed == 5 {
                porttsc.port_speed()
            } else {
                speed
            };
            // 3. Initialize the Input Slot Context data structure (6.2.2)
            device.initialize_slot_context(
                port_index as u8 + 1,
                speed,
                routing,
                parent_hub_slot_id,
                parent_port_index,
            );

            let transfer_ring_dequeue_pointer = device
                .transfer_ring_at(ep0_dci)
                .as_ref()
                .unwrap()
                .buffer_ptr() as *const TrbRaw
                as u64;
            log::debug!(
                "transfer ring dequeue pointer: {:#x}",
                transfer_ring_dequeue_pointer
            );

            let slot_context = device.slot_context();
            log::debug!("slot_context.speed(): {}", slot_context.speed());
            // todo: check this calculation based on xhci spec
            let max_packet_size = Self::max_packet_size_for_control_pipe(slot_context.speed());
            // let max_packet_size = Self::max_packet_size_for_control_pipe(4); // SuperSpeed

            // 5. Initialize the Input default control Endpoint 0 Context (6.2.3)
            device.initialize_endpoint0_context(transfer_ring_dequeue_pointer, max_packet_size);
        }

        // 7. Load the appropriate (Device Slot ID) entry in the Device Context Base Address Array (5.4.7) with a pointer to the Output Device Context data structure (6.2.1).
        self.device_manager.load_device_context(slot_id);

        let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
        port_configure_state.port_config_phase[port_index] = PortConfigPhase::AddressingDevice;

        // 8. Issue an Address Device Command for Device Slot, ...points to the Input Context data structure described above.
        let input_context_pointer = {
            let mut device = kernel_lib::lock!(device);
            let device = device.as_mut().unwrap();
            &*device.input_context as *const InputContextWrapper as u64
        };
        type InputContextRaw = [u32; 0x420 / 4];
        static_assertions::const_assert_eq!(core::mem::size_of::<InputContextRaw>(), 0x420);
        // unsafe {
        //     (input_context_pointer as *mut u32)
        //         .offset(8)
        //         .write_volatile(0x8300000)
        // };
        // unsafe {
        //     (input_context_pointer as *mut u32)
        //         .offset(9)
        //         .write_volatile(0x10000)
        // };
        // unsafe {
        //     (input_context_pointer as *mut u32)
        //         .offset(17)
        //         .write_volatile(0x4000026)
        // };
        let input_context: InputContextRaw =
            unsafe { (input_context_pointer as *const InputContextRaw).read_volatile() };
        log::debug!("input context: {:x?}", input_context);
        let mut address_device_command = trb::command::AddressDevice::new();
        log::debug!("input context pointer: {:#x}", input_context_pointer);
        address_device_command.set_input_context_pointer(input_context_pointer);
        address_device_command.set_slot_id(slot_id as u8);
        log::debug!("address device command: {:#x?}", address_device_command);
        let address_device_command = trb::command::Allowed::AddressDevice(address_device_command);
        let trb_ptr = kernel_lib::lock!(self.command_ring).push(address_device_command) as u64;
        registers.doorbell.update_volatile_at(0, |doorbell| {
            doorbell.set_doorbell_target(0);
            doorbell.set_doorbell_stream_id(0);
        });

        trb_ptr
    }

    pub async fn initialize_device_at(&self, port_idx: u8, slot_id: u8) {
        log::debug!(
            "initialize device at: portsc[{}], slot_id: {}",
            port_idx,
            slot_id
        );

        let device = self.device_manager.device_by_slot_id(slot_id as usize);
        let mut device = kernel_lib::lock!(device);
        let Some(device) = device.as_mut() else {
            log::error!("device not found for slot_id: {}", slot_id);
            panic!("Invalid slot_id!");
        };
        {
            let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
            port_configure_state
                .set_port_phase_at(port_idx as usize, PortConfigPhase::InitializingDevice);
        }

        device.start_initialization(self.class_driver_manager).await;

        {
            let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
            port_configure_state.set_port_phase_at(port_idx as usize, PortConfigPhase::Configured);
            port_configure_state.addressing_port_index = None;
        }
    }

    pub fn max_packet_size_for_control_pipe(slot_speed: u8) -> u16 {
        match slot_speed {
            4 => 512, // SuperSpeed
            3 => 64,  // HighSpeed
            _ => 8,
        }
    }

    pub fn is_port_connected_at(&self, port_index: usize) -> bool {
        kernel_lib::lock!(self.registers)
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

    fn configure_device_context(
        registers: &Arc<Mutex<xhci::Registers<M>>>,
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        command_ring: Arc<Mutex<CommandRing>>,
        user_event_ring: Arc<Mutex<UserEventRing>>,
    ) -> DeviceManager<M, &'static GlobalAllocator> {
        let cloned_registers = Arc::clone(registers);
        let registers = &mut *kernel_lib::lock!(registers);
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
        let mut device_manager = DeviceManager::new(
            max_device_slots_enabled,
            cloned_registers,
            event_ring,
            command_ring,
            user_event_ring,
        );

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
        log::debug!("DeviceContextBaseAddressArrayPointer: 0x{:x}", ptr_head);
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

    pub fn usb_device_host_at(
        &self,
        slot_id: usize,
    ) -> Arc<Mutex<Option<DeviceContextInfo<M, &'static GlobalAllocator>>>> {
        self.device_manager.device_by_slot_id(slot_id)
    }

    async fn reset_connection_at(&self, port_idx: usize) {
        log::debug!("reset_connection_at[{}]", port_idx);
        // reset PortConfigPhase
        let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
        if let Some(addressing_port_index) = port_configure_state.addressing_port_index {
            if addressing_port_index == port_idx {
                port_configure_state.addressing_port_index = None;
            }
        }
        port_configure_state.set_port_phase_at(port_idx, PortConfigPhase::NotConnected);

        let slot_ids = {
            let port_slot_id_map = kernel_lib::lock!(self.port_slot_id_map);
            port_slot_id_map.get(&port_idx).cloned()
        };
        {
            let mut registers = kernel_lib::lock!(self.registers);
            let port_register_sets = &mut registers.port_register_set;
            port_register_sets.update_volatile_at(port_idx, |port| {
                // prevent clearing rw1c bits
                port.portsc.set_0_port_enabled_disabled();
                port.portsc.set_0_connect_status_change();
                port.portsc.set_0_port_enabled_disabled_change();
                port.portsc.set_0_warm_port_reset_change();
                port.portsc.set_0_over_current_change();
                port.portsc.set_0_port_reset_change();
                port.portsc.set_0_port_link_state_change();
                port.portsc.set_0_port_config_error_change();
                // actual reset operation of port
                port.portsc.clear_connect_status_change();
            });
            while port_register_sets
                .read_volatile_at(port_idx)
                .portsc
                .connect_status_change()
            {}
        }

        if let Some(slot_ids) = slot_ids {
            // deallocate DeviceContextInfo
            for slot_id in slot_ids {
                log::debug!("slot_id: {}", slot_id);
                {
                    let mut count = 0;
                    loop {
                        let trb_ptr = {
                            let mut disable_slot = trb::command::DisableSlot::new();
                            disable_slot.set_slot_id(slot_id as u8);
                            let mut command_ring = kernel_lib::lock!(self.command_ring);
                            command_ring.push(trb::command::Allowed::DisableSlot(disable_slot))
                                as u64
                        };
                        {
                            let mut registers = kernel_lib::lock!(self.registers);
                            registers.doorbell.update_volatile_at(0, |doorbell| {
                                doorbell.set_doorbell_target(0);
                                doorbell.set_doorbell_stream_id(0);
                            });
                        }

                        let event_ring = Arc::clone(&self.event_ring);
                        let registers = Arc::clone(&self.registers);
                        let trb =
                            EventRing::get_received_command_trb(event_ring, registers, trb_ptr)
                                .await;
                        log::debug!("trb: {:?}", trb);
                        match trb.completion_code() {
                            Ok(_) => break,
                            Err(_) => {
                                if count < 10 {
                                    count += 1;
                                    continue;
                                } else {
                                    panic!("failed to get transfer trb on slot_id: {}", slot_id);
                                }
                            }
                        }
                    }
                }
                self.device_manager.deallocate_device(slot_id);
            }
        }
        {
            let mut port_slot_id_map = kernel_lib::lock!(self.port_slot_id_map);
            port_slot_id_map.remove(&port_idx);
        }
    }
}

impl<M, MF, KF> XhciController<M, &'static GlobalAllocator, MF, KF>
where
    M: Mapper + Clone + Send + Sync + core::fmt::Debug,
    MF: Fn(u8, &[u8]),
    KF: Fn(u8, &[u8]),
{
    pub async fn process_user_event(&self) {
        let popped = {
            let mut user_event_ring = kernel_lib::lock!(self.user_event_ring);
            user_event_ring.pop()
        };

        let event = match popped {
            Some(event) => event,
            None => return,
        };

        match event {
            super::user_event_ring::UserEvent::InitPortDevice(init_port_device) => {
                self.process_init_port_device_event(init_port_device).await
            }
        }
    }

    async fn process_init_port_device_event(&self, init_port_device: InitPortDevice) {
        log::debug!("InitPortDevice: {:#x?}", &init_port_device);
        let slot_id = {
            let trb_ptr = self.enable_slot_at(init_port_device.port_index as usize);
            let completion = CommandCompletionFuture::new(
                Arc::clone(&self.event_ring),
                Arc::clone(&self.registers),
                trb_ptr,
            )
            .await;
            log::debug!("completion: {:#x?}", &completion);
            let slot_id = completion.slot_id();

            assert_eq!(
                completion.completion_code().unwrap(),
                CompletionCode::Success
            );

            slot_id
        };

        {
            let trb_ptr = self.address_device_at(
                init_port_device.port_index as usize,
                slot_id as usize,
                init_port_device.routing,
                init_port_device.speed,
                init_port_device.parent_hub_slot_id,
                init_port_device.parent_port_index,
            );

            let completion = CommandCompletionFuture::new(
                Arc::clone(&self.event_ring),
                Arc::clone(&self.registers),
                trb_ptr,
            )
            .await;
            log::debug!("completion: {:#x?}", &completion);

            assert_eq!(
                completion.completion_code().unwrap(),
                CompletionCode::Success
            );

            let trb_raw = unsafe {
                TrbRaw::new_from_ptr(completion.command_trb_pointer() as *const [u32; 4])
            };
            let Ok(trb::command::Allowed::AddressDevice(_address_device)) =
                trb::command::Allowed::try_from(trb_raw)
            else {
                log::error!(
                    "Failed to parse CommandCompletionEvent: {:?}, slot_id: {}",
                    completion,
                    slot_id
                );
                return;
            };
        };
        self.initialize_device_at(init_port_device.port_index, slot_id)
            .await;
    }

    async fn process_port_status_change_event(&self, event: trb::event::PortStatusChange) {
        log::debug!("PortStatusChangeEvent: port_id: {}", event.port_id());
        let port_idx = event.port_id() as usize - 1;

        let connecting = {
            let mut registers = kernel_lib::lock!(self.registers);
            let port_register_sets = &mut registers.port_register_set;
            port_register_sets
                .read_volatile_at(port_idx)
                .portsc
                .current_connect_status()
        };
        let port_config_phase = {
            let port_configure_state = kernel_lib::lock!(self.port_configure_state);
            port_configure_state.port_phase_at(port_idx)
        };
        log::debug!("port_config_phase: {:?}", port_config_phase);
        match port_config_phase {
            PortConfigPhase::NotConnected => {
                let can_process = {
                    let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
                    if port_configure_state.addressing_port_index == Some(port_idx)
                        || port_configure_state.addressing_port_index.is_none()
                    {
                        true
                    } else {
                        port_configure_state
                            .set_port_phase_at(port_idx, PortConfigPhase::WaitingAddressed);
                        false
                    }
                };
                if can_process {
                    self.reset_port_at(port_idx);
                    self.enable_slot_at(port_idx);
                }
            }
            PortConfigPhase::ResettingPort => {
                let can_process = {
                    let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
                    if port_configure_state.addressing_port_index == Some(port_idx) {
                        true
                    } else if port_configure_state.addressing_port_index.is_none() {
                        port_configure_state.addressing_port_index = Some(port_idx);
                        true
                    } else {
                        port_configure_state
                            .set_port_phase_at(port_idx, PortConfigPhase::WaitingAddressed);
                        false
                    }
                };
                if can_process {
                    // already called reset_port_at once
                    self.enable_slot_at(port_idx);
                }
            }
            PortConfigPhase::WaitingAddressed => {
                log::debug!("This portidx {} is waiting addressed", port_idx);
                let mut can_start_initialization = false;
                {
                    let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
                    if port_configure_state.addressing_port_index.is_none() {
                        // どのUSBポートも初期化中でない
                        port_configure_state.addressing_port_index = Some(port_idx);
                        port_configure_state
                            .set_port_phase_at(port_idx, PortConfigPhase::NotConnected);
                        can_start_initialization = true;
                    }
                };

                if can_start_initialization {
                    log::debug!(
                        "can start initialization, port_configure_state: {:?}",
                        kernel_lib::lock!(self.port_configure_state)
                    );
                    self.reset_port_at(port_idx);
                    self.enable_slot_at(port_idx);
                } else {
                    kernel_lib::lock!(self.event_ring)
                        .push(trb::event::Allowed::PortStatusChange(event));
                }
                // for port_idx in 0..self.number_of_ports() {
                //     let registers = self.registers();
                //     let port_register_sets = &registers.port_register_set;
                //     let is_connected = port_register_sets
                //         .read_volatile_at(port_idx as usize)
                //         .portsc
                //         .current_connect_status();
                //     drop(registers);
                //     log::debug!("Port {}: is_connected = {}", port_idx, is_connected);
                //     if is_connected {
                //         let port_config_phase = {
                //             let port_configure_state = kernel_lib::lock!(self.port_configure_state);
                //             port_configure_state.port_phase_at(port_idx as usize)
                //         };
                //         if port_config_phase == PortConfigPhase::WaitingAddressed {
                //             self.reset_port_at(port_idx as usize);
                //         }
                //     }
                // }
            }
            state => {
                log::debug!("state: {:?}, connecting: {}", state, connecting);
                if !connecting {
                    log::debug!(
                        "port[{}] is connecting, then lets reset port config phase",
                        port_idx
                    );
                    self.reset_connection_at(port_idx).await;
                }
            }
        }
    }

    async fn process_command_completion_event(&self, event: trb::event::CommandCompletion) {
        let slot_id = event.slot_id();
        let Ok(completion_code) = event.completion_code() else {
            log::error!(
                "Invalid CommandCompletionEvent: {:?}, slot_id: {}",
                event,
                slot_id
            );
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
            log::error!(
                "Failed to parse CommandCompletionEvent: {:?}, slot_id: {}",
                event,
                slot_id
            );
            return;
        };

        log::debug!(
            "CommandCompletionEvent: {:?}, slot_id: {}",
            command_trb,
            slot_id
        );

        match command_trb {
            trb::command::Allowed::Link(_) => todo!(),
            trb::command::Allowed::EnableSlot(_enable_slot) => {
                let addressing_port_idx = {
                    let port_configure_state = kernel_lib::lock!(self.port_configure_state);
                    let Some(addressing_port_phase) = port_configure_state.addressing_port_phase()
                    else {
                        log::error!("port_configure_state: {:?}", &port_configure_state);
                        log::error!(
                            "No addressing port: {:?}",
                            port_configure_state.addressing_port_index
                        );
                        panic!("InvalidPhase");
                    };
                    if addressing_port_phase != PortConfigPhase::EnablingSlot {
                        log::error!("port_configure_state: {:?}", &port_configure_state);
                        log::error!("InvalidPhase: {:?}", addressing_port_phase);
                        panic!("InvalidPhase")
                    }

                    port_configure_state.addressing_port_index.unwrap()
                };

                self.address_device_at(addressing_port_idx, slot_id as usize, 0, 5, None, None);
            }
            trb::command::Allowed::DisableSlot(_) => todo!(),
            trb::command::Allowed::AddressDevice(_address_device) => {
                let port_index = {
                    let device = self.device_manager.device_by_slot_id(slot_id as usize);
                    let mut device = kernel_lib::lock!(device);
                    let Some(device) = device.as_mut() else {
                        log::error!("InvalidSlotId: {}", slot_id);
                        panic!("InvalidSlotId")
                    };

                    let port_index = device.slot_context().root_hub_port_number() - 1;

                    let mut port_configure_state = kernel_lib::lock!(self.port_configure_state);
                    if port_configure_state.addressing_port_index != Some(port_index as usize) {
                        log::error!(
                            "InvalidPhase:\naddressing: {:?}, received: {}",
                            port_configure_state.addressing_port(),
                            port_index
                        );
                        panic!("InvalidPhase")
                    }

                    if port_configure_state.addressing_port_phase()
                        != Some(PortConfigPhase::AddressingDevice)
                    {
                        log::error!(
                            "InvalidPhase: {:?}",
                            port_configure_state.addressing_port_phase()
                        );
                        panic!("InvalidPhase")
                    }

                    port_configure_state.clear_addressing_port_index();
                    for port_idx in 0..port_configure_state.len() {
                        if port_configure_state.port_phase_at(port_idx)
                            == PortConfigPhase::WaitingAddressed
                        {
                            drop(port_configure_state);
                            self.reset_port_at(port_idx);
                            break;
                        }
                    }
                    port_index
                };

                self.initialize_device_at(port_index, slot_id).await;
            }
            trb::command::Allowed::ConfigureEndpoint(_) => {
                let mut event_ring = kernel_lib::lock!(self.event_ring);
                event_ring.push(event::Allowed::CommandCompletion(event));
            }
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

    fn process_transfer_event(&self, event: trb::event::TransferEvent) {
        match event.completion_code() {
            Ok(event::CompletionCode::ShortPacket | event::CompletionCode::Success) => {}
            Ok(code) => {
                log::error!("TransferEvent failed: {:?}", code);
                return;
            }
            Err(code) => {
                log::error!(
                    "Invalid TransferEvent: {:?}, slot_id: {}, code: {:?}",
                    event,
                    event.slot_id(),
                    code
                );
                panic!(
                    "Invalid TransferEvent: {:?}, slot_id: {}",
                    event,
                    event.slot_id()
                );
            }
        };
        let slot_id = event.slot_id();
        let dci = DeviceContextIndex::checked_new(event.endpoint_id());

        let trb = {
            let device = self.usb_device_host_at(slot_id as usize);
            let mut device = kernel_lib::lock!(device);
            let device = device.as_mut().unwrap();
            let transfer_ring = device.transfer_ring_at_mut(dci).as_mut().unwrap();

            let trb_pointer: *mut TrbRaw = event.trb_pointer() as *mut TrbRaw;
            let trb = transfer::Allowed::try_from(unsafe { trb_pointer.read_volatile() }).unwrap();

            if let transfer::Allowed::Normal(normal) = trb {
                transfer_ring.flip_cycle_bit_at(trb_pointer as u64, normal.cycle_bit());
            }
            trb
        };
        if let transfer::Allowed::Normal(normal) = trb {
            // let transfer_ring = device
            //     .transfer_ring_at_mut(DeviceContextIndex::checked_new(dci))
            //     .as_mut()
            //     .unwrap();

            let buffer = normal.data_buffer_pointer() as *mut u8;
            let driver_kind = self.class_driver_manager.driver_kind(slot_id as usize);
            match driver_kind {
                Some(DriverKind::Mouse) => {
                    assert_eq!(
                        normal.trb_transfer_length(),
                        mouse::N_IN_TRANSFER_BYTES as u32
                    );
                    assert_eq!(3, mouse::N_IN_TRANSFER_BYTES as u32);
                    let address = {
                        let device = self.usb_device_host_at(slot_id as usize);
                        let device = kernel_lib::lock!(device);
                        device.as_ref().unwrap().device_address()
                    };
                    let mut mouse = kernel_lib::lock!(self.class_driver_manager.mouse());
                    let buffer =
                        unsafe { core::slice::from_raw_parts(buffer, mouse::N_IN_TRANSFER_BYTES) };
                    mouse.driver.call_callback_at(address, buffer);
                }
                Some(DriverKind::Keyboard) => {
                    let address = {
                        let device = self.usb_device_host_at(slot_id as usize);
                        let device = kernel_lib::lock!(device);
                        device.as_ref().unwrap().device_address()
                    };
                    let mut keyboard = kernel_lib::lock!(self.class_driver_manager.keyboard());
                    let buffer = unsafe {
                        core::slice::from_raw_parts(buffer, keyboard::N_IN_TRANSFER_BYTES)
                    };
                    keyboard.driver.call_callback_at(address, buffer);
                }
                Some(DriverKind::Hub) => {
                    let address = {
                        let device = self.usb_device_host_at(slot_id as usize);
                        let device = kernel_lib::lock!(device);
                        device.as_ref().unwrap().device_address()
                    };
                    let hub = kernel_lib::lock!(self.class_driver_manager.hub());
                    log::error!(
                        "normal trb for hub driver not yet implemented, address: {}, slot_id: {}, hub: {:?}",
                        address,
                        slot_id,
                        hub
                    );
                    return;
                }
                None => todo!(),
            }

            {
                let mut registers = kernel_lib::lock!(self.registers);
                registers
                    .doorbell
                    .update_volatile_at(slot_id as usize, |r| {
                        r.set_doorbell_target(dci.address());
                        r.set_doorbell_stream_id(0);
                    });
            }
        } else {
            log::warn!("ignoring... {:x?}", trb);
        }
    }
}

macro_rules! gen_tick {
    ($fname:ident, $device:ident) => {
        pub fn $fname(&mut self, count: usize) -> Result<(), usb_host::DriverError>
        where
            MF: Fn(u8, &[u8]),
            KF: Fn(u8, &[u8]),
        {
            use usb_host::Driver;
            let driver = kernel_lib::lock!(self.class_driver_manager.$device());
            if let Some(slot_id) = driver.slot_id {
                let device = self.device_manager.device_by_slot_id(slot_id);
                drop(driver);
                let mut device = kernel_lib::lock!(device);
                if let Some(host) = device.as_mut() {
                    let mut driver = kernel_lib::lock!(self.class_driver_manager.$device());

                    driver.driver.tick(count, host)?;
                }
            }

            Ok(())
        }
    };
}

macro_rules! gen_async_tick {
    ($fname:ident, $device:ident) => {
        pub async fn $fname(&self, count: usize) -> Result<(), usb_host::DriverError>
        where
            MF: Fn(u8, &[u8]),
            KF: Fn(u8, &[u8]),
        {
            use crate::usb::traits::AsyncDriver;
            let driver = self.class_driver_manager.$device();
            let driver = kernel_lib::lock!(driver);
            if let Some(slot_id) = driver.slot_id {
                let device = self.device_manager.device_by_slot_id(slot_id);
                drop(driver);
                let mut device = kernel_lib::lock!(device);
                if let Some(host) = device.as_mut() {
                    let driver = self.class_driver_manager.$device();
                    let mut driver = kernel_lib::lock!(driver);
                    // ここもやばい
                    driver.driver.tick(count, host).await?;
                }
            }

            Ok(())
        }
    };
}
impl<M, MF, KF> XhciController<M, &'static GlobalAllocator, MF, KF>
where
    M: Mapper + Clone + Send + Sync,
    MF: Fn(u8, &[u8]),
    KF: Fn(u8, &[u8]),
{
    gen_tick!(tick_keyboard, keyboard);
    gen_tick!(tick_mouse, mouse);
    gen_async_tick!(async_tick_keyboard, keyboard);
    gen_async_tick!(async_tick_mouse, mouse);
}
