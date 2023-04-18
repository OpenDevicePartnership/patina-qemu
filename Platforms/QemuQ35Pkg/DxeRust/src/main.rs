#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]

extern crate alloc;

use r_pi::hob::{self, Hob, HobList, MemoryAllocation, MemoryAllocationModule, PhaseHandoffInformationTable};

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use core::{
    ffi::c_void,
    mem::transmute,
    panic::PanicInfo,
    ptr::slice_from_raw_parts,
    str::{from_utf8, FromStr},
};
use dxe_rust::{
    allocator::{init_memory_support, ALL_ALLOCATORS},
    events::init_events_support,
    pe32, physical_memory, println,
    protocols::init_protocol_support,
    systemtables::EfiSystemTable,
    FRAME_ALLOCATOR,
};
use fv_lib::{FfsFileType, FfsSection, FfsSectionType, FirmwareVolume};
use goblin::pe;
use r_efi::efi::Guid;
use x86_64::{align_down, align_up, structures::paging::PageTableFlags};

#[cfg_attr(target_os = "uefi", export_name = "efi_main")]
pub extern "efiapi" fn _start(hob_list: *const c_void) -> ! {
    // initialize IDT and GDT for exception handling
    dxe_rust::init();

    // Initialize memory subsystem.
    // Unsafe because it assumes DXE loader set up identity mapped paging for
    // the system beforehand and that the PHIT hob contents are correct

    // 1. Initialize global frame allocator with free memory region from PHIT hob.
    //    Note: this is _required_ before the global heap is used, since we need
    //    a source of frames to expand the global heap once it starts being used.
    let phit_hob: *const PhaseHandoffInformationTable = hob_list as *const PhaseHandoffInformationTable;
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

    let mut the_hob_list = HobList::default();
    the_hob_list.discover_hobs(hob_list);

    // 3. iterate over the hob list and map memory ranges from the pre-DXE memory allocation hobs.
    for hob in the_hob_list.iter() {
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

    // heap tests

    // allocate a number on the heap
    let heap_value = Box::new(41);
    println!("heap_value at {:p}", heap_value);

    // create a dynamically sized vector
    let mut vec = Vec::new();
    for i in 0..500 {
        vec.push(i);
    }
    println!("vec at {:p}", vec.as_slice());

    // create a reference counted vector -> will be freed when count reaches 0
    let reference_counted = Rc::new(vec![1, 2, 3]);
    let cloned_reference = reference_counted.clone();
    println!("current reference count is {}", Rc::strong_count(&cloned_reference));
    core::mem::drop(reference_counted);
    println!("reference count is {} now", Rc::strong_count(&cloned_reference));

    // end heap tests

    // retrieve list of fv hobs and print them
    let firmware_volume_hobs = the_hob_list
        .iter()
        .filter(|h| matches!(h, Hob::FirmwareVolume(_) | Hob::FirmwareVolume2(_) | Hob::FirmwareVolume3(_)));
    for hob in firmware_volume_hobs {
        println!("hob: {:?}", hob);
        //for FirmwareVolume() type hobs, print the filesystem
        match hob {
            Hob::FirmwareVolume(fv) => {
                let fv = FirmwareVolume::new(fv.base_address);
                println!("fv: {:?}", fv);
                for file in fv.ffs_files() {
                    println!("    ffs: {:?}", file);
                    for section in file.ffs_sections() {
                        println!("        section: {:?}", section);
                        let mut data = section.section_data();
                        if data.len() > 0x10 {
                            data = &data[..0x10];
                        }
                        println!("           data: {:02x?}", data);
                        if section.section_type() == Some(FfsSectionType::EfiSectionPe32) {
                            match pe::PE::parse(section.section_data()) {
                                Ok(pe_image) => {
                                    println!("        pe RVA: {:x}", pe_image.image_base);
                                    if let Some(debug_data) = pe_image.debug_data {
                                        if let Some(codeview_data) = debug_data.codeview_pdb70_debug_info {
                                            let filename = from_utf8(codeview_data.filename)
                                                .expect("failed to parse codeview filename");
                                            println!("    pe pdb path: {:}", filename.split("\\").last().unwrap());
                                        }
                                    }
                                }
                                Err(msg) => println!(" pe parse error: {:?}", msg),
                            }
                        }
                    }
                }
            }
            Hob::FirmwareVolume2(_) => todo!(),
            _ => (),
        }
    }

    //pe32::parse_first(&parsed_hobs).expect("parsing pe32 failed");

    // Instantiate system table. Note: this eventually should be a global static of some kind.
    let st = EfiSystemTable::init_system_table();

    init_memory_support(st.boot_services());
    init_events_support(st.boot_services());
    init_protocol_support(st.boot_services());

    //
    // PE32 load and relocate testing
    //
    let fv_hob = the_hob_list
        .iter()
        .find_map(|h| {
            if let Hob::FirmwareVolume(fv) = h {
                return Some(fv);
            }
            None
        })
        .expect("FV hob went missing");

    let pe32_section: FfsSection = FirmwareVolume::new(fv_hob.base_address)
        .ffs_files()
        .find_map(|file| {
            if file.file_type() != Some(FfsFileType::EfiFvFileTypeDxeCore) {
                return None;
            }
            file.ffs_sections().find_map(|section| {
                if section.section_type() == Some(FfsSectionType::EfiSectionPe32) {
                    return Some(section);
                }
                None
            })
        })
        .expect("No PE32 sections in FV.");

    let pe = goblin::pe::PE::parse(pe32_section.section_data()).unwrap();
    let size_of_image = pe.header.optional_header.unwrap().windows_fields.size_of_image;

    // for now, allocate the image on the heap (todo: get pages from frame allocator, map them, and then use that memory)
    let mut loaded_image: Vec<u8> = vec![0; size_of_image as usize];
    pe32::pe32_load_image(pe32_section.section_data(), &mut loaded_image).expect("failed to load pe32 image");
    //to apply relocation to the image at its current address, use the following:
    //let image_addr = loaded_image.as_mut() as *mut [u8] as *mut u8 as usize;

    // for test, find the location of the dxe_rust module (i.e. the current module)
    // and match our reloc vs. what was done by PEI.
    // determine the location of this module from the hoblist
    let dxe_core_module_hob = the_hob_list
        .iter()
        .find_map(|h| {
            if let Hob::MemoryAllocationModule(module) = h {
                //todo: we could validate the GUID here to make sure it is dxe core, but I think this is the
                //only memory allocation module hob.
                return Some(module);
            }
            None
        })
        .expect("Couldn't find MemoryAllocationModule HOB for DXE core");

    let image_addr = dxe_core_module_hob.alloc_descriptor.memory_base_address as usize;

    pe32::pe32_relocate_image(image_addr, &mut loaded_image).expect("failed to relocate pe32 image");

    let this_dxe_rust_core_buffer =
        slice_from_raw_parts(image_addr as *const u8, dxe_core_module_hob.alloc_descriptor.memory_length as usize);

    // compare our relocated image to the image we are currently running (relocated by DxeIpl) and see what we get.
    // ignore the .data section (as that will have been changed as a result of execution).
    let data_section = pe
        .sections
        .iter()
        .find_map(|section| {
            if let Result::Ok(name) = section.name() {
                if name == ".data" {
                    return Some(
                        section.virtual_address as usize..(section.virtual_address + section.virtual_size) as usize,
                    );
                }
            }
            None
        })
        .expect("couldn't find data section");

    let mut differences = 0;
    for index in 0..loaded_image.len() {
        if data_section.contains(&index) {
            continue;
        }
        let reference_byte = unsafe { &*this_dxe_rust_core_buffer }[index];
        if loaded_image[index] != reference_byte {
            println!(
                "Relocation differs from reference at byte {:#x}. Expected: {:#x}, got :{:#x}",
                index, reference_byte, loaded_image[index]
            );
            differences += 1;
        }
    }

    println!("Test relocation of DxeRust core complete. Saw {:?} differences.", differences);
    //
    // PE32 load and relocate testing done.
    //

    //
    // attempt to load and execute an external module's entry point.
    //
    // locate the pe32 ffs section from our target file
    //let target_guid_from_str = uuid::Uuid::from_str("35AFEBCD-8485-4865-A9EC-447FF8EA47A9").unwrap().to_bytes_le();
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

    let target_pe = goblin::pe::PE::parse(target_module_pe32.section_data()).unwrap();
    let size_of_target = target_pe.header.optional_header.unwrap().windows_fields.size_of_image;
    //allocate space for the image on the heap
    let mut loaded_target: Vec<u8> = vec![0; size_of_target as usize];
    pe32::pe32_load_image(target_module_pe32.section_data(), &mut loaded_target).expect("failed to load target");
    //apply relocation to the target image at its current address
    let target_image_addr = loaded_target.as_mut() as *mut [u8] as *mut u8 as usize;
    pe32::pe32_relocate_image(target_image_addr, &mut loaded_target).expect("failed to relocate target");

    //relocated image is ready to execute, compute entry point address
    let target_entry_point_addr = target_image_addr + target_pe.entry;

    println!("Invoking target module.");
    let ptr = target_entry_point_addr as *const ();

    //TODO: This definition treats the system table as const; however, entry points can modify it.
    //Even more challenging is that they can hang on to the system table pointer and modify it later (e.g. in a protocol notify).
    //So this should really be a mut - but to make it mut and enforce semantics around modifying it (e.g. only allowing modification
    //in the entry point function, or requiring a call back into rust to modify instead of writing directly) would break
    //current semantics. This needs further thought/review.
    let entry_point: unsafe extern "efiapi" fn(*const c_void, *const r_efi::system::SystemTable) -> u64 =
        unsafe { transmute(ptr) };

    let status = unsafe { entry_point(ptr as *const c_void, st.as_ref()) };

    println!("Back from target module with status {:#x}", status);

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
