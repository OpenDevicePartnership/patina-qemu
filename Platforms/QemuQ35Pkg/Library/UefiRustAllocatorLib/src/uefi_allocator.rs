//! UEFI Allocator
//!
//! Provides memory-type tracking and UEFI pool allocation semantics on top of [`SpinLockedFixedSizeBlockAllocator`].
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
use r_efi::efi;
use uefi_gcd_lib::gcd::SpinLockedGcd;

use crate::{fixed_size_block_allocator::SpinLockedFixedSizeBlockAllocator, AllocationStrategy};
use core::{
  alloc::{Allocator, GlobalAlloc, Layout},
  ffi::c_void,
  fmt::{self, Display},
  ptr::NonNull,
};

const POOL_SIG: u32 = 0x04151980; //arbitrary number.
const UEFI_POOL_ALIGN: usize = 8; //per UEFI spec.

struct AllocationInfo {
  signature: u32,
  memory_type: efi::MemoryType,
  layout: Layout,
}

/// UEFI Allocator
///
/// Wraps a [`SpinLockedFixedSizeBlockAllocator`] to provide additional UEFI-specific functionality:
/// - Association of a particular [`r_efi::efi::MemoryType`] with the allocator
/// - A pool implementation that allows tracking the layout and memory_type of UEFI pool allocations.
///
/// ## Example:
/// ```
/// # use core::alloc::Layout;
/// # use core::ffi::c_void;
/// # use std::alloc::{GlobalAlloc, System};
/// # use r_efi::efi;
/// # use r_pi::dxe_services;
///
/// use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;
/// use uefi_gcd_lib::gcd::SpinLockedGcd;
/// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
/// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
/// #   let base = unsafe { System.alloc(layout) as u64 };
/// #   unsafe {
/// #     gcd.add_memory_space(
/// #       dxe_services::GcdMemoryType::SystemMemory,
/// #       base as usize,
/// #       size,
/// #       0).unwrap();
/// #   }
/// #   base
/// # }
///
/// static GCD: SpinLockedGcd = SpinLockedGcd::new();
/// GCD.init(48,16); //hard-coded processor address size.
///
/// //initialize the gcd for this example with some memory from the System allocator.
/// let base = init_gcd(&GCD, 0x400000);
///
/// let ua = UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, 1 as _);
///
/// unsafe {
///   let mut buffer: *mut c_void = core::ptr::null_mut();
///   assert!(ua.allocate_pool(0x1000, core::ptr::addr_of_mut!(buffer)) == efi::Status::SUCCESS);
///   assert!(buffer as u64 > base);
///   assert!((buffer as u64) < base + 0x400000);
///   assert!(ua.free_pool(buffer) == efi::Status::SUCCESS);
/// }
/// ```
///
pub struct UefiAllocator {
  allocator: SpinLockedFixedSizeBlockAllocator,
  memory_type: efi::MemoryType,
}

impl UefiAllocator {
  /// Creates a new UEFI allocator using the provided `gcd`.
  ///
  /// See [`SpinLockedFixedSizeBlockAllocator::new`]
  pub const fn new(gcd: &'static SpinLockedGcd, memory_type: efi::MemoryType, allocator_handle: efi::Handle) -> Self {
    UefiAllocator { allocator: SpinLockedFixedSizeBlockAllocator::new(gcd, allocator_handle), memory_type }
  }

  /// Indicates whether the given pointer falls within a memory region managed by this allocator.
  ///
  /// See [`SpinLockedFixedSizeBlockAllocator::contains`]
  pub fn contains(&self, ptr: NonNull<u8>) -> bool {
    self.allocator.contains(ptr)
  }

  /// Returns the UEFI memory type associated with this allocator.
  pub fn memory_type(&self) -> efi::MemoryType {
    self.memory_type
  }

