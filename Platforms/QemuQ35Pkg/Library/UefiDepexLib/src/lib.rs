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
#[derive(Debug, Clone, PartialEq)]
pub enum Opcode {
  /// If present, this must be the first and only opcode,
  /// may be used by DXE and SMM drivers.
  Before,
  /// If present, this must be the first and only opcode,
  /// may be used by DXE and SMM drivers.
  After,
  /// A Push opcode is followed by a GUID.
  Push(Option<Uuid>, bool),
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
  /// An unknown opcode. Indicates an unrecognized opcode
  /// that should be treated as an error during evaluation.
  Unknown,
}

/// Converts a UUID to an EFI GUID.
fn guid_from_uuid(uuid: &Uuid) -> efi::Guid {
  let fields = uuid.as_fields();
  efi::Guid::from_fields(fields.0, fields.1, fields.2, fields.3[0], fields.3[1], &fields.3[2..].try_into().unwrap())
}

/// Converts a byte slice to a GUID.
fn uuid_from_slice(slice: &[u8]) -> Option<Uuid> {
  Uuid::from_slice_le(slice).ok()
}

impl<'a> From<&'a [u8]> for Opcode {
  /// Creates an Opcode from a byte slice.
  fn from(bytes: &'a [u8]) -> Self {
    match bytes[0] {
      0x00 => Opcode::Before,
      0x01 => Opcode::After,
      0x02 => {
        Opcode::Push(if bytes.len() > GUID_SIZE + 1 { uuid_from_slice(&bytes[1..1 + GUID_SIZE]) } else { None }, false)
      }
      0x03 => Opcode::And,
      0x04 => Opcode::Or,
      0x05 => Opcode::Not,
      0x06 => Opcode::True,
      0x07 => Opcode::False,
      0x08 => Opcode::End,
      0x09 => Opcode::Sor,
      _ => Opcode::Unknown,
    }
  }
}

#[derive(Debug)]
/// A UEFI dependency expression (DEPEX)
pub struct Depex {
  expression: Vec<Opcode>,
}

impl From<&[u8]> for Depex {
  fn from(value: &[u8]) -> Self {
    let depex_parser = DepexParser::new(value);
    Self { expression: depex_parser.into_iter().collect() }
  }
}

impl From<Vec<u8>> for Depex {
  fn from(value: Vec<u8>) -> Self {
    Self::from(value.as_slice())
  }
}

impl From<&[Opcode]> for Depex {
  fn from(value: &[Opcode]) -> Self {
    Self { expression: value.to_vec() }
  }
}

impl Depex {
  /// Evaluates a DEPEX expression.
  pub fn eval(&mut self, protocol_db: &SpinLockedProtocolDb) -> bool {
    let mut stack = vec![false; DEPEX_STACK_SIZE_INCREMENT];

    for (index, opcode) in self.expression.iter_mut().enumerate() {
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
        Opcode::Push(guid, present) => {
          if guid.is_none() {
            debug_assert!(false, "Exiting early because a PUSH operand is not followed by a GUID.");
            return false;
          }
          if *present {
            stack.push(true)
          } else {
            let result = protocol_db.locate_protocol(guid_from_uuid(&guid.unwrap()));
            if result.is_ok() {
              *present = true;
              stack.push(true);
            } else {
              stack.push(false);
            }
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
        Opcode::Unknown => {
          debug_assert!(false, "Exiting early due to an unknown opcode.");
          return false;
        }
      }
    }
    false
  }
}

struct DepexParser {
  expression: Vec<u8>,
  index: usize,
}

impl DepexParser {
  fn new(expression: &[u8]) -> Self {
    Self { expression: expression.to_vec(), index: 0 }
  }
}

impl Iterator for DepexParser {
  type Item = Opcode;

  /// Iterates over the DEPEX expression, returning the next Opcode.
  fn next(&mut self) -> Option<Opcode> {
    if self.index >= self.expression.len() {
      return None;
    }

    let mut opcode = Opcode::from(slice::from_ref(&self.expression[self.index]));

    if let Opcode::Push(None, _) = opcode {
      let guid_start = self.index + 1;
      opcode = Opcode::Push(uuid_from_slice(&self.expression[guid_start..guid_start + GUID_SIZE]), false);
      self.index += GUID_SIZE;
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
  fn true_should_eval_true() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0x06, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), true);
  }

