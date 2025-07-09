# 🐞 WinDbg + QEMU + Patina UEFI - Debugging Guide

## Overview

QEMU can expose two serial ports—one for **software debugging** and another for **hardware debugging**.

- **Software Debugging Serial Port**
  Used for communicating with the UEFI SW debugger.

- **Hardware Debugging Serial Port**
  Used for low-level QEMU Hardware debugging

Each serial port can be used independently or simultaneously. Both serial ports
expose the [GDB Remote Serial Protocol](https://ftp.gnu.org/old-gnu/Manuals/gdb/html_node/gdb_125.html).

![QEMU Serial Ports](images/qemu_serial_ports.png)

[WinDbg](https://learn.microsoft.com/en-us/windows-hardware/drivers/debuggercmds/windbg-overview)
communicates with QEMU using the [EXDi
Interface](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/configuring-the-exdi-debugger-transport).
In essence, the EXDi interface allows WinDbg to interact with transports like
[GDB Remote Serial Protocol](https://ftp.gnu.org/old-gnu/Manuals/gdb/html_node/gdb_125.html), which QEMU provides.

The `ExdiGdbSrv.dll` in WinDbg acts as a GDB client.

![EXDi DLL](images/windbg_exdi_interface.png)

---

### End-to-End Communication Flow

#### Software Debugging: WinDbg ↔ EXDi ↔ GDB Stub ↔ Serial Port ↔ UEFI Debugger

![WinDbg EXDi QEMU Software Debugging](images/windbg_exdi_qemu_sw_debugging.png)

#### Hardware Debugging: WinDbg ↔ EXDi ↔ GDB Stub ↔ Serial Port ↔ QEMU HW

![WinDbg EXDi QEMU Hardware Debugging](images/windbg_exdi_qemu_hw_debugging.png)

---

## Setting Up WinDbg

- Download and install [Windbg](https://learn.microsoft.com/windows-hardware/drivers/debugger/) which should drop `ExdiGdbSrv.dll`
and `exdiConfigData.xml` in the `C:\Program Files\WindowsApps\Microsoft.WinDbg.Fast_1.xxxxxxxxx\amd64` directory

- Download the latest release of the UEFI WinDbg Extension from [mu_feature_debugger releases](https://github.com/microsoft/mu_feature_debugger/releases/latest)
and extract it's contents to: `%LOCALAPPDATA%\Dbg\EngineExtensions\`

---

## Building Patina DXE Core with Debugging Enabled

Clone the [patina-dxe-core-qemu](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) repo and modify the
PatinaDebugger initialization module's `.with_force_enable()` parameter in the /bin/q35_dxe_core.rs or /bin/sbsa_dxe_core.rs
file from false to true:

  ```rust
  // Before change
  patina_debugger::PatinaDebugger::new(Uart16550::Io { base: 0x3F8 }).with_force_enable(false)

  // After change
  patina_debugger::PatinaDebugger::new(Uart16550::Io { base: 0x3F8 }).with_force_enable(true)
  ```

Then build using cargo

- Q35 build

  ```cmd
  Command: "cargo make q35"
  Output: "/target/x86_64-unknown-uefi/debug/qemu_q35_dxe_core.efi"
  ```

- sbsa build:

  ```cmd
  Command: "cargo make sbsa"
  Output: "target/aarch64-unknown-uefi/debug/qemu_sbsa_dxe_core.efi"
  ```

---

## Launch the Patina QEMU UEFI Using the New Patina DXE Core

The patina-qemu UEFI build by default uses a pre-compiled DXE core binary from a nuget feed provided by the patina-dxe-core-qemu
repository with debug disabled.  To override this default, the platform .FDF file can be updated to point to the new .efi
file produced above, or the build supports a command line parameter to indicate the new DXE core override binary.

To use the command line parameter, after cloning the [patina-qemu](https://github.com/OpenDevicePartnership/patina-qemu)
repository, run the following commands to setup and build a UEFI using the override DXE core binary and execute QEMU with
both serial and GBD support enabled.

```cmd
stuart_setup -c Platforms\QemuQ35Pkg\PlatformBuild.py
stuart_update -c Platforms\QemuQ35Pkg\PlatformBuild.py
stuart_build -c Platforms/QemuQ35Pkg/PlatformBuild.py GDB_SERVER=5555 SERIAL_PORT=56789 --FlashRom BLD_*_DXE_CORE_BINARY_PATH="<path to dxe core file>"
```

As an option if several iterative changes are being made to the DXE core for testing, the Patina project has a
[patina-fw-patcher](https://github.com/OpenDevicePartnership/patina-fw-patcher) utility to replace the DXE core .EFI file
in an already compiled UEFI binary.  Clone the [patina-fw-patcher](https://github.com/OpenDevicePartnership/patina-fw-patcher)
repository then run the following command from this repo to find the UEFI binary, patch in the new DXE core binary, and
execute QEMU.

```cmd
python build_and_run_rust_binary.py --fw-patch-repo "<path to patina-fw-patcher>" --custom-efi "<path to dxe core file>" -s 56789 -g 5555
```

Both the stuart_build or the build_and_run_rust_binary.py commands should launch QEMU and wait for the initial break in:

![QEMU Hardware and Software Debugging ports](images/qemu_sw_hw_debugging_serial_ports.png)

![QEMU Initial break in](images/qemu_initial_break_in.png)

---

## Launching WinDbg Instances

### Instance 1: Software Debugging

1. Launch **WinDbg**. This will connect to QEMU SW serial port(`56789`)

   ![WinDbg Software Launch](images/windbg_launch_sw_debugging.png)

2. Set symbol and source paths:

   ```cmd
   .sympath+ <path to pdb dir> ; usually <cloned dir>\target\x86_64-unknown-uefi\debug\deps
   .srcpath+ <path to src dir> ; usually <cloned dir>\src
   ```

3. Initialize the UEFI debugger extension:

   ```cmd
   !uefiext.init
   ```

4. `!uefiext.help` will list all available commands

   ![WinDbg Software Debugging](images/windbg_sw_debugging.png)

---

### Instance 2: Hardware Debugging (Optional)

1. Launch another **WinDbg** instance

   ![WinDbg Hardware Debugging Launch](images/windbg_launch_hw_debugging.png)

2. It should automatically connect to the QEMU HW serial port(`5555`).
