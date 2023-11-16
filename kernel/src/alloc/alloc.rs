extern crate alloc;
use alloc::boxed::Box;
use core::{alloc::LayoutError, mem::MaybeUninit};
use kernel_lib::allocator::FixedLengthAllocator;

const HEAP_SIZE: usize = 1 << 21;

pub type GlobalAllocator = FixedLengthAllocator<HEAP_SIZE>;

#[global_allocator]
static ALLOCATOR: GlobalAllocator = GlobalAllocator::new();

pub fn alloc_with_boundary<T>(
    alignment: usize,
    boundary: usize,
) -> Result<Box<MaybeUninit<T>, &'static GlobalAllocator>, LayoutError> {
    kernel_lib::allocator::alloc_with_boundary(&ALLOCATOR, alignment, boundary)
}

pub fn alloc_with_boundary_with_default_else<T>(
    alignment: usize,
    boundary: usize,
    default: impl FnOnce() -> T,
) -> Result<Box<T, &'static GlobalAllocator>, LayoutError> {
    kernel_lib::allocator::alloc_with_boundary_with_default_else(
        &ALLOCATOR, alignment, boundary, default,
    )
}

pub fn alloc_array_with_boundary<T>(
    len: usize,
    alignment: usize,
    boundary: usize,
) -> Result<Box<[MaybeUninit<T>], &'static GlobalAllocator>, LayoutError> {
    kernel_lib::allocator::alloc_array_with_boundary(&ALLOCATOR, len, alignment, boundary)
}

pub fn alloc_array_with_boundary_with_default_else<T>(
    len: usize,
    alignment: usize,
    boundary: usize,
    default: impl Fn() -> T,
) -> Result<Box<[T], &'static GlobalAllocator>, LayoutError> {
    kernel_lib::allocator::alloc_array_with_boundary_with_default_else(
        &ALLOCATOR, len, alignment, boundary, default,
    )
}
