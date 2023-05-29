#![cfg_attr(not(feature = "std"), no_std)]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, panic::PanicInfo};

use common::types::KernelMainArg;
use core::fmt::Write;
use kernel::{
    graphics::{init_graphics, init_logger},
    println,
};

#[no_mangle]
extern "C" fn kernel_main(arg: *const KernelMainArg) -> ! {
    let arg = unsafe { (*arg).clone() };
    let graphics_info = arg.graphics_info;
    init_graphics(graphics_info);
    println!("global WRITER initialized?");
    writeln!(
        kernel::graphics::WRITER.0.lock().get_mut().unwrap(),
        "Hello lemola os!!!"
    )
    .unwrap();

    init_logger();
    log::info!("global logger initialized!");
    for i in 0..10 {
        println!("Hello lemola os!!! {}", i);
    }
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[lang = "eh_personality"]
fn eh_personality() {}
