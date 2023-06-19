extern crate alloc;
use alloc::boxed::Box;
use xhci::ring::trb::{self, command};

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

    pub fn toggle_cycle_bit(&mut self) {
        self.cycle_bit = !self.cycle_bit;
    }

    pub fn push(&mut self, mut cmd: command::Allowed) {
        if self.cycle_bit {
            cmd.set_cycle_bit();
        } else {
            cmd.clear_cycle_bit();
        }
        // TODO: 書き込み順番は重要 ?
        self.trb_buffer[self.write_index] = unsafe { core::mem::transmute(cmd.into_raw()) };

        self.write_index += 1;
        if self.write_index == self.trb_buffer.len() - 1 {
            // reached end of the ring
            let mut link = trb::Link::new();
            link.set_toggle_cycle();
            self.trb_buffer[self.write_index] = link;

            self.write_index = 0;
            self.toggle_cycle_bit();
        }
    }
}
