#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use alloc::{string::String, vec};
use common::types::{GraphicsFrameBuffer, KernelMainArg};
use core::arch::asm;
use core::mem::MaybeUninit;
use core::panic;
use elf::{endian::AnyEndian, ElfBytes};
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter};
use log;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::table::boot::ScopedProtocol;
use uefi_services::{print, println};

use uefi::{
    self,
    prelude::*,
    proto::{
        console::text::Output,
        media::file::{File, FileAttribute, RegularFile},
    },
    table::boot::{AllocateType, MemoryDescriptor, MemoryType},
};

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
fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();
    let boot_services = system_table.boot_services();

    reset_text_output(boot_services);

    log::info!("Hello from uefi.rs");

    const BUF_SIZE: usize = 2048;
    let buf = MaybeUninit::<[u8; BUF_SIZE]>::uninit();

    // Safety: function(memory_map) will initialize this buffer.
    let mut uninit_buf = unsafe { buf.assume_init() };
    let memory_maps: Vec<_> = get_memory_map_iter(boot_services, &mut uninit_buf).collect();

    pretty_print_memory_map(memory_maps.iter());
    pretty_print_memory_map(
        memory_maps
            .iter()
            .filter(|memory_descriptor| memory_descriptor.ty == MemoryType::CONVENTIONAL),
    );

    let mut file_protocol = match boot_services.get_image_file_system(boot_services.image_handle())
    {
        Ok(protocol) => protocol,
        Err(err) => {
            panic!("Failed to get_image_file_system, {:?}", err);
        }
    };
    let mut root_dir = match file_protocol.open_volume() {
        Ok(root_dir) => root_dir,
        Err(err) => {
            panic!("Failed to open root_dir, {:?}", err);
        }
    };

    const ENTRY_BUF_SIZE: usize = 10000;
    let mut entry_buf: [u8; ENTRY_BUF_SIZE] =
        unsafe { core::mem::transmute(AlignedU8Array::<ENTRY_BUF_SIZE>::new(0)) };
    let kernel_file_info = loop {
        match root_dir.read_entry(&mut entry_buf) {
            Ok(Some(file_info)) if file_info.file_name() == cstr16!("kernel.elf") => {
                break file_info
            }
            Ok(Some(_)) => continue,
            Ok(None) => {
                log::info!("There's no entry in root_dir");
                return Status::ABORTED;
            }
            Err(err) => {
                panic!("Failed to read_entry, {:?}", err);
            }
        }
    };

    let file_handle = match root_dir.open(
        cstr16!("kernel.elf"),
        uefi::proto::media::file::FileMode::Read,
        FileAttribute::empty(),
    ) {
        Ok(file_handle) => file_handle,
        Err(err) => {
            panic!("Failed to open kernel.elf, {:?}", err);
        }
    };
    // Safety: `kernel.elf` is not a directory.
    let mut kernel_file = unsafe { RegularFile::new(file_handle) };
    let kernel_file_size = kernel_file_info.file_size().try_into().unwrap();
    let mut kernel_buffer = vec![0; kernel_file_size];
    if let Err(err) = kernel_file.read(&mut kernel_buffer) {
        panic!("Failed to read kernel.elf, {:?}", err);
    }

    let elf = match ElfBytes::<AnyEndian>::minimal_parse(&kernel_buffer) {
        Ok(elf) => {
            // for program_header in elf.ehdr {
            //     log::info!("program_header: {:?}", program_header);
            // }
            for section_header in elf.section_headers().unwrap() {
                log::info!("section_header: {:?}", section_header);
            }
            // let (system_table, string_table) = elf.symbol_table().unwrap().unwrap();
            // log::info!("system_table: {:?}", system_table);
            // log::info!("string_table: {:?}", string_table);
            log::info!("elf.ehdr: {:?}", elf.ehdr);
            elf
        }
        Err(err) => {
            panic!("Failed to parse elf, {:?}", err);
        }
    };

    let (load_min_addr, load_max_addr) = calc_load_address_range(&elf);
    log::info!(
        "kernel will be loaded at {:#x} - {:#x}",
        load_min_addr,
        load_max_addr
    );
    let n_pages = ((load_max_addr - load_min_addr + 0xfff) / 0x1000) as usize;
    let allocated_pointer = match boot_services.allocate_pages(
        AllocateType::Address(load_min_addr as usize),
        MemoryType::LOADER_DATA,
        n_pages,
    ) {
        Ok(allocated_pages) => allocated_pages,
        Err(err) => {
            panic!("Failed to allocate_pages, {:?}", err);
        }
    };
    log::info!(
        "memory allocated: {:#x} - {:#x}",
        allocated_pointer,
        allocated_pointer + n_pages as u64 * 4 * 1024
    );

    let memory_type_at_allocated_pointer_before = memory_maps
        .iter()
        .filter(|memory_descriptor| {
            memory_descriptor.phys_start <= allocated_pointer
                && allocated_pointer
                    < memory_descriptor.phys_start + memory_descriptor.page_count * 4 * 1024
        })
        .next()
        .unwrap()
        .ty;
    let memory_type_at_allocated_pointer = get_memory_map_iter(boot_services, &mut uninit_buf)
        .filter(|memory_descriptor| {
            memory_descriptor.phys_start <= allocated_pointer
                && allocated_pointer
                    < memory_descriptor.phys_start + memory_descriptor.page_count * 4 * 1024
        })
        .next()
        .unwrap()
        .ty;
    log::info!(
        "MemoryType at {:#x} before: {:?}",
        allocated_pointer,
        memory_type_at_allocated_pointer_before
    );
    log::info!(
        "MemoryType at {:#x}: {:?}",
        allocated_pointer,
        memory_type_at_allocated_pointer
    );
    unsafe { copy_load_segments(&elf, &kernel_buffer) };
    let entry_point = elf.ehdr.e_entry;
    log::info!("entry_point: {:#x}", entry_point);
    pretty_print_entry_point_asm(entry_point);
    let graphics_frame_buffer = construct_graphics_info(&boot_services);
    log::info!("graphics_frame_buffer: {:?}", graphics_frame_buffer);

    panic!("here1");
    let kernel_main: extern "C" fn(arg: KernelMainArg) -> ! =
        unsafe { core::mem::transmute(entry_point as usize) };
    panic!("here2");

    drop(file_protocol);
    // exit_boot_services before boot
    let buf_size = boot_services.memory_map_size().map_size;
    let mut uninit_buf = Vec::with_capacity(buf_size);
    unsafe { uninit_buf.set_len(buf_size) };
    let (system_table, memory_map) =
        match system_table.exit_boot_services(image_handle, &mut uninit_buf) {
            Ok(ret) => ret,
            Err(err) => {
                panic!("Failed to exit_boot_services, {:?}", err);
            }
        };

    let kernel_main_arg = KernelMainArg {
        graphics_frame_buffer,
    };

    kernel_main(kernel_main_arg);

    #[allow(unreachable_code)]
    Status::SUCCESS
}

