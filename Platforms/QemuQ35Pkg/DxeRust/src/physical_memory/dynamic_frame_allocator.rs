//! # Dynamic Frame Allocator
//! Implements a generic (non-architecture-specific) frame allocator that
//! supports a fixed set of FrameRanges (for initial configuration pre-heap)
//! as well as arbitrary additional FrameRanges that can be added later.

use super::frame::{self, frame_count_to_size, size_to_pages, FrameRange, PhyAddr};
use alloc::vec::Vec;

#[derive(Debug)]
pub enum DynamicFrameAllocatorError {
    /// No further frames available to allocate.
    OutOfFrames,
    /// Address or Address range is invalid.
    OutOfRange,
}

const MAX_FIXED_REGIONS: usize = 4;
/// Defines a region of frames and tracks allocation of those frames.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FrameRegion {
    frame_range: FrameRange, //Frame range for this region.
    next: u64,               //the next free frame in this region.
}
impl FrameRegion {
    /// Creates an empty FrameRegion
    pub const fn empty() -> Self {
        FrameRegion { frame_range: FrameRange::empty(), next: 0 }
    }
    pub fn new_from_base_and_size(base: u64, size: u64) -> Result<Self, DynamicFrameAllocatorError> {
        if let Some(_limit) = base.checked_add(size - 1) {
            return Ok(FrameRegion { frame_range: FrameRange::from_base_and_size(PhyAddr::new(base), size), next: 0 });
        }
        return Err(DynamicFrameAllocatorError::OutOfRange);
    }

    /// Returns the count of free frames in this region
    pub fn free_frames(&self) -> u64 {
        return self.frame_range.frame_count() - self.next;
    }

    /// returns the size (in bytes) of the free space in this region
    pub fn free_size(&self) -> u64 {
        return frame_count_to_size(self.free_frames());
    }

    /// returns the start address of this frame region
    pub fn start_addr(&self) -> PhyAddr {
        self.frame_range.start_addr()
    }

    /// allocates `count` frames from this region and returns the allocated range or an error.
    pub fn allocate_frames(&mut self, count: u64) -> Result<FrameRange, DynamicFrameAllocatorError> {
        if count > self.free_frames() {
            return Err(DynamicFrameAllocatorError::OutOfFrames);
        }

        let alloc_base = self.start_addr() + frame_count_to_size(self.next);

        let new_range = FrameRange::from_base_and_count(alloc_base, count);
        self.next += count;
        Ok(new_range)
    }

    /// allocates `size` bytes from this region and returns the allocated range or an error.
    /// `size` is rounded up to the next page boundary.
    pub fn allocate_size(&mut self, size: u64) -> Result<FrameRange, DynamicFrameAllocatorError> {
        return self.allocate_frames(size_to_pages(size));
    }
}

/// DynamicFrameAllocator tracks allocations in regions of physical memory
#[derive(Debug)]
pub struct DynamicFrameAllocator {
    fixed_regions: [FrameRegion; MAX_FIXED_REGIONS], //used before heap.
    dyn_regions: Option<Vec<FrameRegion>>,           //used after heap.
    region_count: usize,                             //count of valid regions
}

impl DynamicFrameAllocator {
    /// Initialize an empty DynamicFrameAllocator
    pub const fn empty() -> Self {
        DynamicFrameAllocator {
            fixed_regions: [FrameRegion::empty(); MAX_FIXED_REGIONS],
            dyn_regions: None,
            region_count: 0,
        }
    }

    /// This function is unsafe because caller must guarantee that the provided
    /// FrameRegion represents usable physical memory frames.
    pub unsafe fn add_physical_region(&mut self, base: u64, size: u64) -> Result<(), DynamicFrameAllocatorError> {
        match FrameRegion::new_from_base_and_size(base, size) {
            Ok(region) => {
                // the first MAX_FIXED_REGION regions can be added without requiring dynamic
                // allocation - these can be used e.g. to initialize a global heap.
                // after MAX_FIXED_REGION regions have been added, new regions are added
                // to a Vector which can grow to arbitrary size.
                if self.region_count < MAX_FIXED_REGIONS {
                    self.fixed_regions[self.region_count] = region.clone();
                } else {
                    self.dyn_regions.get_or_insert(Vec::new()).push(region.clone());
                }
                self.region_count += 1;
                Ok(())
            }
            Err(_) => Err(DynamicFrameAllocatorError::OutOfRange),
        }
    }

    /// Returns a contiguous FrameRange containing the requested count of frames
    /// from the first physical region which has space, and marks that range as
    /// used.
    pub fn allocate_frame_range_from_count(&mut self, count: u64) -> Result<FrameRange, DynamicFrameAllocatorError> {
        let mut dyn_regions_iter = [FrameRegion::empty(); 0].iter_mut();
        if let Some(regions) = &mut self.dyn_regions {
            dyn_regions_iter = regions.iter_mut();
        }
        let all_regions = self.fixed_regions.iter_mut().chain(dyn_regions_iter);

        for region in all_regions {
            if region.free_frames() >= count {
                return region.allocate_frames(count);
            }
        }
        Err(DynamicFrameAllocatorError::OutOfFrames)
    }

    pub fn allocate_frame_range_from_size(&mut self, size: u64) -> Result<FrameRange, DynamicFrameAllocatorError> {
        self.allocate_frame_range_from_count(frame::size_to_pages(size))
    }
}
