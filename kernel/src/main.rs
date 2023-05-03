#![no_std]
#![no_main]
use core::{arch::asm, panic::PanicInfo};

#[no_mangle]
extern "C" fn kernel_main() -> ! {
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