  #[test]
  fn false_should_eval_false() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0x07, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), false);
  }

  #[test]
  #[should_panic(expected = "Exiting early due to an unexpected BEFORE or AFTER.")]
  fn before_should_eval_false() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0x00, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), false);
  }

  #[test]
  #[should_panic(expected = "Exiting early due to an unexpected BEFORE or AFTER.")]
  fn after_should_eval_false() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0x01, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), false);
  }

  #[test]
  fn sor_first_opcode_should_eval_false() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    // Treated as a no-op, with no other operands, false should be returned
    let mut depex = Depex::from(vec![0x09, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), false);
  }

  #[test]
  fn sor_first_opcode_followed_by_true_should_eval_true() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0x09, 0x06, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), true);
  }

  #[test]
  #[should_panic(expected = "Exiting early due to an unexpected SOR.")]
  fn sor_not_first_opcode_should_eval_false() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0x06, 0x09, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), false);
  }

  #[test]
  #[should_panic(expected = "Exiting early due to an unknown opcode.")]
  fn replacetrue_should_eval_false() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0xFF, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), false);
  }

  #[test]
  #[should_panic(expected = "Exiting early due to an unknown opcode.")]
  fn unknown_opcode_should_return_false() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0xE0, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), false);
  }

  #[test]
  fn not_true_should_eval_false() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0x07, 0x06, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), true);
  }

  #[test]
  fn not_false_should_eval_true() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let mut depex = Depex::from(vec![0x07, 0x05, 0x08]);
    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), true);
  }

  #[test]
  /// Tests a DEPEX expression with all AND operations that should evaluate to true when all protocols are installed.
  ///
  /// This test is based on the following dependency expression:
  ///   PUSH EfiPcdProtocolGuid
  ///   PUSH EfiDevicePathUtilitiesProtocolGuid
  ///   PUSH EfiHiiStringProtocolGuid
  ///   PUSH EfiHiiDatabaseProtocolGuid
  ///   PUSH EfiHiiConfigRoutingProtocolGuid
  ///   PUSH EfiResetArchProtocolGuid
  ///   PUSH EfiVariableWriteArchProtocolGuid
  ///   PUSH EfiVariableArchProtocolGuid
  ///   AND
  ///   AND
  ///   AND
  ///   AND
  ///   AND
  ///   AND
  ///   AND
  ///   END
  fn all_protocols_installed_and_should_eval_true() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let efi_pcd_prot_uuid = Uuid::from_str("13a3f0f6-264a-3ef0-f2e0-dec512342f34").unwrap();
    let efi_pcd_prot_guid: Guid = guid_from_uuid(&efi_pcd_prot_uuid);
    let efi_device_path_utilities_prot_uuid = Uuid::from_str("0379be4e-d706-437d-b037-edb82fb772a4").unwrap();
    let efi_device_path_utilities_prot_guid: Guid = guid_from_uuid(&efi_device_path_utilities_prot_uuid);
    let efi_hii_string_prot_uuid = Uuid::from_str("0fd96974-23aa-4cdc-b9cb-98d17750322a").unwrap();
    let efi_hii_string_prot_guid: Guid = guid_from_uuid(&efi_hii_string_prot_uuid);
    let efi_hii_db_prot_uuid = Uuid::from_str("ef9fc172-a1b2-4693-b327-6d32fc416042").unwrap();
    let efi_hii_db_prot_guid: Guid = guid_from_uuid(&efi_hii_db_prot_uuid);
    let efi_hii_config_routing_prot_uuid = Uuid::from_str("587e72d7-cc50-4f79-8209-ca291fc1a10f").unwrap();
    let efi_hii_config_routing_prot_guid: Guid = guid_from_uuid(&efi_hii_config_routing_prot_uuid);
    let efi_reset_arch_prot_uuid = Uuid::from_str("27cfac88-46cc-11d4-9a38-0090273fc14d").unwrap();
    let efi_reset_arch_prot_guid: Guid = guid_from_uuid(&efi_reset_arch_prot_uuid);
    let efi_var_write_arch_prot_uuid = Uuid::from_str("6441f818-6362-eb44-5700-7dba31dd2453").unwrap();
    let efi_var_write_arch_prot_guid: Guid = guid_from_uuid(&efi_var_write_arch_prot_uuid);
    let efi_var_arch_prot_uuid = Uuid::from_str("1e5668e2-8481-11d4-bcf1-0080c73c8881").unwrap();
    let efi_var_arch_prot_guid: Guid = guid_from_uuid(&efi_var_arch_prot_uuid);

    let interface: *mut c_void = 0x1234 as *mut c_void;
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_pcd_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_device_path_utilities_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_hii_string_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_hii_db_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_hii_config_routing_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_reset_arch_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_var_write_arch_prot_guid, interface).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, efi_var_arch_prot_guid, interface).unwrap();

    println!("Testing DEPEX for BdsDxe DXE driver...\n");

    let expression: &[u8] = &[
      0x02, 0xF6, 0xF0, 0xA3, 0x13, 0x4A, 0x26, 0xF0, 0x3E, 0xF2, 0xE0, 0xDE, 0xC5, 0x12, 0x34, 0x2F, 0x34, 0x02, 0x4E,
      0xBE, 0x79, 0x03, 0x06, 0xD7, 0x7D, 0x43, 0xB0, 0x37, 0xED, 0xB8, 0x2F, 0xB7, 0x72, 0xA4, 0x02, 0x74, 0x69, 0xD9,
      0x0F, 0xAA, 0x23, 0xDC, 0x4C, 0xB9, 0xCB, 0x98, 0xD1, 0x77, 0x50, 0x32, 0x2A, 0x02, 0x72, 0xC1, 0x9F, 0xEF, 0xB2,
      0xA1, 0x93, 0x46, 0xB3, 0x27, 0x6D, 0x32, 0xFC, 0x41, 0x60, 0x42, 0x02, 0xD7, 0x72, 0x7E, 0x58, 0x50, 0xCC, 0x79,
      0x4F, 0x82, 0x09, 0xCA, 0x29, 0x1F, 0xC1, 0xA1, 0x0F, 0x02, 0x88, 0xAC, 0xCF, 0x27, 0xCC, 0x46, 0xD4, 0x11, 0x9A,
      0x38, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0x4D, 0x02, 0x18, 0xF8, 0x41, 0x64, 0x62, 0x63, 0x44, 0xEB, 0x57, 0x00, 0x7D,
      0xBA, 0x31, 0xDD, 0x24, 0x53, 0x02, 0xE2, 0x68, 0x56, 0x1E, 0x81, 0x84, 0xD4, 0x11, 0xBC, 0xF1, 0x00, 0x80, 0xC7,
      0x3C, 0x88, 0x81, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x08,
    ];
    let mut depex = Depex::from(expression.to_vec());

    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), true);
  }

  #[test]
  /// Tests a DEPEX expression with AND and OR operations that should evaluate to true when all protocols are installed.
  ///
  /// This test is based on the following dependency expression:
  ///   PUSH EfiVariableArchProtocolGuid
  ///   PUSH EfiVariableWriteArchProtocolGuid
  ///   PUSH EfiTcgProtocolGuid
  ///   PUSH EfiTrEEProtocolGuid
  ///   OR
  ///   AND
  ///   AND
  ///   PUSH EfiPcdProtocolGuid
  ///   PUSH EfiDevicePathUtilitiesProtocolGuid
  ///   AND
  ///   AND
  ///   END
  fn all_protocols_installed_or_and_should_eval_true() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let efi_var_arch_prot_uuid = Uuid::from_str("1e5668e2-8481-11d4-bcf1-0080c73c8881").unwrap();
    let efi_var_arch_prot_guid: Guid = guid_from_uuid(&efi_var_arch_prot_uuid);
    let efi_var_write_arch_prot_uuid = Uuid::from_str("6441f818-6362-eb44-5700-7dba31dd2453").unwrap();
    let efi_var_write_arch_prot_guid: Guid = guid_from_uuid(&efi_var_write_arch_prot_uuid);
    let efi_tcg_prot_uuid = Uuid::from_str("f541796d-a62e-4954-a775-9584f61b9cdd").unwrap();
    let efi_tcg_prot_guid: Guid = guid_from_uuid(&efi_tcg_prot_uuid);
    let efi_tree_prot_uuid = Uuid::from_str("607f766c-7455-42be-930b-e4d76db2720f").unwrap();
    let efi_tree_prot_guid: Guid = guid_from_uuid(&efi_tree_prot_uuid);
    let efi_pcd_prot_uuid = Uuid::from_str("13a3f0f6-264a-3ef0-f2e0-dec512342f34").unwrap();
    let efi_pcd_prot_guid: Guid = guid_from_uuid(&efi_pcd_prot_uuid);
    let efi_device_path_utilities_prot_uuid = Uuid::from_str("0379be4e-d706-437d-b037-edb82fb772a4").unwrap();
    let efi_device_path_utilities_prot_guid: Guid = guid_from_uuid(&efi_device_path_utilities_prot_uuid);

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
    let mut depex = Depex::from(expression.to_vec());

    assert_eq!(depex.eval(&SPIN_LOCKED_PROTOCOL_DB), true);
  }

  #[test]
  fn guid_to_uuid_conversion_should_produce_correct_bytes() {
    let device_path_protocol_guid_bytes: &[u8] =
      &[0x4E, 0xBE, 0x79, 0x03, 0x06, 0xD7, 0x7D, 0x43, 0xB0, 0x37, 0xED, 0xB8, 0x2F, 0xB7, 0x72, 0xA4];

    let uuid = uuid_from_slice(device_path_protocol_guid_bytes).unwrap();
    assert_eq!(uuid, uuid::Uuid::from_str("0379be4e-d706-437d-b037-edb82fb772a4").unwrap());

    let guid = guid_from_uuid(&uuid);
    assert_eq!(guid.as_bytes(), device_path_protocol_guid_bytes);
  }
}
