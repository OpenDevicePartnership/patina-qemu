
# Patina DXE Core Release Binary Composition and Size Optimization

This document serves as a reference for the current set of size-related
optimizations performed on the DXE Core release binary (even if the numbers
listed below may eventually become outdated). It also summarizes the current
binary composition after the optimizations.

TL;DR: Below is a summary of the current status of the QEMU DXE Core binary
size, marking the logical conclusion of the size optimization efforts—from
1,162 KB down to 762 KB (a reduction of approximately 35%). An additional 150 KB
reduction is possible (but not applied) by completely disabling logging bringing
the binary size to 599 KB, as documented below.

```toml
[profile.release]
codegen-units = 1               # Default is 16; setting it to 1 prioritizes size over compilation speed.
debug = "full"
lto = true                      # Enables Link Time Optimization — significant size reduction.
opt-level = "s"                 # Optimize for size — significant size reduction.
split-debuginfo = "packed"
strip = "symbols"              # Remove symbol information from the final binary(not very relevant for PE files).
incremental = true
```

Below is the composition of the Patina DXE Core release binary for QEMU, located
at `target\x86_64-unknown-uefi\release\qemu_q35_dxe_core.efi`. This represents
the final binary after applying the non-destructive compiler optimizations
outlined in [PR
#19](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu/pull/19):

| Segment       | Size on Disk |
| ------------- | -----------: |
| .text         |     495.0 KB |
| .rdata        |     249.5 KB |
| .data         |       3.0 KB |
| .pdata        |      11.0 KB |
| miscellaneous |       4.0 KB |
| **Total**     |   **762 KB** |

## .text Segment Details

The `.text` segment comprises over 1,050 functions and accounts for 495 KB of
the binary. Remarkably, fewer than 40 of these functions make up approximately
**43% (212 KB)** of the `.text` segment.

| Size on Disk | Name                                                                                                                                               |
| -----------: | -------------------------------------------------------------------------------------------------------------------------------------------------- |
|     33.79 KB | `qemu_q35_dxe_core::_start(enum core::ffi::c_void*)`                                                                                               |
|     19.88 KB | `patina_debugger::debugger::impl$4::handle_interrupt<enum2$<patina_sdk::serial::uart::Uart16550> >(....)`                                          |
|     18.72 KB | `goblin::pe::PE::parse_with_opts(ref$<slice2$<u8> >, goblin::pe::options::ParseOptions*)`                                                          |
|     13.72 KB | `patina_section_extractor::composite::impl$1::extract(patina_section_extractor::composite::CompositeSectionExtractor*, mu_pi::fw_fs::Section*)`    |
|     10.08 KB | `patina_dxe_core::image::core_load_image()`                                                                                                        |
|      9.93 KB | `brotli_decompressor::decode::ProcessCommandsInternal<>(bool, ...)`                                                                                |
|      7.90 KB | `patina_dxe_core::driver_services::core_connect_controller(...)`                                                                                   |
|      5.89 KB | `patina_dxe_core::protocol_db::SpinLockedProtocolDb::install_protocol_interface(....)`                                                             |
|      5.61 KB | `brotli_decompressor::decode::ReadHuffmanCode<>(...)`                                                                                              |
|      5.50 KB | `AsmIdtVectorBegin`                                                                                                                                |
|      5.45 KB | `patina_dxe_core::dispatcher::core_fw_vol_event_protocol_notify(enum core::ffi::c_void*, enum core::ffi::c_void*)`                                 |
|      5.24 KB | `patina_dxe_core::dispatcher::core_dispatcher()`                                                                                                   |
|      5.11 KB | `patina_sdk::component::struct_component::impl$1::initialize<> (*)(.....,qemu_resources::q35::comp..`                                              |
|      4.45 KB | `patina_internal_cpu::interrupts::x64::interrupt_manager::page_fault_handler(int64, r_efi::protocols::debug_support::SystemContext)`               |
|      4.00 KB | `goblin::pe::debug::DebugData::parse_with_opts_and_fixup(....)`                                                                                    |
|      3.93 KB | `patina_dxe_core::pecoff::UefiPeInfo::parse(ref$<slice2$<u8> >)`                                                                                   |
|      3.78 KB | `patina_mtrr::mtrr::MtrrLib<>::mtrr_lib_set_memory_ranges<X64Hal>(....)`                                                                           |
|      3.74 KB | `core::slice::sort::unstable::quicksort::quicksort`                                                                                                |
|      3.55 KB | `patina_internal_cpu::interrupts::x64::interrupt_manager::general_protection_fault_handler(int64, r_efi::protocols::debug_support::SystemContext)` |
|      3.42 KB | `goblin::pe::tls::TlsData::parse_with_opts(...)`                                                                                                   |
|      3.35 KB | `patina_dxe_core::gcd::spin_locked_gcd::SpinLockedGcd::set_memory_space_attributes(unsigned int64, unsigned int64, unsigned int64)`                |
|      2.89 KB | `patina_dxe_core::image::core_unload_image(enum core::ffi::c_void*, bool)`                                                                         |
|      2.75 KB | `patina_dxe_core::protocol_db::SpinLockedProtocolDb::register_protocol_notify(r_efi::base::Guid, enum core::ffi::c_void*)`                         |
|      2.58 KB | `patina_dxe_core::memory_attributes_table::core_install_memory_attributes_table()`                                                                 |
|      2.55 KB | `patina_dxe_core::driver_services::core_disconnect_controller(....)`                                                                               |
|      2.45 KB | `patina_dxe_core::allocator::AllocatorMap::get_or_create_allocator(unsigned int, enum core::ffi::c_void*)`                                         |
|      2.42 KB | `enum2$<alloc::collections::btree::map::entry::Entry<...>(...)`                                                                                    |
|      2.32 KB | `goblin::pe::import::ImportData::parse_with_opts<u32>(...)`                                                                                        |
|      2.29 KB | `core::slice::sort::stable::quicksort::quicksort<>()`                                                                                              |
|      2.28 KB | `goblin::pe::import::ImportData::parse_with_opts<u64>(...)`                                                                                        |
|      2.27 KB | `patina_mtrr::mtrr::MtrrLib<patina_mtrr::hal::X64Hal>::mtrr_lib_calculate_subtractive_path<patina_mtrr::hal::X64Hal>(...)`                         |
|      2.17 KB | `patina_internal_cpu::paging::x64::apply_caching_attributes<patina_mtrr::mtrr::MtrrLib<patina_mtrr::hal::X64Hal> >(...)`                           |
|      2.12 KB | `patina_dxe_core::event_db::SpinLockedEventDb::create_event(....)`                                                                                 |
|      2.08 KB | `alloc::collections::btree::map::BTreeMap<>(....)`                                                                                                 |
|      2.06 KB | `alloc::collections::btree::map::BTreeMap<>(....)`                                                                                                 |
|      2.03 KB | `patina_dxe_core::runtime::set_virtual_address_map(...)`                                                                                           |

## .rdata Segment Details

The `.rdata` segment consists of 115 data members totaling 249.5 KB. Remarkably,
just 3 of these data items account for approximately **56% (137 KB)** of the
segment.

 | Size on disk | Name                                                 |
 | -----------: | ---------------------------------------------------- |
 |     119.9 KB | `brotli_decompressor::dictionary::kBrotliDictionary` |
 |      16.0 KB | `crc32fast::table::CRC32_TABLE`                      |
 |       2.0 KB | `brotli_decompressor::context::kContextLookup`       |

## Reducing Binary Size by Disabling Logging

The only further substantial reduction in the release binary size is observed
when logging is completely disabled, as shown below in
`\patina-dxe-core-qemu\Cargo.toml` using `release_max_level_off`. This reduces
the size from 762 KB to 599 KB — a reduction of approximately **21% (150 KB)**.

```toml
[dependencies]
log = { version = "^0.4", default-features = false, features = ["release_max_level_off"] }
```
