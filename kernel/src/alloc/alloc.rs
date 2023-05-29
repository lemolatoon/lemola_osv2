use core::alloc::{GlobalAlloc, Layout};
use spin::Mutex;

const HEAP_SIZE: usize = 0x1000;
struct FixedLengthAllocatorInner<const SIZE: usize> {
    heap: [u8; SIZE],
    next: usize,
}
pub struct FixedLengthAllocator<const SIZE: usize>(Mutex<FixedLengthAllocatorInner<SIZE>>);

impl FixedLengthAllocator<HEAP_SIZE> {
    pub const fn new() -> Self {
        Self(Mutex::new(FixedLengthAllocatorInner::new()))
    }
}

impl<const SIZE: usize> FixedLengthAllocatorInner<SIZE> {
    pub const fn new() -> Self {
        Self {
            heap: [0; SIZE],
            next: 0,
        }
    }
}

unsafe impl<const SIZE: usize> GlobalAlloc for FixedLengthAllocator<SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.0.lock();
        let start = allocator.next;
        let end = start + layout.size();
        if end > SIZE {
            panic!("[ALLOCATOR] Out of memory");
            core::ptr::null_mut()
        } else {
            allocator.next = end;
            allocator.heap.as_mut_ptr().add(start)
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Do nothing
    }
}

#[global_allocator]
static mut ALLOCATOR: FixedLengthAllocator<HEAP_SIZE> = FixedLengthAllocator::<HEAP_SIZE>::new();
