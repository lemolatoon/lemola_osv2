extern crate alloc;
use core::{alloc::Allocator, future::Future, task::Poll};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use bit_field::BitField;
use kernel_lib::mutex::Mutex;
use static_assertions::const_assert_eq;
use xhci::{
    accessor::{marker::ReadWrite, Mapper},
    registers::runtime::Interrupter,
    ring::trb::{self, event},
    Registers,
};

use crate::{
    alloc::alloc::{
        alloc_array_with_boundary_with_default_else, alloc_with_boundary_with_default_else,
        GlobalAllocator,
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
        log::info!(
            "EventRingSegmentTableEntry::new: ring_segment_base_address = {:#x}, size = {}",
            ring_segment_base_address,
            ring_segment_size
        );
        entry.set_ring_segment_base_address(ring_segment_base_address);
        entry.set_ring_segment_size(ring_segment_size);
        entry
    }
    pub fn ring_segment_base_address(&self) -> u64 {
        ((self.data[1] as u64) << 32) | self.data[0] as u64
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
pub struct EventRing<A: Allocator> {
    #[allow(dead_code)]
    trb_buffer: Box<[trb::Link], A>,
    popped: Vec<event::Allowed>,
    event_ring_segment_table: Box<EventRingSegmentTableEntry, A>,
    cycle_bit: bool,
    n_pop: usize,
}

impl EventRing<&'static GlobalAllocator> {
    pub fn new<M: Mapper + Clone + Send + Sync>(
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
            popped: Vec::new(),
            cycle_bit,
            n_pop: 0,
        }
    }

    pub fn pending_already_popped_queue(&self) -> bool {
        !self.popped.is_empty()
    }

    pub fn cycle_bit(&self) -> bool {
        self.cycle_bit
    }

    pub fn push(&mut self, trb: event::Allowed) {
        self.popped.push(trb);
    }

    pub fn pop_already_popped(&mut self) -> Option<event::Allowed> {
        self.popped.pop()
    }

    pub fn pop<M: Mapper + Clone + Send + Sync>(
        &mut self,
        interrupter: &mut Interrupter<'_, M, ReadWrite>,
    ) -> Result<event::Allowed, TrbRaw> {
        self.n_pop += 1;
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

        interrupter.erdp.update_volatile(|erdp| {
            erdp.set_event_ring_dequeue_pointer(next as u64);
            erdp.clear_event_handler_busy();
        });
        event::Allowed::try_from(popped.into_raw()).map_err(TrbRaw::new_unchecked)
    }

    pub async fn get_received_transfer_trb_on_slot<M: Mapper + Clone + Send + Sync>(
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        registers: Arc<Mutex<Registers<M>>>,
        slot_id: u8,
    ) -> trb::event::TransferEvent {
        TransferEventFuture {
            event_ring,
            registers,
            wait_on: TransferEventWaitKind::SlotId(slot_id),
        }
        .await
    }

    pub async fn get_received_transfer_trb_on_trb<M: Mapper + Clone + Send + Sync>(
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        registers: Arc<Mutex<Registers<M>>>,
        trb_pointer: u64,
    ) -> trb::event::TransferEvent {
        log::debug!("wait on trb: 0x{:x}", trb_pointer);
        TransferEventFuture {
            event_ring,
            registers,
            wait_on: TransferEventWaitKind::TrbPtr(trb_pointer),
        }
        .await
    }

    pub async fn get_received_command_trb<M: Mapper + Clone + Send + Sync>(
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        registers: Arc<Mutex<Registers<M>>>,
        trb_ptr: u64,
    ) -> trb::event::CommandCompletion {
        CommandCompletionFuture {
            event_ring,
            registers,
            wait_on: trb_ptr,
        }
        .await
    }
}

#[derive(Debug, Clone)]
pub enum TransferEventWaitKind {
    SlotId(u8),
    TrbPtr(u64),
    TrbPtrs(Vec<u64>),
}

pub struct TransferEventFuture<M: Mapper + Clone + Send + Sync> {
    pub event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
    pub registers: Arc<Mutex<Registers<M>>>,
    pub wait_on: TransferEventWaitKind,
}

impl<M: Mapper + Clone + Send + Sync> TransferEventFuture<M> {
    pub fn new(
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        registers: Arc<Mutex<Registers<M>>>,
        wait_on: TransferEventWaitKind,
    ) -> Self {
        Self {
            event_ring,
            registers,
            wait_on,
        }
    }
}

