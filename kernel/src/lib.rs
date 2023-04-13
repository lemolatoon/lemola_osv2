#![no_std]
use core::arch::asm;

#[no_mangle]
extern "C" fn kernel_main() -> ! {
    loop {
        unsafe { asm!("hlt") }
    }
}
