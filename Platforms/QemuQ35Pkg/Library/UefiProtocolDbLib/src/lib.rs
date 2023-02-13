//! # UEFI Protocol Database Lib
//! Provides implementation of the UEFI protocol database.
#![no_std]

extern crate alloc;

use alloc::{collections::BTreeMap, vec::Vec};
use core::{cmp::Ordering, ffi::c_void};
use r_efi::system::{
    OPEN_PROTOCOL_BY_CHILD_CONTROLLER, OPEN_PROTOCOL_BY_DRIVER, OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
    OPEN_PROTOCOL_EXCLUSIVE, OPEN_PROTOCOL_GET_PROTOCOL, OPEN_PROTOCOL_TEST_PROTOCOL,
};

/// # Open Protocol Information
/// This structure is used to track open protocol information on a handle.
#[derive(Clone, Copy, Debug)]
pub struct OpenProtocolInformation {
    agent_handle: Option<r_efi::efi::Handle>,
    controller_handle: Option<r_efi::efi::Handle>,
    attributes: u32,
    open_count: u32,
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
        handle: r_efi::efi::Handle,
        agent_handle: Option<r_efi::efi::Handle>,
        controller_handle: Option<r_efi::efi::Handle>,
        attributes: u32,
    ) -> Result<Self, r_efi::efi::Status> {
        const BY_DRIVER_EXCLUSIVE: u32 = OPEN_PROTOCOL_BY_DRIVER | OPEN_PROTOCOL_EXCLUSIVE;
        match attributes {
            OPEN_PROTOCOL_BY_CHILD_CONTROLLER => {
                if agent_handle.is_none() || controller_handle.is_none() || handle == controller_handle.unwrap() {
                    return Err(r_efi::efi::Status::INVALID_PARAMETER);
                }
            }
            OPEN_PROTOCOL_BY_DRIVER | BY_DRIVER_EXCLUSIVE => {
                if agent_handle.is_none() || controller_handle.is_none() {
                    return Err(r_efi::efi::Status::INVALID_PARAMETER);
                }
            }
            OPEN_PROTOCOL_EXCLUSIVE => {
                if agent_handle.is_none() {
                    return Err(r_efi::efi::Status::INVALID_PARAMETER);
                }
            }
            OPEN_PROTOCOL_BY_HANDLE_PROTOCOL | OPEN_PROTOCOL_GET_PROTOCOL | OPEN_PROTOCOL_TEST_PROTOCOL => (),
            _ => return Err(r_efi::efi::Status::INVALID_PARAMETER),
        }
        Ok(OpenProtocolInformation { agent_handle, controller_handle, attributes, open_count: 1 })
    }

    pub fn to_efi_open_protocol(&self) -> r_efi::system::OpenProtocolInformationEntry {
        r_efi::system::OpenProtocolInformationEntry {
            agent_handle: self.agent_handle.unwrap_or(core::ptr::null_mut()),
            controller_handle: self.controller_handle.unwrap_or(core::ptr::null_mut()),
            attributes: self.attributes,
            open_count: self.open_count,
        }
    }
}

struct ProtocolInstance {
    interface: *mut c_void,
    opened_by_driver: bool,
    opened_by_exclusive: bool,
    usage: Vec<OpenProtocolInformation>,
}

#[derive(Eq, PartialEq)]
struct OrdGuid(r_efi::efi::Guid);

impl PartialOrd for OrdGuid {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.as_bytes().partial_cmp(&other.0.as_bytes())
    }
}
impl Ord for OrdGuid {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.as_bytes().cmp(&other.0.as_bytes())
    }
}

// This is the main implementation of the protocol database, but public
// interaction with the database should be via [`SpinLockedProtocolDb`] below.
struct ProtocolDb {
    handles: Vec<BTreeMap<OrdGuid, ProtocolInstance>>,
}

impl ProtocolDb {
    const fn new() -> Self {
        ProtocolDb { handles: Vec::new() }
    }

    fn install_protocol_interface(
        &mut self,
        handle: Option<r_efi::efi::Handle>,
        protocol: r_efi::efi::Guid,
        interface: *mut c_void,
    ) -> Result<r_efi::efi::Handle, r_efi::efi::Status> {
        //generate an output handle.
        let (output_handle, index) = match handle {
            Some(handle) => {
                //installing on existing handle.
                if !self.validate_handle(handle) {
                    //handle is invalid.
                    return Err(r_efi::efi::Status::INVALID_PARAMETER);
                }
                let index = (handle as usize) - 1;
                (handle, index)
            }
            None => {
                //installing on a new handle. Add a BTreeSet to track protocol instances on the new handle.
                let index = self.handles.len();
                self.handles.push(BTreeMap::new());
                let handle = (index + 1) as r_efi::efi::Handle;
                (handle, index)
            }
        };
        assert!(index < self.handles.len()); //above logic should guarantee a valid index.

        if self.handles[index].contains_key(&OrdGuid(protocol)) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        //create a new protocol instance to match the input.
        let protocol_instance =
            ProtocolInstance { interface, opened_by_driver: false, opened_by_exclusive: false, usage: Vec::new() };

        //attempt to add the protocol to the set of protocols on this handle.
        assert!(self.handles[index].insert(OrdGuid(protocol), protocol_instance).is_none());

        Ok(output_handle)
    }

