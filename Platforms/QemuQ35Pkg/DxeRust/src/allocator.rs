use core::{
  ffi::c_void,
  mem,
  slice::{self, from_raw_parts_mut},
};

use alloc::{collections::BTreeMap, vec::Vec};

use crate::{protocols::PROTOCOL_DB, GCD};
use r_efi::{efi, system::TPL_HIGH_LEVEL};
use r_pi::dxe_services::{GcdMemoryType, MemorySpaceDescriptor};
use uefi_protocol_db_lib;
use uefi_rust_allocator_lib::{uefi_allocator::UefiAllocator, AllocationStrategy};

const UEFI_PAGE_SIZE: usize = 0x1000; //per UEFI spec.

// Private tracking guid used to generate new handles for allocator tracking
// {9D1FA6E9-0C86-4F7F-A99B-DD229C9B3893}
const PRIVATE_ALLOCATOR_TRACKING_GUID: efi::Guid =
  efi::Guid::from_fields(0x9d1fa6e9, 0x0c86, 0x4f7f, 0xa9, 0x9b, &[0xdd, 0x22, 0x9c, 0x9b, 0x38, 0x93]);

// The boot services data allocator is special as it is used as the GlobalAllocator instance for the DxeRust core.
// This means that any rust heap allocations (e.g. Box::new()) will come from this allocator unless explicitly directed
// to a different allocator. This allocator does not need to be public since all dynamic allocations will implicitly
// allocate from it.
#[cfg_attr(not(test), global_allocator)]
static EFI_BOOT_SERVICES_DATA_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::BOOT_SERVICES_DATA, uefi_protocol_db_lib::EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE);

// The following allocators are directly used by the core. These allocators are declared static so that they can easily
// be used in the core without e.g. the overhead of acquiring a lock to retrieve them from the allocator map that all
// the other allocators use.
pub static EFI_LOADER_CODE_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::LOADER_CODE, uefi_protocol_db_lib::EFI_LOADER_CODE_ALLOCATOR_HANDLE);

pub static EFI_BOOT_SERVICES_CODE_ALLOCATOR: UefiAllocator =
  UefiAllocator::new(&GCD, efi::BOOT_SERVICES_CODE, uefi_protocol_db_lib::EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE);

pub static EFI_RUNTIME_SERVICES_CODE_ALLOCATOR: UefiAllocator = UefiAllocator::new(
  &GCD,
  efi::RUNTIME_SERVICES_CODE,
  uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE,
);

pub static EFI_RUNTIME_SERVICES_DATA_ALLOCATOR: UefiAllocator = UefiAllocator::new(
  &GCD,
  efi::RUNTIME_SERVICES_DATA,
  uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE,
);

static STATIC_ALLOCATORS: &[&UefiAllocator] = &[
  &EFI_LOADER_CODE_ALLOCATOR,
  &EFI_BOOT_SERVICES_CODE_ALLOCATOR,
  &EFI_BOOT_SERVICES_DATA_ALLOCATOR,
  &EFI_RUNTIME_SERVICES_CODE_ALLOCATOR,
  &EFI_RUNTIME_SERVICES_DATA_ALLOCATOR,
];

// The following structure is used to track additional allocators that are created in response to allocation requests
// that are not satisfied by the static allocators.
static ALLOCATORS: tpl_lock::TplMutex<AllocatorMap> = AllocatorMap::new();
struct AllocatorMap {
  map: BTreeMap<efi::MemoryType, UefiAllocator>,
}

impl AllocatorMap {
  const fn new() -> tpl_lock::TplMutex<Self> {
    tpl_lock::TplMutex::new(TPL_HIGH_LEVEL, AllocatorMap { map: BTreeMap::new() }, "AllocatorMapLock")
  }
}

