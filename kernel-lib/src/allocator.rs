pub mod bump_allocator;
pub mod fixed_length_allocator;

extern crate alloc;
use alloc::boxed::Box;
use core::{
    alloc::{Allocator, Layout},
    mem::MaybeUninit,
    ops::Range,
};
pub use fixed_length_allocator::FixedLengthAllocator;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocationError;

fn ceil(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}
pub(crate) fn align_and_boundary_to(
    from_ptr: usize,
    layout: Layout,
    boundary: usize,
) -> Result<Range<usize>, ()> {
    debug_assert!(boundary == 0 || boundary.is_power_of_two());
    debug_assert!(boundary == 0 || layout.size() <= boundary);

    let mut alloc_ptr = ceil(from_ptr, layout.align());
    if boundary > 0 {
        let next_boundary = ceil(alloc_ptr, boundary);
        // if allocated area steps over boundary
        if next_boundary < alloc_ptr.checked_add(layout.size()).ok_or(())? {
            alloc_ptr = next_boundary;
        }
    }

    let end_ptr = alloc_ptr.checked_add(layout.size()).ok_or(())?;
    Ok(alloc_ptr..end_ptr)
}

#[macro_export]
macro_rules! impl_global_alloc_for_boundary_alloc {
    ($t:ty) => {
        unsafe impl core::alloc::GlobalAlloc for $t {
            unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
                $crate::allocator::BoundaryAlloc::alloc(self, layout, 0)
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
                $crate::allocator::BoundaryAlloc::dealloc(self, ptr, layout)
            }
        }
    };
}

#[macro_export]
macro_rules! impl_allocator_for_global_alloc {
    ($t:ty) => {
        unsafe impl<'a> core::alloc::Allocator for &'a $t {
            fn allocate(
                &self,
                layout: core::alloc::Layout,
            ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
                let ptr = unsafe { core::alloc::GlobalAlloc::alloc(*self, layout) };
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

            unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: core::alloc::Layout) {
                unsafe { core::alloc::GlobalAlloc::dealloc(*self, ptr.as_ptr(), layout) };
            }
        }
    };
}

pub fn alloc_with_boundary_raw(
    allocator: &impl BoundaryAlloc,
    layout: Layout,
    boundary: usize,
) -> *mut u8 {
    unsafe { BoundaryAlloc::alloc(allocator, layout, boundary) }
}

pub fn alloc_with_boundary<'a, T, A>(
    allocator: &'a A,
    alignment: usize,
    boundary: usize,
) -> Result<Box<MaybeUninit<T>, &'a A>, AllocationError>
where
    A: BoundaryAlloc,
    &'a A: Allocator,
{
    let layout = Layout::from_size_align(core::mem::size_of::<T>(), alignment)
        .map_err(|_| AllocationError {})?;
    let ptr = alloc_with_boundary_raw(allocator, layout, boundary) as *mut MaybeUninit<T>;
    if ptr.is_null() {
        return Err(AllocationError {});
    }
    Ok(unsafe { Box::from_raw_in(ptr, allocator) })
}

pub fn alloc_with_boundary_with_default_else<'a, T, A>(
    allocator: &'a A,
    alignment: usize,
    boundary: usize,
    default: impl FnOnce() -> T,
) -> Result<Box<T, &'a A>, AllocationError>
where
    A: BoundaryAlloc,
    &'a A: Allocator,
{
    let mut allocated = alloc_with_boundary::<T, A>(allocator, alignment, boundary)?;
    let ptr = allocated.as_mut_ptr();
    if ptr.is_null() {
        return Err(AllocationError {});
    }
    unsafe { ptr.write(default()) };
    Ok(unsafe { allocated.assume_init() })
}

pub fn alloc_array_with_boundary<'a, T, A>(
    allocator: &'a A,
    len: usize,
    alignment: usize,
    boundary: usize,
) -> Result<Box<[MaybeUninit<T>], &'a A>, AllocationError>
where
    A: BoundaryAlloc,
    &'a A: Allocator,
{
    let size = len * core::mem::size_of::<T>();
    let layout = Layout::from_size_align(size, alignment).map_err(|_| AllocationError {})?;
    let array_pointer = alloc_with_boundary_raw(allocator, layout, boundary) as *mut MaybeUninit<T>;
    if array_pointer.is_null() {
        return Err(AllocationError {});
    }
    let slice = unsafe { core::slice::from_raw_parts_mut(array_pointer, len) };
    Ok(unsafe { Box::from_raw_in(slice, allocator) })
}

