use crate::{physical_memory, utility::Locked};
use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use fixed_size_block::FixedSizeBlockAllocator;

pub mod bump;
pub mod fixed_size_block;
pub mod linked_list;

#[global_allocator]
static ALLOCATOR: Locked<FixedSizeBlockAllocator> = Locked::new(FixedSizeBlockAllocator::new());

#[derive(Debug)]
pub enum HeapError {
    FrameAllocationFailed,
}

/// Initialize the Heap
///
/// Page Protection will be disabled during initialization
pub fn init_heap(size: u64) -> Result<(), HeapError> {
    let range = physical_memory::FRAME_ALLOCATOR
        .lock()
        .allocate_frame_range_from_size(size)
        .map_err(|_| HeapError::FrameAllocationFailed)?;

    unsafe {
        ALLOCATOR.lock().init(range.start_addr().as_u64() as usize, size as usize);
    }

    Ok(())
}

pub struct Dummy;

unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        panic!("dealloc should be never called")
    }
}

/// Align the given address `addr` upwards to alignment `align`.
///
/// Requires that `align` is a power of two.
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
