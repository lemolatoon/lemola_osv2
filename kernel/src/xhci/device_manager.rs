extern crate alloc;
use core::panic;

use alloc::sync::Arc;
use alloc::{boxed::Box, vec::Vec};
use spin::Mutex;
use xhci::accessor::Mapper;
use xhci::context::Device32Byte;

use crate::alloc::alloc::alloc_array_with_boundary_with_default_else;
use crate::memory::PAGE_SIZE;
use crate::usb::device::DeviceContextInfo;

use super::event_ring::EventRing;

#[derive(Debug)]
pub struct DeviceManager<M: Mapper + Clone> {
    /// len is max_slots_enabled
    device_context_array: DeviceContextArray<M>,
    registers: Arc<Mutex<xhci::Registers<M>>>,
    event_ring: Arc<Mutex<EventRing>>,
}

impl<M: Mapper + Clone> DeviceManager<M> {
    pub fn new(
        max_slots: u8,
        registers: Arc<Mutex<xhci::Registers<M>>>,
        event_ring: Arc<Mutex<EventRing>>,
    ) -> Self {
        Self {
            registers,
            device_context_array: DeviceContextArray::new(max_slots),
            event_ring,
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

    pub fn allocate_device(&mut self, slot_id: usize) -> &mut DeviceContextInfo<M> {
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

        let registers = Arc::clone(&self.registers);
        let event_ring = Arc::clone(&self.event_ring);
        self.device_context_array.device_context_infos[slot_id] =
            Some(DeviceContextInfo::blank(slot_id, registers, event_ring));
        self.device_context_array.device_context_infos[slot_id]
            .as_mut()
            .unwrap()
    }

    pub fn device_by_slot_id(&self, slot_id: usize) -> Option<&DeviceContextInfo<M>> {
        self.device_context_array.device_context_infos[slot_id].as_ref()
    }

    pub fn device_by_slot_id_mut(&mut self, slot_id: usize) -> Option<&mut DeviceContextInfo<M>> {
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
struct DeviceContextArray<M: Mapper + Clone> {
    device_contexts: Box<[*mut Device32Byte]>,
    device_context_infos: Vec<Option<DeviceContextInfo<M>>>,
}

impl<M: Mapper + Clone> DeviceContextArray<M> {
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
        let mut device_context_infos = Vec::with_capacity(device_contexts_len);
        device_context_infos.resize_with(device_contexts_len, || None);
        Self {
            device_contexts,
            device_context_infos,
        }
    }

    pub fn max_slots(&self) -> usize {
        self.device_contexts.len() - 1
    }
}