pub fn alloc_array_with_boundary_with_default_else<'a, T, A>(
    allocator: &'a A,
    len: usize,
    alignment: usize,
    boundary: usize,
    default: impl Fn() -> T,
) -> Result<Box<[T], &'a A>, AllocationError>
where
    A: BoundaryAlloc,
    &'a A: Allocator,
{
    let mut uninit_array = alloc_array_with_boundary::<T, A>(allocator, len, alignment, boundary)?;
    for val in uninit_array.iter_mut() {
        unsafe { val.as_mut_ptr().write(default()) };
    }
    // Safety: array is initialized
    Ok(unsafe { uninit_array.assume_init() })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn alloc_huge_times_template(
        allocator: &impl BoundaryAlloc,
        n_times: usize,
        upper_bound: usize,
    ) {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        for _ in 0..n_times {
            let alignment: usize = rng.gen_range(1..upper_bound);
            // alignment must be power of 2
            let alignment = 2i32.pow(alignment.ilog2()) as usize;
            let size = rng.gen_range(0..upper_bound);
            let mut boundary: usize;
            if rng.gen_bool(0.99) {
                boundary = rng.gen_range(size..upper_bound);
                // boundary must be power of 2
                boundary = 2i32.pow(boundary.ilog2()) as usize;
                if boundary < size {
                    boundary *= 2;
                }
            } else {
                boundary = 0;
            }
            let ptr = unsafe {
                BoundaryAlloc::alloc(
                    allocator,
                    Layout::from_size_align(size, alignment).unwrap(),
                    boundary,
                )
            };
            assert!(ptr as usize % alignment == 0);
            if boundary != 0 {
                // boundary check
                let prev_boundary = ptr as usize - (ptr as usize % boundary);
                assert!(
                    prev_boundary <= ptr as usize && ptr as usize + size - 1 < prev_boundary + boundary,
                    "alignment: {:x}, boundary: {:x}, size: {:x}\nallocated area: {:p} - {:p}, boundary: {:p} - {:p}",
                    alignment,
                    boundary,
                    size,
                    ptr,
                    (ptr as usize + size - 1) as *mut u8,
                    prev_boundary as *mut u8,
                    (prev_boundary + boundary) as *mut u8
                );
            }
            unsafe {
                BoundaryAlloc::dealloc(
                    allocator,
                    ptr,
                    Layout::from_size_align(size, alignment).unwrap(),
                );
            }
        }
    }

    pub fn alloc_huge_times_with_value_template<'a, A>(allocator: &'a A, n_times: usize)
    where
        A: BoundaryAlloc,
        &'a A: Allocator,
    {
        for i in 0..n_times {
            let mut vec = Vec::<usize, _>::with_capacity_in(i, allocator);
            let mut vec2 = Vec::<usize, _>::with_capacity_in(i, allocator);
            let mut one = Box::<usize, _>::try_new_uninit_in(allocator).unwrap();
            unsafe {
                one.as_mut_ptr().write_volatile(1);
            }
            for j in 0..i {
                vec.push(core::hint::black_box(j));
            }
            for j in 0..i {
                vec2.push(core::hint::black_box(j));
            }

            assert_eq!(vec.len(), i);
            for (j, val) in vec.into_iter().enumerate() {
                assert_eq!(val, j);
                assert_eq!(vec2[j], j);
            }
            assert_eq!(*unsafe { one.assume_init() }, 1);
        }
    }

    #[test]
    fn ceil_test() {
        assert_eq!(ceil(100, 4), 100);
        assert_eq!(ceil(101, 4), 104);
    }

    #[test]
    fn align_and_boundary_to_test() {
        assert_eq!(
            align_and_boundary_to(
                0x1111,
                Layout::from_size_align(0x100, 0x2000).unwrap(),
                0x400
            ),
            Ok(0x2000..0x2100),
        );
    }

    #[test]
    fn alloc_array_test() {
        let allocator = FixedLengthAllocator::<4096>::new();
        let alignment = 64;
        let len = 100;
        let boundary = 1024;
        let array =
            alloc_array_with_boundary::<u64, _>(&allocator, len, alignment, boundary).unwrap();
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
            alloc_array_with_boundary::<u64, _>(&allocator, len2, alignment2, boundary2).unwrap();
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
            alloc_array_with_boundary::<u8, _>(&allocator, len3, alignment3, boundary3).unwrap();
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
