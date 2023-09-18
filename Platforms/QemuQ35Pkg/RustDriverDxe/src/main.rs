#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![allow(non_snake_case)]

use core::panic::PanicInfo;
use r_efi::efi::Status;
use serial_print_dxe::println;

#[no_mangle]
pub extern "efiapi" fn efi_main(
  _image_handle: *const core::ffi::c_void,
  system_table: *const r_efi::system::SystemTable,
) -> u64 {
  println!("Hello World{}", "!");

  let g_st = unsafe { &(*system_table) };
  let g_rt = unsafe { &*(g_st.runtime_services) };
  let g_bs = unsafe { &*(g_st.boot_services) };

  println!("System Table sig: {:x?}", g_st.hdr);
  println!("Runtime Services sig: {:x?}", g_rt.hdr);
  println!("Boot Services sig: {:x?}", g_bs.hdr);

  Status::SUCCESS.as_usize() as u64
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
  println!("{}", info);
  loop {}
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
  dxe_rust::test_panic_handler(info)
}
