use conquer_once::spin::OnceCell;

use crossbeam_queue::ArrayQueue;

pub enum InterruptionMessage {
    Xhci,
}

static INTERRUPTION_MESSAGE_QUEUE: OnceCell<ArrayQueue<InterruptionMessage>> = OnceCell::uninit();

pub fn init_interrupt_message_queue() {
    INTERRUPTION_MESSAGE_QUEUE
        .try_init_once(|| ArrayQueue::new(100))
        .expect("Interrupt message queue already initialized");
}
