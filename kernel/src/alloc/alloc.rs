use core::alloc::{GlobalAlloc, Layout};
use spin::Mutex;

const HEAP_SIZE: usize = 0x10000;
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
        let current_ptr = allocator.heap.as_mut_ptr().add(start);
        let aligned_ptr = if (current_ptr.add(layout.size()) as usize) % layout.align() == 0 {
            // Aligned
            current_ptr
        } else {
            // Not aligned
            let aligned_ptr =
                current_ptr.add(2 * layout.align() - (current_ptr as usize) % layout.align());
            aligned_ptr
        };
        let end = start + aligned_ptr as usize - current_ptr as usize + layout.size();
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
