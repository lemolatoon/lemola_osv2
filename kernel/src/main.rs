#![no_std]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, panic::PanicInfo};

use common::types::KernelMainArg;
use kernel::graphics::{Color, PixcelWriterBuilder};

static mut _WRITER_BUF: [u8; PixcelWriterBuilder::PIXCEL_WRITER_NECESSARY_BUF_SIZE] =
    [0; PixcelWriterBuilder::PIXCEL_WRITER_NECESSARY_BUF_SIZE];

#[no_mangle]
extern "C" fn kernel_main(arg: *const KernelMainArg) -> ! {
    let arg = unsafe { (*arg).clone() };
    let graphics_info = arg.graphics_info;
    let writer = PixcelWriterBuilder::get_writer(&graphics_info, unsafe { &mut _WRITER_BUF });
    for y in 0..(writer.vertical_resolution() / 2) {
        for x in 0..(writer.horizontal_resolution() / 2) {
            let color = Color::new(255, 0, 0);
            writer.write(x, y, color);
        }
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
