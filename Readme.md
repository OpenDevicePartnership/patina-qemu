# Patina Development Vehicle

[![CI](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/platform-ci.yml/badge.svg)](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/platform-ci.yml)
[![Nightly Regression Tests](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/nightly-os-boot.yml/badge.svg?event=schedule)](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/nightly-os-boot.yml)

The primary purpose of this repository is to serve as a test vehicle for the Patina project. It can be used as a
reference for a simple integration of Patina with EDK II components. It contains a permanent fork of
[OvmfPkg](https://github.com/tianocore/edk2/tree/HEAD/OvmfPkg) from EDK II with changes based on the following:

- Documentation
  - [Open Device Partnership (ODP) documentation](https://opendevicepartnership.org/)
  - [Patina project documentation](https://opendevicepartnership.github.io/patina/)
- GitHub Links
  - [ODP GitHub organization](https://github.com/OpenDevicePartnership)
  - [Patina GitHub repository](https://github.com/OpenDevicePartnership/patina)
  - [Patina DXE Core QEMU repository](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) showcasing the
    Patina DXE core .efi binary used by this repository

## Compiling this Repository

There are two platforms currently supported in this repository: Q35 and SBSA. They have build documentation
listed in their READMEs:

- [QEMU Q35](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/Docs/Q35/QemuQ35_ReadMe.md)
- [QEMU SBSA](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/Docs/SBSA/QemuSbsa_ReadMe.md)

## Platform Validation Testing

This repository contains scheduled github workflows to detect regressions in firmware. See
[Platforms/Docs/Common/regression-testing.md](https://github.com/OpenDevicePartnership/patina-qemu/tree/main/Platforms/Docs/Common/regression-testing.md).
