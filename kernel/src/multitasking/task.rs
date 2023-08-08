extern crate alloc;

use core::{future::Future, pin::Pin, task::{Context, Poll}};
use alloc::boxed::Box;

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum Priority {
    High = 0,
    Default = 10,
}


pub struct Task {
    priority: Priority,
    future: Pin<Box<dyn Future<Output = ()>>>
}

impl Task {
    pub fn new(priority: Priority, future: impl Future<Output = ()> + 'static) -> Self {
        Self {
            priority,
            future: Box::pin(future)
        }
    }

    pub(super) fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}

impl core::cmp::Ord for Task {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Notice that we are reversing the order here
        other.priority.cmp(&self.priority)
    }

}

impl core::cmp::PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl core::cmp::Eq for Task {}


impl core::cmp::PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}