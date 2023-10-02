//! aarch64 serial port
//!
//! Implements an aarch64 serial port instance. Used for debug prints in the QemuSbsaPkg.
//!
//! Portions derived from uart_16550::SerialPort
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
use core::{fmt, ptr};
use lazy_static::lazy_static;
use spin::Mutex;

pub struct SerialPortHandle {
  port: *mut u8,
}
unsafe impl Send for SerialPortHandle {}
unsafe impl Sync for SerialPortHandle {}

impl SerialPortHandle {
  pub const fn new(port: *mut u8) -> Self {
    Self { port }
  }

  /// Writes to the port.
  ///
  /// ## Safety
  ///
  /// This function is unsafe because the I/O port could have side effects that violate memory
  /// safety.
  #[inline]
  pub unsafe fn write(&mut self, byte: u8) {
    unsafe {
      ptr::write_volatile(self.port, byte);
    }
  }
}

impl fmt::Write for SerialPortHandle {
  fn write_str(&mut self, s: &str) -> fmt::Result {
    for byte in s.bytes() {
      unsafe {
        self.write(byte);
      }
    }
    Ok(())
  }
}

lazy_static! {
  pub static ref UART0: Mutex<SerialPortHandle> = {
    // 0x6000_0000 is the PL011 PcdSerialRegisterBase value
    let serial_port = SerialPortHandle::new(0x6000_0000 as *mut u8);
    Mutex::new(serial_port)
  };
}

#[cfg(not(test))]
#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
  use core::fmt::Write;

  UART0.lock().write_fmt(args).expect("Printing to serial failed");
}

#[cfg(test)]
pub fn _print(args: ::core::fmt::Arguments) {
  extern crate alloc;
  use alloc::vec::Vec;

  let mut vec = Vec::new();
  vec.push(args.as_str());
  assert_eq!(vec[0], args.as_str())
}
