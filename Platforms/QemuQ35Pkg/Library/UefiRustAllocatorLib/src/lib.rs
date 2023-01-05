//! # UEFI Rust Allocator Lib
//! Provides an allocation implementation suitable for use in tracking UEFI
//! memory allocations.

#![no_std]
#![feature(const_mut_refs)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(const_trait_impl)]

pub mod fixed_size_block_allocator;
pub mod uefi_allocator;
