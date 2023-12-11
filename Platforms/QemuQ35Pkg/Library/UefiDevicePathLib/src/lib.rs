//! UEFI DevicePath utilities
//!
//! This library provides various utilities for interacting with UEFI device paths.
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
#![no_std]
#![feature(pointer_byte_offsets)]

extern crate alloc;

use alloc::{boxed::Box, vec};
use core::{ptr::slice_from_raw_parts, slice::from_raw_parts};

use r_efi::efi;

/// Returns the count of nodes and size (in bytes) of the given device path.
///
/// count and size outputs both include the terminating end node.
///
/// ## Safety
///
/// device_path input must be a valid pointer to a well-formed device path.
///
/// ## Examples
///
/// ```
/// #![feature(pointer_byte_offsets)]
/// use uefi_device_path_lib::device_path_node_count;
/// use r_efi::efi;
/// let device_path_bytes = [
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6,  //length[0]
///   0x0,  //length[1]
///   0x0,  //func
///   0x1C, //device
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6, //length[0]
///   0x0, //length[1]
///   0x0, //func
///   0x0, //device
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6, //length[0]
///   0x0, //length[1]
///   0x2, //func
///   0x0, //device
///   efi::protocols::device_path::TYPE_END,
///   efi::protocols::device_path::End::SUBTYPE_ENTIRE,
///   0x4,  //length[0]
///   0x00, //length[1]
/// ];
/// let device_path_ptr = device_path_bytes.as_ptr() as *const efi::protocols::device_path::Protocol;
/// let (nodes, length) = device_path_node_count(device_path_ptr);
/// assert_eq!(nodes, 4);
/// assert_eq!(length, device_path_bytes.len());
/// ```
///
pub fn device_path_node_count(device_path: *const efi::protocols::device_path::Protocol) -> (usize, usize) {
  let mut node_count = 0;
  let mut dev_path_size: usize = 0;
  let mut current_node_ptr = device_path;
  loop {
    let current_node = unsafe { *current_node_ptr };
    let current_length: usize = u16::from_le_bytes(current_node.length).try_into().unwrap();
    node_count += 1;
    dev_path_size += current_length;

    if current_node.r#type == efi::protocols::device_path::TYPE_END {
      break;
    }
    current_node_ptr = unsafe { current_node_ptr.byte_offset(current_length.try_into().unwrap()) };
  }
  (node_count, dev_path_size)
}

/// Copies the device path from the given pointer into a Boxed [u8] slice.
pub fn copy_device_path_to_boxed_slice(device_path: *const efi::protocols::device_path::Protocol) -> Box<[u8]> {
  let (_, byte_count) = device_path_node_count(device_path);
  let mut dest_path = vec![0u8; byte_count];
  unsafe {
    dest_path.copy_from_slice(from_raw_parts(device_path as *const u8, byte_count));
  }
  dest_path.into_boxed_slice()
}

