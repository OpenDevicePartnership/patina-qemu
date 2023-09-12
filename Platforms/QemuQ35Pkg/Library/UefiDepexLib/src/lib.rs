//! UEFI Dependency Expression (DEPEX) support
//!
//! This library provides a parser and evaluator for UEFI dependency expressions.
//!
//! ## Examples and Usage
//!
//! static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
//!
//! let efi_var_arch_prot_uuid = Uuid::from_str("1e5668e2-8481-11d4-bcf1-0080c73c8881").unwrap();
//! let efi_var_arch_prot_guid: Guid = unsafe { core::mem::transmute(*efi_var_arch_prot_uuid.as_bytes()) };
//! let efi_var_write_arch_prot_uuid = Uuid::from_str("6441f818-6362-eb44-5700-7dba31dd2453").unwrap();
//! let efi_var_write_arch_prot_guid: Guid = unsafe { core::mem::transmute(*efi_var_write_arch_prot_uuid.as_bytes()) };
//! let efi_tcg_prot_uuid = Uuid::from_str("f541796d-a62e-4954-a775-9584f61b9cdd").unwrap();
//! let efi_tcg_prot_guid: Guid = unsafe { core::mem::transmute(*efi_tcg_prot_uuid.as_bytes()) };
//! let efi_tree_prot_uuid = Uuid::from_str("607f766c-7455-42be-930b-e4d76db2720f").unwrap();
//! let efi_tree_prot_guid: Guid = unsafe { core::mem::transmute(*efi_tree_prot_uuid.as_bytes()) };
//! let efi_pcd_prot_uuid = Uuid::from_str("13a3f0f6-264a-3ef0-f2e0-dec512342f34").unwrap();
//! let efi_pcd_prot_guid: Guid = unsafe { core::mem::transmute(*efi_pcd_prot_uuid.as_bytes()) };
//! let efi_device_path_utilities_prot_uuid = Uuid::from_str("0379be4e-d706-437d-b037-edb82fb772a4").unwrap();
//! let efi_device_path_utilities_prot_guid: Guid = unsafe { core::mem::transmute(*efi_device_path_utilities_prot_uuid.as_bytes()) };
//!
//! let interface: *mut c_void = 0x1234 as *mut c_void;
//! SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_var_arch_prot_guid, interface).unwrap();
//! SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_var_write_arch_prot_guid, interface).unwrap();
//! SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_tcg_prot_guid, interface).unwrap();
//! SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_tree_prot_guid, interface).unwrap();
//! SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_pcd_prot_guid, interface).unwrap();
//! SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_device_path_utilities_prot_guid, interface).unwrap();
//!
//! println!("Testing DEPEX for TcgMor DXE driver...\n");
//!
//! let expression: &[u8] = &[0x02, 0xE2, 0x68, 0x56, 0x1E, 0x81, 0x84, 0xD4, 0x11, 0xBC, 0xF1, 0x00,
//!                           0x80, 0xC7, 0x3C, 0x88, 0x81, 0x02, 0x18, 0xF8, 0x41, 0x64, 0x62, 0x63, 0x44, 0xEB, 0x57, 0x0,
//!                           0x7D, 0xBA, 0x31, 0xDD, 0x24, 0x53, 0x02, 0x6D, 0x79, 0x41, 0xF5, 0x2E, 0xA6, 0x54, 0x49, 0xA7,
//!                           0x75, 0x95, 0x84, 0xF6, 0x1B, 0x9C, 0xDD, 0x02, 0x6C, 0x76, 0x7F, 0x60, 0x55, 0x74, 0xBE, 0x42,
//!                           0x93, 0x0B, 0xE4, 0xD7, 0x6D, 0xB2, 0x72, 0x0F, 0x04, 0x03, 0x03, 0x02, 0xF6, 0xF0, 0xA3, 0x13,
//!                           0x4A, 0x26, 0xF0, 0x3E, 0xF2, 0xE0, 0xDE, 0xC5, 0x12, 0x34, 0x2F, 0x34, 0x02, 0x4E, 0xBE, 0x79,
//!                           0x03, 0x06, 0xD7, 0x7D, 0x43, 0xB0, 0x37, 0xED, 0xB8, 0x2F, 0xB7, 0x72, 0xA4, 0x03, 0x03, 0x08
//!                           ];
//! let mut depex = Depex::new(expression.to_vec());
//! println!("DEPEX debug dump:\n\n{:?}\n", depex);
//!
//! println!("DEPEX opcode dump:\n");
//! for opcode in &mut depex {
//!   println!("opcode is {:?}", opcode);
//! }
//! println!();
//!
//! println!("DEPEX evaluation is : {}\n", depex.eval(&SPIN_LOCKED_PROTOCOL_DB));
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};
use core::{mem, slice};
use r_efi::efi;
use uefi_protocol_db_lib::SpinLockedProtocolDb;
use uuid::Uuid;

/// The size of a GUID in bytes
const GUID_SIZE: usize = mem::size_of::<r_efi::efi::Guid>();

/// The initial size of the dependency expression stack in bytes
const DEPEX_STACK_SIZE_INCREMENT: usize = 0x100;

