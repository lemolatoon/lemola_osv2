extern crate alloc;
use core::mem::MaybeUninit;

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};
use spin::Mutex;
use static_assertions::assert_eq_size_ptr;
use usb_host::DescriptorType;
use xhci::{
    accessor::Mapper,
    context::{Device32Byte, EndpointHandler, Input32Byte, SlotHandler},
    ring::trb::{
        event,
        transfer::{self, TransferType},
    },
};

use crate::{
    usb::setup_packet::{SetupPacketRaw, SetupPacketWrapper},
    xhci::{transfer_ring::TransferRing, trb::TrbRaw},
};

use super::class_driver::ClassDriver;

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct InputContextWrapper(Input32Byte);

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct DeviceContextWrapper(pub Device32Byte);

#[derive(Debug)]
pub struct DeviceContextInfo<M: Mapper + Clone> {
    registers: Arc<Mutex<xhci::Registers<M>>>,
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
    pub fn blank(slot_id: usize, registers: Arc<Mutex<xhci::Registers<M>>>) -> Self {
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
        let buf = &mut self.buf as *mut [u8]; // FIXME: use rusty way to pass this buffer
        self.control_in(endpoint_id, setup_data, buf, None);
        log::debug!("end get_descriptor");
    }

    /// Host to Device
    pub fn control_in(
        &mut self,
        endpoint_id: EndpointId,
        setup_data: SetupPacketWrapper,
        buf: *mut [u8],
        issuer: Option<Box<dyn ClassDriver>>,
    ) {
        // if let Some(issuer) = issuer {
        //     let entry = self.event_waiting_issuer_map.entry(setup_data);
        //     match entry {
        //         btree_map::Entry::Vacant(entry) => entry.insert(issuer),
        //         btree_map::Entry::Occupied(_) => panic!("same setup packet already issued"),
        //     };
        // }
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
        if let Some(buf) = unsafe { buf.as_ref() } {
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
            trb @ (transfer::Allowed::StatusStage(_) | transfer::Allowed::DataStage(_)) => {
                let setup_trb_ref_in_ring = self
                    .setup_stage_map
                    .remove(&trb_pointer)
                    .expect("setup stage trb not found");
                let transfer::Allowed::SetupStage(setup_stage): transfer::Allowed =
                    unsafe { (setup_trb_ref_in_ring as *const TrbRaw).read() }
                        .try_into()
                        .unwrap() else {
                            log::error!("there must be setup stage. at {:?}", trb_pointer);
                            panic!("there must be setup stage. at {:?}", trb_pointer);
                        };
            }
        }
        todo!()
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
    let index = endpoint_number * 2 + if endpoint_number == 0 { 1 } else { 0 };
    return index;
}

impl EndpointId {
    pub fn new(endpoint_number: u8, direct_in: usb_host::Direction) -> Self {
        assert!(endpoint_number < 16);
        Self {
            endpoint_number,
            direct: direct_in,
        }
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
