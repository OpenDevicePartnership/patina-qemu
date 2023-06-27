//! UEFI Rust Allocator Lib
//!
//! Provides an allocator implementation suitable for use in tracking UEFI memory allocations.
//!
//! The foundation of the implementation is
//! [`FixedSizeBlockAllocator`](`fixed_size_block_allocator::FixedSizeBlockAllocator`), which provides a fixed-sized block
//! allocator backed by a linked list allocator, the design of which is based on
//! <https://os.phil-opp.com/allocator-designs/#fixed-size-block-allocator>.
//!
//! A spin-locked version of the implementation is available as
//! [`SpinLockedFixedSizeBlockAllocator`](`fixed_size_block_allocator::SpinLockedFixedSizeBlockAllocator`) which is
//! suitable for use as a global allocator.
//!
//! In addition, [`UefiAllocator`](`uefi_allocator::UefiAllocator`) provides an implementation on top of
//! [`SpinLockedFixedSizeBlockAllocator`](`fixed_size_block_allocator::SpinLockedFixedSizeBlockAllocator`) which
//! implements UEFI's pool semantics and adds support for assigning a UEFI memory type to a particular allocator.
//!
//! ## Examples and Usage
//!
//! Declaring a set of UEFI allocators as global static allocators and setting one of them as the system allocator:
//!
//! ```no_run
//! # use r_efi::efi::BOOT_SERVICES_CODE;
//! # use r_efi::efi::BOOT_SERVICES_DATA;
//! use dynamic_frame_allocator_lib::SpinLockedDynamicFrameAllocator;
//! use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;
//! static FRAME_ALLOCATOR: SpinLockedDynamicFrameAllocator = SpinLockedDynamicFrameAllocator::new();
//! //EfiBootServicesCode
//! pub static EFI_BOOT_SERVICES_CODE_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, BOOT_SERVICES_CODE);
//! //EfiBootServicesData - (use as global allocator)
//! #[global_allocator]
//! pub static EFI_BOOT_SERVICES_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, BOOT_SERVICES_DATA);
//! ```
//!
//! Allocating memory in a particular allocator using Box:
//! ```
//! #![feature(allocator_api)]
//! # use core::alloc::Layout;
//! # use core::ffi::c_void;
//! # use r_efi::efi::BOOT_SERVICES_DATA;
//! # use r_efi::efi::RUNTIME_SERVICES_DATA;
//! # use std::alloc::System;
//! # use std::alloc::GlobalAlloc;
//! use dynamic_frame_allocator_lib::SpinLockedDynamicFrameAllocator;
//! use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;
//! # fn init_frame_allocator(frame_allocator: &SpinLockedDynamicFrameAllocator, size: usize) -> u64 {
//! #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
//! #   let base = unsafe { System.alloc(layout) as u64 };
//! #   unsafe {
//! #     frame_allocator.lock().add_physical_region(base, size as u64).unwrap();
//! #   }
//! #   base
//! # }
//!
//! static FRAME_ALLOCATOR: SpinLockedDynamicFrameAllocator = SpinLockedDynamicFrameAllocator::new();
//! let base = init_frame_allocator(&FRAME_ALLOCATOR, 0x400000);
//!
//! pub static EFI_BOOT_SERVICES_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, BOOT_SERVICES_DATA);
//! pub static EFI_RUNTIME_SERVICES_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, RUNTIME_SERVICES_DATA);
//!
//! //Allocate a box in Boot Services Data
//! let boot_box = Box::new_in(5, &EFI_BOOT_SERVICES_DATA_ALLOCATOR);
//!
//! //Allocate a box in Runtime Services Data
//! let runtime_box = Box::new_in(10, &EFI_RUNTIME_SERVICES_DATA_ALLOCATOR);
//!
//! ```
//!
//! Using UEFI allocator pool semantics:
//! ```
//! # use core::alloc::Layout;
//! # use core::ffi::c_void;
//! # use std::alloc::System;
//! # use std::alloc::GlobalAlloc;
//! use dynamic_frame_allocator_lib::SpinLockedDynamicFrameAllocator;
//! use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;
//! # fn init_frame_allocator(frame_allocator: &SpinLockedDynamicFrameAllocator, size: usize) -> u64 {
//! #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
//! #   let base = unsafe { System.alloc(layout) as u64 };
//! #   unsafe {
//! #     frame_allocator.lock().add_physical_region(base, size as u64).unwrap();
//! #   }
//! #   base
//! # }
//!
//! static FRAME_ALLOCATOR: SpinLockedDynamicFrameAllocator = SpinLockedDynamicFrameAllocator::new();
//! let base = init_frame_allocator(&FRAME_ALLOCATOR, 0x400000);
//!
//! let ua = UefiAllocator::new(&FRAME_ALLOCATOR, r_efi::efi::BOOT_SERVICES_DATA);
//!
//! let mut buffer: *mut c_void = core::ptr::null_mut();
//! assert!(ua.allocate_pool(0x1000, core::ptr::addr_of_mut!(buffer)) == r_efi::efi::Status::SUCCESS);
//! assert!(buffer as u64 > base);
//! assert!((buffer as u64) < base + 0x400000);
//! assert!(unsafe { ua.free_pool(buffer) } == r_efi::efi::Status::SUCCESS);
//! ```
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!

#![no_std]
#![feature(const_mut_refs)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(const_trait_impl)]

pub mod fixed_size_block_allocator;
pub mod uefi_allocator;
