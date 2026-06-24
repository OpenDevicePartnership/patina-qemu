# Building the Firmware

Steps to set up your environment, compile, and run `QemuQ35Pkg` and `QemuArmVirtPkg`.

## Developer Environment

This is a Stuart-based platform, so the default environment requirements come from the
[How to Build with Stuart](https://github.com/tianocore/tianocore.github.io/wiki/How-to-Build-With-Stuart)
instructions.

QEMU is used to run the locally compiled firmware on a virtual platform.

- **Windows:** No action is needed. An [external dependency](https://www.tianocore.org/edk2-pytool-extensions/features/extdep/)
  in the repository provides the necessary QEMU binaries.
- **Linux:** [Install QEMU](https://www.qemu.org/download/#linux).

This build uses [edk2-pytools](https://github.com/tianocore/edk2-pytool-extensions/tree/master/docs) for its
functionality. On most Linux distros, mono and nuget support require an extra step. See:
[A note on nuget on Linux](https://github.com/tianocore/edk2-pytool-extensions/blob/master/docs/usability/using_extdep.md#a-note-on-nuget-on-linux).

## CLANGPDB Toolchain

Both `QemuQ35Pkg` and `QemuArmVirtPkg` use the CLANGPDB toolchain exclusively. This enables development of both
architectures on Linux and Windows, native PE/COFF image generation, and PDBs that work with the
[uefi_debug_tools](https://github.com/microsoft/uefi_debug_tools) WinDbg infrastructure or
[lldb](https://lldb.llvm.org/).

LLVM version 21 or greater is recommended for use with EDK II and related projects. It can be downloaded from
[LLVM itself](https://github.com/llvm/llvm-project/releases). Add the directory containing the `clang` executable to
your `PATH` (restarting the terminal if necessary) and follow the steps below to build with Stuart.

```admonish note title="Hafnium and TF-A"
By default, Hafnium and TF-A (used in the `QemuArmVirtPkg` build) are pulled in as precompiled binaries. Passing
`HAF_TFA_BUILD=TRUE` on the `stuart_build` command line recompiles these components. This is only supported on Linux
as those projects do not build natively on Windows. They still use clang to compile.
```

## Building with Pytools

1. **\[Optional\] Create a Python virtual environment.** Generally once per workspace.

    ```bash
    python -m venv <name of virtual environment>
    ```

2. **\[Optional\] Activate the virtual environment.** Each time a new shell is opened.

    - Linux

      ```bash
      source <name of virtual environment>/bin/activate
      ```

    - Windows

      ```bash
      <name of virtual environment>/Scripts/activate.bat
      ```

3. **Install Pytools.** Generally once per virtual env or whenever `pip-requirements.txt` changes.

    ```bash
    pip install --upgrade -r pip-requirements.txt
    ```

4. **Initialize and update submodules.** Only when submodules have updated.

    First time setup:

    ```bash
    stuart_setup -c Platforms/<Package>/PlatformBuild.py
    ```

    Subsequent submodule updates:

    ```bash
    git submodule update --recursive
    ```

5. **Initialize and update dependencies.** Only as needed when `ext_dep`s change.

    ```bash
    stuart_update -c Platforms/<Package>/PlatformBuild.py
    ```

6. **Compile firmware.**

    ```bash
    stuart_build -c Platforms/<Package>/PlatformBuild.py
    ```

    Use `stuart_build -c Platforms/<Package>/PlatformBuild.py -h` to see additional options like `--clean`.

7. **Running QEMU.**
    - Append `--FlashRom` to the build command and QEMU will run after the build completes, booting the new firmware.
    - Or use `--FlashOnly` to skip the build and launch QEMU with the last built firmware:

      ```bash
      stuart_build -c Platforms/<Package>/PlatformBuild.py --FlashOnly
      ```

```admonish info title="QEMU Notes"
- QEMU is provided on Windows via an external dependency located at `QemuPkg/Binaries`. QEMU must be manually
  downloaded on Linux.
- QEMU on Linux requires at least **version 9.0.2** when booting an operating system. If you are only booting to
  shell, matching the version of the Windows external dependency is acceptable.
- To override the external dependency on Windows, or the installed version on Linux, use `QEMU_PATH=<path>` on the
  command line.
```

### Custom Build Options

| Option | Effect |
|--------|--------|
| `SHUTDOWN_AFTER_RUN=TRUE` | Outputs a `startup.nsh` file to the location mapped as `fs0` with `reset -s` as the final line. Used in CI in combination with `--FlashOnly` to run QEMU to the UEFI shell and then execute the contents of `startup.nsh`. |
| `QEMU_PATH=<path>` | Use a specific QEMU binary. |
| `QEMU_HEADLESS=TRUE` | Run QEMU with no display. CI servers run headless and require this; locally it is not needed. |
| `GDB_SERVER=<TCP port>` | Enables the QEMU GDB server at the provided TCP port. Connect a GDB client for hardware-level debugging. |
| `SERIAL_PORT=<TCP port>` | Enables the specified serial port. Primarily used to connect to the software debugger when enabled. |
| `ENABLE_NETWORK=TRUE` | Enables networking. Currently only supported on the Q35 platform. |
| `PATH_TO_OS=<path>` | Boots an OS image (VHDX or QCOW2) instead of stopping at the UEFI shell. |

`SERIAL_PORT` defaults:

- **Q35**: defaults to `None` to avoid unintended port conflicts in the pipeline. The
  [build_and_run_rust_binary.py](rapid_iteration.md) script defaults to port `50001`.
- **ARM Virt**: defaults to `None`. When set, a separate virtio serial port is exposed on the specified TCP port for
  use by the software debugger. Log output continues to be sent to stdio regardless of this setting, consistent with
  Q35 where the serial port is separate from `debugcon` used for logging.

### Passing Build Defines

To pass build defines through `stuart_build`, prepend `BLD_*_` to the define name and pass it on the command line.
`stuart_build` requires values to be assigned, so add a `=1` suffix for bare defines.

For example, to enable E1000 network support, instead of the traditional `-D E1000_ENABLE`:

```bash
stuart_build -c Platforms/<Package>/PlatformBuild.py BLD_*_E1000_ENABLE=1
```

## References

- [Installing and using Pytools](https://www.tianocore.org/edk2-pytool-extensions/using/install/)
- [Python virtual environments](https://docs.python.org/3/library/venv.html)
