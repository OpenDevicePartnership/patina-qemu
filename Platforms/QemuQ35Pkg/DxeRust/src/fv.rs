// Based on the definitions in
// https://github.com/tianocore/edk2/blob/master/MdePkg/Include/Pi/PiFirmwareVolume.h
// https://github.com/tianocore/edk2/blob/master/MdePkg/Include/Pi/PiFirmwareFile.h

#![allow(dead_code)] //allow private constants that are not (yet) used.

use alloc::string::ToString;
use core::{fmt, mem::size_of, slice};
use r_efi::base::Guid;
use uuid::Uuid;
use x86_64::align_up;

// EFI_FIRMWARE_VOLUME_HEADER
#[repr(C)]
#[derive(Debug)]
struct FirmwareVolumeHeader {
    zero_vector: [u8; 16],
    file_system_guid: Guid,
    fv_length: u64,
    signature: u32,
    attributes: u32,
    header_length: u16,
    checksum: u16,
    ext_header_offset: u16,
    reserved: u8,
    revision: u8,
    //todo: block map starts here, but we don't need it for now.
}

// EFI_FIRMWARE_VOLUME_EXT_HEADER
#[derive(Debug)]
struct FirmwareVolumeExtHeader {
    fv_name: Guid,
    ext_header_size: u32,
}

// File Types Definitions.
const EFI_FV_FILETYPE_ALL: u8 = 0x00;
const EFI_FV_FILETYPE_RAW: u8 = 0x01;
const EFI_FV_FILETYPE_FREEFORM: u8 = 0x02;
const EFI_FV_FILETYPE_SECURITY_CORE: u8 = 0x03;
const EFI_FV_FILETYPE_PEI_CORE: u8 = 0x04;
const EFI_FV_FILETYPE_DXE_CORE: u8 = 0x05;
const EFI_FV_FILETYPE_PEIM: u8 = 0x06;
const EFI_FV_FILETYPE_DRIVER: u8 = 0x07;
const EFI_FV_FILETYPE_COMBINED_PEIM_DRIVER: u8 = 0x08;
const EFI_FV_FILETYPE_APPLICATION: u8 = 0x09;
const EFI_FV_FILETYPE_MM: u8 = 0x0A;
const EFI_FV_FILETYPE_FIRMWARE_VOLUME_IMAGE: u8 = 0x0B;
const EFI_FV_FILETYPE_COMBINED_MM_DXE: u8 = 0x0C;
const EFI_FV_FILETYPE_MM_CORE: u8 = 0x0D;
const EFI_FV_FILETYPE_MM_STANDALONE: u8 = 0x0E;
const EFI_FV_FILETYPE_MM_CORE_STANDALONE: u8 = 0x0F;
const EFI_FV_FILETYPE_OEM_MIN: u8 = 0xc0;
const EFI_FV_FILETYPE_OEM_MAX: u8 = 0xdf;
const EFI_FV_FILETYPE_DEBUG_MIN: u8 = 0xe0;
const EFI_FV_FILETYPE_DEBUG_MAX: u8 = 0xef;
const EFI_FV_FILETYPE_FFS_MIN: u8 = 0xf1; //note, technically includes FFS_PAD at 0xF0.
const EFI_FV_FILETYPE_FFS_MAX: u8 = 0xff;
const EFI_FV_FILETYPE_FFS_PAD: u8 = 0xf0;

// FFS File Attributes.
const FFS_ATTRIB_LARGE_FILE: u8 = 0x01;
const FFS_ATTRIB_DATA_ALIGNMENT_2: u8 = 0x02;
const FFS_ATTRIB_FIXED: u8 = 0x04;
const FFS_ATTRIB_DATA_ALIGNMENT: u8 = 0x38;
const FFS_ATTRIB_CHECKSUM: u8 = 0x40;