/// A UEFI dependency expression (DEPEX) opcode
#[derive(Debug, PartialEq)]
pub enum Opcode {
  /// If present, this must be the first and only opcode,
  /// may be used by DXE and SMM drivers.
  Before,
  /// If present, this must be the first and only opcode,
  /// may be used by DXE and SMM drivers.
  After,
  /// A Push opcode is followed by a GUID.
  Push(Option<Uuid>),
  /// A logical AND operation of the two operands on the top
  /// of the stack.
  And,
  /// A logical OR operation of the two operands on the top
  /// of the stack.
  Or,
  /// A logical NOT operation of the operand on the top of
  /// the stack.
  Not,
  /// Pushes a true value onto the stack.
  True,
  /// Pushes a false value onto the stack.
  False,
  /// The End opcode is the last opcode in the expression.
  End,
  /// If present, this must be the first opcode in the expression.
  /// Used to schedule on request.
  Sor,
  /// Used to dynamically patch the dependency expression
  /// to save time. A EFI_DEP_PUSH is evaluated once and
  /// replaced with EFI_DEP_REPLACE_TRUE.
  ReplaceTrue,
  /// An unknown opcode. Indicates an unrecognized opcode
  /// that should be treated as an error during evaluation.
  Unknown,
}

/// Converts a UUID to an EFI GUID.
fn guid_from_uuid(uuid: &Uuid) -> efi::Guid {
  let bytes = uuid.as_bytes();
  efi::Guid::from_fields(
    u32::from_le_bytes(bytes[0..=3].try_into().unwrap()),
    u16::from_le_bytes(bytes[4..=5].try_into().unwrap()),
    u16::from_le_bytes(bytes[6..=7].try_into().unwrap()),
    bytes[8],
    bytes[9],
    &bytes[10..=15].try_into().unwrap(),
  )
}

/// Converts a byte slice to a GUID.
fn uuid_from_slice(slice: &[u8]) -> Option<Uuid> {
  if slice.len() != GUID_SIZE {
    return None;
  }

  match Uuid::from_slice_le(slice) {
    Ok(uuid) => Some(uuid),
    Err(_) => None,
  }
}

impl<'a> From<&'a [u8]> for Opcode {
  /// Creates an Opcode from a byte slice.
  fn from(bytes: &'a [u8]) -> Self {
    match bytes[0] {
      0x00 => Opcode::Before,
      0x01 => Opcode::After,
      0x02 => Opcode::Push(if bytes.len() > GUID_SIZE + 1 { uuid_from_slice(&bytes[1..1 + GUID_SIZE]) } else { None }),
      0x03 => Opcode::And,
      0x04 => Opcode::Or,
      0x05 => Opcode::Not,
      0x06 => Opcode::True,
      0x07 => Opcode::False,
      0x08 => Opcode::End,
      0x09 => Opcode::Sor,
      0xFF => Opcode::ReplaceTrue,
      _ => Opcode::Unknown,
    }
  }
}

#[derive(Debug)]
/// A UEFI dependency expression (DEPEX)
pub struct Depex {
  expression: Vec<u8>,
  index: usize,
}

impl Depex {
  /// Creates a new DEPEX from a byte vector.
  pub fn new(expression: Vec<u8>) -> Self {
    Depex { expression, index: 0 }
  }

  /// Evaluates a DEPEX expression.
  pub fn eval(mut self, protocol_db: &SpinLockedProtocolDb) -> bool {
    let mut stack = vec![false; DEPEX_STACK_SIZE_INCREMENT];

    for (index, opcode) in self.enumerate() {
      match opcode {
        Opcode::Before | Opcode::After => {
          debug_assert!(false, "Exiting early due to an unexpected BEFORE or AFTER.");
          return false;
        }
        Opcode::Sor => {
          if index != 0 {
            debug_assert!(false, "Exiting early due to an unexpected SOR.");
            return false;
          }
        }
        Opcode::Push(guid) => {
          if guid.is_none() {
            debug_assert!(false, "Exiting early because a PUSH operand is not followed by a GUID.");
            return false;
          }

          let result = protocol_db.locate_protocol(guid_from_uuid(&guid.unwrap()));
          if result.is_ok() {
            // Todo: Replace opcode with `EFI_DEP_REPLACE_TRUE` later
            stack.push(true);
          } else {
            stack.push(false);
          }
        }
        Opcode::And => {
          let operator1 = stack.pop().unwrap_or(false);
          let operator2 = stack.pop().unwrap_or(false);
          stack.push(operator1 && operator2);
        }
        Opcode::Or => {
          let operator1 = stack.pop().unwrap_or(false);
          let operator2 = stack.pop().unwrap_or(false);
          stack.push(operator1 || operator2);
        }
        Opcode::Not => {
          let operator = stack.pop().unwrap_or(false);
          stack.push(!operator);
        }
        Opcode::True => {
          stack.push(true);
        }
        Opcode::False => {
          stack.push(false);
        }
        Opcode::End => {
          let operator = stack.pop().unwrap_or(false);
          return operator;
        }
        Opcode::ReplaceTrue => {
          debug_assert!(false, "The ReplaceTrue operation is not supported (yet).");
          return false;
        }
        Opcode::Unknown => {
          debug_assert!(false, "Exiting early due to an unknown opcode.");
          return false;
        }
      }
    }
    false
  }
}

