use x86_64::structures::idt::{self, InterruptStackFrame};

use crate::xhci::XHC;

static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

fn xhci_interrupt_handler(stack_frame: InterruptStackFrame, index: u8, error_code: Option<u8>) {
    let mut xhc = XHC.lock();
    if let Some(xhc) = xhc.get_mut() {
        xhc.process_event();
    }
}

pub fn init_gdt() {}
