# QemuSbsaPkg

**QemuSbsaPkg...**

- Is another derivative of OvmfPkg based on EDK II QEMU-SBSA ARM machine type.
- Will not support Legacy BIOS or CSM.
- WIll not support S3 sleep functionality.
- Has 64-bit for both PEI and DXE phase.
- Seeks to enable a tightly constrained virtual platform based on the QEMU ARM CPUs.

By solely focusing on the ARM chipset, this package can be optimized such that it is allowed to break compatibility
with other QEMU supported chipsets. The ARM chipset can be paired with an AARCH64 processor to enable a machine
that can emulate ARM based hardware with industry standard features like TrustZone and PCI-E. Although leveraging
SBSA machine type provided by QEMU, the features enabled/included in this package will not be server class platform
centric.

## QEMU-SBSA Platform

SBSA is an ARM based machine type that QEMU emulates.

The advantages of the SBSA over the virtual ARM platform (which is what QEMU often emulates) is that it has
better ARM based platform level support (ACPI, etc.) as well as having an integrated AHCI controller.

## Compiling and Running QEMU

QemuSbsaPkg uses the Patina repositories and edk2-pytools for its build operations.
Specific details can be found here [Development/building.md](../Common/building.md)
