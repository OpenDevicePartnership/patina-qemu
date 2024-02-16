//! Fixed-sized block allocator.
//!
//! Implements a fixed-sized block allocator backed by a linked list allocator. Based on the example fixed-sized block
//! allocator presented here: <https://os.phil-opp.com/allocator-designs/#fixed-size-block-allocator>.
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!

extern crate alloc;
use crate::AllocationStrategy;
use core::{
  alloc::{AllocError, Allocator, GlobalAlloc, Layout},
  cmp::max,
  fmt::{self, Display},
  mem::{align_of, size_of},
  ptr::{self, slice_from_raw_parts_mut, NonNull},
};
use linked_list_allocator::{align_down_size, align_up_size};
use r_efi::efi;
use r_pi::dxe_services::GcdMemoryType;
use uefi_gcd_lib::gcd::SpinLockedGcd;

/// Type for describing errors that this implementation can produce.
#[derive(Debug)]
pub enum FixedSizeBlockAllocatorError {
  /// Could not satisfy allocation request, and expansion failed.
  OutOfMemory,
}

/// Minimum expansion size - allocator will request at least this much memory
/// from the underlying GCD instance expansion is needed.
pub const MIN_EXPANSION: usize = 0x100000;
const ALIGNMENT_BITS: u32 = 12;
const ALIGNMENT: usize = 0x1000;

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

