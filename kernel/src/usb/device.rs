extern crate alloc;
use core::{alloc::Allocator, mem::MaybeUninit, ptr::NonNull};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use async_trait::async_trait;
use kernel_lib::{await_once_noblocking, await_sync};
use spin::Mutex;
use usb_host::{
    ConfigurationDescriptor, DescriptorType, DeviceDescriptor, EndpointDescriptor, SetupPacket,
};
use xhci::{
    accessor::Mapper,
    context::{
        Device32Byte, EndpointHandler, EndpointType, Input32Byte, InputControl32Byte, SlotHandler,
    },
    ring::trb::{
        command, event,
        transfer::{self, TransferType},
    },
};

use crate::{
    alloc::alloc::{alloc_with_boundary_with_default_else, GlobalAllocator},
    usb::{
        descriptor::DescriptorIter,
        setup_packet::{SetupPacketRaw, SetupPacketWrapper},
        traits::AsyncUSBHost,
    },
    xhci::{
        command_ring::CommandRing, event_ring::EventRing, transfer_ring::TransferRing, trb::TrbRaw,
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
pub struct DeviceContextInfo<M: Mapper + Clone, A: Allocator> {
    registers: Arc<Mutex<xhci::Registers<M>>>,
    event_ring: Arc<Mutex<EventRing<A>>>,
    command_ring: Arc<Mutex<CommandRing>>,
    slot_id: usize,
    port_index: usize,
    descriptors: Option<Vec<Descriptor>>,
    pub input_context: Box<InputContextWrapper, A>,
    pub device_context: Box<DeviceContextWrapper, A>,
    // pub event_waiting_issuer_map: BTreeMap<SetupPacketWrapper, Box<dyn ClassDriver>>,
    transfer_rings: [Option<Box<TransferRing<A>, A>>; 31],
}

impl<M: Mapper + Clone> DeviceContextInfo<M, &'static GlobalAllocator> {
    pub fn new(
        port_index: usize,
        slot_id: usize,
        registers: Arc<Mutex<xhci::Registers<M>>>,
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        command_ring: Arc<Mutex<CommandRing>>,
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
        transfer_rings[0] = Some(TransferRing::alloc_new(TRANSFER_RING_BUF_SIZE));
        Self {
            registers,
            event_ring,
            command_ring,
            slot_id,
            port_index,
            descriptors: None,
            input_context: InputContextWrapper::new(), // 0 filled
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
        control.set_add_context_flag(0);
    }

    pub fn enable_endpoint(&mut self, dci: DeviceContextIndex) {
        use xhci::context::InputHandler;
        let control = self.input_context.0.control_mut();
        control.set_add_context_flag(dci.address() as usize);
    }

    pub fn initialize_slot_context(&mut self, port_id: u8, port_speed: u8) {
        use xhci::context::InputHandler;
        log::debug!("initialize_slot_context: port_id: {}", port_id);
        let slot_context = self.input_context.0.device_mut().slot_mut();
        slot_context.set_route_string(0);
        slot_context.set_root_hub_port_number(port_id);
        slot_context.set_context_entries(1);
        slot_context.set_speed(port_speed);
    }

    pub fn slot_context(&self) -> &dyn SlotHandler {
        use xhci::context::InputHandler;
        self.input_context.0.device().slot()
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
        use xhci::context::InputHandler;
        let endpoint_context_0_id = DeviceContextIndex::ep0();
        let endpoint0_context = self
            .input_context
            .0
            .device_mut()
            .endpoint_mut(endpoint_context_0_id.address() as usize);

        log::debug!("max_packet_size: {}", max_packet_size);
        endpoint0_context.set_endpoint_type(EndpointType::Control);
        endpoint0_context.set_max_packet_size(max_packet_size);
        endpoint0_context.set_max_burst_size(0);
        endpoint0_context.set_tr_dequeue_pointer(transfer_ring_dequeue_pointer);
        endpoint0_context.set_dequeue_cycle_state();
        endpoint0_context.set_interval(0);
        endpoint0_context.set_max_primary_streams(0);
        endpoint0_context.set_mult(0);
        endpoint0_context.set_error_count(3);
    }

    pub async fn start_initialization<MF, KF>(
        &mut self,
        class_drivers: &mut ClassDriverManager<MF, KF>,
    ) where
        MF: Fn(u8, &[u8]),
        KF: Fn(u8, &[u8]),
    {
        let device_descriptor = self.request_device_descriptor().await;
        let buffer_len = self
            .transfer_ring_at(DeviceContextIndex::ep0())
            .as_ref()
            .unwrap()
            .buffer_len();
        for _ in 0..buffer_len {
            // ensure transfer ring is correctly working.
            let _ = self.request_device_descriptor().await;
        }
        let descriptors = self.request_config_descriptor_and_rest().await;
        log::debug!("descriptors requested with config: {:?}", descriptors);
        if device_descriptor.b_device_class == 0 {
            let mut boot_keyboard_interface = None;
            let mut mouse_interface = None;
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
                let _address = self.device_address();
                log::warn!("boot keyboard interface ignored");
                // class_drivers
                //     .add_keyboard_device(self.slot_id(), device_descriptor, address)
                //     .unwrap();
            } else {
                log::warn!("no book keyboard interface found");
            }

            if let Some(_mouse_interface) = mouse_interface {
                let address = self.device_address();
                class_drivers
                    .add_mouse_device(self.slot_id(), device_descriptor, address)
                    .unwrap();
            } else {
                log::warn!("no mouse interface found");
            }
        } else {
            log::warn!("unknown device class: {}", device_descriptor.b_device_class);
        }
    }

    /// Host to Device
    pub fn push_control_transfer(
        &mut self,
        endpoint_id: EndpointId,
        setup_data: SetupPacketWrapper,
        buf: Option<NonNull<[u8]>>,
    ) -> u64 {
        let dci: DeviceContextIndex = endpoint_id.address();
        log::debug!(
            "control_in: setup_data: {:?}, ep addr: {:?}",
            &setup_data,
            &dci
        );

        let transfer_ring = self
            .transfer_ring_at_mut(dci)
            .as_mut()
            .expect("transfer ring not allocated")
            .as_mut();

        let setup_data = SetupPacketRaw::from(setup_data.0);
        let mut status_trb = transfer::StatusStage::new();
        let wait_on = if let Some(buf) = buf {
            let buf = unsafe { buf.as_ref() };
            let mut setup_stage_trb = transfer::SetupStage::new();
            setup_stage_trb
                .set_request_type(setup_data.bm_request_type)
                .set_request(setup_data.b_request)
                .set_value(setup_data.w_value)
                .set_index(setup_data.w_index)
                .set_length(setup_data.w_length)
                .set_transfer_type(TransferType::In);
            transfer_ring.push(transfer::Allowed::SetupStage(setup_stage_trb));

            let mut data_stage_trb = transfer::DataStage::new();
            data_stage_trb
                .set_trb_transfer_length(buf.len() as u32)
                .set_data_buffer_pointer(buf.as_ptr() as u64)
                .set_td_size(0)
                .set_direction(transfer::Direction::In)
                .set_interrupt_on_completion();

            let data_trb_ref_in_ring =
                transfer_ring.push(transfer::Allowed::DataStage(data_stage_trb)) as u64;

            transfer_ring.push(transfer::Allowed::StatusStage(status_trb));
            data_trb_ref_in_ring
        } else {
            let mut setup_stage_trb = transfer::SetupStage::new();
            setup_stage_trb
                .set_request_type(setup_data.bm_request_type)
                .set_request(setup_data.b_request)
                .set_value(setup_data.w_value)
                .set_index(setup_data.w_index)
                .set_length(setup_data.w_length)
                .set_transfer_type(TransferType::No);
            transfer_ring.push(transfer::Allowed::SetupStage(setup_stage_trb));

            status_trb.set_direction().set_interrupt_on_completion();
            transfer_ring.push(transfer::Allowed::StatusStage(status_trb)) as u64
        };

        let mut registers = self.registers.lock();
        log::debug!(
            "slot_id: {:?}, dci.address(): {}",
            self.slot_id,
            dci.address()
        );

        registers
            .doorbell
            .update_volatile_at(self.slot_id(), |ring| {
                ring.set_doorbell_target(dci.address());
                ring.set_doorbell_stream_id(0);
            });
        wait_on
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
        log::debug!("w_length: {}", w_length);
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
            let mut registers = self.registers.lock();
            let mut interrupter = registers.interrupter_register_set.interrupter_mut(0);
            EventRing::get_received_transfer_trb_on_trb(event_ring, &mut interrupter, trb_wait_on)
                .await
        };
        match trb.completion_code() {
            Ok(event::CompletionCode::ShortPacket) => {}
            Ok(event::CompletionCode::Success) => {
                return Ok(w_length as usize);
            }
            Ok(err) => {
                log::error!("err: {:?}", err);
                return Err(usb_host::TransferError::Permanent("CompletionCode error"));
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

    async fn async_in_transfer(
        &mut self,
        ep: &mut (dyn usb_host::Endpoint + Send + Sync),
        buf: &mut [u8],
    ) -> Result<usize, usb_host::TransferError> {
        use xhci::context::InputHandler;
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
        let portsc = {
            let registers = self.registers.lock();
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
                        endpoint_context.set_average_trb_length(8); // TODO: set this correctly
                        endpoint_context.set_max_burst_size(0);
                        endpoint_context.set_max_primary_streams(0);
                        endpoint_context.set_max_endpoint_service_time_interval_payload_low(
                            ep.max_packet_size(),
                        );
                        endpoint_context.set_mult(0);
                        let interval = match portsc.port_speed() {
                        1 /* FullSpeed */ | 2 /* LowSpeed */ => endpoint_descriptor.b_interval.ilog2() as u8 + 3,
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
                    let mut command_ring = self.command_ring.lock();
                    command_ring.push(command::Allowed::ConfigureEndpoint(trb))
                } as u64;
                let event_ring = Arc::clone(&self.event_ring);
                let mut registers = self.registers.lock();
                registers.doorbell.update_volatile_at(0, |doorbell| {
                    doorbell.set_doorbell_target(0);
                    doorbell.set_doorbell_stream_id(0);
                });
                let mut interrupter = registers.interrupter_register_set.interrupter_mut(0);
                EventRing::get_received_command_trb(event_ring, &mut interrupter, trb_ptr).await
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
        }

        let event_ring = Arc::clone(&self.event_ring);

        let transfer_ring = self.transfer_ring_at_mut(dci).as_mut().unwrap();
        let mut normal = transfer::Normal::new();
        normal
            .set_interrupt_on_completion()
            .set_interrupt_on_short_packet()
            .set_interrupter_target(0)
            .set_data_buffer_pointer(buf.as_ptr() as u64)
            .set_trb_transfer_length(buf.len() as u32)
            .set_td_size(0);
        transfer_ring.push(transfer::Allowed::Normal(normal));
        transfer_ring.dump_state();

        let mut registers = self.registers.lock();
        registers
            .doorbell
            .update_volatile_at(self.slot_id(), |doorbell| {
                doorbell.set_doorbell_target(dci.address());
                doorbell.set_doorbell_stream_id(0);
            });
        let mut interrupter = registers.interrupter_register_set.interrupter_mut(0);
        let slot_id = self.slot_id();
        let trb = EventRing::get_received_transfer_trb_on_slot(
            event_ring,
            &mut interrupter,
            slot_id as u8,
        )
        .await;
        let transferred_length = trb.trb_transfer_length();
        // let transfer_trb = transfer::Allowed::try_from(unsafe {
        //     (trb.trb_pointer() as *const TrbRaw).read_volatile()
        // })
        // .unwrap();
        // match transfer_trb {
        //     transfer::Allowed::SetupStage(_) => todo!(),
        //     transfer::Allowed::DataStage(_) => todo!(),
        //     transfer::Allowed::StatusStage(_) => todo!(),
        //     transfer::Allowed::Isoch(_) => todo!(),
        //     transfer::Allowed::Link(_) => todo!(),
        //     transfer::Allowed::EventData(_) => todo!(),
        //     transfer::Allowed::Noop(_) => todo!(),
        //     transfer::Allowed::Normal(normal) => {
        //         let buffer = unsafe {
        //             core::slice::from_raw_parts(
        //                 normal.data_buffer_pointer() as *const u8,
        //                 transferred_length as usize,
        //             )
        //         };
        //         buf.copy_from_slice(buffer);
        //     }
        // };
        Ok(transferred_length as usize)
    }
}

impl<M: Mapper + Clone> DeviceContextInfo<M, &'static GlobalAllocator> {
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
        let length = self
            .async_control_transfer(
                &mut endpoint_id,
                bm_request_type,
                b_request,
                w_value,
                w_index,
                Some(buf),
            )
            .await
            .unwrap();
        log::debug!("Transferred {} bytes", length);
        length
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

impl<M: Mapper + Clone> usb_host::USBHost for DeviceContextInfo<M, &'static GlobalAllocator> {
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
        await_once_noblocking!(self.async_control_transfer(
            unsafe { core::mem::transmute(ep) },
            bm_request_type,
            b_request,
            w_value,
            w_index,
            buf
        ))
        .unwrap_or(Err(usb_host::TransferError::Retry("transfer is pending")))
    }

    fn in_transfer(
        &mut self,
        ep: &mut dyn usb_host::Endpoint,
        buf: &mut [u8],
    ) -> Result<usize, usb_host::TransferError> {
        // await_once_noblocking!(self.async_in_transfer(unsafe { core::mem::transmute(ep) }, buf))
        //     .unwrap_or(Err(usb_host::TransferError::Retry("transfer is pending")))
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
impl<M: Mapper + Clone + Sync + Send> AsyncUSBHost
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
}
