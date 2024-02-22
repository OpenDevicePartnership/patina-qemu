//! UEFI Protocol Database support
//!
//! This library provides an UEFI protocol database implementation.
//!
//! ## Examples and Usage
//!
//! ```
//! # use core::ffi::c_void;
//! # use r_efi::efi;
//! # use std::str::FromStr;
//! # use uuid::Uuid;
//! use uefi_protocol_db_lib::SpinLockedProtocolDb;
//!
//! static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
//!
//! let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
//! let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
//! let interface1: *mut c_void = 0x1234 as *mut c_void;
//!
//! SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
//!
//! assert_eq!(SPIN_LOCKED_PROTOCOL_DB.locate_protocol(guid1), Ok(interface1));
//! ```
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation. All rights reserved.
//!
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//!
#![no_std]

extern crate alloc;

use ahash::AHasher;
use alloc::{
  collections::{BTreeMap, BTreeSet},
  vec,
  vec::Vec,
};
use core::{cmp::Ordering, ffi::c_void, hash::Hasher};
use r_efi::efi;

//private UUID used to create the "well-known handles"
const WELL_KNOWN_HANDLE_PROTOCOL_GUID: uuid::Uuid = uuid::Uuid::from_u128(0xfced7c96356e48cba9a9e089b2ddf49b);
pub const INVALID_HANDLE: efi::Handle = 0 as efi::Handle;
pub const DXE_CORE_HANDLE: efi::Handle = 1 as efi::Handle;
pub const RESERVED_MEMORY_ALLOCATOR_HANDLE: efi::Handle = 2 as efi::Handle;
pub const EFI_LOADER_CODE_ALLOCATOR_HANDLE: efi::Handle = 3 as efi::Handle;
pub const EFI_LOADER_DATA_ALLOCATOR_HANDLE: efi::Handle = 4 as efi::Handle;
pub const EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE: efi::Handle = 5 as efi::Handle;
pub const EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE: efi::Handle = 6 as efi::Handle;
pub const EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE: efi::Handle = 7 as efi::Handle;
pub const EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE: efi::Handle = 8 as efi::Handle;
pub const EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE: efi::Handle = 9 as efi::Handle;
pub const EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE: efi::Handle = 10 as efi::Handle;

/// This structure is used to track open protocol information on a handle.
///
/// It is returned from [`get_open_protocol_information`](SpinLockedProtocolDb::get_open_protocol_information)],
/// and used internally to track protocol usage within the database.
///
/// The semantics of this structure follow that of the EFI_OPEN_PROTOCOL_INFORMATION_ENTRY structure defined in UEFI
/// spec version 2.10 section 7.3.11.
///
#[derive(Clone, Copy, Debug)]
pub struct OpenProtocolInformation {
  pub agent_handle: Option<efi::Handle>,
  pub controller_handle: Option<efi::Handle>,
  pub attributes: u32,
  pub open_count: u32,
}

impl PartialEq for OpenProtocolInformation {
  fn eq(&self, other: &Self) -> bool {
    self.agent_handle == other.agent_handle
      && self.controller_handle == other.controller_handle
      && self.attributes == other.attributes
  }
}

impl Eq for OpenProtocolInformation {}

impl OpenProtocolInformation {
  fn new(
    handle: efi::Handle,
    agent_handle: Option<efi::Handle>,
    controller_handle: Option<efi::Handle>,
    attributes: u32,
  ) -> Result<Self, efi::Status> {
    const BY_DRIVER_EXCLUSIVE: u32 = efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE;
    match attributes {
      efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER => {
        if agent_handle.is_none() || controller_handle.is_none() || handle == controller_handle.unwrap() {
          return Err(efi::Status::INVALID_PARAMETER);
        }
      }
      efi::OPEN_PROTOCOL_BY_DRIVER | BY_DRIVER_EXCLUSIVE => {
        if agent_handle.is_none() || controller_handle.is_none() {
          return Err(efi::Status::INVALID_PARAMETER);
        }
      }
      efi::OPEN_PROTOCOL_EXCLUSIVE => {
        if agent_handle.is_none() {
          return Err(efi::Status::INVALID_PARAMETER);
        }
      }
      efi::OPEN_PROTOCOL_BY_HANDLE_PROTOCOL | efi::OPEN_PROTOCOL_GET_PROTOCOL | efi::OPEN_PROTOCOL_TEST_PROTOCOL => (),
      _ => return Err(efi::Status::INVALID_PARAMETER),
    }
    Ok(OpenProtocolInformation { agent_handle, controller_handle, attributes, open_count: 1 })
  }
}

impl From<OpenProtocolInformation> for efi::OpenProtocolInformationEntry {
  fn from(item: OpenProtocolInformation) -> Self {
    efi::OpenProtocolInformationEntry {
      agent_handle: item.agent_handle.unwrap_or(core::ptr::null_mut()),
      controller_handle: item.controller_handle.unwrap_or(core::ptr::null_mut()),
      attributes: item.attributes,
      open_count: item.open_count,
    }
  }
}

struct ProtocolInstance {
  interface: *mut c_void,
  opened_by_driver: bool,
  opened_by_exclusive: bool,
  usage: Vec<OpenProtocolInformation>,
}

#[derive(Debug, Eq, PartialEq)]
struct OrdGuid(efi::Guid);

impl PartialOrd for OrdGuid {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}
impl Ord for OrdGuid {
  fn cmp(&self, other: &Self) -> Ordering {
    self.0.as_bytes().cmp(other.0.as_bytes())
  }
}
/// This structure is used to track notification events for protocol notifies.
///
/// It is returned from [`install_protocol_interface`](SpinLockedProtocolDb::install_protocol_interface) and used
/// internally to track protocol notification registrations.
///
/// The only public member of this structure is `event`, which is an event that the caller can signal to indicate the
/// installation of new protocols.
///
#[derive(Clone, Debug)]
pub struct ProtocolNotify {
  pub event: efi::Event,
  registration: *mut c_void,
  fresh_handles: BTreeSet<efi::Handle>,
}

// This is the main implementation of the protocol database, but public
// interaction with the database should be via [`SpinLockedProtocolDb`] below.
struct ProtocolDb {
  handles: BTreeMap<usize, BTreeMap<OrdGuid, ProtocolInstance>>,
  notifications: BTreeMap<OrdGuid, Vec<ProtocolNotify>>,
  hash_new_handles: bool,
  next_handle: usize,
  next_registration: usize,
}

impl ProtocolDb {
  const fn new() -> Self {
    ProtocolDb {
      handles: BTreeMap::new(),
      notifications: BTreeMap::new(),
      hash_new_handles: false,
      next_handle: 1,
      next_registration: 1,
    }
  }

  fn enable_handle_hashing(&mut self) {
    self.hash_new_handles = true;
  }

