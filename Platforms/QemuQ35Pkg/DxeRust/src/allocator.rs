use core::{
    alloc::{Allocator, Layout},
    ffi::c_void,
    ptr::NonNull,
};

use crate::FRAME_ALLOCATOR;
use r_efi::{
    efi::Status,
    eficall, eficall_abi,
    system::{
        BootServices, MemoryType, ACPI_MEMORY_NVS, ACPI_RECLAIM_MEMORY, BOOT_SERVICES_CODE, BOOT_SERVICES_DATA,
        LOADER_CODE, LOADER_DATA, RUNTIME_SERVICES_CODE, RUNTIME_SERVICES_DATA,
    },
};
use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;

//EfiReservedMemoryType - no allocator (unused).
//EfiLoaderCode
pub static EFI_LOADER_CODE_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, LOADER_CODE);
//EfiLoaderData
pub static EFI_LOADER_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, LOADER_DATA);
//EfiBootServicesCode
pub static EFI_BOOT_SERVICES_CODE_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, BOOT_SERVICES_CODE);
//EfiBootServicesData - (default allocator for DxeRust)
#[cfg_attr(not(test), global_allocator)]
pub static EFI_BOOT_SERVICES_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, BOOT_SERVICES_DATA);
//EfiRuntimeServicesCode
pub static EFI_RUNTIME_SERVICES_CODE_ALLOCATOR: UefiAllocator =
    UefiAllocator::new(&FRAME_ALLOCATOR, RUNTIME_SERVICES_CODE);
//EfiRuntimeServicesData
pub static EFI_RUNTIME_SERVICES_DATA_ALLOCATOR: UefiAllocator =
    UefiAllocator::new(&FRAME_ALLOCATOR, RUNTIME_SERVICES_DATA);
//EfiConventionalMemory - no allocator (free memory)
//EfiUnusableMemory - no allocator (unusable)
//EfiACPIReclaimMemory
pub static EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, ACPI_RECLAIM_MEMORY);
//EfiACPIMemoryNVS
pub static EFI_ACPI_MEMORY_NVS_ALLOCATOR: UefiAllocator = UefiAllocator::new(&FRAME_ALLOCATOR, ACPI_MEMORY_NVS);
//EFiMemoryMappedIo - no allocator (MMIO)
//EFiMemoryMappedIOPortSpace - no allocator (MMIO)
//EfiPalCode - no allocator (no Itanium support)
//EfiPersistentMemory - no allocator (free memory)

pub static ALL_ALLOCATORS: &[&'static UefiAllocator] = &[
    &EFI_LOADER_CODE_ALLOCATOR,
    &EFI_LOADER_DATA_ALLOCATOR,
    &EFI_BOOT_SERVICES_CODE_ALLOCATOR,
    &EFI_BOOT_SERVICES_DATA_ALLOCATOR,
    &EFI_RUNTIME_SERVICES_CODE_ALLOCATOR,
    &EFI_RUNTIME_SERVICES_DATA_ALLOCATOR,
    &EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR,
    &EFI_ACPI_MEMORY_NVS_ALLOCATOR,
];

pub fn get_allocator_for_type(memory_type: MemoryType) -> Option<&'static &'static UefiAllocator> {
    ALL_ALLOCATORS.iter().find(|&&x| x.memory_type() == memory_type)
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

const UEFI_PAGE_SIZE: usize = 0x1000; //per UEFI spec.

eficall! {pub fn allocate_pool (pool_type: r_efi::system::MemoryType, size: usize, buffer: *mut *mut c_void) -> Status {
    if buffer == core::ptr::null_mut() {
        return Status::INVALID_PARAMETER;
    }

    let allocator = get_allocator_for_type(pool_type);
    if allocator.is_none() {
        return Status::INVALID_PARAMETER;
    }
    let allocator = allocator.unwrap();

    allocator.allocate_pool(size, buffer)

}}

eficall! {fn free_pool (buffer: *mut c_void) -> Status {
    if buffer == core::ptr::null_mut() {
        return Status::INVALID_PARAMETER;
    }
    unsafe {
        if ALL_ALLOCATORS.iter().find(|allocator| allocator.free_pool(buffer) != Status::NOT_FOUND).is_some() {
            Status::SUCCESS
        } else {
            Status::INVALID_PARAMETER
        }
    }
}}

eficall! {fn allocate_pages (allocation_type: r_efi::system::AllocateType, memory_type: r_efi::system::MemoryType, pages: usize, memory: *mut r_efi::efi::PhysicalAddress) -> Status {

    if memory == core::ptr::null_mut() {
        return Status::INVALID_PARAMETER;
    }

    //TODO: For now we only support "AnyPages" allocation type.
    if allocation_type != r_efi::system::ALLOCATE_ANY_PAGES {
        return Status::UNSUPPORTED;
    }

    let allocator = match get_allocator_for_type(memory_type) {
        Some(allocator) => allocator,
        None => return Status::INVALID_PARAMETER
    };

    let layout = match Layout::from_size_align(pages * UEFI_PAGE_SIZE, UEFI_PAGE_SIZE) {
        Ok(layout) => layout,
        Err(_) => return Status::INVALID_PARAMETER
    };

    match allocator.allocate(layout) {
        Ok(ptr) => {
            unsafe {memory.write (ptr.as_ptr() as *mut u8 as u64)}
            Status::SUCCESS
        }
        Err(_) => Status::OUT_OF_RESOURCES
    }
}}

eficall! {fn free_pages (memory:r_efi::efi::PhysicalAddress, pages:usize) -> Status {

    let size = match pages.checked_mul(UEFI_PAGE_SIZE) {
        Some(size) => size,
        None => return Status::INVALID_PARAMETER
    };

    if (memory as u64).checked_add(size as u64).is_none() {
        return Status::INVALID_PARAMETER;
    }

    let layout = match Layout::from_size_align(size, UEFI_PAGE_SIZE) {
        Ok(layout) => layout,
        Err(_) => return Status::INVALID_PARAMETER
    };

    let address = match NonNull::new(memory as usize as *mut u8) {
        Some(address) => address,
        None => return Status::INVALID_PARAMETER
    };

    match ALL_ALLOCATORS.iter().find(|x|x.contains(address)) {
        Some(allocator) => {
            unsafe {allocator.deallocate(address, layout)};
            Status::SUCCESS
        },
        None => Status::NOT_FOUND
    }
}}

pub fn init_memory_support(bs: &mut BootServices) {
    bs.allocate_pages = allocate_pages;
    bs.free_pages = free_pages;
    bs.allocate_pool = allocate_pool;
    bs.free_pool = free_pool;
}
