extern crate alloc;
use core::{mem::MaybeUninit, pin::Pin, ptr::NonNull};

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use kernel_lib::await_sync;
use spin::Mutex;
use usb_host::{ConfigurationDescriptor, DescriptorType, DeviceDescriptor, SetupPacket};
use xhci::{
    accessor::Mapper,
    context::{Device32Byte, EndpointHandler, Input32Byte, SlotHandler},
    ring::trb::{
        event,
        transfer::{self, TransferType},
    },
};

use crate::{
    usb::{
        descriptor::DescriptorIter,
        device,
        setup_packet::{SetupPacketRaw, SetupPacketWrapper},
    },
    xhci::{
        event_ring::EventRing,
        transfer_ring::TransferRing,
        trb::{self, TrbRaw},
    },
};

use super::{class_driver::ClassDriver, descriptor::Descriptor};

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct InputContextWrapper(Input32Byte);

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct DeviceContextWrapper(pub Device32Byte);

#[derive(Debug)]
pub struct DeviceContextInfo<M: Mapper + Clone> {
    registers: Arc<Mutex<xhci::Registers<M>>>,
    event_ring: Arc<Mutex<EventRing>>,
    slot_id: usize,
    state: DeviceContextState,
    pub initialization_state: DeviceInitializationState,
    pub input_context: InputContextWrapper,
    pub device_context: DeviceContextWrapper,
    pub buf: [u8; 256],
    // pub event_waiting_issuer_map: BTreeMap<SetupPacketWrapper, Box<dyn ClassDriver>>,
    transfer_rings: [Option<Box<TransferRing>>; 31],
    /// DataStageTRB | StatusStageTRB -> SetupStageTRB
    setup_stage_map: BTreeMap<u64, u64>,
}