impl<'a> AllocatorMap {
  // Returns an iterator that returns references to the static allocators followed by the custom allocators.
  fn iter(&'a self) -> impl Iterator<Item = &UefiAllocator> {
    STATIC_ALLOCATORS.iter().map(|&x| x).chain(self.map.values().map(|x| x))
  }

  fn get_allocator(&'a mut self, memory_type: efi::MemoryType) -> Option<&'a UefiAllocator> {
    //return the static allocator if any
    self.iter().find(|x| x.memory_type() == memory_type)
  }

  fn initialize_allocator(&'a mut self, memory_type: efi::MemoryType, handle: efi::Handle) {
    // the lock ensures exclusive access to the map, but an allocator may have been created already; so only create
    // the allocator if it doesn't yet exist for this memory type.
    if !self.map.contains_key(&memory_type) {
      self.map.insert(memory_type, UefiAllocator::new(&GCD, memory_type, handle));
    }
  }

  //Returns a handle for the given memory type.
  // Handles are sourced from several places (in order).
  // 1. Well-known handles.
  // 2. The handle of an active allocator without a well-known handle that matches the memory type.
  // 3. A freshly created handle.
  //
  // Note: this routine is used to generate new handles for the creation of allocators as needed; this means that an
  // Ok() result from this routine doesn't necessarily guarantee that an allocator associated with this handle exists or
  // memory type exists.
  fn handle_for_memory_type(memory_type: efi::MemoryType) -> Result<efi::Handle, efi::Status> {
    match memory_type {
      efi::RESERVED_MEMORY_TYPE => Ok(uefi_protocol_db_lib::RESERVED_MEMORY_ALLOCATOR_HANDLE),
      efi::LOADER_CODE => Ok(uefi_protocol_db_lib::EFI_LOADER_CODE_ALLOCATOR_HANDLE),
      efi::LOADER_DATA => Ok(uefi_protocol_db_lib::EFI_LOADER_DATA_ALLOCATOR_HANDLE),
      efi::BOOT_SERVICES_CODE => Ok(uefi_protocol_db_lib::EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE),
      efi::BOOT_SERVICES_DATA => Ok(uefi_protocol_db_lib::EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE),
      efi::ACPI_RECLAIM_MEMORY => Ok(uefi_protocol_db_lib::EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE),
      efi::ACPI_MEMORY_NVS => Ok(uefi_protocol_db_lib::EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE),
      // Check to see if it is an invalid type. Memory types efi::PERSISTENT_MEMORY and above to 0x6FFFFFFF are illegal.
      efi::PERSISTENT_MEMORY..=0x6FFFFFFF => Err(efi::Status::INVALID_PARAMETER)?,
      // not a well known handle or illegal memory type - check the active allocators and create a handle if it doesn't
      // already exist.
      _ => {
        if let Some(handle) =
          ALLOCATORS.lock().iter().find_map(|x| if x.memory_type() == memory_type { Some(x.handle()) } else { None })
        {
          return Ok(handle);
        }
        let (handle, _) =
          PROTOCOL_DB.install_protocol_interface(None, PRIVATE_ALLOCATOR_TRACKING_GUID, core::ptr::null_mut())?;
        Ok(handle)
      }
    }
  }

  fn memory_type_for_handle(&self, handle: efi::Handle) -> Option<efi::MemoryType> {
    self.iter().find_map(|x| if x.handle() == handle { Some(x.memory_type()) } else { None })
  }

  // resets the ALLOCATOR map to empty.
  #[cfg(test)]
  unsafe fn reset(&mut self) {
    self.map.clear();
  }
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
  panic!("allocation error: {:?}", layout)
}

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
  if let Some(allocator) = ALLOCATORS.lock().get_allocator(pool_type) {
    let mut buffer: *mut c_void = core::ptr::null_mut();
    let status = unsafe { allocator.allocate_pool(size, core::ptr::addr_of_mut!(buffer)) };
    if status == efi::Status::SUCCESS {
      return Ok(buffer);
    } else {
      return Err(status);
    }
  }

  //if we get here, The requested memory type does not yet have an allocator associated with it.
  //A new handle cannot be created if we are holding the allocator lock, so the handle creation needs to be done outside
  //the lock context.
  let handle = AllocatorMap::handle_for_memory_type(pool_type)?;

  //Create a new allocator with this handle. Note: there are race conditions where more than one handle could be
  //created; but only one new allocator will be created. Do not assume that the handle created here will be the final
  //handle for the allocator.
  ALLOCATORS.lock().initialize_allocator(pool_type, handle);

  //recursively call this function to allocate the memory - the allocator will exist on the next call.
  core_allocate_pool(pool_type, size)
}

