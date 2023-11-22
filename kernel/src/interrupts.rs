pub mod messages;

use x86_64::{
    set_general_handler,
    structures::idt::{self, InterruptStackFrame},
};

use crate::xhci::{get_xhc, write_local_apic_id};

use self::messages::get_interruption_message_queue;

static mut IDT: idt::InterruptDescriptorTable = idt::InterruptDescriptorTable::new();

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptVector {
    Xhci = 64,
}

fn xhci_interrupt_handler(_stack_frame: InterruptStackFrame, _index: u8, _error_code: Option<u64>) {
    let xhc = get_xhc();
    let Some(Ok(trb)) =
        x86_64::instructions::interrupts::without_interrupts(|| xhc.pop_event_ring())
    else {
        return;
    };
    if let Err(err) =
        get_interruption_message_queue().push(messages::InterruptionMessage::Xhci(trb))
    {
        x86_64::instructions::interrupts::without_interrupts(|| {
            log::warn!("An interrupt message is dropped: {:?}", err);
        })
    };

    write_local_apic_id(0xb0, 0);
}

fn general_handler(stack_frame: InterruptStackFrame, index: u8, error_code: Option<u64>) {
    log::error!(
        "Unhandled interrupt: {}, {:#x?}, {:#x?}",
        index,
        stack_frame.clone(),
        error_code
    );
    loop {
        x86_64::instructions::hlt();
    }
}

fn breakpoint_handler(stack_frame: InterruptStackFrame, _index: u8, _error_code: Option<u64>) {
    log::info!("breakpoint handler called");
    log::info!("{:?}", stack_frame);
}

pub fn init_idt() {
    let idt = unsafe { &mut IDT };
    set_general_handler!(idt, general_handler, 0..3);
    set_general_handler!(idt, breakpoint_handler, 3);
    set_general_handler!(idt, general_handler, 4..32);

    set_general_handler!(
        idt,
        xhci_interrupt_handler,
        InterruptVector::Xhci as u8..=InterruptVector::Xhci as u8
    );

    idt.load();
}