impl Iterator for &mut Depex {
  type Item = Opcode;

  /// Iterates over the DEPEX expression, returning the next Opcode.
  fn next(&mut self) -> Option<Opcode> {
    if self.index == self.expression.len() {
      self.index = 0;
      return None;
    }

    let mut opcode = Opcode::from(slice::from_ref(&self.expression[self.index]));

    match opcode {
      Opcode::Push(None) => {
        let guid_start = self.index + 1;
        opcode = Opcode::Push(uuid_from_slice(&self.expression[guid_start..guid_start + GUID_SIZE]));
        self.index += GUID_SIZE;
      }
      Opcode::ReplaceTrue => {
        self.index += GUID_SIZE;
      }
      _ => {}
    }

    self.index += mem::size_of::<u8>();
    Some(opcode)
  }
}

#[cfg(test)]
mod tests {
  extern crate std;
  use core::{ffi::c_void, str::FromStr};
  use r_efi::efi::Guid;
  use std::println;
  use uefi_protocol_db_lib::SpinLockedProtocolDb;
  use uuid::Uuid;

  use super::*;

  #[test]
  /// Tests a DEPEX expression with AND and OR operations that should evaluate to true when all protocols are installed.
  fn all_protocols_installed_or_and_should_eval_true() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let efi_var_arch_prot_uuid = Uuid::from_str("1e5668e2-8481-11d4-bcf1-0080c73c8881").unwrap();
    let efi_var_arch_prot_guid: Guid = unsafe { core::mem::transmute(*efi_var_arch_prot_uuid.as_bytes()) };
    let efi_var_write_arch_prot_uuid = Uuid::from_str("6441f818-6362-eb44-5700-7dba31dd2453").unwrap();
    let efi_var_write_arch_prot_guid: Guid = unsafe { core::mem::transmute(*efi_var_write_arch_prot_uuid.as_bytes()) };
    let efi_tcg_prot_uuid = Uuid::from_str("f541796d-a62e-4954-a775-9584f61b9cdd").unwrap();
    let efi_tcg_prot_guid: Guid = unsafe { core::mem::transmute(*efi_tcg_prot_uuid.as_bytes()) };
    let efi_tree_prot_uuid = Uuid::from_str("607f766c-7455-42be-930b-e4d76db2720f").unwrap();
    let efi_tree_prot_guid: Guid = unsafe { core::mem::transmute(*efi_tree_prot_uuid.as_bytes()) };
    let efi_pcd_prot_uuid = Uuid::from_str("13a3f0f6-264a-3ef0-f2e0-dec512342f34").unwrap();
    let efi_pcd_prot_guid: Guid = unsafe { core::mem::transmute(*efi_pcd_prot_uuid.as_bytes()) };
    let efi_device_path_utilities_prot_uuid = Uuid::from_str("0379be4e-d706-437d-b037-edb82fb772a4").unwrap();
    let efi_device_path_utilities_prot_guid: Guid =
      unsafe { core::mem::transmute(*efi_device_path_utilities_prot_uuid.as_bytes()) };

    let interface: *mut c_void = 0x1234 as *mut c_void;
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_var_arch_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_var_write_arch_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_tcg_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_tree_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_pcd_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_device_path_utilities_prot_guid, interface).unwrap();

    println!("Testing DEPEX for TcgMor DXE driver...\n");

    let expression: &[u8] = &[
      0x02, 0xE2, 0x68, 0x56, 0x1E, 0x81, 0x84, 0xD4, 0x11, 0xBC, 0xF1, 0x00, 0x80, 0xC7, 0x3C, 0x88, 0x81, 0x02, 0x18,
      0xF8, 0x41, 0x64, 0x62, 0x63, 0x44, 0xEB, 0x57, 0x0, 0x7D, 0xBA, 0x31, 0xDD, 0x24, 0x53, 0x02, 0x6D, 0x79, 0x41,
      0xF5, 0x2E, 0xA6, 0x54, 0x49, 0xA7, 0x75, 0x95, 0x84, 0xF6, 0x1B, 0x9C, 0xDD, 0x02, 0x6C, 0x76, 0x7F, 0x60, 0x55,
      0x74, 0xBE, 0x42, 0x93, 0x0B, 0xE4, 0xD7, 0x6D, 0xB2, 0x72, 0x0F, 0x04, 0x03, 0x03, 0x02, 0xF6, 0xF0, 0xA3, 0x13,
      0x4A, 0x26, 0xF0, 0x3E, 0xF2, 0xE0, 0xDE, 0xC5, 0x12, 0x34, 0x2F, 0x34, 0x02, 0x4E, 0xBE, 0x79, 0x03, 0x06, 0xD7,
      0x7D, 0x43, 0xB0, 0x37, 0xED, 0xB8, 0x2F, 0xB7, 0x72, 0xA4, 0x03, 0x03, 0x08,
    ];
    let depex = Depex::new(expression.to_vec());

    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), true);
  }
}
