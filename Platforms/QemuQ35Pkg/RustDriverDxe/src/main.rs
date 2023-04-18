#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]

use core::panic::PanicInfo;
use dxe_rust::serial_println;

#[no_mangle]
pub extern "efiapi" fn efi_main(
    _image_handle: *const core::ffi::c_void,
    system_table: *const r_efi::system::SystemTable,
) -> u64 {
    serial_println!("Hello World{}", "!");

    let g_st = unsafe { &(*system_table) };
    let g_rt = unsafe { &*(g_st.runtime_services) };
    let g_bs = unsafe { &*(g_st.boot_services) };

    serial_println!("System Table sig: {:x?}", g_st.hdr);
    serial_println!("Runtime Services sig: {:x?}", g_rt.hdr);
    serial_println!("Boot Services sig: {:x?}", g_bs.hdr);

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
