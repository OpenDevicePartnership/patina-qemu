use core::{
  alloc::{Allocator, Layout},
  ffi::c_void,
  ptr::NonNull,
  slice::{from_raw_parts, from_raw_parts_mut},
};

use crate::GCD;
use r_efi::{
  efi::Status,
  system::{
    BootServices, MemoryType, ACPI_MEMORY_NVS, ACPI_RECLAIM_MEMORY, ALLOCATE_ADDRESS, ALLOCATE_ANY_PAGES,
    ALLOCATE_MAX_ADDRESS, BOOT_SERVICES_CODE, BOOT_SERVICES_DATA, LOADER_CODE, LOADER_DATA, RESERVED_MEMORY_TYPE,
    RUNTIME_SERVICES_CODE, RUNTIME_SERVICES_DATA,
  },
};
use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;

//EfiReservedMemoryType
pub static EFI_RESERVED_MEMORY_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, RESERVED_MEMORY_TYPE);
//EfiLoaderCode
pub static EFI_LOADER_CODE_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, LOADER_CODE);
//EfiLoaderData
pub static EFI_LOADER_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, LOADER_DATA);
//EfiBootServicesCode
pub static EFI_BOOT_SERVICES_CODE_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, BOOT_SERVICES_CODE);
//EfiBootServicesData - (default allocator for DxeRust)
#[cfg_attr(not(test), global_allocator)]
pub static EFI_BOOT_SERVICES_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, BOOT_SERVICES_DATA);
//EfiRuntimeServicesCode
pub static EFI_RUNTIME_SERVICES_CODE_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, RUNTIME_SERVICES_CODE);
//EfiRuntimeServicesData
pub static EFI_RUNTIME_SERVICES_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, RUNTIME_SERVICES_DATA);
//EfiConventionalMemory - no allocator (free memory)
//EfiUnusableMemory - no allocator (unusable)
//EfiACPIReclaimMemory
pub static EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, ACPI_RECLAIM_MEMORY);
//EfiACPIMemoryNVS
pub static EFI_ACPI_MEMORY_NVS_ALLOCATOR: UefiAllocator = UefiAllocator::new(&GCD, ACPI_MEMORY_NVS);
//EFiMemoryMappedIo - no allocator (MMIO)
//EFiMemoryMappedIOPortSpace - no allocator (MMIO)
//EfiPalCode - no allocator (no Itanium support)
//EfiPersistentMemory - no allocator (free memory)

pub static ALL_ALLOCATORS: &[&'static UefiAllocator] = &[
  &EFI_RESERVED_MEMORY_ALLOCATOR,
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

pub extern "efiapi" fn allocate_pool(
  pool_type: r_efi::system::MemoryType,
  size: usize,
  buffer: *mut *mut c_void,
) -> Status {
  if buffer == core::ptr::null_mut() {
    return Status::INVALID_PARAMETER;
  }

  let allocator = get_allocator_for_type(pool_type);
  if allocator.is_none() {
    return Status::INVALID_PARAMETER;
  }
  let allocator = allocator.unwrap();

  allocator.allocate_pool(size, buffer)
}

extern "efiapi" fn free_pool(buffer: *mut c_void) -> Status {
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
}

extern "efiapi" fn allocate_pages(
  allocation_type: r_efi::system::AllocateType,
  memory_type: r_efi::system::MemoryType,
  pages: usize,
  memory: *mut r_efi::efi::PhysicalAddress,
) -> Status {
  if memory == core::ptr::null_mut() {
    return Status::INVALID_PARAMETER;
  }

  let allocator = match get_allocator_for_type(memory_type) {
    Some(allocator) => allocator,
    None => return Status::INVALID_PARAMETER,
  };

  let layout = match Layout::from_size_align(pages * UEFI_PAGE_SIZE, UEFI_PAGE_SIZE) {
    Ok(layout) => layout,
    Err(_) => return Status::INVALID_PARAMETER,
  };

  match allocation_type {
    ALLOCATE_ANY_PAGES => match allocator.allocate(layout) {
      Ok(ptr) => {
        unsafe { memory.write(ptr.as_ptr() as *mut u8 as u64) }
        Status::SUCCESS
      }
      Err(_) => Status::OUT_OF_RESOURCES,
    },
    ALLOCATE_MAX_ADDRESS => {
      if let Some(address) = unsafe { memory.as_ref() } {
        match unsafe { allocator.allocate_below_address(layout, *address) } {
          Ok(ptr) => {
            unsafe { memory.write(ptr.as_ptr() as *mut u8 as u64) }
            Status::SUCCESS
          }
          Err(_) => Status::OUT_OF_RESOURCES,
        }
      } else {
        Status::INVALID_PARAMETER
      }
    }
    ALLOCATE_ADDRESS => {
      if let Some(address) = unsafe { memory.as_ref() } {
        match unsafe { allocator.allocate_at_address(layout, *address) } {
          Ok(ptr) => {
            unsafe { memory.write(ptr.as_ptr() as *mut u8 as u64) }
            Status::SUCCESS
          }
          Err(_) => Status::OUT_OF_RESOURCES,
        }
      } else {
        Status::INVALID_PARAMETER
      }
    }
    _ => Status::UNSUPPORTED,
  }
}

extern "efiapi" fn free_pages(memory: r_efi::efi::PhysicalAddress, pages: usize) -> Status {
  let size = match pages.checked_mul(UEFI_PAGE_SIZE) {
    Some(size) => size,
    None => return Status::INVALID_PARAMETER,
  };

  if (memory as u64).checked_add(size as u64).is_none() {
    return Status::INVALID_PARAMETER;
  }

  let layout = match Layout::from_size_align(size, UEFI_PAGE_SIZE) {
    Ok(layout) => layout,
    Err(_) => return Status::INVALID_PARAMETER,
  };

  let address = match NonNull::new(memory as usize as *mut u8) {
    Some(address) => address,
    None => return Status::INVALID_PARAMETER,
  };

  match ALL_ALLOCATORS.iter().find(|x| x.contains(address)) {
    Some(allocator) => {
      unsafe { allocator.deallocate(address, layout) };
      Status::SUCCESS
    }
    None => Status::NOT_FOUND,
  }
}

extern "efiapi" fn copy_mem(destination: *mut c_void, source: *mut c_void, length: usize) {
  //nothing about this is safe.
  unsafe {
    let dst_buffer = from_raw_parts_mut(destination as *mut u8, length);
    let src_buffer = from_raw_parts(source as *mut u8, length);

    dst_buffer.copy_from_slice(src_buffer);
  }
}

extern "efiapi" fn set_mem(buffer: *mut c_void, size: usize, value: u8) {
  //nothing about this is safe.
  unsafe {
    let dst_buffer = from_raw_parts_mut(buffer as *mut u8, size);
    dst_buffer.fill(value);
  }
}

pub fn init_memory_support(bs: &mut BootServices) {
  bs.allocate_pages = allocate_pages;
  bs.free_pages = free_pages;
  bs.allocate_pool = allocate_pool;
  bs.free_pool = free_pool;
  bs.copy_mem = copy_mem;
  bs.set_mem = set_mem;
}
