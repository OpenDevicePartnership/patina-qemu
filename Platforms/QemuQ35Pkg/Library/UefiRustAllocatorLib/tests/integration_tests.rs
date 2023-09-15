#![feature(allocator_api)]
#![feature(slice_ptr_get)]
use r_pi::dxe_services::GcdMemoryType;
use uefi_gcd_lib::gcd::SpinLockedGcd;
use uefi_rust_allocator_lib::fixed_size_block_allocator::SpinLockedFixedSizeBlockAllocator;

use core::alloc::{Allocator, Layout};
use std::alloc::{GlobalAlloc, System};

fn init_gcd(gcd: &SpinLockedGcd, size: usize) -> u64 {
  let layout = Layout::from_size_align(size, 0x1000).unwrap();
  let base = unsafe { System.alloc(layout) as u64 };
  unsafe {
    gcd.add_memory_space(GcdMemoryType::SystemMemory, base as usize, size, 0).unwrap();
  }
  base
}

#[test]
fn allocate_deallocate_test() {
  // Create a static GCD for test.
  static GCD: SpinLockedGcd = SpinLockedGcd::new();
  GCD.init(48, 16);

  // Allocate some space on the heap with the global allocator (std) to be used by expand().
  init_gcd(&GCD, 0x400000);

  let fsb = SpinLockedFixedSizeBlockAllocator::new(&GCD);

  let layout = Layout::from_size_align(0x8, 0x8).unwrap();
  let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();

  unsafe { fsb.deallocate(allocation, layout) };

  let layout = Layout::from_size_align(0x20, 0x20).unwrap();
  let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();

  unsafe { fsb.deallocate(allocation, layout) };
}