  /// Allocates a buffer to satisfy `size` and returns in `buffer`.
  ///
  /// # Safety
  /// Buffer input must be a valid memory location to write the allocation to.
  ///
  /// Memory allocated by this routine should be freed by [`Self::free_pool`]
  ///
  /// ## Example
  /// ```
  /// # use core::alloc::Layout;
  /// # use core::ffi::c_void;
  /// # use std::alloc::{GlobalAlloc, System};
  /// # use r_efi::efi;
  /// # use r_pi::dxe_services;
  ///
  /// use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;
  /// use uefi_gcd_lib::gcd::SpinLockedGcd;
  /// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  /// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
  /// #   let base = unsafe { System.alloc(layout) as u64 };
  /// #   unsafe {
  /// #     gcd.add_memory_space(
  /// #       dxe_services::GcdMemoryType::SystemMemory,
  /// #       base as usize,
  /// #       size,
  /// #       0).unwrap();
  /// #   }
  /// #   base
  /// # }
  ///
  /// static GCD: SpinLockedGcd = SpinLockedGcd::new();
  /// GCD.init(48,16); //hard-coded processor address size.
  ///
  /// //initialize the gcd for this example with some memory from the System allocator.
  /// let base = init_gcd(&GCD, 0x400000);
  ///
  /// let ua = UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, 1 as _);
  ///
  /// let mut buffer: *mut c_void = core::ptr::null_mut();
  /// unsafe {
  ///   ua.allocate_pool(0x1000, core::ptr::addr_of_mut!(buffer));
  /// }
  /// ```
  pub unsafe fn allocate_pool(&self, size: usize, buffer: *mut *mut c_void) -> efi::Status {
    let mut allocation_info =
      AllocationInfo { signature: POOL_SIG, memory_type: self.memory_type, layout: Layout::new::<AllocationInfo>() };
    let offset: usize;
    (allocation_info.layout, offset) = allocation_info
      .layout
      .extend(
        Layout::from_size_align(size, UEFI_POOL_ALIGN)
          .unwrap_or_else(|err| panic!("Allocation layout error: {:#?}", err)),
      )
      .unwrap_or_else(|err| panic!("Allocation layout error: {:#?}", err));

    match self.allocator.allocate(allocation_info.layout) {
      Ok(ptr) => {
        let alloc_info_ptr = ptr.as_mut_ptr() as *mut AllocationInfo;
        unsafe {
          alloc_info_ptr.write(allocation_info);
          buffer.write((ptr.as_ptr() as *mut u8 as usize + offset) as *mut c_void);
        }
        efi::Status::SUCCESS
      }
      Err(_) => efi::Status::OUT_OF_RESOURCES,
    }
  }

  /// Frees a buffer allocated by [`Self::allocate_pool`]
  ///
  /// ## Safety
  ///
  /// Caller must guarantee that `buffer` was originally allocated by [`Self::allocate_pool`]
  ///
  /// ## Example
  /// ```
  /// # use core::alloc::Layout;
  /// # use core::ffi::c_void;
  /// # use std::alloc::{GlobalAlloc, System};
  /// # use r_efi::efi;
  /// # use r_pi::dxe_services;
  ///
  /// use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;
  /// use uefi_gcd_lib::gcd::SpinLockedGcd;
  /// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  /// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
  /// #   let base = unsafe { System.alloc(layout) as u64 };
  /// #   unsafe {
  /// #     gcd.add_memory_space(
  /// #       dxe_services::GcdMemoryType::SystemMemory,
  /// #       base as usize,
  /// #       size,
  /// #       0).unwrap();
  /// #   }
  /// #   base
  /// # }
  ///
  /// static GCD: SpinLockedGcd = SpinLockedGcd::new();
  /// GCD.init(48,16); //hard-coded processor address size.
  ///
  /// //initialize the gcd for this example with some memory from the System allocator.
  /// let base = init_gcd(&GCD, 0x400000);
  ///
  /// let ua = UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, 1 as _);
  ///
  ///
  /// let mut buffer: *mut c_void = core::ptr::null_mut();
  /// unsafe {
  ///   ua.allocate_pool(0x1000, core::ptr::addr_of_mut!(buffer));
  /// }
  /// //do stuff with the allocation...
  /// unsafe {
  ///   ua.free_pool(buffer);
  /// }
  /// ```
  pub unsafe fn free_pool(&self, buffer: *mut c_void) -> efi::Status {
    let (_, offset) = Layout::new::<AllocationInfo>()
      .extend(
        Layout::from_size_align(0, UEFI_POOL_ALIGN).unwrap_or_else(|err| panic!("Allocation layout error: {:#?}", err)),
      )
      .unwrap_or_else(|err| panic!("Allocation layout error: {:#?}", err));

    let allocation_info: *mut AllocationInfo = ((buffer as usize) - offset) as *mut AllocationInfo;

    //must be true for any pool allocation
    assert!((*allocation_info).signature == POOL_SIG);
    // check if allocation is from this pool.
    if (*allocation_info).memory_type != self.memory_type {
      return efi::Status::NOT_FOUND;
    }
    //zero after check so it doesn't get reused.
    (*allocation_info).signature = 0;
    self.allocator.deallocate(NonNull::new(allocation_info as *mut u8).unwrap(), (*allocation_info).layout);
    efi::Status::SUCCESS
  }

