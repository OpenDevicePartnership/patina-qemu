use alloc::{vec, vec::Vec};
use core::{mem::size_of, ptr::NonNull, slice::from_raw_parts_mut};

use r_efi::{
  efi::{Boolean, Handle, Status},
  protocols::{device_path, driver_binding},
  system::{BootServices, OPEN_PROTOCOL_BY_CHILD_CONTROLLER, OPEN_PROTOCOL_BY_DRIVER},
};

use crate::protocols::PROTOCOL_DB;

fn get_bindings_for_handles(handles: Vec<Handle>) -> Vec<*mut driver_binding::Protocol> {
  handles
    .iter()
    .filter_map(|x| {
      match PROTOCOL_DB.get_interface_for_handle(x.clone(), driver_binding::PROTOCOL_GUID) {
        Ok(interface) => Some(interface as *mut driver_binding::Protocol),
        Err(_) => None, //ignore handles without driver bindings
      }
    })
    .collect()
}

fn get_platform_driver_override_bindings(_controller_handle: Handle) -> Vec<*mut driver_binding::Protocol> {
  //TODO: implementing this requires adding definition for the Platform Driver Override protocol to r_efi.
  Vec::new()
}

fn get_family_override_bindings() -> Vec<*mut driver_binding::Protocol> {
  //TODO: implementing this requires adding definition for the Driver Family Override protocol to r_efi.
  Vec::new()
}

fn get_bus_specific_override_bindings(_controller_handle: Handle) -> Vec<*mut driver_binding::Protocol> {
  //TODO: implementing this requires adding definition for the Bus Specific Driver Override protocol to r_efi.
  Vec::new()
}

fn get_all_driver_bindings() -> Vec<*mut driver_binding::Protocol> {
  let mut driver_bindings = match PROTOCOL_DB.locate_handles(Some(driver_binding::PROTOCOL_GUID)) {
    Err(_) => return Vec::new(),
    Ok(handles) if handles.len() == 0 => return Vec::new(),
    Ok(handles) => get_bindings_for_handles(handles),
  };

  driver_bindings.sort_unstable_by(|a, b| unsafe { (*(*b)).version.cmp(&(*(*a)).version) });

  driver_bindings
}

fn core_connect_single_controller(
  controller_handle: Handle,
  driver_handles: Vec<Handle>,
  remaining_device_path: Option<*mut device_path::Protocol>,
) -> Result<(), Status> {
  PROTOCOL_DB.validate_handle(controller_handle)?;

  //The following sources for driver instances are considered per UEFI Spec 2.10 section 7.3.12:
  //1. Context Override
  let mut driver_candidates = Vec::new();
  driver_candidates.extend(get_bindings_for_handles(driver_handles));

  //2. Platform Driver Override
  let mut platform_override_drivers = get_platform_driver_override_bindings(controller_handle);
  platform_override_drivers.retain(|x| !driver_candidates.contains(x));
  driver_candidates.append(&mut platform_override_drivers);

  //3. Driver Family Override Search
  let mut family_override_drivers = get_family_override_bindings();
  family_override_drivers.retain(|x| !driver_candidates.contains(x));
  driver_candidates.append(&mut family_override_drivers);

  //4. Bus Specific Driver Override
  let mut bus_override_drivers = get_bus_specific_override_bindings(controller_handle);
  bus_override_drivers.retain(|x| !driver_candidates.contains(x));
  driver_candidates.append(&mut bus_override_drivers);

  //5. Driver Binding Search
  let mut driver_bindings = get_all_driver_bindings();
  driver_bindings.retain(|x| !driver_candidates.contains(x));
  driver_candidates.append(&mut driver_bindings);

  //loop until no more drivers can be started on handle.
  let mut one_started = false;
  loop {
    let mut started_drivers = Vec::new();
    for driver_binding_interface in driver_candidates.clone() {
      let driver_binding = unsafe { &mut *(driver_binding_interface) };
      let device_path =
        remaining_device_path.or(Some(core::ptr::null_mut() as *mut device_path::Protocol)).expect("must be some");
      match (driver_binding.supported)(driver_binding_interface, controller_handle, device_path) {
        Status::SUCCESS => {
          //driver claims support; attempt to start it.
          started_drivers.push(driver_binding_interface);
          if (driver_binding.start)(driver_binding_interface, controller_handle, device_path) == Status::SUCCESS {
            one_started = true;
          }
        }
        _ => continue,
      }
    }
    if started_drivers.len() == 0 {
      break;
    }
    driver_candidates.retain(|x| !started_drivers.contains(x));
  }

  if one_started {
    return Ok(());
  }

  if let Some(device_path) = remaining_device_path {
    if unsafe { (*device_path).r#type == device_path::TYPE_END } {
      return Ok(());
    }
  }

  Err(Status::NOT_FOUND)
}

