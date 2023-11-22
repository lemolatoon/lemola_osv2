#![cfg_attr(not(feature = "std"), no_std)]
#![no_main]
#![feature(lang_items)]
use core::{arch::asm, panic::PanicInfo};

pub extern crate alloc;
use alloc::vec::Vec;
use common::types::{KernelMainArg, MemoryType};
use kernel::{
    alloc::alloc::{init_allocator, GlobalAllocator},
    graphics::{init_graphics, init_logger},
    interrupts::init_idt,
    memory::MemoryMapper,
    multitasking::{
        executor::Executor,
        task::{Priority, Task},
    },
    println, serial_println,
    usb::{
        class_driver::callbacks::{self, init_mouse_cursor_layer},
        device::DeviceContextInfo,
    },
    xhci::init_xhci_controller,
};
use kernel_lib::{render::Vector2D, Color};

const STACK_SIZE: usize = 1024 * 1024;
#[repr(align(16))]
pub struct KernelStack([u8; STACK_SIZE]);
#[no_mangle]
static mut KERNEL_STACK: KernelStack = KernelStack([0; STACK_SIZE]);

#[no_mangle]
extern "C" fn kernel_main(arg: *const KernelMainArg) -> ! {
    let end_ptr = unsafe { KERNEL_STACK.0.as_ptr_range().end };

    unsafe {
        asm!(
            "mov rsp, {0}",
            "call rax",
            in(reg) end_ptr,
            in("rax") kernel_main2 as extern "sysv64" fn(*const KernelMainArg) -> !,
            in("rdi") arg,
            clobber_abi("sysv64")
        )
    };

    loop {
        unsafe {
            asm!("hlt");
        }
    }
}

#[no_mangle]
extern "sysv64" fn kernel_main2(arg: *const KernelMainArg) -> ! {
    let arg = unsafe { (*arg).clone() };
    let graphics_info = arg.graphics_info;
    let pixcel_writer = init_graphics(graphics_info);
    pixcel_writer.fill_rect(Vector2D::new(50, 50), Vector2D::new(50, 50), Color::white());

    init_logger();

    log::info!("global logger initialized!");

    let memory_map_iter = unsafe { arg.memory_map_entry.as_ref().unwrap().into_iter() };
    let heap = memory_map_iter
        .clone()
        .filter_map(|desc| {
            if let MemoryType::CONVENTIONAL
            | MemoryType::BOOT_SERVICES_CODE
            | MemoryType::BOOT_SERVICES_DATA = desc.ty
            {
                Some(desc.phys_start..(desc.phys_start + desc.page_count * 4096))
            } else {
                None
            }
        })
        .max_by_key(|range| range.end - range.start)
        .expect("no conventional memory region found");
    unsafe {
        init_allocator(heap.start as usize, heap.end as usize);
    }
    let memory_map = memory_map_iter.collect::<Vec<_>>();
    for desc in memory_map.iter() {
        log::debug!(
            "[0x{:09x} - 0x{:09x}] of type {:?}",
            desc.phys_start,
            desc.phys_start + desc.page_count * 4096,
            desc.ty
        );
    }

    let class_drivers = kernel::usb::class_driver::ClassDriverManager::new(
        callbacks::mouse(),
        callbacks::keyboard(),
    );
    unsafe {
        init_mouse_cursor_layer();
    }
    let class_drivers: &'static _ = unsafe { &*(&class_drivers as *const _) };
    let controller = init_xhci_controller(class_drivers);
    init_idt();

    static_assertions::assert_impl_all!(DeviceContextInfo<MemoryMapper, &'static GlobalAllocator>: usb_host::USBHost);

    // x86_64::instructions::interrupts::enable();
    x86_64::instructions::interrupts::int3();
    // FIXME: this comment outted code causes infinite exception loop
    // unsafe { asm!("ud2") };

    let mut executor = Executor::new();
    let polling_task = Task::new(Priority::Default, kernel::xhci::poll_forever(controller));
    let lifegame_task = Task::new(Priority::Default, kernel::lifegame::do_lifegame());
    executor.spawn(polling_task);
    executor.spawn(lifegame_task);

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
