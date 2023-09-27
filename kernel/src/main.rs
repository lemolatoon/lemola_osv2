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
    interrupts::init_idt,
    memory::MemoryMapper,
    multitasking::{
        executor::Executor,
        task::{Priority, Task},
    },
    println, serial_println,
    usb::{class_driver::callbacks, device::DeviceContextInfo},
    xhci::init_xhci_controller,
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
        kernel_lib::lock!(kernel::graphics::WRITER.0)
            .get_mut()
            .unwrap(),
        "Hello lemola os!!!"
    )
    .unwrap();

    init_logger();

    log::info!("global logger initialized!");

    pixcel_writer.write_ascii(50, 50, 'A', Color::white(), Color::new(255, 50, 0));

    pixcel_writer.fill_shape(Vector2D::new(30, 50), &MOUSE_CURSOR_SHAPE);

    init_idt();
    let class_drivers = kernel::usb::class_driver::ClassDriverManager::new(
        callbacks::mouse(),
        callbacks::keyboard(),
    );
    let class_drivers: &'static _ = unsafe { &*(&class_drivers as *const _) };
    let controller = init_xhci_controller(class_drivers);

    static_assertions::assert_impl_all!(DeviceContextInfo<MemoryMapper, &'static GlobalAllocator>: usb_host::USBHost);

    x86_64::instructions::interrupts::int3();
    // FIXME: this comment outted code causes infinite exception loop
    // unsafe { asm!("ud2") };

    // x86_64::instructions::interrupts::enable();

    let mut executor = Executor::new();
    let controller: &'static _ = unsafe { &*(&controller as *const _) };
    let polling_task = Task::new(Priority::Default, kernel::xhci::poll_forever(controller));
    let lifegame_task = Task::new(Priority::Default, kernel::lifegame::do_lifegame());
    executor.spawn(polling_task);
    executor.spawn(lifegame_task);

    // x86_64::instructions::interrupts::enable();
    executor.run();
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
