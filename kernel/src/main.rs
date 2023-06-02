#![cfg_attr(not(feature = "std"), no_std)]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, panic::PanicInfo};

pub extern crate alloc;
use alloc::vec::Vec;
use common::types::KernelMainArg;
use core::fmt::Write;
use kernel::{
    graphics::{init_graphics, init_logger},
    println, serial_println,
};
use kernel_lib::{render::Vector2D, shapes::mouse::MOUSE_CURSOR_SHAPE, Color};

#[no_mangle]
extern "C" fn kernel_main(arg: *const KernelMainArg) -> ! {
    serial_println!("Hello lemola os!!! from serial");
    let arg = unsafe { (*arg).clone() };
    let graphics_info = arg.graphics_info;
    let pixcel_writer = init_graphics(graphics_info);
    pixcel_writer.fill_rect(Vector2D::new(50, 50), Vector2D::new(50, 50), Color::white());
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

    pixcel_writer.write_ascii(50, 50, 'A', Color::white(), Color::new(255, 50, 0));

    pixcel_writer.fill_shape(Vector2D::new(30, 50), &MOUSE_CURSOR_SHAPE);
    // let devices = kernel::pci::register::scan_all_bus();
    // devices.iter().for_each(|device| {
    //     if device.vendor_id().is_intel() {
    //         log::info!("device: {:#?}", device);
    //     } else {
    //         log::info!("not intel");
    //     }
    // });
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("KERNEL PANIC: {}", info);
    println!("KERNEL PANIC: {}", info);
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[lang = "eh_personality"]
fn eh_personality() {}
