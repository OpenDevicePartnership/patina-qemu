use alloc::{boxed::Box, vec::Vec};
use core::{
  ffi::c_void,
  mem,
  slice::{self, from_raw_parts},
  sync::atomic::{AtomicPtr, Ordering},
};
use uefi_gcd_lib::gcd;

use r_efi::{
  efi::{self, Guid, Handle, PhysicalAddress, Status},
  system::{self, TableHeader, BOOT_SERVICES_REVISION, BOOT_SERVICES_SIGNATURE, EVT_NOTIFY_SIGNAL, TPL_CALLBACK},
};
use r_pi::{
  cpu_arch,
  dxe_services::{
    DxeServicesTable, GcdAllocateType, GcdIoType, GcdMemoryType, IoSpaceDescriptor, MemorySpaceDescriptor,
    DEX_SERVICES_TABLE_GUID,
  },
};

use serial_print_dxe::println;

use crate::{
  allocator::{allocate_pool, EFI_RUNTIME_SERVICES_DATA_ALLOCATOR},
  dispatcher::core_dispatcher,
  events::EVENT_DB,
  misc_boot_services,
  protocols::PROTOCOL_DB,
  systemtables::EfiSystemTable,
  GCD,
};

static CPU_ARCH_PTR: AtomicPtr<cpu_arch::Protocol> = AtomicPtr::new(core::ptr::null_mut());

fn result_to_efi_status(err: gcd::Error) -> efi::Status {
  match err {
    uefi_gcd_lib::gcd::Error::AccessDenied => Status::ACCESS_DENIED,
    uefi_gcd_lib::gcd::Error::InvalidParameter => Status::INVALID_PARAMETER,
    uefi_gcd_lib::gcd::Error::NotFound => Status::NOT_FOUND,
    uefi_gcd_lib::gcd::Error::NotInitialized => Status::NOT_READY,
    uefi_gcd_lib::gcd::Error::OutOfResources => Status::OUT_OF_RESOURCES,
    uefi_gcd_lib::gcd::Error::Unsupported => Status::UNSUPPORTED,
  }
}

extern "efiapi" fn add_memory_space(
  gcd_memory_type: GcdMemoryType,
  base_address: PhysicalAddress,
  length: u64,
  capabilities: u64,
) -> Status {
  let result = unsafe { GCD.add_memory_space(gcd_memory_type, base_address as usize, length as usize, capabilities) };

  match result {
    Ok(_) => Status::SUCCESS,
    Err(err) => result_to_efi_status(err),
  }
}

extern "efiapi" fn allocate_memory_space(
  gcd_allocate_type: GcdAllocateType,
  gcd_memory_type: GcdMemoryType,
  alignment: u32,
  length: u64,
  base_address: *mut PhysicalAddress,
  image_handle: Handle,
  device_handle: Handle,
) -> Status {
  if base_address.is_null() {
    return Status::INVALID_PARAMETER;
  }

  let allocate_type = match gcd_allocate_type {
    GcdAllocateType::Address => {
      let desired_address = unsafe { *base_address };
      gcd::AllocateType::Address(desired_address as usize)
    }
    GcdAllocateType::AnySearchBottomUp => gcd::AllocateType::BottomUp(None),
    GcdAllocateType::AnySearchTopDown => gcd::AllocateType::TopDown(None),
    GcdAllocateType::MaxAddressSearchBottomUp => {
      let limit = unsafe { *base_address };
      gcd::AllocateType::BottomUp(Some(limit as usize))
    }
    GcdAllocateType::MaxAddressSearchTopDown => {
      let limit = unsafe { *base_address };
      gcd::AllocateType::TopDown(Some(limit as usize))
    }
    _ => return Status::INVALID_PARAMETER,
  };

  let result = GCD.allocate_memory_space(
    allocate_type,
    gcd_memory_type,
    alignment,
    length as usize,
    image_handle,
    if device_handle.is_null() { None } else { Some(device_handle) },
  );

  match result {
    Ok(allocated_addr) => {
      unsafe { base_address.write(allocated_addr as u64) };
      Status::SUCCESS
    }
    Err(err) => result_to_efi_status(err),
  }
}