// Returns the index in the block list for the minimum size block that will
// satisfy allocation for the given layout
fn list_index(layout: &Layout) -> Option<usize> {
  let required_block_size = layout.size().max(layout.align());
  BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

struct BlockListNode {
  next: Option<&'static mut BlockListNode>,
}

struct AllocatorListNode {
  next: Option<*mut AllocatorListNode>,
  allocator: linked_list_allocator::Heap,
}
struct AllocatorIterator {
  current: Option<*mut AllocatorListNode>,
}

impl AllocatorIterator {
  fn new(start_node: Option<*mut AllocatorListNode>) -> Self {
    AllocatorIterator { current: start_node }
  }
}

impl Iterator for AllocatorIterator {
  type Item = *mut AllocatorListNode;
  fn next(&mut self) -> Option<*mut AllocatorListNode> {
    if let Some(current) = self.current {
      self.current = unsafe { (*current).next };
      Some(current)
    } else {
      None
    }
  }
}

/// Fixed Size Block Allocator
///
/// Implements an expandable memory allocator using fixed-sized blocks for speed backed by a linked-list allocator
/// implementation when an appropriate sized free block is not available. If more memory is required than can be
/// satisfied by either the block list or the linked-list, more memory is requested from the GCD supplied at
/// instantiation and a new backing linked-list is created.
///
/// ## Example
/// ```
/// # use core::alloc::Layout;
/// # use std::alloc::System;
/// # use std::alloc::GlobalAlloc;
/// # use r_pi::dxe_services::GcdMemoryType;
///
/// use uefi_gcd_lib::gcd::SpinLockedGcd;
/// use uefi_rust_allocator_lib::fixed_size_block_allocator::FixedSizeBlockAllocator;
/// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
/// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
/// #   let base = unsafe { System.alloc(layout) as u64 };
/// #   unsafe {
/// #     gcd.add_memory_space(
/// #       GcdMemoryType::SystemMemory,
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
/// let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
///
/// let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
/// let allocation = fsb.allocate(layout).unwrap().as_ptr() as *mut u8;
///
/// assert_ne!(allocation, core::ptr::null_mut());
/// ```
///
pub struct FixedSizeBlockAllocator {
  gcd: &'static SpinLockedGcd,
  handle: r_efi::efi::Handle,
  list_heads: [Option<&'static mut BlockListNode>; BLOCK_SIZES.len()],
  allocators: Option<*mut AllocatorListNode>,
}

impl FixedSizeBlockAllocator {
  /// Creates a new empty FixedSizeBlockAllocator that will request memory from `gcd` as needed to satisfy
  /// requests.
  pub const fn new(gcd: &'static SpinLockedGcd, allocator_handle: r_efi::efi::Handle) -> Self {
    const EMPTY: Option<&'static mut BlockListNode> = None;
    FixedSizeBlockAllocator { gcd, handle: allocator_handle, list_heads: [EMPTY; BLOCK_SIZES.len()], allocators: None }
  }

  // Expand the memory available to this allocator by requesting a new contiguous region of memory from the gcd setting
  // up a new allocator node to manage this range
  fn expand(&mut self, layout: Layout) -> Result<(), FixedSizeBlockAllocatorError> {
    let size = layout.pad_to_align().size() + Layout::new::<AllocatorListNode>().pad_to_align().size();
    let size = max(size, MIN_EXPANSION);
    //ensure size is a multiple of alignment to avoid fragmentation.
    let size = align_up_size(size, ALIGNMENT);
    //Allocate memory from the gcd.
    let start_address = self
      .gcd
      .allocate_memory_space(
        uefi_gcd_lib::gcd::AllocateType::BottomUp(None),
        GcdMemoryType::SystemMemory,
        ALIGNMENT_BITS,
        size,
        self.handle,
        None,
      )
      .map_err(|_| FixedSizeBlockAllocatorError::OutOfMemory)?;

    //set up the new allocator, reserving space at the beginning of the range for the AllocatorListNode structure.
    let start_address = start_address as usize;
    let size = size as usize;

    let heap_bottom = start_address + size_of::<AllocatorListNode>();
    let heap_size = size - (heap_bottom - start_address);

    let alloc_node_ptr = start_address as *mut AllocatorListNode;
    let node = AllocatorListNode { next: None, allocator: linked_list_allocator::Heap::empty() };

    //write the allocator node structure into the start of the range, initialize its heap with the remainder of
    //the range, and add the new allocator to the front of the allocator list.
    unsafe {
      alloc_node_ptr.write(node);
      (*alloc_node_ptr).allocator.init(heap_bottom as *mut u8, heap_size);
      (*alloc_node_ptr).next = self.allocators;
    }

    self.allocators = Some(alloc_node_ptr);

    Ok(())
  }

  // allocates from the linked-list backing allocator if a free block of the
  // appropriate size is not available.
  fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
    for node in AllocatorIterator::new(self.allocators) {
      let allocator = unsafe { &mut (*node).allocator };
      if let Ok(ptr) = allocator.allocate_first_fit(layout) {
        return ptr.as_ptr();
      }
    }
    //if we get here, then allocation failed in all current allocation ranges.
    //attempt to expand and then allocate again
    if self.expand(layout).is_err() {
      return ptr::null_mut();
    }
    self.fallback_alloc(layout)
  }

  /// Allocates and returns a pointer to a memory buffer for the given layout.
  ///
  /// This routine is designed to satisfy the [`GlobalAlloc`] trait, except that it requires a mutable self.
  /// [`SpinLockedFixedSizeBlockAllocator`] provides a [`GlobalAlloc`] trait impl by wrapping this routine.
  ///
  /// Memory allocated by this routine should be deallocated with
  /// [`Self::dealloc`]
  ///
  /// ## Errors
  ///
  /// Returns [`core::ptr::null_mut()`] on failure to allocate.
  ///
  /// ## Example
  /// ```
  /// # use core::alloc::Layout;
  /// # use std::alloc::System;
  /// # use std::alloc::GlobalAlloc;
  /// # use r_pi::dxe_services::GcdMemoryType;
  ///
  /// use uefi_gcd_lib::gcd::SpinLockedGcd;
  /// use uefi_rust_allocator_lib::fixed_size_block_allocator::FixedSizeBlockAllocator;
  /// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  /// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
  /// #   let base = unsafe { System.alloc(layout) as u64 };
  /// #   unsafe {
  /// #     gcd.add_memory_space(
  /// #       GcdMemoryType::SystemMemory,
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
  /// //initialize the gcd allocator for this example with some memory from the System allocator.
  /// let base = init_gcd(&GCD, 0x400000);
  ///
  /// let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
  ///
  /// let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
  /// let allocation = fsb.alloc(layout);
  ///
  /// assert_ne!(allocation, core::ptr::null_mut());
  /// ```
  ///
  pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
    match list_index(&layout) {
      Some(index) => {
        match self.list_heads[index].take() {
          Some(node) => {
            self.list_heads[index] = node.next.take();
            node as *mut BlockListNode as *mut u8
          }
          None => {
            // no block exists in list => allocate new block
            let block_size = BLOCK_SIZES[index];
            // only works if all block sizes are a power of 2
            let block_align = block_size;
            let layout = Layout::from_size_align(block_size, block_align).unwrap();
            self.fallback_alloc(layout)
          }
        }
      }
      None => self.fallback_alloc(layout),
    }
  }

  /// Allocates and returns a NonNull byte slice for the given layout.
  ///
  /// This routine is designed to satisfy the [`Allocator`] trait, except that it  requires a mutable self.
  /// [`SpinLockedFixedSizeBlockAllocator`] provides an [`Allocator`] trait impl by wrapping this routine.
  ///
  /// Memory allocated by this routine should be deallocated with
  /// [`Self::deallocate`]
  ///
  /// ## Errors
  ///
  /// returns AllocError on failure to allocate.
  ///
  /// ## Example
  /// ```
  /// # use core::alloc::Layout;
  /// # use std::alloc::System;
  /// # use std::alloc::GlobalAlloc;
  /// # use r_pi::dxe_services::GcdMemoryType;
  ///
  /// use uefi_gcd_lib::gcd::SpinLockedGcd;
  /// use uefi_rust_allocator_lib::fixed_size_block_allocator::FixedSizeBlockAllocator;
  /// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  /// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
  /// #   let base = unsafe { System.alloc(layout) as u64 };
  /// #   unsafe {
  /// #     gcd.add_memory_space(
  /// #       GcdMemoryType::SystemMemory,
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
  /// let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
  ///
  /// let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
  /// let allocation = fsb.allocate(layout).unwrap().as_ptr() as *mut u8;
  ///
  /// assert_ne!(allocation, core::ptr::null_mut());
  /// ```
  pub fn allocate(&mut self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    let allocation = self.alloc(layout);
    let allocation = slice_from_raw_parts_mut(allocation, layout.size());
    let allocation = NonNull::new(allocation).ok_or(AllocError)?;
    Ok(allocation)
  }

  // deallocates back to the linked-list backing allocator if the size of
  // layout being freed is too big to be tracked as a fixed-size free block.
  fn fallback_dealloc(&mut self, ptr: *mut u8, layout: Layout) {
    let ptr = NonNull::new(ptr).unwrap();
    for node in AllocatorIterator::new(self.allocators) {
      let allocator = unsafe { &mut (*node).allocator };
      if (allocator.bottom() <= ptr.as_ptr()) && (ptr.as_ptr() < allocator.top()) {
        unsafe { allocator.deallocate(ptr, layout) };
      }
    }
  }

  /// Deallocates a buffer allocated by [`Self::alloc`].
  ///
  /// This routine is designed to satisfy the [`GlobalAlloc`] trait, except  that it requires a mutable self.
  /// [`SpinLockedFixedSizeBlockAllocator`] provides a [`GlobalAlloc`] trait impl by wrapping this routine.
  ///
  /// ## Safety
  ///
  /// Caller must ensure that `ptr` was created by a call to [`Self::alloc`] with the same `layout`.
  ///
  /// ## Example
  /// ```
  /// # use core::alloc::Layout;
  /// # use std::alloc::System;
  /// # use std::alloc::GlobalAlloc;
  /// # use r_pi::dxe_services::GcdMemoryType;
  ///
  /// use uefi_gcd_lib::gcd::SpinLockedGcd;
  /// use uefi_rust_allocator_lib::fixed_size_block_allocator::FixedSizeBlockAllocator;
  /// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  /// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
  /// #   let base = unsafe { System.alloc(layout) as u64 };
  /// #   unsafe {
  /// #     gcd.add_memory_space(
  /// #       GcdMemoryType::SystemMemory,
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
  /// let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
  ///
  /// let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
  /// let allocation = fsb.alloc(layout);
  ///
  /// unsafe {
  ///   fsb.dealloc(allocation, layout);
  /// }
  /// ```
  pub unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
    match list_index(&layout) {
      Some(index) => {
        let new_node = BlockListNode { next: self.list_heads[index].take() };
        // verify that block has size and alignment required for storing node
        assert!(size_of::<BlockListNode>() <= BLOCK_SIZES[index]);
        assert!(align_of::<BlockListNode>() <= BLOCK_SIZES[index]);
        let new_node_ptr = ptr as *mut BlockListNode;
        unsafe {
          new_node_ptr.write(new_node);
          self.list_heads[index] = Some(&mut *new_node_ptr);
        }
      }
      None => {
        self.fallback_dealloc(ptr, layout);
      }
    }
  }

  /// Deallocates a buffer allocated by [`Self::allocate`] .
  ///
  /// This routine is designed to satisfy the [`Allocator`] trait, except that it requires a mutable self.
  /// [`SpinLockedFixedSizeBlockAllocator`] provides an [`Allocator`] trait impl by wrapping this routine.
  ///
  /// ## Safety
  ///
  /// Caller must ensure that `ptr` was created by a call to [`Self::allocate`] with the same `layout`.
  ///
  /// ## Example
  /// ```
  /// #![feature(slice_ptr_get)]
  /// # use core::alloc::Layout;
  /// # use std::alloc::System;
  /// # use std::alloc::GlobalAlloc;
  /// # use r_pi::dxe_services::GcdMemoryType;
  ///
  /// use uefi_gcd_lib::gcd::SpinLockedGcd;
  /// use uefi_rust_allocator_lib::fixed_size_block_allocator::FixedSizeBlockAllocator;
  /// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  /// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
  /// #   let base = unsafe { System.alloc(layout) as u64 };
  /// #   unsafe {
  /// #     gcd.add_memory_space(
  /// #       GcdMemoryType::SystemMemory,
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
  /// let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
  ///
  /// let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
  /// let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();
  ///
  /// unsafe {
  ///   fsb.deallocate(allocation, layout);
  /// }
  /// ```
  ///
  pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
    self.dealloc(ptr.as_ptr(), layout)
  }

  /// Indicates whether the given pointer falls within a memory region managed by this allocator.
  ///
  /// Note: `true` does not indicate that the pointer corresponds to an active allocation - it may be in either
  /// allocated or freed memory. `true` just means that the pointer falls within a memory region that this allocator
  /// manages.
  ///
  /// ## Example
  /// ```
  /// #![feature(slice_ptr_get)]
  /// # use core::alloc::Layout;
  /// # use std::alloc::System;
  /// # use std::alloc::GlobalAlloc;
  /// # use r_pi::dxe_services::GcdMemoryType;
  ///
  /// use uefi_gcd_lib::gcd::SpinLockedGcd;
  /// use uefi_rust_allocator_lib::fixed_size_block_allocator::FixedSizeBlockAllocator;
  /// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  /// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
  /// #   let base = unsafe { System.alloc(layout) as u64 };
  /// #   unsafe {
  /// #     gcd.add_memory_space(
  /// #       GcdMemoryType::SystemMemory,
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
  /// let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
  ///
  /// let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
  /// let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();
  ///
  /// assert!(fsb.contains(allocation.as_ptr() as *mut u8));
  ///
  /// unsafe {
  ///   fsb.deallocate(allocation, layout);
  /// }
  ///
  /// // even though it is not allocated, this address now belongs to this allocator's managed pool of memory.
  /// assert!(fsb.contains(allocation.as_ptr() as *mut u8));
  /// ```
  ///
  pub fn contains(&self, ptr: *mut u8) -> bool {
    AllocatorIterator::new(self.allocators).any(|node| {
      let allocator = unsafe { &mut (*node).allocator };
      (allocator.bottom() <= ptr) && (ptr < allocator.top())
    })
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
    &mut self,
    allocation_strategy: AllocationStrategy,
    pages: usize,
  ) -> Result<core::ptr::NonNull<[u8]>, efi::Status> {
    // validate allocation strategy addresses for direct address allocation is properly aligned.
    // for BottomUp and TopDown strategies, the address parameter doesn't have to be page-aligned, but
    // the resulting allocation will be page-aligned.
    if let AllocationStrategy::Address(address) = allocation_strategy {
      if address % ALIGNMENT != 0 {
        return Err(efi::Status::INVALID_PARAMETER);
      }
    }

    // Page allocations and pool allocations are disjoint; page allocations are allocated directly from the GCD and are
    // freed straight back to GCD. As such, a tracking allocator structure is not required.
    let start_address = self
      .gcd
      .allocate_memory_space(
        allocation_strategy,
        GcdMemoryType::SystemMemory,
        ALIGNMENT_BITS,
        pages * ALIGNMENT,
        self.handle,
        None,
      )
      .map_err(|err| match err {
        uefi_gcd_lib::gcd::Error::InvalidParameter => efi::Status::INVALID_PARAMETER,
        uefi_gcd_lib::gcd::Error::NotFound => efi::Status::NOT_FOUND,
        _ => efi::Status::OUT_OF_RESOURCES,
      })?;

    let allocation = slice_from_raw_parts_mut(start_address as *mut u8, pages * ALIGNMENT);
    let allocation = NonNull::new(allocation).ok_or(efi::Status::OUT_OF_RESOURCES)?;
    Ok(allocation)
  }

  /// Frees the block of pages at the given address of the given size.
  /// ## Safety
  /// Caller must ensure that the given address corresponds to a valid block of pages that was allocated with
  /// [Self::allocate_pages]
  pub unsafe fn free_pages(&mut self, address: usize, pages: usize) -> Result<(), efi::Status> {
    if address % ALIGNMENT != 0 {
      return Err(efi::Status::INVALID_PARAMETER);
    }

    self.gcd.free_memory_space(address, pages * ALIGNMENT).map_err(|err| match err {
      uefi_gcd_lib::gcd::Error::NotFound => efi::Status::NOT_FOUND,
      _ => efi::Status::INVALID_PARAMETER,
    })?;

    Ok(())
  }
}

