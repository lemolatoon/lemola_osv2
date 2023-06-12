extern crate alloc;
use alloc::boxed::Box;
use core::{
    alloc::{GlobalAlloc, Layout, LayoutError},
    mem::MaybeUninit,
};
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

pub fn alloc_with_boundary_raw(layout: Layout, boundary: usize) -> *mut u8 {
    unsafe { BoundaryAlloc::alloc(&ALLOCATOR, layout, boundary) }
}

pub fn alloc_with_boundary<T>(
    alignment: usize,
    boundary: usize,
) -> Result<Box<MaybeUninit<T>>, LayoutError> {
    let layout = Layout::from_size_align(core::mem::size_of::<T>(), alignment)?;
    let ptr = alloc_with_boundary_raw(layout, boundary) as *mut MaybeUninit<T>;
    debug_assert!(!ptr.is_null());
    Ok(unsafe { Box::from_raw(ptr) })
}

pub fn alloc_with_boundary_with_default_else<T>(
    alignment: usize,
    boundary: usize,
    default: impl FnOnce() -> T,
) -> Result<Box<T>, LayoutError> {
    let mut allocated = alloc_with_boundary::<T>(alignment, boundary)?;
    let ptr = allocated.as_mut_ptr();
    unsafe { ptr.write(default()) };
    Ok(unsafe { allocated.assume_init() })
}

pub fn alloc_array_with_boundary<T>(
    len: usize,
    alignment: usize,
    boundary: usize,
) -> Result<Box<[MaybeUninit<T>]>, LayoutError> {
    let size = len * core::mem::size_of::<*mut T>();
    let layout = Layout::from_size_align(size, alignment)?;
    let array_pointer = alloc_with_boundary_raw(layout, boundary) as *mut MaybeUninit<T>;
    debug_assert!(!array_pointer.is_null());
    let slice = unsafe { core::slice::from_raw_parts_mut(array_pointer, len) };
    Ok(unsafe { Box::from_raw(slice) })
}

pub fn alloc_array_with_boundary_with_default_else<T>(
    len: usize,
    alignment: usize,
    boundary: usize,
    default: impl Fn() -> T,
) -> Result<Box<[T]>, LayoutError> {
    let size = len * core::mem::size_of::<*mut T>();
    let layout = Layout::from_size_align(size, alignment)?;
    let array_pointer = alloc_with_boundary_raw(layout, boundary) as *mut MaybeUninit<T>;
    debug_assert!(!array_pointer.is_null());
    let slice = unsafe { core::slice::from_raw_parts_mut(array_pointer, len) };
    for val in slice.iter_mut() {
        unsafe { val.as_mut_ptr().write(default()) };
    }
    // Safety: slice is initialized
    Ok(unsafe { Box::from_raw(slice as *mut [core::mem::MaybeUninit<T>] as *mut [T]) })
}

#[global_allocator]
static ALLOCATOR: FixedLengthAllocator<HEAP_SIZE> = FixedLengthAllocator::<HEAP_SIZE>::new();
