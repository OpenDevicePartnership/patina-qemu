use crate::{physical_memory, utility::Locked};
use fixed_size_block::FixedSizeBlockAllocator;

pub mod fixed_size_block;

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