impl<M: Mapper + Clone> DeviceContextInfo<M> {
    pub fn blank(
        slot_id: usize,
        registers: Arc<Mutex<xhci::Registers<M>>>,
        event_ring: Arc<Mutex<EventRing>>,
    ) -> Self {
        const TRANSFER_RING_BUF_SIZE: usize = 32;
        let mut transfer_rings: [MaybeUninit<Option<Box<TransferRing>>>; 31] =
            MaybeUninit::uninit_array();
        for transfer_ring in transfer_rings.iter_mut() {
            unsafe {
                transfer_ring.as_mut_ptr().write(None);
            }
        }
        let mut transfer_rings = unsafe {
            // assume init
            core::mem::transmute::<_, [Option<Box<TransferRing>>; 31]>(transfer_rings)
        };
        transfer_rings[0] = Some(TransferRing::alloc_new(TRANSFER_RING_BUF_SIZE));
        Self {
            registers,
            event_ring,
            slot_id,
            state: DeviceContextState::Blank,
            initialization_state: DeviceInitializationState::NotInitialized,
            input_context: InputContextWrapper(Input32Byte::new_32byte()), // 0 filled
            device_context: DeviceContextWrapper(Device32Byte::new_32byte()), // 0 filled
            buf: [0; 256],
            // event_waiting_issuer_map: BTreeMap::new(),
            transfer_rings,
            setup_stage_map: BTreeMap::new(),
        }
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

    pub fn transfer_ring_at(&self, dci: DeviceContextIndex) -> &Option<Box<TransferRing>> {
        &self.transfer_rings[dci.address() as usize - 1]
    }

    pub fn transfer_ring_at_mut(
        &mut self,
        dci: DeviceContextIndex,
    ) -> &mut Option<Box<TransferRing>> {
        &mut self.transfer_rings[dci.address() as usize - 1]
    }

    pub fn initialize_endpoint0_context(
        &mut self,
        transfer_ring_dequeue_pointer: u64,
        max_packet_size: u16,
    ) {
        use xhci::context::EndpointType;
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

    pub async fn start_initialization(&mut self) {
        debug_assert_eq!(
            self.initialization_state,
            DeviceInitializationState::NotInitialized
        );
        self.initialization_state = DeviceInitializationState::Initialize1;
        let device_descriptor = self.request_device_descriptor().await;
        log::debug!("device_descriptor: {:?}", device_descriptor);
        let descriptors = self.request_config_descriptor_and_rest().await;
        log::debug!("descriptors requested with config: {:?}", descriptors);
        if device_descriptor.b_device_class == 0 {
            let mut book_keyboard_interface = None;
            let mut mouse_interface = None;
            for desc in descriptors {
                if let Descriptor::Interface(interface) = desc {
                    match (
                        interface.b_interface_class,
                        interface.b_interface_sub_class,
                        interface.b_interface_protocol,
                    ) {
                        (3, 1, 1) => {
                            log::debug!("HID boot keyboard interface found");
                            book_keyboard_interface = Some(interface);
                        }
                        (3, 1, 2) => {
                            log::debug!("HID mouse interface found");
                            mouse_interface = Some(interface);
                        }
                        unknown => {
                            log::debug!("unknown interface found: {:?}", unknown);
                        }
                    };
                }
            }
            if book_keyboard_interface.is_none() {
                log::warn!("no book keyboard interface found");
            }

            if mouse_interface.is_none() {
                log::warn!("no mouse interface found");
            }
        } else {
            log::warn!("unknown device class: {}", device_descriptor.b_device_class);
        }
        todo!()
    }

    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    /// Host to Device
    pub fn push_control_transfer(
        &mut self,
        endpoint_id: EndpointId,
        setup_data: SetupPacketWrapper,
        buf: Option<NonNull<[u8]>>,
    ) {
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
        if let Some(buf) = buf {
            let buf = unsafe { buf.as_ref() };
            let mut setup_stage_trb = transfer::SetupStage::new();
            setup_stage_trb
                .set_request_type(setup_data.bm_request_type)
                .set_request(setup_data.b_request)
                .set_value(setup_data.w_value)
                .set_index(setup_data.w_index)
                .set_length(setup_data.w_length)
                .set_transfer_type(TransferType::In);
            let setup_trb_ref_in_ring =
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

            transfer_ring.push(transfer::Allowed::StatusStage(status_trb));
            log::debug!(
                "control_in: setup_trb_ref_in_ring: {:?}, data_trb_ref_in_ring: {:?}",
                setup_trb_ref_in_ring,
                data_trb_ref_in_ring
            );
            self.setup_stage_map
                .insert(data_trb_ref_in_ring, setup_trb_ref_in_ring);
        } else {
            let mut setup_stage_trb = transfer::SetupStage::new();
            setup_stage_trb
                .set_request_type(setup_data.bm_request_type)
                .set_request(setup_data.b_request)
                .set_value(setup_data.w_value)
                .set_index(setup_data.w_index)
                .set_length(setup_data.w_length)
                .set_transfer_type(TransferType::No);
            let setup_trb_ref_in_ring =
                transfer_ring.push(transfer::Allowed::SetupStage(setup_stage_trb)) as u64;

            status_trb.set_direction().set_interrupt_on_completion();
            let status_trb_ref_in_ring =
                transfer_ring.push(transfer::Allowed::StatusStage(status_trb)) as u64;

            self.setup_stage_map
                .insert(status_trb_ref_in_ring, setup_trb_ref_in_ring);
        }

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
            })
    }

