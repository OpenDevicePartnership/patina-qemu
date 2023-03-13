# Project Mu Rust Repository

## Important Notes

1. This repository is targeted to transition to Open Source Software (OSS) but is currently private while the initial
   content and direction materialize.
2. This project is experimental and the code within is not recommended to be used in production workloads at this time.
3. Assume that this is an open-source repo. This will make transitioning the repo with useful source history to
   open-source much easier.
   - Do not include internal:
     - Code names
     - Links
     - Road maps
   - In addition to moving the tree to open-source, not all internal engineers may have access to your links or be
     familiar with organization-specific projects and plans.
   - Sweeping changes needed to move the code public like copyrights and patching PR numbers in commit message
     subjects can be updated in batch when the code goes open so those details don't matter as much and are not likely
     to be missed.
4. Run `cargo fmt` before creating pull requests.

## Background

There have been various [instances of advocacy](https://msrc-blog.microsoft.com/2019/11/07/using-rust-in-windows/) for
building system level software in [Rust](https://www.rust-lang.org/).

This repository explores the adoption of Rust for [UEFI](https://uefi.org/) firmware. We plan to enable an incremental
migration of today's firmware components largely written in C to Rust. The primary objective for this effort is to
improve the security and stability of system firmware by leveraging the memory safety offered by Rust while
maintaining similar boot performance.

Eventually, this code is planned to become part of [Project Mu](https://microsoft.github.io/mu/). As of now, Rust
development should mostly take place in this repository to minimize dependencies on the code until it is approved
to be used in production.

## First-Time Tool Setup Instructions

The following instructions install Rust.

1. Download and install rust/cargo from [Getting Started - Rust Programming Language (rust-lang.org)](https://www.rust-lang.org/learn/get-started).
   > rustup-init installs the toolchain and utilities.

2. Make sure it's working - restart a shell after install and make sure the tools are in your path:

   \>`cargo --version`

3. Install the 1.64.0 x86_64 rust toolchain.

   Windows:

   \>`rustup toolchain install 1.64.0-x86_64-pc-windows-msvc`

   \>`rustup component add rust-src --toolchain 1.64.0-x86_64-pc-windows-msvc`

   Linux:

   \>`rustup toolchain install 1.64.0-x86_64-unknown-linux-gnu`

   \>`rustup component add rust-src --toolchain 1.64.0-x86_64-unknown-linux-gnu`

4. Install Cargo Make.

   \>`cargo install --force cargo-make`

Did something not work right? Then go to _[Troubleshooting](#troubleshooting)_.

The following instructions set up the UEFI Rust code repository.

1. Download and install QEMU from: [Download QEMU](https://www.qemu.org/download/#windows) - QEMU.
   > Note, if you install the latest, windows may complain about it being a not-often downloaded file.

2. After installing QEMU, you need to manually add it to your path.
   > By default, 64-bit installs to `C:\Program Files\qemu`

3. Verify it works by executing `qemu-system-x86_64.exe` in a command prompt.
   > You should see a window pop up and attempt to boot to PXE.

4. Clone this repo:

    \>`cd <your source directory>`

    \>`git clone https://microsoft@dev.azure.com/microsoft/MsUEFI/_git/UefiRust`

5. Setup and activate a local Python virtual environment.

    \>`python -m venv UefiRust.venv`

    \>`.\UefiRust.venv\Scripts\Activate.ps1`
    > Use the script that works with your environment (e.g. .ps1 for PowerShell, .bat for "cmd").

6. Switch to the enlistment and install pip modules.

    \>`cd UefiRust`

    \>`pip install --upgrade -r pip-requirements.txt`

7. Fetch submodules and external dependencies.

    `>stuart_setup -c Platforms\QemuQ35Pkg\PlatformBuild.py`

    `>stuart_update -c Platforms\QemuQ35Pkg\PlatformBuild.py`

8. Compile the firmware (above steps are only required to configure the enlistment;subsequent builds can just run
   this command).

    \>`stuart_build -c Platforms\QemuQ35Pkg\PlatformBuild.py`

9. Verify that your UEFI build can successfully execute on QEMU by passing the `--FlashRom` argument to the build:

    \>`stuart_build -c Platforms\QemuQ35Pkg\PlatformBuild.py --FlashRom`

## Rust DXE Core Details

One of the current work items in this repository is a Rust DXE Core, ideally a bare-metal "kernel" that can be
dispatched from DXE IPL.

If QEMU and the Q35 platform build are set up successfully, this should start QEMU and boot the UEFI firmware you
built; log should be displayed in your terminal and the QEMU instance should boot to UEFI shell.

### DXE Core Goals

1. Construction of a bare-metal "kernel" to dispatch from `DxeIpl`.
   1. Built in a basic build environment for no-std
   2. Uses a basic output subsystem (likely legacy UART, but maybe VGA if it works in QEMU before GOP starts it)
   3. Integrated into the Q35 UEFI build as replacement for `DxeMain` with observable debug output
   4. No direct dependencies on PEI except PI abstracted structures

2. Integration of Rust component builds into UEFI build system - i.e. not building in two separate enlistments and
   copying around outputs.

3. Support for CPU interrupts/exception handlers.

4. Support for rudimentary paging and heap allocation.
   1. Investigate `DxeIpl` handoff implementation
   2. Explore how to handle dynamic allocation of different memory types (e.g. RuntimeCode/Data vs.
      BootServicesCode/Data)

## Rust Build Details

Multiple approaches are supported to build Rust UEFI modules.

### Cargo-make
  
  Cargo-make is a CLI tool used to abstract away many of the CLI arguments necessary to build a rust module
  so that developers can easily check, test, and build individual rust packages without the need to copying
  and/or memorizing the long list of arguments.

  cargo-make should already be installed if you followed the
  [First-Time Tool Setup Instructions](#first-time-tool-setup-instructions), however if it is not, simply
  run `cargo-install --force cargo-make`.

  Currently supported:

  1. [Building](#build-with-cargo-make) i.e. `cargo make build <optional: Rust Package>`
  2. [Testing](#test-with-cargo-make) i.e. `cargo make test <optional: Rust Package>`
  3. [Coverage](#coverage-with-cargo-make) i.e. `cargo make cov <optional: Rust Package>`

### Build QemuQ35Pkg (with Rust Modules)

  ```cmd
  stuart_build -c QemuQ35Pkg/PlatformBuild.py TOOL_CHAIN_TAG=VS2022
  ```

  This will also add the section header, FFS header, and FV header for the .efi image. Different modules types such as
  PEIM, DXE, and SMM drivers are supported.

  In summary, no special steps are needed, Rust modules will be built and included in the flash image similar to
  non-Rust modules.

#### Build and Run QEMU After Build

Adding `--flashrom` to the end of the build command will automatically run the generated firmware image on QEMU after
the build is complete.

  ```cmd
  stuart_build -c QemuQ35Pkg/PlatformBuild.py TOOL_CHAIN_TAG=VS2022 --flashrom
  ```

#### Only Run QEMU on Last Build

Adding `--flashonly` to the end of the build command will run QEMU with the last built image (skips building again).

  ```cmd
  stuart_build -c QemuQ35Pkg/PlatformBuild.py TOOL_CHAIN_TAG=VS2022 --flashonly
  ```

#### Control QEMU Shutdown Behavior

By default, QEMU will automatically shutdown after running. QEMU can be kept open, by passing the `SHUTDOWN_AFTER_RUN`
argument to the build command.

  ```cmd
  stuart_build -c QemuQ35Pkg/PlatformBuild.py TOOL_CHAIN_TAG=VS2022 --flashrom SHUTDOWN_AFTER_RUN=FALSE
  ```

### Build with cargo-make

  From the root directory, such as C:/src/UefiRust, run the following command:

  ```cmd
  cargo make build <Optional: Module Name>
  ```
  
  The following command line options are available:

  1. -p PROFILE [development|release]. DEFAULT = development (debug)
  2. -e ARCH=[IA32|X64]. DEFAULT = X64

  Examples:
  
  ```cmd
   cargo make build DxeRust
   cargo make build
   cargo make -p release build DxeRust
   cargo make -e ARCH=IA32 build FvLib
  ```
  
  If a package is not specified, all packages will be built.

  the output is target/[x86_64-unknown-uefi|i686-unknown-uefi]/[debug|release]/module_name.[efi|rlib]

### Test with cargo-make

   From the root directory, such as C:/src/UefiRust, run the following command:

  ```cmd
  cargo make test <Optional: Module Name>
  ```

  Examples:
  
  ```cmd
   cargo make test DxeRust
   cargo make test
  ```

  If a package is not specified, all packages will be tested.

### Coverage with cargo-make

   Linux:

   ```cmd
    sudo apt install libssl-dev
   ```

   From the root directory, such as C:/src/UefiRust, run the following command:

  ```cmd
  cargo make cov <Optional: Module Name>
  ```

  Examples:
  
  ```cmd
   cargo make cov DxeRust
   cargo make cov
  ```

  If a package is not specified, all packages will be tested and code coverage calculated.

  A code coverage report will be printed to the terminal and an html report can be found at target/tarpaulin-report.html

  **WARNING**: Tarpaulin code coverage is supported on windows, however it has only been verified to work on nightly 1.70+.
    If you experience any errors when 

## Supported Build Combinations

1. C source + Rust source mixed in INF (Library or Module)
   - Rust source code is supported by an EDK II build rule – Rust-To-Lib-File (.rs => .lib)
   - >Limitation: Rust cannot have external dependency.
2. Pure Rust Module only.
   - A Cargo.toml file is added to INF file as source.
   - Rust Module build is supported by EDK II build rule – Toml-File.RUST_MODULE (Toml => .efi)
   - >Limitation: Runtime might be a problem, not sure about virtual address translation for rust internal global variable.
3. Pure Rust Module + Pure Rust Library with Cargo Dependency.
   - The cargo dependency means the rust lib dependency declared in Cargo.toml.
4. Pure Rust Module + C Library with EDK II Dependency.
   - Rust Module build is supported by EDK II build rule – Toml-File (Toml => .lib)
   - The EDK II dependency means the EDK II lib dependency declared in INF.
     - If a rust module is built with C, the cargo must use staticlib. Or rlib should be used.
5. C Module + Pure Rust Library with EDK II Dependency.
   - Rust Lib build is supported by EDK II build rule – Toml-File. (Toml => .lib)
6. Pure Rust Module + Pure Rust Library with EDK II Dependency.
   - Same as #4 + #5.

## Testing

Currently, this project only supports host based testing for rust packages that
contain a library. Note that a package that compiles to an efi binary, such as
a DXE_DRIVER, can have a library; it only needs to meet one of these
requirements:

1. A `[[lib]]` section in the cargo.toml file for the Rust package
2. A lib.rs file

**It is the developer responsibility to ensure that the library remains**
***target-triple* agnostic, meaning it must be able to compile to the host**
**machine, along with i386-unknown-uefi and x86_64-unknown-uefi.**
Here are a few suggestions on how to do this:

1. Move all architecture specific functionality to a library and add the
   library to the ci.yaml ignore list
2. Move all architecture specific functionality out of the library and to the
   binary (generally the main.rs)
3. Conditionally compile architecture specific functionality using
   `#[cfg(target_os="uefi")]` or `#[cfg_attr(target_os="uefi", <DECORATOR>)]`.

### Types of Tests

There are multiple types of tests that rust can perform including integration
tests, unit tests, documentation tests, and performance tests. We currently
only care about *Integration Tests* and *Unit Tests*.
[Read More](.pytool\Plugin\CargoTestHostCheck\Readme.md#integration-tests)

### Ways to Test

A CI plugin ([Read More](.pytool\Plugin\CargoTestHostCheck\Readme.md)) exists
that locates all rust packages and executes the tests, if they exist. Executing
tests in this manner can be accomplished through the typical *stuart_ci_build*
process and is automatically executed in the CI pipeline for PRs. The developer
can additionally execute tests via the the cargo test command from within the
rust package as seen below:

1. All Tests: `cargo test --target=<TRIPLE> -Z build-std-features -Z build-std`
2. Unit and Integration Tests: `cargo test --tests --target=<TRIPLE> -Z build-std-features -Z build-std`
3. Unit Tests: `cargo test --lib --target=<TRIPLE> -Z build-std-features -Z build-std`
4. Integration Tests: `cargo test --test='*' --target=<TRIPLE> -Z build-std-features -Z build-std`

*Hint: You can determine your host's target system via the `rustc -vV` command.*

## Notes

1. This project uses `RUSTC_BOOSTRAP=1` environment variable due to internal requirements
   1. This puts us in parity with the nightly features that exist on the toolchain targeted
   2. The `nightly` toolchain may be used in place of this

## Troubleshooting

Installing the toolchain via the rust-toolchain.toml on windows may have the following error:

```bash
INFO - error: the 'cargo.exe' binary, normally provided by the 'cargo' component, is not applicable to the '1.64.0-x86_64-pc-windows-msvc' toolchain
```

To fix this:

```bash
# Reinstall the toolchain
rustup toolchain uninstall 1.64.0-x86_64-pc-windows-msvc
rustup toolchain install 1.64.0-x86_64-pc-windows-msvc

# Add the rust-src back for the toolchain
rustup component add rust-src --toolchain 1.64.0-x86_64-pc-windows-msvc
```
