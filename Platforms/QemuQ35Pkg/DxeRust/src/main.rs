#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![feature(custom_test_frameworks)]

extern crate alloc;

use r_pi::{
  dxe_services::GcdMemoryType,
  hob::{self, Hob, HobList, MemoryAllocation, MemoryAllocationModule, PhaseHandoffInformationTable},
};

use core::{ffi::c_void, panic::PanicInfo};
use dxe_rust::{
  allocator::{init_memory_support, ALL_ALLOCATORS},
  dispatcher::{core_dispatcher, init_dispatcher},
  driver_services::init_driver_services,
  dxe_services::init_dxe_services,
  events::init_events_support,
  fv::init_fv_support,
  image::init_image_support,
  misc_boot_services::init_misc_boot_services_support,
  physical_memory, println,
  protocols::init_protocol_support,
  systemtables::{init_system_table, SYSTEM_TABLE},
  GCD,
};
use r_efi::system::{MEMORY_RO, MEMORY_RP, MEMORY_UC, MEMORY_WB, MEMORY_WC, MEMORY_WP, MEMORY_WT, MEMORY_XP};
use x86_64::{align_down, align_up, structures::paging::PageTableFlags};

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
        GCD.init(cpu.size_of_memory_space as u32);
      }
      _ => (),
    }
  }

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
      .expect("Failed to add initial region to GCD.")
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

  // 4. iterate over the hob list and map memory ranges from the pre-DXE memory allocation hobs.
  // TODO: this maps the pages for these memory ranges; but we should also update the GCD accordingly.
  for hob in hob_list.iter() {
    let range = match hob {
      Hob::MemoryAllocation(MemoryAllocation { header: _, alloc_descriptor: desc })
      | Hob::MemoryAllocationModule(MemoryAllocationModule {
        header: _,
        alloc_descriptor: desc,
        module_name: _,
        entry_point: _,
      }) => {
        let base = desc.memory_base_address;
        let size = desc.memory_length;
        base..base.checked_add(size).expect("Invalid memory allocation hob")
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
      }) => *base_address..base_address.checked_add(*length).expect("Invalid FV hob"),
      _ => continue,
    };
    unsafe {
      physical_memory::x86_64::x86_paging_support::PAGE_TABLE
        .lock()
        .map_range(range, PageTableFlags::PRESENT)
        .expect("Failed to map memory resource");
    }
  }

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
  }

  core_dispatcher();

  for allocator in ALL_ALLOCATORS {
    println!("{}", allocator);
  }

  println!("It did not crash!");

  // Call exit_qemu, which will shutdown qemu if sa-debug-exit,iobase=0xf4,iosize=0x04 is set
  // Else it will hit hlt_loop and wait.
  dxe_rust::exit_qemu(dxe_rust::QemuExitCode::Success);
  dxe_rust::hlt_loop();
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
