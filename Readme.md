# Patina Development Vehicle

[![CI](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/platform-ci.yml/badge.svg)](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/platform-ci.yml)
[![Nightly Regression Tests](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/nightly-os-boot.yml/badge.svg?event=schedule)](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/nightly-os-boot.yml)

The primary purpose of this repository is to serve as a test vehicle for the Patina project. It can also be used as
a reference for a simple integration of Patina with EDK II components. It contains a permanent fork of
[`OvmfPkg`](https://github.com/tianocore/edk2/tree/HEAD/OvmfPkg) from EDK II adapted for Patina.

## Documentation

All `patina-qemu` platform documentation is maintained in the [`docs/`](./docs) mdBook. Start with the
[Introduction](./docs/src/introduction.md). The two supported platforms have their own documentation pages:

- [QEMU Q35](./docs/src/platforms/q35.md)
- [QEMU ARM Virt](./docs/src/platforms/armvirt.md)

Other common topics:

- [Building the Firmware](./docs/src/building/building.md)
- [Rapid Patina Iteration](./docs/src/building/rapid_iteration.md)
- [WinDbg + QEMU + Patina UEFI Debugging Guide](./docs/src/debugging/windbg_uefi.md)
- [WinDbg + QEMU + Windows OS Debugging Guide](./docs/src/debugging/windbg_windows.md)

The book is built with [mdBook](https://rust-lang.github.io/mdBook/).

## Building the Book Locally

The pinned versions of mdBook and its preprocessors are defined in the `[tools]` section of
[`rust-toolchain.toml`](./rust-toolchain.toml). CI installs the same versions from this file.

To install them locally, install [Rust](https://rustup.rs) and then run `cargo install --locked <name>@<version>` for
each entry under `[tools]`. For example:

```bash
cargo install --locked mdbook@<version>
```

Substitute [`cargo binstall`](https://github.com/cargo-bins/cargo-binstall) for `cargo install` if you have it for
faster installs from prebuilt binaries.

Then serve the book from the `docs/` directory:

```bash
cd docs
mdbook serve --open
```

## Related Links

- [Open Device Partnership (ODP)](https://opendevicepartnership.org/)
- [Patina project documentation](https://opendevicepartnership.github.io/patina/)
- [Patina GitHub organization](https://github.com/OpenDevicePartnership)
- [Patina GitHub repository](https://github.com/OpenDevicePartnership/patina)
- [Patina DXE Core QEMU](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu)
  - The Patina DXE Core `.efi` binary used by this repository
