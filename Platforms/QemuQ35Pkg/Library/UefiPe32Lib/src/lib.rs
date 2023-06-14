#![no_std]

extern crate alloc;

use alloc::{
  string::{String, ToString},
  vec::Vec,
};
use goblin;
use scroll::Pread;

#[derive(Debug)]
pub enum Pe32Error {
  ParseError(goblin::error::Error),
  NoOptionalHeader,
  LoadError,
  RelocationError,
}

/// A struct containing information about a PE32 image.
#[derive(PartialEq, Debug)]
pub struct Pe32ImageInfo {
  pub entry_point_offset: usize,
  pub image_type: u16,
  pub size_of_image: u32,
  pub section_alignment: u32,
  pub filename: Option<String>,
}

impl Pe32ImageInfo {
  fn new() -> Self {
    Pe32ImageInfo { entry_point_offset: 0, image_type: 0, size_of_image: 0, section_alignment: 0, filename: None }
  }
}

/// Returns a Pe32ImageInfo for the image contained in the input buffer.
pub fn pe32_get_image_info(image: &[u8]) -> Result<Pe32ImageInfo, Pe32Error> {
  let parsed_pe = goblin::pe::PE::parse(image).map_err(|e| Pe32Error::ParseError(e))?;

  let mut pe_info = Pe32ImageInfo::new();
  pe_info.entry_point_offset = parsed_pe.entry;

  if let Some(optional_header) = parsed_pe.header.optional_header {
    pe_info.image_type = optional_header.windows_fields.subsystem;
    pe_info.size_of_image = optional_header.windows_fields.size_of_image;
    pe_info.section_alignment = optional_header.windows_fields.section_alignment;
  } else {
    return Err(Pe32Error::NoOptionalHeader);
  }

  if let Some(debug_data) = parsed_pe.debug_data {
    if let Some(codeview_data) = debug_data.codeview_pdb70_debug_info {
      let filename_end =
        codeview_data.filename.iter().position(|&c| c == b'\0').unwrap_or(codeview_data.filename.len());
      let filename = String::from_utf8(codeview_data.filename[0..filename_end].to_vec()).ok();
      if let Some(mut filename) = filename {
        if filename.ends_with(".pdb") {
          filename.truncate(filename.len() - 4);
        }
        if let Some(index) = filename.rfind(|c| c == '/' || c == '\\') {
          filename = filename.split_at(index + 1).1.to_string();
        }
        pe_info.filename = Some(filename + ".efi");
      }
    }
  }

  Ok(pe_info)
}

