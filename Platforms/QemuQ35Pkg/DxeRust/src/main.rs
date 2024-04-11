#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![feature(custom_test_frameworks)]

extern crate alloc;

use mu_pi::{hob::HobList, protocols::bds};
use uuid::uuid;

use core::{ffi::c_void, panic::PanicInfo, str::FromStr};
use dxe_rust::{
  allocator::init_memory_support,
  dispatcher::{core_dispatcher, display_discovered_not_dispatched, init_dispatcher},
  driver_services::init_driver_services,
  dxe_services::init_dxe_services,
  events::init_events_support,
  fv::init_fv_support,
  gcd::{add_hob_allocations_to_gcd, add_hob_resource_descriptors_to_gcd, init_gcd},
  image::init_image_support,
  misc_boot_services::init_misc_boot_services_support,
  protocols::{init_protocol_support, PROTOCOL_DB},
  runtime::init_runtime_support,
  systemtables::{init_system_table, SYSTEM_TABLE},
  GCD,
};
use r_efi::efi;

use serial_print_dxe::println;

#[cfg_attr(target_os = "uefi", export_name = "efi_main")]
pub extern "efiapi" fn _start(physical_hob_list: *const c_void) -> ! {
  // initialize IDT and GDT for exception handling
  dxe_rust::init();

  // initialize GCD
  let (free_memory_start, free_memory_size) = init_gcd(physical_hob_list);

  // After this point Rust Heap usage is permitted (since GCD is initialized).
  // Relocate the hobs from the input list pointer into a Vec.
  let mut hob_list = HobList::default();
  hob_list.discover_hobs(physical_hob_list);

  add_hob_resource_descriptors_to_gcd(&hob_list, free_memory_start, free_memory_size);

  add_hob_allocations_to_gcd(&hob_list);

  println!("GCD after initialization is:");
  println!("{:#x?}", GCD);

  // Instantiate system table.
  init_system_table();

  // use a block to limit the lifetime of the lock guard on the SYSTEM_TABLE reference.
  {
    let mut st = SYSTEM_TABLE.lock();
    let st = st.as_mut().expect("System Table not initialized!");

    init_memory_support(st.boot_services());
    init_events_support(st.boot_services());
    init_protocol_support(st.boot_services());
    init_misc_boot_services_support(st.boot_services());
    init_runtime_support(st.runtime_services());
    init_image_support(&hob_list, st);
    init_dispatcher();
    init_fv_support(&hob_list);
    init_dxe_services(st);
    init_driver_services(st.boot_services());
    // re-checksum the system tables after above initialization.
    st.checksum_all();

    // Install HobList configuration table
    let hob_list_guid = uuid::Uuid::from_str("7739F24C-93D7-11D4-9A3A-0090273FC14D").unwrap();
    let hob_list_guid: efi::Guid = unsafe { *(hob_list_guid.to_bytes_le().as_ptr() as *const efi::Guid) };
    dxe_rust::misc_boot_services::core_install_configuration_table(
      hob_list_guid,
      unsafe { (physical_hob_list as *mut c_void).as_mut() },
      st,
    )
    .unwrap();
  }

  let mut st = SYSTEM_TABLE.lock();
  let bs = st.as_mut().unwrap().boot_services() as *mut efi::BootServices;
  drop(st);
  tpl_lock::init_boot_services(bs);

  core_dispatcher().expect("initial dispatch failed.");

  core_display_missing_arch_protocols();

  display_discovered_not_dispatched();

  call_bds();

  // Call exit_qemu, which will shutdown qemu if sa-debug-exit,iobase=0xf4,iosize=0x04 is set
  // Else it will hit hlt_loop and wait.
  dxe_rust::exit_qemu(dxe_rust::QemuExitCode::Success);
  dxe_rust::hlt_loop();
}

const ARCH_PROTOCOLS: &[(uuid::Uuid, &str)] = &[
  (uuid!("a46423e3-4617-49f1-b9ff-d1bfa9115839"), "Security"),
  (uuid!("26baccb1-6f42-11d4-bce7-0080c73c8881"), "Cpu"),
  (uuid!("26baccb2-6f42-11d4-bce7-0080c73c8881"), "Metronome"),
  (uuid!("26baccb3-6f42-11d4-bce7-0080c73c8881"), "Timer"),
  (uuid!("665e3ff6-46cc-11d4-9a38-0090273fc14d"), "Bds"),
  (uuid!("665e3ff5-46cc-11d4-9a38-0090273fc14d"), "Watchdog"),
  (uuid!("b7dfb4e1-052f-449f-87be-9818fc91b733"), "Runtime"),
  (uuid!("1e5668e2-8481-11d4-bcf1-0080c73c8881"), "Variable"),
  (uuid!("6441f818-6362-4e44-b570-7dba31dd2453"), "Variable Write"),
  (uuid!("5053697e-2cbc-4819-90d9-0580deee5754"), "Capsule"),
  (uuid!("1da97072-bddc-4b30-99f1-72a0b56fff2a"), "Monotonic Counter"),
  (uuid!("27cfac88-46cc-11d4-9a38-0090273fc14d"), "Reset"),
  (uuid!("27cfac87-46cc-11d4-9a38-0090273fc14d"), "Real Time Clock"),
];

fn core_display_missing_arch_protocols() {
  for (uuid, name) in ARCH_PROTOCOLS {
    let guid: efi::Guid = unsafe { core::mem::transmute(uuid.to_bytes_le()) };
    if PROTOCOL_DB.locate_protocol(guid).is_err() {
      println!("Missing architectural protocol: {:?}, {:?}", uuid, name);
    }
  }
}

fn call_bds() {
  if let Ok(protocol) = PROTOCOL_DB.locate_protocol(bds::PROTOCOL_GUID) {
    let bds = protocol as *mut bds::Protocol;
    unsafe {
      ((*bds).entry)(bds);
    }
  }
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
  println!("{}", info);
  dxe_rust::exit_qemu(dxe_rust::QemuExitCode::Failed);
  dxe_rust::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
  dxe_rust::test_panic_handler(info)
}
