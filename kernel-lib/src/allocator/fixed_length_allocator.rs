use crate::allocator::BoundaryAlloc;
use core::{
    alloc::{Allocator, GlobalAlloc, Layout, LayoutError},
    mem::MaybeUninit,
};

extern crate alloc;
use crate::mutex::Mutex;
use alloc::boxed::Box;

struct FixedLengthAllocatorInner<const SIZE: usize> {
    heap: [u8; SIZE],
    next: usize,
}
pub struct FixedLengthAllocator<const SIZE: usize>(Mutex<FixedLengthAllocatorInner<SIZE>>);

unsafe impl<'a, const SIZE: usize> Allocator for &'a FixedLengthAllocator<SIZE> {
    fn allocate(
        &self,
        layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        let ptr = unsafe { GlobalAlloc::alloc(*self, layout) };
        if ptr.is_null() {
            Err(core::alloc::AllocError)
        } else {
            Ok(unsafe {
                core::ptr::NonNull::new_unchecked(core::slice::from_raw_parts_mut(
                    ptr,
                    layout.size(),
                ))
            })
        }
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: Layout) {
        unsafe { GlobalAlloc::dealloc(*self, ptr.as_ptr(), layout) };
    }
}

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
        let mut allocator = crate::lock!(self.0);
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
            panic!("[ALLOCATOR] Out of memory");
            #[allow(unreachable_code)]
            core::ptr::null_mut()
        } else {
            allocator.next = end;
            alloc_ptr as *mut u8
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Do nothing
    }
}

unsafe impl<const SIZE: usize> GlobalAlloc for FixedLengthAllocator<SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        BoundaryAlloc::alloc(self, layout, 0)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        BoundaryAlloc::dealloc(self, ptr, layout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alignment_test() {
        let allocator = FixedLengthAllocator::<2048>::new();
        unsafe {
            let align1 = 4;
            let size1 = 3;
            let ptr1 =
                GlobalAlloc::alloc(&allocator, Layout::from_size_align(size1, align1).unwrap());
            assert!(ptr1 as usize % align1 == 0);

            let align2 = 64;
            let size2 = 1024;
            let ptr2 =
                GlobalAlloc::alloc(&allocator, Layout::from_size_align(size2, align2).unwrap());
            assert!(ptr2 as usize > ptr1 as usize + size1 - 1);
            assert!(ptr2 as usize % 64 == 0);

            let align3 = 512;
            let size3 = 64;
            let ptr3 =
                GlobalAlloc::alloc(&allocator, Layout::from_size_align(size3, align3).unwrap());
            assert!(ptr3 as usize > ptr2 as usize + size2 - 1);
            assert!(ptr3 as usize % align3 == 0);
        }
    }

    #[test]
    fn boundary_test() {
        let allocator = FixedLengthAllocator::<2048>::new();
        let boundary = 4;
        let alignment = 4;
        let size = 3;
        let ptr1 = unsafe {
            BoundaryAlloc::alloc(
                &allocator,
                Layout::from_size_align(size, alignment).unwrap(),
                boundary,
            ) as usize
        };
        assert!(ptr1 % alignment == 0);
        let prev_boundary = ptr1 - (ptr1 % boundary);
        assert!(prev_boundary <= ptr1 && ptr1 + size - 1 < prev_boundary + boundary);
        let boundary = 2048;
        let alignment = 64;
        let size = 1024;
        let ptr2 = unsafe {
            BoundaryAlloc::alloc(
                &allocator,
                Layout::from_size_align(size, alignment).unwrap(),
                boundary,
            ) as usize
        };
        assert!(ptr2 % alignment == 0);
        let prev_boundary = ptr2 - (ptr2 % boundary);
        assert!(prev_boundary <= ptr2 && ptr2 + size - 1 < prev_boundary + boundary);
        let boundary = 512;
        let alignment = 512;
        let size = 64;
        let ptr3 = unsafe {
            BoundaryAlloc::alloc(
                &allocator,
                Layout::from_size_align(size, alignment).unwrap(),
                boundary,
            ) as usize
        };
        assert!(ptr3 % alignment == 0);
        let prev_boundary = ptr3 - (ptr3 % boundary);
        assert!(prev_boundary <= ptr3 && ptr3 + size - 1 < prev_boundary + boundary);
    }
}