  fn install_protocol_interface(
    &mut self,
    handle: Option<efi::Handle>,
    protocol: efi::Guid,
    interface: *mut c_void,
  ) -> Result<(efi::Handle, Vec<ProtocolNotify>), efi::Status> {
    //generate an output handle.
    let (output_handle, key) = match handle {
      Some(handle) => {
        //installing on existing handle.
        self.validate_handle(handle)?;
        let key = handle as usize;
        (handle, key)
      }
      None => {
        //installing on a new handle. Add a BTreeMap to track protocol instances on the new handle.
        let mut key;
        if self.hash_new_handles {
          let mut hasher = AHasher::default();
          hasher.write_usize(self.next_handle);
          key = hasher.finish() as usize;
          self.next_handle += 1;
          //make sure we don't collide with an existing key. 0 is reserved for "invalid handle".
          while key == 0 || self.handles.contains_key(&key) {
            hasher.write_usize(self.next_handle);
            key = hasher.finish() as usize;
            self.next_handle += 1;
          }
        } else {
          key = self.next_handle;
          self.next_handle += 1;
        }

        self.handles.insert(key, BTreeMap::new());
        let handle = key as efi::Handle;
        (handle, key)
      }
    };
    debug_assert!(self.handles.contains_key(&key)); //above logic should guarantee a valid key. Is debug assert to avoid perf cost of contains() in release.

    let handle_instance = self.handles.get_mut(&key).unwrap();

    if handle_instance.contains_key(&OrdGuid(protocol)) {
      return Err(efi::Status::INVALID_PARAMETER);
    }

    //create a new protocol instance to match the input.
    let protocol_instance =
      ProtocolInstance { interface, opened_by_driver: false, opened_by_exclusive: false, usage: Vec::new() };

    //attempt to add the protocol to the set of protocols on this handle.
    let exists = handle_instance.insert(OrdGuid(protocol), protocol_instance);
    assert!(exists.is_none()); //should be guaranteed by the `contains_key` check above.

    //determine if there are any events to be notified.
    if let Some(events) = self.notifications.get_mut(&OrdGuid(protocol)) {
      for event in events {
        event.fresh_handles.insert(output_handle);
      }
    }
    let events = match self.notifications.get(&OrdGuid(protocol)) {
      Some(events) => events.clone(),
      None => vec![],
    };

    Ok((output_handle, events))
  }

  fn uninstall_protocol_interface(
    &mut self,
    handle: efi::Handle,
    protocol: efi::Guid,
    interface: *mut c_void,
  ) -> Result<(), efi::Status> {
    self.validate_handle(handle)?;

    let key = handle as usize;
    let handle_instance =
      self.handles.get_mut(&key).expect("Invalid handle should not occur due to prior handle validation.");
    let instance = handle_instance.get(&OrdGuid(protocol)).ok_or(efi::Status::NOT_FOUND)?;

    if instance.interface != interface {
      return Err(efi::Status::NOT_FOUND);
    }

    //Spec requires that an attempt to uninstall an installed protocol interface that is open with an attribute of
    //efi::OPEN_PROTOCOL_BY_DRIVER should force a call to "Disconnect Controller" to attempt to release the interface
    //before uninstalling. As such, this routine simply returns ACCESS_DENIED if any agents are found active on the
    //protocol instance.
    if instance.usage.len() > 0 {
      return Err(efi::Status::ACCESS_DENIED);
    }
    handle_instance.remove(&OrdGuid(protocol));

    //if the last protocol instance on a handle is removed, delete the structures associated with the handles.
    if handle_instance.is_empty() {
      self.handles.remove(&key);
    }

    Ok(())
  }

  fn locate_handles(&mut self, protocol: Option<efi::Guid>) -> Result<Vec<efi::Handle>, efi::Status> {
    let handles: Vec<efi::Handle> = self
      .handles
      .iter()
      .filter_map(|(key, handle_data)| {
        match protocol {
          None => Some(*key as efi::Handle), //"None" means return all handles.
          Some(protocol) if handle_data.contains_key(&OrdGuid(protocol)) => Some(*key as efi::Handle),
          _ => None,
        }
      })
      .collect();
    if handles.is_empty() {
      return Err(efi::Status::NOT_FOUND);
    }
    Ok(handles)
  }

  fn locate_protocol(&mut self, protocol: efi::Guid) -> Result<*mut c_void, efi::Status> {
    let interface = self.handles.values().find_map(|x| x.get(&OrdGuid(protocol)));

    match interface {
      Some(interface) => Ok(interface.interface),
      None => Err(efi::Status::NOT_FOUND),
    }
  }

  fn get_interface_for_handle(&mut self, handle: efi::Handle, protocol: efi::Guid) -> Result<*mut c_void, efi::Status> {
    self.validate_handle(handle)?;

    let key = handle as usize;
    let handle_instance = self.handles.get_mut(&key).ok_or(efi::Status::NOT_FOUND)?;
    let instance = handle_instance.get_mut(&OrdGuid(protocol)).ok_or(efi::Status::NOT_FOUND)?;
    Ok(instance.interface)
  }

  fn validate_handle(&self, handle: efi::Handle) -> Result<(), efi::Status> {
    let handle = handle as usize;
    //to be valid the handle must exist in the handle database (i.e. not have been deleted).
    if !self.handles.contains_key(&handle) {
      return Err(efi::Status::INVALID_PARAMETER);
    }
    Ok(())
  }

  fn add_protocol_usage(
    &mut self,
    handle: efi::Handle,
    protocol: efi::Guid,
    agent_handle: Option<efi::Handle>,
    controller_handle: Option<efi::Handle>,
    attributes: u32,
  ) -> Result<(), efi::Status> {
    self.validate_handle(handle)?;

    if let Some(agent) = agent_handle {
      self.validate_handle(agent)?;
    }

    if let Some(controller) = controller_handle {
      self.validate_handle(controller)?;
    }

    let key = handle as usize;
    let handle_instance = self.handles.get_mut(&key).ok_or(efi::Status::UNSUPPORTED)?;
    let instance = handle_instance.get_mut(&OrdGuid(protocol)).ok_or(efi::Status::UNSUPPORTED)?;

    let new_using_agent = OpenProtocolInformation::new(handle, agent_handle, controller_handle, attributes)?;
    let exact_match = instance.usage.iter_mut().find(|user| user == &&new_using_agent);

    if instance.opened_by_driver && exact_match.is_some() {
      return Err(efi::Status::ALREADY_STARTED);
    }

    if !instance.opened_by_exclusive {
      if let Some(exact_match) = exact_match {
        exact_match.open_count += 1;
        return Ok(());
      }
    }

    const BY_DRIVER_EXCLUSIVE: u32 = efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE;
    match attributes {
      efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE | BY_DRIVER_EXCLUSIVE => {
        //Note: Per UEFI spec, a request to open with efi::OPEN_PROTOCOL_EXCLUSIVE set should result in a disconnect
        //of existing controllers that have the driver efi::OPEN_PROTOCOL_BY_DRIVER. This needs to be done in the
        //caller, since this library doesn't have access to DisconnectController, and is also executing under
        //the SpinLockedProtocolDb lock (which would cause deadlock if DisconnectController attempted to use
        //any of the protocol services). Instead, return ACCESS_DENIED.
        if instance.opened_by_exclusive || instance.opened_by_driver {
          return Err(efi::Status::ACCESS_DENIED);
        }
      }
      efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER
      | efi::OPEN_PROTOCOL_BY_HANDLE_PROTOCOL
      | efi::OPEN_PROTOCOL_GET_PROTOCOL
      | efi::OPEN_PROTOCOL_TEST_PROTOCOL => (),
      _ => panic!("Unsupported attributes: {:#x?}", attributes), //this should have been dealt with in ProtocolUsingAgent::new().
    }

    if agent_handle.is_none() {
      return Ok(()); //don't add the new using_agent if no agent is actually specified.
    }

    if (new_using_agent.attributes & efi::OPEN_PROTOCOL_BY_DRIVER) != 0 {
      instance.opened_by_driver = true;
    }
    if (new_using_agent.attributes & efi::OPEN_PROTOCOL_EXCLUSIVE) != 0 {
      instance.opened_by_exclusive = true;
    }
    instance.usage.push(new_using_agent);

    Ok(())
  }