fn construct_graphics_info(boot_services: &BootServices) -> GraphicsFrameBuffer {
    let mut gop: ScopedProtocol<GraphicsOutput> = match boot_services
        .get_handle_for_protocol::<GraphicsOutput>()
        .and_then(|gop_handle| boot_services.open_protocol_exclusive::<GraphicsOutput>(gop_handle))
    {
        Ok(gop) => gop,
        Err(err) => {
            panic!("Failed to locate_protocol, {:?}", err);
        }
    };
    panic!("gop2");
    let mut frame_buffer = gop.frame_buffer();
    GraphicsFrameBuffer::new(frame_buffer.as_mut_ptr(), frame_buffer.size())
}

fn pretty_print_entry_point_asm(entry_pointer: u64) {
    let mut buf = [0; 16];
    unsafe {
        core::ptr::copy_nonoverlapping(entry_pointer as *const u8, buf.as_mut_ptr(), 16);
    }
    let mut decoder = Decoder::with_ip(64, &buf, entry_pointer, DecoderOptions::NONE);

    // Formatters: Masm*, Nasm*, Gas* (AT&T) and Intel* (XED).
    // For fastest code, see `SpecializedFormatter` which is ~3.3x faster. Use it if formatting
    // speed is more important than being able to re-assemble formatted instructions.
    let mut formatter = IntelFormatter::new();

    // Change some options, there are many more
    formatter.options_mut().set_digit_separator("`");
    formatter.options_mut().set_first_operand_char_index(10);

    // String implements FormatterOutput
    let mut output = String::new();

    // Initialize this outside the loop because decode_out() writes to every field
    let mut instruction = Instruction::default();

    // The decoder also implements Iterator/IntoIterator so you could use a for loop:
    //      for instruction in &mut decoder { /* ... */ }
    // or collect():
    //      let instructions: Vec<_> = decoder.into_iter().collect();
    // but can_decode()/decode_out() is a little faster:
    const HEXBYTES_COLUMN_BYTE_LENGTH: usize = 10;
    while decoder.can_decode() {
        // There's also a decode() method that returns an instruction but that also
        // means it copies an instruction (40 bytes):
        //     instruction = decoder.decode();
        decoder.decode_out(&mut instruction);

        // Format the instruction ("disassemble" it)
        output.clear();
        formatter.format(&instruction, &mut output);

        // Eg. "00007FFAC46ACDB2 488DAC2400FFFFFF     lea       rbp,[rsp-100h]"
        println!("{:016X} ", instruction.ip());
        let start_index = (instruction.ip() - entry_pointer) as usize;
        let instr_bytes = &buf[start_index..start_index + instruction.len()];
        for b in instr_bytes.iter() {
            print!("{:02X}", b);
        }
        if instr_bytes.len() < HEXBYTES_COLUMN_BYTE_LENGTH {
            for _ in 0..HEXBYTES_COLUMN_BYTE_LENGTH - instr_bytes.len() {
                print!("  ");
            }
        }
        println!(" {}", output);
    }
}

