extern crate alloc;
use core::alloc::{Allocator, Layout};

use alloc::boxed::Box;
use xhci::ring::trb::{self, transfer};

use crate::alloc::alloc::{
    alloc_array_with_boundary_with_default_else, alloc_with_boundary_with_default_else,
    GlobalAllocator,
};
use crate::graphics::InstantWriter;
use crate::memory::PAGE_SIZE;
use crate::serial_print;

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
        Self {
            trb_buffer,
            write_index,
            cycle_bit,
        }
    }

    pub fn alloc_new(buf_size: usize) -> Box<Self, &'static GlobalAllocator> {
        const RING_ALIGNMENT: usize = 64;
        const RING_BOUNDARY: usize = PAGE_SIZE;

        alloc_with_boundary_with_default_else(RING_ALIGNMENT, RING_BOUNDARY, || Self::new(buf_size))
            .unwrap()
    }

    pub fn fill_with_normal(&mut self, buf_size: usize) {
        for _idx in 0..(self.buffer_len() - 1) {
            let mut normal = transfer::Normal::new();
            let layout = Layout::from_size_align(buf_size, PAGE_SIZE).unwrap();
            let buf = unsafe { alloc::alloc::alloc_zeroed(layout) };
            normal
                .set_data_buffer_pointer(buf as u64)
                .set_trb_transfer_length(buf_size as u32)
                .set_td_size(0)
                .set_interrupt_on_completion()
                .set_interrupt_on_short_packet()
                .set_interrupter_target(0);
            self.push(transfer::Allowed::Normal(normal));
            self.dump_state();
        }
    }

    pub fn flip_cycle_bit_at(&mut self, trb_pointer: u64) {
        log::debug!(
            "writing trb_ptr: {:p} in [{:p} - {:p}]",
            trb_pointer as *const TrbRaw,
            self.trb_buffer.as_ptr(),
            unsafe { self.trb_buffer.as_ptr().add(self.trb_buffer.len()) }
        );
        log::debug!("buffer_range: {:x?}", self.buffer_range());
        let write_index = self
            .buffer_range()
            .position(|i| i == trb_pointer as usize)
            .unwrap()
            / core::mem::size_of::<TrbRaw>();
        log::debug!("write_index: {}", write_index);
        assert_ne!(write_index, self.trb_buffer.len() - 1);
        self.write_index = write_index;
        self.trb_buffer[write_index].toggle_cycle_bit();

        self.write_index += 1;
        if self.write_index == self.trb_buffer.len() - 1 {
            log::debug!("end of the ring");
            // reached end of the ring
            let mut link = trb::Link::new();
            link.set_ring_segment_pointer(self.trb_buffer.as_ptr() as u64);
            link.set_toggle_cycle();
            if self.cycle_bit {
                link.set_cycle_bit();
            } else {
                link.clear_cycle_bit();
            }
            self.trb_buffer[self.write_index]
                .write_in_order(TrbRaw::new_unchecked(link.into_raw()));

            self.write_index = 0;
            self.toggle_cycle_bit();
        }
        self.dump_state();
    }

    pub fn buffer_range(&self) -> core::ops::Range<usize> {
        let base_ptr = self.buffer_ptr() as *const TrbRaw;
        base_ptr as usize..(unsafe { base_ptr.add(self.buffer_len()) } as usize)
    }

    pub fn cycle_bit(&self) -> bool {
        self.cycle_bit
    }

    pub fn buffer_ptr(&self) -> *const [TrbRaw] {
        &*self.trb_buffer as *const [TrbRaw]
    }

    pub fn buffer_len(&self) -> usize {
        self.trb_buffer.len()
    }

    pub fn toggle_cycle_bit(&mut self) {
        self.cycle_bit = !self.cycle_bit;
    }

    pub fn dump_state(&self) {
        use core::fmt::Write;
        let mut writer = InstantWriter::new(|s| {
            serial_print!("{}", s);
        });
        writeln!(writer, "DEBUG: cycle bits: {}", self.cycle_bit).unwrap();
        self.trb_buffer
            .iter()
            .map(|trb| trb.cycle_bit())
            .for_each(|bit| {
                if bit {
                    write!(writer, "1").unwrap();
                } else {
                    write!(writer, "0").unwrap();
                }
            });
        writeln!(writer).unwrap();
        for _ in 0..(self.write_index.saturating_sub(1)) {
            write!(writer, " ").unwrap();
        }
        writeln!(writer, "^").unwrap();
    }

    #[deprecated]
    pub fn push_with_existing_buf(&mut self, mut cmd: transfer::Normal) -> *mut TrbRaw {
        match transfer::Allowed::try_from(self.trb_buffer[self.write_index].clone().into_raw())
            .unwrap()
        {
            transfer::Allowed::Normal(normal) => {
                let data_buffer_pointer = normal.data_buffer_pointer();
                cmd.set_data_buffer_pointer(data_buffer_pointer);
                cmd.set_trb_transfer_length(normal.trb_transfer_length());
            }
            transfer::Allowed::SetupStage(_) => todo!(),
            transfer::Allowed::DataStage(_) => todo!(),
            transfer::Allowed::StatusStage(_) => todo!(),
            transfer::Allowed::Isoch(_) => todo!(),
            transfer::Allowed::Link(_) => todo!(),
            transfer::Allowed::EventData(_) => todo!(),
            transfer::Allowed::Noop(_) => todo!(),
        }

        self.push(transfer::Allowed::Normal(cmd))
    }

    pub fn push(&mut self, mut cmd: transfer::Allowed) -> *mut TrbRaw {
        if self.cycle_bit {
            cmd.set_cycle_bit();
        } else {
            cmd.clear_cycle_bit();
        }
        self.trb_buffer[self.write_index].write_in_order(TrbRaw::new_unchecked(cmd.into_raw()));

        let trb_ptr = &mut self.trb_buffer[self.write_index] as *mut TrbRaw;
        log::debug!(
            "writing trb_ptr: {:p} in [{:p} - {:p}]",
            trb_ptr,
            self.trb_buffer.as_ptr(),
            unsafe { self.trb_buffer.as_ptr().add(self.trb_buffer.len()) }
        );
        self.write_index += 1;
        if self.write_index == self.trb_buffer.len() - 1 {
            log::debug!("end of the ring");
            // reached end of the ring
            let mut link = trb::Link::new();
            link.set_ring_segment_pointer(self.trb_buffer.as_ptr() as u64);
            link.set_toggle_cycle();
            if self.cycle_bit {
                link.set_cycle_bit();
            } else {
                link.clear_cycle_bit();
            }
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
