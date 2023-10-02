//! Serial Port Print
//!
//! Implements a serial port instance and creates serial_print!, serial_println! macros for debug prints.
//! Note:
//!     Uses hardcoded Serial ports for debug.
//!     * Q35  -> base = 0x402
//!     * Sbsa -> PL011 = 0x6000_0000 (PcdSerialRegisterBase)
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
#[cfg(target_arch = "x86_64")]
pub mod x86_serial_port;
#[cfg(target_arch = "x86_64")]
pub use x86_serial_port::_print;

#[cfg(target_arch = "aarch64")]
pub mod aarch64_serial_port;
#[cfg(target_arch = "aarch64")]
pub use aarch64_serial_port::_print;

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! serial_print {
  ($($arg:tt)*) => {
    $crate::serial_port_print::_print(format_args!($($arg)*))
  };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! serial_println {
  () => ($crate::serial_print!("\n"));
  ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
  ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
    concat!($fmt, "\n"), $($arg)*));
}
