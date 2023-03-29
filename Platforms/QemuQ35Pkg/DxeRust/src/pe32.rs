use core::convert::TryInto;
use r_pi::hob::{Hob, HobList};
use alloc::vec::Vec;
use fv_lib::{FfsFileType, FfsSection, FirmwareVolume};
use scroll::Pread;
use crate::println;

#[derive(Debug)]
pub enum Pe32Error {
    SectionNotFound,
    ParseError(goblin::error::Error),
    LoadError,
    RelocationError,
}

fn get_pe32_section(hob_list: &HobList) -> Option<FfsSection> {
    let fv_hob = hob_list.iter().find_map(|h| {
        if let Hob::FirmwareVolume(fv) = h {
            return Some(fv);
        }
        None
    })?;

    // not very safe, any base address can be parsed to the constructor.
    // it would maybe be a good idea to create a FirmwareVolume in the fv
    // module, attached to the Hob::FirmwareVolume enum.
    let volume = FirmwareVolume::new(fv_hob.base_address);

    let dxe_core_file = volume.ffs_files().find(|f| f.file_type() == Some(FfsFileType::EfiFvFileTypeDxeCore))?;

    dxe_core_file.ffs_sections().find(|s| s.is_pe32())
}

pub fn parse_first(hob_list: &HobList) -> Result<(), Pe32Error> {
    let file_section = get_pe32_section(hob_list).ok_or(Pe32Error::SectionNotFound)?;
    let data = file_section.section_data();
    let pe = goblin::pe::PE::parse(data).map_err(|e| Pe32Error::ParseError(e))?;
    println!("pe parsed: {:?}", pe);

    Ok(())
}

// load a pe32_image into the provided loaded_image buffer.
// loaded_image is provided by the caller (as opposed to being allocated and returned)
// so that it can be placed outside the rust heap if desired.
// caller must ensure that loaded_image is big enough to hold the generated image.
pub fn pe32_load_image(image: &[u8], loaded_image: &mut [u8]) -> Result<(), Pe32Error> {
    let pe = goblin::pe::PE::parse(image).map_err(|e| Pe32Error::ParseError(e))?;
    let size_of_headers =
        pe.header.optional_header.ok_or(Pe32Error::LoadError)?.windows_fields.size_of_headers as usize;

    //zero the buffer (as the section copy below is sparse and will not initialize all bytes)
    loaded_image.fill_with(|| 0);

    //copy the headers
    let dst = loaded_image.get_mut(..size_of_headers).ok_or(Pe32Error::LoadError)?;
    let src = image.get(..size_of_headers).ok_or(Pe32Error::LoadError)?;
    dst.copy_from_slice(src);

    //copy the sections
    for section in pe.sections {
        let mut size = section.virtual_size;
        if size == 0 || size > section.size_of_raw_data {
            size = section.size_of_raw_data;
        }

        let dst = loaded_image
            .get_mut((section.virtual_address as usize)..(section.virtual_address.wrapping_add(size) as usize))
            .ok_or(Pe32Error::LoadError)?;

        let src = image
            .get((section.pointer_to_raw_data as usize)..(section.pointer_to_raw_data.wrapping_add(size) as usize))
            .ok_or(Pe32Error::LoadError)?;

        dst.copy_from_slice(src);
    }

    Ok(())
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pread)]
struct BaseRelocationBlockHeader {
    page_rva: u32,
    block_size: u32,
}
#[repr(C)]
#[derive(Debug, Copy, Clone, Pread)]
struct Relocation {
    type_and_offset: u16,
}

#[derive(Debug, Clone)]
struct RelocationBlock {
    block_header: BaseRelocationBlockHeader,
    relocations: Vec<Relocation>,
}

fn parse_relocation_blocks(block: &[u8]) -> Result<Vec<RelocationBlock>, Pe32Error> {
    let mut offset: usize = 0;
    let mut blocks = Vec::new();

    while offset < block.len() {
        let block_start = offset;
        let block_header: BaseRelocationBlockHeader =
            block.gread_with(&mut offset, scroll::LE).map_err(|_| Pe32Error::RelocationError)?;

        let mut relocations = Vec::new();
        while offset < block_start + block_header.block_size as usize {
            relocations.push(block.gread_with(&mut offset, scroll::LE).map_err(|_| Pe32Error::RelocationError)?);
        }

        blocks.push(RelocationBlock { block_header, relocations });
        // block start on 32-bit boundary, so align up if needed.
        offset = (offset + 3) & !3;
    }

    Ok(blocks)
}

