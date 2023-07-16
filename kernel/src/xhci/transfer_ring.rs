extern crate alloc;
use core::alloc::Allocator;

use alloc::boxed::Box;
use alloc::vec::Vec;
use xhci::ring::trb::{self, transfer};

use crate::alloc::alloc::{
    alloc_array_with_boundary_with_default_else, alloc_with_boundary_with_default_else,
    GlobalAllocator,
};
use crate::memory::PAGE_SIZE;

use super::trb::TrbRaw;

#[derive(Debug)]
pub struct TransferRing<A: Allocator> {
    trb_buffer: Box<[TrbRaw], A>,
    write_index: usize,
    cycle_bit: bool,
}

impl TransferRing<&'static GlobalAllocator> {
    pub fn new(buf_size: usize) -> Self {
        let default = || -> TrbRaw { TrbRaw::new_unchecked([0u32; 4]) };
        const ALIGNMENT: usize = 64;
        // const BOUNDARY: usize = 64 * PAGE_SIZE;
        const BOUNDARY: usize = PAGE_SIZE / 4;
        let trb_buffer =
            alloc_array_with_boundary_with_default_else(buf_size, ALIGNMENT, BOUNDARY, default)
                .expect("Command Ring buffer allocation failed.");
        log::debug!("trb_buffer: {:p}", trb_buffer.as_ptr());
        log::debug!("trb_buffer end: {:p}", unsafe {
            trb_buffer.as_ptr().add(trb_buffer.len())
        });
        let cycle_bit = true;
        let write_index = 0;
        let mut ring = Self {
            trb_buffer,
            write_index,
            cycle_bit,
        };

        // for _ in 0..ring.trb_buffer.len() {
        //     let noop = transfer::Noop::new();
        //     ring.push(transfer::Allowed::Noop(noop));
        // }
        // let noop = transfer::Noop::new();
        // ring.push(transfer::Allowed::Noop(noop));

        ring
    }

    pub fn alloc_new(buf_size: usize) -> Box<Self, &'static GlobalAllocator> {
        const RING_ALIGNMENT: usize = 64;
        const RING_BOUNDARY: usize = PAGE_SIZE;

        alloc_with_boundary_with_default_else(RING_ALIGNMENT, RING_BOUNDARY, || Self::new(buf_size))
            .unwrap()
    }

    pub fn fill_with_normal(&mut self) {
        for _ in 0..self.trb_buffer.len() / 2 - 1 {
            let mut normal = transfer::Normal::new();
            const BUF_LENGTH: usize = 4096;
            let buffer =
                alloc_array_with_boundary_with_default_else(BUF_LENGTH, 4096, 4096, || 0u8)
                    .unwrap();
            normal
                .set_data_buffer_pointer(buffer.as_ptr() as u64)
                .set_trb_transfer_length(BUF_LENGTH as u32)
                .set_td_size(0)
                .set_interrupt_on_completion()
                .set_interrupter_target(0);
            self.push(transfer::Allowed::Normal(normal));
        }
    }

    pub fn cycle_bit(&self) -> bool {
        self.cycle_bit
    }

    pub fn buffer_ptr(&self) -> *const [TrbRaw] {
        &*self.trb_buffer as *const [TrbRaw]
    }

    pub fn toggle_cycle_bit(&mut self) {
        self.cycle_bit = !self.cycle_bit;
    }

    pub fn push(&mut self, mut cmd: transfer::Allowed) -> *mut TrbRaw {
        log::debug!("write cycle_bit: {}", self.cycle_bit);
        log::debug!(
            "trb_buffer: [{:p} - {:p}]",
            self.trb_buffer.as_ptr(),
            unsafe { self.trb_buffer.as_ptr().add(self.trb_buffer.len()) }
        );
        if self.cycle_bit {
            cmd.set_cycle_bit();
        } else {
            cmd.clear_cycle_bit();
        }
        self.trb_buffer[self.write_index].write_in_order(TrbRaw::new_unchecked(cmd.into_raw()));

        let trb_ptr = &mut self.trb_buffer[self.write_index] as *mut TrbRaw;
        log::debug!("writing trb_ptr: {:p}", trb_ptr);
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

    pub fn dump3(&self) {
        log::debug!("trb_buffer: {:p}", self.trb_buffer.as_ptr());
        log::debug!("trb_buffer end: {:p}", unsafe {
            self.trb_buffer.as_ptr().add(self.trb_buffer.len())
        });
        for i in (1..=3).rev() {
            let dump_index = self.write_index as isize - i;
            let dump_index = if dump_index < 0 {
                dump_index + self.trb_buffer.len() as isize
            } else {
                dump_index
            } as usize;
            let trb = unsafe { (&self.trb_buffer[dump_index] as *const TrbRaw).read_volatile() };
            log::debug!("trb[{}]: {:x?}", dump_index, trb.into_raw());
        }
    }
}