    fn uninstall_protocol_interface(
        &mut self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
        interface: *mut c_void,
    ) -> Result<(), r_efi::efi::Status> {
        if !self.validate_handle(handle) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        let index = handle as usize - 1;

        let instance = self.handles[index].get(&OrdGuid(protocol)).ok_or(r_efi::efi::Status::NOT_FOUND)?;

        if instance.interface != interface {
            return Err(r_efi::efi::Status::NOT_FOUND);
        }

        //Spec requires that an attempt to uninstall an installed protocol interface that is open with an attribute of
        //OPEN_PROTOCOL_BY_DRIVER should force a call to "Disconnect Controller" to attempt to release the interface
        //before uninstalling. This logic requires interaction with gBS->DisconnectController, which could in turn have
        //issues with deadlock since this routine is executing under a lock from SimpleLockedProtocolDb. As such, this
        //routine simply returns ACCESS_DENIED if any agents are found active on the protocol instance, and leaves the
        //disconnect logic to the caller (outside this library), which is free to DisconnectController() before
        //attempting this call.
        for agent in &instance.usage {
            if (agent.attributes & r_efi::efi::OPEN_PROTOCOL_BY_DRIVER) != 0 {
                return Err(r_efi::efi::Status::ACCESS_DENIED);
            }
        }
        self.handles[index].remove(&OrdGuid(protocol)).unwrap();
        Ok(())
    }

    //Note: reinstall gets its own routine (instead of having caller call uninstall/install) to handle the corner case
    //where the only protocol interface on a handle is being reinstalled; this means that the handle is technically
    //empty (i.e. invalid) between the uninstall/install, and so install would fail in this case due to invalid handle.
    //The logic of reinstall is otherwise equivalent to uninstall followed by reinstall.
    fn reinstall_protocol_interface(
        &mut self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
        old_interface: *mut c_void,
        new_interface: *mut c_void,
    ) -> Result<(), r_efi::efi::Status> {
        if !self.validate_handle(handle) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        let index = handle as usize - 1;

        let instance = self.handles[index].get(&OrdGuid(protocol)).ok_or(r_efi::efi::Status::NOT_FOUND)?;

        if instance.interface != old_interface {
            return Err(r_efi::efi::Status::NOT_FOUND);
        }

        //Spec requires that an attempt to uninstall an installed protocol interface that is open with an attribute of
        //OPEN_PROTOCOL_BY_DRIVER should force a call to "Disconnect Controller" to attempt to release the interface
        //before uninstalling. This logic requires interaction with gBS->DisconnectController, which could in turn have
        //issues with deadlock since this routine is executing under a lock from SimpleLockedProtocolDb. As such, this
        //routine simply returns ACCESS_DENIED if any agents are found active on the protocol instance, and leaves the
        //disconnect logic to the caller (outside this library), which is free to DisconnectController() before
        //attempting this call.
        for agent in &instance.usage {
            if (agent.attributes & r_efi::efi::OPEN_PROTOCOL_BY_DRIVER) != 0 {
                return Err(r_efi::efi::Status::ACCESS_DENIED);
            }
        }
        self.handles[index].remove(&OrdGuid(protocol)).unwrap();

        //create a new protocol instance to match the input.
        let protocol_instance = ProtocolInstance {
            interface: new_interface,
            opened_by_driver: false,
            opened_by_exclusive: false,
            usage: Vec::new(),
        };

        //attempt to add the protocol to the set of protocols on this handle.
        assert!(self.handles[index].insert(OrdGuid(protocol), protocol_instance).is_none());
        Ok(())
    }

    fn locate_handles(
        &mut self,
        protocol: Option<r_efi::efi::Guid>,
    ) -> Result<Vec<r_efi::efi::Handle>, r_efi::efi::Status> {
        let handles: Vec<r_efi::efi::Handle> = self
            .handles
            .iter()
            .zip(0..self.handles.len())
            .filter_map(|(instance, index)| {
                if protocol.is_none() || instance.contains_key(&OrdGuid(protocol.unwrap())) {
                    Some((index + 1) as r_efi::efi::Handle)
                } else {
                    None
                }
            })
            .collect();
        if handles.len() == 0 {
            return Err(r_efi::efi::Status::NOT_FOUND);
        }
        Ok(handles)
    }

    fn locate_protocol(&mut self, protocol: r_efi::efi::Guid) -> Result<*mut c_void, r_efi::efi::Status> {
        let interface = self.handles.iter().find_map(|x| x.get(&OrdGuid(protocol)));

        match interface {
            Some(interface) => Ok(interface.interface),
            None => Err(r_efi::efi::Status::NOT_FOUND),
        }
    }