/// Safety;
/// - passed elf is parsed from kernel_loaded_buffer.
/// - kernel_loaded_buffer's Loadable program segments(PT_LOAD) ranges' memory must be allocated.
unsafe fn copy_load_segments(elf: &ElfBytes<AnyEndian>, kernel_loaded_buffer: &[u8]) {
    for program_header in elf.segments().unwrap() {
        if program_header.p_type == elf::abi::PT_LOAD {
            let src = &kernel_loaded_buffer[program_header.p_offset as usize
                ..(program_header.p_offset + program_header.p_memsz) as usize];
            let dst = program_header.p_vaddr as *mut u8;
            unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len()) }
        }
    }
}

fn calc_load_address_range(elf: &ElfBytes<AnyEndian>) -> (u64, u64) {
    let mut min = u64::MAX; // The start address of the first PT_LOAD segment.
    let mut max = u64::MIN; // The end address of the last PT_LOAD segment.
    for program_header in elf.segments().unwrap() {
        if program_header.p_type == elf::abi::PT_LOAD {
            min = min.min(program_header.p_vaddr);
            max = max.max(program_header.p_vaddr + program_header.p_memsz);
        }
    }
    (min, max)
}

fn get_memory_map_iter<'buf, const N: usize>(
    boot_services: &BootServices,
    buf: &'buf mut [u8; N],
) -> impl ExactSizeIterator<Item = &'buf MemoryDescriptor> + Clone {
    let Ok((_, iter)) = boot_services.memory_map(buf) else { panic!("Buffer size {} was not enough", N); };
    iter
}

fn pretty_print_memory_map<'a, 'buf>(iter: impl Iterator<Item = &'a &'buf MemoryDescriptor>)
where
    'buf: 'a, // 'buf must live longer than 'a.
{
    for memory_descriptor in iter {
        log::info!(
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

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    println!("[PANIC]: {}", info);
    loop {
        unsafe { asm!("hlt") };
    }
}
