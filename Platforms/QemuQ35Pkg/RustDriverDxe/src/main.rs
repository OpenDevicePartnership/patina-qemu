#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(abi_efiapi)]

use dxe_rust::{println, serial_println};
use core::panic::PanicInfo;

#[no_mangle]
pub extern "efiapi" fn efi_main(
    _hob_list: *const u8
    ) -> u64 {
    serial_println!("Hello World{}", "!");

    0x8000_0000_0000_0003 as u64
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("{}", info);
    loop {}
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    dxe_rust::test_panic_handler(info)
}
