# Patina Platform Repository

This repository demonstrates how to integrate [Patina](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/patina.md)
into a UEFI platform build.

## Repository Background

This repository is a permanent fork of [mu_tiano_platforms](https://github.com/OpenDevicePartnership/patina-qemu) which is
itself a permanent fork of the [OvmfPkg in edk2](https://github.com/tianocore/edk2/tree/HEAD/OvmfPkg). The reason for
a permanent fork is to allow this repository to be independently used within the ownership of the Patina project and,
in doing so, optimized for Rust UEFI development.

## Getting Started

This repository is meant to be a "first stop" for developers exploring Patina in practice - how it would be integrated
in a UEFI platform. Depending on your background, you may find the following entry points to Patina useful:

- All:
  - [Patina Overview](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/patina.md)
- Interested in the Patina DXE Core?
  - [High-Level Differences of Patina DXE Core vs EDK II DXE Core](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/integrate/rust_vs_edk2.md)
- Developers:
  - [Developer Introduction](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/introduction.md)
  - [Patina Code Organization](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/dev/code_organization.md)
  - [The Patina Component Model](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/dxe_core/component_model.md)
  - [General Rust Resources](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/dev/other.md)

## High-Level Overview of Rust Integration Into Platforms

As Rust is integrated into more production platforms, it is important to understand the options available and where
this repository fits in. Recognizing that UEFI inherently supports dynamic integration, at the highest level, there are
two approaches for writing UEFI firmware in Rust:

1. Build the code using Rust tools in a Pure Rust workspace against the UEFI target triple and integrate the resulting
   .efi binary into the platform firmware build process.
   - This is the primary approach taken in this repository.
2. Support for building Rust source code in the firmware build process. If using the EDK II build process, this means
   compiling Rust source code alongside the C source code when building code specified in a DSC file.

We generally recommend **(1)** as the preferred approach for the following reasons:

- It allows for a more natural Rust development experience directly using Rust tools and processes for Rust code.
- It greatly simplifies the firmware build process as it does not need to be enlightened for building Rust code.

This repository integrates Patina code using **(1)** and demonstrates how **(2)** can be used for Rust code
external to Patina to provide a complete example of how to integrate Rust into a UEFI platform build.

In the diagram below, **(1)** is ".efi binary" and **(2)** is either of the other blocks in the diagram depending on
which method is selected.

Ease of integration decreases when not directly using a .efi binary because the EDK II build process must be updated
to support build rules for Rust code. The coupling of the EDK II build system with the Rust/Cargo build system
introduces additional complexity alongside EDK II workspace requirements causing friction with Rust workspace
conventions. EDK II customizations such as PCDs are not natively supported in Rust. Conversely, Rust feature flags
are not supported in the EDK II build process. This further increases integration complexity as consumers must
understand multiple types of customization and how they may cooperate with each other. When Pure Rust code is built
into a .efi binary in **(1)**, that best ensures consumers they are using a "Rust implementation". It is possible that
Rust build may have had C code linked into the binary with a FFI, but that is not a practice in Patina.

![Rust Integration Options](./docs/images/rust_integration_options.png)

Since **(1)** is the preferred approach, we will focus on that in this document. However, it is important to note that
**(2)** is also a valid approach and can be used in conjunction with **(1)**. More details on **(2)** are provided in
the collapsed section below if you are interested in understanding more about that approach.

<details>
<summary>Click to expand for more details on <b>(2)</b></summary>
As of today, [tianocore/edk2](https://github.com/tianocore/edk2) does not support **(2)**. The [Patina](https://github.com/OpenDevicePartnership/patina?tab=readme-ov-file#patina)
open-source project does.

**(2)** is particularly useful for linking Rust code with C code in a given module. However, several combinations are
possible with today's support:

- **C source** + **Rust source mixed in INF** (Library or Module)
  - Rust source code is recognized and supported by an EDK II build rule – Rust-To-Lib-File (.rs => .lib)
- **Pure Rust Module only**.
  - A Cargo.toml file is added to INF file as source.
  - Rust Module build is supported by EDK II build rule – Toml-File.RUST_MODULE (.toml => .efi)
- **Pure Rust Module** + **Pure Rust Library with Cargo Dependency**.
  - The cargo dependency means the rust lib dependency declared in Cargo.toml.
- **Pure Rust Module** + **C Library with EDK II Dependency**.
  - Rust Module build is supported by EDK II build rule – Toml-File (.toml => .lib)
  - The EDK II dependency means the EDK II lib dependency declared in INF.
    - If a rust module is built with C, the cargo must use `staticlib`. Or, `rlib` should be used.
- **C Module** + **Pure Rust Library with EDK II Dependency**.
  - Rust Lib build is supported by EDK II build rule – Toml-File. (.toml => .lib)
- **Pure Rust Module** + **Pure Rust Library with EDK II Dependency**.
  - Same as combining others from above.

After experimenting with **(2)**, we have found that while it  is how most projects initially consider integrating Rust
into their codebase but in practice the integration complexity is high due to the ability to cointegrate the Rust/Carg
build system with the EDK II build system and it naturally leads to Rust source code being maintained in the C codebase
which is not ideal due to language and tooling differences.
</details>

## First-Time Setup Instructions For This Repository

There are two platforms currently supported in this repository - `QemuQ35Pkg` and `QemuSbsaPkg`.

- [QemuQ35Pkg](https://github.com/OpenDevicePartnership/patina-qemu/tree/main/Platforms/QemuQ35Pkg)
  - Intel Q35 chipset with ICH9 south bridge and AMD Cpu
  - This demonstrates x86/x64 UEFI firmware development with Patina.
  - [QemuQ35Pkg Detailed Info](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/Docs/Q35/QemuQ35_ReadMe.md)
- [QemuSbsaPkg](https://github.com/OpenDevicePartnership/patina-qemu/tree/main/Platforms/QemuSbsaPkg)
  - ARM Server Base System Architecture
  - This demonstrates AARCH64 UEFI firmware development with Patina.
  - [QemuSbsaPkg Detailed Info](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/Docs/SBSA/QemuSbsa_ReadMe.md)

  **Supported Host Platforms and Target Architectures**

  | Host Platform | Target Architectures Supported |
  | ------------- | ------------------------------ |
  | Windows       | x64, AArch64                   |
  | WSL           | x64, AArch64                   |
  | Linux         | x64, AArch64                   |

### Option 1: Automated Guided Setup Script - workspace_setup.py

If this is your first time using this repository and you're not familiar with the build process, it is recommended
that you start with `QemuQ35Pkg` and use the workspace setup wizard to get started. You will need to install Python
([Python downloads](https://www.python.org/downloads/)), and then run the following command to start the setup
process:

Linux:

```bash
>python workspace_setup.py
```

Windows:

```powershell
>py -3 workspace_setup.py
```

Using this script, you will be guided through the process of setting up the workspace and installing the required
dependencies. The manual steps are described below.

<!-- markdownlint-disable MD033
MD033: Allow inline HTML for styling elements not easily handled by Markdown.-->
### Option 2: Manual Setup

This is a more detailed, step-by-step manual setup process.

#### Prerequisites

One-time tools and packages required to set up Patina development.

<details>
<summary><b> 🪟 Windows 11 - 24H2 </b></summary>

| Tool                                                                                                                                  | Install Command                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [Chocolatey](https://chocolatey.org/)                                                                                                 | `winget install --id Chocolatey.Chocolatey -e`                                                                                                                                                                                                                                                                                                                                                                                                      |
| [Python 3](https://www.python.org/)                                                                                                   | `winget install --id Python.Python.3.12 -e` <br> **Note:** Disable any app execution alias defined for `python.exe` and `python3.exe` from Windows settings(Apps > Advanced app settings > App execution alias)                                                                                                                                                                                                                                     |
| [Git](https://git-scm.com/)                                                                                                           | `winget install --id Git.Git -e`                                                                                                                                                                                                                                                                                                                                                                                                                    |
| [Rust](https://rustup.rs/)                                                                                                            | `winget install --id Rustlang.Rustup -e` <ol><li> **Add x86_64 uefi target:** `rustup target add x86_64-unknown-uefi` </li><li> **Add aarch64 uefi target:** `rustup target add aarch64-unknown-uefi`</li><li>**Install cargo make:** `cargo install cargo-make`</li><li>**Install cargo tarpaulin:** `cargo install cargo-tarpaulin`</li></ol>                                                                                                     |
| [LLVM](https://llvm.org/)                                                                                                             | `winget install --id LLVM.LLVM -e --override "/S /D=C:\LLVM"` <ul><li>**Note:** `/D=C:\LLVM` override(with no spaces) is needed for AArch64 build of `patina-qemu` repo on Windows.</li></ul>                                                                                                                                                                                                                                                       |
| [GNU Make](https://community.chocolatey.org/packages/make)                                                                            | `choco  install make` <ul><li>**Note:** Needed for AArch64 build of `patina-qemu` repo on Windows.</li></ul>                                                                                                                                                                                                                                                                                                                                        |
| [MSVC BuildTools](https://rust-lang.github.io/rustup/installation/windows-msvc.html#installing-only-the-required-components-optional) | `winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Component.VC.CoreBuildTools --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64  --add Microsoft.VisualStudio.Component.Windows11SDK.22621 --add Microsoft.VisualStudio.Component.VC.Tools.ARM  --add Microsoft.VisualStudio.Component.VC.Tools.ARM64"` <br> **Note:** Only required when building for `std` |
| [Node](https://nodejs.org/en)                                                                                                         | `winget install --id OpenJS.NodeJS.LTS -e` <ol><li> **Add cspell:** `npm install -g cspell@latest` </li><li> **Add markdown lint cli:** `npm install -g markdownlint-cli` </li></ol>                                                                                                                                                                                                                                                                |
| [QEMU](https://www.qemu.org/)                                                                                                         | `winget install --id SoftwareFreedomConservancy.QEMU -e -v 10.0.0`                                                                                                                                                                                                                                                                                                                                                                                  |
| **Optional**                                                                                                                          |                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| [WinDBG](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/)                                                        | `winget install --id Microsoft.WinDbg -e`                                                                                                                                                                                                                                                                                                                                                                                                           |
| [VSCode](https://code.visualstudio.com/)                                                                                              | `winget install --id Microsoft.VisualStudioCode -e`                                                                                                                                                                                                                                                                                                                                                                                                 |
| [Rust Analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)                                          | `code --install-extension rust-lang.rust-analyzer`                                                                                                                                                                                                                                                                                                                                                                                                  |

**Note:** Add the LLVM bin directory (`C:\LLVM\bin`) and the QEMU bin directory
(`C:\Program Files\qemu`) to the `PATH` environment variable.
</details>

<details>
<summary><b> 🐧 Linux/WSL - Ubuntu 24.04 LTS - Bash </b></summary>

| Tool                                                                                            | Install Command                                                                                                                                                                                                                                                                                                                                                                                                          |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Build Essentials                                                                                | `sudo apt update && sudo apt install -y build-essential git nasm m4 bison flex curl wget uuid-dev python3 python3-venv python-is-python3 unzip acpica-tools gcc-multilib mono-complete pkg-config libssl-dev mtools`                                                                                                                                                                                                     |
| [Rust](https://rustup.rs/)                                                                      | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` <br>**Note:** Might have to reopen the terminal <ol><li> **Add x86_64 uefi target:** `rustup target add x86_64-unknown-uefi` </li><li> **Add aarch64 uefi target:** `rustup target add aarch64-unknown-uefi`</li><li>**Install cargo make:** `cargo install cargo-make`</li><li>**Install cargo tarpaulin:** `cargo install cargo-tarpaulin`</li></ol> |
| [Node](https://nodejs.org/en)                                                                   | `curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh \| bash` <br>`source ~/.bashrc`<br>`nvm install --lts` <ol><li> **Add cspell:** `npm install -g cspell@latest` </li><li> **Add markdown lint cli:** `npm install -g markdownlint-cli` </li></ol>                                                                                                                                               |
| [QEMU](https://www.qemu.org/)                                                                   | `sudo apt install -y qemu-system`                                                                                                                                                                                                                                                                                                                                                                                        |
| [LLVM](https://llvm.org/)                                                                       | `sudo apt install -y clang llvm lld`                                                                                                                                                                                                                                                                                                                                                                                     |
| **Optional**                                                                                    |                                                                                                                                                                                                                                                                                                                                                                                                                          |
| [VSCode](https://code.visualstudio.com/docs/setup/linux#_debian-and-ubuntu-based-distributions) | `wget https://go.microsoft.com/fwlink/?LinkID=760868 -O code.deb` <br> `sudo apt install ./code.deb`                                                                                                                                                                                                                                                                                                                     |
| [Rust Analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)    | `code --install-extension rust-lang.rust-analyzer`                                                                                                                                                                                                                                                                                                                                                                       |

</details>

#### Code

| Repo                                                                                   | Clone                                                                     | About                                                                            |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| [patina](https://github.com/OpenDevicePartnership/patina/)                             | `git clone https://github.com/OpenDevicePartnership/patina`               | Patina Firmware. Contains all crates published to [crates.io](https://crates.io) |
| [patina-qemu](https://github.com/OpenDevicePartnership/patina-qemu/)                   | `git clone https://github.com/OpenDevicePartnership/patina-qemu`          | Repository to produce Patina firmware image for QEMU                             |
| [patina-dxe-core-qemu](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/) | `git clone https://github.com/OpenDevicePartnership/patina-dxe-core-qemu` | Repository to produce Patina DXE Core Binary for QEMU                            |

**Note:** Prefer short paths on Windows(`C:\r\`) or Linux(`/home/<username>/r/`)

#### Build and Run

<details>
<summary><b> 🖥️ X64 Target </b></summary>

| Repo                                                                                   | Build Instructions                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [patina-dxe-core-qemu](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/) | `cd <patina-dxe-core-qemu>` <br><br> **Build dxe core efi binary:** <br>`cargo make q35`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| [patina-qemu](https://github.com/OpenDevicePartnership/patina-qemu/)                   | `cd <patina-qemu>` <br><br> **Setup and Activate Virtual Env:** <br> `python -m venv q35env` <br> 🪟 `q35env\Scripts\activate.bat` <br> 🐧 `source q35env/bin/activate` <br><br> **Build Perquisites:** <br>`pip install --upgrade -r pip-requirements.txt` <br><br> **Stuart Setup:** <br>`stuart_setup  -c Platforms/QemuQ35Pkg/PlatformBuild.py` <br> <br>**Stuart Update:** <br>`stuart_update -c Platforms/QemuQ35Pkg/PlatformBuild.py` <br>**Note:** Retry the command if failed with `Filename too long` error <br><br> **Stuart Build and Launch Uefi Shell:** <br>🪟 `stuart_build  -c Platforms/QemuQ35Pkg/PlatformBuild.py --flashrom` <br>🐧 `stuart_build  -c Platforms/QemuQ35Pkg/PlatformBuild.py TOOL_CHAIN_TAG=CLANGPDB --flashrom` <br><br> **Stuart Build and Launch OS(Optional):** <br>🪟 `stuart_build  -c Platforms/QemuQ35Pkg/PlatformBuild.py --flashrom PATH_TO_OS="C:\OS\Windows11.qcow2"` <br>🐧 `stuart_build  -c Platforms/QemuQ35Pkg/PlatformBuild.py TOOL_CHAIN_TAG=CLANGPDB --flashrom PATH_TO_OS="$HOME/OS/Windows11.qcow2"` |
| [patina](https://github.com/OpenDevicePartnership/patina/)                             | No need to build this(except for local development). Crates from this repo are consumed directly from [crates.io](https://crates.io) by `patina-dxe-core-qemu` repo                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |

**Note:** Please note that the paths `C:\r\patina-dxe-core-qemu` and `$HOME/r/patina-dxe-core-qemu`
in `C:\r\patina-dxe-core-qemu\target\x86_64-unknown-uefi` and
`$HOME/r/patina-dxe-core-qemu/target/x86_64-unknown-uefi`, respectively, are user-specific and
depend on the location where the repositories are cloned.

</details>

<details>
<summary><b> 📱 AArch64 Target </b></summary>

| Repo                                                                                   | Build Instructions                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [patina-dxe-core-qemu](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/) | `cd <patina-dxe-core-qemu>` <br><br> **Build dxe core efi binary:** <br>`cargo make sbsa`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| [patina-qemu](https://github.com/OpenDevicePartnership/patina-qemu/)                   | `cd <patina-qemu>` <br><br> **Setup and Activate Virtual Env:** <br> `python -m venv sbsaenv` <br> 🪟 `sbsaenv\Scripts\activate.bat` <br> 🐧 `source sbsaenv/bin/activate` <br><br> **Build Perquisites:** <br>`pip install --upgrade -r pip-requirements.txt` <br><br> **Stuart Setup:** <br>`stuart_setup  -c Platforms/QemuSbsaPkg/PlatformBuild.py` <br> <br>**Stuart Update:** <br>`stuart_update -c Platforms/QemuSbsaPkg/PlatformBuild.py` <br><br> **Stuart Build and Launch Uefi Shell:** <br>🪟 `stuart_build  -c Platforms/QemuSbsaPkg/PlatformBuild.py TOOL_CHAIN_TAG=CLANGPDB --flashrom` <br>🐧 `stuart_build  -c Platforms/QemuSbsaPkg/PlatformBuild.py TOOL_CHAIN_TAG=CLANGPDB --flashrom` <br><br> **Stuart Build and Launch OS(Optional):** <br>🪟 `stuart_build  -c Platforms/QemuSbsaPkg/PlatformBuild.py TOOL_CHAIN_TAG=CLANGPDB --flashrom PATH_TO_OS="C:\OS\Windows11.qcow2"` <br>🐧 `stuart_build  -c Platforms/QemuSbsaPkg/PlatformBuild.py TOOL_CHAIN_TAG=CLANGPDB --flashrom PATH_TO_OS="$HOME/OS/Windows11.qcow2"` |
| [patina](https://github.com/OpenDevicePartnership/patina/)                             | No need to build this(except for local development). Crates from this repo are consumed directly from [crates.io](https://crates.io) by `patina-dxe-core-qemu` repo                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |

**Note:** Please note that the paths `C:\r\patina-dxe-core-qemu` and `$HOME/r/patina-dxe-core-qemu`
in `C:\r\patina-dxe-core-qemu\target\x86_64-unknown-uefi` and
`$HOME/r/patina-dxe-core-qemu/target/x86_64-unknown-uefi`, respectively, are user-specific and
depend on the location where the repositories are cloned.

</details>

#### Local Development

The above steps will help you build and test the vanilla code, with dependencies fetched from
[crates.io](https://crates.io). For local development, you should modify the relevant crates within
the `patina` repository and update the dependencies using appropriate local path.

#### Debugging

- [WinDbg + QEMU + Patina UEFI - Debugging Guide](Platforms/Docs/Common/windbg-qemu-uefi-debugging.md)
- [WinDbg + QEMU + Patina UEFI + Windows OS - Debugging Guide](Platforms/Docs/Common/windbg-qemu-windows-debugging.md)

<!-- markdownlint-enable MD033 -->

## Advanced Usage

### Insert a new DXE Core Driver into the Build

This repository was originally created to demonstrate using Patina modules with an emphasis on ingesting the [Patina
DXE Core](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu).  To modify the build to consume a new DXE
Core binary instead of the pre-built .EFI file from the nuget feed, there are several methods supported.

#### Update the Platform FDF File

The easiest way to inject a new Patina DXE Core driver is to update the platform FDF (`/Platforms/QemuQ35Pkg/QemuQ35Pkg.fdf`
or `/Platforms/QemuQ35Pkg/QemuQ35Pkg.fdf`) file to point to the new file as typically done in UEFI builds that ingest
pre-compiled binaries.  Modify the `SECTION` definition in the `DXE_CORE` file declaration as follows:

```cmd
FILE DXE_CORE = 23C9322F-2AF2-476A-BC4C-26BC88266C71 {
  SECTION PE32 = "<new dxe core file path>"
  SECTION UI = "DxeCore"
}
```

This repository's platform FDF files do support defining a build variable to override the default binary without needing
to modify the FDF file.  This can be set from the stuart_build command line by defining `BLD_*_DXE_CORE_BINARY_PATH':

```cmd
stuart_build -c Platforms\QemuQ35Pkg\PlatformBuild.py --flashonly BLD_*_DXE_CORE_BINARY_PATH="<new dxe core file path>"
```

#### Patching a Pre-Built UEFI Firmware Device Image

If multiple iterations of the DXE core are to be tested, the fastest way to integrate each to a bootable FD image is
using the [Patina FW Patcher](https://github.com/OpenDevicePartnership/patina-fw-patcher). This tool will open an
existing UEFI FD binary, find and replace the current DXE Core driver with a new file, and launch QEMU with the patched
ROM image.

A [build_and_run_rust_binary.py](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/build_and_run_rust_binary.py)
script is provided in the root of this repository to perform all steps necessary to compile the Patina DXE core driver,
call the patcher, and start QEMU.  For more details, run it with the `--help` command line parameter:

```cmd
python build_and_run_rust_binary.py --help
```

- Note 1: This tool is not a general FW patcher to be used on any UEFI FD image.  It relies on specific features
  implemented in this UEFI build.
- Note 2: Because this tool is patching an existing QEMU ROM image, only changes to the Rust DXE Core code will be
  merged.  Any changes to the C code will require running a full stuart_build process to build a new ROM image.
- Note 3: The tool can be enhanced to patch more than the Patina DXE Core.  If there is interest in new features,
  please start a discussion in the tool's repo [discussions](https://github.com/OpenDevicePartnership/patina-fw-patcher/discussions/categories/q-a)
  area.

### Using a Custom QEMU Installation

By default, this repository automates the process of choosing a known working QEMU version and downloading that version
into the workspace for you. If you want to use a custom QEMU installation, you can do so by passing the path to the
Stuart build command with the`QEMU_PATH` argument. For example:

```cmd
stuart_build -c Platforms/QemuQ35Pkg/PlatformBuild.py --flashonly QEMU_PATH="<path to qemu executable>"
```

You can also specify the directory where the QEMU binary is located by passing the `QEMU_DIR` argument. For example:

```cmd
stuart_build -c Platforms/QemuQ35Pkg/PlatformBuild.py --flashonly QEMU_DIR="<path to qemu bin directory>"
```

### Self Certification Tests

Refer to the [Self Certification Test](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/docs/SelfCertifcationTest.md)
documentation for information on how to configure and run the [Self Certification Tests (SCTs)](https://github.com/tianocore/tianocore.github.io/wiki/UEFI-SCT).
