use r_efi::efi::{
    AllocateType, MemoryType, ALLOCATE_ANY_PAGES, BOOT_SERVICES_DATA, RESERVED_MEMORY_TYPE, RUNTIME_SERVICES_DATA,
};
use x86_64::{
    structures::paging::{mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

//use vga_buffer::println; // For debug

use crate::{
    println,
    uefi_allocator::uefi_linked_list::{LinkedListAllocator, UefiAlloc},
};

mod uefi_linked_list;

pub const HEAP_START: usize = 0x_4444_4454_0000; // Start 1 MiB above rust allocator
pub const HEAP_SIZE: usize = 1000 * 1024; // 1000 KiB

const EFI_PAGE_SIZE: usize = 0x1000;
const EFI_PAGE_SHIFT: usize = 12;
const EFI_PAGE_MASK: usize = 0xFFF;

static UEFI_ALLOCATOR: Locked<LinkedListAllocator> = Locked::new(LinkedListAllocator::new());

pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator.allocate_frame().ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }

    unsafe {
        UEFI_ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}

/// A wrapper around spin::Mutex to permit trait implementations.
pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked { inner: spin::Mutex::new(inner) }
    }

    pub fn lock(&self) -> spin::MutexGuard<A> {
        self.inner.lock()
    }
}

/// Align the given address `addr` upwards to alignment `align`.
///
/// Requires that `align` is a power of two.
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// Internal helper functions

fn efi_size_to_pages(a: usize) -> usize {
    let mut x = a >> EFI_PAGE_SHIFT;
    if (a & EFI_PAGE_MASK) > 0 {
        x = x + 1;
    }
    return x;
}

unsafe fn copy_mem(dest: *mut u8, src: *const u8, len: usize) {
    for i in 0..=len {
        *(dest.wrapping_add(i)) = *(src.wrapping_add(i));
    }
}

unsafe fn set_mem(buffer: *mut u8, len: usize, value: u8) {
    for i in 0..=len {
        *(buffer.wrapping_add(i)) = value;
    }
}

fn smaller_of(old_size: usize, new_size: usize) -> usize {
    // MIN(a,b) (((a)>(b))?(b):(a))
    if old_size > new_size {
        return new_size;
    }
    return old_size;
}

/// Internal functions

fn core_allocate_pages(allocation_type: AllocateType, memory_type: MemoryType, pages: usize) -> usize {
    // allocation_type does not seem to be needed
    println!("core_allocate_pages::AllocateType: {}", allocation_type);
    let allocation_size: usize = pages * EFI_PAGE_SIZE;
    return unsafe { UEFI_ALLOCATOR.uefi_alloc(allocation_size, memory_type, EFI_PAGE_SIZE) as usize };
}

fn core_free_pages(buffer: usize, pages: usize) {
    let size = pages * EFI_PAGE_SIZE;
    let ptr = buffer as *mut u8;
    unsafe {
        UEFI_ALLOCATOR.uefi_dealloc(ptr, size);
    }
    println!("core_free_pages: {:x}", ptr as usize);
}

fn core_allocate_pool(memory_type: MemoryType, allocation_size: usize) -> usize {
    return unsafe { UEFI_ALLOCATOR.uefi_alloc(allocation_size, memory_type, 1) as usize };
}

fn core_free_pool(buffer: usize) {
    let size: usize = 0; // TODO: Need to fix
    let ptr = buffer as *mut u8;
    unsafe {
        UEFI_ALLOCATOR.uefi_dealloc(ptr, size);
    }
}

fn internal_allocate_pages(memory_type: MemoryType, pages: usize, alignment: usize) -> usize {
    if pages == 0 {
        return 0; // NULL in C
    }

    if alignment == 0 {
        return core_allocate_pages(ALLOCATE_ANY_PAGES, memory_type, pages);
    }

    if alignment > EFI_PAGE_SIZE {
        let alignment_mask = alignment - 1;
        let real_pages = pages + efi_size_to_pages(alignment);

        assert!(real_pages > pages, "internal_allocate_pages::real_pages needs to be more than pages");

        let mut memory = core_allocate_pages(ALLOCATE_ANY_PAGES, memory_type, real_pages);

        if memory == 0 {
            assert!(memory != 0, "internal_allocate_pages::memory address of zero returned");
            return memory;
        }

        // Successfully allocated the memory

        let aligned_memory = (memory + alignment_mask) & !alignment_mask;
        let mut unaligned_pages = efi_size_to_pages(aligned_memory - memory);
        if unaligned_pages > 0 {
            //
            // Free first unaligned page(s).
            //
            core_free_pages(memory, unaligned_pages);
        }

        memory = aligned_memory + efi_size_to_pages(pages);
        unaligned_pages = real_pages - pages - unaligned_pages;
        if unaligned_pages > 0 {
            //
            // Free last unaligned page(s).
            //
            core_free_pages(memory, unaligned_pages);
        }
    } else {
        //
        // Do not over-allocate pages in this case.
        //
        return core_allocate_pages(ALLOCATE_ANY_PAGES, memory_type, pages);
    }
    return 0; // NULL in C
}

fn internal_free_pages(buffer: usize, pages: usize) {
    assert!(pages != 0, "internal_free_pages::pages should not be 0");

    core_free_pages(buffer, pages);
}

fn internal_allocate_pool(memory_type: MemoryType, allocation_size: usize) -> usize {
    return core_allocate_pool(memory_type, allocation_size);
}

