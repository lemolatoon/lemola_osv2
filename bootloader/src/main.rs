#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;
use core::{arch::asm, mem::MaybeUninit};

use uefi::{
    prelude::*,
    proto::console::text::Output,
    table::boot::{AllocateType, MemoryDescriptor, MemoryType},
};
use uefi_services::println;

#[repr(C)]
struct AlignedU8Array<const N: usize> {
    _align: [u16; 0],
    data: [u8; N],
}

impl<const N: usize> AlignedU8Array<N> {
    fn new(default: u8) -> Self {
        Self {
            _align: [],
            data: [default; N],
        }
    }
}

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

    let mut file_protocol = match boot_services.get_image_file_system(boot_services.image_handle())
    {
        Ok(protocol) => protocol,
        Err(err) => {
            println!("Failed to get_image_file_system, {:?}", err);
            return err.status();
        }
    };
    let mut root_dir = match file_protocol.open_volume() {
        Ok(root_dir) => root_dir,
        Err(err) => {
            println!("Failed to open root_dir, {:?}", err);
            return err.status();
        }
    };

    const ENTRY_BUF_SIZE: usize = 10000;
    let mut entry_buf: [u8; ENTRY_BUF_SIZE] =
        unsafe { core::mem::transmute(AlignedU8Array::<ENTRY_BUF_SIZE>::new(0)) };
    let kernel_file = loop {
        match root_dir.read_entry(&mut entry_buf) {
            Ok(Some(file_info)) if &file_info.file_name().to_string() == "kernel.elf" => {
                break file_info
            }
            Ok(Some(_)) => continue,
            Ok(None) => {
                println!("There's no entry in root_dir");
                return Status::ABORTED;
            }
            Err(err) => {
                println!("Failed to read_entry, {:?}", err);
                return err.status();
            }
        }
    };

    const KERNEL_ENTRY_POINT: usize = 0x101120;
    let allocated_pointer = match boot_services.allocate_pool(
        MemoryType::LOADER_DATA,
        kernel_file.file_size().try_into().unwrap(),
    ) {
        Ok(allocated_pages) => allocated_pages,
        Err(err) => {
            println!("Failed to allocate_pages, {:?}", err);
            return err.status();
        }
    };
    println!("allocated_pointer: {:?}", allocated_pointer);

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
