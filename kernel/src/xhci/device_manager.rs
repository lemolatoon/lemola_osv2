extern crate alloc;
use alloc::vec;
use alloc::{boxed::Box, vec::Vec};
use core::mem::ManuallyDrop;
use core::{alloc::Layout, mem::size_of, pin::Pin};
use xhci::context::Device64Byte;

use crate::alloc::alloc::alloc_with_boundary;

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
}

#[derive(Debug)]
struct DeviceContextArray {
    device_contexts: Box<[*mut Device64Byte]>,
    device_context_infos: Vec<Option<Box<DeviceContextInfo>>>,
}

impl DeviceContextArray {
    pub fn new(max_slots: u8) -> Self {
        let device_contexts_len = max_slots as usize + 1;
        let size = device_contexts_len * size_of::<*mut Device64Byte>();
        const ALIGNMENT: usize = 64;
        let layout = Layout::from_size_align(size, ALIGNMENT).unwrap();
        const PAGE_BOUNDARY: usize = 4096;
        let device_context_pointers =
            alloc_with_boundary(layout, PAGE_BOUNDARY) as *mut *mut Device64Byte;
        let slice = unsafe {
            core::slice::from_raw_parts_mut(device_context_pointers, device_contexts_len)
        };
        // TODO: dropの挙動をよく考える。場合によっては手動でdropをimplする必要あり。（boundary)があるため。
        let mut device_contexts = unsafe { Box::from_raw(slice) };
        for device_context in device_contexts.iter_mut() {
            // init by null_ptr
            *device_context = 0 as *mut Device64Byte;
        }
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
