#![cfg_attr(not(feature = "std"), no_std)]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, panic::PanicInfo};

use common::types::KernelMainArg;
use kernel::graphics::PixcelWriterBuilder;
use kernel_lib::{Color, Writer};

static mut _WRITER_BUF: [u8; PixcelWriterBuilder::PIXCEL_WRITER_NECESSARY_BUF_SIZE] =
    [0; PixcelWriterBuilder::PIXCEL_WRITER_NECESSARY_BUF_SIZE];

#[no_mangle]
extern "C" fn kernel_main(arg: *const KernelMainArg) -> ! {
    let arg = unsafe { (*arg).clone() };
    let graphics_info = arg.graphics_info;
    let writer = PixcelWriterBuilder::get_writer(&graphics_info, unsafe { &mut _WRITER_BUF });
    for y in 0..(writer.vertical_resolution()) {
        for x in 0..(writer.horizontal_resolution()) {
            let color = Color::new(0, 0, 0);
            writer.write(x, y, color);
        }
    }
    let mut writer = Writer::<25, 80>::new(writer);
    for i in 0..20000usize {
        for _ in 0..1000 {
            // sleep
            unsafe { core::ptr::write_volatile(0xb8000 as *mut u8, 0x0a) };
        }
        writer.put_char((('a' as u8) + (i % 26) as u8) as char);
        writer.put_char('\n')
    }
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[lang = "eh_personality"]
fn eh_personality() {}