    fn get_interface_for_handle(
        &mut self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
    ) -> Result<*mut c_void, r_efi::efi::Status> {
        if !self.validate_handle(handle) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }
        let index = handle as usize - 1;
        let instance = self.handles[index].get_mut(&OrdGuid(protocol)).ok_or(r_efi::efi::Status::UNSUPPORTED)?;
        Ok(instance.interface)
    }

    fn validate_handle(&self, handle: r_efi::efi::Handle) -> bool {
        let handle = handle as usize;
        //to be valid, handle must be in the range of handles created,
        if !(handle > 0 && handle <= self.handles.len()) {
            return false;
        }
        //and has to have at least one protocol installed on it.
        let index = handle as usize - 1;
        self.handles[index].len() != 0
    }

    fn add_protocol_usage(
        &mut self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
        agent_handle: Option<r_efi::efi::Handle>,
        controller_handle: Option<r_efi::efi::Handle>,
        attributes: u32,
    ) -> Result<(), r_efi::efi::Status> {
        if !self.validate_handle(handle) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        if agent_handle.is_some() && !self.validate_handle(agent_handle.unwrap()) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        if controller_handle.is_some() && !self.validate_handle(controller_handle.unwrap()) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        let index = handle as usize - 1;
        let instance = self.handles[index].get_mut(&OrdGuid(protocol)).ok_or(r_efi::efi::Status::UNSUPPORTED)?;

        let new_using_agent = OpenProtocolInformation::new(handle, agent_handle, controller_handle, attributes)?;
        let exact_match = instance.usage.iter_mut().find(|user| user == &&new_using_agent);

        if instance.opened_by_driver && exact_match.is_some() {
            return Err(r_efi::efi::Status::ALREADY_STARTED);
        }

        if !instance.opened_by_exclusive && exact_match.is_some() {
            exact_match.unwrap().open_count += 1;
            return Ok(());
        }

        const BY_DRIVER_EXCLUSIVE: u32 = OPEN_PROTOCOL_BY_DRIVER | OPEN_PROTOCOL_EXCLUSIVE;
        match attributes {
            OPEN_PROTOCOL_BY_DRIVER | OPEN_PROTOCOL_EXCLUSIVE | BY_DRIVER_EXCLUSIVE => {
                //Note: Per UEFI spec, a request to open with OPEN_PROTOCOL_EXCLUSIVE set should result in a disconnect
                //of existing controllers that have the driver OPEN_PROTOCOL_BY_DRIVER. This needs to be done in the
                //caller, since this library doesn't have access to DisconnectController, and is also executing under
                //the SpinLockedProtocolDb lock (which would cause deadlock if DisconnectController attempted to use
                //any of the protocol services). Instead, return ACCESS_DENIED.
                if instance.opened_by_exclusive || instance.opened_by_driver {
                    return Err(r_efi::efi::Status::ACCESS_DENIED);
                }
            }
            OPEN_PROTOCOL_BY_CHILD_CONTROLLER
            | OPEN_PROTOCOL_BY_HANDLE_PROTOCOL
            | OPEN_PROTOCOL_GET_PROTOCOL
            | OPEN_PROTOCOL_TEST_PROTOCOL => (),
            _ => panic!("Unsupported attributes: {:#x?}", attributes), //this should have been dealt with in ProtocolUsingAgent::new().
        }

        if agent_handle.is_none() {
            return Ok(()); //don't add the new using_agent if no agent is actually specified.
        }

        if (new_using_agent.attributes & OPEN_PROTOCOL_BY_DRIVER) != 0 {
            instance.opened_by_driver = true;
        }
        if (new_using_agent.attributes & OPEN_PROTOCOL_EXCLUSIVE) != 0 {
            instance.opened_by_exclusive = true;
        }
        instance.usage.push(new_using_agent);

        Ok(())
    }

    fn remove_protocol_usage(
        &mut self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
        agent_handle: Option<r_efi::efi::Handle>,
        controller_handle: Option<r_efi::efi::Handle>,
    ) -> Result<(), r_efi::efi::Status> {
        if !self.validate_handle(handle) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        if agent_handle.is_some() && !self.validate_handle(agent_handle.unwrap()) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        if controller_handle.is_some() && !self.validate_handle(controller_handle.unwrap()) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }
        let index = handle as usize - 1;
        let instance = self.handles[index].get_mut(&OrdGuid(protocol)).ok_or(r_efi::efi::Status::UNSUPPORTED)?;
        let mut removed = false;
        instance.usage.retain(|x| {
            if (x.agent_handle == agent_handle) && (x.controller_handle == controller_handle) {
                //if we are removing the usage that had this instance open by driver (there should be only one)
                //then clear the flag that the instance was opened by driver.
                if (x.attributes & OPEN_PROTOCOL_BY_DRIVER) != 0 {
                    instance.opened_by_driver = false;
                }
                //if we are removing the usage that had this instance open exclusive (there should be only one)
                //then clear the flag that the instance was opened exclusive.
                if (x.attributes & OPEN_PROTOCOL_EXCLUSIVE) != 0 {
                    instance.opened_by_exclusive = false;
                }
                removed = true;
                false //if agent and controller match, do not retain (i.e. remove).
            } else {
                true //if one or the other or both don't match, retain.
            }
        });

        if !removed {
            return Err(r_efi::efi::Status::NOT_FOUND);
        }

        Ok(())
    }

    fn get_open_protocol_information(
        &mut self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
    ) -> Result<Vec<OpenProtocolInformation>, r_efi::efi::Status> {
        if !self.validate_handle(handle) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        let index = handle as usize - 1;
        let instance = self.handles[index].get_mut(&OrdGuid(protocol)).ok_or(r_efi::efi::Status::NOT_FOUND)?;

        Ok(instance.usage.clone())
    }

    fn get_protocols_on_handle(
        &mut self,
        handle: r_efi::efi::Handle,
    ) -> Result<Vec<r_efi::efi::Guid>, r_efi::efi::Status> {
        if !self.validate_handle(handle) {
            return Err(r_efi::efi::Status::INVALID_PARAMETER);
        }

        let index = handle as usize - 1;
        Ok(self.handles[index].keys().clone().map(|x| x.0).collect())
    }
}

