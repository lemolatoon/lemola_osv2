extern crate alloc;
use alloc::boxed::Box;
use bit_field::BitField;
use static_assertions::const_assert_eq;
use xhci::{
    accessor::{marker::ReadWrite, Mapper},
    registers::runtime::Interrupter,
    ring::trb::{self, event},
};

use crate::{
    alloc::alloc::{
        alloc_array_with_boundary_with_default_else, alloc_with_boundary_with_default_else,
    },
    memory::PAGE_SIZE,
    xhci::trb::TrbRaw,
};

#[derive(Debug)]
#[repr(C, align(64))]
pub struct EventRingSegmentTableEntry /* erst */ {
    data: [u32; 4],
}

const_assert_eq!(core::mem::size_of::<EventRingSegmentTableEntry>(), 64);

impl EventRingSegmentTableEntry {
    pub fn new(ring_segment_base_address: u64, ring_segment_size: u16) -> Self {
        let mut entry = Self { data: [0; 4] };
        entry.set_ring_segment_base_address(ring_segment_base_address);
        entry.set_ring_segment_size(ring_segment_size);
        entry
    }
    pub fn ring_segment_base_address(&self) -> u64 {
        ((self.data[1] as u64) << 32) | (self.data[0] & 0xffff_ffff) as u64
    }

    pub fn set_ring_segment_base_address(&mut self, address: u64) {
        assert!(
            address.trailing_zeros() >= 6,
            "The Event Ring Segment Table Base Address must be 64-byte aligned."
        );
        let upper = (address >> 32) as u32;
        let lower = address as u32;
        // little endian
        self.data[1] = upper;
        self.data[0] = lower;
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
    trb_buffer: Box<[trb::Link]>,
    event_ring_segment_table: Box<EventRingSegmentTableEntry>,
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
        let default = || -> trb::Link {
            let mut trb = trb::Link::new();
            trb.clear_cycle_bit();
            trb
        };
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
        const ERST_ALIGNMENT: usize = 64;
        const ERST_BOUNDARY: usize = 64 * 1024;
        let event_ring_segment_table =
            alloc_with_boundary_with_default_else(ERST_ALIGNMENT, ERST_BOUNDARY, || {
                EventRingSegmentTableEntry::new(ring_segment_base_address, ring_segment_size)
            })
            .unwrap();

        primary_interrupter
            .erstsz
            .update_volatile(|table_size_reg| {
                table_size_reg.set(1);
            });

        let trb_buffer_head = trb_buffer.as_ptr() as u64;
        primary_interrupter
            .erdp
            .update_volatile(|event_ring_dequeue_pointer| {
                event_ring_dequeue_pointer.set_event_ring_dequeue_pointer(trb_buffer_head)
            });
        log::debug!(
            "EventRingDequeuePointer(erdp): 0x{:x}(read_volatile), 0x{:x}(set)",
            primary_interrupter
                .erdp
                .read_volatile()
                .event_ring_dequeue_pointer(),
            trb_buffer_head
        );

        let event_ring_table_head_ptr = event_ring_segment_table.as_ref() as *const _;
        log::debug!("event_ring_table_head_ptr: {:p}", event_ring_table_head_ptr);
        primary_interrupter.erstba.update_volatile(
            |event_ring_segment_table_base_address_register| {
                event_ring_segment_table_base_address_register
                    .set(event_ring_table_head_ptr as u64);
            },
        );

        Self {
            event_ring_segment_table,
            trb_buffer,
            cycle_bit,
        }
    }

    pub fn cycle_bit(&self) -> bool {
        self.cycle_bit
    }

    pub fn pop<M: Mapper + Clone>(
        &mut self,
        interrupter: &mut Interrupter<'_, M, ReadWrite>,
    ) -> event::Allowed {
        let dequeue_pointer = interrupter
            .erdp
            .read_volatile()
            .event_ring_dequeue_pointer() as *mut TrbRaw;
        let popped = unsafe { dequeue_pointer.read_volatile() };
        let mut next = unsafe { dequeue_pointer.offset(1) };
        const_assert_eq!(core::mem::size_of::<TrbRaw>(), 16);
        let segment_begin =
            self.event_ring_segment_table.ring_segment_base_address() as *mut TrbRaw;

        let segment_end = unsafe {
            segment_begin.offset(self.event_ring_segment_table.ring_segment_size() as isize)
        };

        if next == segment_end {
            next = segment_begin;
            self.cycle_bit = !self.cycle_bit;
        }

        log::debug!("next dequeue ptr: {:p}", next);
        interrupter.erdp.update_volatile(|erdp| {
            erdp.set_event_ring_dequeue_pointer(next as u64);
        });
        event::Allowed::try_from(popped.into_raw()).expect("unexpected trb")
    }
}
