use alloc::{boxed::Box, vec};
use core::{
  ffi::c_void,
  slice::{from_raw_parts, from_raw_parts_mut},
};
use r_efi::{
  efi::{Guid, Status},
  system::{BootServices, ConfigurationTable},
};

use crate::{
  allocator::EFI_RUNTIME_SERVICES_DATA_ALLOCATOR,
  events::EVENT_DB,
  systemtables::{EfiSystemTable, SYSTEM_TABLE},
};

extern "efiapi" fn calculate_crc32(data: *mut c_void, data_size: usize, crc_32: *mut u32) -> Status {
  if data.is_null() || data_size == 0 || crc_32.is_null() {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  unsafe {
    let buffer = from_raw_parts(data as *mut u8, data_size);
    crc_32.write(crc32fast::hash(buffer));
  }

  r_efi::efi::Status::SUCCESS
}

pub fn core_install_configuration_table(
  vendor_guid: Guid,
  vendor_table: Option<&mut c_void>,
  efi_system_table: &mut EfiSystemTable,
) -> Result<(), Status> {
  let system_table = efi_system_table.as_mut();
  //if a table is already present, reconstruct it from the pointer and length in the st.
  let old_cfg_table = if system_table.configuration_table.is_null() {
    assert_eq!(system_table.number_of_table_entries, 0);
    None
  } else {
    let ct_slice_box = unsafe {
      Box::from_raw_in(
        from_raw_parts_mut(system_table.configuration_table, system_table.number_of_table_entries),
        &EFI_RUNTIME_SERVICES_DATA_ALLOCATOR,
      )
    };
    Some(ct_slice_box)
  };

  // construct the new table contents as a vector.
  let new_table = match old_cfg_table {
    Some(cfg_table) => {
      // a configuration table list is already present.
      let mut current_table = cfg_table.to_vec();
      let existing_entry = current_table.iter_mut().find(|x| x.vendor_guid == vendor_guid);
      if let Some(vendor_table) = vendor_table {
        //vendor_table is some; we are adding or modifying an entry.
        if let Some(entry) = existing_entry {
          //entry exists, modify it.
          entry.vendor_table = vendor_table;
        } else {
          //entry doesn't exist, add it.
          current_table.push(ConfigurationTable { vendor_guid: vendor_guid, vendor_table: vendor_table });
        }
      } else {
        //vendor_table is none; we are deleting an entry.
        if let Some(_entry) = existing_entry {
          //entry exists, we can delete it
          current_table.retain(|x| x.vendor_guid != vendor_guid);
        } else {
          //entry does not exist, we can't delete it. We have to put the original box back
          //in the config table so it doesn't get dropped though. Pointer should be the same
          //so we should not need to recompute CRC.
          system_table.configuration_table = Box::into_raw(cfg_table) as *mut ConfigurationTable;
          return Err(r_efi::efi::Status::NOT_FOUND);
        }
      }
      current_table
    }
    None => {
      //config table list doesn't exist.
      if let Some(table) = vendor_table {
        // table is some, meaning we should create the list and add this as the new entry.
        vec![ConfigurationTable { vendor_guid, vendor_table: table }]
      } else {
        //table is none, but can't delete a table entry in a list that doesn't exist.
        //since the list doesn't exist, we can leave the (null) pointer in the st alone.
        return Err(r_efi::efi::Status::NOT_FOUND);
      }
    }
  };

  if new_table.len() == 0 {
    // if empty, just set config table ptr to null
    system_table.number_of_table_entries = 0;
    system_table.configuration_table = core::ptr::null_mut();
  } else {
    //Box up the new table and put it in the system table. The old table (if any) will be dropped
    //when old_cfg_table goes out of scope at the end of the function.
    system_table.number_of_table_entries = new_table.len();
    let new_table = new_table.to_vec_in(&EFI_RUNTIME_SERVICES_DATA_ALLOCATOR).into_boxed_slice();
    system_table.configuration_table = Box::into_raw(new_table) as *mut ConfigurationTable;
  }
  //since we modified the system table, re-calculate CRC.
  efi_system_table.checksum();

  //signal the table guid as an event group
  EVENT_DB.signal_group(vendor_guid);

  Ok(())
}

extern "efiapi" fn install_configuration_table(table_guid: *mut Guid, table: *mut c_void) -> Status {
  if table_guid.is_null() {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  let table_guid = unsafe { *table_guid };
  let table = unsafe { table.as_mut() };

  let mut st_guard = SYSTEM_TABLE.lock();
  let st = st_guard.as_mut().expect("System table support not initialized");

  match core_install_configuration_table(table_guid, table, st) {
    Err(err) => err,
    Ok(()) => r_efi::efi::Status::SUCCESS,
  }
}

pub fn init_misc_boot_services_support(bs: &mut BootServices) {
  bs.calculate_crc32 = calculate_crc32;
  bs.install_configuration_table = install_configuration_table;
}
