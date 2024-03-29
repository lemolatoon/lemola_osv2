extern crate alloc;
use core::{alloc::Allocator, mem::MaybeUninit, ptr::NonNull};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use async_trait::async_trait;
use bit_field::BitField;
use kernel_lib::{await_sync, mutex::Mutex};
use usb_host::{
    ConfigurationDescriptor, DescriptorType, DeviceDescriptor, EndpointDescriptor, SetupPacket,
};
use xhci::{
    accessor::Mapper,
    context::{
        Device32Byte, EndpointHandler, EndpointType, Input32Byte, InputControl32Byte,
        InputControlHandler, SlotHandler,
    },
    ring::trb::{
        command, event,
        transfer::{self, TransferType},
    },
};

use crate::{
    alloc::alloc::{alloc_with_boundary_with_default_else, GlobalAllocator},
    usb::{
        class_driver::{keyboard, mouse},
        descriptor::DescriptorIter,
        setup_packet::{SetupPacketRaw, SetupPacketWrapper},
        traits::AsyncUSBHost,
    },
    xhci::{
        command_ring::CommandRing,
        event_ring::{
            CommandCompletionFuture, EventRing, TransferEventFuture, TransferEventWaitKind,
        },
        next_route,
        transfer_ring::TransferRing,
        trb::TrbRaw,
        user_event_ring::{InitPortDevice, UserEvent, UserEventRing},
    },
};

use super::{class_driver::ClassDriverManager, descriptor::Descriptor};

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct InputContextWrapper(pub Input32Byte);

impl InputContextWrapper {
    fn new_zeroed() -> Self {
        Self(Input32Byte::new_32byte())
    }

    pub fn new() -> Box<Self, &'static GlobalAllocator> {
        alloc_with_boundary_with_default_else(64, 4096, Self::new_zeroed).unwrap()
    }

    pub fn dump_input_context_control(&self) {
        let input_control = unsafe { (self as *const _ as *const InputControl32Byte).read() };
        let input_control_raw: [u32; 8] = unsafe { core::mem::transmute(input_control) };
        log::debug!("input_control: {:x?}", input_control_raw);
    }
}

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct DeviceContextWrapper(pub Device32Byte);

impl DeviceContextWrapper {
    fn new_zeroed() -> Self {
        Self(Device32Byte::new_32byte())
    }

    pub fn new() -> Box<Self, &'static GlobalAllocator> {
        alloc_with_boundary_with_default_else(64, 4096, Self::new_zeroed).unwrap()
    }
}

#[derive(Debug)]
pub struct DeviceContextInfo<M: Mapper + Clone + Send + Sync, A: Allocator> {
    registers: Arc<Mutex<xhci::Registers<M>>>,
    event_ring: Arc<Mutex<EventRing<A>>>,
    command_ring: Arc<Mutex<CommandRing>>,
    user_event_ring: Arc<Mutex<UserEventRing>>,
    slot_id: usize,
    port_index: usize,
    routing: u32,
    descriptors: Option<Vec<Descriptor>>,
    pub input_context: Box<InputContextWrapper, A>,
    pub device_context: Box<DeviceContextWrapper, A>,
    // pub event_waiting_issuer_map: BTreeMap<SetupPacketWrapper, Box<dyn ClassDriver>>,
    transfer_rings: [Option<Box<TransferRing<A>, A>>; 31],
}

