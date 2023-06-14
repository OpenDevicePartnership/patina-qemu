//! # Dynamic Frame Allocator
//! Implements a generic (non-architecture-specific) bump allocator that
//! supports a fixed set of frame ranges (for initial configuration pre-heap)
//! as well as arbitrary additional frame ranges that can be added once the
//! heap is available.
#![no_std]

extern crate alloc;
use alloc::vec::Vec;

#[derive(Debug, Eq, PartialEq)]
pub enum DynamicFrameAllocatorError {
  /// No further frames available to allocate.
  OutOfFrames,
  /// Address or Address range is invalid.
  OutOfRange,
  /// Address or size is not page-aligned
  InvalidAlignment,
}

pub const FRAME_SIZE: u64 = 0x1000;
const FRAME_SHIFT: u32 = 12;
const FRAME_MASK: u64 = 0xFFF;

const MAX_FIXED_REGIONS: usize = 4;

fn align_check(val: u64) -> Result<(), DynamicFrameAllocatorError> {
  if (val & (FRAME_SIZE - 1)) != 0 {
    Err(DynamicFrameAllocatorError::InvalidAlignment)
  } else {
    Ok(())
  }
}

fn align_up(val: u64) -> Result<u64, DynamicFrameAllocatorError> {
  if val & FRAME_MASK == 0 {
    Ok(val)
  } else {
    (val | FRAME_MASK).checked_add(1).ok_or(DynamicFrameAllocatorError::OutOfRange)
  }
}

// Defines a region of frames and tracks allocation of those frames.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct FrameRegion {
  start_frame_address: u64,
  count: u64,
  next: u64,
}

impl FrameRegion {
  pub const fn empty() -> Self {
    FrameRegion { start_frame_address: 0, count: 0, next: 0 }
  }

  pub fn new_from_base_and_count(base: u64, count: u64) -> Result<Self, DynamicFrameAllocatorError> {
    align_check(base)?;
    base
      .checked_add(count.checked_shl(FRAME_SHIFT).ok_or(DynamicFrameAllocatorError::OutOfRange)?)
      .ok_or(DynamicFrameAllocatorError::OutOfRange)?;

    Ok(FrameRegion { start_frame_address: base, count: count, next: 0 })
  }

  pub fn new_from_base_and_size(base: u64, size: u64) -> Result<Self, DynamicFrameAllocatorError> {
    align_check(size)?;
    Self::new_from_base_and_count(base, size >> FRAME_SHIFT)
  }

  pub fn allocate_frames(&mut self, count: u64) -> Result<u64, DynamicFrameAllocatorError> {
    if count > (self.count - self.next) {
      return Err(DynamicFrameAllocatorError::OutOfFrames);
    }
    let alloc = self.start_frame_address + (self.next << FRAME_SHIFT);
    self.next += count;
    Ok(alloc)
  }

  pub fn free_frames(&self) -> u64 {
    return self.count - self.next;
  }
}
/// DynamicFrameAllocator tracks allocations in regions of physical memory
#[derive(Debug)]
pub struct DynamicFrameAllocator {
  fixed_regions: [FrameRegion; MAX_FIXED_REGIONS],
  dyn_regions: Option<Vec<FrameRegion>>,
  region_count: usize,
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

  /// Add a memory region to be managed by this allocator.
  /// SAFETY: This function is unsafe because caller must guarantee that the provided
  /// base and size represent usable physical memory frames.
  pub unsafe fn add_physical_region(&mut self, base: u64, size: u64) -> Result<(), DynamicFrameAllocatorError> {
    let fr = FrameRegion::new_from_base_and_size(base, size)?;
    if self.region_count < MAX_FIXED_REGIONS {
      self.fixed_regions[self.region_count] = fr;
    } else {
      self.dyn_regions.get_or_insert(Vec::new()).push(fr);
    }
    self.region_count += 1;
    Ok(())
  }

  /// Returns the base address for a range containing the requested count of frames
  /// from the first physical region which has space, and marks that range as
  /// used. Returns the address of the allocated frames.
  pub fn allocate_frames_from_count(&mut self, count: u64) -> Result<u64, DynamicFrameAllocatorError> {
    let dyn_regions_iter = match &mut self.dyn_regions {
      Some(regions) => regions.iter_mut(),
      None => [FrameRegion::empty(); 0].iter_mut(),
    };

    let all_regions = self.fixed_regions.iter_mut().chain(dyn_regions_iter);

    for region in all_regions {
      if region.free_frames() >= count {
        return region.allocate_frames(count);
      }
    }
    Err(DynamicFrameAllocatorError::OutOfFrames)
  }