// FFS File State Bits.
const EFI_FILE_HEADER_CONSTRUCTION: u8 = 0x01;
const EFI_FILE_HEADER_VALID: u8 = 0x02;
const EFI_FILE_DATA_VALID: u8 = 0x04;
const EFI_FILE_MARKED_FOR_UPDATE: u8 = 0x08;
const EFI_FILE_DELETED: u8 = 0x10;
const EFI_FILE_HEADER_INVALID: u8 = 0x20;

// Pseudo type. It is used as a wild card when retrieving sections.
//  The section type EFI_SECTION_ALL matches all section types.
const EFI_SECTION_ALL: u8 = 0x00;

// Encapsulation section Type values.
const EFI_SECTION_COMPRESSION: u8 = 0x01;
const EFI_SECTION_GUID_DEFINED: u8 = 0x02;
const EFI_SECTION_DISPOSABLE: u8 = 0x03;

// Leaf section Type values.
const EFI_SECTION_PE32: u8 = 0x10;
const EFI_SECTION_PIC: u8 = 0x11;
const EFI_SECTION_TE: u8 = 0x12;
const EFI_SECTION_DXE_DEPEX: u8 = 0x13;
const EFI_SECTION_VERSION: u8 = 0x14;
const EFI_SECTION_USER_INTERFACE: u8 = 0x15;
const EFI_SECTION_COMPATIBILITY16: u8 = 0x16;
const EFI_SECTION_FIRMWARE_VOLUME_IMAGE: u8 = 0x17;
const EFI_SECTION_FREEFORM_SUBTYPE_GUID: u8 = 0x18;
const EFI_SECTION_RAW: u8 = 0x19;
const EFI_SECTION_PEI_DEPEX: u8 = 0x1B;
const EFI_SECTION_MM_DEPEX: u8 = 0x1C;

// EFI_FFS_FILE_HEADER
#[repr(C)]
#[derive(Debug)]
struct FfsFileHeader {
    name: Guid,
    integrity_check_header: u8,
    integrity_check_file: u8,
    file_type: u8,
    attributes: u8,
    size: [u8; 3],
    state: u8,
}
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum FfsFileType {
    EfiFvFileTypeAll = EFI_FV_FILETYPE_ALL,
    EfiFvFileTypeRaw = EFI_FV_FILETYPE_RAW,
    EfiFvFileTypeFreeForm = EFI_FV_FILETYPE_FREEFORM,
    EfiFvFileTypeSecurityCore = EFI_FV_FILETYPE_SECURITY_CORE,
    EfiFvFileTypePeiCore = EFI_FV_FILETYPE_PEI_CORE,
    EfiFvFileTypeDxeCore = EFI_FV_FILETYPE_DXE_CORE,
    EfiFvFileTypePeim = EFI_FV_FILETYPE_PEIM,
    EfiFvFileTypeDriver = EFI_FV_FILETYPE_DRIVER,
    EfiFvFileTypeCombinedPeimDriver = EFI_FV_FILETYPE_COMBINED_PEIM_DRIVER,
    EfiFvFileTypeApplication = EFI_FV_FILETYPE_APPLICATION,
    EfiFvFileTypeMm = EFI_FV_FILETYPE_MM,
    EfiFvFileTypeFirmwareVolumeImage = EFI_FV_FILETYPE_FIRMWARE_VOLUME_IMAGE,
    EfiFvFileTypeCombinedMmDxe = EFI_FV_FILETYPE_COMBINED_MM_DXE,
    EfiFvFileTypeMmCore = EFI_FV_FILETYPE_MM_CORE,
    EfiFvFileTypeMmStandalone = EFI_FV_FILETYPE_MM_STANDALONE,
    EfiFvFileTypeMmCoreStandalone = EFI_FV_FILETYPE_MM_CORE_STANDALONE,
    EfiFvFileTypeOem = EFI_FV_FILETYPE_OEM_MIN,
    EfiFvFileTypeDebug = EFI_FV_FILETYPE_DEBUG_MIN,
    EfiFvFileTypeFfsPad = EFI_FV_FILETYPE_FFS_PAD,
    EfiFvFileTypeFfsUnknown = EFI_FV_FILETYPE_FFS_MIN,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum FfsSectionType {
    EfiSectionAll = EFI_SECTION_ALL,
    EfiSectionCompression = EFI_SECTION_COMPRESSION,
    EfiSectionGuidDefined = EFI_SECTION_GUID_DEFINED,
    EfiSectionDisposable = EFI_SECTION_DISPOSABLE,
    EfiSectionPe32 = EFI_SECTION_PE32,
    EfiSectionPic = EFI_SECTION_PIC,
    EfiSectionTe = EFI_SECTION_TE,
    EfiSectionDxeDepex = EFI_SECTION_DXE_DEPEX,
    EfiSectionVersion = EFI_SECTION_VERSION,
    EfiSectionUserInterface = EFI_SECTION_USER_INTERFACE,
    EfiSectionCompatibility16 = EFI_SECTION_COMPATIBILITY16,
    EfiSectionFirmwareVolumeImage = EFI_SECTION_FIRMWARE_VOLUME_IMAGE,
    EfiSectionFreeformSubtypeGuid = EFI_SECTION_FREEFORM_SUBTYPE_GUID,
    EfiSectionRaw = EFI_SECTION_RAW,
    EfiSectionPeiDepex = EFI_SECTION_PEI_DEPEX,
    EfiSectionMmDepex = EFI_SECTION_MM_DEPEX,
}

// EFI_COMMON_SECTION_HEADER
#[repr(C)]
#[derive(Debug)]
struct CommonSectionHeader {
    size: [u8; 3],
    section_type: u8,
}
// EFI_COMPRESSION_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionCompression {
    common_header: CommonSectionHeader,
    uncompressed_length: u32,
    compression_type: u8,
}

