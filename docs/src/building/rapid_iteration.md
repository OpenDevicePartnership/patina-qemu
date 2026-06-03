# Rapid Patina Iteration

The `build_and_run_rust_binary.py` script is intended for rapid development of Patina in this repository. It takes a
prebuilt FD (from a previous `stuart_build` invocation) and patches a new Patina DXE Core into it. This greatly speeds
up the development loop because none of the C components are rebuilt.

```admonish warning
When a C component is changed, `stuart_build` must be re-run to rebuild the full platform. The patcher only updates
the Patina DXE Core within an existing FD.
```

## Prerequisites

The script requires that the following repositories are cloned locally:

- [`patina-dxe-core-qemu`](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) - the Patina DXE Core source
  used by this repository.
- [`patina-fw-patcher`](https://github.com/OpenDevicePartnership/patina-fw-patcher) - the patcher tool that injects the
  new DXE Core into a built FD.

## How it Works

The script will:

1. Build a new Patina DXE Core from `patina-dxe-core-qemu` for the requested platform.
2. Use the patcher to patch the new DXE Core into the existing FD.
3. Launch QEMU to execute the patched FD.

## Running the Script

```bash
python build_and_run_rust_binary.py -p Q35
```

Use `python build_and_run_rust_binary.py -h` to see all related options, including the ARM Virt platform target and
debugger configuration flags.
