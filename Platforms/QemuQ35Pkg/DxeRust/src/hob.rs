// Based on the definitions in https://github.com/tianocore/edk2/blob/master/MdePkg/Include/Pi/PiHob.h

#[allow(unused)]

use crate::println;
use alloc::vec::Vec;
use core::{ffi::c_void, mem::size_of};
use core::fmt;
use indoc::indoc;
use x86_64::{align_down, align_up};

// HOB type field is a UINT16
pub const HANDOFF: u16 = 0x0001;
pub const MEMORY_ALLOCATION: u16 = 0x0002;
pub const RESOURCE_DESCRIPTOR: u16 = 0x0003;
pub const GUID_EXTENSION: u16 = 0x0004;
pub const FV: u16 = 0x0005;
pub const CPU: u16 = 0x0006;
pub const MEMORY_POOL: u16 = 0x0007;
pub const FV2: u16 = 0x0009;
pub const LOAD_PEIM_UNUSED: u16 = 0x000A;
pub const UEFI_CAPSULE: u16 = 0x000B;
pub const FV3: u16 = 0x000C;
pub const UNUSED: u16 = 0xFFFE;
pub const END_OF_HOB_LIST: u16 = 0xFFFF;

mod header {
    use crate::memory_types::EfiMemoryType;

    // EFI_HOB_GENERIC_HEADER
    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    pub struct Hob {
        pub r#type: u16,   // UINT16
        pub length: u16,   // UINT16
        pub reserved: u32, // UINT32
    }

    // EFI_HOB_MEMORY_ALLOCATION_HEADER
    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    pub struct MemoryAllocation {
        pub name: r_efi::base::Guid,    // EFI_GUID
        pub memory_base_address: u64,   // EFI_PHYSICAL_ADDRESS
        pub memory_length: u64,         // UINT64
        pub memory_type: EfiMemoryType, // EFI_MEMORY_TYPE
        pub reserved: [u8; 4],          // UINT8[4]
    }
}

// EFI_HOB_MEMORY_POOL
pub type MemoryPool = header::Hob;

// EFI_HOB_HANDOFF_INFO_TABLE
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PhaseHandoffInformationTable {
    pub header: header::Hob,     // EFI_HOB_GENERIC_HEADER
    pub version: u32,            // UINT32
    pub boot_mode: u32,          // EFI_BOOT_MODE
    pub memory_top: u64,         // EFI_PHYSICAL_ADDRESS
    pub memory_bottom: u64,      // EFI_PHYSICAL_ADDRESS
    pub free_memory_top: u64,    // EFI_PHYSICAL_ADDRESS
    pub free_memory_bottom: u64, // EFI_PHYSICAL_ADDRESS
    pub end_of_hob_list: u64,    // EFI_PHYSICAL_ADDRESS
}

