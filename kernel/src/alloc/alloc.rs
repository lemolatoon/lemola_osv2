use core::alloc::{GlobalAlloc, Layout};
use spin::Mutex;

const HEAP_SIZE: usize = 1 << 20;
struct FixedLengthAllocatorInner<const SIZE: usize> {
    heap: [u8; SIZE],
    next: usize,
}
pub struct FixedLengthAllocator<const SIZE: usize>(Mutex<FixedLengthAllocatorInner<SIZE>>);

impl<const SIZE: usize> FixedLengthAllocator<SIZE> {
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

fn ceil(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

unsafe impl<const SIZE: usize> BoundaryAlloc for FixedLengthAllocator<SIZE> {
    unsafe fn alloc(&self, layout: Layout, boundary: usize) -> *mut u8 {
        let mut allocator = self.0.lock();
        let start = allocator.next;
        let current_ptr = allocator.heap.as_mut_ptr().add(start);
        let mut alloc_ptr = ceil(current_ptr as usize, layout.align());
        if boundary > 0 {
            let next_boundary = ceil(alloc_ptr, boundary);
            // if allocated area steps over boundary
            if next_boundary < alloc_ptr + layout.size() {
                alloc_ptr = next_boundary;
            }
        }
        let end = alloc_ptr + layout.size() - allocator.heap.as_ptr() as usize;
        if end > SIZE {
            log::debug!("layout: {:?}", layout);
            panic!("[ALLOCATOR] Out of memory");
            core::ptr::null_mut()
        } else {
            allocator.next = end;
            alloc_ptr as *mut u8
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout, _boundary: usize) {
        // Do nothing
    }
}

pub unsafe trait BoundaryAlloc {
    unsafe fn alloc(&self, layout: Layout, boundary: usize) -> *mut u8;

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout, boundary: usize);
}

unsafe impl<const SIZE: usize> GlobalAlloc for FixedLengthAllocator<SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        BoundaryAlloc::alloc(self, layout, 0)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        BoundaryAlloc::dealloc(self, ptr, layout, 0);
    }
}

pub fn alloc_with_boundary(layout: Layout, boundary: usize) -> *mut u8 {
    unsafe { BoundaryAlloc::alloc(&ALLOCATOR, layout, boundary) }
}

pub fn dealloc_with_boundary(ptr: *mut u8, layout: Layout, boundary: usize) {
    unsafe { BoundaryAlloc::dealloc(&ALLOCATOR, ptr, layout, boundary) }
}

#[global_allocator]
static ALLOCATOR: FixedLengthAllocator<HEAP_SIZE> = FixedLengthAllocator::<HEAP_SIZE>::new();