  fn remove_protocol_usage(
    &mut self,
    handle: efi::Handle,
    protocol: efi::Guid,
    agent_handle: Option<efi::Handle>,
    controller_handle: Option<efi::Handle>,
  ) -> Result<(), efi::Status> {
    self.validate_handle(handle)?;

    if let Some(agent) = agent_handle {
      self.validate_handle(agent)?;
    }

    if let Some(controller) = controller_handle {
      self.validate_handle(controller)?;
    }

    let key = handle as usize;
    let handle_instance = self.handles.get_mut(&key).expect("valid handle, but no entry in self.handles");
    let instance = handle_instance.get_mut(&OrdGuid(protocol)).ok_or(efi::Status::NOT_FOUND)?;
    let mut removed = false;
    instance.usage.retain(|x| {
      if (x.agent_handle == agent_handle) && (x.controller_handle == controller_handle) {
        //if we are removing the usage that had this instance open by driver (there should be only one)
        //then clear the flag that the instance was opened by driver.
        if (x.attributes & efi::OPEN_PROTOCOL_BY_DRIVER) != 0 {
          instance.opened_by_driver = false;
        }
        //if we are removing the usage that had this instance open exclusive (there should be only one)
        //then clear the flag that the instance was opened exclusive.
        if (x.attributes & efi::OPEN_PROTOCOL_EXCLUSIVE) != 0 {
          instance.opened_by_exclusive = false;
        }
        removed = true;
        false //if agent and controller match, do not retain (i.e. remove).
      } else {
        true //if one or the other or both don't match, retain.
      }
    });

    if !removed {
      return Err(efi::Status::NOT_FOUND);
    }

    Ok(())
  }

  fn get_open_protocol_information_by_protocol(
    &mut self,
    handle: efi::Handle,
    protocol: efi::Guid,
  ) -> Result<Vec<OpenProtocolInformation>, efi::Status> {
    self.validate_handle(handle)?;

    let key = handle as usize;
    let handle_instance = self.handles.get_mut(&key).ok_or(efi::Status::NOT_FOUND)?;
    let instance = handle_instance.get_mut(&OrdGuid(protocol)).ok_or(efi::Status::NOT_FOUND)?;

    Ok(instance.usage.clone())
  }

  fn get_open_protocol_information(
    &mut self,
    handle: efi::Handle,
  ) -> Result<Vec<(efi::Guid, Vec<OpenProtocolInformation>)>, efi::Status> {
    let key = handle as usize;
    let handle_instance = self.handles.get(&key).ok_or(efi::Status::NOT_FOUND)?;

    let usages = handle_instance.iter().map(|(guid, instance)| (guid.0, instance.usage.clone())).collect();

    Ok(usages)
  }

  fn get_protocols_on_handle(&mut self, handle: efi::Handle) -> Result<Vec<efi::Guid>, efi::Status> {
    self.validate_handle(handle)?;

    let key = handle as usize;
    Ok(self.handles[&key].keys().clone().map(|x| x.0).collect())
  }

  fn register_protocol_notify(&mut self, protocol: efi::Guid, event: efi::Event) -> Result<*mut c_void, efi::Status> {
    let registration = self.next_registration as *mut c_void;
    self.next_registration += 1;
    let protocol_notify = ProtocolNotify { event, registration, fresh_handles: BTreeSet::new() };

    if let Some(existing_key) = self.notifications.get_mut(&OrdGuid(protocol)) {
      existing_key.push(protocol_notify);
    } else {
      let events: Vec<ProtocolNotify> = vec![protocol_notify];
      self.notifications.insert(OrdGuid(protocol), events);
    }
    Ok(registration)
  }

  fn unregister_protocol_notify_event(&mut self, event: efi::Event) {
    for (_, v) in self.notifications.iter_mut() {
      v.retain(|x| x.event != event);
    }
  }

  fn unregister_protocol_notify_events(&mut self, events: Vec<efi::Event>) {
    for event in events {
      self.unregister_protocol_notify_event(event);
    }
  }

  fn next_handle_for_registration(&mut self, registration: *mut c_void) -> Option<efi::Handle> {
    for (_, v) in self.notifications.iter_mut() {
      if let Some(index) = v.iter().position(|notify| notify.registration == registration) {
        if let Some(handle) = v[index].fresh_handles.pop_first() {
          return Some(handle);
        }
      }
    }
    None
  }

  fn get_child_handles(&mut self, parent_handle: efi::Handle) -> Vec<efi::Handle> {
    if self.validate_handle(parent_handle).is_err() {
      return Vec::new();
    }

    let handles = &self.handles[&(parent_handle as usize)];
    let mut child_handles: Vec<efi::Handle> = handles
      .iter()
      .flat_map(|(_, instance)| {
        //iterate over all the protocol instance usages for the parent handle....
        instance.usage.iter().filter_map(|open_info| {
          //and select the ones that opened a protocol instance on the parent_handle BY_CHILD_CONTROLLER
          //and return the controller_handles that did so (these are the child handles we're looking for).
          if (open_info.attributes & efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER) != 0 {
            Some(open_info.controller_handle.expect("Controller handle must exist if opened by child controller"))
          } else {
            None
          }
        })
      })
      .collect();
    child_handles.sort(); //dedup needs a sorted vector
    child_handles.dedup(); //remove any duplicate handles
    child_handles
  }
}

/// Spin-Locked protocol database instance.
///
/// This is the main access point for interaction with the protocol database.
/// The protocol database is intended to be used as a global singleton, so access
/// is only allowed through this structure which ensures that the event database
/// is properly guarded against race conditions.
///
/// ## Examples
/// ```
/// use uefi_protocol_db_lib::SpinLockedProtocolDb;
/// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
/// ```
///
pub struct SpinLockedProtocolDb {
  inner: tpl_lock::TplMutex<ProtocolDb>,
}

impl SpinLockedProtocolDb {
  /// Creates a new instance of SpinLockedProtocolDb.
  pub const fn new() -> Self {
    SpinLockedProtocolDb { inner: tpl_lock::TplMutex::new(efi::TPL_NOTIFY, ProtocolDb::new(), "ProtocolLock") }
  }

  fn lock(&self) -> tpl_lock::TplGuard<ProtocolDb> {
    self.inner.lock()
  }

  /// Initialize the protocol database. Installs well-known handles, and then enables hashing to ensure handles are
  /// opaque.
  pub fn init_protocol_db(&self) {
    let well_known_handle_guid: efi::Guid =
      unsafe { core::mem::transmute(*WELL_KNOWN_HANDLE_PROTOCOL_GUID.as_bytes()) };

    let well_known_handles = &[
      DXE_CORE_HANDLE,
      RESERVED_MEMORY_ALLOCATOR_HANDLE,
      EFI_LOADER_CODE_ALLOCATOR_HANDLE,
      EFI_LOADER_DATA_ALLOCATOR_HANDLE,
      EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE,
      EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE,
      EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE,
      EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE,
      EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE,
      EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE,
    ];

    for target_handle in well_known_handles.iter() {
      let (handle, _) = self
        .install_protocol_interface(None, well_known_handle_guid, core::ptr::null_mut())
        .expect("failed to install well-known handle");
      assert_eq!(handle, *target_handle);
    }
    self.lock().enable_handle_hashing();
  }

  /// Installs a protocol interface on the given handle.
  ///
  /// This function closely matches the semantics of the EFI_BOOT_SERVICES.InstallProtocolInterface() API in
  /// UEFI spec 2.10 section 7.3.2. Please refer to the spec for details on the input parameters.
  ///
  /// On success, this function returns the handle on which the protocol is installed (which may be newly created if
  /// no handle was provided on input), as well as a vector of [`ProtocolNotify`] structures that the caller can use to
  /// signal events for any registered notifies on this protocol installation.
  ///
  /// ## Errors
  ///
  /// Returns r_efi:efi::Status::INVALID_PARAMETER if incorrect parameters are given.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// ```
  ///
  pub fn install_protocol_interface(
    &self,
    handle: Option<efi::Handle>,
    guid: efi::Guid,
    interface: *mut c_void,
  ) -> Result<(efi::Handle, Vec<ProtocolNotify>), efi::Status> {
    self.lock().install_protocol_interface(handle, guid, interface)
  }

