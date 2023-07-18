use x86_64::{
    set_general_handler,
    structures::idt::{self, InterruptStackFrame},
};

use crate::xhci::XHC;

static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptVector {
    Xhci = 14,
}

fn xhci_interrupt_handler(_stack_frame: InterruptStackFrame, _index: u8, _error_code: Option<u64>) {
    log::info!("xhci interrupt handler called");
    let mut xhc = XHC.lock();
    if let Some(xhc) = xhc.get_mut() {
        xhc.process_event();
    }
}

fn general_handler(stack_frame: InterruptStackFrame, index: u8, error_code: Option<u64>) {
    log::error!(
        "Unhandled interrupt: {}, {:?}, {:?}",
        index,
        stack_frame.clone(),
        error_code
    );
}

pub fn init_gdt() {
    let idt = unsafe { &mut IDT };
    set_general_handler!(idt, general_handler, 0..14);

    set_general_handler!(idt, xhci_interrupt_handler, 14);
    static_assertions::const_assert_eq!(InterruptVector::Xhci as u8, 14);

    set_general_handler!(idt, general_handler, 15..64);

    idt.load();
}
