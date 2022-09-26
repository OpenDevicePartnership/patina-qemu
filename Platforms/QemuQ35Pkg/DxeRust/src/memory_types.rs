use num_traits::PrimInt;

// EFI_MEMORY_TYPE
// MU_BASECORE/MdePkg/Include/Uefi/UefiMultiPhase.h
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub enum EfiMemoryType {
    ReservedMemoryType,
    LoaderCode,
    LoaderData,
    BootServicesCode,
    BootServicesData,
    RuntimeServicesCode,
    RuntimeServicesData,
    ConventionalMemory,
    UnusableMemory,
    AcpiReclaimMemory,
    AcpiMemoryNvs,
    MemoryMappedIO,
    MemoryMappedIOPortSpace,
    PalCode,
    PersistentMemory,
}

impl<T: PrimInt> From<T> for EfiMemoryType {
    fn from(val: T) -> Self {
        use EfiMemoryType::*;

        match val.to_u8() {
            Some(0) => ReservedMemoryType,
            Some(1) => LoaderCode,
            Some(2) => LoaderData,
            Some(3) => BootServicesCode,
            Some(4) => BootServicesData,
            Some(5) => RuntimeServicesCode,
            Some(6) => RuntimeServicesData,
            Some(7) => ConventionalMemory,
            Some(8) => UnusableMemory,
            Some(9) => AcpiReclaimMemory,
            Some(10) => AcpiMemoryNvs,
            Some(11) => MemoryMappedIO,
            Some(12) => MemoryMappedIOPortSpace,
            Some(13) => PalCode,
            Some(14) => PersistentMemory,
            _ => panic!("Invalid memory type")
        }
    }
}
