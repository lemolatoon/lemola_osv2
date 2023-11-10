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

/// # Safety
/// Type impls this trait must properly allocate or deallocate memory
pub unsafe trait BoundaryAlloc {
    /// This Safety clause is brought from GlobalAlloc
    /// # Safety
    /// This function is unsafe because undefined behavior can result if the caller does not ensure that layout has non-zero size.
    unsafe fn alloc(&self, layout: Layout, boundary: usize) -> *mut u8;

    /// This Safety clause is brought from GlobalAlloc
    /// # Safety
    /// This function is unsafe because undefined behavior can result if the caller does not ensure all of the following:
    /// - ptr must denote a block of memory currently allocated via this allocator,
    /// - layout must be the same layout that was used to allocate that block of memory.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);
}

unsafe impl<const SIZE: usize> GlobalAlloc for FixedLengthAllocator<SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        BoundaryAlloc::alloc(self, layout, 0)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        BoundaryAlloc::dealloc(self, ptr, layout);
    }
}

pub fn alloc_with_boundary_raw(
    allocator: &impl BoundaryAlloc,
    layout: Layout,
    boundary: usize,
) -> *mut u8 {
    unsafe { BoundaryAlloc::alloc(allocator, layout, boundary) }
}

pub fn alloc_with_boundary<const SIZE: usize, T>(
    allocator: &FixedLengthAllocator<SIZE>,
    alignment: usize,
    boundary: usize,
) -> Result<Box<MaybeUninit<T>, &'_ FixedLengthAllocator<SIZE>>, LayoutError> {
    let layout = Layout::from_size_align(core::mem::size_of::<T>(), alignment)?;
    let ptr = alloc_with_boundary_raw(allocator, layout, boundary) as *mut MaybeUninit<T>;
    log::debug!("allocating {:x}", ptr as usize);
    let (until, heap_start, heap_end) = {
        let allocator = crate::lock!(allocator.0);
        (
            allocator.next,
            allocator.heap.as_ptr() as usize,
            allocator.heap.as_ptr() as usize + SIZE,
        )
    };
    log::debug!(
        "[{:x}..{:x}] in [{:x}...{:x}]",
        ptr as usize,
        until,
        heap_start,
        heap_end
    );
    debug_assert!(!ptr.is_null());
    Ok(unsafe { Box::from_raw_in(ptr, allocator) })
}

pub fn alloc_with_boundary_with_default_else<const SIZE: usize, T>(
    allocator: &FixedLengthAllocator<SIZE>,
    alignment: usize,
    boundary: usize,
    default: impl FnOnce() -> T,
) -> Result<Box<T, &'_ FixedLengthAllocator<SIZE>>, LayoutError> {
    let mut allocated = alloc_with_boundary::<SIZE, T>(allocator, alignment, boundary)?;
    let ptr = allocated.as_mut_ptr();
    unsafe { ptr.write(default()) };
    Ok(unsafe { allocated.assume_init() })
}

pub fn alloc_array_with_boundary<const SIZE: usize, T>(
    allocator: &FixedLengthAllocator<SIZE>,
    len: usize,
    alignment: usize,
    boundary: usize,
) -> Result<Box<[MaybeUninit<T>], &'_ FixedLengthAllocator<SIZE>>, LayoutError> {
    let size = len * core::mem::size_of::<T>();
    let layout = Layout::from_size_align(size, alignment)?;
    let array_pointer = alloc_with_boundary_raw(allocator, layout, boundary) as *mut MaybeUninit<T>;
    debug_assert!(!array_pointer.is_null());
    let slice = unsafe { core::slice::from_raw_parts_mut(array_pointer, len) };
    Ok(unsafe { Box::from_raw_in(slice, allocator) })
}

