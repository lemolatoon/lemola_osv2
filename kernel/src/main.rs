#![cfg_attr(not(feature = "std"), no_std)]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, ffi::c_void, panic::PanicInfo};

pub extern crate alloc;
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
    let devices = kernel::pci::register::scan_all_bus();
    for device in &devices {
        let class_code = device.class_code();
        serial_println!(
            "{:x>02}{:x>02}{:x>02}",
            class_code.base(),
            class_code.sub(),
            class_code.interface()
        );
    }
    let xhci_device = devices
        .iter()
        .find(|pci_device| pci_device.class_code().is_xhci() && pci_device.vendor_id().is_intel())
        .map_or_else(
            || {
                devices
                    .iter()
                    .find(|pci_device| pci_device.class_code().is_xhci())
            },
            |x| Some(x),
        )
        .expect("xhci device not found");
    log::info!("xhci device found");
    let xhc_bar = xhci_device.read_bar_64(0).unwrap();
    let xhc_mmio_base = xhc_bar & 0xffff_fff0; // 下位4bitはBARのフラグ

    log::info!("xhc_mmio_base: {:?}", xhc_mmio_base as *const c_void);
    // let controller = mikanos_usb::ffi::new_controller(xhc_mmio_base as usize);

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
