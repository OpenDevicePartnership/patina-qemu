# Introduction

**Welcome to the Patina QEMU Platforms book.** This book documents the two virtual platforms maintained in the
[`patina-qemu`](https://github.com/OpenDevicePartnership/patina-qemu) repository and the workflows used to build,
debug, and validate them.

The primary purpose of `patina-qemu` is to serve as a test vehicle for the [Patina](https://github.com/OpenDevicePartnership/patina)
project. It can also be used as a reference for a simple integration of Patina with EDK II components. The repository
contains a permanent fork of [`OvmfPkg`](https://github.com/tianocore/edk2/tree/HEAD/OvmfPkg) from EDK II adapted for
Patina.

**Platforms**:

- [QEMU Q35](./platforms/q35.md) - an x86_64 platform based on the Q35 chipset.
- [QEMU ARM Virt](./platforms/armvirt.md) - an AArch64 platform based on the Arm Server Base System Architecture.

## What is in this book

This book is organized into the following sections:

1. **[Building and Running](building/building.md)** - Setting up your environment, compiling the firmware, running it
   in QEMU, and the rapid Patina iteration workflow used by most Patina developers.
2. **[Debugging](debugging/windbg_uefi.md)** - Connecting WinDbg to UEFI firmware running in QEMU and to a Windows
   guest OS booted on top of it.
3. **[Testing](testing/regression_testing.md)** - Scheduled regression tests run against the default branch.
4. **[Performance](performance/results.md)** - Collecting and interpreting boot performance data, and the size of the
   Patina DXE Core release binary.

## Related Documentation

- [Open Device Partnership (ODP)](https://opendevicepartnership.org/)
- [Patina project documentation](https://opendevicepartnership.github.io/patina/) - the main Patina book
- [Patina GitHub organization](https://github.com/OpenDevicePartnership)
- [Patina DXE Core QEMU](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) - the Patina DXE Core
  binary crate consumed by this repository