/// Computes the remaining device path and the number of nodes in common for two device paths.
///
/// if device path `a` is a prefix of or identical to device path `b`, result is Some(pointer to the portion of
/// device path `b` that remains after removing device path `a`, nodes_in_common).
/// if device path `a` is not a prefix of device path `b` (i.e. the first node in `a` that is different from
/// `b` is not an end node), then the result is None.
///
/// note: nodes_in_common does not count the terminating end node.
///
/// ## Safety
///
/// a and b inputs must be a valid pointers to well-formed device paths.
///
/// ## Examples
///
/// ```
/// #![feature(pointer_byte_offsets)]
/// use uefi_device_path_lib::{device_path_node_count, remaining_device_path};
/// use core::mem::size_of;
/// use r_efi::efi;
/// let device_path_a_bytes = [
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6,  //length[0]
///   0x0,  //length[1]
///   0x0,  //func
///   0x1C, //device
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6, //length[0]
///   0x0, //length[1]
///   0x0, //func
///   0x0, //device
///   efi::protocols::device_path::TYPE_END,
///   efi::protocols::device_path::End::SUBTYPE_ENTIRE,
///   0x4,  //length[0]
///   0x00, //length[1]
/// ];
/// let device_path_a = device_path_a_bytes.as_ptr() as *const efi::protocols::device_path::Protocol;
/// let device_path_b_bytes = [
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6,  //length[0]
///   0x0,  //length[1]
///   0x0,  //func
///   0x1C, //device
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6, //length[0]
///   0x0, //length[1]
///   0x0, //func
///   0x0, //device
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6, //length[0]
///   0x0, //length[1]
///   0x2, //func
///   0x0, //device
///   efi::protocols::device_path::TYPE_END,
///   efi::protocols::device_path::End::SUBTYPE_ENTIRE,
///   0x4,  //length[0]
///   0x00, //length[1]
/// ];
/// let device_path_b = device_path_b_bytes.as_ptr() as *const efi::protocols::device_path::Protocol;
/// let device_path_c_bytes = [
///   efi::protocols::device_path::TYPE_HARDWARE,
///   efi::protocols::device_path::Hardware::SUBTYPE_PCI,
///   0x6,  //length[0]
///   0x0,  //length[1]
///   0x0,  //func
///   0x0A, //device
///   efi::protocols::device_path::TYPE_END,
///   efi::protocols::device_path::End::SUBTYPE_ENTIRE,
///   0x4,  //length[0]
///   0x00, //length[1]
/// ];
/// let device_path_c = device_path_c_bytes.as_ptr() as *const efi::protocols::device_path::Protocol;
/// // a is a prefix of b.
/// let result = remaining_device_path(device_path_a, device_path_b);
/// assert!(result.is_some());
/// let result = result.unwrap();
/// // the remaining device path of b after going past the prefix in a should start at the size of a in bytes minus the size of the end node.
/// let a_path_length = device_path_node_count(device_path_a);
/// let offset = a_path_length.1 - size_of::<efi::protocols::device_path::End>();
/// let offset = offset.try_into().unwrap();
/// let expected_ptr =
///   unsafe { device_path_b_bytes.as_ptr().byte_offset(offset) } as *const efi::protocols::device_path::Protocol;
/// assert_eq!(result, (expected_ptr, a_path_length.0 - 1));

/// //b is equal to b.
/// let result = remaining_device_path(device_path_b, device_path_b);
/// assert!(result.is_some());
/// let result = result.unwrap();
/// let b_path_length = device_path_node_count(device_path_b);
/// let offset = b_path_length.1 - size_of::<efi::protocols::device_path::End>();
/// let offset = offset.try_into().unwrap();
/// let expected_ptr =
///   unsafe { device_path_b_bytes.as_ptr().byte_offset(offset) } as *const efi::protocols::device_path::Protocol;
/// assert_eq!(result, (expected_ptr, b_path_length.0 - 1));

/// //a is not a prefix of c.
/// let result = remaining_device_path(device_path_a, device_path_c);
/// assert!(result.is_none());

/// //b is not a prefix of a.
/// let result = remaining_device_path(device_path_b, device_path_a);
/// assert!(result.is_none());
/// ```
///

pub fn remaining_device_path(
  a: *const efi::protocols::device_path::Protocol,
  b: *const efi::protocols::device_path::Protocol,
) -> Option<(*const efi::protocols::device_path::Protocol, usize)> {
  let mut a_ptr = a;
  let mut b_ptr = b;
  let mut node_count = 0;
  loop {
    let a_node = unsafe { *a_ptr };
    let b_node = unsafe { *b_ptr };

    if a_node.r#type == efi::protocols::device_path::TYPE_END
      && a_node.sub_type == efi::protocols::device_path::End::SUBTYPE_ENTIRE
    {
      return Some((b_ptr, node_count));
    }

    node_count += 1;

    let a_length: usize = u16::from_le_bytes(a_node.length).try_into().unwrap();
    let b_length: usize = u16::from_le_bytes(b_node.length).try_into().unwrap();
    let a_slice = unsafe { slice_from_raw_parts(a_ptr as *const u8, a_length).as_ref() };
    let b_slice = unsafe { slice_from_raw_parts(b_ptr as *const u8, b_length).as_ref() };

    if a_slice != b_slice {
      return None;
    }

    a_ptr = unsafe { a_ptr.byte_offset(a_length.try_into().unwrap()) };
    b_ptr = unsafe { b_ptr.byte_offset(b_length.try_into().unwrap()) };
  }
}

