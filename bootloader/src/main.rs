#![no_std]
#![no_main]

use core::{arch::asm, mem::MaybeUninit};

use uefi::{
    prelude::*,
    proto::console::text::Output,
    table::boot::{MemoryDescriptor, MemoryType},
};
use uefi_services::println;

#[entry]
fn main(_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();
    let boot_services = system_table.boot_services();

    reset_text_output(boot_services);

    println!("Hello from uefi.rs");

    const BUF_SIZE: usize = 2048;
    let buf = MaybeUninit::<[u8; BUF_SIZE]>::uninit();

    // Safety: function(memory_map) will initialize this buffer.
    let mut uninit_buf = unsafe { buf.assume_init() };
    let iter = get_memory_map_iter(boot_services, &mut uninit_buf);

    pretty_print_memory_map(iter.clone());
    pretty_print_memory_map(
        iter.filter(|memory_descriptor| memory_descriptor.ty == MemoryType::CONVENTIONAL),
    );

    loop {
        unsafe {
            asm!("hlt");
        }
    }

    Status::SUCCESS
}

fn get_memory_map_iter<'buf, const N: usize>(
    boot_services: &BootServices,
    buf: &'buf mut [u8; N],
) -> impl ExactSizeIterator<Item = &'buf MemoryDescriptor> + Clone {
    let Ok((_, iter)) = boot_services.memory_map(buf) else { panic!("Buffer size {} was not enough", N); };
    iter
}

fn pretty_print_memory_map<'buf>(iter: impl Iterator<Item = &'buf MemoryDescriptor>) {
    for memory_descriptor in iter {
        println!(
            "{{ addr: [ {:#010x} - {:#010x} ], type: {:?}, size: {:#06} KiB }}",
            memory_descriptor.phys_start,
            memory_descriptor.page_count * 4 * 1024 + memory_descriptor.phys_start - 1,
            memory_descriptor.ty,
            memory_descriptor.page_count * 4
        );
    }
}

fn reset_text_output(boot_services: &BootServices) {
    let handle = boot_services.get_handle_for_protocol::<Output>().unwrap();
    let mut simple_text_output_protocol = boot_services
        .open_protocol_exclusive::<Output>(handle)
        .unwrap();
    simple_text_output_protocol.reset(true).unwrap();
}
