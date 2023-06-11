extern crate alloc;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use xhci::ring::trb::command;
use xhci::ring::trb::Type;

use crate::alloc::alloc::alloc_array_with_boundary_with_default_else;
use crate::memory::PAGE_SIZE;

pub struct CommandRing {
    trb_buffer: Box<[command::Allowed]>,
    write_index: usize,
    cycle_bit: bool,
}

impl CommandRing {
    pub fn new(buf_size: usize) -> Self {
        let default = || {
            let mut allowed = command::Allowed::Noop(command::Noop::new());
            allowed.clear_cycle_bit();
            allowed
        };
        log::debug!("Command Ring init value Allowed: {:?}", default());
        log::debug!("Command Ring init value Allowed: {:?}", unsafe {
            core::mem::transmute::<_, [u8; 20]>(default())
        });
        const ALIGNMENT: usize = 64;
        const BOUNDARY: usize = 64 * PAGE_SIZE;
        let trb_buffer =
            alloc_array_with_boundary_with_default_else(buf_size, ALIGNMENT, BOUNDARY, default)
                .expect("Command Ring buffer allocation failed.");
        let cycle_bit = true;
        let write_index = 0;
        Self {
            trb_buffer,
            write_index,
            cycle_bit,
        }
    }

    pub fn buffer_ptr(&self) -> *const command::Allowed {
        self.trb_buffer.as_ptr()
    }
}
