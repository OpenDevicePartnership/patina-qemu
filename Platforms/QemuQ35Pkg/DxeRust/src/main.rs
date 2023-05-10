#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![feature(custom_test_frameworks)]

extern crate alloc;

use r_pi::{
    firmware_volume::{FfsSection, FfsSectionType, FirmwareVolume},
    hob::{self, Hob, HobList, MemoryAllocation, MemoryAllocationModule, PhaseHandoffInformationTable},
};

use core::{ffi::c_void, panic::PanicInfo, str::FromStr};
use dxe_rust::{
    allocator::{init_memory_support, ALL_ALLOCATORS},
    events::init_events_support,
    image::{core_load_image, get_dxe_core_handle, init_image_support, start_image},
    physical_memory, println,
    protocols::init_protocol_support,
    systemtables::EfiSystemTable,
    FRAME_ALLOCATOR,
};
use r_efi::efi::Guid;
use x86_64::{align_down, align_up, structures::paging::PageTableFlags};

#[cfg_attr(target_os = "uefi", export_name = "efi_main")]
pub extern "efiapi" fn _start(physical_hob_list: *const c_void) -> ! {
    // initialize IDT and GDT for exception handling
    dxe_rust::init();

    // Initialize memory subsystem.
    // Unsafe because it assumes DXE loader set up identity mapped paging for
    // the system beforehand and that the PHIT hob contents are correct

    // 1. Initialize global frame allocator with free memory region from PHIT hob.
    //    Note: this is _required_ before the global heap is used, since we need
    //    a source of frames to expand the global heap once it starts being used.
    let phit_hob: *const PhaseHandoffInformationTable = physical_hob_list as *const PhaseHandoffInformationTable;
    let free_start = unsafe { align_up((*phit_hob).free_memory_bottom, 0x1000) };
    let free_size = unsafe { align_down((*phit_hob).free_memory_top, 0x1000) - free_start };
    unsafe {
        FRAME_ALLOCATOR
            .lock()
            .add_physical_region(free_start, free_size)
            .expect("Failed to add initial region to global frame allocator.")
    };

    // 2. set up new page tables to replace those set up by the loader.
    //    initially map EfiMemoryBottom->EfiMemoryTop.
    let memory_start = unsafe { (*phit_hob).memory_bottom };
    let memory_end = unsafe { (*phit_hob).memory_top };
    unsafe {
        physical_memory::x86_64::x86_paging_support::PAGE_TABLE
            .lock()
            .init(memory_start..memory_end)
            .expect("Failed to initialize page table");
    }

    let mut hob_list = HobList::default();
    hob_list.discover_hobs(physical_hob_list);

    // 3. iterate over the hob list and map memory ranges from the pre-DXE memory allocation hobs.
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
            | Hob::FirmwareVolume2(hob::FirmwareVolume2 {
                header: _,
                base_address,
                length,
                fv_name: _,
                file_name: _,
            })
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

    // Instantiate system table. TODO: this instantiates it on the stack. It needs to be instantiated in runtime memory.
    let st = EfiSystemTable::init_system_table();

    init_memory_support(st.boot_services());
    init_events_support(st.boot_services());
    init_protocol_support(st.boot_services());
    init_image_support(&hob_list, &st);

    //
    // attempt to load and execute an external module's entry point.
    //
    let fv_hob = hob_list
        .iter()
        .find_map(|h| {
            if let Hob::FirmwareVolume(fv) = h {
                return Some(fv);
            }
            None
        })
        .expect("FV hob went missing");

    // locate the pe32 ffs section from our target file
    let target_guid_from_str = uuid::Uuid::from_str("AAB84920-C0C2-46F9-82DF-0C383381BC58").unwrap().to_bytes_le();
    let target_guid: Guid = unsafe { *(target_guid_from_str.as_ptr() as *const Guid) };

    let target_module_pe32: FfsSection = FirmwareVolume::new(fv_hob.base_address)
        .ffs_files()
        .find_map(|file| {
            if file.file_name() != target_guid {
                return None;
            }
            file.ffs_sections().find_map(|section| {
                if section.section_type() == Some(FfsSectionType::EfiSectionPe32) {
                    println!("Located target module {:?}", file);
                    return Some(section);
                }
                None
            })
        })
        .expect("Target module not found.");

    let image_handle =
        core_load_image(get_dxe_core_handle(), core::ptr::null_mut(), Some(target_module_pe32.section_data())).unwrap();
    let status = start_image(image_handle, core::ptr::null_mut(), core::ptr::null_mut());

    println!("Back from target module with status {:#x}", status.as_usize());

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