impl Display for FixedSizeBlockAllocator {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    writeln!(f, "Allocation Ranges:")?;
    for node in AllocatorIterator::new(self.allocators) {
      let allocator = unsafe { &mut (*node).allocator };
      writeln!(
        f,
        "  PhysRange: {:#x}-{:#x}, Size: {:#x}, Used: {:#x} Free: {:#x} Maint: {:#x}",
        align_down_size(allocator.bottom() as usize, 0x1000), //account for AllocatorListNode
        allocator.top() as usize,
        align_up_size(allocator.size(), 0x1000), //account for AllocatorListNode
        allocator.used(),
        allocator.free(),
        align_up_size(allocator.size(), 0x100) - allocator.size()
      )?;
    }
    Ok(())
  }
}

/// Spin Locked Fixed Size Block Allocator
///
/// A wrapper for [`FixedSizeBlockAllocator`] that provides Sync/Send via means of a spin mutex.
///
/// ## Example
/// ```
/// #![feature(allocator_api)]
/// # use core::alloc::Layout;
/// # use core::alloc::Allocator;
/// # use core::alloc::GlobalAlloc;
/// # use std::alloc::System;
/// # use r_pi::dxe_services::GcdMemoryType;
///
/// use uefi_gcd_lib::gcd::SpinLockedGcd;
/// use uefi_rust_allocator_lib::fixed_size_block_allocator::SpinLockedFixedSizeBlockAllocator;
/// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
/// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
/// #   let base = unsafe { System.alloc(layout) as u64 };
/// #   unsafe {
/// #     gcd.add_memory_space(
/// #       GcdMemoryType::SystemMemory,
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
///static ALLOCATOR: SpinLockedFixedSizeBlockAllocator  = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);
///
/// //initialize the gcd for this example with some memory from the System allocator.
/// let base = init_gcd(&GCD, 0x400000);
///
/// let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
/// let allocation = ALLOCATOR.allocate(layout).unwrap().as_ptr() as *mut u8;
///
/// assert_ne!(allocation, core::ptr::null_mut());
/// ```
///
pub struct SpinLockedFixedSizeBlockAllocator {
  inner: tpl_lock::TplMutex<FixedSizeBlockAllocator>,
}