#[cfg(test)]
mod tests {
  use core::mem::size_of;

  use efi::protocols::device_path::{End, Hardware, TYPE_END, TYPE_HARDWARE};

  use super::*;

  #[test]
  fn device_path_node_count_should_return_the_right_number_of_nodes_and_length() {
    //build a device path as a byte array for the test.
    let device_path_bytes = [
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6,  //length[0]
      0x0,  //length[1]
      0x0,  //func
      0x1C, //device
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6, //length[0]
      0x0, //length[1]
      0x0, //func
      0x0, //device
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6, //length[0]
      0x0, //length[1]
      0x2, //func
      0x0, //device
      TYPE_END,
      End::SUBTYPE_ENTIRE,
      0x4,  //length[0]
      0x00, //length[1]
    ];
    let device_path_ptr = device_path_bytes.as_ptr() as *const efi::protocols::device_path::Protocol;
    let (nodes, length) = device_path_node_count(device_path_ptr);
    assert_eq!(nodes, 4);
    assert_eq!(length, device_path_bytes.len());
  }

  #[test]
  fn remaining_device_path_should_return_remaining_device_path() {
    //build device paths as byte arrays for the tests.
    let device_path_a_bytes = [
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6,  //length[0]
      0x0,  //length[1]
      0x0,  //func
      0x1C, //device
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6, //length[0]
      0x0, //length[1]
      0x0, //func
      0x0, //device
      TYPE_END,
      End::SUBTYPE_ENTIRE,
      0x4,  //length[0]
      0x00, //length[1]
    ];
    let device_path_a = device_path_a_bytes.as_ptr() as *const efi::protocols::device_path::Protocol;
    let device_path_b_bytes = [
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6,  //length[0]
      0x0,  //length[1]
      0x0,  //func
      0x1C, //device
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6, //length[0]
      0x0, //length[1]
      0x0, //func
      0x0, //device
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6, //length[0]
      0x0, //length[1]
      0x2, //func
      0x0, //device
      TYPE_END,
      End::SUBTYPE_ENTIRE,
      0x4,  //length[0]
      0x00, //length[1]
    ];
    let device_path_b = device_path_b_bytes.as_ptr() as *const efi::protocols::device_path::Protocol;
    let device_path_c_bytes = [
      TYPE_HARDWARE,
      Hardware::SUBTYPE_PCI,
      0x6,  //length[0]
      0x0,  //length[1]
      0x0,  //func
      0x0A, //device
      TYPE_END,
      End::SUBTYPE_ENTIRE,
      0x4,  //length[0]
      0x00, //length[1]
    ];
    let device_path_c = device_path_c_bytes.as_ptr() as *const efi::protocols::device_path::Protocol;

    // a is a prefix of b.
    let result = remaining_device_path(device_path_a, device_path_b);
    assert!(result.is_some());
    let result = result.unwrap();
    // the remaining device path of b after going past the prefix in a should start at the size of a in bytes minus the size of the end node.
    let a_path_length = device_path_node_count(device_path_a);
    let offset = a_path_length.1 - size_of::<efi::protocols::device_path::End>();
    let offset = offset.try_into().unwrap();
    let expected_ptr =
      unsafe { device_path_b_bytes.as_ptr().byte_offset(offset) } as *const efi::protocols::device_path::Protocol;
    assert_eq!(result, (expected_ptr, a_path_length.0 - 1));

    //b is equal to b.
    let result = remaining_device_path(device_path_b, device_path_b);
    assert!(result.is_some());
    let result = result.unwrap();
    let b_path_length = device_path_node_count(device_path_b);
    let offset = b_path_length.1 - size_of::<efi::protocols::device_path::End>();
    let offset = offset.try_into().unwrap();
    let expected_ptr =
      unsafe { device_path_b_bytes.as_ptr().byte_offset(offset) } as *const efi::protocols::device_path::Protocol;
    assert_eq!(result, (expected_ptr, b_path_length.0 - 1));

    //a is not a prefix of c.
    let result = remaining_device_path(device_path_a, device_path_c);
    assert!(result.is_none());

    //b is not a prefix of a.
    let result = remaining_device_path(device_path_b, device_path_a);
    assert!(result.is_none());
  }
}