  /// Returns the base address for a range containing the requested memory size
  /// from the first physical region which has space, and marks that range as
  /// used. Size will be aligned up to the next page boundary if not already aligned.
  /// Returns the address and (possibly) adjusted size.
  pub fn allocate_frames_from_size(&mut self, size: u64) -> Result<(u64, u64), DynamicFrameAllocatorError> {
    let size = align_up(size)?;
    let addr = self.allocate_frames_from_count(size >> FRAME_SHIFT)?;
    Ok((addr, size))
  }
}

/// SpinLockedDynamicFrameAllocator tracks allocations in regions of physical memory and provides
/// synchronization via means of a spin mutex.
pub struct SpinLockedDynamicFrameAllocator {
  inner: spin::Mutex<DynamicFrameAllocator>,
}

impl SpinLockedDynamicFrameAllocator {
  /// Creates an new SpinLockedDynamicFrameAllocator
  pub const fn new() -> Self {
    SpinLockedDynamicFrameAllocator { inner: spin::Mutex::new(DynamicFrameAllocator::empty()) }
  }

  /// Locks the allocator and returns a MutexGuard<DynamicFrameAllocator> instance the caller can interact with.
  /// The allocator is unlocked when this instance is dropped.
  pub fn lock(&self) -> spin::MutexGuard<DynamicFrameAllocator> {
    self.inner.lock()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn construct_empty_frame_region() {
    let fr = FrameRegion::empty();
    assert_eq!(fr.start_frame_address, 0);
    assert_eq!(fr.count, 0);
    assert_eq!(fr.next, 0);
  }

  #[test]
  fn construct_frame_region_from_base_and_count() {
    let fr = FrameRegion::new_from_base_and_count(0x1000, 2).unwrap();
    assert_eq!(fr.start_frame_address, 0x1000);
    assert_eq!(fr.count, 2);
    assert_eq!(fr.next, 0);

    let fr = FrameRegion::new_from_base_and_count(0x1234, 0x1000);
    assert_eq!(fr, Err(DynamicFrameAllocatorError::InvalidAlignment));

    let fr = FrameRegion::new_from_base_and_count(0x1000, u64::MAX);
    assert_eq!(fr, Err(DynamicFrameAllocatorError::OutOfRange));

    let fr = FrameRegion::new_from_base_and_count(0x1000, u64::MAX >> FRAME_SHIFT);
    assert_eq!(fr, Err(DynamicFrameAllocatorError::OutOfRange));
  }

  #[test]
  fn construct_frame_region_from_base_and_size() {
    let fr = FrameRegion::new_from_base_and_size(0x1000, 0x2000).unwrap();
    assert_eq!(fr.start_frame_address, 0x1000);
    assert_eq!(fr.count, 2);
    assert_eq!(fr.next, 0);

    let fr = FrameRegion::new_from_base_and_size(0x1000, 0x1234);
    assert_eq!(fr, Err(DynamicFrameAllocatorError::InvalidAlignment));

    let fr = FrameRegion::new_from_base_and_size(0x1234, 0x1000);
    assert_eq!(fr, Err(DynamicFrameAllocatorError::InvalidAlignment));

    let fr = FrameRegion::new_from_base_and_size(u64::MAX - 0xFFF, 0x2000);
    assert_eq!(fr, Err(DynamicFrameAllocatorError::OutOfRange));
  }

  #[test]
  fn allocate_frames_by_count() {
    let mut fr = FrameRegion::new_from_base_and_count(0x1000, 10).unwrap();
    let range = fr.allocate_frames(2).unwrap();
    assert_eq!(range, 0x1000);
    assert_eq!(fr.next, 2);

    let mut fr = FrameRegion::new_from_base_and_count(0x1000, 10).unwrap();
    let range = fr.allocate_frames(20);
    assert_eq!(range, Err(DynamicFrameAllocatorError::OutOfFrames));
  }

  #[test]
  fn free_frames() {
    let mut fr = FrameRegion::new_from_base_and_count(0x1000, 10).unwrap();
    fr.allocate_frames(2).unwrap();

    let free_frames = fr.free_frames();
    assert_eq!(free_frames, 8);
  }
  #[test]
  fn construct_empty_dynamic_frame_allocator() {
    let fa = DynamicFrameAllocator::empty();
    assert_eq!(fa.fixed_regions, [FrameRegion::empty(); MAX_FIXED_REGIONS]);
    assert_eq!(fa.dyn_regions, None);
    assert_eq!(fa.region_count, 0);
  }

  #[test]
  fn add_region_to_dynamic_frame_allocator() {
    let mut fa = DynamicFrameAllocator::empty();
    for i in 1..=MAX_FIXED_REGIONS {
      let test_val = i as u64;
      unsafe {
        fa.add_physical_region(test_val * FRAME_SIZE, test_val * FRAME_SIZE).unwrap();
      }
      for region_idx in 0..i {
        let test_val = (region_idx as u64) + 1;
        assert_eq!(
          fa.fixed_regions[region_idx],
          FrameRegion { start_frame_address: test_val * FRAME_SIZE, count: test_val, next: 0 }
        );
      }
      for region in &fa.fixed_regions[i..] {
        assert_eq!(region, &FrameRegion::empty());
      }
      assert_eq!(fa.dyn_regions, None);
      assert_eq!(fa.region_count, i);
    }

    let region_count = fa.region_count;
    unsafe {
      fa.add_physical_region(5 * FRAME_SIZE, 5 * FRAME_SIZE).unwrap();
    }
    assert_eq!(fa.region_count, region_count + 1);
    let dyn_regions = fa.dyn_regions.unwrap();
    let region = dyn_regions.get(0).unwrap();
    assert_eq!(region, &FrameRegion { start_frame_address: 5 * FRAME_SIZE, count: 5, next: 0 });
  }

  #[test]
  fn dynamic_allocate_frames_by_count() {
    let mut fa = DynamicFrameAllocator::empty();
    for i in 1..=10 {
      let test_val = i as u64;
      unsafe {
        fa.add_physical_region(test_val * FRAME_SIZE, test_val * FRAME_SIZE).unwrap();
      }
    }

    //allocate 3 frames - should come from the 3rd fixed region added above.
    let addr = fa.allocate_frames_from_count(3).unwrap();
    assert_eq!(fa.fixed_regions[2].start_frame_address, addr);
    assert_eq!(fa.fixed_regions[2].next, 3);
    assert_eq!(fa.fixed_regions[2].free_frames(), 0);

    //allocate 3 more frames - should come from the 4th fixed region added above.
    let addr = fa.allocate_frames_from_count(3).unwrap();
    assert_eq!(fa.fixed_regions[3].start_frame_address, addr);
    assert_eq!(fa.fixed_regions[3].next, 3);
    assert_eq!(fa.fixed_regions[3].free_frames(), 1);

    //allocate 1 frame - should come from the 1st fixed region added above.
    let addr = fa.allocate_frames_from_count(1).unwrap();
    assert_eq!(fa.fixed_regions[0].start_frame_address, addr);
    assert_eq!(fa.fixed_regions[0].next, 1);
    assert_eq!(fa.fixed_regions[0].free_frames(), 0);

    //allocate 2 frames - should come from the 2nd fixed region added above.
    let addr = fa.allocate_frames_from_count(2).unwrap();
    assert_eq!(fa.fixed_regions[1].start_frame_address, addr);
    assert_eq!(fa.fixed_regions[1].next, 2);
    assert_eq!(fa.fixed_regions[1].free_frames(), 0);

    //allocate 1 more frame - since first 3 regions are full, should come from the 4th fixed region.
    let addr = fa.allocate_frames_from_count(1).unwrap();
    assert_eq!(fa.fixed_regions[3].start_frame_address + 3 * FRAME_SIZE, addr);
    assert_eq!(fa.fixed_regions[3].next, 4);
    assert_eq!(fa.fixed_regions[3].free_frames(), 0);

    //allocate 7 frames - should from the (7 - MAX_FIXED_REGIONS)th dynamic region allocated above.
    let addr = fa.allocate_frames_from_count(7).unwrap();
    let dyn_regions = fa.dyn_regions.unwrap();
    assert_eq!(dyn_regions[7 - MAX_FIXED_REGIONS - 1].start_frame_address, addr);
    assert_eq!(dyn_regions[7 - MAX_FIXED_REGIONS - 1].next, 7);
    assert_eq!(dyn_regions[7 - MAX_FIXED_REGIONS - 1].free_frames(), 0);
  }

  #[test]
  fn dynamic_allocate_frames_by_size() {
    let mut fa = DynamicFrameAllocator::empty();
    for i in 1..=10 {
      let test_val = i as u64;
      unsafe {
        fa.add_physical_region(test_val * FRAME_SIZE, test_val * FRAME_SIZE).unwrap();
      }
    }
    //allocate 0x2123 bytes - should align up to 0x3000 and come from the
    //3rd fixed region added above.
    let (addr, size) = fa.allocate_frames_from_size(0x2123).unwrap();
    assert_eq!(size, 0x3000);
    assert_eq!(fa.fixed_regions[2].start_frame_address, addr);
    assert_eq!(fa.fixed_regions[2].next, size >> FRAME_SHIFT);
    assert_eq!(fa.fixed_regions[2].free_frames(), 0);

    //allocate 0x3000 more bytes - should come from the 4th fixed region added above.
    let (addr, size) = fa.allocate_frames_from_size(0x3000).unwrap();
    assert_eq!(fa.fixed_regions[3].start_frame_address, addr);
    assert_eq!(size, 0x3000);
    assert_eq!(fa.fixed_regions[3].next, 3);
    assert_eq!(fa.fixed_regions[3].free_frames(), 1);

    //allocate 0x1000 bytes - should come from the 1st fixed region added above.
    let (addr, size) = fa.allocate_frames_from_size(0x1000).unwrap();
    assert_eq!(fa.fixed_regions[0].start_frame_address, addr);
    assert_eq!(size, 0x1000);
    assert_eq!(fa.fixed_regions[0].next, 1);
    assert_eq!(fa.fixed_regions[0].free_frames(), 0);

    //allocate 0x2000 bytes - should come from the 2nd fixed region added above.
    let (addr, size) = fa.allocate_frames_from_size(0x2000).unwrap();
    assert_eq!(fa.fixed_regions[1].start_frame_address, addr);
    assert_eq!(size, 0x2000);
    assert_eq!(fa.fixed_regions[1].next, 2);
    assert_eq!(fa.fixed_regions[1].free_frames(), 0);

    //allocate 0x1000 more bytes - since first 3 regions are full, should come from the 4th fixed region.
    let (addr, size) = fa.allocate_frames_from_size(0x1000).unwrap();
    assert_eq!(fa.fixed_regions[3].start_frame_address + 3 * FRAME_SIZE, addr);
    assert_eq!(size, 0x1000);
    assert_eq!(fa.fixed_regions[3].next, 4);
    assert_eq!(fa.fixed_regions[3].free_frames(), 0);

    //allocate 0x7000 bytes - should from the (7 - MAX_FIXED_REGIONS)th dynamic region allocated above.
    let (addr, size) = fa.allocate_frames_from_size(0x7000).unwrap();
    let dyn_regions = fa.dyn_regions.unwrap();
    assert_eq!(dyn_regions[7 - MAX_FIXED_REGIONS - 1].start_frame_address, addr);
    assert_eq!(size, 0x7000);
    assert_eq!(dyn_regions[7 - MAX_FIXED_REGIONS - 1].next, 7);
    assert_eq!(dyn_regions[7 - MAX_FIXED_REGIONS - 1].free_frames(), 0);
  }

  #[test]
  fn construct_spin_locked_dynamic_allocator() {
    let fa = SpinLockedDynamicFrameAllocator::new();
    for i in 1..=10 {
      let test_val = i as u64;
      unsafe {
        fa.lock().add_physical_region(test_val * FRAME_SIZE, test_val * FRAME_SIZE).unwrap();
      }
    }
    //allocate 3 frames - should come from the 3rd fixed region added above.
    let addr = fa.lock().allocate_frames_from_count(3).unwrap();
    assert_eq!(fa.lock().fixed_regions[2].start_frame_address, addr);
    assert_eq!(fa.lock().fixed_regions[2].next, 3);
    assert_eq!(fa.lock().fixed_regions[2].free_frames(), 0);
  }
}