/// Connects a controller to drivers
///
/// This function matches the behavior of EFI_BOOT_SERVICES.ConnectController() API in the UEFI spec 2.10 section
/// 7.3.12. Refer to the UEFI spec description for details on input parameters, behavior, and error return codes.
///
/// ## Safety:
/// This routine cannot hold the protocol db lock while executing DriverBinding->Supported()/Start() since
/// they need to access protocol db services. That means this routine can't guarantee that driver bindings remain
/// valid for the duration of its execution. For example, if a driver were be unloaded in a timer callback after
/// returning true from Supported() before Start() is called, then the driver binding instance would be uninstalled or
/// invalid and Start() would be an invalid function pointer when invoked. In general, the spec implicitly assumes
/// that driver binding instances that are valid at the start of he call to ConnectController() must remain valid for
/// the duration of the ConnectController() call. If this is not true, then behavior is undefined. This function is
/// marked unsafe for this reason.
///
/// ## Example
///
/// ```no_run
/// let result = core_connect_controller(controller_handle, Vec::new(), None, false);
/// ```
///
pub unsafe fn core_connect_controller(
  handle: Handle,
  driver_handles: Vec<Handle>,
  remaining_device_path: Option<*mut device_path::Protocol>,
  recursive: bool,
) -> Result<(), Status> {
  //TODO: security support: check whether the user has permissions to start UEFI device drivers.

  let return_status = core_connect_single_controller(handle, driver_handles, remaining_device_path);

  if recursive {
    for child in PROTOCOL_DB.get_child_handles(handle) {
      //ignore the return value to match behavior of edk2 reference.
      _ = core_connect_controller(child, Vec::new(), None, true);
    }
  }

  return_status
}

extern "efiapi" fn connect_controller(
  handle: Handle,
  driver_image_handle: *mut Handle,
  remaining_device_path: *mut device_path::Protocol,
  recursive: Boolean,
) -> Status {
  let driver_handles = if driver_image_handle.is_null() {
    Vec::new()
  } else {
    let mut count = 0;
    let mut current_ptr = driver_image_handle;
    loop {
      let current_val = unsafe { *current_ptr };
      if current_val.is_null() {
        break;
      }
      count += 1;
      current_ptr = unsafe { current_ptr.offset(size_of::<Handle>() as isize) };
    }
    let slice = unsafe { from_raw_parts_mut(driver_image_handle, count) };
    slice.to_vec().clone()
  };

  let device_path = NonNull::new(remaining_device_path).map(|x| x.as_ptr());
  unsafe {
    match core_connect_controller(handle, driver_handles, device_path, recursive.into()) {
      Err(err) => err,
      _ => Status::SUCCESS,
    }
  }
}

