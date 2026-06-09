/** @file
  Installs the FF-A SSDT ACPI table.

  Reads the compiled FF-A SSDT (raw AML) from the firmware volume file identified
  by PcdFfaSsdtFileGuid and installs it through the EFI_ACPI_TABLE_PROTOCOL. The
  table is installed in addition to the platform tables that QEMU provides over
  fw_cfg.

  Copyright (c) Microsoft Corporation.
  SPDX-License-Identifier: BSD-2-Clause-Patent
**/

#include <PiDxe.h>
#include <IndustryStandard/Acpi.h>
#include <Protocol/AcpiTable.h>
#include <Library/BaseLib.h>
#include <Library/BaseMemoryLib.h>
#include <Library/DebugLib.h>
#include <Library/DxeServicesLib.h>
#include <Library/MemoryAllocationLib.h>
#include <Library/UefiBootServicesTableLib.h>
#include <Library/UefiDriverEntryPoint.h>
#include <Library/UefiLib.h>

/**
  Install the FF-A SSDT once the ACPI Table Protocol is available.

  @param[in]  Event     Event whose notification function is being invoked.
  @param[in]  Context   Pointer to the notification function's context (unused).
**/
STATIC
VOID
EFIAPI
InstallFfaSsdt (
  IN EFI_EVENT  Event,
  IN VOID       *Context
  )
{
  EFI_STATUS                   Status;
  EFI_ACPI_TABLE_PROTOCOL      *AcpiTable;
  EFI_ACPI_DESCRIPTION_HEADER  *Ssdt;
  UINTN                        SsdtSize;
  UINTN                        TableKey;

  Ssdt = NULL;

  Status = gBS->LocateProtocol (&gEfiAcpiTableProtocolGuid, NULL, (VOID **)&AcpiTable);
  if (EFI_ERROR (Status)) {
    //
    // The notify callback may run before the protocol is installed; it will be
    // invoked again once the protocol becomes available.
    //
    return;
  }

  //
  // Read the compiled SSDT (raw AML) out of this driver's own FFS file. The
  // build compiles Ffa.asl and stores it as a raw section alongside this
  // module's PE32, so it is addressed by the caller-id (module) GUID.
  //
  Status = GetSectionFromFv (
             &gEfiCallerIdGuid,
             EFI_SECTION_RAW,
             0,
             (VOID **)&Ssdt,
             &SsdtSize
             );
  if (EFI_ERROR (Status) || (Ssdt == NULL) || (SsdtSize < sizeof (*Ssdt))) {
    DEBUG ((DEBUG_ERROR, "%a: FF-A SSDT not found in FV: %r\n", __func__, Status));
    goto Done;
  }

  if (Ssdt->Length > SsdtSize) {
    DEBUG ((DEBUG_ERROR, "%a: FF-A SSDT length 0x%x exceeds section 0x%Lx\n", __func__, Ssdt->Length, (UINT64)SsdtSize));
    goto Done;
  }

  //
  // The ACPI Table Protocol re-checksums the table on install, so we do not
  // need to fix up the checksum here.
  //
  Status = AcpiTable->InstallAcpiTable (
                        AcpiTable,
                        Ssdt,
                        Ssdt->Length,
                        &TableKey
                        );
  if (EFI_ERROR (Status)) {
    DEBUG ((DEBUG_ERROR, "%a: failed to install FF-A SSDT: %r\n", __func__, Status));
  } else {
    DEBUG ((DEBUG_INFO, "%a: FF-A SSDT installed\n", __func__));
  }

Done:
  if (Event != NULL) {
    gBS->CloseEvent (Event);
  }

  if (Ssdt != NULL) {
    FreePool (Ssdt);
  }
}

/**
  Driver entry point. Registers a callback for the ACPI Table Protocol.

  @param[in]  ImageHandle  The firmware-allocated handle for the EFI image.
  @param[in]  SystemTable  A pointer to the EFI System Table.

  @retval EFI_SUCCESS  The callback was registered successfully.
**/
EFI_STATUS
EFIAPI
FfaSsdtInstallEntryPoint (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{
  VOID  *Registration;

  EfiCreateProtocolNotifyEvent (
    &gEfiAcpiTableProtocolGuid,
    TPL_CALLBACK,
    InstallFfaSsdt,
    NULL,
    &Registration
    );

  return EFI_SUCCESS;
}
