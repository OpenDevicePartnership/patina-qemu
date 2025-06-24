
# Patina DXE Core Release Binary Composition and Size Optimizations

This document serves as a reference for the current set of size-related
optimizations performed on the Patina DXE Core release binary.

**TL;DR:** Below is a summary of the current status of the QEMU DXE Core binary
size, marking the logical conclusion of the size optimization efforts.

1. Rust compiler size related optimizations (**Applied**)
2. Disabling logging can save an additional 150KB (**Not Applied**)
3. Defaulting to UEFI decompression in the section extractor while skipping
   Brotli and CRC32 can save another 150KB (**Not Applied**)
4. Excluding patina debugger support will save another 55KB (**Not Applied**)

Bringing the size from 1,162 KB to **406 KB** (A reduction of 65%). The details
are documented below.

## 1. Common Rust compiler optimizations focused on reducing binary size

**Optimization Status:** Applied

```toml
[profile.release]
codegen-units = 1               # Default is 16; setting it to 1 prioritizes size over compilation speed.
debug = "full"
lto = true                      # Enables Link Time Optimization - significant size reduction.
opt-level = "s"                 # Optimize for size - significant size reduction.
split-debuginfo = "packed"
strip = "symbols"               # Remove symbol information from the final binary(not very relevant for PE files).
incremental = true
```

Below is the composition of the Patina DXE Core release binary for QEMU, located
at `target\x86_64-unknown-uefi\release\qemu_q35_dxe_core.efi`. This represents
the final binary after applying the non-destructive compiler optimizations
outlined in [PR #19](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/pull/19):

| Segment       | Size on Disk |
| ------------- | -----------: |
| .text         |     495.0 KB |
| .rdata        |     249.5 KB |
| .data         |       3.0 KB |
| .pdata        |      11.0 KB |
| miscellaneous |       4.0 KB |
| **Total**     |   **762 KB** |

**Results:** **1,162 KB** down to **762 KB** (a reduction of approximately **35%**)

## 2. Logging: Reducing Binary Size by Disabling Logging

**Optimization Status:** Not Applied

The other substantial reduction in the release binary size is observed when
logging is completely disabled, as shown below in
`\patina-dxe-core-qemu\Cargo.toml` using `release_max_level_off`.

```toml
[dependencies]
log = { version = "^0.4", default-features = false, features = ["release_max_level_off"] }
```

**Results:** **762 KB** to **599 KB** (a reduction of approximately **160 KB**(**21%**))

## 3. Decompression: Reducing Binary Size by Defaulting to UEFI decompression

**Optimization Status:** Not Applied

Next, Defaulting to UEFI decompression in the section extractor while skipping
Brotli and CRC32 can save another 140KB.

```toml
[dependencies]
patina_section_extractor = {  version = "4", registry = "patina-fw", default-features = false, features=["uefi_decompress"] }
```

**Results:** **599 KB** to **460 KB** (a reduction of approximately **140 KB**(**23%**))

## 4. Debugger: Reducing Binary Size by excluding debugger support

**Optimization Status:** Not Applied

Next, Excluding debugger support can save another 55KB. This reduces the size
from 460 KB to **~406 KB**.

```rust
// Commenting below lines will exclude debugger support
static DEBUGGER: patina_debugger::PatinaDebugger<Uart16550> =
    patina_debugger::PatinaDebugger::new(Uart16550::Io { base: 0x3F8 })
        .with_force_enable(false)
        .with_log_policy(patina_debugger::DebuggerLoggingPolicy::FullLogging);

    patina_debugger::set_debugger(&DEBUGGER);
```

```toml
[dependencies]
# Commenting below line will exclude debugger support
# patina_debugger = { version = "4", registry = "patina-fw" }
```

**Results:** **460 KB** to **406 KB** (a reduction of approximately **55 KB**(**11%**))

## Release binary size after above optimizations

```cmd
C:\r\patina-dxe-core-qemu>dir C:\r\patina-dxe-core-qemu\target\x86_64-unknown-uefi\release\qemu_q35_dxe_core.efi
 Volume in drive C has no label.
 Volume Serial Number is 40DF-F702

 Directory of C:\r\patina-dxe-core-qemu\target\x86_64-unknown-uefi\release

06/24/2025  04:53 PM           406,528 qemu_q35_dxe_core.efi
               1 File(s)        406,528 bytes
               0 Dir(s)  183,428,534,272 bytes free
```

## Project MU vs Patina FV size comparison

Up to this point, we have seen what constitutes the Rust DXE Core binary and how
it can be optimized. This section attempts to perform a platform image-level
comparison between a C-based UEFI firmware image and a Rust-based Patina UEFI
firmware image. There are many components that differ between the C-based UEFI
and the Rust-based UEFI images. Below is a summary of the components that have
been folded into the Rust DXE Core. On the left, you can see the size
represented by the Project MU-based QEMU FV file (DxeCore, RuntimeDxe, CpuDxe),
and on the right, the size represented by the Patina FV file (DxeCore).

| Module     |                     Mu |                 Patina |
| ---------- | ---------------------: | ---------------------: |
| DxeCore    |     0x2EC3E (188.0 KB) |     0x63430 (405.0 KB) |
| RuntimeDxe |        0x206E (8.1 KB) |                        |
| CpuDxe     |     0x19266 (100.2 KB) |                        |
| **TOTAL**  | **0x49F12 (296.3 KB)** | **0x63430 (405.0 KB)** |

All in all, the Rust binary includes the following additional features that are
not available in the C-based UEFI image, with a size increase of approximately
110 KB compared to the C-based image:

- It incorporates functionality that is typically split across multiple EFI
  binaries (DxeCore, RuntimeDxe, CpuDxe) in C-based implementations.
- It includes advanced features such as built-in source-level debugging
  capabilities.
- It supports two dispatchers: pure Rust component dispatch and FFS dispatch.
- It provides stringent memory protection features not present in most C-based
  DXE Core implementations.
