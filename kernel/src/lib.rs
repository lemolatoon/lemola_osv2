#![cfg_attr(not(feature = "std"), no_std)]
#![feature(ptr_alignment_type)]
#![feature(const_option)]
#![feature(new_uninit)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_as_bytes)]
#![feature(adt_const_params)]
#![feature(allocator_api)]
pub mod alloc;
pub mod font;
pub mod graphics;
pub mod memory;
pub mod pci;
pub mod serial;
pub mod usb;
pub mod xhci;
