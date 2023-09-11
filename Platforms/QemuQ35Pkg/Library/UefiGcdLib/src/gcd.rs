use core::{mem, ptr};

use alloc::slice;
use r_efi::efi::Handle;
use r_pi::dxe_services::{GcdMemoryType, MemorySpaceDescriptor};

use crate::{ensure, error};

use super::{
  memory_block::{self, Error as MemoryBlockError, MemoryBlock, MemoryBlockSplit, StateTransition},
  sorted_slice::{self, Error as SortedSliceError, SortedSlice, SortedSliceKey},
};

const MEMORY_BLOCK_SLICE_LEN: usize = 4096;
pub const MEMORY_BLOCK_SLICE_SIZE: usize = MEMORY_BLOCK_SLICE_LEN * mem::size_of::<MemoryBlock>();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
  InvalidParameter,
  OutOfResources,
  Unsupported,
  AccessDenied,
  NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InternalError {
  MemoryBlockErr(MemoryBlockError),
  SortedSliceErr(SortedSliceError),
}

#[derive(Debug, Clone, Copy)]
pub enum AllocateType {
  // Allocate from the lowest address to the highest address or until the specify address is reached (max address).
  BottomUp(Option<usize>),
  // Allocate from the highest address to the lowest address or until the specify address is reached (min address).
  TopDown(Option<usize>),
  // Allocate at this address.
  Address(usize),
}

#[derive(Debug)]
///The Global Coherency Domain (GCD) Services are used to manage the memory and I/O resources visible to the boot processor.
pub struct GCD {
  maximum_address: usize,
  memory_blocks: Option<SortedSlice<'static, MemoryBlock>>,
}

impl GCD {
  /// Create an instance of the Global Coherency Domain (GCD).
  pub fn new(processor_address_bits: u32) -> Self {
    Self { memory_blocks: None, maximum_address: 1 << processor_address_bits }
  }

