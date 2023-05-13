#![no_std]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, panic::PanicInfo};

use common::types::KernelMainArg;

#[no_mangle]
extern "C" fn kernel_main(arg: *const KernelMainArg) -> ! {
    let arg = unsafe { (*arg).clone() };
    let graphics_frame_buffer = arg.graphics_frame_buffer;
    // let size = 1921024;
    // let base: *mut u8 = 0x80000000 as *mut u8;
    let size = graphics_frame_buffer.size();
    let base: *mut u8 = graphics_frame_buffer.base();
    for i in 0..size {
        unsafe { *base.offset(i as isize) = (i % 256) as u8 };
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
