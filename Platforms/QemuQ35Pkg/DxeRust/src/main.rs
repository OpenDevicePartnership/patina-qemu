#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![feature(custom_test_frameworks)]

extern crate alloc;

use r_pi::{
  dxe_services::{GcdIoType, GcdMemoryType, MemorySpaceDescriptor},
  hob::{self, Hob, HobList, MemoryAllocation, MemoryAllocationModule, PhaseHandoffInformationTable},
};
use uefi_protocol_db_lib::{
  DXE_CORE_HANDLE, EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE, EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE,
  EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE, EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE, EFI_LOADER_CODE_ALLOCATOR_HANDLE,
  EFI_LOADER_DATA_ALLOCATOR_HANDLE, EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE,
  EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE, RESERVED_MEMORY_ALLOCATOR_HANDLE,
};
use uuid::uuid;

use core::{ffi::c_void, ops::Range, panic::PanicInfo, str::FromStr};
use dxe_rust::{
  allocator::init_memory_support,
  dispatcher::{core_dispatcher, display_discovered_not_dispatched, init_dispatcher},
  driver_services::init_driver_services,
  dxe_services::{get_memory_space_descriptor, init_dxe_services},
  events::init_events_support,
  fv::init_fv_support,
  image::init_image_support,
  misc_boot_services::init_misc_boot_services_support,
  physical_memory,
  protocols::{init_protocol_support, PROTOCOL_DB},
  systemtables::{init_system_table, SYSTEM_TABLE},
  GCD,
};
use r_efi::{
  efi,
  system::{
    ACPI_MEMORY_NVS, ACPI_RECLAIM_MEMORY, BOOT_SERVICES_CODE, BOOT_SERVICES_DATA, LOADER_CODE, LOADER_DATA, MEMORY_RO,
    MEMORY_RP, MEMORY_UC, MEMORY_WB, MEMORY_WC, MEMORY_WP, MEMORY_WT, MEMORY_XP, RESERVED_MEMORY_TYPE,
    RUNTIME_SERVICES_CODE, RUNTIME_SERVICES_DATA,
  },
};
use uefi_gcd_lib::gcd;
use x86_64::{
  align_down, align_up,
  structures::paging::{page_table, PageTableFlags},
};

use serial_print_dxe::println;

