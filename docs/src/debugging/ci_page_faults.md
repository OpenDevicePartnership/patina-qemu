# CI Page Fault Diagnostics

The nightly Q35 Ubuntu boot jobs enable `QEMU_DIAGNOSTICS_DIR=boot-diagnostics`.
The same option can be supplied to a local `stuart_build --FlashOnly` command.
This diagnostic mode requires Linux and QEMU's Unix-socket QMP support.

## Artifacts

Each target uploads `boot-diagnostics-Q35-<target>-SSD-ubuntu` for seven days:

- `guest.log`: firmware debug-console output.
- `launch.json`: the QEMU executable, argument list, and version.
- `fault.json`: QMP responses, including per-vCPU registers, address translations,
  512 bytes from the original exception stack, and 32 bytes at its instruction pointer.
- Host kernel and CPU details, QEMU and swtpm versions, disk information, and
  SHA256 hashes of the initial disk, seed, and firmware FD files.
- The matching FAT driver's EFI, PDB, and map files, plus the DXE dependency manifest.

The existing firmware artifacts contain the pre-boot FD files. The Ubuntu image
artifact contains the pristine disk and seed ISO; use fresh copies for each reproduction.

## Capture Behavior

Diagnostic mode removes the `isa-debug-exit` device so a firmware panic cannot
immediately terminate QEMU before inspection. Normal OS power-off still exits QEMU.
After the exception handler prints its registers and reaches stack tracing, the
collector stops all vCPUs and reads the saved exception addresses through QMP.
It then terminates QEMU and marks the run failed. A ten-minute deadline also
attempts collection and terminates QEMU if the monitor is reachable.

The monitor's current registers belong to the exception handler, not necessarily
the faulting instruction. The raw stack and code reads use addresses parsed from
the original exception log. Each vCPU is selected in turn so differences in its
active address space remain visible. Inspect QMP responses for translation/read
errors; a nested fault or address-space change can prevent a read.

For the reported `Fat+0x1056` fault, compare the exact artifact's disassembly with
the local `InternalMemCopyMem` implementation. If it has only pushed RSI and RDI,
the return address is the third 64-bit word at the saved RSP (`RSP + 0x10`).
Subtract the FAT image load base from that address and resolve it using the map/PDB
to identify the copy's caller. Do not assume offsets match across builds.

## Limits

This captures the fault's stack and runtime environment, not allocation/free
history. Proving a use-after-free still requires allocator and page-mapping
instrumentation in the Patina DXE core; the prebuilt binary cannot provide that
history through QMP. An unmapped page by itself does not prove a use-after-free.

Diagnostic mode changes panic handling and introduces host-side output processing.
Compare with a normal run if the failure is timing-sensitive. Stack artifacts
contain raw guest memory and should be handled as debugging data.

Run the collector tests with:

```bash
python3 -m unittest discover -s Platforms/QemuQ35Pkg/Plugins/QemuRunner -p 'test_fault_capture.py' -v
```