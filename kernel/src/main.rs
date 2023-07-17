#![cfg_attr(not(feature = "std"), no_std)]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, panic::PanicInfo};

pub extern crate alloc;
use common::types::KernelMainArg;
use core::fmt::Write;
use kernel::{
    alloc::alloc::GlobalAllocator,
    graphics::{init_graphics, init_logger},
    memory::MemoryMapper,
    println, serial_println,
    usb::device::DeviceContextInfo,
    xhci::{init_xhci_controller, XHC},
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

    init_xhci_controller();

    let mut count = 1;
    static_assertions::assert_impl_all!(DeviceContextInfo<MemoryMapper, &'static GlobalAllocator>: usb_host::USBHost);

    let mut controller = XHC.lock();
    let controller = controller.get_mut().unwrap();
    loop {
        count += 1;
        if count < 10 {
            controller.process_event();
        }
        controller.tick_mouse(count).unwrap();
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