/// Disconnects drivers from a controller.
///
/// This function matches the behavior of EFI_BOOT_SERVICES.ConnectController() API in the UEFI spec 2.10 section
/// 7.3.13. Refer to the UEFI spec description for details on input parameters, behavior, and error return codes.
///
/// ## Safety:
/// This routine cannot hold the protocol db lock while executing DriverBinding->Supported()/Start() since
/// they need to access protocol db services. That means this routine can't guarantee that driver bindings remain
/// valid for the duration of its execution. For example, if a driver were be unloaded in a timer callback after
/// returning true from Supported() before Start() is called, then the driver binding instance would be uninstalled or
/// invalid and Start() would be an invalid function pointer when invoked. In general, the spec implicitly assumes
/// that driver binding instances that are valid at the start of he call to ConnectController() must remain valid for
/// the duration of the ConnectController() call. If this is not true, then behavior is undefined. This function is
/// marked unsafe for this reason.
///
/// ## Example
///
/// ```no_run
/// let result = core_disconnect_controller(controller_handle, None, None);
/// ```
///
pub unsafe fn core_disconnect_controller(
  controller_handle: Handle,
  driver_image_handle: Option<Handle>,
  child_handle: Option<Handle>,
) -> Result<(), Status> {
  PROTOCOL_DB.validate_handle(controller_handle)?;

  // determine which driver_handles should be stopped.
  let mut drivers_managing_controller = match driver_image_handle {
    Some(handle) => vec![handle], //use the specified driver_image_handle.
    None => {
      //driver image handle not specified, attempt to stop all drivers managing controller_handle.
      PROTOCOL_DB
        .get_open_protocol_information(controller_handle)?
        .iter()
        .flat_map(|(_guid, open_info)| {
          open_info.iter().filter_map(|x| {
            if (x.attributes & OPEN_PROTOCOL_BY_DRIVER) != 0 {
              Some(x.agent_handle.expect("BY_DRIVER usage must have an agent handle").clone())
            } else {
              None
            }
          })
        })
        .collect()
    }
  };
  drivers_managing_controller.sort_unstable();
  drivers_managing_controller.dedup();

  let mut stop_count = 0;
  for driver_handle in drivers_managing_controller {
    //determine which child handles should be stopped.
    let mut child_handles: Vec<_> = PROTOCOL_DB
      .get_open_protocol_information(controller_handle)?
      .iter()
      .flat_map(|(_guid, open_info)| {
        open_info.iter().filter_map(|x| {
          if (x.agent_handle == Some(driver_handle)) && ((x.attributes & OPEN_PROTOCOL_BY_CHILD_CONTROLLER) != 0) {
            Some(x.controller_handle.expect("controller handle required when open by child controller"))
          } else {
            None
          }
        })
      })
      .collect();
    child_handles.sort_unstable();
    child_handles.dedup();

    let total_children = child_handles.len();
    if let Some(handle) = child_handle {
      //if the child handle has been specified, only try and close that. This also checks that the specified child handle is legit.
      child_handles.retain(|x| x == &handle);
    }

    //resolve the handle to the driver_binding.
    let driver_binding_interface =
      PROTOCOL_DB.get_interface_for_handle(driver_handle, driver_binding::PROTOCOL_GUID)?;
    let driver_binding_interface = driver_binding_interface as *mut driver_binding::Protocol;
    let driver_binding = unsafe { &mut *(driver_binding_interface) };

    let mut status = Status::SUCCESS;
    if child_handles.len() > 0 {
      //disconnect the child controllers.
      status = (driver_binding.stop)(
        driver_binding_interface,
        controller_handle,
        child_handles.len(),
        child_handles.as_mut_ptr(),
      );
    }

    if (status == Status::SUCCESS) && (child_handles.len() == total_children) {
      status = (driver_binding.stop)(driver_binding_interface, controller_handle, 0, core::ptr::null_mut());
    }

    if status == Status::SUCCESS {
      stop_count += 1;
    }
  }

  if stop_count > 0 {
    Ok(())
  } else {
    Err(Status::NOT_FOUND)
  }
}

extern "efiapi" fn disconnect_controller(
  controller_handle: Handle,
  driver_image_handle: Handle,
  child_handle: Handle,
) -> Status {
  let driver_image_handle = NonNull::new(driver_image_handle).map(|x| x.as_ptr());
  let child_handle = NonNull::new(child_handle).map(|x| x.as_ptr());
  unsafe {
    match core_disconnect_controller(controller_handle, driver_image_handle, child_handle) {
      Err(err) => err,
      _ => Status::SUCCESS,
    }
  }
}

pub fn init_driver_services(bs: &mut BootServices) {
  bs.connect_controller = connect_controller;
  bs.disconnect_controller = disconnect_controller;
}