pub fn alloc_array_with_boundary_with_default_else<const SIZE: usize, T>(
    allocator: &FixedLengthAllocator<SIZE>,
    len: usize,
    alignment: usize,
    boundary: usize,
    default: impl Fn() -> T,
) -> Result<Box<[T], &'_ FixedLengthAllocator<SIZE>>, LayoutError> {
    let mut uninit_array =
        alloc_array_with_boundary::<SIZE, T>(allocator, len, alignment, boundary)?;
    for val in uninit_array.iter_mut() {
        unsafe { val.as_mut_ptr().write(default()) };
    }
    // Safety: array is initialized
    Ok(unsafe { uninit_array.assume_init() })
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
        let ptr1 = alloc_with_boundary_raw(
            &allocator,
            Layout::from_size_align(size, alignment).unwrap(),
            boundary,
        ) as usize;
        assert!(ptr1 % alignment == 0);
        let prev_boundary = ptr1 - (ptr1 % boundary);
        assert!(prev_boundary <= ptr1 && ptr1 + size - 1 < prev_boundary + boundary);
        let boundary = 2048;
        let alignment = 64;
        let size = 1024;
        let ptr2 = alloc_with_boundary_raw(
            &allocator,
            Layout::from_size_align(size, alignment).unwrap(),
            boundary,
        ) as usize;
        assert!(ptr2 % alignment == 0);
        let prev_boundary = ptr2 - (ptr2 % boundary);
        assert!(prev_boundary <= ptr2 && ptr2 + size - 1 < prev_boundary + boundary);
        let boundary = 512;
        let alignment = 512;
        let size = 64;
        let ptr3 = alloc_with_boundary_raw(
            &allocator,
            Layout::from_size_align(size, alignment).unwrap(),
            boundary,
        ) as usize;
        assert!(ptr3 % alignment == 0);
        let prev_boundary = ptr3 - (ptr3 % boundary);
        assert!(prev_boundary <= ptr3 && ptr3 + size - 1 < prev_boundary + boundary);
    }

    #[test]
    fn alloc_array_test() {
        let allocator = FixedLengthAllocator::<4096>::new();
        let alignment = 64;
        let len = 100;
        let boundary = 1024;
        let array =
            alloc_array_with_boundary::<_, u64>(&allocator, len, alignment, boundary).unwrap();
        let start_ptr = array.as_ptr() as usize;
        let end_ptr = start_ptr + array.len() * core::mem::size_of::<u64>();
        // check boundary
        let prev_boundary = start_ptr - (start_ptr % boundary);
        assert!(prev_boundary <= start_ptr && end_ptr - 1 < (prev_boundary + boundary));
        // check alignment
        assert!(start_ptr % alignment == 0);
        // check length
        assert_eq!(array.len(), len);

        let alignment2 = 4;
        let len2 = 100;
        let boundary2 = 1024;
        let array2 =
            alloc_array_with_boundary::<_, u64>(&allocator, len2, alignment2, boundary2).unwrap();
        let start_ptr2 = array2.as_ptr() as usize;
        let end_ptr2 = start_ptr2 + array2.len() * core::mem::size_of::<u64>();
        // check boundary
        let prev_boundary2 = start_ptr2 - (start_ptr2 % boundary2);
        assert!(prev_boundary2 <= start_ptr2 && end_ptr2 - 1 < (prev_boundary2 + boundary2));
        // check alignment
        assert!(start_ptr % alignment2 == 0);
        // check length
        assert_eq!(array2.len(), len2);

        // check that the two arrays are not overlapping
        assert!(end_ptr <= start_ptr2);

        let alignment3 = 1024;
        let len3 = 1024;
        let boundary3 = 1024;
        let array3 =
            alloc_array_with_boundary::<_, u8>(&allocator, len3, alignment3, boundary3).unwrap();
        let start_ptr3 = array3.as_ptr() as usize;
        let end_ptr3 = start_ptr3 + array3.len() * core::mem::size_of::<u8>(); // this ptr is not included in the array

        // check boundary
        let prev_boundary3 = start_ptr3 - (start_ptr3 % boundary3);
        assert!(prev_boundary3 <= start_ptr3 && end_ptr3 - 1 < (prev_boundary3 + boundary3));
        // check alignment
        assert!(start_ptr3 % alignment3 == 0);
        // check length
        assert_eq!(array3.len(), len3);
        // check that the two arrays are not overlapping
        assert!(end_ptr2 <= start_ptr3);
    }
}
