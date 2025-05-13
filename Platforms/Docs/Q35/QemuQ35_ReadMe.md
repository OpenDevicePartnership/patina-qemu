# QemuQ35Pkg

**QemuQ35Pkg...**

- Is a derivative of OvmfPkg.
- Will not support Legacy BIOS or CSM.
- WIll not support S3 sleep functionality.
- Has a 32-bit PEI phase and a 64-bit DXE phase.
- Seeks to enable a tightly constrained virtual platform based on the QEMU Q35 machine type.

By solely focusing on the Q35 chipset, this package can be optimized such that it is allowed to break compatibility
with other QEMU supported chipsets. The Q35 chipset can be paired with an IA32 or X64 processor to enable a machine
that can emulate PC class hardware with industry standard features like SMM and PCI-E.

## Table of Contents

- [QemuQ35Pkg](#qemuq35pkg)
  - [Q35 Platform](#q35-platform)
  - [Compiling and Running QEMU](#compiling-and-running-qemu)

## Q35 Platform

Q35 is a machine type that QEMU emulates.
Below is a diagram from Qemu.org about the Q35 chipset which emulates a ICH9 (I/O controller hub).

![Q35 ICH9](https://wiki.qemu.org/images/4/46/QEMU-ICH9.png)

The advantages of the ICH9 over the I440FX (which is what QEMU often emulates) is that it has PCI-E instead of just PCI
as well as having an integrated AHCI controller and no ISA bus.

Visit the feature wiki detailing QEMU Q35 for more information: <https://wiki.qemu.org/Features/Q35>

## Compiling and Running QEMU

QemuQ35Pkg uses the Patina repositories and EDK II PyTools for its build operations.
Specific details can be found here [Development/building.md](../Common/building.md)

