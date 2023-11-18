use crate::{impl_allocator_for_global_alloc, impl_global_alloc_for_boundary_alloc};

use super::{align_and_boundary_to, BoundaryAlloc};

pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
    n_allocations: usize,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            n_allocations: 0,
        }
    }

    /// Initialize the allocator with the given heap range
    /// # Safety
    /// The caller must ensure that the given heap range is unused permanently.
    /// Also, this method must be called only once.
    pub unsafe fn init(&mut self, heap_start: usize, heap_end: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_end;
        self.next = heap_start;
    }
}

unsafe impl BoundaryAlloc for crate::mutex::Mutex<BumpAllocator> {
    unsafe fn alloc(&self, layout: core::alloc::Layout, boundary: usize) -> *mut u8 {
        let mut allocator = crate::lock!(self);
        let Ok(alloc_start) = align_and_boundary_to(allocator.next, layout, boundary) else {
            return core::ptr::null_mut();
        };

        if alloc_start.end >= allocator.heap_end {
            return core::ptr::null_mut();
        }

        allocator.next = alloc_start.end;
        allocator.n_allocations += 1;

        alloc_start.start as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        let mut allocator = crate::lock!(self);
        allocator.n_allocations -= 1;
        if allocator.n_allocations == 0 {
            log::debug!("dealloc: Resetting allocator");
            allocator.next = allocator.heap_start;
        }
    }
}

impl_global_alloc_for_boundary_alloc!(crate::mutex::Mutex<BumpAllocator>);
impl_allocator_for_global_alloc!(crate::mutex::Mutex<BumpAllocator>);

#[cfg(test)]
mod tests {
    use std::println;

    use super::*;
    use crate::allocator::tests::{
        alloc_huge_times_template, alloc_huge_times_with_value_template,
    };
    #[test]
    fn alloc_huge_times() {
        const SIZE: usize = 100 * 1024;
        static HEAP: &[u8] = &[0u8; SIZE];
        let allocator = crate::mutex::Mutex::new(BumpAllocator::new());
        unsafe {
            crate::lock!(allocator).init(HEAP.as_ptr() as usize, HEAP.as_ptr() as usize + SIZE)
        };
        alloc_huge_times_template(&allocator, SIZE / 1024, 1000);
    }

    #[test]
    fn alloc_huge_times_with_value() {
        const SIZE: usize = 100 * 1024;
        static mut HEAP: &[u8] = &[0u8; SIZE];
        let allocator = crate::mutex::Mutex::new(BumpAllocator::new());
        unsafe {
            crate::lock!(allocator).init(HEAP.as_ptr() as usize, HEAP.as_ptr() as usize + SIZE)
        };
        // TODO: fix this
        // alloc_huge_times_with_value_template(&allocator, SIZE / 1024);
    }
}