fn internal_allocate_copy_pool(memory_type: MemoryType, allocation_size: usize, buffer: usize) -> usize {
    let addr = internal_allocate_pool(memory_type, allocation_size);
    if addr > 0 {
        let dest_buffer = addr as usize as *mut u8;
        let src_buffer = buffer as usize as *mut u8;
        unsafe {
            copy_mem(dest_buffer, src_buffer, allocation_size);
        } // Copy buffer contents into new allocation
    }
    return addr; // NULL in C
}

fn internal_allocate_zero_pool(memory_type: MemoryType, allocation_size: usize) -> usize {
    let addr = internal_allocate_pool(memory_type, allocation_size);
    if addr > 0 {
        let buffer = addr as usize as *mut u8;
        unsafe {
            set_mem(buffer, allocation_size, 0);
        } // zero the memory
    }
    return addr; // NULL in C
}

fn internal_reallocate_pool(memory_type: MemoryType, old_size: usize, new_size: usize, _old_buffer: usize) -> usize {
    let new_buffer = internal_allocate_zero_pool(memory_type, new_size);
    if (new_buffer != 0) && (_old_buffer != 0) {
        // CopyMem (NewBuffer, OldBuffer, MIN (OldSize, NewSize));
        let nbuffer = new_buffer as usize as *mut u8;
        let _obuffer = _old_buffer as usize as *mut u8;
        unsafe {
            copy_mem(nbuffer, _obuffer, smaller_of(old_size, new_size));
        }
        free_pool(_old_buffer);
    }
    return 0; // NULL in C
}

fn internal_free_pool(buffer: usize) {
    core_free_pool(buffer);
}

/// Public functions

pub fn allocate_pages(pages: usize) -> usize {
    internal_allocate_pages(BOOT_SERVICES_DATA, pages, 0)
}

pub fn allocate_runtime_pages(pages: usize) -> usize {
    internal_allocate_pages(RUNTIME_SERVICES_DATA, pages, 0)
}
pub fn allocate_reserved_pages(pages: usize) -> usize {
    internal_allocate_pages(RESERVED_MEMORY_TYPE, pages, 0)
}

pub fn free_pages(buffer: usize, pages: usize) {
    internal_free_pages(buffer, pages)
}

pub fn allocate_aligned_pages(pages: usize, alignment: usize) -> usize {
    internal_allocate_pages(BOOT_SERVICES_DATA, pages, alignment)
}

pub fn allocate_aligned_runtime_pages(pages: usize, alignment: usize) -> usize {
    internal_allocate_pages(RUNTIME_SERVICES_DATA, pages, alignment)
}

pub fn allocate_aligned_reserved_pages(pages: usize, alignment: usize) -> usize {
    internal_allocate_pages(RESERVED_MEMORY_TYPE, pages, alignment)
}

pub fn free_aligned_pages(buffer: usize, pages: usize) {
    internal_free_pages(buffer, pages)
}

pub fn allocate_pool(allocation_size: usize) -> usize {
    internal_allocate_pool(BOOT_SERVICES_DATA, allocation_size)
}

pub fn allocate_runtime_pool(allocation_size: usize) -> usize {
    internal_allocate_pool(RUNTIME_SERVICES_DATA, allocation_size)
}
pub fn allocate_reserved_pool(allocation_size: usize) -> usize {
    internal_allocate_pool(RESERVED_MEMORY_TYPE, allocation_size)
}

pub fn allocate_zero_pool(allocation_size: usize) -> usize {
    internal_allocate_zero_pool(BOOT_SERVICES_DATA, allocation_size)
}
pub fn allocate_runtime_zero_pool(allocation_size: usize) -> usize {
    internal_allocate_zero_pool(RUNTIME_SERVICES_DATA, allocation_size)
}
pub fn allocate_reserved_zero_pool(allocation_size: usize) -> usize {
    internal_allocate_zero_pool(RESERVED_MEMORY_TYPE, allocation_size)
}

pub fn allocate_copy_pool(allocation_size: usize, buffer: usize) -> usize {
    internal_allocate_copy_pool(BOOT_SERVICES_DATA, allocation_size, buffer)
}

pub fn allocate_runtime_copy_pool(allocation_size: usize, buffer: usize) -> usize {
    internal_allocate_copy_pool(RUNTIME_SERVICES_DATA, allocation_size, buffer)
}
pub fn allocate_reserved_copy_pool(allocation_size: usize, buffer: usize) -> usize {
    internal_allocate_copy_pool(RESERVED_MEMORY_TYPE, allocation_size, buffer)
}

pub fn reallocate_pool(old_size: usize, new_size: usize, _old_buffer: usize) -> usize {
    internal_reallocate_pool(BOOT_SERVICES_DATA, old_size, new_size, _old_buffer)
}

pub fn reallocate_runtime_pool(old_size: usize, new_size: usize, _old_buffer: usize) -> usize {
    internal_reallocate_pool(RUNTIME_SERVICES_DATA, old_size, new_size, _old_buffer)
}

pub fn reallocate_reserved_pool(old_size: usize, new_size: usize, _old_buffer: usize) -> usize {
    internal_reallocate_pool(RESERVED_MEMORY_TYPE, old_size, new_size, _old_buffer)
}

pub fn free_pool(buffer: usize) {
    internal_free_pool(buffer)
}
