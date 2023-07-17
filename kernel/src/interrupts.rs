use core::arch::asm;

use x86_64::{
    set_general_handler,
    structures::idt::{self, InterruptStackFrame},
};

use crate::{serial_println, xhci::XHC};

static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

fn notify_end_of_interrupt() {
    const PTR: *mut i32 = 0xfee000b0 as *mut i32;
    unsafe { PTR.write_volatile(0) };
}

fn xhci_interrupt_handler(stack_frame: InterruptStackFrame, index: u8, error_code: Option<u64>) {
    let mut xhc = XHC.lock();
    if let Some(xhc) = xhc.get_mut() {
        xhc.process_event();
    }

    notify_end_of_interrupt();
}

fn general_handler(stack_frame: InterruptStackFrame, index: u8, error_code: Option<u64>) {
    serial_println!(
        "Unhandled interrupt: {}, {:?}, {:?}",
        index,
        stack_frame.clone(),
        error_code
    );
    unsafe { core::ptr::null_mut::<u8>().write_volatile(0) };
    notify_end_of_interrupt();
}

pub fn init_gdt() {
    let idt = unsafe { &mut IDT };
    set_general_handler!(idt, general_handler, 0..14);
    set_general_handler!(idt, xhci_interrupt_handler, 14);
    set_general_handler!(idt, general_handler, 15..64);

    idt.load();
}
