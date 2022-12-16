use crate::utility::Locked;
use fixed_size_block::FixedSizeBlockAllocator;
use r_efi::system::{
    MemoryType, ACPI_MEMORY_NVS, ACPI_RECLAIM_MEMORY, BOOT_SERVICES_CODE, BOOT_SERVICES_DATA, LOADER_CODE, LOADER_DATA,
    RUNTIME_SERVICES_CODE, RUNTIME_SERVICES_DATA,
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