  /// Removes a protocol interface from the given handle.
  ///
  /// This function closely matches the semantics of the EFI_BOOT_SERVICES.UninstallProtocolInterface() API in
  /// UEFI spec 2.10 section 7.3.3. Please refer to the spec for details on the input parameters.
  ///
  /// ## Errors
  ///
  /// Returns r_efi:efi::Status::INVALID_PARAMETER if incorrect parameters are given.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid1, interface1).unwrap();
  ///
  ///  ```
  ///
  pub fn uninstall_protocol_interface(
    &self,
    handle: efi::Handle,
    guid: efi::Guid,
    interface: *mut c_void,
  ) -> Result<(), efi::Status> {
    self.lock().uninstall_protocol_interface(handle, guid, interface)
  }

  /// Returns a vector of handles that have the specified protocol installed on them.
  ///
  /// On success, this function returns a vector of [`efi::Handle`] that have this protocol installed on them.
  ///
  /// If protocol is `None` on input, then all handles with any protocols installed on them are returned.
  ///
  /// ## Errors
  ///
  /// Returns [`r_efi:efi::Status::INVALID_PARAMETER`] if incorrect parameters are given.
  /// Returns [`r_efi:efi::Status::NOT_FOUND`] if no matching handles are found.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let interface2: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (handle2, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface2).unwrap();
  ///
  /// let handles = SPIN_LOCKED_PROTOCOL_DB.locate_handles(Some(guid1)).unwrap();
  /// assert!(handles.contains(&handle));
  /// assert!(handles.contains(&handle2));
  /// ```
  ///
  pub fn locate_handles(&self, protocol: Option<efi::Guid>) -> Result<Vec<efi::Handle>, efi::Status> {
    self.lock().locate_handles(protocol)
  }

  /// Returns an instance of the specified protocol interface from any handle.
  ///
  /// On success, this function returns the protocol interface pointer for the given protocol from any handle. If
  /// multiple handles exist with this protocol installed on them, no guarantees are made about which handle the
  /// interface will come from.
  ///
  /// ## Errors
  ///
  /// Returns [`r_efi:efi::Status::INVALID_PARAMETER`] if incorrect parameters are given.
  /// Returns [`r_efi:efi::Status::NOT_FOUND`] if no matching interfaces are found.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let interface2: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (handle2, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface2).unwrap();
  ///
  /// let interface = SPIN_LOCKED_PROTOCOL_DB.locate_protocol(guid1).unwrap();
  /// assert!(interface == interface1 || interface == interface2);
  /// ```
  ///
  pub fn locate_protocol(&self, protocol: efi::Guid) -> Result<*mut c_void, efi::Status> {
    self.lock().locate_protocol(protocol)
  }

  /// Returns the interface for the specified protocol on the given handle if it exists
  ///
  /// On success, this function returns the protocol interface pointer for the given protocol on the specified handle.
  ///
  /// ## Errors
  ///
  /// Returns [`r_efi:efi::Status::INVALID_PARAMETER`] if incorrect parameters are given.
  /// Returns [`r_efi:efi::Status::NOT_FOUND`] if no matching interfaces are found on the given handle.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let interface2: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (handle2, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface2).unwrap();
  ///
  /// assert_eq!(interface1, SPIN_LOCKED_PROTOCOL_DB.get_interface_for_handle(handle, guid1).unwrap());
  /// assert_eq!(interface2, SPIN_LOCKED_PROTOCOL_DB.get_interface_for_handle(handle2, guid1).unwrap());
  /// ```
  ///
  pub fn get_interface_for_handle(&self, handle: efi::Handle, protocol: efi::Guid) -> Result<*mut c_void, efi::Status> {
    self.lock().get_interface_for_handle(handle, protocol)
  }

  /// Returns Ok(()) if the handle is a valid handle, Err(Status::INVALID_PARAMETER) otherwise.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let interface2: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let handle2 = (handle as usize + 1) as efi::Handle;
  ///
  /// assert_eq!(SPIN_LOCKED_PROTOCOL_DB.validate_handle(handle), Ok(()));
  /// assert_eq!(SPIN_LOCKED_PROTOCOL_DB.validate_handle(handle2), Err(efi::Status::INVALID_PARAMETER));
  /// ```
  ///
  pub fn validate_handle(&self, handle: efi::Handle) -> Result<(), efi::Status> {
    self.lock().validate_handle(handle)
  }

  /// Adds a protocol usage on the specified handle/protocol.
  ///
  /// This function generally matches the behavior of EFI_BOOT_SERVICES.OpenProtocol() API in the UEFI spec 2.10 section
  /// 7.3.9, with the exception that operations requiring interactions with the UEFI driver model are not supported and
  /// are expected to be handled by the caller. Where appropriate, this function returns error status to allow the
  /// caller to implement the behavior that the spec requires for interaction with the UEFI driver model. Refer to the
  /// UEFI spec description for general operation and details on input parameters.
  ///
  /// # Errors
  ///
  /// Returns [`r_efi:efi::Status::INVALID_PARAMETER`] if incorrect parameters are given.
  /// Returns [`r_efi:efi::Status::NOT_FOUND`] if no matching interfaces are found.
  /// Returns [`r_efi:efi::Status::ALREADY_STARTED`] if attributes is BY_DRIVER and there is an existing usage by the
  ///   agent handle.
  /// Returns [`r_efi:efi::Status::ACCESS_DENIED`] if attributes is efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE |
  ///   BY_DRIVER_EXCLUSIVE and there is an existing usage that conflicts with those attributes.
  /// Returns [`efi::Status::UNSUPPORTED`] if the handle does not support the specified protocol.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (agent_handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (controller_handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle, guid1, Some(agent_handle), Some(controller_handle), efi::OPEN_PROTOCOL_BY_DRIVER).unwrap();
  ///
  /// ```
  ///
  pub fn add_protocol_usage(
    &self,
    handle: efi::Handle,
    protocol: efi::Guid,
    agent_handle: Option<efi::Handle>,
    controller_handle: Option<efi::Handle>,
    attributes: u32,
  ) -> Result<(), efi::Status> {
    self.lock().add_protocol_usage(handle, protocol, agent_handle, controller_handle, attributes)
  }

  /// Removes a protocol usage from the specified handle/protocol.
  ///
  /// This function generally matches the behavior of EFI_BOOT_SERVICES.CloseProtocol() API in the UEFI spec 2.10
  /// section 7.3.10. Refer to the UEFI spec description for details on input parameters.
  ///
  /// # Errors
  ///
  /// Returns [`r_efi:efi::Status::INVALID_PARAMETER`] if incorrect parameters are given.
  /// Returns [`r_efi:efi::Status::NOT_FOUND`] if the specified handle does not support the specified protocol.
  /// Returns [`r_efi:efi::Status::NOT_FOUND`] if the protocol interface specified by handle and protocol are not
  ///   opened by the specified agent and controller handle.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (agent_handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (controller_handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle, guid1, Some(agent_handle), Some(controller_handle), efi::OPEN_PROTOCOL_BY_DRIVER).unwrap();
  ///
  /// SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle, guid1, Some(agent_handle), Some(controller_handle)).unwrap();
  ///
  /// ```
  ///
  pub fn remove_protocol_usage(
    &self,
    handle: efi::Handle,
    protocol: efi::Guid,
    agent_handle: Option<efi::Handle>,
    controller_handle: Option<efi::Handle>,
  ) -> Result<(), efi::Status> {
    self.lock().remove_protocol_usage(handle, protocol, agent_handle, controller_handle)
  }

  /// Returns open protocol information for the given handle/protocol.
  ///
  /// This function generally matches the behavior of EFI_BOOT_SERVICES.OpenProtocolInformation() API in the UEFI spec
  /// 2.10 section 7.3.11. Refer to the UEFI spec description for details on input parameters.
  ///
  /// # Errors
  ///
  /// Returns [`r_efi:efi::Status::INVALID_PARAMETER`] if incorrect parameters are given.
  /// Returns [`r_efi:efi::Status::NOT_FOUND`] if the specified handle does not support the specified protocol.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (agent_handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (controller_handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle, guid1, Some(agent_handle), Some(controller_handle), efi::OPEN_PROTOCOL_BY_DRIVER).unwrap();
  ///
  /// let open_protocol_info = SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information_by_protocol(handle, guid1).unwrap();
  /// assert_eq!(open_protocol_info.len(), 1);
  ///
  /// ```
  ///
  pub fn get_open_protocol_information_by_protocol(
    &self,
    handle: efi::Handle,
    protocol: efi::Guid,
  ) -> Result<Vec<OpenProtocolInformation>, efi::Status> {
    self.lock().get_open_protocol_information_by_protocol(handle, protocol)
  }

