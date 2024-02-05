use core::fmt::Debug;

use r_efi::efi;
use r_pi::dxe_services;

use crate::error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
  InvalidStateTransition,
  BlockOutsideRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryBlock {
  Unallocated(dxe_services::MemorySpaceDescriptor),
  Allocated(dxe_services::MemorySpaceDescriptor),
}

#[derive(Debug)]
pub enum StateTransition {
  Add(dxe_services::GcdMemoryType, u64),
  Remove,
  Allocate(efi::Handle, Option<efi::Handle>),
  Free,
  SetAttributes(u64),
  SetCapabilities(u64),
}

#[derive(Debug)]
pub enum MemoryBlockSplit<'a> {
  Same(&'a mut MemoryBlock),
  Before(&'a mut MemoryBlock, MemoryBlock),
  After(&'a mut MemoryBlock, MemoryBlock),
  Middle(&'a mut MemoryBlock, MemoryBlock, MemoryBlock),
}

impl MemoryBlock {
  pub fn merge(&mut self, other: &mut MemoryBlock) -> bool {
    if self.is_same_state(other) && self.end() == other.start() {
      self.as_mut().length += other.as_ref().length;
      other.as_mut().length = 0;
      true
    } else {
      false
    }
  }

  pub fn split(&mut self, base_address: usize, len: usize) -> Result<MemoryBlockSplit, Error> {
    let start = base_address;
    let end = base_address + len;

    if !(self.start() <= start && start < end && end <= self.end()) {
      return Err(Error::BlockOutsideRange);
    }

    if self.start() == start && end == self.end() {
      return Ok(MemoryBlockSplit::Same(self));
    }

    if self.start() == start && end < self.end() {
      let mut next = MemoryBlock::clone(self);

      self.as_mut().base_address = base_address as u64;
      self.as_mut().length = len as u64;
      next.as_mut().base_address = end as u64;
      next.as_mut().length -= len as u64;

      return Ok(MemoryBlockSplit::Before(self, next));
    }

    if self.start() < start && end == self.end() {
      let mut next = MemoryBlock::clone(self);

      self.as_mut().length -= len as u64;
      next.as_mut().base_address = base_address as u64;
      next.as_mut().length = len as u64;

      return Ok(MemoryBlockSplit::After(self, next));
    }

    if self.start() < start && end < self.end() {
      let mut next = MemoryBlock::clone(self);
      let mut last = MemoryBlock::clone(self);

      self.as_mut().length = (start - self.start()) as u64;
      next.as_mut().base_address = base_address as u64;
      next.as_mut().length = len as u64;
      last.as_mut().length = (last.end() - end) as u64;
      last.as_mut().base_address = end as u64;

      return Ok(MemoryBlockSplit::Middle(self, next, last));
    }

    unreachable!()
  }

  pub fn split_state_transition(
    &mut self,
    base_address: usize,
    len: usize,
    transition: StateTransition,
  ) -> Result<MemoryBlockSplit, Error> {
    let mut split = self.split(base_address, len)?;

    match &mut split {
      MemoryBlockSplit::Same(mb) => {
        mb.state_transition(transition)?;
      }
      MemoryBlockSplit::Before(mb, next) => {
        if let Err(e) = mb.state_transition(transition) {
          mb.merge(next);
          error!(e);
        }
      }
      MemoryBlockSplit::After(prev, mb) => {
        if let Err(e) = mb.state_transition(transition) {
          prev.merge(mb);
          error!(e)
        }
      }
      MemoryBlockSplit::Middle(prev, mb, next) => {
        if let Err(e) = mb.state_transition(transition) {
          mb.merge(next);
          prev.merge(mb);
          error!(e)
        }
      }
    }

    Ok(split)
  }

  pub fn is_same_state(&self, other: &MemoryBlock) -> bool {
    matches!((self, other),
      (MemoryBlock::Unallocated(self_desc), MemoryBlock::Unallocated(other_desc)) |
      (MemoryBlock::Allocated(self_desc), MemoryBlock::Allocated(other_desc))
        if self_desc.memory_type == other_desc.memory_type
          && self_desc.attributes == other_desc.attributes
          && self_desc.capabilities == other_desc.capabilities
          && self_desc.device_handle == other_desc.device_handle
          && self_desc.image_handle == other_desc.image_handle
    )
  }

  pub fn state_transition(&mut self, transition: StateTransition) -> Result<(), Error> {
    match transition {
      StateTransition::Add(memory_type, capabilities) => self.add_transition(memory_type, capabilities),
      StateTransition::Remove => self.remove_transition(),
      StateTransition::Allocate(image_handle, device_handle) => self.allocate_transition(image_handle, device_handle),
      StateTransition::Free => self.free_transition(),
      StateTransition::SetAttributes(attributes) => self.attribute_transition(attributes),
      StateTransition::SetCapabilities(capabilities) => self.capabilities_transition(capabilities),
    }
  }

  pub fn add_transition(&mut self, memory_type: dxe_services::GcdMemoryType, capabilities: u64) -> Result<(), Error> {
    match self {
      Self::Unallocated(md)
        if md.memory_type == dxe_services::GcdMemoryType::NonExistent
          && memory_type != dxe_services::GcdMemoryType::NonExistent =>
      {
        md.memory_type = memory_type;
        md.capabilities = capabilities;
        Ok(())
      }
      _ => Err(Error::InvalidStateTransition),
    }
  }

  pub fn remove_transition(&mut self) -> Result<(), Error> {
    match self {
      Self::Unallocated(md) if md.memory_type != dxe_services::GcdMemoryType::NonExistent => {
        md.memory_type = dxe_services::GcdMemoryType::NonExistent;
        md.capabilities = 0;
        Ok(())
      }
      _ => Err(Error::InvalidStateTransition),
    }
  }

  pub fn allocate_transition(
    &mut self,
    image_handle: efi::Handle,
    device_handle: Option<efi::Handle>,
  ) -> Result<(), Error> {
    match self {
      Self::Unallocated(md)
        if !matches!(
          md.memory_type,
          dxe_services::GcdMemoryType::NonExistent | dxe_services::GcdMemoryType::Unaccepted
        ) =>
      {
        md.image_handle = image_handle;
        if let Some(device_handle) = device_handle {
          md.device_handle = device_handle;
        }
        *self = Self::Allocated(*md);
        Ok(())
      }
      _ => Err(Error::InvalidStateTransition),
    }
  }

  pub fn free_transition(&mut self) -> Result<(), Error> {
    match self {
      Self::Allocated(md) if md.memory_type != dxe_services::GcdMemoryType::NonExistent => {
        md.image_handle = 0 as efi::Handle;
        md.device_handle = 0 as efi::Handle;
        *self = Self::Unallocated(*md);
        Ok(())
      }
      _ => Err(Error::InvalidStateTransition),
    }
  }

  pub fn attribute_transition(&mut self, attributes: u64) -> Result<(), Error> {
    match self {
      Self::Allocated(md) | Self::Unallocated(md) if md.memory_type != dxe_services::GcdMemoryType::NonExistent => {
        if (md.capabilities | attributes) != md.capabilities {
          Err(Error::InvalidStateTransition)
        } else {
          md.attributes = attributes;
          Ok(())
        }
      }
      _ => Err(Error::InvalidStateTransition),
    }
  }

  pub fn capabilities_transition(&mut self, capabilities: u64) -> Result<(), Error> {
    match self {
      Self::Allocated(md) | Self::Unallocated(md) if md.memory_type != dxe_services::GcdMemoryType::NonExistent => {
        if (capabilities | md.attributes) != capabilities {
          Err(Error::InvalidStateTransition)
        } else {
          md.capabilities = capabilities;
          Ok(())
        }
      }
      _ => Err(Error::InvalidStateTransition),
    }
  }

  pub fn start(&self) -> usize {
    self.as_ref().base_address as usize
  }

  pub fn end(&self) -> usize {
    (self.as_ref().base_address + self.as_ref().length) as usize
  }

  pub fn len(&self) -> usize {
    self.as_ref().length as usize
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }
}

impl AsRef<dxe_services::MemorySpaceDescriptor> for MemoryBlock {
  fn as_ref(&self) -> &dxe_services::MemorySpaceDescriptor {
    match self {
      MemoryBlock::Unallocated(msd) | MemoryBlock::Allocated(msd) => msd,
    }
  }
}

impl AsMut<dxe_services::MemorySpaceDescriptor> for MemoryBlock {
  fn as_mut(&mut self) -> &mut dxe_services::MemorySpaceDescriptor {
    match self {
      MemoryBlock::Unallocated(msd) | MemoryBlock::Allocated(msd) => msd,
    }
  }
}