extern "efiapi" fn free_memory_space(base_address: PhysicalAddress, length: u64) -> Status {
  let result = GCD.free_memory_space(base_address as usize, length as usize);

  match result {
    Ok(_) => Status::SUCCESS,
    Err(err) => result_to_efi_status(err),
  }
}

extern "efiapi" fn remove_memory_space(base_address: PhysicalAddress, length: u64) -> Status {
  let result = GCD.remove_memory_space(base_address as usize, length as usize);
  match result {
    Ok(_) => Status::SUCCESS,
    Err(err) => result_to_efi_status(err),
  }
}

pub extern "efiapi" fn get_memory_space_descriptor(
  base_address: PhysicalAddress,
  descriptor: *mut MemorySpaceDescriptor,
) -> Status {
  if descriptor.is_null() {
    return Status::INVALID_PARAMETER;
  }

  //Note: this would be more efficient if it was done in the GCD; rather than retrieving all the descriptors and
  //searching them here. It is done this way for simplicity - it can be optimized if it proves too slow.

  //allocate an empty vector with enough space for all the descriptors with some padding (in the event)
  //that extra descriptors come into being after creation but before usage.
  let mut descriptors: Vec<MemorySpaceDescriptor> = Vec::with_capacity(GCD.memory_descriptor_count() + 10);
  let result = GCD.get_memory_descriptors(&mut descriptors);

  if let Err(err) = result {
    return result_to_efi_status(err);
  }

  let target_descriptor =
    descriptors.iter().find(|x| (x.base_address <= base_address) && (base_address < (x.base_address + x.length)));

  if let Some(target_descriptor) = target_descriptor {
    unsafe { descriptor.write(*target_descriptor) };
    Status::SUCCESS
  } else {
    Status::NOT_FOUND
  }
}

extern "efiapi" fn set_memory_space_attributes(base_address: PhysicalAddress, length: u64, attributes: u64) -> Status {
  let result = GCD.set_memory_space_attributes(base_address as usize, length as usize, attributes);
  if let Err(err) = result {
    return result_to_efi_status(err);
  }

  let result = cpu_set_memory_space_attributes(base_address, length, attributes);
  if let Err(err) = result {
    return err;
  }

  efi::Status::SUCCESS
}

extern "efiapi" fn set_memory_space_capabilities(
  base_address: PhysicalAddress,
  length: u64,
  capabilities: u64,
) -> Status {
  let result = GCD.set_memory_space_capabilities(base_address as usize, length as usize, capabilities);
  if let Err(err) = result {
    return result_to_efi_status(err);
  }

  let result = cpu_set_memory_space_attributes(base_address, length, capabilities & !system::MEMORY_RUNTIME);
  if let Err(err) = result {
    return err;
  }

  efi::Status::SUCCESS
}

extern "efiapi" fn get_memory_space_map(
  number_of_descriptors: *mut u32,
  memory_space_map: *mut *mut MemorySpaceDescriptor,
) -> Status {
  if number_of_descriptors.is_null() || memory_space_map.is_null() {
    return Status::INVALID_PARAMETER;
  }

  //allocate an empty vector with enough space for all the descriptors with some padding (in the event)
  //that extra descriptors come into being after creation but before usage.
  let mut descriptors: Vec<MemorySpaceDescriptor> = Vec::with_capacity(GCD.memory_descriptor_count() + 10);
  let result = GCD.get_memory_descriptors(&mut descriptors);

  if let Err(err) = result {
    return result_to_efi_status(err);
  }

  //caller is supposed to free the handle buffer using free pool, so we need to allocate it using allocate pool.
  let buffer_size = descriptors.len() * mem::size_of::<MemorySpaceDescriptor>();
  match allocate_pool(r_efi::efi::BOOT_SERVICES_DATA, buffer_size, memory_space_map as *mut *mut c_void) {
    r_efi::efi::Status::SUCCESS => (),
    err => return err,
  };

  //allocation succeeded, so set the descriptor count for caller and copy the descriptors into the output buffer.
  unsafe {
    number_of_descriptors.write(descriptors.len() as u32);
    slice::from_raw_parts_mut(*memory_space_map, descriptors.len()).copy_from_slice(&descriptors);
  }

  Status::SUCCESS
}