/// # Spin-Locked UEFI Protocol Database
/// Implements UEFI protocol database support using a spinlock for mutex.
pub struct SpinLockedProtocolDb {
    inner: spin::Mutex<ProtocolDb>,
}

impl SpinLockedProtocolDb {
    /// Creates a new instance of SpinLockedProtocolDb.
    pub const fn new() -> Self {
        SpinLockedProtocolDb { inner: spin::Mutex::new(ProtocolDb::new()) }
    }

    fn lock(&self) -> spin::MutexGuard<ProtocolDb> {
        self.inner.lock()
    }

    /// Installs a protocol interface on the given handle.
    pub fn install_protocol_interface(
        &self,
        handle: Option<r_efi::efi::Handle>,
        guid: r_efi::efi::Guid,
        interface: *mut c_void,
    ) -> Result<r_efi::efi::Handle, r_efi::efi::Status> {
        self.lock().install_protocol_interface(handle, guid, interface)
    }

    /// Removes a protocol interface from the given handle.
    pub fn uninstall_protocol_interface(
        &self,
        handle: r_efi::efi::Handle,
        guid: r_efi::efi::Guid,
        interface: *mut c_void,
    ) -> Result<(), r_efi::efi::Status> {
        self.lock().uninstall_protocol_interface(handle, guid, interface)
    }

    /// Replaces an interface on the given handle with a new one.
    pub fn reinstall_protocol_interface(
        &self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
        old_interface: *mut c_void,
        new_interface: *mut c_void,
    ) -> Result<(), r_efi::efi::Status> {
        self.lock().reinstall_protocol_interface(handle, protocol, old_interface, new_interface)
    }

    /// Returns a vector of handles that have the specified protocol installed on them.
    pub fn locate_handles(
        &self,
        protocol: Option<r_efi::efi::Guid>,
    ) -> Result<Vec<r_efi::efi::Handle>, r_efi::efi::Status> {
        self.lock().locate_handles(protocol)
    }

    /// Returns the first instance of the specified protocol interface from any handle
    pub fn locate_protocol(&self, protocol: r_efi::efi::Guid) -> Result<*mut c_void, r_efi::efi::Status> {
        self.lock().locate_protocol(protocol)
    }

    /// Returns the interface for the specified protocol on the given handle if it exists
    pub fn get_interface_for_handle(
        &self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
    ) -> Result<*mut c_void, r_efi::efi::Status> {
        self.lock().get_interface_for_handle(handle, protocol)
    }

    /// Returns true if the handle is a valid handle, false otherwise.
    pub fn validate_handle(&self, handle: r_efi::efi::Handle) -> bool {
        self.lock().validate_handle(handle)
    }

    /// Adds a protocol usage on the specified handle/protocol.
    /// Implementation generally follows the behavior of "OpenProtocol" Boot Services API from the UEFI spec,
    /// except for requirements for UEFI driver model interactions which caller must manage.
    pub fn add_protocol_usage(
        &self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
        agent_handle: Option<r_efi::efi::Handle>,
        controller_handle: Option<r_efi::efi::Handle>,
        attributes: u32,
    ) -> Result<(), r_efi::efi::Status> {
        self.lock().add_protocol_usage(handle, protocol, agent_handle, controller_handle, attributes)
    }

    /// Removes a protocol usage from the specified handle/protocol.
    /// Implementation generally follows the behavior of "CloseProtocol" Boot Services API from the UEFI spec.
    pub fn remove_protocol_usage(
        &self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
        agent_handle: Option<r_efi::efi::Handle>,
        controller_handle: Option<r_efi::efi::Handle>,
    ) -> Result<(), r_efi::efi::Status> {
        self.lock().remove_protocol_usage(handle, protocol, agent_handle, controller_handle)
    }

