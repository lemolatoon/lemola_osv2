extern crate alloc;
use core::panic;

use alloc::vec;
use alloc::{boxed::Box, vec::Vec};
use static_assertions::assert_eq_size;
use xhci::context::{
    Device32Byte, Device64Byte, EndpointHandler, Input32Byte, Input64Byte, SlotHandler,
};

use crate::alloc::alloc::alloc_array_with_boundary_with_default_else;
use crate::memory::PAGE_SIZE;

#[derive(Debug)]
pub struct DeviceManager {
    /// len is max_slots_enabled
    device_context_array: DeviceContextArray,
}

impl DeviceManager {
    pub fn new(max_slots: u8) -> Self {
        Self {
            device_context_array: DeviceContextArray::new(max_slots),
        }
    }

    pub fn set_scratchpad_buffer_array(&mut self, ptr_head: Box<[*mut [u8; PAGE_SIZE]]>) {
        // This pointer cast is safe, because it is based on xhci specification
        self.device_context_array.device_contexts[0] =
            Box::leak(ptr_head) as *mut [*mut [u8; PAGE_SIZE]] as *mut Device32Byte;
    }

    pub fn get_device_contexts_head_ptr(&mut self) -> *mut *mut Device32Byte {
        self.device_context_array.device_contexts.as_mut_ptr()
    }

    pub fn allocate_device(&mut self, slot_id: usize) -> &mut DeviceContextInfo {
        if slot_id > self.device_context_array.max_slots() {
            log::error!(
                "slot_id is out of range: {} / {}",
                slot_id,
                self.device_context_array.max_slots()
            );
            panic!("slot_id is out of range");
        }

        if self.device_context_array.device_context_infos[slot_id].is_some() {
            log::error!("device context at {} is already allocated", slot_id);
            panic!("device context at {} is already allocated", slot_id);
        }

        self.device_context_array.device_context_infos[slot_id] =
            Some(DeviceContextInfo::blank(slot_id));
        self.device_context_array.device_context_infos[slot_id]
            .as_mut()
            .unwrap()
    }

    pub fn device_by_slot_id(&self, slot_id: usize) -> Option<&DeviceContextInfo> {
        self.device_context_array.device_context_infos[slot_id].as_ref()
    }

    pub fn device_by_slot_id_mut(&mut self, slot_id: usize) -> Option<&mut DeviceContextInfo> {
        self.device_context_array.device_context_infos[slot_id].as_mut()
    }

    pub fn load_device_context(&mut self, slot_id: usize) {
        if slot_id > self.device_context_array.max_slots() {
            log::error!("Invalid slot_id: {}", slot_id);
            panic!("Invalid slot_id: {}", slot_id);
        }
        let device_context_info = self.device_context_array.device_context_infos[slot_id]
            .as_mut()
            .unwrap();
        self.device_context_array.device_contexts[slot_id] =
            &mut device_context_info.device_context.0 as *mut Device32Byte;
    }
}

#[derive(Debug)]
struct DeviceContextArray {
    device_contexts: Box<[*mut Device32Byte]>,
    device_context_infos: Vec<Option<DeviceContextInfo>>,
}

impl DeviceContextArray {
    pub fn new(max_slots: u8) -> Self {
        let device_contexts_len = max_slots as usize + 1;
        const ALIGNMENT: usize = 64;
        // allow this because xhci specification says we shall initialized with 0
        #[allow(clippy::zero_ptr)]
        let device_contexts = alloc_array_with_boundary_with_default_else(
            device_contexts_len,
            ALIGNMENT,
            PAGE_SIZE,
            || 0 as *mut Device32Byte,
        )
        .expect("DeviceContextArray allocation failed");
        let device_context_infos = vec![None; device_contexts_len];
        Self {
            device_contexts,
            device_context_infos,
        }
    }

    pub fn max_slots(&self) -> usize {
        self.device_contexts.len() - 1
    }
}

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct InputContextWrapper(Input32Byte);

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct DeviceContextWrapper(pub Device32Byte);

#[derive(Debug, Clone)]
pub struct DeviceContextInfo {
    slot_id: usize,
    state: DeviceContextState,
    pub initialization_state: DeviceInitializationState,
    pub input_context: InputContextWrapper,
    pub device_context: DeviceContextWrapper,
}

impl DeviceContextInfo {
    pub fn blank(slot_id: usize) -> Self {
        Self {
            slot_id,
            state: DeviceContextState::Blank,
            initialization_state: DeviceInitializationState::NotInitialized,
            input_context: InputContextWrapper(Input32Byte::new_32byte()), // 0 filled
            device_context: DeviceContextWrapper(Device32Byte::new_32byte()), // 0 filled
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

    pub fn enable_endpoint(&mut self, endpoint_id: EndpointId) {
        use xhci::context::InputHandler;
        let control = self.input_context.0.control_mut();
        control.set_add_context_flag(endpoint_id.address());
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

    pub fn endpoint_context(&self, endpoint_id: EndpointId) -> &dyn EndpointHandler {
        use xhci::context::InputHandler;
        self.input_context
            .0
            .device()
            .endpoint(endpoint_id.address())
    }

    pub fn endpoint_context_mut(&mut self, endpoint_id: EndpointId) -> &mut dyn EndpointHandler {
        use xhci::context::InputHandler;
        self.input_context
            .0
            .device_mut()
            .endpoint_mut(endpoint_id.address())
    }

    pub fn initialize_endpoint0_context(
        &mut self,
        transfer_ring_dequeue_pointer: u64,
        max_packet_size: u16,
    ) {
        use xhci::context::EndpointType;
        use xhci::context::InputHandler;
        let endpoint_context_0_id = EndpointId::new(0, false);
        let endpoint0_context = self
            .input_context
            .0
            .device_mut()
            .endpoint_mut(endpoint_context_0_id.address());

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
        // self.get_descriptor(
        //     EndpointId::default_control_pipe(),
        //     descriptor_type,
        //     descriptor_index,
        // )
        todo!("get descriptor")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointId {
    address: usize,
}

impl EndpointId {
    pub fn new(endpoint_number: usize, direct_in: bool) -> Self {
        let address = endpoint_number * 2
            + if endpoint_number == 0 {
                1
            } else {
                direct_in as usize
            };
        Self { address }
    }

    pub fn address(&self) -> usize {
        self.address
    }

    pub const fn default_control_pipe() -> Self {
        Self { address: 1 }
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