extern "efiapi" fn free_pool(buffer: *mut c_void) -> efi::Status {
  if buffer.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }
  let allocators = ALLOCATORS.lock();
  unsafe {
    if allocators.iter().any(|allocator| allocator.free_pool(buffer) == efi::Status::SUCCESS) {
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

  if let Some(allocator) = ALLOCATORS.lock().get_allocator(memory_type) {
    let result = match allocation_type {
      efi::ALLOCATE_ANY_PAGES => allocator.allocate_pages(AllocationStrategy::BottomUp(None), pages),
      efi::ALLOCATE_MAX_ADDRESS => {
        let address = unsafe { memory.as_ref().expect("checked non-null is null") };
        allocator.allocate_pages(AllocationStrategy::BottomUp(Some(*address as usize)), pages)
      }
      efi::ALLOCATE_ADDRESS => {
        let address = unsafe { memory.as_ref().expect("checked non-null is null") };
        allocator.allocate_pages(AllocationStrategy::Address(*address as usize), pages)
      }
      _ => Err(efi::Status::INVALID_PARAMETER),
    };

    if let Ok(ptr) = result {
      unsafe { memory.write(ptr.as_ptr() as *mut u8 as u64) }
      return efi::Status::SUCCESS;
    } else {
      return result.unwrap_err();
    }
  }

  //if we get here, The requested memory type does not yet have an allocator associated with it.
  //A new handle cannot be created if we are holding the allocator lock, so the handle creation needs to be done outside
  //the lock context.
  let handle = match AllocatorMap::handle_for_memory_type(memory_type) {
    Ok(handle) => handle,
    Err(err) => return err,
  };

  //Create a new allocator with this handle. Note: there are race conditions where more than one handle could be
  //created; but only one new allocator will be created. Do not assume that the handle created here will be the final
  //handle for the allocator.
  ALLOCATORS.lock().initialize_allocator(memory_type, handle);

  //recursively call this function to allocate the memory - the allocator will exist on the next call.
  allocate_pages(allocation_type, memory_type, pages, memory)
}

extern "efiapi" fn free_pages(memory: efi::PhysicalAddress, pages: usize) -> efi::Status {
  let size = match pages.checked_mul(UEFI_PAGE_SIZE) {
    Some(size) => size,
    None => return efi::Status::INVALID_PARAMETER,
  };

  if memory.checked_add(size as u64).is_none() {
    return efi::Status::INVALID_PARAMETER;
  }

  if memory.checked_rem(UEFI_PAGE_SIZE as efi::PhysicalAddress) != Some(0) {
    return efi::Status::INVALID_PARAMETER;
  }

  let allocators = ALLOCATORS.lock();

  unsafe {
    if allocators.iter().any(|allocator| allocator.free_pages(memory as usize, pages).is_ok()) {
      efi::Status::SUCCESS
    } else {
      efi::Status::NOT_FOUND
    }
  }
}

extern "efiapi" fn copy_mem(destination: *mut c_void, source: *mut c_void, length: usize) {
  //nothing about this is safe.
  unsafe { core::ptr::copy(source as *mut u8, destination as *mut u8, length) }
}

extern "efiapi" fn set_mem(buffer: *mut c_void, size: usize, value: u8) {
  //nothing about this is safe.
  unsafe {
    let dst_buffer = from_raw_parts_mut(buffer as *mut u8, size);
    dst_buffer.fill(value);
  }
}

fn get_memory_map_descriptors() -> Vec<efi::MemoryDescriptor> {
  let mut descriptors: Vec<MemorySpaceDescriptor> = Vec::with_capacity(GCD.memory_descriptor_count() + 10);
  GCD.get_memory_descriptors(&mut descriptors).expect("get_memory_descriptors failed.");

  descriptors
    .iter()
    .filter_map(|descriptor| {
      let memory_type = ALLOCATORS.lock().memory_type_for_handle(descriptor.image_handle).or_else(|| {
        //descriptor doesn't correspond to allocated memory, so determine if it should be part of the map.
        match descriptor.image_handle {
          // free memory not tracked by any allocator.
          uefi_protocol_db_lib::INVALID_HANDLE if descriptor.memory_type == GcdMemoryType::SystemMemory => {
            Some(efi::CONVENTIONAL_MEMORY)
          }
          // MMIO. Note: there could also be MMIO tracked by the allocators which would not hit this case.
          _ if descriptor.memory_type == GcdMemoryType::MemoryMappedIo => Some(efi::MEMORY_MAPPED_IO),

          // Persistent. Note: this type is not allocatable, but might be created by agents other than the core directly
          // in the GCD.
          _ if descriptor.memory_type == GcdMemoryType::Persistent => Some(efi::PERSISTENT_MEMORY),

          // Unaccepted. Note: this type is not allocatable, but might be created by agents other than the core directly
          // in the GCD.
          _ if descriptor.memory_type == GcdMemoryType::Unaccepted => Some(efi::UNACCEPTED_MEMORY_TYPE),
          // Other memory types are ignored for purposes of the memory map
          _ => None,
        }
      })?;

      let number_of_pages = descriptor.length >> 12;
      if number_of_pages == 0 {
        return None; //skip entries for things smaller than a page
      }
      if (descriptor.base_address % 0x1000) != 0 {
        return None; //skip entries not page aligned.
      }

      //TODO: update/mask attributes.

      Some(efi::MemoryDescriptor {
        r#type: memory_type,
        physical_start: descriptor.base_address,
        virtual_start: 0,
        number_of_pages,
        attribute: match memory_type {
          efi::RUNTIME_SERVICES_CODE | efi::RUNTIME_SERVICES_DATA => descriptor.attributes | efi::MEMORY_RUNTIME,
          _ => descriptor.attributes,
        },
      })
    })
    .collect()
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

  let map_size = unsafe { *memory_map_size };

  let mut efi_descriptors = get_memory_map_descriptors();

  assert_ne!(efi_descriptors.len(), 0);

  let required_map_size = efi_descriptors.len() * mem::size_of::<efi::MemoryDescriptor>();

  unsafe { memory_map_size.write(required_map_size) };

  if map_size < required_map_size {
    return efi::Status::BUFFER_TOO_SMALL;
  }

  if memory_map.is_null() {
    return efi::Status::INVALID_PARAMETER;
  }

  // Rust will try to prevent an unaligned copy, given no one checks whether their points are aligned
  // treat the slice as a u8 slice and copy the bytes.
  for descriptor in efi_descriptors.iter_mut() {
    descriptor.attribute = descriptor.attribute & !efi::MEMORY_ACCESS_MASK;
  }
  let efi_descriptors_ptr = efi_descriptors.as_ptr() as *mut u8;

  unsafe {
    core::ptr::copy(efi_descriptors_ptr, memory_map as *mut u8, required_map_size);

    if !map_key.is_null() {
      let memory_map_as_bytes = slice::from_raw_parts(memory_map as *mut u8, required_map_size);
      map_key.write(crc32fast::hash(memory_map_as_bytes) as usize);
    }
  }

  efi::Status::SUCCESS
}

pub fn terminate_memory_map(map_key: usize) -> efi::Status {
  let mut mm_desc = get_memory_map_descriptors();
  for descriptor in mm_desc.iter_mut() {
    descriptor.attribute = descriptor.attribute & !efi::MEMORY_ACCESS_MASK;
  }
  let mm_desc_size = mm_desc.len() * mem::size_of::<efi::MemoryDescriptor>();
  let mm_desc_bytes: &[u8] = unsafe { slice::from_raw_parts(mm_desc.as_ptr() as *const u8, mm_desc_size) };

  let current_map_key = crc32fast::hash(&mm_desc_bytes) as usize;
  if map_key == current_map_key {
    efi::Status::SUCCESS
  } else {
    efi::Status::INVALID_PARAMETER
  }
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

#[cfg(test)]
mod tests {

  use crate::test_support;

  use super::*;
  use r_efi::efi;

  #[test]
  fn init_memory_support_should_populate_boot_services_ptrs() {
    let boot_services = core::mem::MaybeUninit::zeroed();
    let mut boot_services: efi::BootServices = unsafe { boot_services.assume_init() };
    init_memory_support(&mut boot_services);
    assert!(boot_services.allocate_pages == allocate_pages);
    assert!(boot_services.free_pages == free_pages);
    assert!(boot_services.allocate_pool == allocate_pool);
    assert!(boot_services.free_pool == free_pool);
    assert!(boot_services.copy_mem == copy_mem);
    assert!(boot_services.get_memory_map == get_memory_map);
  }

  #[test]
  fn new_should_create_new_allocator_map() {
    let _map = AllocatorMap::new();
  }

  #[test]
  fn well_known_allocators_should_be_retrievable() {
    let test_lock = test_support::GLOBAL_STATE_TEST_LOCK.lock();

    unsafe {
      test_support::init_test_gcd(None);
      test_support::init_test_protocol_db();
      ALLOCATORS.lock().reset();
    }

    let mut allocators = ALLOCATORS.lock();

    for (mem_type, handle) in [
      (efi::LOADER_CODE, uefi_protocol_db_lib::EFI_LOADER_CODE_ALLOCATOR_HANDLE),
      (efi::BOOT_SERVICES_CODE, uefi_protocol_db_lib::EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE),
      (efi::BOOT_SERVICES_DATA, uefi_protocol_db_lib::EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE),
      (efi::RUNTIME_SERVICES_CODE, uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE),
      (efi::RUNTIME_SERVICES_DATA, uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE),
    ] {
      let allocator = allocators.get_allocator(mem_type).unwrap();
      assert_eq!(allocator.handle(), handle);
    }

    drop(test_lock);
  }

  #[test]
  fn new_allocators_should_be_created_on_demand() {
    let test_lock = test_support::GLOBAL_STATE_TEST_LOCK.lock();
    unsafe {
      test_support::init_test_gcd(Some(0x4000000));
      test_support::init_test_protocol_db();
      ALLOCATORS.lock().reset();
    }

    for (mem_type, handle) in [
      (efi::RESERVED_MEMORY_TYPE, uefi_protocol_db_lib::RESERVED_MEMORY_ALLOCATOR_HANDLE),
      (efi::LOADER_CODE, uefi_protocol_db_lib::EFI_LOADER_CODE_ALLOCATOR_HANDLE),
      (efi::LOADER_DATA, uefi_protocol_db_lib::EFI_LOADER_DATA_ALLOCATOR_HANDLE),
      (efi::BOOT_SERVICES_CODE, uefi_protocol_db_lib::EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE),
      (efi::BOOT_SERVICES_DATA, uefi_protocol_db_lib::EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE),
      (efi::RUNTIME_SERVICES_CODE, uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE),
      (efi::RUNTIME_SERVICES_DATA, uefi_protocol_db_lib::EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE),
      (efi::ACPI_RECLAIM_MEMORY, uefi_protocol_db_lib::EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE),
      (efi::ACPI_MEMORY_NVS, uefi_protocol_db_lib::EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE),
    ] {
      let ptr = core_allocate_pool(mem_type, 0x1000).unwrap();
      assert!(!ptr.is_null());

      let mut allocators = ALLOCATORS.lock();

      let allocator = allocators.get_allocator(mem_type).unwrap();
      assert_eq!(allocator.handle(), handle);
      assert_eq!(allocators.memory_type_for_handle(handle), Some(mem_type));
      drop(allocators);
      assert_eq!(AllocatorMap::handle_for_memory_type(mem_type).unwrap(), handle);
    }

    // make sure invalid mem types throw an error.
    assert_eq!(core_allocate_pool(efi::PERSISTENT_MEMORY, 0x1000), Err(efi::Status::INVALID_PARAMETER));
    assert_eq!(core_allocate_pool(efi::PERSISTENT_MEMORY + 0x1000, 0x1000), Err(efi::Status::INVALID_PARAMETER));

    // check "OEM" and "OS" custom memory types.
    let ptr = core_allocate_pool(0x71234567, 0x1000).unwrap();
    assert!(!ptr.is_null());

    let ptr = core_allocate_pool(0x81234567, 0x1000).unwrap();
    assert!(!ptr.is_null());

    let mut allocators = ALLOCATORS.lock();
    let allocator = allocators.get_allocator(0x71234567).unwrap();
    let handle = allocator.handle();
    assert_eq!(allocators.memory_type_for_handle(handle), Some(0x71234567));
    drop(allocators);
    assert_eq!(AllocatorMap::handle_for_memory_type(0x71234567).unwrap(), handle);

    let mut allocators = ALLOCATORS.lock();
    let allocator = allocators.get_allocator(0x81234567).unwrap();
    let handle = allocator.handle();
    assert_eq!(allocators.memory_type_for_handle(handle), Some(0x81234567));
    drop(allocators);
    assert_eq!(AllocatorMap::handle_for_memory_type(0x81234567).unwrap(), handle);

    drop(test_lock);
  }

  #[test]
  fn allocate_pool_should_allocate_pool() {
    let test_lock = test_support::GLOBAL_STATE_TEST_LOCK.lock();
    unsafe {
      test_support::init_test_gcd(Some(0x1000000));
      test_support::init_test_protocol_db();
      ALLOCATORS.lock().reset();
    }

    let mut buffer_ptr = core::ptr::null_mut();
    assert_eq!(
      allocate_pool(efi::BOOT_SERVICES_DATA, 0x1000, core::ptr::addr_of_mut!(buffer_ptr)),
      efi::Status::SUCCESS
    );

    let mut buffer_ptr = core::ptr::null_mut();
    assert_eq!(
      allocate_pool(efi::BOOT_SERVICES_DATA, 0x2000000, core::ptr::addr_of_mut!(buffer_ptr)),
      efi::Status::OUT_OF_RESOURCES
    );

    assert_eq!(
      allocate_pool(efi::BOOT_SERVICES_DATA, 0x2000000, core::ptr::null_mut()),
      efi::Status::INVALID_PARAMETER
    );

    drop(test_lock);
  }

  #[test]
  fn free_pool_should_free_pool() {
    let test_lock = test_support::GLOBAL_STATE_TEST_LOCK.lock();
    unsafe {
      test_support::init_test_gcd(Some(0x1000000));
      test_support::init_test_protocol_db();
      ALLOCATORS.lock().reset();
    }

    let mut buffer_ptr = core::ptr::null_mut();
    assert_eq!(
      allocate_pool(efi::BOOT_SERVICES_DATA, 0x1000, core::ptr::addr_of_mut!(buffer_ptr)),
      efi::Status::SUCCESS
    );

    assert_eq!(free_pool(buffer_ptr), efi::Status::SUCCESS);

    assert_eq!(free_pool(core::ptr::null_mut()), efi::Status::INVALID_PARAMETER);
    //TODO: these cause non-unwinding panic which crashes the test even with "#[should_panic]".
    //assert_eq!(free_pool(buffer_ptr), efi::Status::INVALID_PARAMETER);
    //assert_eq!(free_pool(((buffer_ptr as usize) + 10) as *mut c_void), efi::Status::INVALID_PARAMETER);
    drop(test_lock);
  }

  #[test]
  fn allocate_pages_should_allocate_pages() {
    let test_lock = test_support::GLOBAL_STATE_TEST_LOCK.lock();
    unsafe {
      test_support::init_test_gcd(Some(0x1000000));
      test_support::init_test_protocol_db();
      ALLOCATORS.lock().reset();
    }

    //test test null memory pointer fails with invalid param.
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ANY_PAGES,
        efi::BOOT_SERVICES_DATA,
        0x4,
        core::ptr::null_mut() as *mut efi::PhysicalAddress
      ),
      efi::Status::INVALID_PARAMETER
    );

    //test successful allocate_any
    let mut buffer_ptr: *mut u8 = core::ptr::null_mut();
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ANY_PAGES,
        efi::BOOT_SERVICES_DATA,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::SUCCESS
    );
    free_pages(buffer_ptr as u64, 0x10);

    //test successful allocate_address at the address that was just freed
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ADDRESS,
        efi::BOOT_SERVICES_DATA,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::SUCCESS
    );
    free_pages(buffer_ptr as u64, 0x10);

    //test successful allocate_max where max is greater than the address that was just freed.
    buffer_ptr = buffer_ptr.wrapping_add(0x11 * 0x1000);
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_MAX_ADDRESS,
        efi::BOOT_SERVICES_DATA,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::SUCCESS
    );
    free_pages(buffer_ptr as u64, 0x10);

    //test unsuccessful allocate_max where max is less than the address that was just freed.
    buffer_ptr = buffer_ptr.wrapping_sub(0x12 * 0x1000);
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_MAX_ADDRESS,
        efi::BOOT_SERVICES_DATA,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::NOT_FOUND
    );

    //test invalid allocation type
    assert_eq!(
      allocate_pages(
        0x12345,
        efi::BOOT_SERVICES_DATA,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::INVALID_PARAMETER
    );

    //test creation of new allocator for OS/OEM defined allocator type.
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ANY_PAGES,
        0x71234567,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::SUCCESS
    );
    free_pages(buffer_ptr as u64, 0x10);
    let mut allocators = ALLOCATORS.lock();
    let allocator = allocators.get_allocator(0x71234567).unwrap();
    let handle = allocator.handle();
    assert_eq!(allocators.memory_type_for_handle(handle), Some(0x71234567));
    drop(allocators);
    assert_eq!(AllocatorMap::handle_for_memory_type(0x71234567).unwrap(), handle);

    //test that creation of new allocator for illegal type fails.
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ANY_PAGES,
        efi::PERSISTENT_MEMORY,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::INVALID_PARAMETER
    );

    drop(test_lock);
  }

  #[test]
  fn free_pages_error_scenarios_should_be_handled_properly() {
    let test_lock = test_support::GLOBAL_STATE_TEST_LOCK.lock();
    unsafe {
      test_support::init_test_gcd(Some(0x1000000));
      test_support::init_test_protocol_db();
      ALLOCATORS.lock().reset();
    }

    assert_eq!(free_pages(0x12345000, usize::MAX & !0xFFF), efi::Status::INVALID_PARAMETER);
    assert_eq!(free_pages(u64::MAX & !0xFFF, 0x10), efi::Status::INVALID_PARAMETER);
    assert_eq!(free_pages(0x12345678, 1), efi::Status::INVALID_PARAMETER);
    assert_eq!(free_pages(0x12345000, 1), efi::Status::NOT_FOUND);

    drop(test_lock);
  }

  #[test]
  fn copy_mem_should_copy_mem() {
    let mut dest = vec![0xa5u8; 0x10];
    let mut src = vec![0x5au8; 0x10];
    copy_mem(dest.as_mut_ptr() as *mut c_void, src.as_mut_ptr() as *mut c_void, 0x10);
    assert_eq!(dest, src);
  }

  #[test]
  fn set_mem_should_set_mem() {
    let mut dest = vec![0xa5u8; 0x10];
    set_mem(dest.as_mut_ptr() as *mut c_void, 0x10, 0x00);
    assert_eq!(dest, vec![0x00u8; 0x10]);
  }

  #[test]
  fn get_memory_map_should_return_a_memory_map() {
    let test_lock = test_support::GLOBAL_STATE_TEST_LOCK.lock();
    unsafe {
      test_support::init_test_gcd(Some(0x1000000));
      test_support::init_test_protocol_db();
      ALLOCATORS.lock().reset();
    }

    // allocate some "custom" type pages to create something interesting to find in the map.
    let mut buffer_ptr: *mut u8 = core::ptr::null_mut();
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ANY_PAGES,
        0x71234567,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::SUCCESS
    );

    // allocate some "custom" type pages to create something interesting to find in the map.
    let mut runtime_buffer_ptr: *mut u8 = core::ptr::null_mut();
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ANY_PAGES,
        efi::RUNTIME_SERVICES_DATA,
        0x10,
        core::ptr::addr_of_mut!(runtime_buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::SUCCESS
    );

    let mut memory_map_size = 0;
    let mut map_key = 0;
    let mut descriptor_size = 0;
    let mut version = 0;
    let status = get_memory_map(
      core::ptr::addr_of_mut!(memory_map_size),
      core::ptr::null_mut(),
      core::ptr::addr_of_mut!(map_key),
      core::ptr::addr_of_mut!(descriptor_size),
      core::ptr::addr_of_mut!(version),
    );
    assert_eq!(status, efi::Status::BUFFER_TOO_SMALL);
    assert_ne!(memory_map_size, 0);
    assert_eq!(descriptor_size, core::mem::size_of::<efi::MemoryDescriptor>());
    assert_eq!(version, 1);
    assert_eq!(map_key, 0);

    let mut memory_map_buffer: Vec<efi::MemoryDescriptor> =
      vec![
        efi::MemoryDescriptor { r#type: 0, physical_start: 0, virtual_start: 0, number_of_pages: 0, attribute: 0 };
        memory_map_size / descriptor_size
      ];

    let status = get_memory_map(
      core::ptr::addr_of_mut!(memory_map_size),
      memory_map_buffer.as_mut_ptr(),
      core::ptr::addr_of_mut!(map_key),
      core::ptr::addr_of_mut!(descriptor_size),
      core::ptr::addr_of_mut!(version),
    );
    assert_eq!(status, efi::Status::SUCCESS);
    assert_eq!(memory_map_size, memory_map_buffer.len() * core::mem::size_of::<efi::MemoryDescriptor>());
    assert_eq!(descriptor_size, core::mem::size_of::<efi::MemoryDescriptor>());
    assert_eq!(version, 1);
    assert_ne!(map_key, 0);

    //make sure that the custom "allocate_pages" shows up in the map somewhere.
    memory_map_buffer
      .iter()
      .find(|x| {
        x.physical_start <= buffer_ptr as efi::PhysicalAddress
          && x.physical_start.checked_add(x.number_of_pages * UEFI_PAGE_SIZE as u64).unwrap()
            > buffer_ptr as efi::PhysicalAddress
          && x.r#type == 0x71234567
      })
      .expect("Failed to find custom allocation.");

    //make sure that the runtime "allocate_pages" shows up in the map somewhere.
    memory_map_buffer
      .iter()
      .find(|x| {
        x.physical_start <= runtime_buffer_ptr as efi::PhysicalAddress
          && x.physical_start.checked_add(x.number_of_pages * UEFI_PAGE_SIZE as u64).unwrap()
            > runtime_buffer_ptr as efi::PhysicalAddress
          && x.r#type == efi::RUNTIME_SERVICES_DATA
          && (x.attribute & efi::MEMORY_RUNTIME) != 0
      })
      .expect("Failed to find runtime allocation.");

    //get_memory_map with null size should return invalid parameter
    let status = get_memory_map(
      core::ptr::null_mut(),
      memory_map_buffer.as_mut_ptr(),
      core::ptr::addr_of_mut!(map_key),
      core::ptr::addr_of_mut!(descriptor_size),
      core::ptr::addr_of_mut!(version),
    );
    assert_eq!(status, efi::Status::INVALID_PARAMETER);

    //get_memory_map with non-null size but null map should return invalid parameter
    let status = get_memory_map(
      core::ptr::addr_of_mut!(memory_map_size),
      core::ptr::null_mut(),
      core::ptr::addr_of_mut!(map_key),
      core::ptr::addr_of_mut!(descriptor_size),
      core::ptr::addr_of_mut!(version),
    );
    assert_eq!(status, efi::Status::INVALID_PARAMETER);

    drop(test_lock);
  }

  #[test]
  fn terminate_map_should_validate_the_map_key() {
    let test_lock = test_support::GLOBAL_STATE_TEST_LOCK.lock();
    unsafe {
      test_support::init_test_gcd(Some(0x1000000));
      test_support::init_test_protocol_db();
      ALLOCATORS.lock().reset();
    }

    // allocate some "custom" type pages to create something interesting to find in the map.
    let mut buffer_ptr: *mut u8 = core::ptr::null_mut();
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ANY_PAGES,
        0x71234567,
        0x10,
        core::ptr::addr_of_mut!(buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::SUCCESS
    );

    // allocate some "custom" type pages to create something interesting to find in the map.
    let mut runtime_buffer_ptr: *mut u8 = core::ptr::null_mut();
    assert_eq!(
      allocate_pages(
        efi::ALLOCATE_ANY_PAGES,
        efi::RUNTIME_SERVICES_DATA,
        0x10,
        core::ptr::addr_of_mut!(runtime_buffer_ptr) as *mut efi::PhysicalAddress
      ),
      efi::Status::SUCCESS
    );

    //get the map.
    let mut memory_map_size = 0;
    let mut map_key = 0;
    let mut descriptor_size = 0;
    let mut version = 0;
    let status = get_memory_map(
      core::ptr::addr_of_mut!(memory_map_size),
      core::ptr::null_mut(),
      core::ptr::addr_of_mut!(map_key),
      core::ptr::addr_of_mut!(descriptor_size),
      core::ptr::addr_of_mut!(version),
    );
    assert_eq!(status, efi::Status::BUFFER_TOO_SMALL);

    let mut memory_map_buffer: Vec<efi::MemoryDescriptor> =
      vec![
        efi::MemoryDescriptor { r#type: 0, physical_start: 0, virtual_start: 0, number_of_pages: 0, attribute: 0 };
        memory_map_size / descriptor_size
      ];

    let status = get_memory_map(
      core::ptr::addr_of_mut!(memory_map_size),
      memory_map_buffer.as_mut_ptr(),
      core::ptr::addr_of_mut!(map_key),
      core::ptr::addr_of_mut!(descriptor_size),
      core::ptr::addr_of_mut!(version),
    );
    assert_eq!(status, efi::Status::SUCCESS);

    assert_eq!(terminate_memory_map(map_key), efi::Status::SUCCESS);
    assert_eq!(terminate_memory_map(map_key + 1), efi::Status::INVALID_PARAMETER);

    drop(test_lock);
  }
}