  unsafe fn init_memory_blocks(
    &mut self,
    memory_type: GcdMemoryType,
    base_address: usize,
    len: usize,
    capabilities: u64,
  ) -> Result<usize, Error> {
    ensure!(memory_type == GcdMemoryType::SystemMemory && len >= MEMORY_BLOCK_SLICE_SIZE, Error::OutOfResources);

    let unallocated_memory_space = MemoryBlock::Unallocated(MemorySpaceDescriptor {
      memory_type: GcdMemoryType::NonExistent,
      base_address: 0,
      length: self.maximum_address as u64,
      ..Default::default()
    });

    let mut memory_blocks =
      SortedSlice::new(slice::from_raw_parts_mut::<'static>(base_address as *mut u8, MEMORY_BLOCK_SLICE_SIZE));
    memory_blocks.add(unallocated_memory_space).map_err(|_| Error::OutOfResources)?;
    self.memory_blocks.replace(memory_blocks);

    self.add_memory_space(memory_type, base_address, len, capabilities)?;

    self.allocate_memory_space(
      AllocateType::Address(base_address),
      GcdMemoryType::SystemMemory,
      0,
      MEMORY_BLOCK_SLICE_SIZE,
      1 as _,
      None,
    )
  }

  /// This service adds reserved memory, system memory, or memory-mapped I/O resources to the global coherency domain of the processor.
  ///
  /// # Safety
  /// Since the first call with enough system memory will cause the creation of an array at `base_address` + [MEMORY_BLOCK_SLICE_SIZE].
  /// The memory from `base_address` to `base_address+len` must be inside the valid address range of the program and not in use.
  ///
  /// # Documentation
  /// UEFI Platform Initialization Specification, Release 1.8, Section II-7.2.4.1
  pub unsafe fn add_memory_space(
    &mut self,
    memory_type: GcdMemoryType,
    base_address: usize,
    len: usize,
    capabilities: u64,
  ) -> Result<usize, Error> {
    ensure!(len > 0, Error::InvalidParameter);
    ensure!(base_address + len <= self.maximum_address, Error::Unsupported);

    let Some(memory_blocks) = &mut self.memory_blocks else {
      return self.init_memory_blocks(memory_type, base_address, len, capabilities);
    };

    let idx = match memory_blocks.search_idx_with_key(&(base_address as u64)) {
      Ok(i) => i,
      Err(i) => i - 1,
    };

    ensure!(memory_blocks[idx].as_ref().memory_type == GcdMemoryType::NonExistent, Error::AccessDenied);

    match Self::split_state_transition_at_idx(
      memory_blocks,
      idx,
      base_address,
      len,
      StateTransition::Add(memory_type, capabilities),
    ) {
      Ok(idx) => Ok(idx),
      Err(InternalError::MemoryBlockErr(MemoryBlockError::BlockOutsideRange)) => error!(Error::AccessDenied),
      Err(InternalError::MemoryBlockErr(MemoryBlockError::InvalidStateTransition)) => error!(Error::InvalidParameter),
      Err(InternalError::SortedSliceErr(SortedSliceError::NotEnoughMemory)) => error!(Error::OutOfResources),
      Err(e) => panic!("{e:?}"),
    }
  }

  /// This service removes reserved memory, system memory, or memory-mapped I/O resources from the global coherency domain of the processor.
  ///
  /// # Documentation
  /// UEFI Platform Initialization Specification, Release 1.8, Section II-7.2.4.4
  pub fn remove_memory_space(&mut self, base_address: usize, len: usize) -> Result<(), Error> {
    ensure!(len > 0, Error::InvalidParameter);
    ensure!(base_address + len <= self.maximum_address, Error::Unsupported);

    let memory_blocks = self.memory_blocks.as_mut().ok_or(Error::NotFound)?;

    let idx = match memory_blocks.search_idx_with_key(&(base_address as u64)) {
      Ok(i) => i,
      Err(i) => i - 1,
    };

    match Self::split_state_transition_at_idx(memory_blocks, idx, base_address, len, StateTransition::Remove) {
      Ok(_) => Ok(()),
      Err(InternalError::MemoryBlockErr(MemoryBlockError::BlockOutsideRange)) => error!(Error::NotFound),
      Err(InternalError::MemoryBlockErr(MemoryBlockError::InvalidStateTransition)) => match memory_blocks[idx] {
        MemoryBlock::Unallocated(_) => error!(Error::NotFound),
        MemoryBlock::Allocated(_) => error!(Error::AccessDenied),
      },
      Err(InternalError::SortedSliceErr(SortedSliceError::NotEnoughMemory)) => error!(Error::OutOfResources),
      Err(e) => panic!("{e:?}"),
    }
  }

  /// This service allocates nonexistent memory, reserved memory, system memory, or memory-mapped I/O resources from the global coherency domain of the processor.
  ///
  /// # Documentation
  /// UEFI Platform Initialization Specification, Release 1.8, Section II-7.2.4.2
  pub fn allocate_memory_space(
    &mut self,
    allocate_type: AllocateType,
    memory_type: GcdMemoryType,
    alignment: u32,
    len: usize,
    image_handle: Handle,
    device_handle: Option<Handle>,
  ) -> Result<usize, Error> {
    ensure!(
      len > 0 && image_handle > ptr::null_mut() && memory_type != GcdMemoryType::Unaccepted,
      Error::InvalidParameter
    );

    match allocate_type {
      AllocateType::BottomUp(max_address) => self.allocate_bottom_up(
        memory_type,
        alignment,
        len,
        image_handle,
        device_handle,
        max_address.unwrap_or(usize::MAX),
      ),
      AllocateType::TopDown(min_address) => {
        self.allocate_top_down(memory_type, alignment, len, image_handle, device_handle, min_address.unwrap_or(0))
      }
      AllocateType::Address(address) => {
        ensure!(address + len <= self.maximum_address, Error::Unsupported);
        self.allocate_address(memory_type, alignment, len, image_handle, device_handle, address)
      }
    }
  }

  fn allocate_bottom_up(
    &mut self,
    memory_type: GcdMemoryType,
    alignment: u32,
    len: usize,
    image_handle: Handle,
    device_handle: Option<Handle>,
    max_address: usize,
  ) -> Result<usize, Error> {
    ensure!(len > 0, Error::InvalidParameter);

    let memory_blocks = self.memory_blocks.as_mut().ok_or(Error::NotFound)?;

    for i in 0..memory_blocks.len() {
      let mb = &memory_blocks[i];
      if mb.len() < len {
        continue;
      }
      let address = mb.start();
      let mut addr = address & (usize::MAX << alignment);
      if addr < address {
        addr += 1 << alignment;
      }
      ensure!(addr + len <= max_address, Error::NotFound);
      if mb.as_ref().memory_type != memory_type {
        continue;
      }

      match Self::split_state_transition_at_idx(
        memory_blocks,
        i,
        addr,
        len,
        StateTransition::Allocate(image_handle, device_handle),
      ) {
        Ok(_) => return Ok(addr),
        Err(InternalError::MemoryBlockErr(_)) => continue,
        Err(InternalError::SortedSliceErr(SortedSliceError::NotEnoughMemory)) => error!(Error::OutOfResources),
        Err(e) => panic!("{e:?}"),
      }
    }
    Err(Error::NotFound)
  }

  fn allocate_top_down(
    &mut self,
    memory_type: GcdMemoryType,
    alignment: u32,
    len: usize,
    image_handle: Handle,
    device_handle: Option<Handle>,
    min_address: usize,
  ) -> Result<usize, Error> {
    ensure!(len > 0, Error::InvalidParameter);

    let memory_blocks = self.memory_blocks.as_mut().ok_or(Error::NotFound)?;

    for i in (0..memory_blocks.len()).rev() {
      let mb = &memory_blocks[i];
      if mb.len() < len {
        continue;
      }
      let mut addr = mb.end() - len;
      if addr < mb.start() {
        continue;
      }
      addr = addr & (usize::MAX << alignment);
      ensure!(addr >= min_address, Error::NotFound);

      if mb.as_ref().memory_type != memory_type {
        continue;
      }

      match Self::split_state_transition_at_idx(
        memory_blocks,
        i,
        addr,
        len,
        StateTransition::Allocate(image_handle, device_handle),
      ) {
        Ok(_) => return Ok(addr),
        Err(InternalError::MemoryBlockErr(_)) => continue,
        Err(InternalError::SortedSliceErr(SortedSliceError::NotEnoughMemory)) => error!(Error::OutOfResources),
        Err(e) => panic!("{e:?}"),
      }
    }
    Err(Error::NotFound)
  }

  fn allocate_address(
    &mut self,
    memory_type: GcdMemoryType,
    alignment: u32,
    len: usize,
    image_handle: Handle,
    device_handle: Option<Handle>,
    address: usize,
  ) -> Result<usize, Error> {
    ensure!(len > 0, Error::InvalidParameter);
    let memory_blocks = self.memory_blocks.as_mut().ok_or(Error::NotFound)?;

    let idx = match memory_blocks.search_idx_with_key(&(address as u64)) {
      Ok(i) => i,
      Err(i) => i - 1,
    };

    ensure!(
      memory_blocks[idx].as_ref().memory_type == memory_type && address == address & (usize::MAX << alignment),
      Error::NotFound
    );

    match Self::split_state_transition_at_idx(
      memory_blocks,
      idx,
      address,
      len,
      StateTransition::Allocate(image_handle, device_handle),
    ) {
      Ok(_) => Ok(address),
      Err(InternalError::MemoryBlockErr(_)) => error!(Error::NotFound),
      Err(InternalError::SortedSliceErr(SortedSliceError::NotEnoughMemory)) => error!(Error::OutOfResources),
      Err(e) => panic!("{e:?}"),
    }
  }

  /// This service frees nonexistent memory, reserved memory, system memory, or memory-mapped I/O resources from the global coherency domain of the processor.
  ///
  /// # Documentation
  /// UEFI Platform Initialization Specification, Release 1.8, Section II-7.2.4.3
  pub fn free_memory_space(&mut self, base_address: usize, len: usize) -> Result<(), Error> {
    ensure!(len > 0, Error::InvalidParameter);
    ensure!(base_address + len <= self.maximum_address, Error::Unsupported);

    let memory_blocks = self.memory_blocks.as_mut().ok_or(Error::NotFound)?;

    let idx = match memory_blocks.search_idx_with_key(&(base_address as u64)) {
      Ok(i) => i,
      Err(i) => i - 1,
    };

    match Self::split_state_transition_at_idx(memory_blocks, idx, base_address, len, StateTransition::Free) {
      Ok(_) => Ok(()),
      Err(InternalError::MemoryBlockErr(_)) => error!(Error::NotFound),
      Err(InternalError::SortedSliceErr(SortedSliceError::NotEnoughMemory)) => error!(Error::OutOfResources),
      Err(e) => panic!("{e:?}"),
    }
  }

  fn split_state_transition_at_idx(
    memory_blocks: &mut SortedSlice<MemoryBlock>,
    idx: usize,
    base_address: usize,
    len: usize,
    transition: StateTransition,
  ) -> Result<usize, InternalError> {
    let mb_before_split = memory_blocks[idx];
    let new_idx = match memory_blocks[idx].split_state_transition(base_address, len, transition)? {
      MemoryBlockSplit::Same(_) => Ok(idx),
      MemoryBlockSplit::After(_, next) => memory_blocks.add(next),
      MemoryBlockSplit::Before(_, next) => memory_blocks.add(next).map(|_| idx),
      MemoryBlockSplit::Middle(_, next, next2) => memory_blocks.add_contiguous_slice(&[next, next2]),
    };

    let mut idx = match new_idx {
      Ok(idx) => idx,
      Err(e) => {
        memory_blocks[idx] = mb_before_split;
        error!(e)
      }
    };

    if let Ok([removed, next]) = memory_blocks.get_many_mut([idx, idx + 1]) {
      if removed.merge(next) {
        memory_blocks.remove_at_idx(idx + 1);
      }
    }

    if idx > 0 {
      if let Ok([prev, removed]) = memory_blocks.get_many_mut([idx - 1, idx]) {
        if prev.merge(removed) {
          memory_blocks.remove_at_idx(idx);
          idx -= 1;
        }
      }
    }

    Ok(idx)
  }

  pub fn memory_block_count(&self) -> usize {
    self.memory_blocks.as_ref().map(|mbs| mbs.len()).unwrap_or(0)
  }
}

impl SortedSliceKey for MemoryBlock {
  type Key = u64;
  fn ordering_key(&self) -> &Self::Key {
    &self.as_ref().base_address
  }
}

impl From<sorted_slice::Error> for InternalError {
  fn from(value: sorted_slice::Error) -> Self {
    InternalError::SortedSliceErr(value)
  }
}

impl From<memory_block::Error> for InternalError {
  fn from(value: memory_block::Error) -> Self {
    InternalError::MemoryBlockErr(value)
  }
}

#[cfg(test)]
mod tests {
  extern crate std;
  use super::*;
  use alloc::{vec, vec::Vec};

