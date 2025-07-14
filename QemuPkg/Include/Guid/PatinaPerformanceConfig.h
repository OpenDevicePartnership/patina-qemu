/** @file
  Patina Performance Configuration HOB

  This file defines the structure for the Patina Performance Configuration HOB for QEMU platform usage.

  Copyright (c) Microsoft Corporation. All rights reserved.

  SPDX-License-Identifier: BSD-2-Clause-Patent
**/

#ifndef PATINA_PERFORMANCE_CONFIG_H_
#define PATINA_PERFORMANCE_CONFIG_H_

#define PATINA_PERFORMANCE_CONFIG_HOB_GUID \
    { 0xfd87f2d8, 0x112d, 0x4640, { 0x9c, 0x00, 0xd3, 0x7d, 0x2a, 0x1f, 0xb7, 0x5d } }

//
// This cannot be declared in any existing package declaration file because it is owned by Patina Rust code
// (not owned by C code).
//
GLOBAL_REMOVE_IF_UNREFERENCED EFI_GUID  gPatinaPerformanceConfigHobGuid = PATINA_PERFORMANCE_CONFIG_HOB_GUID;

#pragma pack(push, 1)
typedef struct {
  ///
  /// Whether the Patina Performance component should be enabled.
  ///
  BOOLEAN    Enabled;
  ///
  /// A mask for selecting measurements enabled in the Patina Performance component.
  ///
  UINT32     EnabledMeasurements;
} PATINA_PERFORMANCE_CONFIG_HOB;
#pragma pack(pop)

#endif
