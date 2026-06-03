# QEMU Arm Virt

`QemuArmVirtPkg`...

- Is a derivative of [ArmVirtPkg](https://github.com/tianocore/edk2/tree/master/ArmVirtPkg) based on the QEMU ARM-Virt machine type.
- Will not support Legacy BIOS or CSM.
- Will not support S3 sleep functionality.
- Has 64-bit SEC and DXE phases.
- Targets a tightly constrained virtual platform based on the QEMU ARM CPUs.

By focusing solely on the ARM chipset, this package is allowed to break compatibility with other QEMU supported
chipsets. The ARM chipset can be paired with an AArch64 processor to emulate ARM-based hardware with industry standard
features like TrustZone and PCI-E.

## Building and Running

`QemuArmVirtPkg` uses the Patina repositories and EDK II PyTools for its build operations. See
[Building the Firmware](../building/building.md) for full instructions.
