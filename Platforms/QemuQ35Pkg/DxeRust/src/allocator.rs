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
use r_efi::efi;
use r_pi::dxe_services::{GcdMemoryType, MemorySpaceDescriptor};
use uefi_protocol_db_lib;
use uefi_rust_allocator_lib::uefi_allocator::UefiAllocator;

//EfiReservedMemoryType
pub static EFI_RESERVED_MEMORY_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::RESERVED_MEMORY_TYPE, uefi_protocol_db_lib::RESERVED_MEMORY_ALLOCATOR_HANDLE);
//EfiLoaderCode
pub static EFI_LOADER_CODE_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::LOADER_CODE, uefi_protocol_db_lib::EFI_LOADER_CODE_ALLOCATOR_HANDLE);
//EfiLoaderData
pub static EFI_LOADER_DATA_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::LOADER_DATA, uefi_protocol_db_lib::EFI_LOADER_DATA_ALLOCATOR_HANDLE);
//EfiBootServicesCode
pub static EFI_BOOT_SERVICES_CODE_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::BOOT_SERVICES_CODE, uefi_protocol_db_lib::EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE);
//EfiBootServicesData - (default allocator for DxeRust)
#[cfg_attr(not(test), global_allocator)]
pub static EFI_BOOT_SERVICES_DATA_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, uefi_protocol_db_lib::EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE);
//EfiRuntimeServicesCode
pub static EFI_RUNTIME_SERVICES_CODE_ALLOCATOR: UefiAllocator = UefiAllocator::new(
  &GCD,
  efi::RUNTIME_SERVICES_CODE,
  uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE,
);
//EfiRuntimeServicesData
pub static EFI_RUNTIME_SERVICES_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(
  &GCD,
  efi::RUNTIME_SERVICES_DATA,
  uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE,
);
//EfiConventionalMemory - no allocator (free memory)
//EfiUnusableMemory - no allocator (unusable)
//EfiACPIReclaimMemory
pub static EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::ACPI_RECLAIM_MEMORY, uefi_protocol_db_lib::EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE);
//EfiACPIMemoryNVS
pub static EFI_ACPI_MEMORY_NVS_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::ACPI_MEMORY_NVS, uefi_protocol_db_lib::EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE);
//EFiMemoryMappedIo - no allocator (MMIO)
//EFiMemoryMappedIOPortSpace - no allocator (MMIO)
//EfiPalCode - no allocator (no Itanium support)
//EfiPersistentMemory - no allocator (free memory)

pub static ALL_ALLOCATORS: &[&UefiAllocator] = &[
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

pub fn get_allocator_for_type(memory_type: efi::MemoryType) -> Option<&'static &'static UefiAllocator> {
  ALL_ALLOCATORS.iter().find(|&&x| x.memory_type() == memory_type)
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
  panic!("allocation error: {:?}", layout)
}

const UEFI_PAGE_SIZE: usize = 0x1000; //per UEFI spec.

extern "efiapi" fn allocate_pool(pool_type: efi::MemoryType, size: usize, buffer: *mut *mut c_void) -> efi::Status {
  if buffer.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }

  match core_allocate_pool(pool_type, size) {
    Err(err) => err,
    Ok(allocation) => unsafe {
      buffer.write(allocation);
      efi::Status::SUCCESS
    },
  }
}

pub fn core_allocate_pool(pool_type: efi::MemoryType, size: usize) -> Result<*mut c_void, efi::Status> {
  if let Some(allocator) = get_allocator_for_type(pool_type) {
    let mut buffer: *mut c_void = core::ptr::null_mut();
    let status = unsafe { allocator.allocate_pool(size, core::ptr::addr_of_mut!(buffer)) };
    if status == efi::Status::SUCCESS {
      Ok(buffer)
    } else {
      Err(status)
    }
  } else {
    Err(efi::Status::INVALID_PARAMETER)
  }
}

extern "efiapi" fn free_pool(buffer: *mut c_void) -> efi::Status {
  if buffer.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }
  unsafe {
    if ALL_ALLOCATORS.iter().any(|allocator| allocator.free_pool(buffer) != efi::Status::NOT_FOUND) {
      efi::Status::SUCCESS
    } else {
      efi::Status::INVALID_PARAMETER
    }
  }
}

extern "efiapi" fn allocate_pages(
  allocation_type: efi::AllocateType,
  memory_type: efi::MemoryType,
  pages: usize,
  memory: *mut efi::PhysicalAddress,
) -> efi::Status {
  if memory.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }

  let allocator = match get_allocator_for_type(memory_type) {
    Some(allocator) => allocator,
    None => return efi::Status::INVALID_PARAMETER,
  };

  let layout = match Layout::from_size_align(pages * UEFI_PAGE_SIZE, UEFI_PAGE_SIZE) {
    Ok(layout) => layout,
    Err(_) => return efi::Status::INVALID_PARAMETER,
  };

  match allocation_type {
    efi::ALLOCATE_ANY_PAGES => match allocator.allocate_any_address(layout) {
      Ok(ptr) => {
        unsafe { memory.write(ptr.as_ptr() as *mut u8 as u64) }
        efi::Status::SUCCESS
      }
      Err(_) => efi::Status::OUT_OF_RESOURCES,
    },
    efi::ALLOCATE_MAX_ADDRESS => {
      if let Some(address) = unsafe { memory.as_ref() } {
        match allocator.allocate_below_address(layout, *address) {
          Ok(ptr) => {
            unsafe { memory.write(ptr.as_ptr() as *mut u8 as u64) }
            efi::Status::SUCCESS
          }
          Err(_) => efi::Status::OUT_OF_RESOURCES,
        }
      } else {
        efi::Status::INVALID_PARAMETER
      }
    }
    efi::ALLOCATE_ADDRESS => {
      if let Some(address) = unsafe { memory.as_ref() } {
        match allocator.allocate_at_address(layout, *address) {
          Ok(ptr) => {
            unsafe { memory.write(ptr.as_ptr() as *mut u8 as u64) }
            efi::Status::SUCCESS
          }
          Err(_) => efi::Status::OUT_OF_RESOURCES,
        }
      } else {
        efi::Status::INVALID_PARAMETER
      }
    }
    _ => efi::Status::UNSUPPORTED,
  }
}