    pub fn on_transfer_event_received(&mut self, event: event::TransferEvent) {
        let trb_pointer = event.trb_pointer();
        log::debug!("trb_pointer: {}", trb_pointer);
        if trb_pointer == 0 {
            log::debug!("Invalid trb_pointer: null");
            return;
        }
        let trb: transfer::Allowed = unsafe { (trb_pointer as *const TrbRaw).read() }
            .try_into()
            .unwrap();
        match trb {
            transfer::Allowed::Normal(_) => todo!("normal"),
            transfer::Allowed::Isoch(_) => todo!("isoch"),
            transfer::Allowed::Link(_) => todo!("link"),
            transfer::Allowed::EventData(_) => todo!("event data"),
            transfer::Allowed::Noop(_) => todo!("noop"),
            transfer::Allowed::SetupStage(_) => todo!("setup stage"),
            transfer::Allowed::StatusStage(_) | transfer::Allowed::DataStage(_) => {
                log::debug!("setup_stage_map: {:?}", &self.setup_stage_map);
                let setup_stage = {
                    let setup_trb_ref_in_ring = self
                        .setup_stage_map
                        .remove(&trb_pointer)
                        .expect("setup stage trb not found")
                        as *const TrbRaw;
                    let transfer::Allowed::SetupStage(setup_stage): transfer::Allowed =
                        unsafe { setup_trb_ref_in_ring.read() }.try_into().unwrap()
                    else {
                        log::error!("there must be setup stage. at {:?}", trb_pointer);
                        panic!("there must be setup stage. at {:?}", trb_pointer);
                    };
                    setup_stage
                };

                log::debug!("setup_stage: {:?}", setup_stage);
                let mut device_descriptor: MaybeUninit<DeviceDescriptor> = MaybeUninit::uninit();
                let device_descriptor = unsafe {
                    device_descriptor
                        .as_mut_ptr()
                        .copy_from_nonoverlapping(self.buf.as_ptr().cast(), 1);
                    device_descriptor.assume_init()
                };
                log::debug!("device_descriptor: {:?}", device_descriptor);
            }
        }
        todo!()
    }

    pub async fn async_control_transfer(
        &mut self,
        ep: &mut dyn usb_host::Endpoint,
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
        self.push_control_transfer(endpoint_id, setup_packet, buf.map(|buf| buf[..].into()));
        let event_ring = Arc::clone(&self.event_ring);
        let trb = {
            let mut registers = self.registers.lock();
            let mut interrupter = registers.interrupter_register_set.interrupter_mut(0);
            EventRing::get_received_transfer_trb(event_ring, &mut interrupter).await
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
        return Ok(w_length as usize - trb.trb_transfer_length() as usize);
    }
}

impl<M: Mapper + Clone> DeviceContextInfo<M> {
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
        DescriptorIter::new(&buf).collect()
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
        return length;
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

pub const fn calc_dci(endpoint_number: u8, direct: usb_host::Direction) -> u8 {
    endpoint_number * 2 + if endpoint_number == 0 { 1 } else { 0 }
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

    fn set_in_toggle(&mut self, toggle: bool) {
        todo!()
    }

    fn out_toggle(&self) -> bool {
        todo!()
    }

    fn set_out_toggle(&mut self, toggle: bool) {
        todo!()
    }
}

impl<M: Mapper + Clone> usb_host::USBHost for DeviceContextInfo<M> {
    fn control_transfer(
        &mut self,
        ep: &mut dyn usb_host::Endpoint,
        bm_request_type: usb_host::RequestType,
        b_request: usb_host::RequestCode,
        w_value: usb_host::WValue,
        w_index: u16,
        buf: Option<&mut [u8]>,
    ) -> Result<usize, usb_host::TransferError> {
        todo!()
    }

    fn in_transfer(
        &mut self,
        ep: &mut dyn usb_host::Endpoint,
        buf: &mut [u8],
    ) -> Result<usize, usb_host::TransferError> {
        todo!()
    }

    fn out_transfer(
        &mut self,
        ep: &mut dyn usb_host::Endpoint,
        buf: &[u8],
    ) -> Result<usize, usb_host::TransferError> {
        todo!()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DeviceInitializationState {
    NotInitialized,
    Initialize1,
    Initialize2,
    Initialize3,
    Initialized,
}

impl DeviceInitializationState {
    pub fn next(&self) -> Self {
        match self {
            Self::NotInitialized => Self::Initialize1,
            Self::Initialize1 => Self::Initialize2,
            Self::Initialize2 => Self::Initialize3,
            Self::Initialize3 => Self::Initialized,
            Self::Initialized => Self::Initialized,
        }
    }

    pub fn is_initialized(&self) -> bool {
        matches!(self, Self::Initialized)
    }

    pub fn is_initializing(&self) -> bool {
        !matches!(self, Self::NotInitialized | Self::Initialized)
    }

    pub fn advance(&mut self) {
        *self = self.next();
    }
}

#[derive(Debug, Clone, Copy)]
enum DeviceContextState {
    Invalid,
    Blank,
    SlotAssigning,
    SlotAssigned,
}