extern "efiapi" fn add_io_space(gcd_io_type: GcdIoType, base_address: PhysicalAddress, length: u64) -> Status {
  let result = GCD.add_io_space(gcd_io_type, base_address as usize, length as usize);
  match result {
    Ok(_) => Status::SUCCESS,
    Err(err) => result_to_efi_status(err),
  }
}

extern "efiapi" fn allocate_io_space(
  gcd_allocate_type: GcdAllocateType,
  gcd_io_type: GcdIoType,
  alignment: u32,
  length: u64,
  base_address: *mut PhysicalAddress,
  image_handle: Handle,
  device_handle: Handle,
) -> Status {
  if base_address.is_null() {
    return Status::INVALID_PARAMETER;
  }

  let allocate_type = match gcd_allocate_type {
    GcdAllocateType::Address => {
      let desired_address = unsafe { *base_address };
      gcd::AllocateType::Address(desired_address as usize)
    }
    GcdAllocateType::AnySearchBottomUp => gcd::AllocateType::BottomUp(None),
    GcdAllocateType::AnySearchTopDown => gcd::AllocateType::TopDown(None),
    GcdAllocateType::MaxAddressSearchBottomUp => {
      let limit = unsafe { *base_address };
      gcd::AllocateType::BottomUp(Some(limit as usize))
    }
    GcdAllocateType::MaxAddressSearchTopDown => {
      let limit = unsafe { *base_address };
      gcd::AllocateType::TopDown(Some(limit as usize))
    }
    _ => return Status::INVALID_PARAMETER,
  };

  let result = GCD.allocate_io_space(
    allocate_type,
    gcd_io_type,
    alignment,
    length as usize,
    image_handle,
    if device_handle.is_null() { None } else { Some(device_handle) },
  );

  match result {
    Ok(allocated_addr) => {
      unsafe { base_address.write(allocated_addr as u64) };
      Status::SUCCESS
    }
    Err(err) => result_to_efi_status(err),
  }
}

extern "efiapi" fn free_io_space(base_address: PhysicalAddress, length: u64) -> Status {
  let result = GCD.free_io_space(base_address as usize, length as usize);

  match result {
    Ok(_) => Status::SUCCESS,
    Err(err) => result_to_efi_status(err),
  }
}

extern "efiapi" fn remove_io_space(base_address: PhysicalAddress, length: u64) -> Status {
  let result = GCD.remove_io_space(base_address as usize, length as usize);
  match result {
    Ok(_) => Status::SUCCESS,
    Err(err) => result_to_efi_status(err),
  }
}

extern "efiapi" fn get_io_space_descriptor(
  base_address: PhysicalAddress,
  descriptor: *mut IoSpaceDescriptor,
) -> Status {
  //Note: this would be more efficient if it was done in the GCD; rather than retrieving all the descriptors and
  //searching them here. It is done this way for simplicity - it can be optimized if it proves too slow.

  //allocate an empty vector with enough space for all the descriptors with some padding (in the event)
  //that extra descriptors come into being after creation but before usage.
  let mut descriptors: Vec<IoSpaceDescriptor> = Vec::with_capacity(GCD.io_descriptor_count() + 10);
  let result = GCD.get_io_descriptors(&mut descriptors);

  if let Err(err) = result {
    return result_to_efi_status(err);
  }

  let target_descriptor =
    descriptors.iter().find(|x| (x.base_address <= base_address) && (base_address < (x.base_address + x.length)));

  if let Some(target_descriptor) = target_descriptor {
    unsafe { descriptor.write(*target_descriptor) };
    Status::SUCCESS
  } else {
    Status::NOT_FOUND
  }
}

