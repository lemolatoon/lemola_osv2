#![cfg_attr(not(feature = "std"), no_std)]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, panic::PanicInfo};

use common::types::KernelMainArg;
use kernel::{graphics::init_graphics, println};

#[no_mangle]
extern "C" fn kernel_main(arg: *const KernelMainArg) -> ! {
    let arg = unsafe { (*arg).clone() };
    let graphics_info = arg.graphics_info;
    init_graphics(graphics_info);
    for i in 0..100 {
        println!("Hello lemola os!!! {}", i);
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
