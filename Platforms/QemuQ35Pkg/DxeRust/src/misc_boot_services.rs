use core::{ffi::c_void, slice::from_raw_parts};

use r_efi::{efi::Status, eficall, eficall_abi, system::BootServices};

eficall! {fn calculate_crc32 (data: *mut c_void, data_size: usize, crc_32: *mut u32) -> Status {
  if data.is_null() || data_size == 0 || crc_32.is_null() {
    return r_efi::efi::Status::INVALID_PARAMETER;
  }

  unsafe {
    let buffer = from_raw_parts(data as *mut u8, data_size);
    crc_32.write(crc32fast::hash(buffer));
  }

  r_efi::efi::Status::SUCCESS
}}

pub fn init_misc_boot_services_support(bs: &mut BootServices) {
  bs.calculate_crc32 = calculate_crc32;
}
