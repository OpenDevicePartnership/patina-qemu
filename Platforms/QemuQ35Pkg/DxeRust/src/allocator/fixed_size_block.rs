use crate::physical_memory::FRAME_ALLOCATOR;

use super::Locked;
use alloc::alloc::{GlobalAlloc, Layout};
use core::{
    cmp::max,
    fmt::{self, Display},
    mem::{self, align_of, size_of},
    ptr::{self, NonNull},
};
use linked_list_allocator::{align_down, align_up};

pub const MIN_EXPANSION: usize = 0x100000;

pub enum AllocationError {
    FrameAllocatorError,
}

/// The block sizes to use.
///
/// The sizes must each be power of 2 because they are also used as
/// the block alignment (alignments must be always powers of 2).
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Choose an appropriate block size for the given layout.
///
/// Returns an index into the `BLOCK_SIZES` array.
fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

struct ListNode {
    next: Option<&'static mut ListNode>,
}

struct AllocatorListNode {
    next: Option<*mut AllocatorListNode>,
    allocator: linked_list_allocator::Heap,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    allocators: Option<*mut AllocatorListNode>,
}

// Safety: users of FixedSizeBlockAllocator are expected to use a locked
// instance of the allocator if it will be accessible across threads.
unsafe impl Send for FixedSizeBlockAllocator {}
unsafe impl Sync for FixedSizeBlockAllocator {}

impl FixedSizeBlockAllocator {
    /// Creates an empty FixedSizeBlockAllocator.
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator { list_heads: [EMPTY; BLOCK_SIZES.len()], allocators: None }
    }

    fn expand(&mut self, size: usize) -> Result<(), AllocationError> {
        // claim some frames from the global FRAME allocator.
        let memory_range = FRAME_ALLOCATOR
            .lock()
            .allocate_frame_range_from_size(size as u64)
            .map_err(|_| AllocationError::FrameAllocatorError)?;

        // set up the allocator, reserving space at the beginning of the range
        // for the AllocatorListNode structure.
        let aligned_node_addr = align_up(memory_range.start_addr().as_u64() as usize, align_of::<AllocatorListNode>());
        let heap_bottom = aligned_node_addr + size_of::<AllocatorListNode>();
        let heap_size =
            memory_range.frame_range_size() as usize - (heap_bottom - memory_range.start_addr().as_u64() as usize);
        let node_ptr = aligned_node_addr as *mut AllocatorListNode;
        let node = AllocatorListNode { next: None, allocator: linked_list_allocator::Heap::empty() };

        unsafe {
            node_ptr.write(node);
            (*node_ptr).allocator.init(heap_bottom as usize, heap_size as usize);
            // Insert the new allocator to the beginning of the allocator list.
            (*node_ptr).next = self.allocators;
        }

        self.allocators = Some(node_ptr);
        Ok(())
    }

    /// Allocates using the fallback allocators.
    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        for node in AllocatorIterator::new(self.allocators) {
            let allocator = unsafe { &mut (*node).allocator };
            if let Ok(ptr) = allocator.allocate_first_fit(layout) {
                return ptr.as_ptr();
            }
        }
        //if we get here, then allocation failed in all current allocation ranges.
        //attempt to expand and then allocate again.
        let expand_size =
            layout.align() + layout.size() + align_of::<AllocatorListNode>() + size_of::<AllocatorListNode>();
        if let Err(_) = self.expand(max(expand_size, MIN_EXPANSION)) {
            return ptr::null_mut();
        }
        return self.fallback_alloc(layout);
    }

    fn fallback_dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let ptr = NonNull::new(ptr).unwrap();
        let address = ptr.as_ptr() as usize;
        for node in AllocatorIterator::new(self.allocators) {
            let allocator = unsafe { &mut (*node).allocator };
            if (allocator.bottom() <= address) && (address < allocator.top()) {
                unsafe { allocator.deallocate(ptr, layout) };
            }
        }
    }
}

impl Display for Locked<FixedSizeBlockAllocator> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fsb = self.lock();
        write!(f, "Allocation Ranges:\n")?;
        for node in AllocatorIterator::new(fsb.allocators) {
            let allocator = unsafe { &mut (*node).allocator };
            write!(
                f,
                "  PhysRange: {:#x}-{:#x}, Size: {:#x}, Used: {:#x} Free: {:#x} Maint: {:#x}\n",
                align_down(allocator.bottom(), 0x1000), //account for AllocatorListNode
                allocator.top(),
                align_up(allocator.size(), 0x1000), //account for AllocatorListNode
                allocator.used(),
                allocator.free(),
                align_up(allocator.size(), 0x100) - allocator.size()
            )?;
        }
        Ok(())
    }
}

struct AllocatorIterator {
    current: Option<*mut AllocatorListNode>,
}

impl AllocatorIterator {
    fn new(start_node: Option<*mut AllocatorListNode>) -> Self {
        AllocatorIterator { current: start_node }
    }
}

impl Iterator for AllocatorIterator {
    type Item = *mut AllocatorListNode;
    fn next(&mut self) -> Option<*mut AllocatorListNode> {
        if let Some(current) = self.current {
            self.current = unsafe { (*current).next };
            Some(current)
        } else {
            None
        }
    }
}

unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                match allocator.list_heads[index].take() {
                    Some(node) => {
                        allocator.list_heads[index] = node.next.take();
                        node as *mut ListNode as *mut u8
                    }
                    None => {
                        // no block exists in list => allocate new block
                        let block_size = BLOCK_SIZES[index];
                        // only works if all block sizes are a power of 2
                        let block_align = block_size;
                        let layout = Layout::from_size_align(block_size, block_align).unwrap();
                        allocator.fallback_alloc(layout)
                    }
                }
            }
            None => allocator.fallback_alloc(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                let new_node = ListNode { next: allocator.list_heads[index].take() };
                // verify that block has size and alignment required for storing node
                assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node_ptr = ptr as *mut ListNode;
                new_node_ptr.write(new_node);
                allocator.list_heads[index] = Some(&mut *new_node_ptr);
            }
            None => {
                allocator.fallback_dealloc(ptr, layout);
            }
        }
    }
}