// EFI_GUID_DEFINED_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionGuidDefined {
    common_header: CommonSectionHeader,
    section_definition_guid: Guid,
    data_offset: u16,
    attributes: u16,
    // Guid-specific header fields.
}

// EFI_DISPOSABLE_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionDisposable {
    common_header: CommonSectionHeader,
}

// EFI_PE32_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionPe32 {
    common_header: CommonSectionHeader,
}

// EFI_PIC_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionPic {
    common_header: CommonSectionHeader,
}

// EFI_TE_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionTe {
    common_header: CommonSectionHeader,
}

// EFI_DXE_DEPEX_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionDxeDepex {
    common_header: CommonSectionHeader,
}

// EFI_VERSION_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionVersion {
    common_header: CommonSectionHeader,
    build_number: u16,
}

// EFI_USER_INTERFACE_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionUserInterface {
    common_header: CommonSectionHeader,
}

// EFI_COMPATIBILITY16_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionCompatibility16 {
    common_header: CommonSectionHeader,
}

// EFI_FIRMWARE_VOLUME_IMAGE_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionFirmwareVolumeImage {
    common_header: CommonSectionHeader,
}

// EFI_FREEFORM_SUBTYPE_GUID_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionFreeformSubtypeGuid {
    common_header: CommonSectionHeader,
    sub_type_guid: Guid,
}

// EFI_RAW_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionRaw {
    common_header: CommonSectionHeader,
}

// EFI_PEI_DEPEX_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionPeiDepex {
    common_header: CommonSectionHeader,
}

// EFI_MM_DEPEX_SECTION
#[repr(C)]
#[derive(Debug)]
struct FfsSectionMmDepex {
    common_header: CommonSectionHeader,
}

#[derive(Debug, Copy, Clone)]
enum GenericFfsSection {
    Compression(*const FfsSectionCompression),
    GuidDefined(*const FfsSectionGuidDefined),
    Disposable(*const FfsSectionDisposable),
    Pe32(*const FfsSectionPe32),
    Pic(*const FfsSectionPic),
    Te(*const FfsSectionTe),
    DxeDepex(*const FfsSectionDxeDepex),
    Version(*const FfsSectionVersion),
    UserInterface(*const FfsSectionUserInterface),
    Compatibility16(*const FfsSectionCompatibility16),
    FirmwareVolumeImage(*const FfsSectionFirmwareVolumeImage),
    FreeformSubtypeGuid(*const FfsSectionFreeformSubtypeGuid),
    Raw(*const FfsSectionRaw),
    PeiDepex(*const FfsSectionPeiDepex),
    MmDepex(*const FfsSectionMmDepex),
    Unknown(*const CommonSectionHeader),
}