impl<M: Mapper + Clone + Send + Sync> Future for TransferEventFuture<M> {
    type Output = trb::event::TransferEvent;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let registers = Arc::clone(&self.registers);
        let event_ring = Arc::clone(&self.event_ring);
        let wait_on = &self.wait_on;
        let event_ring_dequeue_pointer = {
            let mut registers = kernel_lib::lock!(registers);
            let interrupter = registers.interrupter_register_set.interrupter_mut(0);
            interrupter
                .erdp
                .read_volatile()
                .event_ring_dequeue_pointer() as *const trb::Link
        };
        let event_ring_trb = unsafe { event_ring_dequeue_pointer.read_volatile() };
        let event_ring_cycle_bit = {
            let event_ring = kernel_lib::lock!(event_ring);
            event_ring.cycle_bit()
        };
        if event_ring_trb.cycle_bit() != event_ring_cycle_bit {
            // EventRing does not have front
            return Poll::Pending;
        }
        let popped_trb = {
            let mut registers = kernel_lib::lock!(registers);
            let mut interrupter = registers.interrupter_register_set.interrupter_mut(0);
            let mut event_ring = kernel_lib::lock!(event_ring);
            event_ring.pop(&mut interrupter)
        };
        match wait_on {
            TransferEventWaitKind::SlotId(slot_id) => match popped_trb {
                Ok(event::Allowed::TransferEvent(event)) if event.slot_id() == *slot_id => {
                    log::debug!("got event: {:x?}", event);
                    Poll::Ready(event)
                }
                Ok(trb) => {
                    // EventRing does not have front
                    log::warn!("ignoring trb: {:x?}", trb);
                    {
                        let mut event_ring = kernel_lib::lock!(event_ring);
                        event_ring.push(trb);
                    }
                    Poll::Pending
                }
                Err(trb) => {
                    log::info!("ignoring err...: {:x?}", trb);
                    Poll::Pending
                }
            },
            TransferEventWaitKind::TrbPtr(ptr) => {
                match popped_trb {
                    Ok(event::Allowed::TransferEvent(event)) if event.trb_pointer() == *ptr => {
                        log::debug!("got event: {:?}", event);
                        Poll::Ready(event)
                    }
                    Ok(trb) => {
                        // EventRing does not have front
                        log::warn!("ignoring trb: {:x?}", trb);
                        {
                            let mut event_ring = kernel_lib::lock!(event_ring);
                            event_ring.push(trb);
                        }
                        Poll::Pending
                    }
                    Err(trb) => {
                        log::info!("ignoring err...: {:x?}", trb);
                        Poll::Pending
                    }
                }
            }
            TransferEventWaitKind::TrbPtrs(ptrs) => {
                for ptr in ptrs {
                    match &popped_trb {
                        Ok(event::Allowed::TransferEvent(event)) if event.trb_pointer() == *ptr => {
                            return Poll::Ready(*event);
                        }
                        Ok(_trb) => {
                            // ignoring...
                        }
                        Err(trb) => {
                            log::info!("ignoring err...: {:x?}", trb);

                            return Poll::Pending;
                        }
                    }
                }

                // EventRing does not have front
                log::warn!("ignoring trb: {:x?} for {:?}", &popped_trb, ptrs);
                {
                    let mut event_ring = kernel_lib::lock!(event_ring);
                    event_ring.push(popped_trb.unwrap());
                }

                Poll::Pending
            }
        }
    }
}

pub struct CommandCompletionFuture<M: Mapper + Clone + Send + Sync> {
    pub event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
    pub registers: Arc<Mutex<Registers<M>>>,
    pub wait_on: u64, // trb_ptr
}

impl<M: Mapper + Clone + Send + Sync> CommandCompletionFuture<M> {
    pub fn new(
        event_ring: Arc<Mutex<EventRing<&'static GlobalAllocator>>>,
        registers: Arc<Mutex<Registers<M>>>,
        wait_on: u64,
    ) -> Self {
        Self {
            event_ring,
            registers,
            wait_on,
        }
    }
}

impl<M: Mapper + Clone + Send + Sync> Future for CommandCompletionFuture<M> {
    type Output = trb::event::CommandCompletion;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        // FIXME: this is safe because called member methods does not move them, but their must be a better way
        let Self {
            registers,
            event_ring,
            wait_on,
        } = unsafe { self.get_unchecked_mut() };
        let event_ring_trb = unsafe {
            let mut registers = kernel_lib::lock!(registers);
            (registers
                .interrupter_register_set
                .interrupter_mut(0)
                .erdp
                .read_volatile()
                .event_ring_dequeue_pointer() as *const trb::Link)
                .read_volatile()
        };
        let mut event_ring = kernel_lib::lock!(event_ring);
        if event_ring_trb.cycle_bit() != event_ring.cycle_bit() {
            // EventRing does not have front
            return Poll::Pending;
        }
        let trb = {
            let mut registers = kernel_lib::lock!(registers);
            let mut interrupter = registers.interrupter_register_set.interrupter_mut(0);
            event_ring.pop(&mut interrupter)
        };
        match trb {
            Ok(event::Allowed::CommandCompletion(event))
                if event.command_trb_pointer() == *wait_on =>
            {
                Poll::Ready(event)
            }
            Ok(trb) => {
                // EventRing does not have front
                event_ring.push(trb);
                Poll::Pending
            }
            Err(trb) => {
                log::info!("ignoring err...: {:x?}", trb);
                Poll::Pending
            }
        }
    }
}
