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

- Download and install [Windbg](https://learn.microsoft.com/windows-hardware/drivers/debugger/)
- Verify `ExdiGdbSrv.dll` and `exdiConfigData.xml` were installed into the
  `C:\Program Files\WindowsApps\Microsoft.WinDbg.Fast_1.xxxxxxxxx\amd64` directory
- Download the latest release of the UEFI WinDbg Extension from [mu_feature_debugger releases](https://github.com/microsoft/mu_feature_debugger/releases/latest)
- Extract its contents to: `%LOCALAPPDATA%\Dbg\EngineExtensions\`

---

## Launching Patina QEMU UEFI with Debugging Enabled

The patina-qemu UEFI build by default uses a pre-compiled DXE core binary from a nuget feed provided by the
[patina-dxe-core-qemu](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) repository.  Since these
binaries have debug disabled, the following steps need to be performed to enable debug and override the default.

Note: The following steps are for the Q35 build, but the same can be done for the SBSA build.  They also use the
build command line parameter BLD_*_DXE_CORE_BINARY_PATH to override the current DXE core with the new file.  For
other options such as patching a UEFI FD binary, see the patina-qemu readme [advanced usage](https://github.com/OpenDevicePartnership/patina-qemu?tab=readme-ov-file#advanced-usage)
section.

- Clone the patina DXE Core QEMU repository
- Open the `/bin/q35_dxe_core.rs` file and locate the static `DEBUGGER` declaration
- Change the Patina Debugger `.with_force_enable()` module's input from `false` to `true`

  ```rust
  // Before change
  patina_debugger::PatinaDebugger::new(Uart16550::Io { base: 0x3F8 }).with_force_enable(false)
  // After change
  patina_debugger::PatinaDebugger::new(Uart16550::Io { base: 0x3F8 }).with_force_enable(true)
  ```

- Build a new Patina DXE Core EFI driver

  ```cmd
  cargo make q35
  ```

- Copy the output file `/target/x86_64-unknown-uefi/debug/qemu_q35_dxe_core.efi` back to where this patina-qemu code is
  being compiled.  The next step assumes it was copied to the patina-qemu root folder.
- Rebuild the patina-qemu UEFI with the new DXE driver and execute QEMU with both serial and GBD support enabled.
  The command line parameter BLD_*_DXE_CORE_BINARY_PATH is used to indicate the override DXE core driver.

  ```cmd
  stuart_build -c Platforms/QemuQ35Pkg/PlatformBuild.py GDB_SERVER=5555 SERIAL_PORT=56789 --FlashRom BLD_*_DXE_CORE_BINARY_PATH="./qemu_q35_dxe_core.efi"
  ```

- The stuart_build command should launch QEMU and wait for the initial break in:

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
