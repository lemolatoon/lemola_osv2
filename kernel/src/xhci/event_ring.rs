extern crate alloc;
use alloc::boxed::Box;
use bit_field::BitField;
use xhci::{
    accessor::{marker::ReadWrite, Mapper},
    registers::runtime::Interrupter,
    ring::trb::event,
};

use crate::{alloc::alloc::alloc_array_with_boundary_with_default_else, memory::PAGE_SIZE};

#[derive(Debug)]
#[repr(transparent)]
pub struct EventRingSegmentTableEntry {
    data: [u32; 4],
}

impl EventRingSegmentTableEntry {
    pub fn new(ring_segment_base_address: u64, ring_segment_size: u16) -> Self {
        let mut entry = Self { data: [0; 4] };
        entry.set_ring_segment_base_address(ring_segment_base_address);
        entry.set_ring_segment_size(ring_segment_size);
        entry
    }
    pub fn ring_segment_base_address(&self) -> u64 {
        ((self.data[0] as u64) << 32) | self.data[1] as u64
    }

    pub fn set_ring_segment_base_address(&mut self, address: u64) {
        let upper = (address >> 32) as u32;
        let lower = address as u32;
        self.data[0] = upper;
        self.data[1] = lower;
    }

    pub fn ring_segment_size(&self) -> u16 {
        self.data[2].get_bits(0..16).try_into().unwrap()
    }

    pub fn set_ring_segment_size(&mut self, ring_segment_size: u16) {
        self.data[2].set_bits(0..16, ring_segment_size as u32);
    }
}

#[derive(Debug)]
pub struct EventRing {
    trb_buffer: Box<[event::Allowed]>,
    event_ring_segment_table_entry: EventRingSegmentTableEntry,
    cycle_bit: bool,
}

impl EventRing {
    pub fn new<M: Mapper + Clone>(
        buf_size: u16,
        primary_interrupter: &mut Interrupter<'_, M, ReadWrite>,
    ) -> Self {
        let cycle_bit = true;
        const ALIGNMENT: usize = 64;
        const BOUNDARY: usize = 64 * PAGE_SIZE;
        let default = || -> event::Allowed { unsafe { core::mem::transmute([0u32; 5]) } };
        let trb_buffer = alloc_array_with_boundary_with_default_else(
            buf_size as usize,
            ALIGNMENT,
            BOUNDARY,
            default,
        )
        .expect("Command Ring buffer allocation failed.");

        let ring_segment_base_address = trb_buffer.as_ptr() as u64;
        let ring_segment_size = trb_buffer.len() as u16;
        debug_assert_eq!(buf_size, ring_segment_size);
        let event_ring_segment_table_entry =
            EventRingSegmentTableEntry::new(ring_segment_base_address, ring_segment_size);

        primary_interrupter
            .erstsz
            .update_volatile(|table_size_reg| {
                table_size_reg.set(1);
            });

        primary_interrupter
            .erdp
            .update_volatile(|event_ring_dequeue_pointer| {
                event_ring_dequeue_pointer
                    .set_event_ring_dequeue_pointer(&trb_buffer[0] as *const _ as u64)
            });

        Self {
            event_ring_segment_table_entry,
            trb_buffer,
            cycle_bit,
        }
    }

    pub fn cycle_bit(&self) -> bool {
        self.cycle_bit
    }

    pub fn pop<M: Mapper + Clone>(&mut self, interrupter: &mut Interrupter<'_, M, ReadWrite>) {
        let dequeue_pointer = interrupter
            .erdp
            .read_volatile()
            .event_ring_dequeue_pointer() as *mut event::Allowed;
        let mut next = unsafe { dequeue_pointer.add(1) };
        let segment_begin = self
            .event_ring_segment_table_entry
            .ring_segment_base_address() as *mut event::Allowed;

        let segment_end = unsafe {
            segment_begin.add(self.event_ring_segment_table_entry.ring_segment_size() as usize)
        };

        if next == segment_end {
            next = segment_begin;
            self.cycle_bit = !self.cycle_bit;
        }

        interrupter.erstba.update_volatile(|erstba| {
            erstba.set(next as u64);
        });
    }
}
