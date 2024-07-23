//! Serial Print Dxe
//!
//! Implements print! and prinln! macro support for serial port printing.
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
#![no_std]
#[cfg(not(feature = "std"))]
pub mod serial_port_print;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub use {std::print, std::println};

#[cfg(not(feature = "std"))]
mod no_std_debug {
    #[macro_export]
    macro_rules! print {
    ($fmt:expr) => ($crate::serial_print!($fmt));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!($fmt, $($arg)*));
  }

    #[macro_export]
    macro_rules! println {
    ($fmt:expr) => ($crate::serial_println!($fmt));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_println!($fmt, $($arg)*));
  }
}

#[cfg(test)]
mod tests {

    use crate::print;

    #[test]
    fn test_print() {
        print!("This is a test");
    }
}