#[derive(Clone, Debug)]
pub enum Hob<'a> {
    Handoff(&'a PhaseHandoffInformationTable),
    MemoryAllocation(&'a MemoryAllocation),
    MemoryAllocationModule(&'a MemoryAllocationModule),
    Capsule(&'a Capsule),
    ResourceDescriptor(&'a ResourceDescriptor),
    GuidHob(&'a GuidHob),
    FirmwareVolume(&'a FirmwareVolume),
    FirmwareVolume2(&'a FirmwareVolume2),
    FirmwareVolume3(&'a FirmwareVolume3),
    Cpu(&'a Cpu),
    Misc(u16),
}

pub struct HobList<'a>(Vec<Hob<'a>>);

impl Default for HobList<'_> {
    fn default() -> Self {
        HobList::new()
    }
}

impl HobList<'_> {
    pub fn new() -> Self {
        HobList(Vec::new())
    }

    pub fn iter(&self) -> impl Iterator<Item=&Hob> {
        self.0.iter()
    }

    pub fn discover_hobs<'a>(&mut self, hob_list: *const c_void) {
        let mut hob_header: *const header::Hob = hob_list as *const header::Hob;

        loop {
            let current_header = unsafe { &*hob_header.cast::<header::Hob>() };
            match current_header.r#type {
                HANDOFF => {
                    let phit_hob = unsafe {
                        &*hob_header.cast::<PhaseHandoffInformationTable>()
                    };
                    self.0.push(Hob::Handoff(phit_hob));
                }
                MEMORY_ALLOCATION => {
                    if current_header.length == size_of::<MemoryAllocationModule>() as u16 {
                        let mem_alloc_hob =
                        unsafe { &*hob_header.cast::<MemoryAllocationModule>() };
                        self.0.push(Hob::MemoryAllocationModule(mem_alloc_hob));
                    } else {
                        let mem_alloc_hob =
                            unsafe { &*hob_header.cast::<MemoryAllocation>() };
                        self.0.push(Hob::MemoryAllocation(mem_alloc_hob));
                    }
                }
                RESOURCE_DESCRIPTOR => {
                    let resource_desc_hob =
                        unsafe { &*hob_header.cast::<ResourceDescriptor>() };
                    self.0.push(Hob::ResourceDescriptor(resource_desc_hob));
                }
                GUID_EXTENSION => {
                    let guid_hob = unsafe { &*hob_header.cast::<GuidHob>() };
                    self.0.push(Hob::GuidHob(guid_hob));
                }
                FV => {
                    let fv_hob = unsafe { &*hob_header.cast::<FirmwareVolume>() };

                    self.0.push(Hob::FirmwareVolume(fv_hob));
                }
                FV2 => {
                    let fv2_hob =
                        unsafe { &*hob_header.cast::<FirmwareVolume2>() };
                    self.0.push(Hob::FirmwareVolume2(fv2_hob));
                }
                FV3 => {
                    let fv3_hob =
                        unsafe { &*hob_header.cast::<FirmwareVolume3>() };
                    self.0.push(Hob::FirmwareVolume3(fv3_hob));
                }
                CPU => {
                    let cpu_hob = unsafe { &*hob_header.cast::<Cpu>() };
                    self.0.push(Hob::Cpu(cpu_hob));
                }
                UEFI_CAPSULE => {
                    let capsule_hob = unsafe { &*hob_header.cast::<Capsule>() };
                    self.0.push(Hob::Capsule(capsule_hob));
                }
                END_OF_HOB_LIST => {
                    break;
                }
                _ => {
                    self.0.push(Hob::Misc(current_header.r#type));
                }
            }
            let next_hob = hob_header as usize + current_header.length as usize;
            hob_header = next_hob as *const header::Hob;
        }
    }
}

impl<'a> IntoIterator for HobList<'a> {
    type Item = Hob<'a>;
    type IntoIter = <Vec<Hob<'a>> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl fmt::Debug for HobList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for hob in self.0.clone().into_iter() {
            match hob {
                Hob::Handoff(hob) => {
                    write!(f, indoc! {"
                        PHASE HANDOFF INFORMATION TABLE (PHIT) HOB
                          HOB Length: 0x{:x}
                          Version: 0x{:x}
                          Boot Mode: 0x{:x}
                          Memory Bottom: 0x{:x}
                          Memory Top: 0x{:x}
                          Free Memory Bottom: 0x{:x}
                          Free Memory Top: 0x{:x}
                          End of HOB List: 0x{:x}\n"},
                        hob.header.length,
                        hob.header.length,
                        hob.version,
                        hob.boot_mode,
                        align_up(hob.memory_bottom, 0x1000),
                        align_down(hob.memory_top, 0x1000),
                        align_up(hob.free_memory_bottom, 0x1000),
                        align_down(hob.free_memory_top, 0x1000))?;
                },
                Hob::MemoryAllocation(hob) => {
                    write!(f, indoc! {"
                        MEMORY ALLOCATION HOB
                          HOB Length: 0x{:x}
                          Memory Base Address: 0x{:x}
                          Memory Length: 0x{:x}
                          Memory Type: {:?}\n"},
                        hob.header.length,
                        hob.alloc_descriptor.memory_base_address,
                        hob.alloc_descriptor.memory_length,
                        hob.alloc_descriptor.memory_type)?;
                },
                Hob::ResourceDescriptor(hob) => {
                    write!(f, indoc! {"
                        RESOURCE DESCRIPTOR HOB
                          HOB Length: 0x{:x}
                          Resource Type: 0x{:x}
                          Resource Attribute Type: 0x{:x}
                          Resource Start Address: 0x{:x}
                          Resource Length: 0x{:x}\n"},
                        hob.header.length,
                        hob.resource_type,
                        hob.resource_attribute,
                        hob.physical_start,
                        hob.resource_length)?;
                },
                Hob::GuidHob(hob) => {
                    write!(f, indoc! {"
                        GUID HOB
                          HOB Length: 0x{:x}\n"},
                        hob.header.length)?;
                },
                Hob::FirmwareVolume2(hob) => {
                    write!(f, indoc! {"
                        FIRMWARE VOLUME 2 (FV2) HOB
                          Base Address: 0x{:x}
                          Length: 0x{:x}\n"},
                        hob.base_address,
                        hob.length)?;
                }
                Hob::FirmwareVolume3(hob) => {
                    write!(f, indoc! {"
                        FIRMWARE VOLUME 3 (FV3) HOB
                          Base Address: 0x{:x}
                          Length: 0x{:x}\n"},
                        hob.base_address,
                        hob.length)?;
                },
                Hob::Cpu(hob) => {
                    write!(f, indoc! {"
                        CPU HOB
                          Memory Space Size: 0x{:x}
                          IO Space Size: 0x{:x}\n"},
                        hob.size_of_memory_space,
                        hob.size_of_io_space)?;
                },
                Hob::Capsule(hob) => {
                    write!(f, indoc! {"
                        CAPSULE HOB
                          Base Address: 0x{:x}
                          Length: 0x{:x}\n"},
                        hob.base_address,
                        hob.length)?;
                }
                _ => ()
            }
        }
        write!(f, "Parsed HOBs")
    }
}

// EFI_HOB_MEMORY_ALLOCATION
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MemoryAllocation {
    pub header: header::Hob,                        // EFI_HOB_GENERIC_HEADER
    pub alloc_descriptor: header::MemoryAllocation,   // EFI_HOB_MEMORY_ALLOCATION_HEADER
                                                    // Additional data pertaining to the "Name" GUID memory may go here.
}

// EFI_HOB_MEMORY_ALLOCATION_STACK
pub type MemoryAllocationStack = MemoryAllocation;

// EFI_HOB_MEMORY_ALLOCATION_BSP_STORE
pub type MemoryAllocationBspStore = MemoryAllocation;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MemoryAllocationModule {
    pub header: header::Hob,                        // EFI_HOB_GENERIC_HEADER
    pub alloc_descriptor: header::MemoryAllocation, // EFI_HOB_MEMORY_ALLOCATION_HEADER
    pub module_name: r_efi::base::Guid,           // EFI_GUID
    pub entry_point: u64,                         // EFI_PHYSICAL_ADDRESS
}

// EFI_HOB_RESOURCE_DESCRIPTOR
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ResourceDescriptor {
    pub header: header::Hob,        // EFI_HOB_GENERIC_HEADER
    pub owner: r_efi::base::Guid, // EFI_GUID
    pub resource_type: u32,       // EFI_RESOURCE_TYPE
    pub resource_attribute: u32,  // EFI_RESOURCE_ATTRIBUTE_TYPE
    pub physical_start: u64,      // EFI_PHYSICAL_ADDRESS
    pub resource_length: u64,     // UINT64
}

// EFI_HOB_GUID_TYPE
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GuidHob {
    pub header: header::Hob, // EFI_HOB_GENERIC_HEADER
    pub name: r_efi::base::Guid, // EFI_GUID
                           // Data follows the HOB
}

// EFI_HOB_FIRMWARE_VOLUME
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FirmwareVolume {
    pub header: header::Hob, // EFI_HOB_GENERIC_HEADER
    pub base_address: u64, // EFI_PHYSICAL_ADDRESS
    pub length: u64,       // UINT64
}

// EFI_HOB_FIRMWARE_VOLUME2
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FirmwareVolume2 {
    pub header: header::Hob,            // EFI_HOB_GENERIC_HEADER
    pub base_address: u64,            // EFI_PHYSICAL_ADDRESS
    pub length: u64,                  // UIN64
    pub fv_name: r_efi::base::Guid,   // EFI_GUID
    pub file_name: r_efi::base::Guid, // EFI_GUID
}

// EFI_HOB_FIRMWARE_VOLUME3
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FirmwareVolume3 {
    pub header: header::Hob,                 // EFI_HOB_GENERIC_HEADER
    pub base_address: u64,                 // EFI_PHYSICAL_ADDRESS
    pub length: u64,                       // UINT64
    pub authentication_status: u32,        // UINT32
    pub extracted_fv: r_efi::efi::Boolean, // BOOLEAN
    pub fv_name: r_efi::base::Guid,        // EFI_GUID
    pub file_name: r_efi::base::Guid,      // EFI_GUID
}

// EFI_HOB_CPU
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Cpu {
    pub header: header::Hob,        // EFI_HOB_GENERIC_HEADER
    pub size_of_memory_space: u8, // UINT8
    pub size_of_io_space: u8,     // UINT8
    pub reserved: [u8; 6],        // UINT8[6]
}

// EFI_HOB_CAPSULE
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Capsule {
    pub header: header::Hob, // EFI_HOB_GENERIC_HEADER
    pub base_address: u8,  // EFI_PHYSICAL_ADDRESS
    pub length: u8,        // UINT64
}
