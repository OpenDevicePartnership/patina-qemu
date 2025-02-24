/** @file
  Build FV related hobs for platform.

  Copyright (c) 2006 - 2013, Intel Corporation. All rights reserved.<BR>
  SPDX-License-Identifier: BSD-2-Clause-Patent

**/

#include "PiPei.h"
#include "Platform.h"
#include <Library/DebugLib.h>
#include <Library/HobLib.h>
#include <Library/PeiServicesLib.h>
#include <Library/PcdLib.h>

/**
  Publish PEI & DXE (Decompressed) Memory based FVs to let PEI
  and DXE know about them.

  @retval EFI_SUCCESS   Platform PEI FVs were initialized successfully.

**/
EFI_STATUS
PeiFvInitialization (
  VOID
  )
{
  DEBUG ((DEBUG_INFO, "Platform PEI Firmware Volume Initialization\n"));

  //
  // Create a memory allocation HOB for the PEI FV.
  //
  BuildMemoryAllocationHob (
    PcdGet32 (PcdOvmfPeiMemFvBase),
    PcdGet32 (PcdOvmfPeiMemFvSize),
    EfiBootServicesData
    );

  //
  // Let DXE know about the DXE FV
  //
  BuildFvHob (PcdGet32 (PcdOvmfDxeMemFvBase), PcdGet32 (PcdOvmfDxeMemFvSize));

  //
  // Create a memory allocation HOB for the DXE FV.
  //
  BuildMemoryAllocationHob (
    PcdGet32 (PcdOvmfDxeMemFvBase),
    PcdGet32 (PcdOvmfDxeMemFvSize),
    EfiBootServicesData
    );

  //
  // Let PEI know about the DXE FV so it can find DXE drivers
  //
  PeiServicesInstallFvInfoPpi (
    NULL,
    (VOID *)(UINTN)PcdGet32 (PcdOvmfDxeMemFvBase),
    PcdGet32 (PcdOvmfDxeMemFvSize),
    NULL,
    NULL
    );

  //
  // Let DXE know about the Rust DXE FV
  //
  BuildFvHob (PcdGet32 (PcdOvmfRustDxeMemFvBase), PcdGet32 (PcdOvmfRustDxeMemFvSize));

  //
  // Create a memory allocation HOB for the Rust DXE FV.
  //
  BuildMemoryAllocationHob (
    PcdGet32 (PcdOvmfRustDxeMemFvBase),
    PcdGet32 (PcdOvmfRustDxeMemFvSize),
    EfiBootServicesData
    );

  //
  // Let PEI know about the Rust DXE FV so it can find the Rust DXE Core
  //
  PeiServicesInstallFvInfoPpi (
    NULL,
    (VOID *)(UINTN)PcdGet32 (PcdOvmfRustDxeMemFvBase),
    PcdGet32 (PcdOvmfRustDxeMemFvSize),
    NULL,
    NULL
    );

  return EFI_SUCCESS;
}
