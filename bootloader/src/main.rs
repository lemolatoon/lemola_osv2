#![no_main]
#![no_std]
#![feature(abi_efiapi)]

use core::arch::asm;

use uefi::prelude::*;
use uefi_services::println;

#[entry]
fn main(_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();

    println!("Hello from uefi.rs");

    loop {
        unsafe {
            asm!("hlt");
        }
    }

    Status::SUCCESS
}
