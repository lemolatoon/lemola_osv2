extern crate alloc;
use alloc::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserEvent {
    InitPortDevice(InitPortDevice),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitPortDevice {
    pub port_index: u8,
    pub routing: u32,
    pub speed: u8,
    pub parent_hub_slot_id: Option<u8>,
    pub parent_port_index: Option<u8>,
}

#[derive(Debug)]
pub struct UserEventRing {
    data: VecDeque<UserEvent>,
}

impl Default for UserEventRing {
    fn default() -> Self {
        Self::new()
    }
}

impl UserEventRing {
    pub fn new() -> Self {
        Self {
            data: VecDeque::new(),
        }
    }

    pub fn push(&mut self, event: UserEvent) {
        self.data.push_back(event);
    }

    pub fn pop(&mut self) -> Option<UserEvent> {
        self.data.pop_front()
    }
}
