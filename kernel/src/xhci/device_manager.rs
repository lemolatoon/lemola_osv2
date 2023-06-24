extern crate alloc;
use core::panic;

use alloc::vec;
use alloc::{boxed::Box, vec::Vec};
use xhci::context::{Device64Byte, Input64Byte};
use xhci::registers::operational::PortStatusAndControlRegister;

use crate::alloc::alloc::{
    alloc_array_with_boundary_with_default_else, alloc_with_boundary, alloc_with_boundary_raw,
    alloc_with_boundary_with_default_else,
};
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
            Box::leak(ptr_head) as *mut [*mut [u8; PAGE_SIZE]] as *mut Device64Byte;
    }

    pub fn get_device_contexts_head_ptr(&mut self) -> *mut *mut Device64Byte {
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
}

#[derive(Debug)]
struct DeviceContextArray {
    device_contexts: Box<[*mut Device64Byte]>,
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
            || 0 as *mut Device64Byte,
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
pub struct DeviceContextInfo {
    slot_id: usize,
    state: DeviceContextState,
    pub input_context: Input64Byte,
}

impl DeviceContextInfo {
    pub fn blank(slot_id: usize) -> Self {
        Self {
            slot_id,
            state: DeviceContextState::Blank,
            input_context: Input64Byte::new_64byte(), // 0 filled
        }
    }

    pub fn slot_id(&self) -> usize {
        self.slot_id
    }

    pub fn enable_slot_context(&mut self) {
        use xhci::context::InputHandler;
        let control = self.input_context.control_mut();
        control.set_add_context_flag(1);
    }

    pub fn enable_endpoint(&mut self, endpoint_id: EndpointId) {
        use xhci::context::InputHandler;
        let control = self.input_context.control_mut();
        control.set_add_context_flag(1 << endpoint_id.address());
    }

    pub fn initialize_slot_context(&mut self, port_id: u8, port_speed: u8) {
        use xhci::context::InputHandler;
        let slot_context = self.input_context.device_mut().slot_mut();
        slot_context.set_route_string(0);
        slot_context.set_root_hub_port_number(port_id);
        slot_context.set_context_entries(1);
        slot_context.set_speed(port_speed);
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
            .device_mut()
            .endpoint_mut(endpoint_context_0_id.address() - 1);

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
}

#[derive(Debug, Clone, Copy)]
enum DeviceContextState {
    Invalid,
    Blank,
    SlotAssigning,
    SlotAssigned,
}
