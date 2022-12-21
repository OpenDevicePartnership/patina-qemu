use core::{alloc::Allocator, alloc::Layout, ffi::c_void, ptr::NonNull};

use crate::utility::Locked;
use fixed_size_block::FixedSizeBlockAllocator;
use r_efi::{
    efi::Status,
    eficall, eficall_abi,
    system::{
        BootServices, MemoryType, ACPI_MEMORY_NVS, ACPI_RECLAIM_MEMORY, BOOT_SERVICES_CODE, BOOT_SERVICES_DATA,
        LOADER_CODE, LOADER_DATA, RUNTIME_SERVICES_CODE, RUNTIME_SERVICES_DATA,
    },
};

pub mod fixed_size_block;

//EfiReservedMemoryType - no allocator (unused).
//EfiLoaderCode
pub static EFI_LOADER_CODE_ALLOCATOR: Locked<FixedSizeBlockAllocator> = Locked::new(FixedSizeBlockAllocator::new());
//EfiLoaderData
pub static EFI_LOADER_DATA_ALLOCATOR: Locked<FixedSizeBlockAllocator> = Locked::new(FixedSizeBlockAllocator::new());
//EfiBootServicesCode
pub static EFI_BOOT_SERVICES_CODE_ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());
//EfiBootServicesData - (default allocator for DxeRust)
#[cfg_attr(not(test), global_allocator)]
pub static EFI_BOOT_SERVICES_DATA_ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());
//EfiRuntimeServicesCode
pub static EFI_RUNTIME_SERVICES_CODE_ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());
//EfiRuntimeServicesData
pub static EFI_RUNTIME_SERVICES_DATA_ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());
//EfiConventionalMemory - no allocator (free memory)
//EfiUnusableMemory - no allocator (unusable)
//EfiACPIReclaimMemory
pub static EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());
//EfiACPIMemoryNVS
pub static EFI_ACPI_MEMORY_NVS_ALLOCATOR: Locked<FixedSizeBlockAllocator> = Locked::new(FixedSizeBlockAllocator::new());
//EFiMemoryMappedIo - no allocator (MMIO)
//EFiMemoryMappedIOPortSpace - no allocator (MMIO)
//EfiPalCode - no allocator (no Itanium support)
//EfiPersistentMemory - no allocator (free memory)

static ALL_ALLOCATORS: &[&Locked<FixedSizeBlockAllocator>] = &[
    &EFI_LOADER_CODE_ALLOCATOR,
    &EFI_LOADER_DATA_ALLOCATOR,
    &EFI_BOOT_SERVICES_CODE_ALLOCATOR,
    &EFI_BOOT_SERVICES_DATA_ALLOCATOR,
    &EFI_RUNTIME_SERVICES_CODE_ALLOCATOR,
    &EFI_RUNTIME_SERVICES_DATA_ALLOCATOR,
    &EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR,
    &EFI_ACPI_MEMORY_NVS_ALLOCATOR,
];

pub fn get_allocator_for_type(memory_type: MemoryType) -> Option<&'static Locked<FixedSizeBlockAllocator>> {
    match memory_type {
        LOADER_CODE => Some(&EFI_LOADER_CODE_ALLOCATOR),
        LOADER_DATA => Some(&EFI_LOADER_DATA_ALLOCATOR),
        BOOT_SERVICES_CODE => Some(&EFI_BOOT_SERVICES_CODE_ALLOCATOR),
        BOOT_SERVICES_DATA => Some(&EFI_BOOT_SERVICES_DATA_ALLOCATOR),
        RUNTIME_SERVICES_CODE => Some(&EFI_RUNTIME_SERVICES_CODE_ALLOCATOR),
        RUNTIME_SERVICES_DATA => Some(&EFI_RUNTIME_SERVICES_DATA_ALLOCATOR),
        ACPI_RECLAIM_MEMORY => Some(&EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR),
        ACPI_MEMORY_NVS => Some(&EFI_ACPI_MEMORY_NVS_ALLOCATOR),
        _ => None,
    }
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

const POOL_SIG: u32 = 0x04151980; //arbitrary number.
const UEFI_POOL_ALIGN: usize = 8; //per UEFI spec.
const UEFI_PAGE_SIZE: usize = 0x1000; //per UEFI spec.
struct AllocationInfo {
    signature: u32,
    memory_type: r_efi::system::MemoryType,
    layout: Layout,
}

eficall! {pub fn allocate_pool (pool_type: r_efi::system::MemoryType, size: usize, buffer: *mut *mut c_void) -> Status {
    if buffer == core::ptr::null_mut() {
        return Status::INVALID_PARAMETER;
    }

    let allocator = get_allocator_for_type(pool_type);
    if allocator.is_none() {
        return Status::INVALID_PARAMETER;
    }
    let allocator = allocator.unwrap();

    let mut allocation_info = AllocationInfo {
        signature: POOL_SIG,
        memory_type: pool_type,
        layout: Layout::new::<AllocationInfo>()};

    let offset:usize;
    (allocation_info.layout, offset) = allocation_info.layout
        .extend(
            Layout::from_size_align(size, UEFI_POOL_ALIGN)
                .unwrap_or_else(|err|panic!("Allocation layout error: {:#?}", err))
        ).unwrap_or_else(|err|panic!("Allocation layout error: {:#?}", err));


    match allocator.allocate(allocation_info.layout) {
        Ok(ptr) => {
            let alloc_info_ptr = ptr.as_mut_ptr() as *mut AllocationInfo;
            unsafe {
                alloc_info_ptr.write(allocation_info);
                buffer.write((ptr.as_ptr() as *mut u8 as usize + offset) as *mut c_void);
            }
            Status::SUCCESS
        }
        Err(_) => Status::OUT_OF_RESOURCES
    }
}}

eficall! {fn free_pool (buffer: *mut c_void) -> Status {
    if buffer == core::ptr::null_mut() {
        return Status::INVALID_PARAMETER;
    }
    let (_, offset) = Layout::new::<AllocationInfo>()
        .extend(
            Layout::from_size_align(0, UEFI_POOL_ALIGN)
                .unwrap_or_else(|err|panic!("Allocation layout error: {:#?}", err))
        ).unwrap_or_else(|err|panic!("Allocation layout error: {:#?}", err));

    let allocation_info: *mut AllocationInfo = ((buffer as usize) - offset) as *mut AllocationInfo;

    unsafe {
        //must be true for any pool allocation
        assert!((*allocation_info).signature == POOL_SIG);
        //zero after check so it doesn't get reused.
        (*allocation_info).signature = 0;
        //must exist for any real pool allocation
        let allocator = get_allocator_for_type((*allocation_info).memory_type).unwrap();
        allocator.deallocate(NonNull::new(allocation_info as *mut u8).unwrap(), (*allocation_info).layout);
    }

    Status::SUCCESS
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
