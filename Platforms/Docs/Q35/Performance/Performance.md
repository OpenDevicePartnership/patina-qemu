# Q35 Performance

Performance on Q35 is managed by a Rust component that writes and publishes the FPDT. 
For more information on the functionality of the performance component, see the 
[documentation](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/components/patina_performance.md)
in the main `patina` repo. 
This document focuses primarily on how to collect and interpret performance results.

## Collecting Performance

### 1. Enable Performance Component

The performance component must be enabled to collect FPDT results. Example code can be found under 
[Enabling Performance Measurements](https://github.com/OpenDevicePartnership/patina/blob/main/docs/src/components/patina_performance.md#enabling-performance-measurements)
in the main `patina` repo.

Note that performance collection is already enabled by default for Q35 in [`patina-dxe-core-qemu`](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/blob/main/bin/q35_dxe_core.rs).

### 2. Boot and Collect FPDT Binary

When enabled, the performance component will collect FPDT results during UEFI boot.
