#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(abi_efiapi)]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use r_efi::efi::Guid;
use core::ffi::c_void;
use core::mem::transmute;
use core::panic::PanicInfo;
use core::ptr::slice_from_raw_parts;
use core::str::{from_utf8, FromStr};
use dxe_rust::fv::{FirmwareVolume, FfsSectionType, FfsSection, FfsFileType};
use dxe_rust::hob::{Hob, HobList, PhaseHandoffInformationTable};
use dxe_rust::memory::{self, DynamicFrameAllocator};
use dxe_rust::memory_region::{MemoryRegion, MemoryRegionKind};
use dxe_rust::uefi_allocator::allocate_runtime_pages;
use dxe_rust::uefi_allocator::allocate_zero_pool;
use dxe_rust::uefi_allocator::free_pages;
use dxe_rust::println;
use goblin::pe;
use dxe_rust::pe32;
use x86_64::{align_down, align_up};

pub const PHYS_MEMORY_OFFSET: u64 = 0x00; // Handoff happens with memory identity mapped.

#[cfg_attr(target_os = "uefi", export_name = "efi_main")]
pub extern "efiapi" fn _start(hob_list: *const c_void) -> ! {
    use dxe_rust::allocator;
    use dxe_rust::uefi_allocator;
    use x86_64::VirtAddr;

    println!("HOB list is here - {:?}", hob_list);
    let phit_hob: *const PhaseHandoffInformationTable = hob_list as *const PhaseHandoffInformationTable;

    // initialize IDT and GDT for exception handling
    dxe_rust::init();

    // initialize memory subsystem
    let phys_mem_offset = VirtAddr::new(PHYS_MEMORY_OFFSET);
    let mut mapper = unsafe { memory::init_offset(phys_mem_offset) };

    let mut frame_allocator = DynamicFrameAllocator::init();

    let phit_region = MemoryRegion {
        start: unsafe { align_up((*phit_hob).free_memory_bottom, 0x1000) },
        end: unsafe { align_down((*phit_hob).free_memory_top, 0x1000) },
        kind: MemoryRegionKind::Usable,
    };

    unsafe { frame_allocator.add_region(phit_region) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");
    uefi_allocator::init_heap(&mut mapper, &mut frame_allocator).expect("UEFI heap initialization failed");

    // Test code
    let x1: usize = allocate_runtime_pages(5);
    println!("allocate_runtime_pages returned {:x}", x1);

    println!("before allocate_zero_pool");
    let x2: usize = allocate_zero_pool(999);
    println!("allocate_zero_pool returned {:x}", x2);

//    let x2: usize = allocate_runtime_pages(2);
//    println!("allocate_runtime_pages returned {:x}", x2);

    let x3: usize = allocate_runtime_pages(7);
    println!("allocate_runtime_pages returned {:x}", x3);

    free_pages(x1, 5);
    free_pages(x3, 7);

    println!("Finished freeing pages");

    let converted_mem_type = dxe_rust::memory_types::EfiMemoryType::from(4u16);
    println!("Converted memory type of {} is {:?}", 4, converted_mem_type);
    println!("Initialization complete.");

    // HobList will cache the HOB list in the heap
    // Discover additional HOBs now that the heap is initialized
    let mut the_hob_list = HobList::default();
    the_hob_list.discover_hobs(hob_list);

    println!("HOB List: {:?}", the_hob_list);

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
    println!(
        "current reference count is {}",
        Rc::strong_count(&cloned_reference)
    );
    core::mem::drop(reference_counted);
    println!(
        "reference count is {} now",
        Rc::strong_count(&cloned_reference)
    );

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
                                            let filename = from_utf8(codeview_data.filename).expect("failed to parse codeview filename");
                                            println!("    pe pdb path: {:}", filename.split("\\").last().unwrap());
                                        }
                                    }
                                }
                                Err(msg) => println!(" pe parse error: {:?}", msg)
                            }
                        }
                    }
                }
            },
            Hob::FirmwareVolume2(_) => todo!(),
            _ => (),
        }
    }

    //pe32::parse_first(&parsed_hobs).expect("parsing pe32 failed");

    //
    // PE32 load and relocate testing
    //
    let fv_hob = the_hob_list.iter().find_map(|h| {
        if let Hob::FirmwareVolume(fv) = h {
            return Some(fv);
        }
        None
    }).expect("FV hob went missing");

    let pe32_section: FfsSection = FirmwareVolume::new(fv_hob.base_address)
        .ffs_files()
        .find_map(|file|{
            if file.file_type() != Some(FfsFileType::EfiFvFileTypeDxeCore) {
                return None;
            }
            file.ffs_sections().find_map(|section|{
                if section.section_type() == Some(FfsSectionType::EfiSectionPe32) {
                    return Some(section)
                }
                None
            })
        }).expect("No PE32 sections in FV.");

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
    let dxe_core_module_hob = the_hob_list.iter().find_map(|h| {
        if let Hob::MemoryAllocationModule(module) = h {
            //todo: we could validate the GUID here to make sure it is dxe core, but I think this is the
            //only memory allocation module hob.
            return Some(module);
        }
        None
    }).expect("Couldn't find MemoryAllocationModule HOB for DXE core");

    let image_addr = dxe_core_module_hob.alloc_descriptor.memory_base_address as usize;

    pe32::pe32_relocate_image(image_addr, &mut loaded_image).expect("failed to relocate pe32 image");

    let this_dxe_rust_core_buffer =
        slice_from_raw_parts(
            image_addr as *const u8,
            dxe_core_module_hob.alloc_descriptor.memory_length as usize);


    // compare our relocated image to the image we are currently running (relocated by DxeIpl) and see what we get.
    // ignore the .data section (as that will have been changed as a result of execution).
    let data_section = pe.sections
        .iter()
        .find_map(|section|{
            if let Result::Ok(name) = section.name() {
                if name == ".data" {
                    return Some(section.virtual_address as usize..(section.virtual_address+section.virtual_size) as usize);
                }
            }
            None
        }).expect("couldn't find data section");

    let mut differences = 0;
    for index in 0..loaded_image.len() {
        if data_section.contains(&index) { continue; }
        let reference_byte = unsafe{&*this_dxe_rust_core_buffer}[index];
        if loaded_image[index] != reference_byte {
            println!("Relocation differs from reference at byte {:#x}. Expected: {:#x}, got :{:#x}", index, reference_byte, loaded_image[index]);
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
    let target_guid_from_str = uuid::Uuid::from_str("35AFEBCD-8485-4865-A9EC-447FF8EA47A9").unwrap().to_bytes_le();
    let target_guid: Guid = unsafe {*(target_guid_from_str.as_ptr() as *const Guid)};

    let target_module_pe32: FfsSection = FirmwareVolume::new(fv_hob.base_address)
    .ffs_files()
    .find_map(|file|{
        if file.file_name() != target_guid {
            return None;
        }
        file.ffs_sections().find_map(|section|{
            if section.section_type() == Some(FfsSectionType::EfiSectionPe32) {
                println!("Located target module {:?}", file);
                return Some(section)
            }
            None
        })
    }).expect("Target module not found.");


    let target_pe = goblin::pe::PE::parse(target_module_pe32.section_data()).unwrap();
    let size_of_target = target_pe.header.optional_header.unwrap().windows_fields.size_of_image;
    //allocate space for the image on the heap
    let mut loaded_target: Vec<u8> = vec![0; size_of_target as usize];
    pe32::pe32_load_image(target_module_pe32.section_data(), & mut loaded_target).expect("failed to load target");
    //apply relocation to the target image at its current address
    let target_image_addr = loaded_target.as_mut() as *mut [u8] as *mut u8 as usize;
    pe32::pe32_relocate_image(target_image_addr, &mut loaded_target).expect("failed to relocate target");

    //relocated image is ready to execute, compute entry point address
    let target_entry_point_addr = target_image_addr + target_pe.entry;

    println!("Invoking target module.");
    let ptr = target_entry_point_addr as *const ();
    let entry_point: unsafe extern "efiapi" fn(*const u8) -> u64 = unsafe { transmute (ptr)};

    let status = unsafe {entry_point(hob_list as *const u8)};

    println!("Back from target module with status {:#x}", status);

    println!("It did not crash!");
    dxe_rust::hlt_loop();
}


/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
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
