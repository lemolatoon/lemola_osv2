extern crate alloc;
use alloc::boxed::Box;
use core::mem::MaybeUninit;
use kernel_lib::{
    allocator::{bump_allocator::BumpAllocator, AllocationError},
    mutex::Mutex,
};

pub type GlobalAllocator = Mutex<BumpAllocator>;

#[global_allocator]
static ALLOCATOR: GlobalAllocator = Mutex::new(BumpAllocator::new());

/// Initialize the global allocator.
/// # Safety
/// The caller must ensure that the given heap range is unused permanently.
/// Also, this method must be called only once. before any allocation.
pub unsafe fn init_allocator(heap_start: usize, heap_end: usize) {
    log::debug!(
        "Initializing allocator: {:#x} - {:#x}",
        heap_start,
        heap_end
    );
    kernel_lib::lock!(ALLOCATOR).init(heap_start, heap_end);
}

pub fn alloc_with_boundary<T>(
    alignment: usize,
    boundary: usize,
) -> Result<Box<MaybeUninit<T>, &'static GlobalAllocator>, AllocationError> {
    kernel_lib::allocator::alloc_with_boundary(&ALLOCATOR, alignment, boundary)
}

pub fn alloc_with_boundary_with_default_else<T>(
    alignment: usize,
    boundary: usize,
    default: impl FnOnce() -> T,
) -> Result<Box<T, &'static GlobalAllocator>, AllocationError> {
    kernel_lib::allocator::alloc_with_boundary_with_default_else(
        &ALLOCATOR, alignment, boundary, default,
    )
}

pub fn alloc_array_with_boundary<T>(
    len: usize,
    alignment: usize,
    boundary: usize,
) -> Result<Box<[MaybeUninit<T>], &'static GlobalAllocator>, AllocationError> {
    kernel_lib::allocator::alloc_array_with_boundary(&ALLOCATOR, len, alignment, boundary)
}

pub fn alloc_array_with_boundary_with_default_else<T>(
    len: usize,
    alignment: usize,
    boundary: usize,
    default: impl Fn() -> T,
) -> Result<Box<[T], &'static GlobalAllocator>, AllocationError> {
    kernel_lib::allocator::alloc_array_with_boundary_with_default_else(
        &ALLOCATOR, len, alignment, boundary, default,
    )
}
