use core::cell::Cell;

extern crate alloc;
use alloc::vec::Vec;
use spin::MutexGuard;

const BUF_LEN: usize = 1024;
#[derive(Debug)]
pub struct Mutex<T> {
    inner: spin::Mutex<T>,
    file: Cell<[Option<&'static str>; BUF_LEN]>,
    line: Cell<[Option<u32>; BUF_LEN]>,
}
unsafe impl<T> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(inner: T) -> Self {
        Self {
            inner: spin::Mutex::new(inner),
            file: Cell::new([None; BUF_LEN]),
            line: Cell::new([None; BUF_LEN]),
        }
    }
    pub fn lock(&self, file: &'static str, line: u32) -> MutexGuard<T> {
        self.store_file_line(file, line);
        self.dump_state_if_locked();
        self.inner.lock()
    }

    pub fn store_file_line(&self, file: &'static str, line: u32) {
        let file_head_ptr = self.file.as_ptr() as *mut Option<&'static str>;
        for index in 0..BUF_LEN {
            let ptr = unsafe { file_head_ptr.add(index) };
            if unsafe { ptr.read() }.is_none() {
                unsafe { ptr.write(Some(file)) };
                break;
            }
        }

        let line_head_ptr = self.line.as_ptr() as *mut Option<_>;
        for index in 0..BUF_LEN {
            let ptr = unsafe { line_head_ptr.add(index) };
            if unsafe { ptr.read() }.is_none() {
                unsafe { ptr.write(Some(line)) };
                break;
            }
        }
    }

    pub fn dump_state_if_locked(&self) {
        const MAX: usize = 10000;
        let mut count = 0;
        loop {
            if !self.is_locked() {
                return;
            }
            count += 1;
            if count > MAX {
                break;
            }
        }
        self.print_file_line();
    }

    pub fn print_file_line(&self) {
        let info = self
            .file
            .get()
            .iter()
            .zip(self.line.get().iter())
            .filter_map(|(f, l)| f.and_then(|f| l.and_then(|l| Some((f, l)))))
            .collect::<Vec<_>>();
        log::debug!("{:?}", info);
    }

    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }

    pub fn try_lock(&self, file: &'static str, line: u32) -> Option<MutexGuard<T>> {
        self.store_file_line(file, line);
        self.dump_state_if_locked();
        self.inner.try_lock()
    }

    pub fn _lock_raw(&self) -> MutexGuard<T> {
        self.inner.lock()
    }
}

#[macro_export]
macro_rules! lock {
    // switch implementation if you analyze deadlock
    // ($mutex:expr) => {
    //     $mutex.lock(file!(), line!())
    // };
    ($mutex:expr) => {
        $mutex._lock_raw()
    };
}