    /// Returns open protocol information for the given handle/protocol.
    pub fn get_open_protocol_information(
        &self,
        handle: r_efi::efi::Handle,
        protocol: r_efi::efi::Guid,
    ) -> Result<Vec<OpenProtocolInformation>, r_efi::efi::Status> {
        self.lock().get_open_protocol_information(handle, protocol)
    }

    /// Returns the list of protocols that are open on the given handle.
    pub fn get_protocols_on_handle(
        &self,
        handle: r_efi::efi::Handle,
    ) -> Result<Vec<r_efi::efi::Guid>, r_efi::efi::Status> {
        self.lock().get_protocols_on_handle(handle)
    }
}

unsafe impl Send for SpinLockedProtocolDb {}
unsafe impl Sync for SpinLockedProtocolDb {}

#[cfg(test)]
mod tests {
    extern crate std;
    use core::str::FromStr;
    use std::println;

    use r_efi::{efi::Guid, system::OPEN_PROTOCOL_BY_DRIVER};
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
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        assert_ne!(handle, 0 as *mut c_void);
        let test_instance = ProtocolInstance {
            interface: interface1,
            opened_by_driver: false,
            opened_by_exclusive: false,
            usage: Vec::new(),
        };
        let index = handle as usize - 1;
        let protocol_instance = &SPIN_LOCKED_PROTOCOL_DB.lock().handles[index];
        let created_instance = protocol_instance.get(&OrdGuid(guid1)).unwrap();
        assert_eq!(test_instance.interface, created_instance.interface);
    }

    #[test]
    fn uninstall_protocol_interface_should_uninstall_protocol_interface() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let index = handle as usize - 1;

        SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid1, interface1).unwrap();

        let protocol_instance = &SPIN_LOCKED_PROTOCOL_DB.lock().handles[index];
        assert_eq!(protocol_instance.contains_key(&OrdGuid(guid1)), false);
    }

    #[test]
    fn uninstall_protocol_interface_should_give_access_denied_if_interface_in_use() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let index = handle as usize - 1;

        // fish out the created instance, and add a fake ProtocolUsingAgent to simulate the
        // protocol being "OPEN_PROTOCOL_BY_DRIVER"
        let mut instance = SPIN_LOCKED_PROTOCOL_DB.lock().handles[index].remove(&OrdGuid(guid1)).unwrap();

        instance.usage.push(OpenProtocolInformation {
            agent_handle: None,
            controller_handle: None,
            attributes: OPEN_PROTOCOL_BY_DRIVER,
            open_count: 1,
        });

        SPIN_LOCKED_PROTOCOL_DB.lock().handles[index].insert(OrdGuid(guid1), instance);

        let err = SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid1, interface1);
        assert_eq!(err, Err(r_efi::efi::Status::ACCESS_DENIED));

        let protocol_instance = &SPIN_LOCKED_PROTOCOL_DB.lock().handles[index];
        assert_eq!(protocol_instance.contains_key(&OrdGuid(guid1)), true);
    }

    #[test]
    fn uninstall_protocol_interface_should_give_not_found_if_not_found() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let uuid2 = Uuid::from_str("9c5dca1d-ac0f-46db-9eba-2bc961c711a2").unwrap();
        let guid2: Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
        let interface2: *mut c_void = 0x4321 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        let err = SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid2, interface1);
        assert_eq!(err, Err(r_efi::efi::Status::NOT_FOUND));

        let err = SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle, guid1, interface2);
        assert_eq!(err, Err(r_efi::efi::Status::NOT_FOUND));
    }

    #[test]
    fn reinstall_protocol_interface_should_replace_the_interface() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;
        let interface2: *mut c_void = 0x4321 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let index = handle as usize - 1;

        SPIN_LOCKED_PROTOCOL_DB.reinstall_protocol_interface(handle, guid1, interface1, interface2).unwrap();
        let protocol_instance = &SPIN_LOCKED_PROTOCOL_DB.lock().handles[index];
        assert_eq!(protocol_instance.get(&OrdGuid(guid1)).unwrap().interface, interface2);
    }

    #[test]
    fn locate_handle_should_locate_handles() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let uuid2 = Uuid::from_str("9c5dca1d-ac0f-46db-9eba-2bc961c711a2").unwrap();
        let guid2: Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
        let interface2: *mut c_void = 0x4321 as *mut c_void;

        let uuid3 = Uuid::from_str("2a32017e-7e6b-4563-890d-fff945530438").unwrap();
        let guid3: Guid = unsafe { core::mem::transmute(*uuid3.as_bytes()) };

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        assert_eq!(
            handle1,
            SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(Some(handle1), guid2, interface2).unwrap()
        );
        let handle2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle3 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle4 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        assert_eq!(
            handle4,
            SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(Some(handle4), guid2, interface2).unwrap()
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

        assert_eq!(SPIN_LOCKED_PROTOCOL_DB.locate_handles(Some(guid3)), Err(r_efi::efi::Status::NOT_FOUND));
    }

    #[test]
    fn validate_handle_should_validate_good_handles_and_reject_bad_ones() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        assert!(SPIN_LOCKED_PROTOCOL_DB.validate_handle(handle1));
        let handle2 = (handle1 as usize + 1) as r_efi::efi::Handle;
        assert!(!SPIN_LOCKED_PROTOCOL_DB.validate_handle(handle2));
    }

    #[test]
    fn validate_handle_empty_handles_are_invalid() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        SPIN_LOCKED_PROTOCOL_DB.uninstall_protocol_interface(handle1, guid1, interface1).unwrap();
        assert!(!SPIN_LOCKED_PROTOCOL_DB.validate_handle(handle1));
    }

    #[test]
    fn add_protocol_usage_should_update_protocol_usages() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle3 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        //Adding a usage
        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), OPEN_PROTOCOL_GET_PROTOCOL)
            .unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        drop(protocol_db);

        //Adding the exact same usage should not create a new usage; it should update open_count
        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), OPEN_PROTOCOL_GET_PROTOCOL)
            .unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(2, protocol_user_list[0].open_count);
        drop(protocol_db);
    }
    #[test]
    fn add_protocol_usage_by_child_controller() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle3 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle4 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        //Adding a usage BY_CHILD_CONTROLLER should succeed.
        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), OPEN_PROTOCOL_BY_CHILD_CONTROLLER)
            .unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        drop(protocol_db);

        //Adding a protocol BY_CHILD_CONTROLLER should fail if agent and controller not specified.
        let result =
            SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, None, None, OPEN_PROTOCOL_BY_CHILD_CONTROLLER);
        assert_eq!(result, Err(r_efi::efi::Status::INVALID_PARAMETER));
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        drop(protocol_db);

        //Adding a protocol BY_CHILD_CONTROLLER should fail if controller_handle matches handle.
        let result = SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
            handle1,
            guid1,
            Some(handle2),
            Some(handle1),
            OPEN_PROTOCOL_BY_CHILD_CONTROLLER,
        );
        assert_eq!(result, Err(r_efi::efi::Status::INVALID_PARAMETER));
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        drop(protocol_db);

        //Adding a protocol BY_CHILD_CONTROLLER should succeed even if another agent has protocol open on handle with "exclusive".
        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle4, guid1, Some(handle2), Some(handle1), OPEN_PROTOCOL_EXCLUSIVE)
            .unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle4 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        assert_eq!(OPEN_PROTOCOL_EXCLUSIVE, protocol_user_list[0].attributes);
        drop(protocol_db);

        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle4, guid1, Some(handle2), Some(handle3), OPEN_PROTOCOL_BY_CHILD_CONTROLLER)
            .unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle4 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(2, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        assert_eq!(1, protocol_user_list[1].open_count);
        assert_eq!(OPEN_PROTOCOL_EXCLUSIVE, protocol_user_list[0].attributes);
        assert_eq!(OPEN_PROTOCOL_BY_CHILD_CONTROLLER, protocol_user_list[1].attributes);
        drop(protocol_db);
    }

    fn test_driver_and_exclusive_protocol_usage(test_attributes: u32) {
        println!("Testing add_protocol_usage for attributes: {:#x?}", test_attributes);
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle3 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle4 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        //Adding a usage BY_DRIVER should succeed if no other handles are in the database.
        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), test_attributes)
            .unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        assert_eq!(test_attributes, protocol_user_list[0].attributes);
        drop(protocol_db);

        //Adding the same usage with same attributes again should result in ALREADY_STARTED if it was opened BY_DRIVER.
        if (test_attributes & OPEN_PROTOCOL_BY_DRIVER) != 0 {
            let result = SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
                handle1,
                guid1,
                Some(handle2),
                Some(handle3),
                test_attributes,
            );
            assert_eq!(result, Err(r_efi::efi::Status::ALREADY_STARTED));
            let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
            let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
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
            OPEN_PROTOCOL_BY_DRIVER,
        );
        assert_eq!(result, Err(r_efi::efi::Status::ACCESS_DENIED));
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
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
            OPEN_PROTOCOL_EXCLUSIVE,
        );
        assert_eq!(result, Err(r_efi::efi::Status::ACCESS_DENIED));
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        assert_eq!(test_attributes, protocol_user_list[0].attributes);
        drop(protocol_db);

        //Adding a usage BY_CHILD_CONTROLLER should result in a new usage record.
        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle1, guid1, Some(handle4), Some(handle3), OPEN_PROTOCOL_BY_CHILD_CONTROLLER)
            .unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(2, protocol_user_list.len());
        assert_eq!(test_attributes, protocol_user_list[0].attributes);
        assert_eq!(1, protocol_user_list[0].open_count);
        assert_eq!(OPEN_PROTOCOL_BY_CHILD_CONTROLLER, protocol_user_list[1].attributes);
        assert_eq!(1, protocol_user_list[1].open_count);
        drop(protocol_db);
    }

    #[test]
    fn add_protocol_usage_by_driver_and_exclusive() {
        //For this library implementation, BY_DRIVER, EXCLUSIVE, and BY_DRIVER_EXCLUSIVE function identically (except
        //for the contents of the attributes field in the usage record). See note in [`add_protocol_usage()`] above for
        //further discussion.
        for test_attributes in
            [OPEN_PROTOCOL_BY_DRIVER, OPEN_PROTOCOL_EXCLUSIVE, OPEN_PROTOCOL_BY_DRIVER | OPEN_PROTOCOL_EXCLUSIVE]
        {
            test_driver_and_exclusive_protocol_usage(test_attributes);
        }
    }

    fn test_handle_get_or_test_protocol_usage(test_attributes: u32) {
        println!("Testing add_protocol_usage for attributes: {:#x?}", test_attributes);
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle3 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle4 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        //Adding a usage should succeed if no other handles are in the database.
        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), test_attributes)
            .unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        assert_eq!(test_attributes, protocol_user_list[0].attributes);
        drop(protocol_db);

        //Adding a usage should succeed even if agent_handle is None, but new record should not be added.
        SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, None, Some(handle3), test_attributes).unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        assert_eq!(test_attributes, protocol_user_list[0].attributes);
        drop(protocol_db);

        //Adding a usage should succeed even if agent_handle is None and ControllerHandle is node, but new record should not be added.
        SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, None, None, test_attributes).unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[0].open_count);
        assert_eq!(test_attributes, protocol_user_list[0].attributes);
        drop(protocol_db);

        //Adding a usage should succeed even if controller_handle is none, and a new record should be added.
        SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle1, guid1, Some(handle2), None, test_attributes).unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
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
                OPEN_PROTOCOL_BY_DRIVER | OPEN_PROTOCOL_EXCLUSIVE,
            )
            .unwrap();

        //Adding a usage should succeed even though the handle is already open BY_DRIVER_EXCLUSIVE
        SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(handle4, guid1, Some(handle2), None, test_attributes).unwrap();
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(2, protocol_user_list.len());
        assert_eq!(1, protocol_user_list[1].open_count);
        assert_eq!(test_attributes, protocol_user_list[1].attributes);
        drop(protocol_db);
    }

    #[test]
    fn add_protocol_usage_by_handle_get_or_test() {
        for test_attributes in
            [OPEN_PROTOCOL_BY_HANDLE_PROTOCOL, OPEN_PROTOCOL_GET_PROTOCOL, OPEN_PROTOCOL_TEST_PROTOCOL]
        {
            test_handle_get_or_test_protocol_usage(test_attributes);
        }
    }

    #[test]
    fn remove_protocol_usage_should_succeed_regardless_of_attributes() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let agent = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let controller = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        for attributes in [
            OPEN_PROTOCOL_BY_CHILD_CONTROLLER,
            OPEN_PROTOCOL_BY_DRIVER,
            OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
            OPEN_PROTOCOL_EXCLUSIVE,
            OPEN_PROTOCOL_BY_DRIVER | OPEN_PROTOCOL_EXCLUSIVE,
            OPEN_PROTOCOL_GET_PROTOCOL,
            OPEN_PROTOCOL_TEST_PROTOCOL,
        ] {
            let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
            SPIN_LOCKED_PROTOCOL_DB
                .add_protocol_usage(handle, guid1, Some(agent), Some(controller), attributes)
                .unwrap();
            SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle, guid1, Some(agent), Some(controller)).unwrap();
            let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
            let protocol_user_list = &protocol_db.handles[(handle as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
            assert_eq!(0, protocol_user_list.len());
            drop(protocol_db);
        }
    }

    #[test]
    fn remove_protocol_usage_should_return_not_found_if_usage_not_found() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle3 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), OPEN_PROTOCOL_BY_DRIVER)
            .unwrap();

        let result = SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, Some(handle3), Some(handle2));
        assert_eq!(result, Err(r_efi::efi::Status::NOT_FOUND));
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        drop(protocol_db);

        let result = SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, None, Some(handle3));
        assert_eq!(result, Err(r_efi::efi::Status::NOT_FOUND));
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        drop(protocol_db);

        let result = SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, Some(handle2), None);
        assert_eq!(result, Err(r_efi::efi::Status::NOT_FOUND));
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        drop(protocol_db);

        let result = SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, None, None);
        assert_eq!(result, Err(r_efi::efi::Status::NOT_FOUND));
        let protocol_db = SPIN_LOCKED_PROTOCOL_DB.lock();
        let protocol_user_list = &protocol_db.handles[(handle1 as usize) - 1].get(&OrdGuid(guid1)).unwrap().usage;
        assert_eq!(1, protocol_user_list.len());
        drop(protocol_db);
    }

    #[test]
    fn add_protocol_usage_should_succeed_after_remove_protocol_usage() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle1 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle3 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle4 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle1, guid1, Some(handle2), Some(handle3), OPEN_PROTOCOL_BY_DRIVER)
            .unwrap();

        //adding it agin with different agent handle should fail with access denied.
        assert_eq!(
            SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
                handle1,
                guid1,
                Some(handle4),
                Some(handle3),
                OPEN_PROTOCOL_BY_DRIVER
            ),
            Err(r_efi::efi::Status::ACCESS_DENIED)
        );

        SPIN_LOCKED_PROTOCOL_DB.remove_protocol_usage(handle1, guid1, Some(handle2), Some(handle3)).unwrap();

        //adding it agin with different agent handle should succeed.
        assert_eq!(
            SPIN_LOCKED_PROTOCOL_DB.add_protocol_usage(
                handle1,
                guid1,
                Some(handle4),
                Some(handle3),
                OPEN_PROTOCOL_BY_DRIVER
            ),
            Ok(())
        );
    }

    #[test]
    fn get_open_protocol_information_returns_information() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let attributes_list = [
            OPEN_PROTOCOL_BY_DRIVER | OPEN_PROTOCOL_EXCLUSIVE,
            OPEN_PROTOCOL_BY_CHILD_CONTROLLER,
            OPEN_PROTOCOL_BY_HANDLE_PROTOCOL,
            OPEN_PROTOCOL_GET_PROTOCOL,
            OPEN_PROTOCOL_TEST_PROTOCOL,
        ];

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let mut test_info = Vec::new();
        for attributes in attributes_list {
            let agent = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
            let controller = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
            test_info.push((Some(agent), Some(controller), attributes));
            SPIN_LOCKED_PROTOCOL_DB
                .add_protocol_usage(handle, guid1, Some(agent), Some(controller), attributes)
                .unwrap();
        }

        let open_protocol_info_list = SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information(handle, guid1).unwrap();
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
    fn get_open_protocol_information_should_return_not_found_if_handle_or_protocol_not_present() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let uuid2 = Uuid::from_str("98d32ea1-e980-46b5-bb2c-564934c8cce6").unwrap();
        let guid2: Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
        let interface2: *mut c_void = 0x4321 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let handle2 = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid2, interface2).unwrap();
        let agent = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let controller = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle, guid1, Some(agent), Some(controller), OPEN_PROTOCOL_BY_DRIVER)
            .unwrap();

        let result = SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information(handle, guid2);
        assert_eq!(result, Err(r_efi::efi::Status::NOT_FOUND));

        let result = SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information(handle2, guid1);
        assert_eq!(result, Err(r_efi::efi::Status::NOT_FOUND));
    }

    #[test]
    fn to_efi_open_protocol_should_match_source_open_protocol_information_entry() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let agent = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        let controller = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        SPIN_LOCKED_PROTOCOL_DB
            .add_protocol_usage(handle, guid1, Some(agent), Some(controller), OPEN_PROTOCOL_BY_DRIVER)
            .unwrap();

        for info in SPIN_LOCKED_PROTOCOL_DB.get_open_protocol_information(handle, guid1).unwrap() {
            let efi_info = info.to_efi_open_protocol();
            assert_eq!(efi_info.agent_handle, info.agent_handle.unwrap());
            assert_eq!(efi_info.controller_handle, info.controller_handle.unwrap());
            assert_eq!(efi_info.attributes, info.attributes);
            assert_eq!(efi_info.open_count, info.open_count);
        }
    }

    #[test]
    fn get_interface_for_handle_should_return_the_interface() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();

        let returned_iface = SPIN_LOCKED_PROTOCOL_DB.get_interface_for_handle(handle, guid1).unwrap();
        assert_eq!(interface1, returned_iface);
    }

    #[test]
    fn get_protocols_on_handle_should_return_protocols_on_handle() {
        static SPIN_LOCKED_PROTOCOL_DB: SpinLockedProtocolDb = SpinLockedProtocolDb::new();

        let uuid1 = Uuid::from_str("0e896c7a-57dc-4987-bc22-abc3a8263210").unwrap();
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let uuid2 = Uuid::from_str("98d32ea1-e980-46b5-bb2c-564934c8cce6").unwrap();
        let guid2: Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
        let interface2: *mut c_void = 0x4321 as *mut c_void;

        let handle = SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
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
        let guid1: Guid = unsafe { core::mem::transmute(*uuid1.as_bytes()) };
        let interface1: *mut c_void = 0x1234 as *mut c_void;

        let uuid2 = Uuid::from_str("98d32ea1-e980-46b5-bb2c-564934c8cce6").unwrap();
        let guid2: Guid = unsafe { core::mem::transmute(*uuid2.as_bytes()) };
        let interface2: *mut c_void = 0x4321 as *mut c_void;

        SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid1, interface1).unwrap();
        SPIN_LOCKED_PROTOCOL_DB.install_protocol_interface(None, guid2, interface2).unwrap();

        assert_eq!(SPIN_LOCKED_PROTOCOL_DB.locate_protocol(guid1), Ok(interface1));
        assert_eq!(SPIN_LOCKED_PROTOCOL_DB.locate_protocol(guid2), Ok(interface2));
    }
}
