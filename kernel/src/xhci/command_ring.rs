extern crate alloc;
use alloc::boxed::Box;
use xhci::ring::trb;

use crate::alloc::alloc::alloc_array_with_boundary_with_default_else;
use crate::memory::PAGE_SIZE;

#[derive(Debug)]
pub struct CommandRing {
    trb_buffer: Box<[trb::Link]>,
    write_index: usize,
    cycle_bit: bool,
}

impl CommandRing {
    pub fn new(buf_size: usize) -> Self {
        let default = || -> trb::Link {
            let mut trb = trb::Link::new();
            trb.clear_cycle_bit();
            trb
        };
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

    pub fn buffer_ptr(&self) -> *const trb::Link {
        self.trb_buffer.as_ptr()
    }
}
