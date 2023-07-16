extern crate alloc;
use alloc::boxed::Box;
use xhci::ring::trb::{self, command};

use crate::alloc::alloc::{alloc_array_with_boundary_with_default_else, GlobalAllocator};
use crate::memory::PAGE_SIZE;

use super::trb::TrbRaw;

#[derive(Debug)]
pub struct CommandRing {
    trb_buffer: Box<[TrbRaw], &'static GlobalAllocator>,
    write_index: usize,
    cycle_bit: bool,
}

impl CommandRing {
    pub fn new(buf_size: usize) -> Self {
        let default = || -> TrbRaw { TrbRaw::new_unchecked([0u32; 4]) };
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

    pub fn buffer_ptr(&self) -> *const [TrbRaw] {
        &*self.trb_buffer as *const [TrbRaw]
    }

    pub fn toggle_cycle_bit(&mut self) {
        self.cycle_bit = !self.cycle_bit;
    }

    pub fn push(&mut self, mut cmd: command::Allowed) -> *const TrbRaw {
        if self.cycle_bit {
            cmd.set_cycle_bit();
        } else {
            cmd.clear_cycle_bit();
        }
        self.trb_buffer[self.write_index].write_in_order(TrbRaw::new_unchecked(cmd.into_raw()));
        let trb_ptr = &self.trb_buffer[self.write_index] as *const TrbRaw;
        log::debug!(
            "command ring trb ptr: {:p}",
            &self.trb_buffer[self.write_index]
        );

        self.write_index += 1;
        if self.write_index == self.trb_buffer.len() - 1 {
            log::debug!("end of the ring");
            // reached end of the ring
            let mut link = trb::Link::new();
            link.set_toggle_cycle();
            self.trb_buffer[self.write_index]
                .write_in_order(TrbRaw::new_unchecked(link.into_raw()));

            self.write_index = 0;
            self.toggle_cycle_bit();
        }
        trb_ptr
    }
}
