extern crate alloc;

use core::task::Context;

use super::task;
use alloc::collections::VecDeque;
use kernel_lib::futures::dummy_waker;

pub struct Executor {
    task_queue: VecDeque<task::Task>,
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor {
    pub fn new() -> Self {
        Self {
            task_queue: VecDeque::new(),
        }
    }

    pub fn spawn(&mut self, task: task::Task) {
        self.task_queue.push_back(task);
    }

    pub fn run(&mut self) -> ! {
        loop {
            if let Some(mut task) = self.task_queue.pop_front() {
                let waker = dummy_waker();
                let mut context = Context::from_waker(&waker);
                match task.poll(&mut context) {
                    core::task::Poll::Ready(()) => {}
                    core::task::Poll::Pending => {
                        self.task_queue.push_back(task);
                    }
                }
            }
        }
    }
}
