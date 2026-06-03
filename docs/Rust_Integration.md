# High-Level Overview of Rust Integration

At a high level there are two basic approaches to integrate Rust code into a UEFI build process:

1. Build the code using Rust tools in a stand-alone workspace to produce a .efi binary that is later integrated into the
Firmware Device (FD) image
2. Add support to the EDK II build infrastructure to compile the Rust source code alongside the C source code when processing
each module specified in a DSC file

The Patina project focuses on the first option of pre-compiling Rust based modules to produce .efi binaries, then adding
the file paths to the platform .fdf file to be ingested when creating the firmware volumes and final firmware device file.

The main reason for this choice was simplicity of understanding and upkeep.  Currently the [EDK II](https://github.com/tianocore/edk2)
build environment does not support compiling Rust drivers without the end user updating and maintaining the build rules
to handle the Rust toolchain.  This coupling of the EDK II build system with the Rust/Cargo build system introduced complexity
that can be overcome, but proved too complicated to provide a single sample solution to cover all (or most) scenarios in
a concise manor.  In addition, the complexity from co-integrating the Rust/Cargo build system with the EDK II build system
lead to Rust source code being maintained in the C codebase which is not ideal due to language and tooling differences.

## Stand-Alone Build Integration

The [Patina DXE Core QEMU](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) repository provides a sample
.efi driver that is ingested by this repository using integration option 1.  It contains code to build a fully Rust based
DXE core .efi driver, targeted toward either the Arm Virt or Q35 platform, and its Continuous Integration GitHub action publishes
the debug and release .efi drivers to a Nuget feed that this repository can then use during its build.

This repository, [Patina QEMU UEFI](https://github.com/), has its own Continuous Integration GitHub action to pull the nuget
feed and extract the binary to be used in its build process.  The platform .fdf file points to where the binary was extracted
and the firmware device linking process places the Patina DXE Core .efi binary into a firmware volume inside the final FD file.

To replace the DXE Core .efi file with a new user built binary, both the platform
[QemuQ35Pkg.fdf](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/QemuQ35Pkg/QemuQ35Pkg.fdf)
and [QemuArmVirtPkg.fdf](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/QemuArmVirtPkg/QemuArmVirtPkg.fdf)
files in this repository have a `[FV.RUST_DXE_CORE]` firmware volume definition section that contains a single driver,
the Patina DXE Core QEMU .efi binary. There are several `#if` statements in the file to support multiple options, but the
easiest method to inject a user built binary is to update the `FILE DXE_CORE` section to point directly to the newly built
.efi binary and re-compile the UEFI.

```text
  FILE DXE_CORE = 23C9322F-2AF2-476A-BC4C-26BC88266C71 {
    SECTION PE32 = "< new dxe core file path >"
    SECTION UI = "DxeCore"
  }
```

## Stand-Alone Build Options

The reason the `FILE DXE_CORE` section contains the `#if` statements is to support selecting either the debug or release
flavor of the binary from the nuget feed and to support using a command line parameter to define a newly created .efi binary.
By setting the `DXE_CORE_BINARY_OVERRIDE` define to the new binary's file path when running Stuart, the nuget feed binary
will be overriden without having to modify the .fdf file.

```cmd
  stuart_build -c Platforms\QemuQ35Pkg\PlatformBuild.py --flashrom BLD_*_DXE_CORE_BINARY_OVERRIDE="<new dxe core file path>"
```

Or if multiple iterations of replacement and testing are needed, the Patina DXE Core binary was placed in its own firmware
volume in the .fdf file to allow supporting the [Patina FW Patcher](https://github.com/OpenDevicePartnership/patina-fw-patcher).
The [build_and_run_rust_binary.py](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/build_and_run_rust_binary.py)
script in the root of this repository can perform all steps necessary to compile the Patina DXE core driver, call the patcher,
and start QEMU.  For more details, run it with the `--help` command line parameter:

```cmd
  python build_and_run_rust_binary.py --help
```
