use core::{
  alloc::{Allocator, Layout},
  ffi::c_void,
  mem,
  ptr::NonNull,
  slice::{self, from_raw_parts, from_raw_parts_mut},
};

use alloc::vec::Vec;
use crc32fast::Hasher;

use crate::GCD;
use r_efi::{
  efi::Status,
  system::{
    BootServices, MemoryType, ACPI_MEMORY_NVS, ACPI_RECLAIM_MEMORY, ALLOCATE_ADDRESS, ALLOCATE_ANY_PAGES,
    ALLOCATE_MAX_ADDRESS, BOOT_SERVICES_CODE, BOOT_SERVICES_DATA, CONVENTIONAL_MEMORY, LOADER_CODE, LOADER_DATA,
    MEMORY_MAPPED_IO, RESERVED_MEMORY_TYPE, RUNTIME_SERVICES_CODE, RUNTIME_SERVICES_DATA,
  },
};
use r_pi::dxe_services::{GcdMemoryType, MemorySpaceDescriptor};
use uefi_protocol_db_lib::{
  EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE, EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE,
  EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE, EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE, EFI_LOADER_CODE_ALLOCATOR_HANDLE,
  EFI_LOADER_DATA_ALLOCATOR_HANDLE, EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE,
  EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE, INVALID_HANDLE, RESERVED_MEMORY_ALLOCATOR_HANDLE,
};
use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;

//EfiReservedMemoryType
pub static EFI_RESERVED_MEMORY_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, RESERVED_MEMORY_TYPE, RESERVED_MEMORY_ALLOCATOR_HANDLE);
//EfiLoaderCode
pub static EFI_LOADER_CODE_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, LOADER_CODE, EFI_LOADER_CODE_ALLOCATOR_HANDLE);
//EfiLoaderData
pub static EFI_LOADER_DATA_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, LOADER_DATA, EFI_LOADER_DATA_ALLOCATOR_HANDLE);
//EfiBootServicesCode
pub static EFI_BOOT_SERVICES_CODE_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, BOOT_SERVICES_CODE, EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE);
//EfiBootServicesData - (default allocator for DxeRust)
#[cfg_attr(not(test), global_allocator)]
pub static EFI_BOOT_SERVICES_DATA_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, BOOT_SERVICES_DATA, EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE);
//EfiRuntimeServicesCode
pub static EFI_RUNTIME_SERVICES_CODE_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, RUNTIME_SERVICES_CODE, EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE);
//EfiRuntimeServicesData
pub static EFI_RUNTIME_SERVICES_DATA_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, RUNTIME_SERVICES_DATA, EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE);
//EfiConventionalMemory - no allocator (free memory)
//EfiUnusableMemory - no allocator (unusable)
//EfiACPIReclaimMemory
pub static EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, ACPI_RECLAIM_MEMORY, EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE);
//EfiACPIMemoryNVS
pub static EFI_ACPI_MEMORY_NVS_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, ACPI_MEMORY_NVS, EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE);
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

extern "efiapi" fn get_memory_map(
  memory_map_size: *mut usize,
  memory_map: *mut r_efi::system::MemoryDescriptor,
  map_key: *mut usize,
  descriptor_size: *mut usize,
  descriptor_version: *mut u32,
) -> Status {
  if memory_map_size.is_null() {
    return Status::INVALID_PARAMETER;
  }

  if !descriptor_size.is_null() {
    unsafe { descriptor_size.write(mem::size_of::<r_efi::system::MemoryDescriptor>()) };
  }

  if !descriptor_version.is_null() {
    unsafe { descriptor_version.write(r_efi::system::MEMORY_DESCRIPTOR_VERSION) };
  }

  //allocate an empty vector with enough space for all the descriptors with some padding (in the event)
  //that extra descriptors come into being after creation but before usage.
  let mut descriptors: Vec<MemorySpaceDescriptor> = Vec::with_capacity(GCD.memory_descriptor_count() + 10);
  GCD.get_memory_descriptors(&mut descriptors).expect("get_memory_descriptors failed.");

  let map_size = unsafe { *memory_map_size };

  let efi_descriptors: Vec<r_efi::system::MemoryDescriptor> = descriptors
    .iter()
    .filter_map(|descriptor| {
      let memory_type = match descriptor.image_handle {
        RESERVED_MEMORY_ALLOCATOR_HANDLE => RESERVED_MEMORY_TYPE,
        EFI_LOADER_CODE_ALLOCATOR_HANDLE => LOADER_CODE,
        EFI_LOADER_DATA_ALLOCATOR_HANDLE => LOADER_DATA,
        EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE => BOOT_SERVICES_CODE,
        EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE => BOOT_SERVICES_DATA,
        EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE => RUNTIME_SERVICES_CODE,
        EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE => RUNTIME_SERVICES_DATA,
        EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE => ACPI_RECLAIM_MEMORY,
        EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE => ACPI_MEMORY_NVS,
        INVALID_HANDLE if descriptor.memory_type == GcdMemoryType::SystemMemory => CONVENTIONAL_MEMORY,
        _ if descriptor.memory_type == GcdMemoryType::MemoryMappedIo => MEMORY_MAPPED_IO,
        _ => return None, //Not a type of memory to go in the EFI system map.
      };

      let number_of_pages = descriptor.length >> 12;
      if number_of_pages == 0 {
        return None; //skip entries for things smaller than a page.
      }
      if (descriptor.base_address % 0x1000) != 0 {
        return None; //skip entries not page aligned.
      }
      Some(r_efi::system::MemoryDescriptor {
        r#type: memory_type,
        physical_start: descriptor.base_address,
        virtual_start: descriptor.base_address,
        number_of_pages: number_of_pages,
        attribute: descriptor.attributes,
      })
    })
    .collect();

  assert_ne!(efi_descriptors.len(), 0);

  let required_map_size = efi_descriptors.len() * mem::size_of::<r_efi::system::MemoryDescriptor>();

  unsafe { memory_map_size.write(required_map_size) };

  if map_size < required_map_size {
    return Status::BUFFER_TOO_SMALL;
  }

  let mut hash = Hasher::new();

  unsafe {
    slice::from_raw_parts_mut(memory_map, efi_descriptors.len()).copy_from_slice(&efi_descriptors);

    if !map_key.is_null() {
      let memory_map_as_bytes = slice::from_raw_parts(memory_map as *mut u8, required_map_size);
      hash.update(memory_map_as_bytes);
      map_key.write(hash.finalize() as usize);
    }
  }

  Status::SUCCESS
}

pub fn init_memory_support(bs: &mut BootServices) {
  bs.allocate_pages = allocate_pages;
  bs.free_pages = free_pages;
  bs.allocate_pool = allocate_pool;
  bs.free_pool = free_pool;
  bs.copy_mem = copy_mem;
  bs.set_mem = set_mem;
  bs.get_memory_map = get_memory_map;
}
