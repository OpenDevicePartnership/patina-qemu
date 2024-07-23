#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![allow(non_snake_case)]

use core::panic::PanicInfo;
use r_efi::efi::Status;

#[no_mangle]
pub extern "efiapi" fn efi_main(
    _image_handle: *const core::ffi::c_void,
    _system_table: *const r_efi::system::SystemTable,
) -> u64 {
    Status::SUCCESS.as_usize() as u64
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    dxe_rust::test_panic_handler(info)
}