  /// Attempts to allocate the given number of pages according to the given allocation strategy.
  /// Valid allocation strategies are:
  /// - BottomUp(None): Allocate the block of pages from the lowest available free memory.
  /// - BottomUp(Some(address)): Allocate the block of pages from the lowest available free memory. Fail if memory
  ///     cannot be found below `address`.
  /// - TopDown(None): Allocate the block of pages from the highest available free memory.
  /// - TopDown(Some(address)): Allocate the block of pages from the highest available free memory. Fail if memory
  ///      cannot be found above `address`.
  /// - Address(address): Allocate the block of pages at exactly the given address (or fail).
  /// If an address is specified as part of a strategy, it must be page-aligned.
  pub fn allocate_pages(
    &self,
    allocation_strategy: AllocationStrategy,
    pages: usize,
  ) -> Result<core::ptr::NonNull<[u8]>, efi::Status> {
    self.allocator.allocate_pages(allocation_strategy, pages)
  }

  /// Frees the block of pages at the given address of the given size.
  /// ## Safety
  /// Caller must ensure that the given address corresponds to a valid block of pages that was allocated with
  /// [Self::allocate_pages]
  pub unsafe fn free_pages(&self, address: usize, pages: usize) -> Result<(), efi::Status> {
    self.allocator.free_pages(address, pages)
  }
}

unsafe impl GlobalAlloc for UefiAllocator {
  unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
    self.allocator.alloc(layout)
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
    self.allocator.dealloc(ptr, layout)
  }
}

unsafe impl Allocator for UefiAllocator {
  fn allocate(&self, layout: core::alloc::Layout) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
    self.allocator.allocate(layout)
  }
  unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: core::alloc::Layout) {
    self.allocator.deallocate(ptr, layout)
  }
}

// returns a string for the given memory type.
fn string_for_memory_type(memory_type: efi::MemoryType) -> &'static str {
  match memory_type {
    efi::LOADER_CODE => "Loader Code",
    efi::LOADER_DATA => "Loader Data",
    efi::BOOT_SERVICES_CODE => "BootServices Code",
    efi::BOOT_SERVICES_DATA => "BootServices Data",
    efi::RUNTIME_SERVICES_CODE => "RuntimeServices Code",
    efi::RUNTIME_SERVICES_DATA => "RuntimeServices Data",
    efi::ACPI_RECLAIM_MEMORY => "ACPI Reclaim",
    efi::ACPI_MEMORY_NVS => "ACPI NVS",
    _ => "Unknown",
  }
}

impl Display for UefiAllocator {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "Memory Type: {} ", string_for_memory_type(self.memory_type))?;
    self.allocator.fmt(f)
  }
}
#[cfg(test)]
mod tests {
  extern crate std;
  use std::alloc::{GlobalAlloc, System};

  use r_pi::dxe_services;

  use super::*;
  use crate::AllocationStrategy;

  fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
    let layout = Layout::from_size_align(size, 0x1000).unwrap();
    let base = unsafe { System.alloc(layout) as u64 };
    unsafe {
      gcd.add_memory_space(dxe_services::GcdMemoryType::SystemMemory, base as usize, size, 0).unwrap();
    }
    base
  }

  #[test]
  fn test_uefi_allocator_new() {
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);
    let ua = UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, 1 as _);
    assert_eq!(ua.memory_type, efi::BOOT_SERVICES_DATA);
  }

  #[test]
  fn test_allocate_pool() {
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    let base = init_gcd(&GCD, 0x400000);

    let ua = UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, 1 as _);

    let mut buffer: *mut c_void = core::ptr::null_mut();
    assert_eq!(unsafe { ua.allocate_pool(0x1000, core::ptr::addr_of_mut!(buffer)) }, efi::Status::SUCCESS);
    assert!(buffer as u64 > base);
    assert!((buffer as u64) < base + 0x400000);

    let (layout, offset) = Layout::new::<AllocationInfo>()
      .extend(
        Layout::from_size_align(0x1000, UEFI_POOL_ALIGN)
          .unwrap_or_else(|err| panic!("Allocation layout error: {:#?}", err)),
      )
      .unwrap_or_else(|err| panic!("Allocation layout error: {:#?}", err));

    let allocation_info: *mut AllocationInfo = ((buffer as usize) - offset) as *mut AllocationInfo;
    unsafe {
      let allocation_info = &*allocation_info;
      assert_eq!(allocation_info.signature, POOL_SIG);
      assert_eq!(allocation_info.memory_type, efi::BOOT_SERVICES_DATA);
      assert_eq!(allocation_info.layout, layout)
    }
  }

  #[test]
  fn test_free_pool() {
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    let base = init_gcd(&GCD, 0x400000);

    let ua = UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, 1 as _);

    let mut buffer: *mut c_void = core::ptr::null_mut();
    assert_eq!(unsafe { ua.allocate_pool(0x1000, core::ptr::addr_of_mut!(buffer)) }, efi::Status::SUCCESS);

    assert!(unsafe { ua.free_pool(buffer) } == efi::Status::SUCCESS);

    let (_, offset) = Layout::new::<AllocationInfo>()
      .extend(
        Layout::from_size_align(0x1000, UEFI_POOL_ALIGN)
          .unwrap_or_else(|err| panic!("Allocation layout error: {:#?}", err)),
      )
      .unwrap_or_else(|err| panic!("Allocation layout error: {:#?}", err));

    let allocation_info: *mut AllocationInfo = ((buffer as usize) - offset) as *mut AllocationInfo;
    unsafe {
      let allocation_info = &*allocation_info;
      assert_eq!(allocation_info.signature, 0);
    }

    let prev_buffer = buffer;
    assert_eq!(unsafe { ua.allocate_pool(0x1000, core::ptr::addr_of_mut!(buffer)) }, efi::Status::SUCCESS);
    assert!(buffer as u64 > base);
    assert!((buffer as u64) < base + 0x400000);
    assert_eq!(buffer, prev_buffer);
  }

  #[test]
  fn test_allocate_and_free_pages() {
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    let base = init_gcd(&GCD, 0x400000);

    let ua = UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, 1 as _);

    let buffer = ua.allocate_pages(AllocationStrategy::BottomUp(None), 4).unwrap();
    let buffer_address = buffer.as_ptr() as *mut u8 as u64;
    assert_eq!(buffer_address & 0xFFF, 0); // must be page aligned.
    assert_eq!(buffer.len(), 0x1000 * 4); //should be 4 pages in size.
    assert!(buffer_address >= base);
    assert!(buffer_address < base + 0x400000);

    unsafe {
      ua.free_pages(buffer_address as usize, 4).unwrap();
    }

    let buffer = ua.allocate_pages(AllocationStrategy::Address(buffer_address as usize), 4).unwrap();
    let buffer_address2 = buffer.as_ptr() as *mut u8 as u64;
    assert_eq!(buffer_address, buffer_address2);
    assert_eq!(buffer.len(), 0x1000 * 4); //should be 4 pages in size.

    unsafe {
      ua.free_pages(buffer_address2 as usize, 4).unwrap();
    }
  }
}
