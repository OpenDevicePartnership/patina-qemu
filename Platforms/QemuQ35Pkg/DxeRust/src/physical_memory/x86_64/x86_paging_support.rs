//! # X86 Paging Support module
//! provides utilities related to x86 paging.
use core::{
  alloc::{Allocator, Layout},
  ops::Range,
};

use alloc::alloc::Global;
use x86_64::{
  registers::control::{Cr3, Cr3Flags},
  structures::paging::{
    mapper::MapToError, FrameAllocator, Mapper, OffsetPageTable, PageSize, PageTable, PageTableFlags, PhysFrame,
    Size4KiB,
  },
  PhysAddr, VirtAddr,
};

use r_efi::efi;

pub static PAGE_TABLE: tpl_lock::TplMutex<GlobalPageTable> =
  tpl_lock::TplMutex::new(efi::TPL_HIGH_LEVEL, GlobalPageTable::empty(), "PageLock");

#[derive(Debug)]
pub enum PageTableError {
  NotReady,
  AlreadyStarted,
  InvalidRange,
  MapFailure,
  OutOfMemory,
}

pub struct GlobalPageTable {
  mapper: Option<OffsetPageTable<'static>>,
}

impl GlobalPageTable {
  pub const fn empty() -> Self {
    GlobalPageTable { mapper: None }
  }
  /// Initialize page table
  ///
  /// # Safety
  /// Caller must ensure:
  /// 1. Global allocator is initialized,
  /// 2. Global allocator has enough memory required to allocate memory for
  ///    pages to cover the range memory_start..memory_end when setting up the
  ///    page table,
  /// 3. memory_start..memory_end contains the current stack, heap and dxe
  ///    rust code so that the program is still viable after switching to this
  ///    page table.
  pub unsafe fn init(&mut self, range: Range<u64>) -> Result<(), PageTableError> {
    if let Some(_mapper) = &self.mapper {
      return Err(PageTableError::AlreadyStarted);
    }

    let layout = Layout::new::<PageTable>();
    let l4_ptr = Global.allocate_zeroed(layout).map_err(|_| PageTableError::OutOfMemory)?.as_mut_ptr();

    let level_4_table = &mut *(l4_ptr as *mut PageTable);

    let mapper = OffsetPageTable::new(level_4_table, VirtAddr::new(0));
    self.mapper = Some(mapper);

    //map the provided initial range into the table. do_map() will request additional frames as needed
    //from the global allocator.
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    self.do_map(range, flags, false)?;

    // now that the table is constructed, write Cr3 to activate it.
    let level_4_table_ptr = self.mapper.as_mut().unwrap().level_4_table() as *const PageTable as u64;
    let level_4_frame = PhysFrame::containing_address(PhysAddr::new(level_4_table_ptr));
    Cr3::write(level_4_frame, Cr3Flags::empty());

    Ok(())
  }

  /// Map a range of memory and set flags as specified.
  /// # Safety
  /// Caller must ensure:
  /// 1. Global allocator is initialized
  /// 2. Global allocator has enough memory to allocate pages to cover the
  ///    range specified.
  /// 3. The specified range corresponds to functional usable memory.
  /// 4. FIXME: if a frame in the range is already mapped, it won't be updated to the new flags
  pub unsafe fn map_range(&mut self, range: Range<u64>, flags: PageTableFlags) -> Result<(), PageTableError> {
    self.do_map(range, flags, true)?;
    Ok(())
  }

  unsafe fn do_map(&mut self, range: Range<u64>, flags: PageTableFlags, flush: bool) -> Result<(), PageTableError> {
    //FIXME: this is not optimized for time or space - it allocates 4K pages everywhere. This is big and slow,
    //especially when mapping large ranges, but this naive approach makes it easier to deal with overlapping
    //mapping requests. A less naive implementation may require a custom paging implementation instead of using
    //OffsetPageTable from x86_64.
    let mapper = self.mapper.as_mut().ok_or(PageTableError::NotReady)?;
    let frames =
      range.step_by(Size4KiB::SIZE as usize).map(|addr| PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(addr)));
    for frame in frames {
      let map_result = mapper.identity_map(frame, flags, &mut FrameAllocatorWrapper {});
      match map_result {
        Ok(flush_option) => {
          if flush {
            flush_option.flush()
          } else {
            flush_option.ignore()
          }
        }
        Err(err) => {
          match err {
            MapToError::PageAlreadyMapped(_) => (), //do nothing if frame already mapped.
            MapToError::FrameAllocationFailed => panic!("out of memory!"),
            _ => return Err(PageTableError::MapFailure),
          }
        }
      }
    }
    Ok(())
  }
}

struct FrameAllocatorWrapper {}

unsafe impl FrameAllocator<Size4KiB> for FrameAllocatorWrapper {
  /// allocates and returns a 4K page
  fn allocate_frame(&mut self) -> Option<PhysFrame> {
    let frame_size = Size4KiB::SIZE as usize;
    let layout = unsafe { Layout::from_size_align_unchecked(frame_size, frame_size) };
    match Global.allocate(layout) {
      Ok(ptr) => match PhysFrame::from_start_address(PhysAddr::new(ptr.as_ptr() as *const u8 as u64)) {
        Ok(frame) => Some(frame),
        Err(_) => None,
      },
      Err(_) => None,
    }
  }
}
