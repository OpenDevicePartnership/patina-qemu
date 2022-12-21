
/** @file
*  This driver is a test driver for DxeRust FFI interfaces.
*
*  Copyright (c) Microsoft Corporation. All rights reserved.
*
**/

#include <Uefi.h>
#include <Library/DebugLib.h>
#include <Library/UefiBootServicesTableLib.h>

EFI_STATUS
EFIAPI
RustFfiTestEntry (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{
  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));
  return EFI_SUCCESS;
}