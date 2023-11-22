use conquer_once::spin::OnceCell;

use crossbeam_queue::ArrayQueue;
use xhci::ring::trb::event;

#[derive(Debug, Clone)]
pub enum InterruptionMessage {
    Xhci(event::Allowed),
}

static INTERRUPTION_MESSAGE_QUEUE: OnceCell<ArrayQueue<InterruptionMessage>> = OnceCell::uninit();

pub fn get_interruption_message_queue() -> &'static ArrayQueue<InterruptionMessage> {
    INTERRUPTION_MESSAGE_QUEUE
        .get()
        .expect("Interrupt message queue not initialized")
}

pub fn init_interrupt_message_queue() {
    INTERRUPTION_MESSAGE_QUEUE
        .try_init_once(|| ArrayQueue::new(100))
        .expect("Interrupt message queue already initialized");
}
