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
pub enum IoBlock {
  Unallocated(dxe_services::IoSpaceDescriptor),
  Allocated(dxe_services::IoSpaceDescriptor),
}

#[derive(Debug)]
pub enum StateTransition {
  Add(dxe_services::GcdIoType),
  Remove,
  Allocate(efi::Handle, Option<efi::Handle>),
  Free,
}

#[derive(Debug)]
pub enum IoBlockSplit<'a> {
  Same(&'a mut IoBlock),
  Before(&'a mut IoBlock, IoBlock),
  After(&'a mut IoBlock, IoBlock),
  Middle(&'a mut IoBlock, IoBlock, IoBlock),
}

impl IoBlock {
  pub fn merge(&mut self, other: &mut IoBlock) -> bool {
    if self.is_same_state(other) && self.end() == other.start() {
      self.as_mut().length += other.as_ref().length;
      other.as_mut().length = 0;
      true
    } else {
      false
    }
  }

  pub fn split(&mut self, base_address: usize, len: usize) -> Result<IoBlockSplit, Error> {
    let start = base_address;
    let end = base_address + len;

    if !(self.start() <= start && start < end && end <= self.end()) {
      return Err(Error::BlockOutsideRange);
    }

    if self.start() == start && end == self.end() {
      return Ok(IoBlockSplit::Same(self));
    }

    if self.start() == start && end < self.end() {
      let mut next = IoBlock::clone(self);

      self.as_mut().base_address = base_address as u64;
      self.as_mut().length = len as u64;
      next.as_mut().base_address = end as u64;
      next.as_mut().length -= len as u64;

      return Ok(IoBlockSplit::Before(self, next));
    }

    if self.start() < start && end == self.end() {
      let mut next = IoBlock::clone(self);

      self.as_mut().length -= len as u64;
      next.as_mut().base_address = base_address as u64;
      next.as_mut().length = len as u64;

      return Ok(IoBlockSplit::After(self, next));
    }

    if self.start() < start && end < self.end() {
      let mut next = IoBlock::clone(self);
      let mut last = IoBlock::clone(self);

      self.as_mut().length = (start - self.start()) as u64;
      next.as_mut().base_address = base_address as u64;
      next.as_mut().length = len as u64;
      last.as_mut().length = (last.end() - end) as u64;
      last.as_mut().base_address = end as u64;

      return Ok(IoBlockSplit::Middle(self, next, last));
    }

    unreachable!()
  }

  pub fn split_state_transition(
    &mut self,
    base_address: usize,
    len: usize,
    transition: StateTransition,
  ) -> Result<IoBlockSplit, Error> {
    let mut split = self.split(base_address, len)?;

    match &mut split {
      IoBlockSplit::Same(mb) => {
        mb.state_transition(transition)?;
      }
      IoBlockSplit::Before(mb, next) => {
        if let Err(e) = mb.state_transition(transition) {
          mb.merge(next);
          error!(e);
        }
      }
      IoBlockSplit::After(prev, mb) => {
        if let Err(e) = mb.state_transition(transition) {
          prev.merge(mb);
          error!(e)
        }
      }
      IoBlockSplit::Middle(prev, mb, next) => {
        if let Err(e) = mb.state_transition(transition) {
          mb.merge(next);
          prev.merge(mb);
          error!(e)
        }
      }
    }

    Ok(split)
  }

  pub fn is_same_state(&self, other: &IoBlock) -> bool {
    matches!((self, other),
      (IoBlock::Unallocated(self_desc), IoBlock::Unallocated(other_desc)) |
      (IoBlock::Allocated(self_desc), IoBlock::Allocated(other_desc))
        if self_desc.io_type == other_desc.io_type
           && self_desc.device_handle == other_desc.device_handle
           && self_desc.image_handle == other_desc.image_handle
    )
  }

  pub fn state_transition(&mut self, transition: StateTransition) -> Result<(), Error> {
    match transition {
      StateTransition::Add(io_type) => self.add_transition(io_type),
      StateTransition::Remove => self.remove_transition(),
      StateTransition::Allocate(image_handle, device_handle) => self.allocate_transition(image_handle, device_handle),
      StateTransition::Free => self.free_transition(),
    }
  }

  pub fn add_transition(&mut self, io_type: dxe_services::GcdIoType) -> Result<(), Error> {
    match self {
      Self::Unallocated(id)
        if id.io_type == dxe_services::GcdIoType::NonExistent && io_type != dxe_services::GcdIoType::NonExistent =>
      {
        id.io_type = io_type;
        Ok(())
      }
      _ => Err(Error::InvalidStateTransition),
    }
  }

  pub fn remove_transition(&mut self) -> Result<(), Error> {
    match self {
      Self::Unallocated(id) if id.io_type != dxe_services::GcdIoType::NonExistent => {
        id.io_type = dxe_services::GcdIoType::NonExistent;
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
      Self::Unallocated(id) if id.io_type != dxe_services::GcdIoType::NonExistent => {
        id.image_handle = image_handle;
        if let Some(device_handle) = device_handle {
          id.device_handle = device_handle;
        }
        *self = Self::Allocated(*id);
        Ok(())
      }
      _ => Err(Error::InvalidStateTransition),
    }
  }

  pub fn free_transition(&mut self) -> Result<(), Error> {
    match self {
      Self::Allocated(id) if id.io_type != dxe_services::GcdIoType::NonExistent => {
        id.image_handle = 0 as efi::Handle;
        id.device_handle = 0 as efi::Handle;
        *self = Self::Unallocated(*id);
        Ok(())
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

impl AsRef<dxe_services::IoSpaceDescriptor> for IoBlock {
  fn as_ref(&self) -> &dxe_services::IoSpaceDescriptor {
    match self {
      IoBlock::Unallocated(msd) | IoBlock::Allocated(msd) => msd,
    }
  }
}

impl AsMut<dxe_services::IoSpaceDescriptor> for IoBlock {
  fn as_mut(&mut self) -> &mut dxe_services::IoSpaceDescriptor {
    match self {
      IoBlock::Unallocated(msd) | IoBlock::Allocated(msd) => msd,
    }
  }
}
