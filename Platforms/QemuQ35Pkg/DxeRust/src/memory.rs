use alloc::vec::Vec;
use x86_64::{
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, RecursivePageTable, Size4KiB},
    PhysAddr, VirtAddr,
};

use crate::{
    memory_region::{MemoryRegion, MemoryRegionKind},
    println,
};

/// Unset page protection bit in CR0
///
/// Page protection might be enabled in PEI, so we want to disable that
/// before to the first allocation.
pub unsafe fn disable_page_protection() {
    use x86_64::registers::control::{Cr0, Cr0Flags};

    let mut flags = Cr0::read();
    flags -= Cr0Flags::WRITE_PROTECT;
    Cr0::write(flags);
}

/// Initialize a new OffsetPageTable.
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
pub unsafe fn init_offset(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

/// Initialize a new RecursivePageTable.
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
pub unsafe fn init_recursive(physical_memory_offset: VirtAddr) -> RecursivePageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    //println!("L4 table: {:?}", level_4_table);
    RecursivePageTable::new(level_4_table).expect("Cannot create RecursivePageTable")
}

/// Returns a mutable reference to the active level 4 table.
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    &mut *page_table_ptr // unsafe
}

/// A FrameAllocator that always returns `None`.
pub struct EmptyFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for EmptyFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        None
    }
}

/// A FrameAllocator that returns usable frames from a hard coded region.
pub struct HardCodedFrameAllocator {
    hardcoded_map: &'static [MemoryRegion],
    next: usize,
}

impl HardCodedFrameAllocator {
    /// Create a FrameAllocator from the passed memory map.
    ///
    /// This function is unsafe because the caller must guarantee that the passed
    /// memory map is valid. The main requirement is that all frames that are marked
    /// as `USABLE` in it are really unused.
    pub unsafe fn init(hardcoded_map: &'static [MemoryRegion]) -> Self {
        HardCodedFrameAllocator { hardcoded_map, next: 0 }
    }

    /// Returns an iterator over the usable frames specified in the memory map.
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        // get usable regions from memory map
        let regions = self.hardcoded_map.iter();
        let usable_regions = regions.filter(|r| r.kind == MemoryRegionKind::Usable);
        // map each region to its address range
        let addr_ranges = usable_regions.map(|r| r.start..r.end);
        // transform to an iterator of frame start addresses
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(4096));
        // create `PhysFrame` types from the start addresses
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }

    pub fn print_frames(&self) {
        println!("HardCodedFrameAllocator usable frames:");
        for frame in self.usable_frames() {
            println!("frame: {:?}", frame);
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for HardCodedFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}

// A FrameAllocator that returns frames from a pool of frames.
// New memory regions for use by the frame allocator can be added dynamically.
const MAX_FIXED_REGIONS: usize = 4;
pub struct DynamicFrameAllocator {
    fixed_regions: [MemoryRegion; MAX_FIXED_REGIONS], //used before heap.
    dyn_regions: Option<Vec<MemoryRegion>>,           //used after heap.
    map_count: usize,
    next: usize,
}

impl DynamicFrameAllocator {
    pub fn init() -> Self {
        DynamicFrameAllocator {
            fixed_regions: [MemoryRegion::empty(); MAX_FIXED_REGIONS],
            dyn_regions: None,
            map_count: 0,
            next: 0,
        }
    }

    /// This function is unsafe because the caller must guarantee that the passed
    /// memory map is valid. The main requirement is that all frames that are marked
    /// as `USABLE` in it are really unused.
    pub unsafe fn add_region(&mut self, region: MemoryRegion) {
        // the first MAX_FIXED_REGIONS can be added without requiring dynamic allocation - these can be used to
        // initialize the heap. After that, new regions are added to a Vector which can grow to arbitrary size.
        //
        // Hackathon Note: this is sort of useless at this point because the heap is allocated from the first added
        // region and never increases in size, so regions added after that are never used (i.e. allocate_frame() will
        // never be called enough times to get into ranges beyond the first). A future enhancement might be to
        // dynamically grow the heap if it is over-allocated.
        if self.map_count < MAX_FIXED_REGIONS {
            println!("Adding {:?} as fixed region.", region);
            self.fixed_regions[self.map_count] = region.clone();
        } else {
            println!("Adding {:?} as dynamic region.", region);
            self.dyn_regions.get_or_insert(Vec::new()).push(region.clone());
        }
        self.map_count += 1;
    }

    /// Returns an iterator over the usable frames specified in the memory maps.
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> + '_ {
        // get usable regions from memory map
        let mut dyn_regions_iter = [MemoryRegion::empty(); 0].iter();
        if let Some(regions) = &self.dyn_regions {
            dyn_regions_iter = regions.iter();
        }
        let all_regions = self.fixed_regions.iter().chain(dyn_regions_iter);

        all_regions
            .filter_map(|r|
                // filter unused regions and convert to address range
                if r.kind == MemoryRegionKind::Usable {
                    Some(r.start..r.end)
                } else {
                    None
                }
            )
            .flat_map(|r| r.step_by(4096)) // convert range to frame start addresses within the range
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr))) // convert frame start addresses to PhysFrame and return
    }

    pub fn print_frames(&self) {
        println!("DynamicFrameAllocator usable frames:");
        for frame in self.usable_frames() {
            println!("frame: {:?}", frame);
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for DynamicFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}