extern "efiapi" fn get_io_space_map(
  number_of_descriptors: *mut u32,
  io_space_map: *mut *mut IoSpaceDescriptor,
) -> Status {
  //allocate an empty vector with enough space for all the descriptors with some padding (in the event)
  //that extra descriptors come into being after creation but before usage.
  let mut descriptors: Vec<IoSpaceDescriptor> = Vec::with_capacity(GCD.io_descriptor_count() + 10);
  let result = GCD.get_io_descriptors(&mut descriptors);

  if let Err(err) = result {
    return result_to_efi_status(err);
  }

  //caller is supposed to free the handle buffer using free pool, so we need to allocate it using allocate pool.
  let buffer_size = descriptors.len() * mem::size_of::<IoSpaceDescriptor>();
  match allocate_pool(r_efi::efi::BOOT_SERVICES_DATA, buffer_size, io_space_map as *mut *mut c_void) {
    r_efi::efi::Status::SUCCESS => (),
    err => return err,
  };

  //allocation succeeded, so set the descriptor count for caller and copy the descriptors into the output buffer.
  unsafe {
    number_of_descriptors.write(descriptors.len() as u32);
    slice::from_raw_parts_mut(*io_space_map, descriptors.len()).copy_from_slice(&descriptors);
  }

  Status::SUCCESS
}

extern "efiapi" fn dispatch() -> Status {
  match core_dispatcher() {
    Err(err) => return err,
    Ok(()) => return efi::Status::SUCCESS,
  }
}

extern "efiapi" fn schedule(_firmware_volume_handle: Handle, _file_name: *const Guid) -> Status {
  todo!();
  //Status::UNSUPPORTED
}

extern "efiapi" fn trust(_firmware_volume_handle: Handle, _file_name: *const Guid) -> Status {
  todo!();
  //Status::UNSUPPORTED
}

extern "efiapi" fn process_firmware_volume(
  _firmware_volume_header: *const c_void,
  _size: u32,
  _firmware_volume_handle: *mut Handle,
) -> Status {
  todo!();
  //Status::UNSUPPORTED
}

// This routine passes attribute changes into the CPU architectural protocol so that the CPU attribute states for memory
// are consistent with the GCD view. If the CPU arch protocol is not available yet, then this routine does nothing.
fn cpu_set_memory_space_attributes(
  base_address: PhysicalAddress,
  length: u64,
  attributes: u64,
) -> Result<(), efi::Status> {
  //Note: matches EDK C reference behavior, but caller really should pass all intended attributes rather than assuming behavior.
  if attributes == 0 {
    return Ok(());
  }

  let cpu_arch_ptr = CPU_ARCH_PTR.load(Ordering::SeqCst);
  if let Some(cpu_arch) = unsafe { cpu_arch_ptr.as_mut() } {
    let status = (cpu_arch.set_memory_attributes)(cpu_arch_ptr, base_address, length, attributes);
    if status.is_error() {
      println!(
        "Warning: cpu_arch.set_memory_attributes({:x?},{:x?},{:x?}) returned: {:x?}",
        base_address, length, attributes, status
      );
      return Err(status);
    }
  }
  Ok(())
}

//This call back is invoked when the CPU Architectural protocol is installed. It updates the global atomic CPU_ARCH_PTR
//to point to the CPU architectural protocol interface.
extern "efiapi" fn cpu_arch_available(event: r_efi::efi::Event, _context: *mut c_void) {
  match PROTOCOL_DB.locate_protocol(cpu_arch::PROTOCOL) {
    Ok(cpu_arch_ptr) => {
      CPU_ARCH_PTR.store(cpu_arch_ptr as *mut cpu_arch::Protocol, Ordering::SeqCst);
      EVENT_DB.close_event(event).unwrap();
    }
    Err(err) => panic!("Unable to locate timer arch: {:?}", err),
  }
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

  //set up call back for cpu arch protocol installation.
  let event = EVENT_DB
    .create_event(EVT_NOTIFY_SIGNAL, TPL_CALLBACK, Some(cpu_arch_available), None, None)
    .expect("Failed to create timer available callback.");

  PROTOCOL_DB
    .register_protocol_notify(cpu_arch::PROTOCOL, event)
    .expect("Failed to register protocol notify on timer arch callback.");
}
