//! # Physical Memory
//!
//! The `physical_memory` module contains abstractions for interacting with and
//! allocating physical memory.
use crate::utility::Locked;

use self::dynamic_frame_allocator::DynamicFrameAllocator;

pub mod dynamic_frame_allocator;
pub mod frame;
pub mod x86_64;

pub static FRAME_ALLOCATOR: Locked<DynamicFrameAllocator> = Locked::new(DynamicFrameAllocator::empty());