impl<M: Mapper + Clone + Send + Sync> DeviceContextInfo<M, &'static GlobalAllocator> {
    pub fn new(
        port_index: usize,
        routing: u32,
        slot_id: usize,
        registers: Arc<Mutex<xhci::Registers<M>>>,
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        command_ring: Arc<Mutex<CommandRing>>,
        user_event_ring: Arc<Mutex<UserEventRing>>,
    ) -> Self {
        const TRANSFER_RING_BUF_SIZE: usize = 32;
        #[allow(clippy::type_complexity)]
        let mut transfer_rings: [MaybeUninit<
            Option<Box<TransferRing<&'static GlobalAllocator>, &'static GlobalAllocator>>,
        >; 31] = MaybeUninit::uninit_array();
        for transfer_ring in transfer_rings.iter_mut() {
            unsafe {
                transfer_ring.as_mut_ptr().write(None);
            }
        }
        let mut transfer_rings = unsafe {
            // assume init
            core::mem::transmute::<
                _,
                [Option<Box<TransferRing<&'static GlobalAllocator>, &'static GlobalAllocator>>; 31],
            >(transfer_rings)
        };
        // 4.3.3 Device Slot Initialization
        // 4. Allocate and initialize the Transfer Ring for Default Control Endpoint...
        transfer_rings[0] = Some(TransferRing::alloc_new(TRANSFER_RING_BUF_SIZE));
        Self {
            registers,
            event_ring,
            command_ring,
            user_event_ring,
            slot_id,
            port_index,
            descriptors: None,
            routing,
            // 4.3.3 Device Slot Initialization
            // 1. Allocate an Input Context ...
            input_context: InputContextWrapper::new(), // 0 filled
            // 6. Allocate the Output Device Context data structure (6.2.1)...
            device_context: DeviceContextWrapper::new(), // 0 filled
            transfer_rings,
        }
    }

    pub fn device_address(&self) -> u8 {
        use xhci::context::DeviceHandler;
        self.device_context.0.slot().usb_device_address()
    }

    pub fn slot_id(&self) -> usize {
        self.slot_id
    }

    pub fn enable_slot_context(&mut self) {
        use xhci::context::InputHandler;
        let control = self.input_context.0.control_mut();
        // 6.2.5.1 Input Control Context
        // set A0
        control.set_add_context_flag(0);
    }

    pub fn enable_endpoint(&mut self, dci: DeviceContextIndex) {
        use xhci::context::InputHandler;
        let control = self.input_context.0.control_mut();
        control.set_add_context_flag(dci.address() as usize);
    }

    pub fn initialize_slot_context(
        &mut self,
        port_id: u8,
        port_speed: u8,
        routing: u32,
        parent_hub_slot_id: Option<u8>,
        parent_port_index: Option<u8>,
    ) {
        // 4.3.3 Device Slot Initialization
        // 3. Initialize the Input Slot Context data structure (6.2.2)
        use xhci::context::InputHandler;
        log::debug!("initialize_slot_context: port_id: {}", port_id);
        let slot_context = self.input_context.0.device_mut().slot_mut();
        // Route String = Topology defined. (To access a device attached directly to a Root Hub port, the Route String shall equal '0'.)
        slot_context.set_route_string(routing & 0x3_ffff);
        // and the Root Hub Port Number shall indicate the specific Root Hub port to use.
        slot_context.set_root_hub_port_number(port_id);
        if let Some(parent_hub_slot_id) = parent_hub_slot_id {
            slot_context.set_parent_hub_slot_id(parent_hub_slot_id);
        }
        if let Some(parent_port_index) = parent_port_index {
            slot_context.set_parent_port_number(parent_port_index);
        }
        // Context Entries = 1
        slot_context.set_context_entries(1);
        slot_context.set_speed(port_speed);
    }

    pub fn slot_context(&self) -> &dyn SlotHandler {
        use xhci::context::InputHandler;
        self.input_context.0.device().slot()
    }

    pub fn slot_context_mut(&mut self) -> &mut (dyn SlotHandler + Send + Sync) {
        use xhci::context::InputHandler;
        unsafe { core::mem::transmute(self.input_context.0.device_mut().slot_mut()) }
    }

    pub fn input_context_mut(&mut self) -> &mut (dyn InputControlHandler + Send + Sync) {
        use xhci::context::InputHandler;
        unsafe { core::mem::transmute(self.input_context.0.control_mut()) }
    }

    pub fn endpoint_context(&self, dci: DeviceContextIndex) -> &dyn EndpointHandler {
        use xhci::context::InputHandler;
        self.input_context
            .0
            .device()
            .endpoint(dci.address() as usize)
    }

    pub fn endpoint_context_mut(&mut self, dci: DeviceContextIndex) -> &mut dyn EndpointHandler {
        use xhci::context::InputHandler;
        self.input_context
            .0
            .device_mut()
            .endpoint_mut(dci.address() as usize)
    }

    pub fn transfer_ring_at(
        &self,
        dci: DeviceContextIndex,
    ) -> &Option<Box<TransferRing<&'static GlobalAllocator>, &'static GlobalAllocator>> {
        &self.transfer_rings[dci.address() as usize - 1]
    }

    pub fn transfer_ring_at_mut(
        &mut self,
        dci: DeviceContextIndex,
    ) -> &mut Option<Box<TransferRing<&'static GlobalAllocator>, &'static GlobalAllocator>> {
        &mut self.transfer_rings[dci.address() as usize - 1]
    }

    pub fn initialize_endpoint0_context(
        &mut self,
        transfer_ring_dequeue_pointer: u64,
        max_packet_size: u16,
    ) {
        // 4.3.3 Device Slot Initialization
        // 5. Initialize the Input default control Endpoint 0 Context (6.2.3)
        use xhci::context::InputHandler;
        let endpoint_context_0_id = DeviceContextIndex::ep0();
        let endpoint0_context = self
            .input_context
            .0
            .device_mut()
            .endpoint_mut(endpoint_context_0_id.address() as usize);

        log::debug!("max_packet_size: {}", max_packet_size);
        // EP Type = Control
        endpoint0_context.set_endpoint_type(EndpointType::Control);
        // Max Packet Size
        endpoint0_context.set_max_packet_size(max_packet_size);
        // Max Burst Size = 0
        endpoint0_context.set_max_burst_size(0);
        // TR Dequeue Pointer = Start address of first segment of the Default Control Endpoint Transfer Ring
        endpoint0_context.set_tr_dequeue_pointer(transfer_ring_dequeue_pointer);
        // Dequeue Cycle State(DCS) = 1
        endpoint0_context.set_dequeue_cycle_state();
        // interval = 0
        endpoint0_context.set_interval(0);
        // Max Primary Streams (MaxPStreams) = 0
        endpoint0_context.set_max_primary_streams(0);
        // Mult = 0
        endpoint0_context.set_mult(0);
        // Error Count(CErr) = 3
        endpoint0_context.set_error_count(3);

        // 6.2.3 Endpoint Context
        // Note: Software shall set Average TRB Length to ‘8’ for control endpoints.
        endpoint0_context.set_average_trb_length(8);
    }

    pub async fn start_initialization<MF, KF>(&mut self, class_drivers: &ClassDriverManager<MF, KF>)
    where
        MF: Fn(u8, &[u8]),
        KF: Fn(u8, &[u8]),
    {
        let device_descriptor = self.request_device_descriptor().await;
        {
            let buffer_len = self
                .transfer_ring_at(DeviceContextIndex::ep0())
                .as_ref()
                .unwrap()
                .buffer_len();
            for _ in 0..buffer_len {
                // ensure transfer ring is correctly working.
                let _ = self.request_device_descriptor().await;
            }
        }
        let descriptors = self.request_config_descriptor_and_rest().await;
        log::debug!("descriptors requested with config: {:?}", descriptors);
        let mut boot_keyboard_interface = None;
        let mut mouse_interface = None;
        let mut hub_interface = None;
        let mut endpoint_descriptor = None;
        for desc in descriptors {
            if let Descriptor::Interface(interface) = desc {
                match (
                    interface.b_interface_class,
                    interface.b_interface_sub_class,
                    interface.b_interface_protocol,
                ) {
                    (3, 1, 1) => {
                        log::debug!("HID boot keyboard interface found");
                        boot_keyboard_interface = Some(interface);
                    }
                    (3, 1, 2) => {
                        log::debug!("HID mouse interface found");
                        mouse_interface = Some(interface);
                    }
                    (9, 0, protocol) => {
                        match protocol {
                            0 => log::debug!("Full-Speed hub found"),
                            1 => log::debug!("Hi-speed hub with single TT found"),
                            2 => log::debug!("Hi-speed hub with multiple TTs found"),
                            _ => log::debug!("unknown hub found"),
                        };
                        hub_interface = Some(interface);
                    }
                    unknown => {
                        log::debug!("unknown interface found: {:?}", unknown);
                    }
                };
            } else if let Descriptor::Endpoint(endpoint) = desc {
                log::debug!("endpoint: {:?}", endpoint);
                if (boot_keyboard_interface.is_some() || mouse_interface.is_some())
                    && endpoint_descriptor.is_none()
                {
                    endpoint_descriptor = Some(endpoint);
                }
            }
        }
        if let Some(_boot_keyboard_interface) = boot_keyboard_interface {
            let dci = DeviceContextIndex::from(endpoint_descriptor.as_ref().unwrap());
            let address = self.device_address();
            log::info!("add keyboard device");
            class_drivers
                .add_keyboard_device(self.slot_id(), device_descriptor, address)
                .unwrap();
            {
                let mut driver_info = kernel_lib::lock!(class_drivers.keyboard());
                driver_info.driver.tick_until_running_state(self).unwrap();
                let ep = driver_info.driver.endpoints_mut(address)[0]
                    .as_mut()
                    .unwrap();
                await_sync!(self.init_transfer_ring_for_interrupt_at(
                    ep,
                    endpoint_descriptor.as_ref().unwrap()
                ))
                .unwrap();
            };
            let transfer_ring = self
                .transfer_ring_at_mut(dci)
                .as_mut()
                .expect("transfer ring not allocated")
                .as_mut();
            transfer_ring.fill_with_normal(keyboard::N_IN_TRANSFER_BYTES);
            {
                // door-bell
                let mut registers = kernel_lib::lock!(self.registers);
                registers
                    .doorbell
                    .update_volatile_at(self.slot_id(), |doorbell| {
                        doorbell.set_doorbell_target(dci.address());
                        doorbell.set_doorbell_stream_id(0);
                    });
            }
        }
        if let Some(_mouse_interface) = mouse_interface {
            let dci = DeviceContextIndex::from(endpoint_descriptor.as_ref().unwrap());
            let address = self.device_address();
            class_drivers
                .add_mouse_device(self.slot_id(), device_descriptor, address)
                .unwrap();
            {
                let mut driver_info = kernel_lib::lock!(class_drivers.mouse());
                driver_info.driver.tick_until_running_state(self).unwrap();
                let ep = driver_info.driver.endpoints_mut(address)[0]
                    .as_mut()
                    .unwrap();
                await_sync!(self.init_transfer_ring_for_interrupt_at(
                    ep,
                    endpoint_descriptor.as_ref().unwrap()
                ))
                .unwrap();
            };
            let transfer_ring = self
                .transfer_ring_at_mut(dci)
                .as_mut()
                .expect("transfer ring not allocated")
                .as_mut();
            transfer_ring.fill_with_normal(mouse::N_IN_TRANSFER_BYTES);
            {
                // door-bell
                let mut registers = kernel_lib::lock!(self.registers);
                registers
                    .doorbell
                    .update_volatile_at(self.slot_id(), |doorbell| {
                        doorbell.set_doorbell_target(dci.address());
                        doorbell.set_doorbell_stream_id(0);
                    });
            }
        }
        if let Some(_hub_interface) = hub_interface {
            let address = self.device_address();
            class_drivers
                .add_hub_device(self.slot_id(), device_descriptor, address)
                .unwrap();
            {
                let mut driver_info = kernel_lib::lock!(class_drivers.hub());
                driver_info.driver.tick_until_running_state(self).unwrap();
            };
        }
    }

    /// Host to Device
    pub fn push_control_transfer(
        &mut self,
        endpoint_id: EndpointId,
        setup_data: SetupPacketWrapper,
        buf: Option<NonNull<[u8]>>,
    ) -> TransferEventWaitKind {
        let dci: DeviceContextIndex = endpoint_id.address();

        let transfer_ring = self
            .transfer_ring_at_mut(dci)
            .as_mut()
            .expect("transfer ring not allocated")
            .as_mut();

        let setup_data = SetupPacketRaw::from(setup_data.0);
        let mut status_trb = transfer::StatusStage::new();
        let wait_ons = if let Some(buf) = buf {
            let buf = unsafe { buf.as_ref() };
            let mut setup_stage_trb = transfer::SetupStage::new();
            setup_stage_trb
                .set_request_type(setup_data.bm_request_type)
                .set_request(setup_data.b_request)
                .set_value(setup_data.w_value)
                .set_index(setup_data.w_index)
                .set_length(setup_data.w_length)
                .set_transfer_type(TransferType::In);
            let setup_trb_ptr =
                transfer_ring.push(transfer::Allowed::SetupStage(setup_stage_trb)) as u64;

            let mut data_stage_trb = transfer::DataStage::new();
            data_stage_trb
                .set_trb_transfer_length(buf.len() as u32)
                .set_data_buffer_pointer(buf.as_ptr() as u64)
                .set_td_size(0)
                .set_direction(transfer::Direction::In)
                .set_interrupt_on_completion();

            let data_trb_ref_in_ring =
                transfer_ring.push(transfer::Allowed::DataStage(data_stage_trb)) as u64;

            let status_trb_ptr =
                transfer_ring.push(transfer::Allowed::StatusStage(status_trb)) as u64;
            alloc::vec![setup_trb_ptr, data_trb_ref_in_ring, status_trb_ptr]
        } else {
            let mut setup_stage_trb = transfer::SetupStage::new();
            setup_stage_trb
                .set_request_type(setup_data.bm_request_type)
                .set_request(setup_data.b_request)
                .set_value(setup_data.w_value)
                .set_index(setup_data.w_index)
                .set_length(setup_data.w_length)
                .set_transfer_type(TransferType::No);
            let setup_stage_trb_ptr =
                transfer_ring.push(transfer::Allowed::SetupStage(setup_stage_trb)) as u64;

            status_trb.set_direction().set_interrupt_on_completion();
            let status_trb_ptr =
                transfer_ring.push(transfer::Allowed::StatusStage(status_trb)) as u64;

            alloc::vec![setup_stage_trb_ptr, status_trb_ptr]
        };

        let mut registers = kernel_lib::lock!(self.registers);

        registers
            .doorbell
            .update_volatile_at(self.slot_id(), |ring| {
                ring.set_doorbell_target(dci.address());
                ring.set_doorbell_stream_id(0);
            });
        TransferEventWaitKind::TrbPtrs(wait_ons)
    }

    pub async fn async_control_transfer(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        bm_request_type: usb_host::RequestType,
        b_request: usb_host::RequestCode,
        w_value: usb_host::WValue,
        w_index: u16,
        buf: Option<&mut [u8]>,
    ) -> Result<usize, usb_host::TransferError> {
        let w_length = buf.as_ref().map_or(0, |buf| buf.len() as u16);
        let setup_packet = SetupPacket {
            bm_request_type,
            b_request,
            w_value,
            w_index,
            w_length,
        }
        .into();
        let endpoint_id = EndpointId::from_endpoint(ep);
        let trb_wait_on =
            self.push_control_transfer(endpoint_id, setup_packet, buf.map(|buf| buf[..].into()));
        let event_ring = Arc::clone(&self.event_ring);
        let trb = {
            TransferEventFuture::new(event_ring, Arc::clone(&self.registers), trb_wait_on).await
        };
        match trb.completion_code() {
            Ok(event::CompletionCode::ShortPacket) => {}
            Ok(event::CompletionCode::Success) => {
                return Ok(w_length as usize);
            }
            Ok(err) => {
                log::error!("err: {:?}", err);
                return Err(usb_host::TransferError::Retry("CompletionCode error"));
            }
            Err(err) => {
                log::debug!("err: {:?}", err);
                return Err(usb_host::TransferError::Permanent(
                    "Unknown completion code",
                ));
            }
        }
        Ok(w_length as usize - trb.trb_transfer_length() as usize)
    }

    pub async fn async_register_hub(
        &mut self,
        _address: u8,
    ) -> Result<(), usb_host::TransferError> {
        // https://github.com/foliagecanine/tritium-os/blob/master/kernel/arch/i386/usb/xhci.c#L810
        log::debug!("[xHCI] Attempting to register hub");
        let slot_context = self.slot_context_mut();

        // 6.2.2 Slot Context
        // Context Entries. This field identifies the index of the last valid Endpoint Context within this
        // Device Context structure. The value of ‘0’ is Reserved and is not a valid entry for this field. Valid
        // entries for this field shall be in the range of 1-31. This field indicates the size of the Device
        // Context structure. For example, ((Context Entries+1) * 32 bytes) = Total bytes for this structure.
        // Note, Output Context Entries values are written by the xHC, and Input Context Entries values are
        // written by software.
        const XHCI_SLOT_ENTRY_HUB: u8 = 26;
        slot_context.set_context_entries(XHCI_SLOT_ENTRY_HUB);

        let input_context = self.input_context_mut();
        input_context.set_add_context_flag(0);
        input_context.clear_add_context_flag(1);
        for i in 2..32 {
            input_context.clear_drop_context_flag(i);
            input_context.clear_add_context_flag(i);
        }

        let input_context_pointer = &*self.input_context as *const InputContextWrapper as u64;

        let mut evaluate_context = command::EvaluateContext::new();
        evaluate_context.set_input_context_pointer(input_context_pointer);
        evaluate_context.set_slot_id(self.slot_id() as u8);
        let trb = command::Allowed::EvaluateContext(evaluate_context);

        let trb_ptr = {
            let mut command_ring = kernel_lib::lock!(self.command_ring);
            command_ring.push(trb) as u64
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
        let recieved = CommandCompletionFuture::new(event_ring, registers, trb_ptr).await;
        log::debug!("recieved: {:?}", &recieved);
        assert!(recieved.completion_code().unwrap() == event::CompletionCode::Success);

        Ok(())
    }

    async fn async_assign_address(
        &mut self,
        _hub_address: u8,
        port_index: u8,
        device_is_low_speed: bool,
    ) -> Result<(), usb_host::TransferError> {
        let hub_port_index = self.port_index as u8;
        let routing = next_route(self.routing, port_index + 1);
        let speed = if device_is_low_speed { 1 } else { 0 };
        let _parent_hub_slot_id = self.slot_id() as u8;
        let _parent_port_index = self.port_index as u8;
        let init_port_device = InitPortDevice {
            port_index: hub_port_index,
            routing,
            speed,
            parent_hub_slot_id: None,
            parent_port_index: None,
        };
        {
            let mut user_event_ring = kernel_lib::lock!(&self.user_event_ring);
            user_event_ring.push(UserEvent::InitPortDevice(init_port_device))
        }
        Ok(())
    }

    async fn init_transfer_ring_for_interrupt_at(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        endpoint_descriptor: &EndpointDescriptor,
    ) -> Result<bool, usb_host::TransferError> {
        use xhci::context::InputHandler;
        let dci = DeviceContextIndex::from(endpoint_descriptor);
        log::debug!("dci: {:?}", dci);
        let portsc = {
            let registers = kernel_lib::lock!(self.registers);
            registers
                .port_register_set
                .read_volatile_at(self.port_index)
                .portsc
        };
        if self.transfer_ring_at(dci).is_none() {
            // Configure endpoint
            self.input_context = InputContextWrapper::new();
            {
                let input_control_context = self.input_context.0.control_mut();
                input_control_context.set_add_context_flag(0);
                match ep.transfer_type() {
                    usb_host::TransferType::Interrupt => {
                        let transfer_ring = TransferRing::alloc_new(32);
                        // transfer_ring.fill_with_normal();
                        input_control_context.set_add_context_flag(dci.address() as usize);
                        let device_context = self.input_context.0.device_mut();
                        // Setup endpoint context
                        let endpoint_context = device_context.endpoint_mut(dci.address() as usize);
                        endpoint_context.set_endpoint_type(EndpointType::InterruptIn);
                        endpoint_context.set_tr_dequeue_pointer(transfer_ring.buffer_ptr()
                            as *const TrbRaw
                            as u64);
                        endpoint_context.set_dequeue_cycle_state();
                        endpoint_context.set_error_count(3);
                        endpoint_context.set_max_packet_size(ep.max_packet_size());
                        endpoint_context.set_average_trb_length(1); // TODO: set this correctly
                        endpoint_context.set_max_burst_size(0);
                        endpoint_context.set_max_primary_streams(0);
                        endpoint_context.set_max_endpoint_service_time_interval_payload_low(
                            ep.max_packet_size(),
                        );
                        endpoint_context.set_mult(0);
                        log::debug!("port speed: {}", portsc.port_speed());
                        let interval = match portsc.port_speed() {
                        1 /* FullSpeed */ | 2 /* LowSpeed */ => endpoint_descriptor.b_interval.reverse_bits().get_bit(0) /* most significant bit */ as u8 + 3,
                        3 /* HighSpeed */ | 4 /* SuperSpeed */ => endpoint_descriptor.b_interval - 1,
                        _ => return Err(usb_host::TransferError::Permanent("Unknown speed")),
                    };
                        endpoint_context.set_interval(interval);
                        // End Setup endpoint context
                        *self.transfer_ring_at_mut(dci) = Some(transfer_ring);
                    }
                    usb_host::TransferType::Control => todo!(),
                    usb_host::TransferType::Isochronous => todo!(),
                    usb_host::TransferType::Bulk => todo!(),
                }
                let device_context = self.input_context.0.device_mut();
                device_context.slot_mut().set_context_entries(dci.address());
            }

            let trb = {
                let mut trb = command::ConfigureEndpoint::new();
                trb.set_input_context_pointer(
                    &*self.input_context as *const InputContextWrapper as u64,
                );
                trb.set_slot_id(self.slot_id() as u8);
                let trb_ptr = {
                    let mut command_ring = kernel_lib::lock!(self.command_ring);
                    command_ring.push(command::Allowed::ConfigureEndpoint(trb))
                } as u64;
                {
                    let mut registers = kernel_lib::lock!(self.registers);
                    registers.doorbell.update_volatile_at(0, |doorbell| {
                        doorbell.set_doorbell_target(0);
                        doorbell.set_doorbell_stream_id(0);
                    });
                }
                let event_ring = Arc::clone(&self.event_ring);
                let registers = Arc::clone(&self.registers);
                EventRing::get_received_command_trb(event_ring, registers, trb_ptr).await
            };
            match trb.completion_code() {
                Ok(event::CompletionCode::Success) => {
                    log::debug!("ConfigureEndpoint Success");
                }
                code => {
                    log::debug!("ConfigureEndpoint {:?}", code);
                    return Err(usb_host::TransferError::Retry("CompletionCode error"));
                }
            };

            return Ok(true);
        }

        Ok(false)
    }

    async fn async_in_transfer(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        buf: &mut [u8],
    ) -> Result<usize, usb_host::TransferError> {
        if self.descriptors.is_none() {
            self.request_config_descriptor_and_rest().await;
        }
        let endpoint_descriptor = self
            .descriptors
            .as_ref()
            .unwrap()
            .iter()
            .filter_map(|descriptor| {
                if let Descriptor::Endpoint(endpoint_descriptor) = descriptor {
                    if endpoint_descriptor.b_endpoint_address & 0x7f == ep.endpoint_num() {
                        Some(*endpoint_descriptor)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .next()
            .ok_or(usb_host::TransferError::Permanent(
                "Endpoint Descriptor Not Found",
            ))?;
        let dci = DeviceContextIndex::from(&endpoint_descriptor);
        assert!(matches!(
            ep.transfer_type(),
            usb_host::TransferType::Interrupt
        ));
        log::debug!("dci: {:?}", dci);
        self.init_transfer_ring_for_interrupt_at(ep, &endpoint_descriptor)
            .await?;

        let event_ring = Arc::clone(&self.event_ring);
        let transfer_ring = self.transfer_ring_at_mut(dci).as_mut().unwrap();
        transfer_ring.dump_state();
        let mut normal = transfer::Normal::new();
        normal
            .set_data_buffer_pointer(buf.as_ptr() as u64)
            .set_trb_transfer_length(buf.len() as u32)
            .set_td_size(0)
            .set_interrupt_on_completion()
            .set_interrupt_on_short_packet()
            .set_interrupter_target(0);
        transfer_ring.push(transfer::Allowed::Normal(normal));

        let slot_id = self.slot_id();
        {
            let mut registers = kernel_lib::lock!(self.registers);
            registers
                .doorbell
                .update_volatile_at(self.slot_id(), |doorbell| {
                    doorbell.set_doorbell_target(dci.address());
                    doorbell.set_doorbell_stream_id(0);
                });
        }
        // TODO: ここでawaitをまたいでlockを保持しているのがdeadlockになっているので、registersをArc::cloneして渡すようにする
        let trb = {
            log::debug!("before debug");
            EventRing::get_received_transfer_trb_on_slot(
                event_ring,
                Arc::clone(&self.registers),
                slot_id as u8,
            )
            .await
        };
        let transferred_length = trb.trb_transfer_length();

        let transfer_ring = self.transfer_ring_at_mut(dci).as_mut().unwrap();
        transfer_ring.dump_state();

        log::debug!("trb pointer: {:p}", trb.trb_pointer() as *const TrbRaw);
        let transfer_trb = transfer::Allowed::try_from(unsafe {
            (trb.trb_pointer() as *const TrbRaw).read_volatile()
        })
        .unwrap();
        match transfer_trb {
            transfer::Allowed::SetupStage(_) => todo!(),
            transfer::Allowed::DataStage(_) => todo!(),
            transfer::Allowed::StatusStage(_) => todo!(),
            transfer::Allowed::Isoch(_) => todo!(),
            transfer::Allowed::Link(_) => todo!(),
            transfer::Allowed::EventData(_) => todo!(),
            transfer::Allowed::Noop(_) => todo!(),
            transfer::Allowed::Normal(normal) => {
                let normal_data_buffer_pointer = normal.data_buffer_pointer();
                let copying_length = core::cmp::min(transferred_length, buf.len() as u32);
                let buffer = unsafe {
                    core::slice::from_raw_parts(
                        normal_data_buffer_pointer as *const u8,
                        copying_length as usize,
                    )
                };
                log::debug!("transferred_length {}", transferred_length);
                let transferred_length_given_buf = unsafe {
                    core::slice::from_raw_parts_mut(buf.as_mut_ptr(), copying_length as usize)
                };
                log::debug!("data_buffer_pointer 0x{:x}", normal_data_buffer_pointer);
                log::debug!(
                    "src [{:p} - {:p}] -> dst [{:p} - {:p}]",
                    buffer.as_ptr(),
                    unsafe { buffer.as_ptr().add(buffer.len()) },
                    transferred_length_given_buf.as_mut_ptr(),
                    unsafe {
                        transferred_length_given_buf
                            .as_mut_ptr()
                            .add(transferred_length_given_buf.len())
                    }
                );
                transferred_length_given_buf.copy_from_slice(buffer);
            }
        };
        Ok(transferred_length as usize)
    }
}

impl<M: Mapper + Clone + Send + Sync> DeviceContextInfo<M, &'static GlobalAllocator> {
    // request descriptor impls

    pub async fn request_device_descriptor(&mut self) -> DeviceDescriptor {
        let mut device_descriptor: MaybeUninit<DeviceDescriptor> = MaybeUninit::uninit();
        let length = self
            .request_descriptor(
                EndpointId::default_control_pipe(),
                DescriptorType::Device,
                0,
                as_byte_slice_mut(&mut device_descriptor),
            )
            .await;
        assert_eq!(length, core::mem::size_of::<DeviceDescriptor>());
        unsafe { device_descriptor.assume_init() }
    }

    pub async fn request_config_descriptor_and_rest(&mut self) -> Vec<Descriptor> {
        let mut config_descriptor_buf: MaybeUninit<ConfigurationDescriptor> = MaybeUninit::uninit();
        let length = self
            .request_descriptor(
                EndpointId::default_control_pipe(),
                DescriptorType::Configuration,
                0,
                as_byte_slice_mut(&mut config_descriptor_buf),
            )
            .await;
        assert_eq!(length, core::mem::size_of::<ConfigurationDescriptor>());
        let config_descriptor = unsafe { config_descriptor_buf.assume_init() };
        let mut buf: Vec<u8> = Vec::with_capacity(config_descriptor.w_total_length as usize);
        let length = {
            let buf: &mut [u8] = unsafe {
                buf.set_len(buf.capacity());
                &mut buf[..]
            };
            self.request_descriptor(
                EndpointId::default_control_pipe(),
                DescriptorType::Configuration,
                0,
                buf,
            )
            .await
        };
        assert_eq!(length, buf.len());
        let descriptors: Vec<Descriptor> = DescriptorIter::new(&buf).map(Into::into).collect();
        self.descriptors = Some(descriptors.clone());
        descriptors
    }

    /// return actual length transferred
    pub async fn request_descriptor(
        &mut self,
        mut endpoint_id: EndpointId,
        descriptor_type: DescriptorType,
        descriptor_index: u8,
        buf: &mut [u8],
    ) -> usize {
        let bm_request_type = (
            usb_host::RequestDirection::DeviceToHost,
            usb_host::RequestKind::Standard,
            usb_host::RequestRecipient::Device,
        )
            .into();
        let b_request = usb_host::RequestCode::GetDescriptor;
        let w_value = (descriptor_index, descriptor_type as u8).into();
        let w_index = 0;

        let mut count = 0;
        loop {
            let length = self
                .async_control_transfer(
                    &mut endpoint_id,
                    bm_request_type,
                    b_request,
                    w_value,
                    w_index,
                    Some(buf),
                )
                .await;
            if let Ok(length) = length {
                break length;
            }
            count += 1;
            if count > 100 {
                panic!("too many retries: {:?}", length);
            }
        }
    }
}

fn as_byte_slice_mut<T>(buf: &mut T) -> &mut [u8] {
    let buf: &mut [u8] = unsafe {
        core::slice::from_raw_parts_mut(buf as *mut T as *mut u8, core::mem::size_of::<T>())
    };
    buf
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EndpointId {
    endpoint_number: u8,
    direct: usb_host::Direction,
}

#[derive(Clone, Copy, Debug, PartialEq, Ord, PartialOrd, Eq)]
pub struct DeviceContextIndex(u8);
impl DeviceContextIndex {
    pub fn checked_new(dci: u8) -> Self {
        assert!(dci < 32);
        Self(dci)
    }

    pub fn new(endpoint_number: u8, direct: usb_host::Direction) -> Self {
        let dci = calc_dci(endpoint_number, direct);
        Self::checked_new(dci)
    }

    pub fn from_endpoint_address(endpoint_address: u8) -> Self {
        let dci = (endpoint_address & 0xf) * 2 + (endpoint_address >> 7);
        Self::checked_new(dci)
    }

    pub const fn address(&self) -> u8 {
        self.0
    }

    pub const fn ep0() -> Self {
        Self(1)
    }
}

impl From<EndpointId> for DeviceContextIndex {
    fn from(endpoint_id: EndpointId) -> Self {
        Self::new(endpoint_id.endpoint_number, endpoint_id.direct)
    }
}

impl From<&EndpointDescriptor> for DeviceContextIndex {
    fn from(value: &EndpointDescriptor) -> Self {
        Self::checked_new((value.b_endpoint_address & 0xf) * 2 + (value.b_endpoint_address >> 7))
    }
}

pub const fn calc_dci(endpoint_number: u8, direct: usb_host::Direction) -> u8 {
    endpoint_number * 2
        + if endpoint_number == 0 {
            1
        } else if let usb_host::Direction::In = direct {
            1
        } else {
            0
        }
}

impl EndpointId {
    pub fn new(endpoint_number: u8, direct_in: usb_host::Direction) -> Self {
        assert!(endpoint_number < 16);
        Self {
            endpoint_number,
            direct: direct_in,
        }
    }

    pub fn from_endpoint(endpoint: &dyn usb_host::Endpoint) -> Self {
        Self::new(endpoint.endpoint_num(), endpoint.direction())
    }

    pub fn address(&self) -> DeviceContextIndex {
        DeviceContextIndex(calc_dci(self.endpoint_number, self.direct))
    }

    pub const fn default_control_pipe() -> Self {
        Self {
            endpoint_number: 0,
            direct: usb_host::Direction::Out, // actually default control pipe is IN/OUT
        }
    }
}

impl usb_host::Endpoint for EndpointId {
    fn endpoint_num(&self) -> u8 {
        self.endpoint_number
    }

    fn direction(&self) -> usb_host::Direction {
        self.direct
    }

    fn address(&self) -> u8 {
        EndpointId::address(self).address()
    }

    fn transfer_type(&self) -> usb_host::TransferType {
        todo!()
    }

    fn max_packet_size(&self) -> u16 {
        todo!()
    }

    fn in_toggle(&self) -> bool {
        todo!()
    }

    fn set_in_toggle(&mut self, _toggle: bool) {
        todo!()
    }

    fn out_toggle(&self) -> bool {
        todo!()
    }

    fn set_out_toggle(&mut self, _toggle: bool) {
        todo!()
    }
}

impl<M: Mapper + Clone + Send + Sync> usb_host::USBHost
    for DeviceContextInfo<M, &'static GlobalAllocator>
{
    fn control_transfer(
        &mut self,
        ep: &mut dyn usb_host::Endpoint,
        bm_request_type: usb_host::RequestType,
        b_request: usb_host::RequestCode,
        w_value: usb_host::WValue,
        w_index: u16,
        buf: Option<&mut [u8]>,
    ) -> Result<usize, usb_host::TransferError> {
        // Returned None means the transfer is Pending yet
        await_sync!(self.async_control_transfer(
            unsafe { core::mem::transmute(ep) },
            bm_request_type,
            b_request,
            w_value,
            w_index,
            buf
        ))
    }

    fn in_transfer(
        &mut self,
        ep: &mut dyn usb_host::Endpoint,
        buf: &mut [u8],
    ) -> Result<usize, usb_host::TransferError> {
        // await_once_noblocking!(self.async_in_transfer(unsafe { core::mem::transmute(ep) }, buf))
        //     .unwrap_or(Err(usb_host::TransferError::Retry("transfer is pending")))
        log::debug!("in_transfer await_sync!");
        await_sync!(self.async_in_transfer(unsafe { core::mem::transmute(ep) }, buf))
    }

    fn out_transfer(
        &mut self,
        _ep: &mut dyn usb_host::Endpoint,
        _buf: &[u8],
    ) -> Result<usize, usb_host::TransferError> {
        todo!()
    }
}

#[async_trait]
impl<M: Mapper + Clone + Send + Sync + Sync + Send> AsyncUSBHost
    for DeviceContextInfo<M, &'static GlobalAllocator>
{
    async fn control_transfer(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        bm_request_type: usb_host::RequestType,
        b_request: usb_host::RequestCode,
        w_value: usb_host::WValue,
        w_index: u16,
        buf: Option<&mut [u8]>,
    ) -> Result<usize, usb_host::TransferError> {
        self.async_control_transfer(ep, bm_request_type, b_request, w_value, w_index, buf)
            .await
    }

    async fn in_transfer(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        buf: &mut [u8],
    ) -> Result<usize, usb_host::TransferError> {
        self.async_in_transfer(ep, buf).await
    }

    async fn out_transfer(
        &mut self,
        _ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        _buf: &[u8],
    ) -> Result<usize, usb_host::TransferError> {
        todo!()
    }

    async fn register_hub(&mut self, address: u8) -> Result<(), usb_host::TransferError> {
        self.async_register_hub(address).await
    }

    async fn assign_address(
        &mut self,
        hub_address: u8,
        port_index: u8,
        device_is_low_speed: bool,
    ) -> Result<(), usb_host::TransferError> {
        self.async_assign_address(hub_address, port_index, device_is_low_speed)
            .await
    }
}
