/** @file
*
*  Copyright (c) 2019, Linaro Limited. All rights reserved.
*  Copyright (c) Microsoft Corporation.
*
*  SPDX-License-Identifier: BSD-2-Clause-Patent
*
**/

#include <IndustryStandard/ArmStdSmc.h>

#include <Library/ArmLib.h>
#include <Library/ArmMonitorLib.h>
#include <Library/ArmPlatformLib.h>
#include <Library/BaseMemoryLib.h>
#include <Library/DebugLib.h>
#include <Library/MemoryAllocationLib.h>

#include <Ppi/ArmMpCoreInfo.h>

//
// QEMU SBSA exposes the CPU topology through TF-A SiP service calls. Use the
// same SMC IDs as SbsaQemuAcpiDxe so the firmware has a single source of
// truth for both the PEI MP core info table and the runtime ACPI tables.
//   SIP_SVC_GET_CPU_COUNT  -> Arg1 = number of CPUs
//   SIP_SVC_GET_CPU_NODE   -> Arg2 = MPIDR of the requested CPU index
//
#define SIP_SVC_GET_CPU_COUNT  SMC_SIP_FUNCTION_ID (200)
#define SIP_SVC_GET_CPU_NODE   SMC_SIP_FUNCTION_ID (201)
#define SMC_SIP_CALL_SUCCESS   SMC_ARCH_CALL_SUCCESS

//
// This library is linked into XIP code, so it cannot keep mutable globals
// (writable .data is not available pre-PEI). Each invocation of
// PrePeiCoreGetMpCoreInfo() therefore re-queries TF-A and allocates a fresh
// ARM_CORE_INFO table from the PEI memory pool.
//

/**
  Query TF-A for the total number of CPUs in the system via the QEMU SBSA
  SiP service.

  @param[out]  CpuCount  On success, the number of CPUs reported by TF-A.

  @retval EFI_SUCCESS       The CPU count was retrieved successfully.
  @retval EFI_DEVICE_ERROR  The SiP call failed or reported no CPUs.
**/
STATIC
EFI_STATUS
SbsaQemuGetCpuCount (
  OUT UINT32  *CpuCount
  )
{
  ARM_MONITOR_ARGS  SmcArgs;

  ZeroMem (&SmcArgs, sizeof (SmcArgs));
  SmcArgs.Arg0 = SIP_SVC_GET_CPU_COUNT;
  ArmMonitorCall (&SmcArgs);

  if (SmcArgs.Arg0 != SMC_SIP_CALL_SUCCESS) {
    DEBUG ((
      DEBUG_ERROR,
      "%a: SIP_SVC_GET_CPU_COUNT call failed (status=0x%lx).\n",
      __func__,
      (UINT64)SmcArgs.Arg0
      ));
    return EFI_DEVICE_ERROR;
  }

  if (SmcArgs.Arg1 == 0) {
    DEBUG ((DEBUG_ERROR, "%a: TF-A reported zero CPUs.\n", __func__));
    return EFI_DEVICE_ERROR;
  }

  *CpuCount = (UINT32)SmcArgs.Arg1;
  DEBUG ((DEBUG_INFO, "%a: TF-A reports %u CPU(s).\n", __func__, *CpuCount));
  return EFI_SUCCESS;
}

/**
  Query TF-A for the MPIDR of a single CPU via the QEMU SBSA SiP service.

  @param[in]   CpuId  Flat CPU index (0..CpuCount-1).
  @param[out]  Mpidr  On success, the MPIDR value reported by TF-A.

  @retval EFI_SUCCESS       The MPIDR was retrieved successfully.
  @retval EFI_DEVICE_ERROR  The SiP call failed.
**/
STATIC
EFI_STATUS
SbsaQemuGetMpidr (
  IN  UINTN   CpuId,
  OUT UINT64  *Mpidr
  )
{
  ARM_MONITOR_ARGS  SmcArgs;

  ZeroMem (&SmcArgs, sizeof (SmcArgs));
  SmcArgs.Arg0 = SIP_SVC_GET_CPU_NODE;
  SmcArgs.Arg1 = CpuId;
  ArmMonitorCall (&SmcArgs);

  if (SmcArgs.Arg0 != SMC_SIP_CALL_SUCCESS) {
    DEBUG ((
      DEBUG_ERROR,
      "%a: SIP_SVC_GET_CPU_NODE failed for CPU %u (status=0x%lx).\n",
      __func__,
      (UINT32)CpuId,
      (UINT64)SmcArgs.Arg0
      ));
    return EFI_DEVICE_ERROR;
  }

  *Mpidr = (UINT64)SmcArgs.Arg2;
  return EFI_SUCCESS;
}

