//! # X86 Paging Support module
//! provides utilities related to x86 paging.

use core::ops::Range;

use x86_64::{
    registers::control::{Cr3, Cr3Flags},
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, OffsetPageTable, PageSize, PageTable, PageTableFlags, PhysFrame,
        Size4KiB,
    },
    PhysAddr, VirtAddr,
};

use crate::{physical_memory, utility::Locked};

pub static PAGE_TABLE: Locked<GlobalPageTable> = Locked::new(GlobalPageTable::empty());

#[derive(Debug)]
pub enum PageTableError {
    NotReady,
    AlreadyStarted,
    InvalidRange,
    MapFailure,
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
    /// Unsafe because it assumes:
    /// 1. Global FRAME_ALLOCATOR is available
    /// 2. FRAME_ALLOCATOR has enough frames required to cover the range
    ///    memory_start..memory_end when setting up the page table,
    /// 3. memory_start..memory_end contains the current stack, heap and dxe rust code
    ///    so that the program is still viable after switching to this page table.
    pub unsafe fn init(&mut self, range: Range<u64>) -> Result<(), PageTableError> {
        if let Some(_mapper) = &self.mapper {
            return Err(PageTableError::AlreadyStarted);
        }
        // allocate a frame from the global allocator for our empty table.
        let page_table_addr = physical_memory::FRAME_ALLOCATOR
            .lock()
            .allocate_frame_range_from_count(1)
            .expect("failed to allocate initial frame for page table.")
            .start_addr()
            .as_u64();

        //init a new offset page table using that frame zero-ed out to indicate nothing present
        let level_4_table = &mut *(page_table_addr as *mut PageTable);
        level_4_table.zero();

        let mapper = OffsetPageTable::new(level_4_table, VirtAddr::new(0));
        self.mapper = Some(mapper);

        //map the provided initial range into the table. do_map() will request additional frames as needed
        //from the global allocator.
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        self.do_map(range, flags, false)?;

        // now that the table is constructed, write Cr3 to activate it.
        let level_4_table_ptr = self.mapper.as_mut().unwrap().level_4_table() as *mut PageTable as u64;
        let level_4_frame = PhysFrame::containing_address(PhysAddr::new(level_4_table_ptr));
        Cr3::write(level_4_frame, Cr3Flags::empty());

        Ok(())
    }

    /// Map a range of memory and set flags as specified.
    ///
    /// Unsafe because it assumes:
    /// 1. Global FRAME_ALLOCATOR is available
    /// 2. FRAME_ALLOCATOR has enough frames to cover the range specified.
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
        let frames = range
            .step_by(Size4KiB::SIZE as usize)
            .map(|addr| PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(addr)));
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
        match physical_memory::FRAME_ALLOCATOR.lock().allocate_frame_range_from_count(1) {
            Ok(frame_range) => {
                let start_addr = PhysAddr::new(frame_range.start_addr().as_u64());
                match PhysFrame::from_start_address(start_addr) {
                    Ok(frame) => Some(frame),
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    }
}