///does a PE32 load for the image specified by image into the memory buffer specified by loaded_image.
pub fn pe32_load_image(image: &[u8], loaded_image: &mut [u8]) -> Result<(), Pe32Error> {
  let pe = goblin::pe::PE::parse(image).map_err(|e| Pe32Error::ParseError(e))?;
  let size_of_headers = pe.header.optional_header.ok_or(Pe32Error::LoadError)?.windows_fields.size_of_headers as usize;

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

/// relocate the given image at the base address specified by destination.
/// the image base in the header is updated, and all relocation fixups are applied.
/// Note: image is expected to have already been loaded by pe32_load_image.
pub fn pe32_relocate_image(destination: usize, image: &mut [u8]) -> Result<(), Pe32Error> {
  let pe = goblin::pe::PE::parse(&image).map_err(|e| Pe32Error::ParseError(e))?;
  let current_base = pe.header.optional_header.ok_or(Pe32Error::RelocationError)?.windows_fields.image_base;
  let adjustment = (destination as u64).wrapping_sub(current_base);

  if adjustment != 0 {
    //write the new base in to the "Optional Header Windows-Specific Fields" in image.
    //goblin doesn't give us an easy way to determine that raw offset, so we have to build it ourselves.
    //safety: the following code assumes the image always has "Optional" headers, but we return error above
    //if it doesn't.
    let mut windows_fields_offset = pe.header.dos_header.pe_pointer;
    windows_fields_offset += goblin::pe::header::SIZEOF_COFF_HEADER as u32;
    windows_fields_offset += 4; //PE32 signature
    windows_fields_offset += goblin::pe::optional_header::SIZEOF_STANDARD_FIELDS_64 as u32;
    let windows_field_addr = (image as *const [u8] as *const u8 as usize) + windows_fields_offset as usize;
    let windows_fields = windows_field_addr as *mut goblin::pe::optional_header::WindowsFields64;
    unsafe {
      (*windows_fields).image_base = destination as u64;
    }

    let pe_opt_header = pe.header.optional_header.ok_or(Pe32Error::RelocationError)?;

    let reloc_section_option = pe_opt_header.data_directories.get_base_relocation_table();

    if let Some(reloc_section) = reloc_section_option {
      let relocation_data = image
        .get(reloc_section.virtual_address as usize..(reloc_section.virtual_address + reloc_section.size) as usize)
        .ok_or(Pe32Error::RelocationError)?;

      for reloc_block in parse_relocation_blocks(relocation_data)? {
        for reloc in reloc_block.relocations {
          let fixup_type = reloc.type_and_offset >> 12;
          let fixup = reloc_block.block_header.page_rva as usize + (reloc.type_and_offset & 0xFFF) as usize;

          match fixup_type {
            0x00 => (), // IMAGE_REL_BASE_ABSOLUTE - no action, //IMAGE_REL_BASE_ABSOLUTE: no action.
            0x0A => {
              //IMAGE_REL_BASED_DIR64
              let mut fixup_value =
                u64::from_le_bytes(image[fixup..fixup + 8].try_into().map_err(|_| Pe32Error::RelocationError)?);

              fixup_value = fixup_value.wrapping_add(adjustment);

              let subslice = image.get_mut(fixup..fixup + 8).ok_or(Pe32Error::RelocationError)?;

              subslice.copy_from_slice(&fixup_value.to_le_bytes()[..]);
            }
            _ => todo!(), // Other fixups not implemented at this time
          }
        }
      }
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  extern crate std;
  use std::{fs::File, io::Read};

  use alloc::vec;

  use super::*;

  macro_rules! test_collateral {
    ($fname:expr) => {
      concat!(env!("CARGO_MANIFEST_DIR"), "/resources/test/", $fname)
    };
  }

  #[test]
  fn pe32_get_image_info_should_return_image_info() {
    // test_image.pe32 file is just a copy of RustFfiTestDxe.efi module copied and renamed.
    let mut test_file = File::open(test_collateral!("test_image.pe32")).expect("failed to open test file.");
    let mut buffer = Vec::new();

    test_file.read_to_end(&mut buffer).expect("failed to read test file");

    let result = pe32_get_image_info(&buffer).unwrap();

    //Note: these are the expected values for test_image.pe32, if that file is updated, these values
    //will change.
    assert_eq!(result.entry_point_offset, 0x11B8);
    assert_eq!(result.image_type, 0x0B); //EFI_BOOT_SERVICE_DRIVER
    assert_eq!(result.size_of_image, 0x14000);
    assert_eq!(result.section_alignment, 0x1000);
    assert_eq!(result.filename, Some(String::from("RustFfiTestDxe.efi")));
  }

  #[test]
  fn pe32_load_image_should_load_the_image() {
    let mut test_file = File::open(test_collateral!("test_image.pe32")).expect("failed to open test file.");
    let mut image = Vec::new();

    test_file.read_to_end(&mut image).expect("failed to read test file");
    let image_info = pe32_get_image_info(&image).unwrap();

    let mut loaded_image: Vec<u8> = vec![0; image_info.size_of_image as usize];

    pe32_load_image(&image, &mut loaded_image).unwrap();
    assert_eq!(loaded_image.len(), image_info.size_of_image as usize);

    // the reference "test_image_loaded.bin" was generated by calling pe32_load_image to generate a loaded image buffer
    // and then dumping ito a file. This ensures that future changes to the code that case load to change unexpectedly
    // will fail to match.
    let mut loaded_file =
      File::open(test_collateral!("test_image_loaded.bin")).expect("failed to open loaded image test file.");
    let mut loaded_image_reference = Vec::new();
    loaded_file.read_to_end(&mut loaded_image_reference).expect("failed to read loaded image test file");

    assert_eq!(loaded_image.len(), loaded_image_reference.len());

    let first_mismatch = loaded_image.iter().enumerate().find(|(idx, byte)| &&loaded_image_reference[*idx] != byte);

    assert!(first_mismatch.is_none(), "loaded image mismatch at idx: {:#x?}", first_mismatch.unwrap());
  }

  #[test]
  fn pe32_load_image_should_have_same_image_info() {
    let mut test_file = File::open(test_collateral!("test_image.pe32")).expect("failed to open test file.");
    let mut image = Vec::new();

    test_file.read_to_end(&mut image).expect("failed to read test file");
    let mut image_info = pe32_get_image_info(&image).unwrap();

    let mut loaded_image: Vec<u8> = vec![0; image_info.size_of_image as usize];

    pe32_load_image(&image, &mut loaded_image).unwrap();
    let loaded_image_info = pe32_get_image_info(&loaded_image).unwrap();

    //debug information is not included when loading an image in the present implementation, so filename will not be present.
    image_info.filename = None;
    assert_eq!(image_info, loaded_image_info);
  }

  #[test]
  fn pe32_relocate_image_should_relocate_the_image() {
    let mut test_file = File::open(test_collateral!("test_image.pe32")).expect("failed to open test file.");
    let mut image = Vec::new();

    test_file.read_to_end(&mut image).expect("failed to read test file");
    let image_info = pe32_get_image_info(&image).unwrap();

    let mut relocated_image: Vec<u8> = vec![0; image_info.size_of_image as usize];

    pe32_load_image(&image, &mut relocated_image).unwrap();

    pe32_relocate_image(0x04158000, &mut relocated_image).unwrap();

    // the reference "test_image_relocated.bin" was generated by calling pe32_load_image and pe32_relocate_image
    // to generate a loaded image buffer and then dumping ito a file. This ensures that future changes to the code
    // that case load to change unexpectedly will fail to match.
    let mut relocated_file =
      File::open(test_collateral!("test_image_relocated.bin")).expect("failed to open relocated image test file.");
    let mut relocated_image_reference = Vec::new();
    relocated_file.read_to_end(&mut relocated_image_reference).expect("failed to read relocated image test file");

    let first_mismatch =
      relocated_image.iter().enumerate().find(|(idx, byte)| &&relocated_image_reference[*idx] != byte);

    assert!(first_mismatch.is_none(), "relocated image mismatch at idx: {:#x?}", first_mismatch.unwrap());
  }

  #[test]
  fn pe32_relocate_image_should_work_multiple_times() {
    let mut test_file = File::open(test_collateral!("test_image.pe32")).expect("failed to open test file.");
    let mut image = Vec::new();

    test_file.read_to_end(&mut image).expect("failed to read test file");
    let image_info = pe32_get_image_info(&image).unwrap();

    let mut relocated_image: Vec<u8> = vec![0; image_info.size_of_image as usize];

    pe32_load_image(&image, &mut relocated_image).unwrap();

    pe32_relocate_image(0x04158000, &mut relocated_image).unwrap();

    let mut reclocated_image_copy = relocated_image.clone();

    pe32_relocate_image(0x80000415, &mut reclocated_image_copy).unwrap();

    assert_ne!(relocated_image, reclocated_image_copy);

    pe32_relocate_image(0x04158000, &mut reclocated_image_copy).unwrap();

    assert_eq!(relocated_image, reclocated_image_copy);
  }
}