  /// Returns open protocol information for the given handle.
  ///
  ///
  /// # Errors
  ///
  /// Returns [`r_efi:efi::Status::INVALID_PARAMETER`] if incorrect parameters are given.
  /// Returns [`r_efi:efi::Status::NOT_FOUND`] if the specified handle does not support the specified protocol.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (agent_handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (controller_handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle, guid1, Some(agent_handle), Some(controller_handle), efi::OPEN_PROTOCOL_BY_DRIVER).unwrap();
  ///
  /// let open_protocol_info = SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information(handle).unwrap();
  /// assert_eq!(open_protocol_info.len(), 1);
  /// let (guid, open_info) = &open_protocol_info[0];
  /// assert_eq!(*guid, guid1);
  /// assert_eq!(open_info[0].agent_handle, Some(agent_handle));
  /// assert_eq!(open_info[0].controller_handle, Some(controller_handle));
  /// assert_ne!(open_info[0].attributes & efi::OPEN_PROTOCOL_BY_DRIVER, 0);
  ///
  /// ```
  ///
  pub fn get_open_protocol_information(
    &self,
    handle: efi::Handle,
  ) -> Result<Vec<(efi::Guid, Vec<OpenProtocolInformation>)>, efi::Status> {
    self.lock().get_open_protocol_information(handle)
  }

  /// Returns a vector of protocol GUIDs that are installed on the given handle.
  ///
  /// This function generally matches the behavior of EFI_BOOT_SERVICES.ProtocolsPerHandle() API in the UEFI spec
  /// 2.10 section 7.3.14. Refer to the UEFI spec description for details on input parameters.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  /// let uuid2 = Uuid::from_str("fe20c1a0-a4ac-48f1-9307-e2f8206e1019").unwrap();
  /// let guid2: efi::Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
  /// let interface2: *mut c_void = 0x4321 as *mut c_void;
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(Some(handle), guid2, interface2).unwrap();
  ///
  /// let protocols = SPIN_LOCKED_PROTOCOL_DB.get_protocols_on_handle(handle).unwrap();
  ///
  /// assert!(protocols.contains(&guid1));
  /// assert!(protocols.contains(&guid2));
  ///
  /// ```
  ///
  pub fn get_protocols_on_handle(&self, handle: efi::Handle) -> Result<Vec<efi::Guid>, efi::Status> {
    self.lock().get_protocols_on_handle(handle)
  }

  /// Registers a notification event to be returned on protocol installation.
  ///
  /// This function generally matches the behavior of EFI_BOOT_SERVICES.RegisterProtocolNotify() API in the UEFI spec
  /// 2.10 section 7.3.5. Refer to the UEFI spec description for details on input parameters. This implementation does
  /// not actually fire the event; instead, a list notifications is returned by [`install_protocol_interface`] so that
  /// the caller can fire the events.
  ///
  /// Returns a registration token that can be used with [`next_handle_for_registration`] to iterate over handles
  /// that have fresh installations of the specified protocol.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  ///
  /// let event: efi::Event = 0x4321 as efi::Event;
  ///
  /// let registration = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event);
  ///
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// assert_eq!(notifies[0].event, event);
  ///
  /// ```
  ///
  pub fn register_protocol_notify(&self, protocol: efi::Guid, event: efi::Event) -> Result<*mut c_void, efi::Status> {
    self.lock().register_protocol_notify(protocol, event)
  }

  /// De-registers a list of previously installed protocol notifies.
  ///
  /// This can be used by the caller to remove previously registered event notifications.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  ///
  /// let event: efi::Event = 0x4321 as efi::Event;
  ///
  /// let registration = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event);
  ///
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// assert_eq!(notifies[0].event, event);
  ///
  /// SPIN_LOCKED_PROTOCOL_DB.unregister_protocol_notify_events (vec![event]);
  ///
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// assert_eq!(notifies.len(), 0);
  /// ```
  ///
  pub fn unregister_protocol_notify_events(&self, events: Vec<efi::Event>) {
    self.lock().unregister_protocol_notify_events(events);
  }

  /// Registers the next handle for which a protocol has been installed that matches the registration.
  ///
  /// ## Examples
  ///
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  ///
  /// let event: efi::Event = 0x4321 as efi::Event;
  ///
  /// let registration = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event).unwrap();
  ///
  /// let (handle, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (handle2, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (handle3, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// assert_eq!(Some(handle), SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(registration));
  /// assert_eq!(Some(handle2), SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(registration));
  /// assert_eq!(Some(handle3), SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(registration));
  /// assert_eq!(None, SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(registration));
  /// ```
  ///
  pub fn next_handle_for_registration(&self, registration: *mut c_void) -> Option<efi::Handle> {
    self.lock().next_handle_for_registration(registration)
  }

  /// Returns a vector of controller handles that have parent_handle open BY_CHILD_CONTROLLER.
  ///
  /// ## Examples
  /// ```
  /// # use core::ffi::c_void;
  /// # use r_efi::efi;
  /// # use std::str::FromStr;
  /// # use uuid::Uuid;
  /// use uefi_protocol_db_lib::SpinLockedProtocolDb;
  ///
  /// static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
  /// let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
  /// let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
  /// let interface1: *mut c_void = 0x1234 as *mut c_void;
  ///
  /// let (controller, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (driver, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (child1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (child2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (child3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (_notchild1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  /// let (_notchild2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
  ///
  /// for child in [child1, child2, child3] {
  ///   SPIN_LOCKED_PROTOCOL_DB
  ///     .add_protocol_usage(controller, guid1, Some(driver), Some(child), efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER)
  ///     .unwrap();
  /// }
  ///
  /// let child_list = SPIN_LOCKED_PROTOCOL_DB.get_child_handles(controller);
  /// assert!(child_list.len() == 3);
  /// for child in [child1, child2, child3] {
  ///   assert!(child_list.contains(&child));
  /// }
  /// ```
  pub fn get_child_handles(&self, parent_handle: efi::Handle) -> Vec<efi::Handle> {
    self.lock().get_child_handles(parent_handle)
  }
}

unsafe impl Send for SpinLockedProtocolDb {}
unsafe impl Sync for SpinLockedProtocolDb {}

#[cfg(test)]
mod tests {
  extern crate std;
  use core::str::FromStr;
  use std::println;

  use r_efi::efi;
  use uuid::Uuid;

  use super::*;

