extern crate alloc;
use alloc::vec;
use alloc::{boxed::Box, vec::Vec};
use xhci::context::Device64Byte;

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
            Box::leak(ptr_head) as *mut [*mut [u8; PAGE_SIZE]] as *mut Device64Byte;
    }

    pub fn get_device_contexts_head_ptr(&mut self) -> *mut *mut Device64Byte {
        self.device_context_array.device_contexts.as_mut_ptr()
    }
}

#[derive(Debug)]
struct DeviceContextArray {
    device_contexts: Box<[*mut Device64Byte]>,
    device_context_infos: Vec<Option<Box<DeviceContextInfo>>>,
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
}

#[derive(Debug, Clone)]
struct DeviceContextInfo {
    slot_id: usize,
    state: DeviceContextState,
}

impl DeviceContextInfo {
    pub fn blank(slot_id: usize) -> Self {
        Self {
            slot_id,
            state: DeviceContextState::Blank,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DeviceContextState {
    Invalid,
    Blank,
    SlotAssigning,
    SlotAssigned,
}