// public Fv interface
#[derive(Copy, Clone)]
pub struct FfsSection {
    containing_ffs: FfsFile,
    common_header: *const CommonSectionHeader,
    ffs_section: GenericFfsSection,
}

impl FfsSection {
    pub fn new(containing_ffs: FfsFile, base_address: u64) -> FfsSection {
        let common_header: *const CommonSectionHeader = base_address as *const CommonSectionHeader;
        FfsSection {
            containing_ffs,
            common_header,
            ffs_section: match unsafe { (*common_header).section_type } {
                EFI_SECTION_COMPRESSION => {
                    GenericFfsSection::Compression(common_header as *const FfsSectionCompression)
                }
                EFI_SECTION_GUID_DEFINED => {
                    GenericFfsSection::GuidDefined(common_header as *const FfsSectionGuidDefined)
                }
                EFI_SECTION_DISPOSABLE => GenericFfsSection::Disposable(common_header as *const FfsSectionDisposable),
                EFI_SECTION_PE32 => GenericFfsSection::Pe32(common_header as *const FfsSectionPe32),
                EFI_SECTION_PIC => GenericFfsSection::Pic(common_header as *const FfsSectionPic),
                EFI_SECTION_TE => GenericFfsSection::Te(common_header as *const FfsSectionTe),
                EFI_SECTION_DXE_DEPEX => GenericFfsSection::DxeDepex(common_header as *const FfsSectionDxeDepex),
                EFI_SECTION_VERSION => GenericFfsSection::Version(common_header as *const FfsSectionVersion),
                EFI_SECTION_USER_INTERFACE => {
                    GenericFfsSection::UserInterface(common_header as *const FfsSectionUserInterface)
                }
                EFI_SECTION_COMPATIBILITY16 => {
                    GenericFfsSection::Compatibility16(common_header as *const FfsSectionCompatibility16)
                }
                EFI_SECTION_FIRMWARE_VOLUME_IMAGE => {
                    GenericFfsSection::FirmwareVolumeImage(common_header as *const FfsSectionFirmwareVolumeImage)
                }
                EFI_SECTION_FREEFORM_SUBTYPE_GUID => {
                    GenericFfsSection::FreeformSubtypeGuid(common_header as *const FfsSectionFreeformSubtypeGuid)
                }
                EFI_SECTION_RAW => GenericFfsSection::Raw(common_header as *const FfsSectionRaw),
                EFI_SECTION_PEI_DEPEX => GenericFfsSection::PeiDepex(common_header as *const FfsSectionPeiDepex),
                EFI_SECTION_MM_DEPEX => GenericFfsSection::MmDepex(common_header as *const FfsSectionMmDepex),
                _ => GenericFfsSection::Unknown(common_header),
            },
        }
    }

    pub fn is_pe32(&self) -> bool {
        if let GenericFfsSection::Pe32(_) = &self.ffs_section {
            return true;
        }
        false
    }

    pub fn base_address(&self) -> u64 {
        self.common_header as u64
    }