  #[test]
  fn new_should_create_protocol_db() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();
    assert_eq!(SPIN_LOCKED_PROTOCOL_DB.lock().handles.len(), 0)
  }

  #[test]
  fn install_protocol_interface_should_install_protocol_interface() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    assert_ne!(handle, 0 as *mut c_void);
    let test_instance = ProtocolInstance {
      interface: interface1,
      opened_by_driver: false,
      opened_by_exclusive: false,
      usage: Vec::new(),
    };
    let key = handle as usize;
    let mut db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_instance = db.handles.get_mut(&key).unwrap();
    let created_instance = protocol_instance.get(&OrdGuid(guid1)).unwrap();
    assert_eq!(test_instance.interface, created_instance.interface);
  }

  #[test]
  fn uninstall_protocol_interface_should_uninstall_protocol_interface() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let key = handle as usize;

    SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid1, interface1).unwrap();

    let mut db = SPIN_LOCKED_PROTOCOL_DB.lock();
    assert!(db.handles.get_mut(&key).is_none());
  }

  #[test]
  fn uninstall_protocol_interface_should_give_access_denied_if_interface_in_use() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let key = handle as usize;

    // fish out the created instance, and add a fake ProtocolUsingAgent to simulate the
    // protocol being "efi::OPEN_PROTOCOL_BY_DRIVER"
    let mut instance = SPIN_LOCKED_PROTOCOL_DB.lock().handles.get_mut(&key).unwrap().remove(&OrdGuid(guid1)).unwrap();

    instance.usage.push(OpenProtocolInformation {
      agent_handle: None,
      controller_handle: None,
      attributes: efi::OPEN_PROTOCOL_BY_DRIVER,
      open_count: 1,
    });

    SPIN_LOCKED_PROTOCOL_DB.lock().handles.get_mut(&key).unwrap().insert(OrdGuid(guid1), instance);

    let err = SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid1, interface1);
    assert_eq!(err, Err(efi::Status::ACCESS_DENIED));

    let mut db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_instance = db.handles.get_mut(&key).unwrap();
    assert_eq!(protocol_instance.contains_key(&OrdGuid(guid1)), true);
  }

  #[test]
  fn uninstall_protocol_interface_should_give_not_found_if_not_found() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let uuid2 = Uuid::from_str("9c5dca1d-ac0f-46db-9eba-2bc961c711a2").unwrap();
    let guid2: efi::Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
    let interface2: *mut c_void = 0x4321 as *mut c_void;

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    let err = SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid2, interface1);
    assert_eq!(err, Err(efi::Status::NOT_FOUND));

    let err = SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid1, interface2);
    assert_eq!(err, Err(efi::Status::NOT_FOUND));
  }

  #[test]
  fn locate_handle_should_locate_handles() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let uuid2 = Uuid::from_str("9c5dca1d-ac0f-46db-9eba-2bc961c711a2").unwrap();
    let guid2: efi::Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
    let interface2: *mut c_void = 0x4321 as *mut c_void;

    let uuid3 = Uuid::from_str("2a32017e-7e6b-4563-890d-fff945530438").unwrap();
    let guid3: efi::Guid = unsafe { core::mem::transmute(*uuid3.as_bytes()) };

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    assert_eq!(
      handle1,
      SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(Some(handle1), guid2, interface2).unwrap().0
    );
    let (handle2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle4, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    assert_eq!(
      handle4,
      SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(Some(handle4), guid2, interface2).unwrap().0
    );

    let handles = SPIN_LOCKED_PROTOCOL_DB.locate_handles(None).unwrap();
    for handle in [handle1, handle2, handle3, handle4] {
      assert!(handles.contains(&handle));
    }

    let handles = SPIN_LOCKED_PROTOCOL_DB.locate_handles(Some(guid2)).unwrap();
    for handle in [handle1, handle4] {
      assert!(handles.contains(&handle));
    }
    for handle in [handle2, handle3] {
      assert!(!handles.contains(&handle));
    }

    assert_eq!(SPIN_LOCKED_PROTOCOL_DB.locate_handles(Some(guid3)), Err(efi::Status::NOT_FOUND));
  }

  #[test]
  fn validate_handle_should_validate_good_handles_and_reject_bad_ones() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    assert_eq!(SPIN_LOCKED_PROTOCOL_DB.validate_handle(handle1), Ok(()));
    let handle2 = (handle1 as usize + 1) as efi::Handle;
    assert_eq!(SPIN_LOCKED_PROTOCOL_DB.validate_handle(handle2), Err(efi::Status::INVALID_PARAMETER));
  }

  #[test]
  fn validate_handle_empty_handles_are_invalid() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle1, guid1, interface1).unwrap();
    assert_eq!(SPIN_LOCKED_PROTOCOL_DB.validate_handle(handle1), Err(efi::Status::INVALID_PARAMETER));
  }

  #[test]
  fn add_protocol_usage_should_update_protocol_usages() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    //Adding a usage
    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), efi::OPEN_PROTOCOL_GET_PROTOCOL)
      .unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    drop(protocol_db);

    //Adding the exact same usage should not create a new usage; it should update open_count
    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), efi::OPEN_PROTOCOL_GET_PROTOCOL)
      .unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(2, protocol_user_list[0].open_count);
    drop(protocol_db);
  }
  #[test]
  fn add_protocol_usage_by_child_controller() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle4, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    //Adding a usage BY_CHILD_CONTROLLER should succeed.
    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER)
      .unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    drop(protocol_db);

    //Adding a protocol BY_CHILD_CONTROLLER should fail if agent and controller not specified.
    let result =
      SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, None, None, efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER);
    assert_eq!(result, Err(efi::Status::INVALID_PARAMETER));
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    drop(protocol_db);

    //Adding a protocol BY_CHILD_CONTROLLER should fail if controller_handle matches handle.
    let result = SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
      handle1,
      guid1,
      Some(handle2),
      Some(handle1),
      efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER,
    );
    assert_eq!(result, Err(efi::Status::INVALID_PARAMETER));
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    drop(protocol_db);

    //Adding a protocol BY_CHILD_CONTROLLER should succeed even if another agent has protocol open on handle with "exclusive".
    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle4, guid1, Some(handle2), Some(handle1), efi::OPEN_PROTOCOL_EXCLUSIVE)
      .unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle4 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(efi::OPEN_PROTOCOL_EXCLUSIVE, protocol_user_list[0].attributes);
    drop(protocol_db);

    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle4, guid1, Some(handle2), Some(handle3), efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER)
      .unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle4 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(2, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(1, protocol_user_list[1].open_count);
    assert_eq!(efi::OPEN_PROTOCOL_EXCLUSIVE, protocol_user_list[0].attributes);
    assert_eq!(efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER, protocol_user_list[1].attributes);
    drop(protocol_db);
  }

  fn test_driver_and_exclusive_protocol_usage(test_attributes: u32) {
    println!("Testing add_protocol_usage for attributes: {:#x?}", test_attributes);
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle4, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    //Adding a usage BY_DRIVER should succeed if no other handles are in the database.
    SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), test_attributes).unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(test_attributes, protocol_user_list[0].attributes);
    drop(protocol_db);

    //Adding the same usage with same attributes again should result in ALREADY_STARTED if it was opened BY_DRIVER.
    if (test_attributes & efi::OPEN_PROTOCOL_BY_DRIVER) != 0 {
      let result =
        SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), test_attributes);
      assert_eq!(result, Err(efi::Status::ALREADY_STARTED));
      let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
      let protocol_user_list =
        &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
      assert_eq!(1, protocol_user_list.len());
      assert_eq!(1, protocol_user_list[0].open_count);
      assert_eq!(test_attributes, protocol_user_list[0].attributes);
      drop(protocol_db);
    }

    //Adding a different usage with BY_DRIVER on same handle should result in ACCESS_DENIED
    let result = SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
      handle1,
      guid1,
      Some(handle4),
      Some(handle3),
      efi::OPEN_PROTOCOL_BY_DRIVER,
    );
    assert_eq!(result, Err(efi::Status::ACCESS_DENIED));
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(test_attributes, protocol_user_list[0].attributes);
    drop(protocol_db);

    //Adding a different usage with EXCLUSIVE should result in ACCESS_DENIED
    let result = SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
      handle1,
      guid1,
      Some(handle4),
      Some(handle3),
      efi::OPEN_PROTOCOL_EXCLUSIVE,
    );
    assert_eq!(result, Err(efi::Status::ACCESS_DENIED));
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(test_attributes, protocol_user_list[0].attributes);
    drop(protocol_db);

    //Adding a usage BY_CHILD_CONTROLLER should result in a new usage record.
    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle1, guid1, Some(handle4), Some(handle3), efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER)
      .unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(2, protocol_user_list.len());
    assert_eq!(test_attributes, protocol_user_list[0].attributes);
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER, protocol_user_list[1].attributes);
    assert_eq!(1, protocol_user_list[1].open_count);
    drop(protocol_db);
  }

  #[test]
  fn add_protocol_usage_by_driver_and_exclusive() {
    //For this library implementation, BY_DRIVER, EXCLUSIVE, and BY_DRIVER_EXCLUSIVE function identically (except
    //for the contents of the attributes field in the usage record). See note in [`add_protocol_usage()`] above for
    //further discussion.
    for test_attributes in [
      efi::OPEN_PROTOCOL_BY_DRIVER,
      efi::OPEN_PROTOCOL_EXCLUSIVE,
      efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE,
    ] {
      test_driver_and_exclusive_protocol_usage(test_attributes);
    }
  }

  fn test_handle_get_or_test_protocol_usage(test_attributes: u32) {
    println!("Testing add_protocol_usage for attributes: {:#x?}", test_attributes);
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle4, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    //Adding a usage should succeed if no other handles are in the database.
    SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), test_attributes).unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(test_attributes, protocol_user_list[0].attributes);
    drop(protocol_db);

    //Adding a usage should succeed even if agent_handle is None, but new record should not be added.
    SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, None, Some(handle3), test_attributes).unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(test_attributes, protocol_user_list[0].attributes);
    drop(protocol_db);

    //Adding a usage should succeed even if agent_handle is None and ControllerHandle is node, but new record should not be added.
    SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, None, None, test_attributes).unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(test_attributes, protocol_user_list[0].attributes);
    drop(protocol_db);

    //Adding a usage should succeed even if controller_handle is none, and a new record should be added.
    SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, Some(handle2), None, test_attributes).unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(2, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[0].open_count);
    assert_eq!(test_attributes, protocol_user_list[0].attributes);
    assert_eq!(1, protocol_user_list[1].open_count);
    assert_eq!(test_attributes, protocol_user_list[1].attributes);
    drop(protocol_db);

    //Add a BY_DRIVER_EXCLUSIVE usage for testing.
    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(
        handle4,
        guid1,
        Some(handle2),
        Some(handle3),
        efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE,
      )
      .unwrap();

    //Adding a usage should succeed even though the handle is already open BY_DRIVER_EXCLUSIVE
    SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle4, guid1, Some(handle2), None, test_attributes).unwrap();
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(2, protocol_user_list.len());
    assert_eq!(1, protocol_user_list[1].open_count);
    assert_eq!(test_attributes, protocol_user_list[1].attributes);
    drop(protocol_db);
  }

  #[test]
  fn add_protocol_usage_by_handle_get_or_test() {
    for test_attributes in
      [efi::OPEN_PROTOCOL_BY_HANDLE_PROTOCOL, efi::OPEN_PROTOCOL_GET_PROTOCOL, efi::OPEN_PROTOCOL_TEST_PROTOCOL]
    {
      test_handle_get_or_test_protocol_usage(test_attributes);
    }
  }

  #[test]
  fn remove_protocol_usage_should_succeed_regardless_of_attributes() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (agent, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (controller, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    for attributes in [
      efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER,
      efi::OPEN_PROTOCOL_BY_DRIVER,
      efi::OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
      efi::OPEN_PROTOCOL_EXCLUSIVE,
      efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE,
      efi::OPEN_PROTOCOL_GET_PROTOCOL,
      efi::OPEN_PROTOCOL_TEST_PROTOCOL,
    ] {
      let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
      SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle, guid1, Some(agent), Some(controller), attributes).unwrap();
      SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle, guid1, Some(agent), Some(controller)).unwrap();
      let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
      let protocol_user_list =
        &protocol_db.handles.get(&(handle as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
      assert_eq!(0, protocol_user_list.len());
      drop(protocol_db);
    }
  }

  #[test]
  fn remove_protocol_usage_should_return_not_found_if_usage_not_found() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), efi::OPEN_PROTOCOL_BY_DRIVER)
      .unwrap();

    let result = SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, Some(handle3), Some(handle2));
    assert_eq!(result, Err(efi::Status::NOT_FOUND));
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    drop(protocol_db);

    let result = SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, None, Some(handle3));
    assert_eq!(result, Err(efi::Status::NOT_FOUND));
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    drop(protocol_db);

    let result = SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, Some(handle2), None);
    assert_eq!(result, Err(efi::Status::NOT_FOUND));
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    drop(protocol_db);

    let result = SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, None, None);
    assert_eq!(result, Err(efi::Status::NOT_FOUND));
    let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
    let protocol_user_list = &protocol_db.handles.get(&(handle1 as usize)).unwrap().get(&OrdGuid(guid1)).unwrap().usage;
    assert_eq!(1, protocol_user_list.len());
    drop(protocol_db);
  }

  #[test]
  fn add_protocol_usage_should_succeed_after_remove_protocol_usage() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle4, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), efi::OPEN_PROTOCOL_BY_DRIVER)
      .unwrap();

    //adding it agin with different agent handle should fail with access denied.
    assert_eq!(
      SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
        handle1,
        guid1,
        Some(handle4),
        Some(handle3),
        efi::OPEN_PROTOCOL_BY_DRIVER
      ),
      Err(efi::Status::ACCESS_DENIED)
    );

    SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, Some(handle2), Some(handle3)).unwrap();

    //adding it agin with different agent handle should succeed.
    assert_eq!(
      SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
        handle1,
        guid1,
        Some(handle4),
        Some(handle3),
        efi::OPEN_PROTOCOL_BY_DRIVER
      ),
      Ok(())
    );
  }

  #[test]
  fn get_open_protocol_information_by_protocol_returns_information() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let attributes_list = [
      efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE,
      efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER,
      efi::OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
      efi::OPEN_PROTOCOL_GET_PROTOCOL,
      efi::OPEN_PROTOCOL_TEST_PROTOCOL,
    ];

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let mut test_info = Vec::new();
    for attributes in attributes_list {
      let (agent, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
      let (controller, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
      test_info.push((Some(agent), Some(controller), attributes));
      SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle, guid1, Some(agent), Some(controller), attributes).unwrap();
    }

    let open_protocol_info_list =
      SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information_by_protocol(handle, guid1).unwrap();
    assert_eq!(attributes_list.len(), test_info.len());
    assert_eq!(attributes_list.len(), open_protocol_info_list.len());
    for idx in 0..attributes_list.len() {
      assert_eq!(test_info[idx].0, open_protocol_info_list[idx].agent_handle);
      assert_eq!(test_info[idx].1, open_protocol_info_list[idx].controller_handle);
      assert_eq!(test_info[idx].2, open_protocol_info_list[idx].attributes);
      assert_eq!(1, open_protocol_info_list[idx].open_count);
    }
  }

  #[test]
  fn get_open_protocol_information_by_protocol_should_return_not_found_if_handle_or_protocol_not_present() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let uuid2 = Uuid::from_str("98d32ea1-e980-46b5-bb2c-564934c8cce6").unwrap();
    let guid2: efi::Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
    let interface2: *mut c_void = 0x4321 as *mut c_void;

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (handle2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid2, interface2).unwrap();
    let (agent, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (controller, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle, guid1, Some(agent), Some(controller), efi::OPEN_PROTOCOL_BY_DRIVER)
      .unwrap();

    let result = SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information_by_protocol(handle, guid2);
    assert_eq!(result, Err(efi::Status::NOT_FOUND));

    let result = SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information_by_protocol(handle2, guid1);
    assert_eq!(result, Err(efi::Status::NOT_FOUND));
  }

  #[test]
  fn to_efi_open_protocol_should_match_source_open_protocol_information_entry() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (agent, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (controller, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    SPIN_LOCKED_PROTOCOL_DB
      .add_protocol_usage(handle, guid1, Some(agent), Some(controller), efi::OPEN_PROTOCOL_BY_DRIVER)
      .unwrap();

    for info in SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information_by_protocol(handle, guid1).unwrap() {
      let efi_info = efi::OpenProtocolInformationEntry::from(info);
      assert_eq!(efi_info.agent_handle, info.agent_handle.unwrap());
      assert_eq!(efi_info.controller_handle, info.controller_handle.unwrap());
      assert_eq!(efi_info.attributes, info.attributes);
      assert_eq!(efi_info.open_count, info.open_count);
    }
  }

  #[test]
  fn get_open_protocol_information_should_return_all_open_protocol_info() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let attributes_list = [
      efi::OPEN_PROTOCOL_BY_DRIVER | efi::OPEN_PROTOCOL_EXCLUSIVE,
      efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER,
      efi::OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
      efi::OPEN_PROTOCOL_GET_PROTOCOL,
      efi::OPEN_PROTOCOL_TEST_PROTOCOL,
    ];

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let mut test_info = Vec::new();
    for attributes in attributes_list {
      let (agent, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
      let (controller, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
      test_info.push((Some(agent), Some(controller), attributes));
      SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle, guid1, Some(agent), Some(controller), attributes).unwrap();
    }

    let open_protocol_info_list = SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information(handle).unwrap();
    assert_eq!(attributes_list.len(), test_info.len());
    assert_eq!(open_protocol_info_list.len(), 1);
    for idx in 0..attributes_list.len() {
      assert_eq!(guid1, open_protocol_info_list[0].0);
      assert_eq!(test_info[idx].0, open_protocol_info_list[0].1[idx].agent_handle);
      assert_eq!(test_info[idx].1, open_protocol_info_list[0].1[idx].controller_handle);
      assert_eq!(test_info[idx].2, open_protocol_info_list[0].1[idx].attributes);
      assert_eq!(1, open_protocol_info_list[0].1[idx].open_count);
    }
  }

  #[test]
  fn get_interface_for_handle_should_return_the_interface() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    let returned_iface = SPIN_LOCKED_PROTOCOL_DB.get_interface_for_handle(handle, guid1).unwrap();
    assert_eq!(interface1, returned_iface);
  }

  #[test]
  fn get_protocols_on_handle_should_return_protocols_on_handle() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let uuid2 = Uuid::from_str("98d32ea1-e980-46b5-bb2c-564934c8cce6").unwrap();
    let guid2: efi::Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
    let interface2: *mut c_void = 0x4321 as *mut c_void;

    let (handle, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(Some(handle), guid2, interface2).unwrap();

    let protocol_list = SPIN_LOCKED_PROTOCOL_DB.get_protocols_on_handle(handle).unwrap();
    assert_eq!(protocol_list.len(), 2);
    assert!(protocol_list.contains(&guid1));
    assert!(protocol_list.contains(&guid2));
  }

  #[test]
  fn locate_protocol_should_return_protocol() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let uuid2 = Uuid::from_str("98d32ea1-e980-46b5-bb2c-564934c8cce6").unwrap();
    let guid2: efi::Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
    let interface2: *mut c_void = 0x4321 as *mut c_void;

    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid2, interface2).unwrap();

    assert_eq!(SPIN_LOCKED_PROTOCOL_DB.locate_protocol(guid1), Ok(interface1));
    assert_eq!(SPIN_LOCKED_PROTOCOL_DB.locate_protocol(guid2), Ok(interface2));
  }

  #[test]
  fn register_protocol_notify_should_register_protocol_notify() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };

    let event = 0x1234 as *mut c_void;
    let result = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event);
    assert!(result.is_ok());
    assert!(!result.unwrap().is_null());

    {
      let notifications = &SPIN_LOCKED_PROTOCOL_DB.lock().notifications;
      assert_eq!(notifications.len(), 1);
      let notify_list = notifications.get(&OrdGuid(guid1)).unwrap();
      assert_eq!(notify_list.len(), 1);
      assert_eq!(notify_list[0].event, event);
      assert_eq!(notify_list[0].fresh_handles.len(), 0);
      assert_eq!(notify_list[0].registration, result.unwrap());
    }

    let event2 = 0x4321 as *mut c_void;
    let result = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event2);
    assert!(result.is_ok());
    assert!(!result.unwrap().is_null());

    {
      let notifications = &SPIN_LOCKED_PROTOCOL_DB.lock().notifications;
      assert_eq!(notifications.len(), 1);
      let notify_list = notifications.get(&OrdGuid(guid1)).unwrap();
      assert_eq!(notify_list.len(), 2);
      assert_eq!(notify_list[0].event, event);
      assert_eq!(notify_list[0].fresh_handles.len(), 0);

      assert_eq!(notify_list[1].event, event2);
      assert_eq!(notify_list[1].fresh_handles.len(), 0);
      assert_eq!(notify_list[1].registration, result.unwrap());
    }
  }
  #[test]
  fn install_protocol_interface_should_return_registered_notifies() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let event = 0x8765 as *mut c_void;
    let reg1 = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event).unwrap();
    let event2 = 0x4321 as *mut c_void;
    let reg2 = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event2).unwrap();

    let result = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1);
    assert!(result.is_ok());
    let result = result.unwrap();
    let notify_list = result.1;
    assert_eq!(notify_list.len(), 2);
    assert_eq!(notify_list[0].event, event);
    assert_eq!(notify_list[0].fresh_handles.len(), 1);
    assert!(notify_list[0].fresh_handles.get(&result.0).is_some());
    assert_eq!(notify_list[0].registration, reg1);

    assert_eq!(notify_list[1].event, event2);
    assert_eq!(notify_list[1].fresh_handles.len(), 1);
    assert!(notify_list[1].fresh_handles.get(&result.0).is_some());
    assert_eq!(notify_list[1].registration, reg2);
  }

  #[test]
  fn unregister_protocol_notifies_should_unregister_protocol_notifies() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let event = 0x8765 as *mut c_void;
    SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event).unwrap();
    let event2 = 0x4321 as *mut c_void;
    SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event2).unwrap();

    let (_, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    let events = notifies.iter().map(|x| x.event).collect();

    SPIN_LOCKED_PROTOCOL_DB.unregister_protocol_notify_events(events);

    let (_, notifies) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    assert_eq!(notifies.len(), 0);
  }

  #[test]
  fn next_handle_for_registration_should_return_next_handle_for_registration() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let event = 0x8765 as *mut c_void;
    let reg1 = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event).unwrap();
    let event2 = 0x4321 as *mut c_void;
    let reg2 = SPIN_LOCKED_PROTOCOL_DB.register_protocol_notify(guid1, event2).unwrap();

    let hnd1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap().0;
    let hnd2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap().0;
    let hnd3 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap().0;

    let result = SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(reg1);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), hnd1);

    let result = SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(reg1);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), hnd2);

    let result = SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(reg1);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), hnd3);

    let result = SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(reg2);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), hnd1);

    let result = SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(reg2);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), hnd2);

    let result = SPIN_LOCKED_PROTOCOL_DB.next_handle_for_registration(reg2);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), hnd3);
  }

  #[test]
  fn get_child_handles_should_return_child_handles() {
    static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

    let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
    let guid1: efi::Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
    let interface1: *mut c_void = 0x1234 as *mut c_void;

    let (controller, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (driver, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (child1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (child2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (child3, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (_notchild1, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
    let (_notchild2, _) = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

    for child in [child1, child2, child3] {
      SPIN_LOCKED_PROTOCOL_DB
        .add_protocol_usage(controller, guid1, Some(driver), Some(child), efi::OPEN_PROTOCOL_BY_CHILD_CONTROLLER)
        .unwrap();
    }

    let child_list = SPIN_LOCKED_PROTOCOL_DB.get_child_handles(controller);
    assert!(child_list.len() == 3);
    for child in [child1, child2, child3] {
      assert!(child_list.contains(&child));
    }
  }
}
