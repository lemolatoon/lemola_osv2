#![cfg_attr(not(feature = "std"), no_std)]
#![feature(ptr_alignment_type)]
#![feature(const_option)]
#![feature(new_uninit)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_as_bytes)]
#![allow(incomplete_features)]
#![feature(adt_const_params)]
#![feature(allocator_api)]
#![feature(async_fn_in_trait)]
#![feature(abi_x86_interrupt)]
#![feature(const_trait_impl)]
#![feature(atomic_bool_fetch_not)]
pub mod alloc;
pub mod font;
pub mod graphics;
pub mod interrupts;
pub mod lifegame;
pub mod memory;
pub mod multitasking;
pub mod pci;
pub mod serial;
pub mod usb;
pub mod xhci;
