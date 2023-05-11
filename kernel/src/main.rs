#![no_std]
#![no_main]
use core::{arch::asm, panic::PanicInfo};

use common::types::KernelMainArg;

#[no_mangle]
extern "C" fn kernel_main(arg: KernelMainArg) -> ! {
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
