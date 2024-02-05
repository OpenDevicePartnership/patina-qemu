/** @file
*  This driver is a test driver for DxeRust FFI image interfaces.
*
*  Copyright (c) Microsoft Corporation. All rights reserved.
*
**/

#include <Uefi.h>
#include <Library/BaseMemoryLib.h>
#include <Library/DebugLib.h>
#include <Library/UefiBootServicesTableLib.h>
#include <Library/MemoryAllocationLib.h>

#define TEST_EXIT_DATA  L"TestExitDataBufferData"

EFI_STATUS
EFIAPI
RustFfiImageTestEntry (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{
  EFI_STATUS  Status;
  CHAR16      *ExitData;

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  ExitData = AllocatePool (sizeof (TEST_EXIT_DATA));
  ASSERT (ExitData != NULL);

  CopyMem (ExitData, TEST_EXIT_DATA, sizeof (TEST_EXIT_DATA));

  DEBUG ((DEBUG_INFO, "[%a] Calling Exit with ExitData %s\n", __FUNCTION__, ExitData));

  Status = gBS->Exit (ImageHandle, EFI_SUCCESS, sizeof (TEST_EXIT_DATA), ExitData);
  ASSERT_EFI_ERROR (Status);

  return EFI_DEVICE_ERROR;
}