impl SpinLockedFixedSizeBlockAllocator {
  /// Creates a new empty FixedSizeBlockAllocator that will request memory from `gcd` as needed to satisfy
  /// requests.
  pub const fn new(gcd: &'static SpinLockedGcd, allocator_handle: r_efi::efi::Handle) -> Self {
    SpinLockedFixedSizeBlockAllocator {
      inner: tpl_lock::TplMutex::new(
        efi::TPL_HIGH_LEVEL,
        FixedSizeBlockAllocator::new(gcd, allocator_handle),
        "FsbLock",
      ),
    }
  }

  /// Locks the allocator
  ///
  /// This can be used to do several actions on the allocator atomically.
  ///
  /// ## Example
  /// ```
  /// #![feature(allocator_api)]
  /// #![feature(slice_ptr_get)]
  /// # use core::alloc::Layout;
  /// # use core::alloc::Allocator;
  /// # use core::alloc::GlobalAlloc;
  /// # use std::alloc::System;
  /// # use r_pi::dxe_services::GcdMemoryType;
  ///
  /// use uefi_gcd_lib::gcd::SpinLockedGcd;
  /// use uefi_rust_allocator_lib::fixed_size_block_allocator::SpinLockedFixedSizeBlockAllocator;
  /// # fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  /// #   let layout = Layout::from_size_align(size, 0x1000).unwrap();
  /// #   let base = unsafe { System.alloc(layout) as u64 };
  /// #   unsafe {
  /// #     gcd.add_memory_space(
  /// #       GcdMemoryType::SystemMemory,
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
  /// static ALLOCATOR: SpinLockedFixedSizeBlockAllocator  = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);
  ///
  /// //initialize the gcd for this example with some memory from the System allocator.
  /// let base = init_gcd(&GCD, 0x400000);
  ///
  /// let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
  ///
  /// {
  ///   //acquire the lock
  ///   let mut locked_alloc = ALLOCATOR.lock();
  ///   //atomic operations
  ///   let allocation = locked_alloc.allocate(layout).unwrap().as_non_null_ptr();
  ///   let allocation2 = locked_alloc.allocate(layout).unwrap().as_non_null_ptr();
  ///   unsafe {
  ///     locked_alloc.deallocate(allocation, layout);
  ///     locked_alloc.deallocate(allocation2, layout);
  ///   }
  /// }
  ///
  /// ```
  ///
  pub fn lock(&self) -> tpl_lock::TplGuard<FixedSizeBlockAllocator> {
    self.inner.lock()
  }