    pub fn section_type(&self) -> Option<FfsSectionType> {
        match self.ffs_section {
            GenericFfsSection::Compression(_) => Some(FfsSectionType::EfiSectionCompression),
            GenericFfsSection::GuidDefined(_) => Some(FfsSectionType::EfiSectionGuidDefined),
            GenericFfsSection::Disposable(_) => Some(FfsSectionType::EfiSectionDisposable),
            GenericFfsSection::Pe32(_) => Some(FfsSectionType::EfiSectionPe32),
            GenericFfsSection::Pic(_) => Some(FfsSectionType::EfiSectionPic),
            GenericFfsSection::Te(_) => Some(FfsSectionType::EfiSectionTe),
            GenericFfsSection::DxeDepex(_) => Some(FfsSectionType::EfiSectionDxeDepex),
            GenericFfsSection::Version(_) => Some(FfsSectionType::EfiSectionVersion),
            GenericFfsSection::UserInterface(_) => Some(FfsSectionType::EfiSectionUserInterface),
            GenericFfsSection::Compatibility16(_) => Some(FfsSectionType::EfiSectionCompatibility16),
            GenericFfsSection::FirmwareVolumeImage(_) => Some(FfsSectionType::EfiSectionFirmwareVolumeImage),
            GenericFfsSection::FreeformSubtypeGuid(_) => Some(FfsSectionType::EfiSectionFreeformSubtypeGuid),
            GenericFfsSection::Raw(_) => Some(FfsSectionType::EfiSectionRaw),
            GenericFfsSection::PeiDepex(_) => Some(FfsSectionType::EfiSectionPeiDepex),
            GenericFfsSection::MmDepex(_) => Some(FfsSectionType::EfiSectionMmDepex),
            _ => None,
        }
    }

    pub fn section_size(&self) -> u64 {
        let mut size: u64 = 0;
        unsafe {
            size += (*self.common_header).size[0] as u64;
            size += ((*self.common_header).size[1] as u64) << 8;
            size += ((*self.common_header).size[2] as u64) << 16;
        }
        size
    }

    pub fn section_data(&self) -> &[u8] {
        let data_offset = match self.ffs_section {
            GenericFfsSection::Compression(_) => size_of::<FfsSectionCompression>() as u64,
            GenericFfsSection::FreeformSubtypeGuid(_) => size_of::<FfsSectionFreeformSubtypeGuid>() as u64,
            GenericFfsSection::GuidDefined(guid_defined) => unsafe { (*guid_defined).data_offset as u64 },
            _ => size_of::<CommonSectionHeader>() as u64,
        };

        let data_start_addr = self.base_address() + data_offset;
        let data_size = self.section_size() - data_offset;

        unsafe { slice::from_raw_parts(data_start_addr as *const u8, data_size as usize) }
    }

    pub fn next_section(&self) -> Option<FfsSection> {
        let mut next_section_address = self.common_header as u64;
        next_section_address += self.section_size();

        // per the PI spec, "The section headers aligned on 4 byte boundaries relative to the start of the file's image"
        // but, in fact, that just means "4-byte aligned" per the EDK2 implementation.
        next_section_address = align_up(next_section_address, 0x4);

        // check to see if we ran off the end of the file yet.
        if next_section_address <= (self.containing_ffs.top_address() - size_of::<CommonSectionHeader>() as u64) {
            return Some(FfsSection::new(self.containing_ffs, next_section_address));
        }
        None
    }
}

impl fmt::Debug for FfsSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FfsSection @{:#x} type: {:?} size: {:?}",
            self.base_address(),
            self.section_type(),
            self.section_size()
        )
    }
}

struct FfsSectionIterator {
    next_section: Option<FfsSection>,
}

impl FfsSectionIterator {
    pub fn new(start_section: Option<FfsSection>) -> FfsSectionIterator {
        FfsSectionIterator { next_section: start_section }
    }
}

impl Iterator for FfsSectionIterator {
    type Item = FfsSection;
    fn next(&mut self) -> Option<FfsSection> {
        let current = self.next_section?;
        self.next_section = current.next_section();
        Some(current)
    }
}

#[derive(Copy, Clone)]
pub struct FfsFile {
    containing_fv: FirmwareVolume,
    ffs_file: *const FfsFileHeader,
}

impl FfsFile {
    pub fn new(containing_fv: FirmwareVolume, base_address: u64) -> FfsFile {
        let ffs_file: *const FfsFileHeader = base_address as *const FfsFileHeader;
        FfsFile { containing_fv, ffs_file }
    }

    pub fn file_size(&self) -> u64 {
        let mut size: u64 = 0;
        unsafe {
            size += (*self.ffs_file).size[0] as u64;
            size += ((*self.ffs_file).size[1] as u64) << 8;
            size += ((*self.ffs_file).size[2] as u64) << 16;
        }
        size
    }

