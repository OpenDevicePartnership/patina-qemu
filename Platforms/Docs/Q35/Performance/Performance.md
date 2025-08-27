# Q35 Performance

Performance on Q35 is managed by a Rust component that writes and publishes the
[Firmware Basic Boot Performance Table (FBPT)](https://uefi.org/specs/ACPI/6.5/05_ACPI_Software_Programming_Model.html#firmware-basic-boot-performance-table).
For more information on the functionality of the performance component, see the
[documentation](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/components/patina_performance.md)
in the main `patina` repo.
This document focuses primarily on how to collect and interpret performance results.

## Collecting Performance

### 1. Enable Performance Component

The performance component must be enabled to collect FBPT results. Example code can be found under
[Enabling Performance Measurements](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/components/patina_performance.md#enabling-performance-measurements)
in the main `patina` repo.

Note that
[performance collection is already enabled by default](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/blob/main/bin/q35_dxe_core.rs)
for Q35 in `patina-dxe-core-qemu`.

### 2. Boot and Collect FBPT Binary

When enabled, the performance component will collect FBPT results during UEFI boot.
This is done by setting `BLD_*_PERF_TRACE_ENABLE=TRUE` as an environment variable to `stuart_build`:

```
stuart_build  -c Platforms/QemuQ35Pkg/PlatformBuild.py --flashrom BLD_*_PERF_TRACE_ENABLE=TRUE
```

For more information on the `stuart` system, see the `patina-qemu` [README.md](https://github.com/OpenDevicePartnership/patina-qemu?tab=readme-ov-file#build-and-run).

A normal Q35 build will produce `FbptDump.efi`, a UEFI shell app that collects FBPT results in a `.bin` file.
The [source code for `FbptDump`](https://github.com/microsoft/mu_plus/tree/dev/202502/UefiTestingPkg/PerfTests/FbptDump)
can be found in `mu_plus`.

### 3. Interpret Performance Results

There are
[two scripts](https://github.com/tianocore/edk2-pytool-extensions/blob/master/edk2toolext/perf/fpdt_parser.py)
to convert the FBPT binary dump into usable results in `edk2-pytool-extensions`.

First, [`fpdt_parser.py`](https://github.com/tianocore/edk2-pytool-extensions/blob/master/edk2toolext/perf/fpdt_parser.py)
converts the `.bin` file into an XML.
Then, [`perf_report_generator.py`](https://github.com/tianocore/edk2-pytool-extensions/blob/master/edk2toolext/perf/perf_report_generator.py)
converts the XML into an HTML page with filtering and graphing options.

An example of these results can be found in [Q35_Results.md](Q35_Results.md).
