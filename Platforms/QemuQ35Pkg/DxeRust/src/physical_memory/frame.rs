//! # Frame
//! Provides support structures for managing physical frames of memory.
use core::ops::Add;

pub const FRAME_SIZE: u64 = 0x1000;
const FRAME_SHIFT: u64 = 12;

/// Represents a 64-bit physical address
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PhyAddr(u64);

impl PhyAddr {
    /// creates a new instance of PhyAddr from a u64 address value.
    pub fn new(addr: u64) -> Self {
        PhyAddr(addr)
    }

    /// aligns the PhyAddr down to the next lowest boundary represented by align.
    pub fn align_down<U>(self, align: U) -> Self
    where
        U: Into<u64>,
    {
        PhyAddr(align_down(self.0, align.into()))
    }

    /// aligns the PhyAddr up to the next highest boundary represented by align.
    pub fn align_up<U>(self, align: U) -> Self
    where
        U: Into<u64>,
    {
        PhyAddr::new(align_up(self.0, align.into()))
    }

    /// returns the PhyAddr as a raw u64
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl Add<u64> for PhyAddr {
    type Output = Self;
    #[inline]
    fn add(self, rhs: u64) -> Self::Output {
        PhyAddr::new(self.0 + rhs)
    }
}

/// Represents a generic physical 4K Frame.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Frame {
    start_address: PhyAddr,
}

impl Frame {
    pub const fn empty() -> Self {
        Frame { start_address: PhyAddr(0) }
    }
    pub fn containing_addr(addr: PhyAddr) -> Self {
        Frame { start_address: addr.align_down(FRAME_SIZE) }
    }
    pub fn start_addr(self) -> PhyAddr {
        self.start_address
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FrameRange {
    first_frame: Frame,
    frame_count: u64,
}

impl FrameRange {
    pub const fn empty() -> Self {
        FrameRange { first_frame: Frame::empty(), frame_count: 0 }
    }
    /// creates a new instance of FrameRange from the specified base and frame count.
    pub fn from_base_and_count(base: PhyAddr, count: u64) -> Self {
        FrameRange { first_frame: Frame::containing_addr(base), frame_count: count }
    }

    /// creates a new instance of FrameRange from the specified base and size (in bytes).
    pub fn from_base_and_size(base: PhyAddr, size: u64) -> Self {
        FrameRange { first_frame: Frame::containing_addr(base), frame_count: size >> FRAME_SHIFT }
    }

    /// returns the frame count for this range.
    pub fn frame_count(self) -> u64 {
        self.frame_count
    }

    /// returns the current size (in bytes) for this range
    pub fn frame_range_size(self) -> u64 {
        self.frame_count << FRAME_SHIFT
    }

    /// returns the start address for this range
    pub fn start_addr(self) -> PhyAddr {
        self.first_frame.start_addr()
    }
}

pub const fn size_to_pages(size: u64) -> u64 {
    align_up(size, FRAME_SIZE) >> FRAME_SHIFT
}

pub const fn frame_count_to_size(count: u64) -> u64 {
    count << FRAME_SHIFT
}

const fn align_down(value: u64, align: u64) -> u64 {
    assert!(align.is_power_of_two(), "`align` must be power of two");
    value & !(align - 1)
}

const fn align_up(value: u64, align: u64) -> u64 {
    assert!(align.is_power_of_two(), "`align` must be power of two");
    let align_mask = align - 1;
    if value & align_mask == 0 {
        value // already aligned
    } else {
        (value | align_mask).checked_add(1).expect("attempt to add with overflow")
    }
}