    pub fn file_type_raw(&self) -> u8 {
        unsafe { (*self.ffs_file).file_type }
    }

    pub fn file_type(&self) -> Option<FfsFileType> {
        match self.file_type_raw() {
            EFI_FV_FILETYPE_RAW => Some(FfsFileType::EfiFvFileTypeRaw),
            EFI_FV_FILETYPE_FREEFORM => Some(FfsFileType::EfiFvFileTypeFreeForm),
            EFI_FV_FILETYPE_SECURITY_CORE => Some(FfsFileType::EfiFvFileTypeSecurityCore),
            EFI_FV_FILETYPE_PEI_CORE => Some(FfsFileType::EfiFvFileTypePeiCore),
            EFI_FV_FILETYPE_DXE_CORE => Some(FfsFileType::EfiFvFileTypeDxeCore),
            EFI_FV_FILETYPE_PEIM => Some(FfsFileType::EfiFvFileTypePeim),
            EFI_FV_FILETYPE_DRIVER => Some(FfsFileType::EfiFvFileTypeDriver),
            EFI_FV_FILETYPE_COMBINED_PEIM_DRIVER => Some(FfsFileType::EfiFvFileTypeCombinedPeimDriver),
            EFI_FV_FILETYPE_APPLICATION => Some(FfsFileType::EfiFvFileTypeApplication),
            EFI_FV_FILETYPE_MM => Some(FfsFileType::EfiFvFileTypeMm),
            EFI_FV_FILETYPE_FIRMWARE_VOLUME_IMAGE => Some(FfsFileType::EfiFvFileTypeFirmwareVolumeImage),
            EFI_FV_FILETYPE_COMBINED_MM_DXE => Some(FfsFileType::EfiFvFileTypeCombinedMmDxe),
            EFI_FV_FILETYPE_MM_CORE => Some(FfsFileType::EfiFvFileTypeMmCore),
            EFI_FV_FILETYPE_MM_STANDALONE => Some(FfsFileType::EfiFvFileTypeMmStandalone),
            EFI_FV_FILETYPE_MM_CORE_STANDALONE => Some(FfsFileType::EfiFvFileTypeMmCoreStandalone),
            EFI_FV_FILETYPE_OEM_MIN..=EFI_FV_FILETYPE_OEM_MAX => Some(FfsFileType::EfiFvFileTypeOem),
            EFI_FV_FILETYPE_DEBUG_MIN..=EFI_FV_FILETYPE_DEBUG_MAX => Some(FfsFileType::EfiFvFileTypeDebug),
            EFI_FV_FILETYPE_FFS_PAD => Some(FfsFileType::EfiFvFileTypeFfsPad),
            EFI_FV_FILETYPE_FFS_MIN..=EFI_FV_FILETYPE_FFS_MAX => Some(FfsFileType::EfiFvFileTypeFfsUnknown),
            _ => None,
        }
    }

    pub fn file_name(&self) -> Guid {
        unsafe { (*self.ffs_file).name }
    }

    pub fn base_address(&self) -> u64 {
        self.ffs_file as u64
    }

    pub fn top_address(&self) -> u64 {
        self.base_address() + self.file_size()
    }

    pub fn next_ffs_file(&self) -> Option<FfsFile> {
        let mut next_file_address = self.ffs_file as u64;
        next_file_address += self.file_size();

        // per the PI spec, "Given a file F, the next file header is located at the next 8-byte aligned firmware volume
        // offset following the last byte the file F"
        // but, in fact, that just means "8-byte aligned" per the EDK2 implementation.
        next_file_address = align_up(next_file_address, 0x8);

        // check to see if we ran off the end of the FV yet.
        if next_file_address <= (self.containing_fv.top_address() - size_of::<FfsFileHeader>() as u64) {
            let file = FfsFile::new(self.containing_fv, next_file_address);
            // To be super paranoid, we could check a lot of things here to make sure we have a
            // legit file and didn't run into empty space at the end of the FV. For now, assume
            // if the "file_type" is something legit, that the file is good.

            if file.file_type().is_some() {
                return Some(file);
            }
        }
        None
    }

