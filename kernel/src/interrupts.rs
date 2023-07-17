use x86_64::structures::idt::{self, InterruptStackFrame};

static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

fn xhci_interrupt_handler(stack_frame: InterruptStackFrame, index: u8, error_code: Option<u8>) {}

pub fn init_gdt() {}