  /// Indicates whether the given pointer falls within a memory region managed by this allocator.
  ///
  /// See [`FixedSizeBlockAllocator::contains()`]
  pub fn contains(&self, ptr: NonNull<u8>) -> bool {
    self.lock().contains(ptr.as_ptr())
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
    self.lock().allocate_pages(allocation_strategy, pages)
  }

  /// Frees the block of pages at the given address of the given size.
  /// ## Safety
  /// Caller must ensure that the given address corresponds to a valid block of pages that was allocated with
  /// [Self::allocate_pages]
  pub unsafe fn free_pages(&self, address: usize, pages: usize) -> Result<(), efi::Status> {
    self.lock().free_pages(address, pages)
  }

  /// Returns the allocator handle associated with this allocator.
  pub fn handle(&self) -> efi::Handle {
    self.inner.lock().handle
  }
}

unsafe impl GlobalAlloc for SpinLockedFixedSizeBlockAllocator {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    self.lock().alloc(layout)
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    self.lock().dealloc(ptr, layout)
  }
}

unsafe impl Allocator for SpinLockedFixedSizeBlockAllocator {
  fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    self.lock().allocate(layout)
  }
  unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
    self.lock().deallocate(ptr, layout)
  }
}

impl Display for SpinLockedFixedSizeBlockAllocator {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.lock().fmt(f)
  }
}

