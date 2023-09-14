#![cfg_attr(not(test), no_std)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(const_option)]
#![feature(allocator_api, new_uninit)]
#![feature(slice_ptr_get)]
#![feature(c_variadic)]
#![feature(pointer_byte_offsets)]
use uefi_gcd_lib::gcd::SpinLockedGcd;

extern crate alloc;
pub mod allocator;
pub mod dispatcher;
pub mod driver_services;
pub mod dxe_services;
pub mod events;
pub mod fv;
pub mod gdt;
pub mod image;
#[cfg(target_os = "uefi")]
pub mod interrupts;
pub mod misc_boot_services;
pub mod physical_memory;
pub mod protocols;
pub mod serial;
pub mod systemtables;
pub mod utility;

pub static GCD: SpinLockedGcd = SpinLockedGcd::new();

#[cfg(target_os = "uefi")]
pub fn init() {
  gdt::init();
  interrupts::init_idt();
  x86_64::instructions::interrupts::enable();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
  Success = 0x10,
  Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
  use x86_64::instructions::port::Port;

  unsafe {
    let mut port = Port::new(0xf4);
    port.write(exit_code as u32);
  }
}

pub fn hlt_loop() -> ! {
  loop {
    x86_64::instructions::hlt();
  }
}

#[macro_export]
macro_rules! println {
    ($fmt:expr) => ($crate::serial_println!($fmt));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_println!($fmt, $($arg)*));
}

#[macro_export]
macro_rules! print {
    ($fmt:expr) => ($crate::serial_print!($fmt));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!($fmt, $($arg)*));
}