  #[test]
  fn test_gcd_initialization() {
    let gdc = GCD::new(48);
    assert_eq!(2_usize.pow(48), gdc.maximum_address);
    assert!(gdc.memory_blocks.is_none());
    assert_eq!(0, gdc.memory_block_count())
  }

  #[test]
  fn test_add_memory_space_before_memory_blocks_instantiated() {
    let mem = unsafe { get_memory(MEMORY_BLOCK_SLICE_SIZE) };
    let address = mem.as_ptr() as usize;
    let mut gcd = GCD::new(48);

    assert_eq!(
      Err(Error::OutOfResources),
      unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, address, MEMORY_BLOCK_SLICE_SIZE, 0) },
      "First add memory space should be a system memory."
    );
    assert_eq!(0, gcd.memory_block_count());

    assert_eq!(
      Err(Error::OutOfResources),
      unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, address, MEMORY_BLOCK_SLICE_SIZE - 1, 0) },
      "First add memory space with system memory should contain enough space to contain the block list."
    );
    assert_eq!(0, gcd.memory_block_count());
  }

  #[test]
  fn test_add_memory_space_with_all_memory_type() {
    let (mut gcd, _) = create_gcd();

    assert_eq!(Ok(0), unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, 0, 1, 0) });
    assert_eq!(Ok(1), unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 1, 1, 0) });
    assert_eq!(Ok(2), unsafe { gcd.add_memory_space(GcdMemoryType::Persistent, 2, 1, 0) });
    assert_eq!(Ok(3), unsafe { gcd.add_memory_space(GcdMemoryType::MoreReliable, 3, 1, 0) });
    assert_eq!(Ok(4), unsafe { gcd.add_memory_space(GcdMemoryType::Unaccepted, 4, 1, 0) });
    assert_eq!(Ok(5), unsafe { gcd.add_memory_space(GcdMemoryType::MemoryMappedIo, 5, 1, 0) });

    let snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::InvalidParameter),
      unsafe { gcd.add_memory_space(GcdMemoryType::NonExistent, 10, 1, 0) },
      "Can't manually add NonExistent memory space manually."
    );

    assert!(is_gcd_memory_slice_valid(&gcd));
    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_add_memory_space_with_0_len_block() {
    let (mut gcd, _) = create_gcd();
    let snapshot = copy_memory_block(&gcd);
    assert_eq!(Err(Error::InvalidParameter), unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0, 0, 0) });
    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_add_memory_space_when_memory_block_full() {
    let (mut gcd, address) = create_gcd();
    let addr = address + MEMORY_BLOCK_SLICE_SIZE;

    let mut n = 0;
    while gcd.memory_block_count() < MEMORY_BLOCK_SLICE_LEN {
      assert!(unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, addr + n, 1, n as u64) }.is_ok());
      n += 1;
    }

    assert!(is_gcd_memory_slice_valid(&gcd));
    let memory_blocks_snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::OutOfResources),
      unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, 30_000, 1000, 0) },
      "Should return out of memory if there is no space in memory blocks."
    );

    assert_eq!(memory_blocks_snapshot, copy_memory_block(&gcd),);
  }

  #[test]
  fn test_add_memory_space_outside_processor_range() {
    let (mut gcd, _) = create_gcd();

    let snapshot = copy_memory_block(&gcd);

    assert_eq!(Err(Error::Unsupported), unsafe {
      gcd.add_memory_space(GcdMemoryType::SystemMemory, gcd.maximum_address + 1, 1, 0)
    });
    assert_eq!(Err(Error::Unsupported), unsafe {
      gcd.add_memory_space(GcdMemoryType::SystemMemory, gcd.maximum_address, 1, 0)
    });
    assert_eq!(Err(Error::Unsupported), unsafe {
      gcd.add_memory_space(GcdMemoryType::SystemMemory, gcd.maximum_address - 1, 2, 0)
    });

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_add_memory_space_in_range_already_added() {
    let (mut gcd, _) = create_gcd();
    // Add block to test the boundary on.
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 1000, 10, 0) }.unwrap();

    let snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::AccessDenied),
      unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, 1002, 5, 0) },
      "Can't add inside a range previously added."
    );
    assert_eq!(
      Err(Error::AccessDenied),
      unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, 998, 5, 0) },
      "Can't add partially inside a range previously added (Start)."
    );
    assert_eq!(
      Err(Error::AccessDenied),
      unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, 1009, 5, 0) },
      "Can't add partially inside a range previously added (End)."
    );

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_add_memory_space_in_range_already_allocated() {
    let (mut gcd, address) = create_gcd();
    // Add unallocated block after allocated one.
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, address - 100, 100, 0) }.unwrap();

    let snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::AccessDenied),
      unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, address, 5, 0) },
      "Can't add inside a range previously allocated."
    );
    assert_eq!(
      Err(Error::AccessDenied),
      unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, address - 100, 200, 0) },
      "Can't add partially inside a range previously allocated."
    );

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_add_memory_space_block_merging() {
    let (mut gcd, _) = create_gcd();

    assert_eq!(Ok(1), unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 1000, 10, 0) });
    let block_count = gcd.memory_block_count();

    // Test merging when added after
    match unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 1010, 10, 0) } {
      Ok(idx) => {
        let mb = gcd.memory_blocks.as_ref().unwrap()[idx];
        assert_eq!(1000, mb.as_ref().base_address);
        assert_eq!(20, mb.as_ref().length);
        assert_eq!(block_count, gcd.memory_block_count());
      }
      Err(e) => assert!(false, "{e:?}"),
    }

    // Test merging when added before
    match unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 990, 10, 0) } {
      Ok(idx) => {
        let mb = gcd.memory_blocks.as_ref().unwrap()[idx];
        assert_eq!(990, mb.as_ref().base_address);
        assert_eq!(30, mb.as_ref().length);
        assert_eq!(block_count, gcd.memory_block_count());
      }
      Err(e) => assert!(false, "{e:?}"),
    }

    assert!(
      unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, 1020, 10, 0) }.is_ok(),
      "A different memory type should note result in a merge."
    );
    assert_eq!(block_count + 1, gcd.memory_block_count());
    assert!(
      unsafe { gcd.add_memory_space(GcdMemoryType::Reserved, 1030, 10, 1) }.is_ok(),
      "A different capabilities should note result in a merge."
    );
    assert_eq!(block_count + 2, gcd.memory_block_count());

    assert!(is_gcd_memory_slice_valid(&gcd));
  }

  #[test]
  fn test_add_memory_space_state() {
    let (mut gcd, _) = create_gcd();
    match unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 100, 10, 123) } {
      Ok(idx) => {
        let mb = gcd.memory_blocks.as_ref().unwrap()[idx];
        match mb {
          MemoryBlock::Unallocated(md) => {
            assert_eq!(100, md.base_address);
            assert_eq!(10, md.length);
            assert_eq!(123, md.capabilities);
            assert_eq!(0, md.image_handle as usize);
            assert_eq!(0, md.device_handle as usize);
          }
          MemoryBlock::Allocated(_) => assert!(false, "Add should keep the block unallocated"),
        }
      }
      Err(e) => assert!(false, "{e:?}"),
    }
  }

  #[test]
  fn test_remove_memory_space_before_memory_blocks_instantiated() {
    let mem = unsafe { get_memory(MEMORY_BLOCK_SLICE_SIZE) };
    let address = mem.as_ptr() as usize;
    let mut gcd = GCD::new(48);

    assert_eq!(Err(Error::NotFound), gcd.remove_memory_space(address, MEMORY_BLOCK_SLICE_SIZE));
  }

  #[test]
  fn test_remove_memory_space_with_0_len_block() {
    let (mut gcd, _) = create_gcd();

    // Add memory space to remove in a valid area.
    assert!(unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0, 10, 0) }.is_ok());

    let snapshot = copy_memory_block(&gcd);
    assert_eq!(Err(Error::InvalidParameter), gcd.remove_memory_space(5, 0));

    assert_eq!(
      Err(Error::InvalidParameter),
      gcd.remove_memory_space(10, 0),
      "If there is no allocate done first, 0 length invalid param should have priority."
    );

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_remove_memory_space_outside_processor_range() {
    let (mut gcd, _) = create_gcd();
    // Add memory space to remove in a valid area.
    assert!(unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, gcd.maximum_address - 10, 10, 0) }.is_ok());

    let snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::Unsupported),
      gcd.remove_memory_space(gcd.maximum_address - 10, 11),
      "An address outside the processor range support is invalid."
    );
    assert_eq!(
      Err(Error::Unsupported),
      gcd.remove_memory_space(gcd.maximum_address, 10),
      "An address outside the processor range support is invalid."
    );

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_remove_memory_space_in_range_not_added() {
    let (mut gcd, _) = create_gcd();
    // Add memory space to remove in a valid area.
    assert!(unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 100, 10, 0) }.is_ok());

    let snapshot = copy_memory_block(&gcd);

    assert_eq!(Err(Error::NotFound), gcd.remove_memory_space(95, 10), "Can't remove memory space partially added.");
    assert_eq!(Err(Error::NotFound), gcd.remove_memory_space(105, 10), "Can't remove memory space partially added.");
    assert_eq!(
      Err(Error::NotFound),
      gcd.remove_memory_space(10, 10),
      "Can't remove memory space not previously added."
    );

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_remove_memory_space_in_range_allocated() {
    let (mut gcd, address) = create_gcd();

    let snapshot = copy_memory_block(&gcd);

    // Not found has a priority over the access denied because the check if the range is valid is done earlier.
    assert_eq!(
      Err(Error::NotFound),
      gcd.remove_memory_space(address - 5, 10),
      "Can't remove memory space partially allocated."
    );
    assert_eq!(
      Err(Error::NotFound),
      gcd.remove_memory_space(address + MEMORY_BLOCK_SLICE_SIZE - 5, 10),
      "Can't remove memory space partially allocated."
    );

    assert_eq!(
      Err(Error::AccessDenied),
      gcd.remove_memory_space(address + 10, 10),
      "Can't remove memory space not previously allocated."
    );

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_remove_memory_space_when_memory_block_full() {
    let (mut gcd, address) = create_gcd();
    let addr = address + MEMORY_BLOCK_SLICE_SIZE;

    assert!(unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, addr, 10, 0 as u64) }.is_ok());
    let mut n = 1;
    while gcd.memory_block_count() < MEMORY_BLOCK_SLICE_LEN {
      assert!(unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, addr + 10 + n, 1, n as u64) }.is_ok());
      n += 1;
    }

    assert!(is_gcd_memory_slice_valid(&gcd));
    let memory_blocks_snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::OutOfResources),
      gcd.remove_memory_space(addr, 5),
      "Should return out of memory if there is no space in memory blocks."
    );

    assert_eq!(memory_blocks_snapshot, copy_memory_block(&gcd),);
  }

  #[test]
  fn test_remove_memory_space_block_merging() {
    let (mut gcd, address) = create_gcd();
    assert!(unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 1, address - 2, 0) }.is_ok());

    let block_count = gcd.memory_block_count();

    for i in 1..10 {
      assert!(gcd.remove_memory_space(address - 1 - i, 1).is_ok());
    }

    // First index because the add memory started at address 1.
    assert_eq!(address - 10, gcd.memory_blocks.as_ref().unwrap()[2].as_ref().base_address as usize);
    assert_eq!(10, gcd.memory_blocks.as_ref().unwrap()[2].as_ref().length as usize);
    assert_eq!(block_count, gcd.memory_block_count());
    assert!(is_gcd_memory_slice_valid(&gcd));

    for i in 1..10 {
      assert!(gcd.remove_memory_space(0 + i, 1).is_ok());
    }
    // First index because the add memory started at address 1.
    assert_eq!(0, gcd.memory_blocks.as_ref().unwrap()[0].as_ref().base_address as usize);
    assert_eq!(10, gcd.memory_blocks.as_ref().unwrap()[0].as_ref().length as usize);
    assert_eq!(block_count, gcd.memory_block_count());
    assert!(is_gcd_memory_slice_valid(&gcd));

    // Removing in the middle should create a 2 new block.
    assert!(gcd.remove_memory_space(100, 1).is_ok());
    assert_eq!(block_count + 2, gcd.memory_block_count());
    assert!(is_gcd_memory_slice_valid(&gcd));
  }

  #[test]
  fn test_remove_memory_space_state() {
    let (mut gcd, address) = create_gcd();
    assert!(unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0, address, 123) }.is_ok());

    match gcd.remove_memory_space(0, 10) {
      Ok(_) => {
        let mb = gcd.memory_blocks.as_ref().unwrap()[0];
        match mb {
          MemoryBlock::Unallocated(md) => {
            assert_eq!(0, md.base_address);
            assert_eq!(10, md.length);
            assert_eq!(0, md.capabilities);
            assert_eq!(0, md.image_handle as usize);
            assert_eq!(0, md.device_handle as usize);
          }
          MemoryBlock::Allocated(_) => assert!(false, "remove should keep the block unallocated"),
        }
      }
      Err(e) => assert!(false, "{e:?}"),
    }
  }

  #[test]
  fn test_allocate_memory_space_before_memory_blocks_instantiated() {
    let mut gcd = GCD::new(48);
    assert_eq!(
      Err(Error::NotFound),
      gcd.allocate_memory_space(AllocateType::Address(0), GcdMemoryType::SystemMemory, 0, 10, 1 as _, None)
    );
  }

  #[test]
  fn test_allocate_memory_space_with_0_len_block() {
    let (mut gcd, _) = create_gcd();
    let snapshot = copy_memory_block(&gcd);
    assert_eq!(
      Err(Error::InvalidParameter),
      gcd.allocate_memory_space(AllocateType::BottomUp(None), GcdMemoryType::Reserved, 0, 0, 1 as _, None),
    );
    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_allocate_memory_space_with_null_image_handle() {
    let (mut gcd, _) = create_gcd();
    let snapshot = copy_memory_block(&gcd);
    assert_eq!(
      Err(Error::InvalidParameter),
      gcd.allocate_memory_space(AllocateType::BottomUp(None), GcdMemoryType::Reserved, 0, 10, ptr::null_mut(), None),
    );
    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_allocate_memory_space_with_address_outside_processor_range() {
    let (mut gcd, _) = create_gcd();
    let snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::Unsupported),
      gcd.allocate_memory_space(
        AllocateType::Address(gcd.maximum_address - 100),
        GcdMemoryType::Reserved,
        0,
        1000,
        1 as _,
        None
      ),
    );
    assert_eq!(
      Err(Error::Unsupported),
      gcd.allocate_memory_space(
        AllocateType::Address(gcd.maximum_address + 100),
        GcdMemoryType::Reserved,
        0,
        1000,
        1 as _,
        None
      ),
    );

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_allocate_memory_space_with_all_memory_type() {
    let (mut gcd, _) = create_gcd();
    for (i, memory_type) in [
      GcdMemoryType::Reserved,
      GcdMemoryType::SystemMemory,
      GcdMemoryType::Persistent,
      GcdMemoryType::MemoryMappedIo,
      GcdMemoryType::MoreReliable,
      GcdMemoryType::Unaccepted,
    ]
    .into_iter()
    .enumerate()
    {
      unsafe { gcd.add_memory_space(memory_type, i * 10, 10, 0) }.unwrap();
      let res = gcd.allocate_memory_space(AllocateType::Address(i * 10), memory_type, 0, 10, 1 as _, None);
      match memory_type {
        GcdMemoryType::Unaccepted => assert_eq!(Err(Error::InvalidParameter), res),
        _ => assert!(res.is_ok()),
      }
    }
  }

  #[test]
  fn test_allocate_memory_space_when_memory_blocks_full() {
    let (mut gcd, address) = create_gcd();
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0, address, 0) }.unwrap();

    let mut n = 1;
    while gcd.memory_block_count() < MEMORY_BLOCK_SLICE_LEN {
      gcd.allocate_memory_space(AllocateType::BottomUp(None), GcdMemoryType::SystemMemory, 0, 1, n as _, None).unwrap();
      n += 1;
    }

    assert!(is_gcd_memory_slice_valid(&gcd));
    let memory_blocks_snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::OutOfResources),
      gcd.allocate_memory_space(AllocateType::BottomUp(None), GcdMemoryType::SystemMemory, 0, 1, 1 as _, None)
    );

    assert_eq!(memory_blocks_snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_allocate_memory_space_with_no_memory_space_available() {
    let (mut gcd, _) = create_gcd();

    // Add memory space of len 100 to multiple space.
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0, 100, 0) }.unwrap();
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 1000, 100, 0) }.unwrap();
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, gcd.maximum_address - 100, 100, 0) }.unwrap();

    let memory_blocks_snapshot = copy_memory_block(&gcd);

    // Try to allocate chunk bigger than 100.
    for allocate_type in [
      AllocateType::BottomUp(None),
      AllocateType::BottomUp(Some(10_000)),
      AllocateType::TopDown(None),
      AllocateType::TopDown(Some(10_000)),
      AllocateType::Address(10_000),
    ] {
      assert_eq!(
        Err(Error::NotFound),
        gcd.allocate_memory_space(allocate_type, GcdMemoryType::SystemMemory, 0, 1000, 1 as _, None),
        "Assert fail with allocate type: {allocate_type:?}"
      );
    }

    assert_eq!(memory_blocks_snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_allocate_memory_space_alignment() {
    let (mut gcd, _) = create_gcd();
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0, 0x1000, 0) }.unwrap();

    assert_eq!(
      Ok(0),
      gcd.allocate_memory_space(AllocateType::BottomUp(None), GcdMemoryType::SystemMemory, 0, 0x0f, 1 as _, None),
      "Allocate bottom up without alignment"
    );
    assert_eq!(
      Ok(0x10),
      gcd.allocate_memory_space(AllocateType::BottomUp(None), GcdMemoryType::SystemMemory, 4, 0x10, 1 as _, None),
      "Allocate bottom up with alignment of 4 bits (find first address that is aligned)"
    );
    assert_eq!(
      Ok(0x20),
      gcd.allocate_memory_space(AllocateType::BottomUp(None), GcdMemoryType::SystemMemory, 4, 100, 1 as _, None),
      "Allocate bottom up with alignment of 4 bits (already aligned)"
    );
    assert_eq!(
      Ok(0xff1),
      gcd.allocate_memory_space(AllocateType::TopDown(None), GcdMemoryType::SystemMemory, 0, 0x0f, 1 as _, None),
      "Allocate top down without alignment"
    );
    assert_eq!(
      Ok(0xfe0),
      gcd.allocate_memory_space(AllocateType::TopDown(None), GcdMemoryType::SystemMemory, 4, 0x0f, 1 as _, None),
      "Allocate top down with alignment of 4 bits (find first address that is aligned)"
    );
    assert_eq!(
      Ok(0xf00),
      gcd.allocate_memory_space(AllocateType::TopDown(None), GcdMemoryType::SystemMemory, 4, 0xe0, 1 as _, None),
      "Allocate top down with alignment of 4 bits (already aligned)"
    );
    assert_eq!(
      Ok(0xa00),
      gcd.allocate_memory_space(AllocateType::Address(0xa00), GcdMemoryType::SystemMemory, 4, 100, 1 as _, None),
      "Allocate Address with alignment of 4 bits (already aligned)"
    );

    assert!(is_gcd_memory_slice_valid(&gcd));
    let memory_blocks_snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::NotFound),
      gcd.allocate_memory_space(AllocateType::Address(0xa0f), GcdMemoryType::SystemMemory, 4, 100, 1 as _, None),
    );

    assert_eq!(memory_blocks_snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_allocate_memory_space_block_merging() {
    let (mut gcd, _) = create_gcd();
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0x1000, 0x1000, 0) }.unwrap();

    for allocate_type in [AllocateType::BottomUp(None), AllocateType::TopDown(None)] {
      let block_count = gcd.memory_block_count();
      assert!(
        gcd.allocate_memory_space(allocate_type, GcdMemoryType::SystemMemory, 0, 1, 1 as _, None).is_ok(),
        "{allocate_type:?}"
      );
      assert_eq!(block_count + 1, gcd.memory_block_count());
      assert!(
        gcd.allocate_memory_space(allocate_type, GcdMemoryType::SystemMemory, 0, 1, 1 as _, None).is_ok(),
        "{allocate_type:?}"
      );
      assert_eq!(block_count + 1, gcd.memory_block_count());
      assert!(
        gcd.allocate_memory_space(allocate_type, GcdMemoryType::SystemMemory, 0, 1, 2 as _, None).is_ok(),
        "{allocate_type:?}: A different image handle should not result in a merge."
      );
      assert_eq!(block_count + 2, gcd.memory_block_count());
      assert!(
        gcd.allocate_memory_space(allocate_type, GcdMemoryType::SystemMemory, 0, 1, 2 as _, Some(1 as _)).is_ok(),
        "{allocate_type:?}: A different device handle should not result in a merge."
      );
      assert_eq!(block_count + 3, gcd.memory_block_count());
    }

    let block_count = gcd.memory_block_count();
    assert_eq!(
      Ok(0x1000 + 4),
      gcd.allocate_memory_space(
        AllocateType::Address(0x1000 + 4),
        GcdMemoryType::SystemMemory,
        0,
        1,
        2 as _,
        Some(1 as _)
      ),
      "Merge should work with address allocation too."
    );
    assert_eq!(block_count, gcd.memory_block_count());

    assert!(is_gcd_memory_slice_valid(&gcd));
  }

  #[test]
  fn test_allocate_memory_space_with_address_not_added() {
    let (mut gcd, _) = create_gcd();

    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0x100, 10, 0) }.unwrap();

    let snapshot = copy_memory_block(&gcd);

    assert_eq!(
      Err(Error::NotFound),
      gcd.allocate_memory_space(AllocateType::Address(0x100), GcdMemoryType::SystemMemory, 0, 11, 1 as _, None),
    );
    assert_eq!(
      Err(Error::NotFound),
      gcd.allocate_memory_space(AllocateType::Address(0x95), GcdMemoryType::SystemMemory, 0, 10, 1 as _, None),
    );
    assert_eq!(
      Err(Error::NotFound),
      gcd.allocate_memory_space(AllocateType::Address(110), GcdMemoryType::SystemMemory, 0, 5, 1 as _, None),
    );
    assert_eq!(
      Err(Error::NotFound),
      gcd.allocate_memory_space(AllocateType::Address(0), GcdMemoryType::SystemMemory, 0, 5, 1 as _, None),
    );

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_allocate_memory_space_with_address_allocated() {
    let (mut gcd, address) = create_gcd();
    assert_eq!(
      Err(Error::NotFound),
      gcd.allocate_memory_space(AllocateType::Address(address), GcdMemoryType::SystemMemory, 0, 5, 1 as _, None),
    );
  }

  #[test]
  fn test_free_memory_space_before_memory_blocks_instantiated() {
    let mut gcd = GCD::new(48);
    assert_eq!(Err(Error::NotFound), gcd.free_memory_space(0, 100));
  }

  #[test]
  fn test_free_memory_space_when_0_len_block() {
    let (mut gcd, _) = create_gcd();
    let snapshot = copy_memory_block(&gcd);
    assert_eq!(Err(Error::InvalidParameter), gcd.remove_memory_space(0, 0));
    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_free_memory_space_outside_processor_range() {
    let (mut gcd, _) = create_gcd();

    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, gcd.maximum_address - 100, 100, 0) }.unwrap();
    gcd
      .allocate_memory_space(
        AllocateType::Address(gcd.maximum_address - 100),
        GcdMemoryType::SystemMemory,
        0,
        100,
        1 as _,
        None,
      )
      .unwrap();

    let snapshot = copy_memory_block(&gcd);

    assert_eq!(Err(Error::Unsupported), gcd.free_memory_space(gcd.maximum_address, 10));
    assert_eq!(Err(Error::Unsupported), gcd.free_memory_space(gcd.maximum_address - 99, 100));
    assert_eq!(Err(Error::Unsupported), gcd.free_memory_space(gcd.maximum_address + 1, 100));

    assert_eq!(snapshot, copy_memory_block(&gcd));
  }

  #[test]
  fn test_free_memory_space_in_range_not_allocated() {
    let (mut gcd, _) = create_gcd();
    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 1000, 100, 0) }.unwrap();
    gcd.allocate_memory_space(AllocateType::Address(1000), GcdMemoryType::SystemMemory, 0, 100, 1 as _, None).unwrap();

    assert_eq!(Err(Error::NotFound), gcd.free_memory_space(1050, 100));
    assert_eq!(Err(Error::NotFound), gcd.free_memory_space(950, 100));
    assert_eq!(Err(Error::NotFound), gcd.free_memory_space(0, 100));
  }

  #[test]
  fn test_free_memory_space_when_memory_block_full() {
    let (mut gcd, _) = create_gcd();

    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0, 100, 0) }.unwrap();
    gcd.allocate_memory_space(AllocateType::Address(0), GcdMemoryType::SystemMemory, 0, 100, 1 as _, None).unwrap();

    let mut n = 1;
    while gcd.memory_block_count() < MEMORY_BLOCK_SLICE_LEN {
      unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 1000 + n, 1, n as u64) }.unwrap();
      n += 1;
    }
    let memory_blocks_snapshot = copy_memory_block(&gcd);

    assert_eq!(Err(Error::OutOfResources), gcd.free_memory_space(0, 1));

    assert_eq!(memory_blocks_snapshot, copy_memory_block(&gcd),);
  }

  #[test]
  fn test_free_memory_space_merging() {
    let (mut gcd, _) = create_gcd();

    unsafe { gcd.add_memory_space(GcdMemoryType::SystemMemory, 0, 1000, 0) }.unwrap();
    gcd.allocate_memory_space(AllocateType::Address(0), GcdMemoryType::SystemMemory, 0, 1000, 1 as _, None).unwrap();

    let block_count = gcd.memory_block_count();
    assert_eq!(Ok(()), gcd.free_memory_space(0, 100), "Free beginning of a block.");
    assert_eq!(block_count + 1, gcd.memory_block_count());
    assert_eq!(Ok(()), gcd.free_memory_space(500, 100), "Free in the middle of a block");
    assert_eq!(block_count + 3, gcd.memory_block_count());
    assert_eq!(Ok(()), gcd.free_memory_space(900, 100), "Free at the end of a block");
    assert_eq!(block_count + 4, gcd.memory_block_count());

    let block_count = gcd.memory_block_count();
    assert_eq!(Ok(()), gcd.free_memory_space(100, 100));
    assert_eq!(block_count, gcd.memory_block_count());
    let mb = gcd.memory_blocks.as_ref().unwrap()[0];
    assert_eq!(0, mb.as_ref().base_address);
    assert_eq!(200, mb.as_ref().length);

    assert_eq!(Ok(()), gcd.free_memory_space(600, 100));
    assert_eq!(block_count, gcd.memory_block_count());
    let mb = gcd.memory_blocks.as_ref().unwrap()[2];
    assert_eq!(500, mb.as_ref().base_address);
    assert_eq!(200, mb.as_ref().length);

    assert_eq!(Ok(()), gcd.free_memory_space(800, 100));
    assert_eq!(block_count, gcd.memory_block_count());
    let mb = gcd.memory_blocks.as_ref().unwrap()[4];
    assert_eq!(800, mb.as_ref().base_address);
    assert_eq!(200, mb.as_ref().length);

    assert_eq!(Ok(()), gcd.free_memory_space(750, 10));
    assert_eq!(block_count + 2, gcd.memory_block_count());

    assert!(is_gcd_memory_slice_valid(&gcd));
  }

  fn create_gcd() -> (GCD, usize) {
    let mem = unsafe { get_memory(MEMORY_BLOCK_SLICE_SIZE) };
    let address = mem.as_ptr() as usize;
    let mut gcd = GCD::new(48);
    unsafe {
      gcd.add_memory_space(GcdMemoryType::SystemMemory, address, MEMORY_BLOCK_SLICE_SIZE, 0).unwrap();
    }
    (gcd, address)
  }

  fn copy_memory_block(gcd: &GCD) -> Vec<MemoryBlock> {
    let Some(memory_blocks) = &gcd.memory_blocks else {
      return vec![];
    };
    memory_blocks.iter().map(|b| MemoryBlock::clone(b)).collect()
  }

  fn is_gcd_memory_slice_valid(gcd: &GCD) -> bool {
    if let Some(memory_blocks) = gcd.memory_blocks.as_ref() {
      match memory_blocks.first().map(|b| b.start()) {
        Some(0) => (),
        _ => return false,
      }
      let mut last_addr = 0;
      let mut w = memory_blocks.windows(2);
      while let Some([a, b]) = w.next() {
        if a.end() != b.start() || a.is_same_state(b) {
          return false;
        }
        last_addr = b.end();
      }
      if last_addr != gcd.maximum_address {
        return false;
      }
    }
    true
  }

  unsafe fn get_memory(size: usize) -> &'static mut [u8] {
    let addr = alloc::alloc::alloc(alloc::alloc::Layout::from_size_align(size, 8).unwrap());
    core::slice::from_raw_parts_mut(addr, size)
  }
}
