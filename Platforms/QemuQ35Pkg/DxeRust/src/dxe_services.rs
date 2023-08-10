mod gcd;
mod memory_block;
mod sorted_slice;

use alloc::boxed::Box;
use core::{ffi::c_void, mem, slice::from_raw_parts};

use r_efi::{
  efi::{AllocateType, Guid, Handle, PhysicalAddress, Status},
  system::{TableHeader, BOOT_SERVICES_REVISION, BOOT_SERVICES_SIGNATURE},
};
use r_pi::dxe_services::{
  DxeServicesTable, GcdAllocateType, GcdIoType, GcdMemoryType, IoSpaceDescriptor, MemorySpaceDescriptor,
  DEX_SERVICES_TABLE_GUID,
};

use crate::{allocator::EFI_RUNTIME_SERVICES_DATA_ALLOCATOR, misc_boot_services, systemtables::EfiSystemTable};

extern "efiapi" fn add_memory_space(
  _gcd_memory_type: GcdMemoryType,
  _base_address: PhysicalAddress,
  _length: u64,
  _capabilities: u64,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn allocate_memory_space(
  _gcd_allocate_type: GcdAllocateType,
  _gcd_memory_type: GcdMemoryType,
  _alignment: u32,
  _length: u64,
  _base_address: *mut PhysicalAddress,
  _image_handle: Handle,
  _device_handle: Handle,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn free_memory_space(_base_address: PhysicalAddress, _length: u64) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn remove_memory_space(_base_address: PhysicalAddress, _length: u64) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn get_memory_space_descriptor(
  _base_address: PhysicalAddress,
  _descriptor: *mut MemorySpaceDescriptor,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn set_memory_space_attributes(
  _base_address: PhysicalAddress,
  _length: u64,
  _attributes: u64,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn set_memory_space_capabilities(
  _base_address: PhysicalAddress,
  _length: u64,
  _capabilities: u64,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn get_memory_space_map(
  _number_of_descriptors: *mut u32,
  _memory_space_map: *mut *mut MemorySpaceDescriptor,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn add_io_space(_gcd_io_type: GcdIoType, _base_address: PhysicalAddress, _length: u64) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn allocate_io_space(
  _allocate_type: AllocateType,
  _gcd_io_type: GcdIoType,
  _alignment: u32,
  _length: u64,
  _base_address: *mut PhysicalAddress,
  _image_handle: Handle,
  _device_handle: Handle,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn free_io_space(_base_address: PhysicalAddress, _length: u64) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn remove_io_space(_base_address: PhysicalAddress, _length: u64) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn get_io_space_descriptor(
  _base_address: PhysicalAddress,
  _descriptor: *mut IoSpaceDescriptor,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn get_io_space_map(
  _number_of_descriptor: *mut u32,
  _io_space_map: *mut *mut IoSpaceDescriptor,
) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn dispatch() -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn schedule(_firmware_volume_handle: Handle, _file_name: *const Guid) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn trust(_firmware_volume_handle: Handle, _file_name: *const Guid) -> Status {
  Status::UNSUPPORTED
}

extern "efiapi" fn process_firmware_volume(
  _firmware_volume_header: *const c_void,
  _size: u32,
  _firmware_volume_handle: *mut Handle,
) -> Status {
  Status::UNSUPPORTED
}

pub fn init_dxe_services(system_table: &mut EfiSystemTable) {
  let mut dxe_system_table = DxeServicesTable {
    header: TableHeader {
      signature: BOOT_SERVICES_SIGNATURE,
      revision: BOOT_SERVICES_REVISION,
      header_size: mem::size_of::<DxeServicesTable>() as u32,
      crc32: 0,
      reserved: 0,
    },
    add_memory_space,
    allocate_memory_space,
    free_memory_space,
    remove_memory_space,
    get_memory_space_descriptor,
    set_memory_space_attributes,
    get_memory_space_map,
    add_io_space,
    allocate_io_space,
    free_io_space,
    remove_io_space,
    get_io_space_descriptor,
    get_io_space_map,
    dispatch,
    schedule,
    trust,
    process_firmware_volume,
    set_memory_space_capabilities,
  };
  let dxe_system_table_ptr = &dxe_system_table as *const DxeServicesTable;
  let crc32 =
    unsafe { crc32fast::hash(from_raw_parts(dxe_system_table_ptr as *const u8, mem::size_of::<DxeServicesTable>())) };
  dxe_system_table.header.crc32 = crc32;

  let dxe_system_table = Box::new_in(dxe_system_table, &EFI_RUNTIME_SERVICES_DATA_ALLOCATOR);

  let _ = misc_boot_services::core_install_configuration_table(
    DEX_SERVICES_TABLE_GUID,
    unsafe { (Box::into_raw(dxe_system_table) as *mut c_void).as_mut() },
    system_table,
  );
}
