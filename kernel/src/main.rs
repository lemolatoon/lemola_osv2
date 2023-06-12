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
    xhci::controller::XhciController,
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

    pixcel_writer.write_ascii(50, 50, 'A', Color::white(), Color::new(255, 50, 0));

    pixcel_writer.fill_shape(Vector2D::new(30, 50), &MOUSE_CURSOR_SHAPE);
    let devices = kernel::pci::register::scan_all_bus();
    for device in &devices {
        serial_println!(
            "vend: {}, class: {}, head: {}",
            device.vendor_id(),
            device.class_code(),
            device.header_type()
        );
    }
    let xhci_device = devices
        .iter()
        .find(|pci_device| {
            pci_device.class_code().is_xhci_controller() && pci_device.vendor_id().is_intel()
        })
        .map_or_else(
            || {
                devices
                    .iter()
                    .find(|pci_device| pci_device.class_code().is_xhci_controller())
            },
            Some,
        )
        .expect("xhci device not found");
    log::info!(
        "xhci device found, {:x}, {:x}, {:x}",
        xhci_device.bus(),
        xhci_device.device(),
        xhci_device.function()
    );
    serial_println!(
        "vend: {}, class: {}, head: {}",
        xhci_device.vendor_id(),
        xhci_device.class_code(),
        xhci_device.header_type()
    );
    let xhc_bar = xhci_device.read_bar(0).unwrap();
    let xhc_mmio_base = xhc_bar & 0xffff_ffff_ffff_fff0; // 下位4bitはBARのフラグ

    log::info!("xhc_mmio_base: {:?}", xhc_mmio_base as *const c_void);
    let memory_mapper = kernel::memory::MemoryMapper::new();
    let mut controller = unsafe { XhciController::new(xhc_mmio_base as usize, memory_mapper) };
    log::info!("xhc initialized");
    controller.run();

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
