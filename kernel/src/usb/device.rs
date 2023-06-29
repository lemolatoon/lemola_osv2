extern crate alloc;
use alloc::{
    boxed::Box,
    collections::{btree_map, BTreeMap},
};
use usb_host::{DescriptorType, DeviceDescriptor, Endpoint};
use xhci::context::{Device32Byte, EndpointHandler, Input32Byte, SlotHandler};

use crate::usb::setup_packet::SetupPacketWrapper;

use super::class_driver::ClassDriver;

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct InputContextWrapper(Input32Byte);

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct DeviceContextWrapper(pub Device32Byte);

#[derive(Debug)]
pub struct DeviceContextInfo {
    slot_id: usize,
    state: DeviceContextState,
    pub initialization_state: DeviceInitializationState,
    pub input_context: InputContextWrapper,
    pub device_context: DeviceContextWrapper,
    pub buf: [u8; 256],
    pub event_waiting_issuer_map: BTreeMap<SetupPacketWrapper, Box<dyn ClassDriver>>,
}

impl DeviceContextInfo {
    pub fn blank(slot_id: usize) -> Self {
        Self {
            slot_id,
            state: DeviceContextState::Blank,
            initialization_state: DeviceInitializationState::NotInitialized,
            input_context: InputContextWrapper(Input32Byte::new_32byte()), // 0 filled
            device_context: DeviceContextWrapper(Device32Byte::new_32byte()), // 0 filled
            buf: [0; 256],
            event_waiting_issuer_map: BTreeMap::new(),
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

    pub fn start_initialization(&mut self) {
        debug_assert_eq!(
            self.initialization_state,
            DeviceInitializationState::NotInitialized
        );
        self.initialization_state = DeviceInitializationState::Initialize1;
        self.get_descriptor(
            EndpointId::default_control_pipe(),
            DescriptorType::Device,
            0,
        );
    }

    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    pub fn get_descriptor(
        &mut self,
        endpoint_id: EndpointId,
        descriptor_type: DescriptorType,
        descriptor_index: u8,
    ) {
        let setup_data = SetupPacketWrapper::descriptor(
            descriptor_type,
            descriptor_index,
            self.buf_len() as u16,
        );
        self.control_in(endpoint_id, setup_data, None);
    }

    pub fn control_in(
        &mut self,
        endpoint_id: EndpointId,
        setup_data: SetupPacketWrapper,
        issuer: Option<Box<dyn ClassDriver>>,
    ) {
        if let Some(issuer) = issuer {
            let entry = self.event_waiting_issuer_map.entry(setup_data);
            match entry {
                btree_map::Entry::Vacant(entry) => entry.insert(issuer),
                btree_map::Entry::Occupied(_) => panic!("same setup packet already issued"),
            };
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EndpointId {
    endpoint_number: u8,
    direct: usb_host::Direction,
}
#[derive(Clone, Copy, Debug, PartialEq, Ord, PartialOrd, Eq)]
pub struct DeviceContextIndex(u8);
impl DeviceContextIndex {
    pub const fn new(endpoint_number: u8, direct: usb_host::Direction) -> Self {
        calc_dci(endpoint_number, direct)
    }

    pub const fn address(&self) -> u8 {
        self.0
    }

    pub const fn ep0() -> Self {
        Self(1)
    }
}

pub const fn calc_dci(endpoint_number: u8, direct: usb_host::Direction) -> DeviceContextIndex {
    let index = endpoint_number * 2 + if endpoint_number == 0 { 1 } else { 0 };
    DeviceContextIndex(index)
}

impl EndpointId {
    pub fn new(endpoint_number: u8, direct_in: usb_host::Direction) -> Self {
        Self {
            endpoint_number,
            direct: direct_in,
        }
    }

    pub const fn default_control_pipe() -> Self {
        Self {
            endpoint_number: 0,
            direct: usb_host::Direction::Out, // actually default control pipe is IN/OUT
        }
    }
}

impl Endpoint for EndpointId {
    fn address(&self) -> u8 {
        calc_dci(self.endpoint_num(), self.direction()).address()
    }

    fn endpoint_num(&self) -> u8 {
        self.endpoint_number
    }

    fn transfer_type(&self) -> usb_host::TransferType {
        todo!()
    }

    fn direction(&self) -> usb_host::Direction {
        self.direct
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
        match self {
            Self::Initialized => true,
            _ => false,
        }
    }

    pub fn is_initializing(&self) -> bool {
        match self {
            Self::NotInitialized | Self::Initialized => false,
            _ => true,
        }
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