/**
  Build the per-CPU ARM_CORE_INFO table dynamically, sized to the number of
  CPUs reported by TF-A, with MPIDR values fetched via the QEMU SBSA SiP
  service.

  A fresh table is allocated on every call (this library runs XIP and cannot
  use mutable globals to cache state). The caller of the
  ARM_MP_CORE_INFO_PPI copies the contents into a HOB, so the buffer is only
  required to outlive the GetMpCoreInfo() invocation.

  @param[out]  CoreCount     Number of entries written into the returned table.
  @param[out]  ArmCoreTable  Pointer to the freshly allocated ARM_CORE_INFO
                             table.

  @retval EFI_SUCCESS            Table successfully built.
  @retval EFI_DEVICE_ERROR       A required SiP call failed.
  @retval EFI_OUT_OF_RESOURCES   Failed to allocate memory for the table.
**/
STATIC
EFI_STATUS
BuildArmPlatformMpCoreInfoTable (
  OUT UINTN          *CoreCount,
  OUT ARM_CORE_INFO  **ArmCoreTable
  )
{
  EFI_STATUS     Status;
  UINT32         NumCpus;
  UINTN          Index;
  ARM_CORE_INFO  *Table;

  Status = SbsaQemuGetCpuCount (&NumCpus);
  if (EFI_ERROR (Status)) {
    return Status;
  }

  Table = AllocateZeroPool (sizeof (ARM_CORE_INFO) * NumCpus);
  if (Table == NULL) {
    DEBUG ((
      DEBUG_ERROR,
      "%a: Failed to allocate ARM_CORE_INFO table for %u CPUs.\n",
      __func__,
      NumCpus
      ));
    return EFI_OUT_OF_RESOURCES;
  }

  for (Index = 0; Index < NumCpus; Index++) {
    Status = SbsaQemuGetMpidr (Index, &Table[Index].Mpidr);
    if (EFI_ERROR (Status)) {
      FreePool (Table);
      return Status;
    }

    //
    // QEMU SBSA does not expose a poll-based mailbox; secondary cores are
    // released via PSCI. Mark the mailbox addresses as unused.
    //
    Table[Index].MailboxSetAddress   = (EFI_PHYSICAL_ADDRESS)0;
    Table[Index].MailboxGetAddress   = (EFI_PHYSICAL_ADDRESS)0;
    Table[Index].MailboxClearAddress = (EFI_PHYSICAL_ADDRESS)0;
    Table[Index].MailboxClearValue   = (UINT64)0xFFFFFFFF;
  }

  *CoreCount    = NumCpus;
  *ArmCoreTable = Table;
  return EFI_SUCCESS;
}

/**
  Return the current Boot Mode

  This function returns the boot reason on the platform

**/
EFI_BOOT_MODE
ArmPlatformGetBootMode (
  VOID
  )
{
  return BOOT_WITH_FULL_CONFIGURATION;
}

/**
  Initialize controllers that must setup in the normal world

  This function is called by the ArmPlatformPkg/PrePi or ArmPlatformPkg/PlatformPei
  in the PEI phase.

**/
RETURN_STATUS
ArmPlatformInitialize (
  IN  UINTN  MpId
  )
{
  return RETURN_SUCCESS;
}

EFI_STATUS
PrePeiCoreGetMpCoreInfo (
  OUT UINTN          *CoreCount,
  OUT ARM_CORE_INFO  **ArmCoreTable
  )
{
  if (!ArmIsMpCore ()) {
    return EFI_UNSUPPORTED;
  }

  return BuildArmPlatformMpCoreInfoTable (CoreCount, ArmCoreTable);
}

ARM_MP_CORE_INFO_PPI  mMpCoreInfoPpi = { PrePeiCoreGetMpCoreInfo };

EFI_PEI_PPI_DESCRIPTOR  gPlatformPpiTable[] = {
  {
    EFI_PEI_PPI_DESCRIPTOR_PPI,
    &gArmMpCoreInfoPpiGuid,
    &mMpCoreInfoPpi
  }
};

VOID
ArmPlatformGetPlatformPpiList (
  OUT UINTN                   *PpiListSize,
  OUT EFI_PEI_PPI_DESCRIPTOR  **PpiList
  )
{
  if (ArmIsMpCore ()) {
    *PpiListSize = sizeof (gPlatformPpiTable);
    *PpiList     = gPlatformPpiTable;
  } else {
    *PpiListSize = 0;
    *PpiList     = NULL;
  }
}