extern "efiapi" fn free_pages(memory: efi::PhysicalAddress, pages: usize) -> efi::Status {
  let size = match pages.checked_mul(UEFI_PAGE_SIZE) {
    Some(size) => size,
    None => return efi::Status::INVALID_PARAMETER,
  };

  if memory.checked_add(size as u64).is_none() {
    return efi::Status::INVALID_PARAMETER;
  }

  let layout = match Layout::from_size_align(size, UEFI_PAGE_SIZE) {
    Ok(layout) => layout,
    Err(_) => return efi::Status::INVALID_PARAMETER,
  };

  let address = match NonNull::new(memory as usize as *mut u8) {
    Some(address) => address,
    None => return efi::Status::INVALID_PARAMETER,
  };

  if address.as_ptr().align_offset(UEFI_PAGE_SIZE) != 0 {
    return efi::Status::INVALID_PARAMETER;
  }

  match ALL_ALLOCATORS.iter().find(|x| x.contains(address)) {
    Some(allocator) => {
      unsafe { allocator.deallocate(address, layout) };
      efi::Status::SUCCESS
    }
    None => efi::Status::NOT_FOUND,
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
  memory_map: *mut efi::MemoryDescriptor,
  map_key: *mut usize,
  descriptor_size: *mut usize,
  descriptor_version: *mut u32,
) -> efi::Status {
  if memory_map_size.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }

  if !descriptor_size.is_null() {
    unsafe { descriptor_size.write(mem::size_of::<efi::MemoryDescriptor>()) };
  }

  if !descriptor_version.is_null() {
    unsafe { descriptor_version.write(efi::MEMORY_DESCRIPTOR_VERSION) };
  }

  //allocate an empty vector with enough space for all the descriptors with some padding (in the event)
  //that extra descriptors come into being after creation but before usage.
  let mut descriptors: Vec<MemorySpaceDescriptor> = Vec::with_capacity(GCD.memory_descriptor_count() + 10);
  GCD.get_memory_descriptors(&mut descriptors).expect("get_memory_descriptors failed.");

  let map_size = unsafe { *memory_map_size };

  let efi_descriptors: Vec<efi::MemoryDescriptor> = descriptors
    .iter()
    .filter_map(|descriptor| {
      let memory_type = match descriptor.image_handle {
        uefi_protocol_db_lib::RESERVED_MEMORY_ALLOCATOR_HANDLE => efi::RESERVED_MEMORY_TYPE,
        uefi_protocol_db_lib::EFI_LOADER_CODE_ALLOCATOR_HANDLE => efi::LOADER_CODE,
        uefi_protocol_db_lib::EFI_LOADER_DATA_ALLOCATOR_HANDLE => efi::LOADER_DATA,
        uefi_protocol_db_lib::EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE => efi::BOOT_SERVICES_CODE,
        uefi_protocol_db_lib::EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE => efi::BOOT_SERVICES_DATA,
        uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE => efi::RUNTIME_SERVICES_CODE,
        uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE => efi::RUNTIME_SERVICES_DATA,
        uefi_protocol_db_lib::EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE => efi::ACPI_RECLAIM_MEMORY,
        uefi_protocol_db_lib::EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE => efi::ACPI_MEMORY_NVS,
        uefi_protocol_db_lib::INVALID_HANDLE if descriptor.memory_type == GcdMemoryType::SystemMemory => {
          efi::CONVENTIONAL_MEMORY
        }
        _ if descriptor.memory_type == GcdMemoryType::MemoryMappedIo => efi::MEMORY_MAPPED_IO,
        _ => return None, //Not a type of memory to go in the EFI system map.
      };

      let number_of_pages = descriptor.length >> 12;
      if number_of_pages == 0 {
        return None; //skip entries for things smaller than a page.
      }
      if (descriptor.base_address % 0x1000) != 0 {
        return None; //skip entries not page aligned.
      }
      Some(efi::MemoryDescriptor {
        r#type: memory_type,
        physical_start: descriptor.base_address,
        virtual_start: descriptor.base_address,
        number_of_pages,
        attribute: descriptor.attributes,
      })
    })
    .collect();

  assert_ne!(efi_descriptors.len(), 0);

  let required_map_size = efi_descriptors.len() * mem::size_of::<efi::MemoryDescriptor>();

  unsafe { memory_map_size.write(required_map_size) };

  if map_size < required_map_size {
    return efi::Status::BUFFER_TOO_SMALL;
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

  efi::Status::SUCCESS
}

pub fn init_memory_support(bs: &mut efi::BootServices) {
  bs.allocate_pages = allocate_pages;
  bs.free_pages = free_pages;
  bs.allocate_pool = allocate_pool;
  bs.free_pool = free_pool;
  bs.copy_mem = copy_mem;
  bs.set_mem = set_mem;
  bs.get_memory_map = get_memory_map;
}
