use xhci::context::{Device32Byte, EndpointHandler, Input32Byte, SlotHandler};

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
