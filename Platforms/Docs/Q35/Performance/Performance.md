# Q35 Performance

Performance on Q35 is managed by a Rust component that writes and publishes the FBPT.
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
[performance collection is already enabled by default]((https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/blob/main/bin/q35_dxe_core.rs))
for Q35 in `patina-dxe-core-qemu`.

### 2. Boot and Collect FBPT Binary

When enabled, the performance component will collect FBPT results during UEFI boot.
`BLD_*_PERF_TRACE_ENABLE=TRUE` must be enabled.

A normal Q35 build will produce `FbptDump.efi`, a UEFI shell app that collects FBPT results in a `.bin` file.
The [source code for `FbptDump`](https://github.com/microsoft/mu_plus/tree/dev/202502/UefiTestingPkg/PerfTests/FbptDump)
can be found in `mu_plus`.

### 3. Interpret Performance Results

There are
[two scripts](https://github.com/tianocore/edk2-pytool-extensions/blob/master/edk2toolext/perf/fpdt_parser.py)
to convert the FBPT binary dump into usable results in `edk2-pytool-extensions`.

First, [`fpdt_parser.py`](https://github.com/tianocore/edk2-pytool-exatensions/blob/master/edk2toolext/perf/fpdt_parser.py)
converts the `.bin` file into an XML.
Then, [`perf_report_generator.py`](https://github.com/tianocore/edk2-pytool-extensions/blob/master/edk2toolext/perf/perf_report_generator.py)
converts the XML into an HTML page with filtering and graphing options.

An example of these results can be found in [Q35_Results.md](Q35_Results.md).
