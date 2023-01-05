#![feature(allocator_api)]
#![feature(slice_ptr_get)]
use dynamic_frame_allocator_lib::SpinLockedDynamicFrameAllocator;
use uefi_rust_allocator_lib::fixed_size_block_allocator::SpinLockedFixedSizeBlockAllocator;

use core::alloc::{Allocator, Layout};
use std::alloc::{GlobalAlloc, System};

fn init_frame_allocator(frame_allocator: &SpinLockedDynamicFrameAllocator, size: usize) -> u64 {
    let layout = Layout::from_size_align(size, 0x1000).unwrap();
    let base = unsafe { System.alloc(layout) as u64 };
    unsafe {
        frame_allocator.lock().add_physical_region(base, size as u64).unwrap();
    }
    base
}

#[test]
fn allocate_deallocate_test() {
    // Create a static frame allocator for testing expand.
    static FRAME_ALLOCATOR: SpinLockedDynamicFrameAllocator = SpinLockedDynamicFrameAllocator::new();

    // Allocate some space on the heap with the global allocator (std) to be used by expand().
    init_frame_allocator(&FRAME_ALLOCATOR, 0x400000);

    let fsb = SpinLockedFixedSizeBlockAllocator::new(&FRAME_ALLOCATOR);

    let layout = Layout::from_size_align(0x8, 0x8).unwrap();
    let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();

    unsafe { fsb.deallocate(allocation, layout) };

    let layout = Layout::from_size_align(0x20, 0x20).unwrap();
    let allocation = fsb.allocate(layout).unwrap().as_non_null_ptr();

    unsafe { fsb.deallocate(allocation, layout) };
}
