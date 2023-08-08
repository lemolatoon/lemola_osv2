extern crate alloc;
use core::alloc::Allocator;
use core::panic;

use alloc::sync::Arc;
use alloc::{boxed::Box, vec::Vec};
use spin::Mutex;
use xhci::accessor::Mapper;
use xhci::context::Device32Byte;

use crate::alloc::alloc::{alloc_array_with_boundary_with_default_else, GlobalAllocator};
use crate::memory::PAGE_SIZE;
use crate::usb::device::{DeviceContextInfo, DeviceContextWrapper};

use super::command_ring::CommandRing;
use super::event_ring::EventRing;

type Device32BytePtr = u64;

#[derive(Debug)]
pub struct DeviceManager<M: Mapper + Clone + Send + Sync, A: Allocator> {
    /// len is max_slots_enabled
    device_context_array: DeviceContextArray<M, A>,
    registers: Arc<Mutex<xhci::Registers<M>>>,
    event_ring: Arc<Mutex<EventRing<A>>>,
    command_ring: Arc<Mutex<CommandRing>>,
}

impl<M: Mapper + Clone + Send + Sync + Send> DeviceManager<M, &'static GlobalAllocator> {
    pub fn new(
        max_slots: u8,
        registers: Arc<Mutex<xhci::Registers<M>>>,
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        command_ring: Arc<Mutex<CommandRing>>,
    ) -> Self {
        Self {
            registers,
            device_context_array: DeviceContextArray::new(max_slots),
            event_ring,
            command_ring,
        }
    }

    pub fn set_scratchpad_buffer_array(
        &mut self,
        ptr_head: Box<[*mut [u8; PAGE_SIZE]], impl core::alloc::Allocator>,
    ) {
        // This pointer cast is safe, because it is based on xhci specification
        let mut device_contexts = self.device_context_array.device_contexts.lock();
        device_contexts[0] =
            Box::leak(ptr_head) as *mut [*mut [u8; PAGE_SIZE]] as *mut Device32Byte
                as Device32BytePtr;
    }

    pub fn get_device_contexts_head_ptr(&mut self) -> usize {
        self.device_context_array.device_contexts.lock().as_mut_ptr() as usize
    }

    pub fn allocate_device(
        &self,
        port_index: usize,
        slot_id: usize,
    ) -> Arc<Mutex<Option<DeviceContextInfo<M, &'static GlobalAllocator>>>> {
        if slot_id > self.device_context_array.max_slots() {
            log::error!(
                "slot_id is out of range: {} / {}",
                slot_id,
                self.device_context_array.max_slots()
            );
            panic!("slot_id is out of range");
        }

        let mut device_context_info =
            self.device_context_array.device_context_infos[slot_id].lock();
        if device_context_info.is_some() {
            log::error!("device context at {} is already allocated", slot_id);
            panic!("device context at {} is already allocated", slot_id);
        }

        let registers = Arc::clone(&self.registers);
        let event_ring = Arc::clone(&self.event_ring);
        let command_ring = Arc::clone(&self.command_ring);
        *device_context_info = Some(DeviceContextInfo::new(
            port_index,
            slot_id,
            registers,
            event_ring,
            command_ring,
        ));
        Arc::clone(&self.device_context_array.device_context_infos[slot_id])
    }

    pub fn device_by_slot_id(
        &self,
        slot_id: usize,
    ) -> Arc<Mutex<Option<DeviceContextInfo<M, &'static GlobalAllocator>>>> {
        Arc::clone(&self.device_context_array.device_context_infos[slot_id])
    }

    pub fn load_device_context(&self, slot_id: usize) {
        if slot_id > self.device_context_array.max_slots() {
            log::error!("Invalid slot_id: {}", slot_id);
            panic!("Invalid slot_id: {}", slot_id);
        }
        let mut device_context_info =
            self.device_context_array.device_context_infos[slot_id].lock();
        let device_context_info = device_context_info.as_mut().unwrap();
        let mut device_contexts = self.device_context_array.device_contexts.lock();
        device_contexts[slot_id] = &*device_context_info.device_context
            as *const DeviceContextWrapper
            as *mut Device32Byte
            as Device32BytePtr;
    }
}

#[derive(Debug)]
struct DeviceContextArray<M: Mapper + Clone + Send + Sync, A: Allocator> {
    device_contexts: Mutex<Box<[Device32BytePtr], A>>,
    device_context_infos: Vec<Arc<Mutex<Option<DeviceContextInfo<M, A>>>>>,
}

impl<M: Mapper + Clone + Send + Sync> DeviceContextArray<M, &'static GlobalAllocator> {
    pub fn new(max_slots: u8) -> Self {
        let device_contexts_len = max_slots as usize + 1;
        const ALIGNMENT: usize = 64;
        // allow this because xhci specification says we shall initialized with 0
        #[allow(clippy::zero_ptr)]
        let device_contexts = alloc_array_with_boundary_with_default_else(
            device_contexts_len,
            ALIGNMENT,
            PAGE_SIZE,
            || 0 as Device32BytePtr,
        )
        .expect("DeviceContextArray allocation failed");
        let device_contexts = Mutex::new(device_contexts);

        let mut device_context_infos = Vec::with_capacity(device_contexts_len);
        device_context_infos.resize_with(device_contexts_len, || Arc::new(Mutex::new(None)));
        let device_context_infos = device_context_infos;


        Self {
            device_contexts,
            device_context_infos,
        }
    }

    pub fn max_slots(&self) -> usize {
        self.device_contexts.lock().len() - 1
    }
}
