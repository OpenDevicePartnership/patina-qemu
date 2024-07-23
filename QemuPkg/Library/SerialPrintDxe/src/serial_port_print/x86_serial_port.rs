//! x86 serial port
//!
//! Implements an x86_64 serial port instance. Used for debug prints in the QemuQ35Pkg.
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
use lazy_static::lazy_static;
use spin::Mutex;
use uart_16550::SerialPort;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x402) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[cfg(not(test))]
#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        let serial_lock = SERIAL1.try_lock();
        if let Some(mut serial) = serial_lock {
            serial.write_fmt(args).expect("Printing to serial failed");
        }
    });
}

#[cfg(test)]
pub fn _print(args: ::core::fmt::Arguments) {
    extern crate alloc;
    use alloc::vec::Vec;

    let mut vec = Vec::new();
    vec.push(args.as_str());
    assert_eq!(vec[0], args.as_str())
}