// relocate the given image at the base address specified by destination.
// the image base in the header is update, and all relocation fixups are applied.
pub fn pe32_relocate_image(destination: usize, image: &mut [u8]) -> Result<(), Pe32Error> {
    let pe = goblin::pe::PE::parse(&image).map_err(|e| Pe32Error::ParseError(e))?;
    let current_base = pe.header.optional_header.ok_or(Pe32Error::RelocationError)?.windows_fields.image_base;
    let adjustment = (destination as u64).wrapping_sub(current_base);
    //println!("current_base {:#x} adjusting to destination {:#x} with adjustment {:#x}", current_base, destination, adjustment);
    if adjustment != 0 {
        //write the new base in to the "Optional Header Windows-Specific Fields" in image.
        //goblin doesn't give us an easy way to determine that raw offset, so we have to build it ourselves.
        //safety: the following code assumes the image always has "Optional" headers, but we return error above
        //if it doesn't.
        unsafe {
            let mut windows_fields_offset = pe.header.dos_header.pe_pointer;
            windows_fields_offset += goblin::pe::header::SIZEOF_COFF_HEADER as u32;
            windows_fields_offset += 4; //PE32 signature
            windows_fields_offset += goblin::pe::optional_header::SIZEOF_STANDARD_FIELDS_64 as u32;
            let windows_field_addr = (image as *const [u8] as *const u8 as usize) + windows_fields_offset as usize;
            let windows_fields = windows_field_addr as *mut goblin::pe::optional_header::WindowsFields64;

            (*windows_fields).image_base = destination as u64;
        }

        //println!("PE: {:#?}", pe);
        let pe_opt_header = pe.header.optional_header.ok_or(Pe32Error::RelocationError)?;

        let reloc_section_option = pe_opt_header.data_directories.get_base_relocation_table();

        if let Some(reloc_section) = reloc_section_option {
            let relocation_data = image
                .get(
                    reloc_section.virtual_address as usize
                        ..(reloc_section.virtual_address + reloc_section.size) as usize,
                )
                .ok_or(Pe32Error::RelocationError)?;

            for reloc_block in parse_relocation_blocks(relocation_data)? {
                //println!("processing block with rva {:x}", reloc_block.block_header.page_rva);
                for reloc in reloc_block.relocations {
                    let fixup_type = reloc.type_and_offset >> 12;
                    let fixup = reloc_block.block_header.page_rva as usize + (reloc.type_and_offset & 0xFFF) as usize;

                    match fixup_type {
                        0x00 => {
                            //println!("  IMAGE_REL_BASE_ABSOLUTE: no action");
                            ()
                        } //IMAGE_REL_BASE_ABSOLUTE - no action, //IMAGE_REL_BASE_ABSOLUTE: no action.
                        0x0A => {
                            //IMAGE_REL_BASED_DIR64
                            let mut fixup_value = u64::from_le_bytes(
                                image[fixup..fixup + 8].try_into().map_err(|_| Pe32Error::RelocationError)?,
                            );
                            //print!("  IMAGE_REL_BASED_DIR64: Adjusting {:#x} to ", fixup_value);
                            fixup_value = fixup_value.wrapping_add(adjustment);
                            //println!("{:#x} at offset {:#x}", fixup_value, fixup);
                            let subslice = image.get_mut(fixup..fixup + 8).ok_or(Pe32Error::RelocationError)?;

                            subslice.copy_from_slice(&fixup_value.to_le_bytes()[..]);
                        }
                        _ => todo!(), // Other fixups not implemented at this time
                    }
                }
            }
        }
        //println!("relocation of current_base {:#x} adjusting to destination {:#x} with adjustment {:#x} complete.", current_base, destination, adjustment);
    }
    Ok(())
}
