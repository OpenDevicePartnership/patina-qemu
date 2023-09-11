//! UEFI Global Coherency Domain Support
//!
//! This library provides an implementation of the PI spec GCD
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
#![no_std]
#![feature(get_many_mut)]
#![feature(is_sorted)]
extern crate alloc;

pub mod gcd;
pub mod memory_block;
pub mod sorted_slice;

#[macro_export]
macro_rules! ensure {
  ($condition:expr, $err:expr) => {{
    if !($condition) {
      error!($err);
    }
  }};
}

#[macro_export]
macro_rules! error {
  ($err:expr) => {{
    return Err($err.into()).into();
  }};
}