#[cfg_attr(target_os = "uefi", export_name = "efi_main")]
pub extern "efiapi" fn _start(physical_hob_list: *const c_void) -> ! {
  // initialize IDT and GDT for exception handling
  dxe_rust::init();

  // Initialize memory subsystem.
  // Unsafe because it assumes DXE loader set up identity mapped paging for
  // the system beforehand and that the PHIT hob contents are correct

  // 1. Initialize GCD with free memory region from PHIT hob and CPU info hob.
  //    Note: this is _required_ before the global heap is used, since we need a source of memory to expand the global
  //    heap once it starts being used.

  let mut free_memory_start: u64 = 0;
  let mut free_memory_size: u64 = 0;
  let mut memory_start: u64 = 0;
  let mut memory_end: u64 = 0;

  let hob_list =
    Hob::Handoff(unsafe { (physical_hob_list as *const PhaseHandoffInformationTable).as_ref::<'static>().unwrap() });
  for hob in &hob_list {
    match hob {
      Hob::Handoff(handoff) => {
        free_memory_start = align_up(handoff.free_memory_bottom, 0x1000);
        free_memory_size = align_down(handoff.free_memory_top, 0x1000) - free_memory_start;
        memory_start = handoff.memory_bottom;
        memory_end = handoff.memory_top;
      }
      Hob::Cpu(cpu) => {
        GCD.init(cpu.size_of_memory_space as u32, cpu.size_of_io_space as u32);
      }
      _ => (),
    }
  }

  println!("memory_start: {:x?}", memory_start);
  println!("memory_size: {:x?}", memory_end - memory_start);
  println!("free_memory_start: {:x?}", free_memory_start);
  println!("free_memory_size: {:x?}", free_memory_size);

  // make sure the PHIT is present and it was reasonable.
  assert!(free_memory_size > 0, "Not enough free memory for DXE core to start");
  assert!(memory_start < memory_end, "Not enough memory available for DXE core to start.");

  // initialize the GCD with an initial memory space. Note: this will fail if GCD.init() above didn't happen.
  unsafe {
    GCD
      .add_memory_space(
        GcdMemoryType::SystemMemory,
        free_memory_start as usize,
        free_memory_size as usize,
        MEMORY_UC | MEMORY_WC | MEMORY_WT | MEMORY_WB | MEMORY_WP | MEMORY_RP | MEMORY_XP | MEMORY_RO,
      )
      .expect("Failed to add initial region to GCD.");

    // Mark the first page of memory as non-existent
    GCD
      .add_memory_space(GcdMemoryType::Reserved, 0, 0x1000, 0)
      .expect("Failed to mark the first page as non-existent in the GCD.");
  };

  // 2. set up new page tables to replace those set up by the loader.
  //    initially map EfiMemoryBottom->EfiMemoryTop.
  unsafe {
    physical_memory::x86_64::x86_paging_support::PAGE_TABLE
      .lock()
      .init(memory_start..memory_end)
      .expect("Failed to initialize page table");
  }

  // 3. At this point Rust Heap usage is permitted (since GCD is initialized and memory is mapped).
  // That means that HobList::discover can be used to relocate the hobs from the input list into a Vec.
  let mut hob_list = HobList::default();
  hob_list.discover_hobs(physical_hob_list);

  // 4. iterate over the hob list and map resource descriptor hobs in the gcd and page table
  for hob in hob_list.iter() {
    let mut gcd_mem_type: GcdMemoryType = GcdMemoryType::NonExistent;
    let mut mem_range: Range<u64> = 0..0;
    let mut resource_attributes: u32 = 0;
    let mut page_table_flags: page_table::PageTableFlags = PageTableFlags::PRESENT;

    if let Hob::ResourceDescriptor(res_desc) = hob {
      mem_range = res_desc.physical_start
        ..res_desc.physical_start.checked_add(res_desc.resource_length).expect("Invalid resource descriptor hob");

      match res_desc.resource_type {
        hob::EFI_RESOURCE_SYSTEM_MEMORY => {
          resource_attributes = res_desc.resource_attribute;

          if resource_attributes & hob::MEMORY_ATTRIBUTE_MASK == hob::TESTED_MEMORY_ATTRIBUTES {
            if resource_attributes & hob::EFI_RESOURCE_ATTRIBUTE_MORE_RELIABLE
              == hob::EFI_RESOURCE_ATTRIBUTE_MORE_RELIABLE
            {
              gcd_mem_type = GcdMemoryType::MoreReliable;
            } else {
              gcd_mem_type = GcdMemoryType::SystemMemory;
            }
            page_table_flags |= PageTableFlags::WRITABLE;
          }

          if (resource_attributes & hob::MEMORY_ATTRIBUTE_MASK == (hob::INITIALIZED_MEMORY_ATTRIBUTES))
            || (resource_attributes & hob::MEMORY_ATTRIBUTE_MASK == (hob::PRESENT_MEMORY_ATTRIBUTES))
          {
            gcd_mem_type = GcdMemoryType::Reserved;
            page_table_flags |= PageTableFlags::WRITABLE;
          }

          if resource_attributes & hob::EFI_RESOURCE_ATTRIBUTE_PERSISTENT == hob::EFI_RESOURCE_ATTRIBUTE_PERSISTENT {
            gcd_mem_type = GcdMemoryType::Persistent;
          }

          if res_desc.physical_start < 0x1000 {
            let adjusted_base: u64 = 0x1000;
            mem_range = adjusted_base
              ..adjusted_base
                .checked_add(res_desc.resource_length - adjusted_base)
                .expect("Invalid resource descriptor hob length");
          }
        }
        hob::EFI_RESOURCE_MEMORY_MAPPED_IO | hob::EFI_RESOURCE_FIRMWARE_DEVICE => {
          resource_attributes = res_desc.resource_attribute;
          gcd_mem_type = GcdMemoryType::MemoryMappedIo;
          page_table_flags |= PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        }
        hob::EFI_RESOURCE_MEMORY_MAPPED_IO_PORT | hob::EFI_RESOURCE_MEMORY_RESERVED => {
          gcd_mem_type = GcdMemoryType::Reserved;
          page_table_flags |= PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        }
        hob::EFI_RESOURCE_IO => {
          println!("Mapping io range {:x?} as {:?}", res_desc.physical_start..res_desc.resource_length, GcdIoType::Io);
          GCD
            .add_io_space(GcdIoType::Io, res_desc.physical_start as usize, res_desc.resource_length as usize)
            .expect("Failed to add IO space to GCD");
        }
        hob::EFI_RESOURCE_IO_RESERVED => {
          println!(
            "Mapping io range {:x?} as {:?}",
            res_desc.physical_start..res_desc.resource_length,
            GcdIoType::Reserved
          );
          GCD
            .add_io_space(GcdIoType::Reserved, res_desc.physical_start as usize, res_desc.resource_length as usize)
            .expect("Failed to add IO space to GCD");
        }
        _ => {
          debug_assert!(false, "Unknown resource type in HOB");
        }
      };

      if gcd_mem_type != GcdMemoryType::NonExistent {
        assert!(res_desc.attributes_valid());
      }
    }

    if gcd_mem_type != GcdMemoryType::NonExistent {
      for split_range in remove_range_overlap(&mem_range, &(free_memory_start..(free_memory_start + free_memory_size)))
        .into_iter()
        .take_while(|r| r.is_some())
      {
        if let Some(actual_range) = split_range {
          println!(
            "Mapping memory range {:x?} as {:?} with attributes {:x?}",
            actual_range, gcd_mem_type, resource_attributes
          );
          unsafe {
            GCD
              .add_memory_space(
                gcd_mem_type,
                actual_range.start as usize,
                actual_range.end.saturating_sub(actual_range.start) as usize,
                gcd::get_capabilities(gcd_mem_type, resource_attributes as u64),
              )
              .expect("Failed to add memory space to GCD");
            physical_memory::x86_64::x86_paging_support::PAGE_TABLE
              .lock()
              .map_range(actual_range, page_table_flags)
              .expect("Failed to map memory resource");
          }
        }
      }
    }
  }

  // 5. iterate over the hob list and memory allocation and fv hobs to the hob list
  for hob in hob_list.iter() {
    match hob {
      Hob::MemoryAllocation(MemoryAllocation { header: _, alloc_descriptor: desc })
      | Hob::MemoryAllocationModule(MemoryAllocationModule {
        header: _,
        alloc_descriptor: desc,
        module_name: _,
        entry_point: _,
      }) => {
        let mut descriptor: MemorySpaceDescriptor = MemorySpaceDescriptor::default();

        if get_memory_space_descriptor(desc.memory_base_address, &mut descriptor as *mut MemorySpaceDescriptor)
          == efi::Status::SUCCESS
        {
          let allocator_handle = match desc.memory_type {
            RESERVED_MEMORY_TYPE => RESERVED_MEMORY_ALLOCATOR_HANDLE,
            LOADER_CODE => EFI_LOADER_CODE_ALLOCATOR_HANDLE,
            LOADER_DATA => EFI_LOADER_DATA_ALLOCATOR_HANDLE,
            BOOT_SERVICES_CODE => EFI_BOOT_SERVICES_CODE_ALLOCATOR_HANDLE,
            BOOT_SERVICES_DATA => EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE,
            RUNTIME_SERVICES_CODE => EFI_RUNTIME_SERVICES_CODE_ALLOCATOR_HANDLE,
            RUNTIME_SERVICES_DATA => EFI_RUNTIME_SERVICES_DATA_ALLOCATOR_HANDLE,
            ACPI_RECLAIM_MEMORY => EFI_ACPI_RECLAIM_MEMORY_ALLOCATOR_HANDLE,
            ACPI_MEMORY_NVS => EFI_ACPI_MEMORY_NVS_ALLOCATOR_HANDLE,
            _ => DXE_CORE_HANDLE,
          };
          let result = GCD.allocate_memory_space(
            gcd::AllocateType::Address(desc.memory_base_address as usize),
            descriptor.memory_type,
            0,
            desc.memory_length as usize,
            allocator_handle,
            None,
          );
          if let Err(_) = result {
            println!(
              "Failed to allocate memory space for memory allocation HOB at {:x?} of length {:x?}.",
              desc.memory_base_address, desc.memory_length
            );
          }
        }
      }
      Hob::FirmwareVolume(hob::FirmwareVolume { header: _, base_address, length })
      | Hob::FirmwareVolume2(hob::FirmwareVolume2 { header: _, base_address, length, fv_name: _, file_name: _ })
      | Hob::FirmwareVolume3(hob::FirmwareVolume3 {
        header: _,
        base_address,
        length,
        authentication_status: _,
        extracted_fv: _,
        fv_name: _,
        file_name: _,
      }) => {
        let result = GCD.allocate_memory_space(
          gcd::AllocateType::Address(*base_address as usize),
          GcdMemoryType::MemoryMappedIo,
          0,
          *length as usize,
          EFI_BOOT_SERVICES_DATA_ALLOCATOR_HANDLE,
          None,
        );
        if let Err(_) = result {
          println!("Memory space is not yet available for the FV at {:x?} of length {:x?}.", base_address, length);
        }
      }
      _ => continue,
    };
  }

  println!("GCD after HOB iteration is:");
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
    init_image_support(&hob_list, &st);
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

  core_dispatcher();

  core_display_missing_arch_protocols();

  display_discovered_not_dispatched();

  call_bds();

  // Call exit_qemu, which will shutdown qemu if sa-debug-exit,iobase=0xf4,iosize=0x04 is set
  // Else it will hit hlt_loop and wait.
  dxe_rust::exit_qemu(dxe_rust::QemuExitCode::Success);
  dxe_rust::hlt_loop();
}

fn remove_range_overlap<T: PartialOrd + Copy>(a: &Range<T>, b: &Range<T>) -> [Option<Range<T>>; 2] {
  if a.start < b.end && a.end > b.start {
    // Check if `a` has a portion before the overlap
    let first_range = if a.start < b.start { Some(a.start..b.start) } else { None };

    // Check if `a` has a portion after the overlap
    let second_range = if a.end > b.end { Some(b.end..a.end) } else { None };

    [first_range, second_range]
  } else {
    // No overlap
    [Some(a.start..a.end), None]
  }
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

type BdsEntry = extern "efiapi" fn(*mut BdsProtocol);
#[repr(C)]
struct BdsProtocol {
  entry: BdsEntry,
}
fn call_bds() {
  let bds_guid: efi::Guid =
    unsafe { core::mem::transmute(uuid!("665e3ff6-46cc-11d4-9a38-0090273fc14d").to_bytes_le()) };
  if let Ok(protocol) = PROTOCOL_DB.locate_protocol(bds_guid) {
    let bds = protocol as *mut BdsProtocol;
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

#[test_case]
fn trivial_assertion() {
  assert_eq!(1, 1);
}
