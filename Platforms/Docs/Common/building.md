# Building

Steps to setup your environment, compile, and run QemuQ35Pkg and QemuSbsaPkg.

## Developer environment

This is a Stuart-based platform and thus the default environment requirements can be found at the
[How to Build with Stuart](https://github.com/tianocore/tianocore.github.io/wiki/How-to-Build-With-Stuart) instructions.

QEMU is used to run the locally compiled firmware on a virtual platform. If you are on windows,
no action is needed, we provide an [external dependency](https://www.tianocore.org/edk2-pytool-extensions/features/extdep/)
that includes the necessary QEMU binaries.

If you are on Linux, [install it](https://www.qemu.org/download/#linux).

This build uses edk2-pytools for functionality.  Documentation can be found [here](https://github.com/tianocore/edk2-pytool-extensions/tree/master/docs).
On most Linux distros this requires an extra step for mono and nuget support.

<https://github.com/tianocore/edk2-pytool-extensions/blob/master/docs/usability/using_extdep.md#a-note-on-nuget-on-linux>

## CLANGPDB

Both QemuQ35Pkg and QemuSbsaPkg use the CLANGPDB toolchain exclusively. This allows development of both architectures
on Linux and Windows, native PE/COFF image generation, and PDBs to connect with the
[uefi_debug_tools](https://github.com/microsoft/uefi_debug_tools) WinDbg infrastructure or
[lldb](https://lldb.llvm.org/).

LLVM version 21 or greater is recommended to use with EDK II and related projects. It can be downloaded through many
sources, but the simplest is from [LLVM itself](https://github.com/llvm/llvm-project/releases). Add the directory
containing the clang executable to your PATH variable (restarting a terminal if necessary) and follow the instructions
below on building with stuart.

>**Note:**: By default, Hafnium and TF-A (only used in the QemuSbsaPkg build) are pulled in as precompiled binaries.
> Passing HAF_TFA_BUILD=TRUE on the stuart_build command line will recompile these components. This is only supported
> on Linux as these projects do not build natively on Windows. They still use clang to compile.

## Building with Pytools

1. [Optional] Create a Python Virtual Environment - generally once per workspace

    ``` bash
    python -m venv <name of virtual environment>
    ```

2. [Optional] Activate Virtual Environment - each time new shell opened
    - Linux

      ```bash
      source <name of virtual environment>/bin/activate
      ```

    - Windows

      ``` bash
      <name of virtual environment>/Scripts/activate.bat
      ```

3. Install Pytools - generally once per virtual env or whenever pip-requirements.txt changes

    ``` bash
    pip install --upgrade -r pip-requirements.txt
    ```

4. Initialize & Update Submodules - only when submodules updated

    First time setup:

    ``` bash
    stuart_setup -c Platforms/<Package>/PlatformBuild.py
    ```

    Subsequently when submodules are updated:

    ```bash
    git submodule update --recursive
    ```

5. Initialize & Update Dependencies - only as needed when ext_deps change

    ``` bash
    stuart_update -c Platforms/<Package>/PlatformBuild.py
    ```

6. Compile Firmware

    ``` bash
    stuart_build -c Platforms/<Package>/PlatformBuild.py
    ```

    - use `stuart_build -c Platforms/<Package>/PlatformBuild.py -h` option to see additional
    options like `--clean`

7. Running QEMU
    - You can add `--FlashRom` to the end of your build command and QEMU will run after the
    build is complete, booting this FW.
    - or use the `--FlashOnly` feature to skip the build and launch QEMU with the last built FW.

      ``` bash
      stuart_build -c Platforms/<Package>/PlatformBuild.py --FlashOnly
      ```

### Notes

1. QEMU is provided on Windows via an external dependency located at QemuPkg/Binaries; Qemu must be manually downloaded
   on Linux.
2. QEMU for Linux requires at least **version 9.0.2** when booting an operating system; if you are only booting to
   shell, matching the version to the Windows external dependency is acceptable.
3. If you want to override the external dependency on Windows, or the installed version on Linux, you can use
   `QEMU_PATH = <path>` on the command line.

### Custom Build Options

**SHUTDOWN_AFTER_RUN=TRUE** will output a *startup.nsh* file to the location mapped as fs0 with `reset -s` as
the final line. This is used in CI in combination with the `--FlashOnly` feature to run QEMU to the UEFI shell
and then execute the contents of *startup.nsh*.

**QEMU_PATH** Can specify the path to a specific QEMU binary to use.

**QEMU_HEADLESS=TRUE** Since CI servers run headless QEMU must be told to run with no display otherwise
an error occurs. Locally you don't need to set this.

**GDB_SERVER=\<TCP Port\>** Enables the QEMU GDB server at the provided TCP port. This can be connected to a GDB client
for debugging via the hardware debugger.

**SERIAL_PORT=\<Serial Port\>** Enables the specified serial port to be used. Primarily this is used to connect to the
software debugger when enabled. On Q35 this defaults to 50001. SBSA only has a single serial port for normal world and
so by default does not set this so it can send serial output to stdio. Setting this for SBSA will prevent logs from
coming over stdio and instead will go to this TCP port.

**ENABLE_NETWORK=TRUE** will enable networking (currently supported on the QEMU Q35 platform).

### Passing Build Defines

To pass build defines through *stuart_build*, prepend `BLD_*_` to the define name and pass it on the
command-line. *stuart_build* currently requires values to be assigned, so add a `=1` suffix for bare defines.
For example, to enable the E1000 network support, instead of the traditional "-D E1000_ENABLE", the stuart_build
command-line would be:

`stuart_build -c Platforms/<Package>/PlatformBuild.py BLD_*_E1000_ENABLE=1`

## The build_and_run_rust_binary.py Script

The build_and_run_rust_binary.py script is intended for rapid development of Patina in this repository. It takes a
prebuilt FD (from a previous stuart_build invocation) and patches a new Patina DXE core into it. This greatly speeds
up the development loop as none of the C components are rebuilt. However, when a C component is changed, stuart_build
must be re-run to rebuild the full platform, not just Patina.

It requires that [patina-dxe-core-qemu](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) and
[patina-fw-patcher](https://github.com/OpenDevicePartnership/patina-fw-patcher) are cloned locally. The script will
then build a Patina DXE core from patina-dxe-core-qemu and use the patcher to patch it into the existing FD. Finally,
it will launch QEMU to execute this patched FD. Use `python build_and_run_rust_binary.py -h` to see all related options.

## References

- [Installing and using Pytools](https://www.tianocore.org/edk2-pytool-extensions/using/install/)
- More on [python virtual environments](https://docs.python.org/3/library/venv.html)
