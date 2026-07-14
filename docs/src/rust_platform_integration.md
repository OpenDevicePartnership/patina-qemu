# High-Level Overview of Rust Integration

At a high level, there are two basic approaches to integrate Rust code into a UEFI build process:

1. Build the code using Rust tools in a stand-alone workspace to produce an `.efi` binary that is later integrated
   into the Firmware Device (FD) image.
2. Add support to the EDK II build infrastructure to compile the Rust source code alongside the C source code when
   processing each module specified in a DSC file.

The Patina project uses the first option of pre-compiling Rust-based modules to produce `.efi` binaries, then adding
the file paths to the platform `.fdf` file to be ingested when creating the firmware volumes and final firmware device
file.

There are several reasons why this approach was chosen:

1. The Rust toolchain is not integrated into the EDK II build system, so it is not possible to build Rust code directly
   from the EDK II build system without adding additional complexity to the build process.
2. Rust uses Cargo as its build system, which is opinionated about how it builds, organizes files in a workspace, and
   manages dependencies. This conflicts with the EDK II build system, which is also opinionated about how it builds,
   organizes files, and manages dependencies. The two do not complement each other.
3. Rust tooling and crate management get in the way of normal C development, forcing all C developers to understand and
   manage Rust toolchains and environments just to continue building C code.
4. C toolchain management and EDK II-specific build complexities get in the way of normal Rust development, coupling
   what is otherwise a publicly well-documented and standardized Rust build process with other systems that do not
   add value to Rust development.
5. Keeping C code and Rust code in a single workspace encourages the development of Rust code that is tightly coupled
   with C code, which is directly in opposition to the Patina goal of pure Rust UEFI firmware.
6. Using a binary allows the EDK II and C build process to be skipped entirely when just editing Rust code (with
   binary patching). Since the Rust build is significantly faster than an EDK II C build, this allows Rust changes to
   be tested in seconds instead of minutes.

```admonish warning title="Building Rust directly with the EDK II build system is not recommended"
The Patina team previously made changes to the EDK II build system to support building Rust code directly and used
that model for a few years. Based on the drawbacks experienced with that approach (outlined above), the team did not
upstream those build changes to EDK II and highly discourages taking this approach.
```

## Stand-Alone Build Integration

The [Patina DXE Core QEMU](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) repository provides an
`.efi` DXE Core binary that is ingested by this repository: one binary for ArmVirt (AARCH64) and one for Q35 (X64).

The binaries are published in that repository as GitHub releases -
[patina-dxe-core-qemu Releases](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/releases).

This repository, [Patina QEMU UEFI](https://github.com/OpenDevicePartnership/patina-qemu), automatically pulls the
release version specified in
[`QemuPkg/Binaries/DXECORE.QEMU_ext_dep.json`](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/QemuPkg/Binaries/DXECORE.QEMU_ext_dep.json)
during the patina-qemu firmware build process.

The [QemuQ35Pkg.fdf](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/QemuQ35Pkg/QemuQ35Pkg.fdf)
and [QemuArmVirtPkg.fdf](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/QemuArmVirtPkg/QemuArmVirtPkg.fdf)
files both include the binary the same way, differing only in the DXE Core binary's file name (`qemu_q35_dxe_core.efi`
vs. `qemu_armvirt_dxe_core.efi`). The ArmVirt version is shown below for reference:

```text
FILE DXE_CORE = 23C9322F-2AF2-476A-BC4C-26BC88266C71 {
!ifdef $(DXE_CORE_BINARY_OVERRIDE)
  SECTION PE32 = $(DXE_CORE_BINARY_OVERRIDE)                                  # User defined override
!else
  !if $(TARGET) == RELEASE
    SECTION PE32 = $(DXE_CORE_PATH)/release/qemu_armvirt_dxe_core.efi # Nuget feed release build
  !else
    SECTION PE32 = $(DXE_CORE_PATH)/debug/qemu_armvirt_dxe_core.efi
  !endif
!endif
  SECTION UI = "DxeCore"
}
```

The `DXE_CORE_PATH` variable is automatically set by the build system to the location of the downloaded binaries.

## Stand-Alone Build Options

These `.fdf` files also support a `DXE_CORE_BINARY_OVERRIDE` build variable that can be set on the command line to
take precedence over the default binary:

```cmd
stuart_build -c Platforms\QemuQ35Pkg\PlatformBuild.py --FlashRom BLD_*_DXE_CORE_BINARY_OVERRIDE="<new dxe core file path>"
```

```admonish tip title="Faster build and testing"
Rather than rebuilding the full platform to test a new DXE Core binary, these platforms can patch Rust code directly
into a prebuilt platform firmware binary (FD file) and boot it within seconds using the `build_and_run_rust_binary.py`
script. See [Rapid Patina Iteration](building/rapid_iteration.md) for details.
```