unsafe impl Sync for SpinLockedFixedSizeBlockAllocator {}
unsafe impl Send for SpinLockedFixedSizeBlockAllocator {}

#[cfg(test)]
mod tests {
  extern crate std;
  use core::alloc::GlobalAlloc;
  use std::alloc::System;

  use super::*;

  fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
    let layout = Layout::from_size_align(size, 0x1000).unwrap();
    let base = unsafe { System.alloc(layout) as u64 };
    unsafe {
      gcd.add_memory_space(GcdMemoryType::SystemMemory, base as usize, size, 0).unwrap();
    }
    base
  }

  #[test]
  fn test_list_index() {
    let layout = Layout::from_size_align(8, 1).unwrap();
    assert_eq!(list_index(&layout), Some(0));

    let layout = Layout::from_size_align(12, 8).unwrap();
    assert_eq!(list_index(&layout), Some(1));

    let layout = Layout::from_size_align(8, 32).unwrap();
    assert_eq!(list_index(&layout), Some(2));

    let layout = Layout::from_size_align(4096, 32).unwrap();
    assert_eq!(list_index(&layout), Some(9));

    let layout = Layout::from_size_align(1, 4096).unwrap();
    assert_eq!(list_index(&layout), Some(9));

    let layout = Layout::from_size_align(8192, 1).unwrap();
    assert_eq!(list_index(&layout), None);
  }

  #[test]
  fn test_construct_empty_fixed_size_block_allocator() {
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);
    let fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
    assert!(core::ptr::eq(fsb.gcd, &GCD));
    assert!(fsb.list_heads.iter().all(|x| x.is_none()));
    assert!(fsb.allocators.is_none());
  }

  #[test]
  fn test_expand() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    let base = init_gcd(&GCD, 0x400000);

    //verify no allocators exist before expand.
    let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
    assert!(fsb.allocators.is_none());

    //expand by a page. This will round up to MIN_EXPANSION.
    let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
    fsb.expand(layout).unwrap();
    assert!(fsb.allocators.is_some());
    unsafe {
      assert!((*fsb.allocators.unwrap()).next.is_none());
      assert!((*fsb.allocators.unwrap()).allocator.bottom() as usize > base as usize);
      assert_eq!((*fsb.allocators.unwrap()).allocator.free(), MIN_EXPANSION - size_of::<AllocatorListNode>());
    }
    //expand by larger than MIN_EXPANSION.
    let layout = Layout::from_size_align(MIN_EXPANSION + 0x1000, 0x10).unwrap();
    fsb.expand(layout).unwrap();
    assert!(fsb.allocators.is_some());
    unsafe {
      assert!((*fsb.allocators.unwrap()).next.is_some());
      assert!((*(*fsb.allocators.unwrap()).next.unwrap()).next.is_none());
      assert!((*fsb.allocators.unwrap()).allocator.bottom() as usize > base as usize);
      assert_eq!(
        (*fsb.allocators.unwrap()).allocator.free(),
        //expected free: size + a page to hold allocator node - size of allocator node.
        layout.pad_to_align().size() + 0x1000 - size_of::<AllocatorListNode>()
      );
    }
  }

  #[test]
  fn test_allocation_iterator() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    init_gcd(&GCD, 0x800000);

    let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);
    let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
    fsb.expand(layout).unwrap();
    fsb.expand(layout).unwrap();
    fsb.expand(layout).unwrap();
    fsb.expand(layout).unwrap();
    fsb.expand(layout).unwrap();

    assert_eq!(5, AllocatorIterator::new(fsb.allocators).count());
    assert!(AllocatorIterator::new(fsb.allocators)
      .all(|node| unsafe { (*node).allocator.free() == MIN_EXPANSION - size_of::<AllocatorListNode>() }));
  }

  #[test]
  fn test_fallback_alloc() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    let base = init_gcd(&GCD, 0x400000);

    let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);

    let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
    let allocation = fsb.fallback_alloc(layout);
    assert!(fsb.allocators.is_some());
    assert!((allocation as u64) > base);
    assert!((allocation as u64) < base + MIN_EXPANSION as u64);
  }

  #[test]
  fn test_alloc() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    let base = init_gcd(&GCD, 0x400000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
    let allocation = unsafe { fsb.alloc(layout) };
    assert!(fsb.lock().allocators.is_some());
    assert!((allocation as u64) > base);
    assert!((allocation as u64) < base + MIN_EXPANSION as u64);
  }

  #[test]
  fn test_allocate() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    let base = init_gcd(&GCD, 0x400000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let layout = Layout::from_size_align(0x1000, 0x10).unwrap();
    let allocation = fsb.allocate(layout).unwrap().as_ptr() as *mut u8;
    assert!(fsb.lock().allocators.is_some());
    assert!((allocation as u64) > base);
    assert!((allocation as u64) < base + MIN_EXPANSION as u64);
  }

  #[test]
  fn test_fallback_dealloc() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    init_gcd(&GCD, 0x400000);

    let mut fsb = FixedSizeBlockAllocator::new(&GCD, 1 as _);

    let layout = Layout::from_size_align(0x8, 0x8).unwrap();
    let allocation = fsb.fallback_alloc(layout);

    fsb.fallback_dealloc(allocation, layout);
    unsafe {
      assert_eq!((*fsb.allocators.unwrap()).allocator.free(), MIN_EXPANSION - size_of::<AllocatorListNode>());
    }
  }

  #[test]
  fn test_dealloc() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    init_gcd(&GCD, 0x400000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let layout = Layout::from_size_align(0x8, 0x8).unwrap();
    let allocation = unsafe { fsb.alloc(layout) };

    unsafe { fsb.dealloc(allocation, layout) };
    let free_block_ptr =
      fsb.lock().list_heads[list_index(&layout).unwrap()].take().unwrap() as *mut BlockListNode as *mut u8;
    assert_eq!(free_block_ptr, allocation);

    let layout = Layout::from_size_align(0x20, 0x20).unwrap();
    let allocation = unsafe { fsb.alloc(layout) };

    unsafe { fsb.dealloc(allocation, layout) };
    let free_block_ptr =
      fsb.lock().list_heads[list_index(&layout).unwrap()].take().unwrap() as *mut BlockListNode as *mut u8;
    assert_eq!(free_block_ptr, allocation);
  }

  #[test]
  fn test_deallocate() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    init_gcd(&GCD, 0x400000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let layout = Layout::from_size_align(0x8, 0x8).unwrap();
    let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();
    let allocation_ptr = allocation.as_ptr() as *mut u8;

    unsafe { fsb.deallocate(allocation, layout) };
    let free_block_ptr =
      fsb.lock().list_heads[list_index(&layout).unwrap()].take().unwrap() as *mut BlockListNode as *mut u8;
    assert_eq!(free_block_ptr, allocation_ptr);

    let layout = Layout::from_size_align(0x20, 0x20).unwrap();
    let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();
    let allocation_ptr = allocation.as_ptr() as *mut u8;

    unsafe { fsb.deallocate(allocation, layout) };
    let free_block_ptr =
      fsb.lock().list_heads[list_index(&layout).unwrap()].take().unwrap() as *mut BlockListNode as *mut u8;
    assert_eq!(free_block_ptr, allocation_ptr);
  }

  #[test]
  fn test_contains() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    init_gcd(&GCD, 0x400000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let layout = Layout::from_size_align(0x8, 0x8).unwrap();
    let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();
    assert!(fsb.contains(allocation));
  }

  #[test]
  fn test_allocate_pages() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to back the test GCD.
    let address = init_gcd(&GCD, 0x1000000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let pages = 4;

    let allocation =
      fsb.allocate_pages(uefi_gcd_lib::gcd::AllocateType::BottomUp(None), pages).unwrap().as_non_null_ptr();

    assert!(allocation.as_ptr() as u64 >= address);
    assert!((allocation.as_ptr() as u64) < address + 0x1000000);

    unsafe {
      fsb.free_pages(allocation.as_ptr() as usize, pages).unwrap();
    };
  }

  #[test]
  fn test_allocate_at_address() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to back the test GCD.
    let address = init_gcd(&GCD, 0x1000000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let target_address = address + 0x400000 - 8 * (ALIGNMENT as u64);
    let pages = 4;

    let allocation = fsb
      .allocate_pages(uefi_gcd_lib::gcd::AllocateType::Address(target_address as usize), pages)
      .unwrap()
      .as_non_null_ptr();

    assert_eq!(allocation.as_ptr() as u64, target_address);

    unsafe {
      fsb.free_pages(allocation.as_ptr() as usize, pages).unwrap();
    };
  }

  #[test]
  fn test_allocate_below_address() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to be back the test GCD.
    let address = init_gcd(&GCD, 0x1000000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let target_address = address + 0x400000 - 8 * (ALIGNMENT as u64);
    let pages = 4;

    let allocation = fsb
      .allocate_pages(uefi_gcd_lib::gcd::AllocateType::BottomUp(Some(target_address as usize)), pages)
      .unwrap()
      .as_non_null_ptr();
    assert!((allocation.as_ptr() as u64) < target_address);

    unsafe {
      fsb.free_pages(allocation.as_ptr() as usize, pages).unwrap();
    };
  }

  #[test]
  fn test_allocate_above_address() {
    // Create a static GCD
    static GCD: SpinLockedGcd = SpinLockedGcd::new();
    GCD.init(48, 16);

    // Allocate some space on the heap with the global allocator (std) to back the test GCD.
    let address = init_gcd(&GCD, 0x1000000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD, 1 as _);

    let target_address = address + 0x400000 - 8 * (ALIGNMENT as u64);
    let pages = 4;

    let allocation = fsb
      .allocate_pages(uefi_gcd_lib::gcd::AllocateType::TopDown(Some(target_address as usize)), pages)
      .unwrap()
      .as_non_null_ptr();
    assert!((allocation.as_ptr() as u64) > target_address);

    unsafe {
      fsb.free_pages(allocation.as_ptr() as usize, pages).unwrap();
    };
  }
}
