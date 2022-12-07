use crate::utility::Locked;
use fixed_size_block::FixedSizeBlockAllocator;

pub mod fixed_size_block;

#[global_allocator]
pub static ALLOCATOR: Locked<FixedSizeBlockAllocator> = Locked::new(FixedSizeBlockAllocator::new());