    pub fn first_ffs_section(&self) -> Option<FfsSection> {
        if self.file_size() <= size_of::<FfsFileHeader>() as u64 {
            return None;
        }
        Some(FfsSection::new(*self, self.base_address() + size_of::<FfsFileHeader>() as u64))
    }

    pub fn ffs_sections(&self) -> impl Iterator<Item = FfsSection> {
        FfsSectionIterator::new(self.first_ffs_section())
    }
}

impl fmt::Debug for FfsFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FfsFile @{:#x} type: {:?} name: {:?} size: {:?}",
            self.base_address(),
            self.file_type(),
            Uuid::from_bytes_le(*self.file_name().as_bytes()),
            self.file_size()
        )
    }
}

struct FfsFileIterator {
    next_ffs: Option<FfsFile>,
}

impl FfsFileIterator {
    pub fn new(start_file: FfsFile) -> FfsFileIterator {
        FfsFileIterator { next_ffs: Some(start_file) }
    }
}

impl Iterator for FfsFileIterator {
    type Item = FfsFile;
    fn next(&mut self) -> Option<FfsFile> {
        let current = self.next_ffs?;
        self.next_ffs = current.next_ffs_file();
        Some(current)
    }
}

#[derive(Copy, Clone)]
pub struct FirmwareVolume {
    fv_header: *const FirmwareVolumeHeader,
}

impl FirmwareVolume {
    pub fn new(base_address: u64) -> FirmwareVolume {
        let fv_header = base_address as *const FirmwareVolumeHeader;
        //Note: this assumes that base_address points to something that is an FV with an FFS on it.
        //More robust code would evaluate the FV header file_system_guid and make sure it actually has the correct
        //filesystem type, and probably a number of other sanity checks.
        FirmwareVolume { fv_header: fv_header }
    }

    fn ext_header(&self) -> Option<*const FirmwareVolumeExtHeader> {
        let ext_header_offset = unsafe { (*self.fv_header).ext_header_offset as u64 };
        if ext_header_offset == 0 {
            return None;
        }
        Some((self.base_address() + ext_header_offset) as *const FirmwareVolumeExtHeader)
    }

    pub fn fv_name(&self) -> Option<Guid> {
        if let Some(ext_header) = self.ext_header() {
            return unsafe { Some((*ext_header).fv_name) };
        }
        None
    }

    pub fn first_ffs_file(&self) -> FfsFile {
        let mut ffs_address = self.fv_header as u64;
        if let Some(ext_header) = self.ext_header() {
            // if ext header exists, then file starts after ext header
            unsafe {
                ffs_address += (*self.fv_header).ext_header_offset as u64;
                ffs_address += (*ext_header).ext_header_size as u64;
            }
        } else {
            // otherwise the file starts after the main header.
            unsafe { ffs_address += (*self.fv_header).header_length as u64 }
        }
        ffs_address = align_up(ffs_address, 0x8);
        // Note: it appears possible from the EDK2 implementation that an FV could have a file system with no actual
        // files.
        // More robust code would check and handle that case.
        FfsFile::new(*self, ffs_address)
    }

    pub fn ffs_files(&self) -> impl Iterator<Item = FfsFile> {
        FfsFileIterator::new(self.first_ffs_file())
    }

    pub fn base_address(&self) -> u64 {
        self.fv_header as u64
    }

    pub fn top_address(&self) -> u64 {
        unsafe { self.base_address() + (*self.fv_header).fv_length }
    }
}

impl fmt::Debug for FirmwareVolume {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FirmwareVolume@{:#x}-{:#x} name: {:}",
            self.base_address(),
            self.top_address(),
            match self.fv_name() {
                Some(guid) => Uuid::from_bytes_le(*guid.as_bytes()).to_string(),
                None => "Unspecified".to_string(),
            }
        )
    }
}
