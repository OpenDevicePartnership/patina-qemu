/** @file
*  This driver is a test driver for DxeRust FFI interfaces.
*
*  Copyright (c) Microsoft Corporation. All rights reserved.
*
**/

#include <Uefi.h>
#include <Library/DebugLib.h>
#include <Library/MemoryAllocationLib.h>
#include <Library/UefiBootServicesTableLib.h>

EFI_MEMORY_TYPE  ValidMemoryTypes[] = {
  EfiLoaderCode,
  EfiLoaderData,
  EfiBootServicesCode,
  EfiBootServicesData,
  EfiRuntimeServicesCode,
  EfiRuntimeServicesData,
  EfiACPIReclaimMemory,
  EfiACPIMemoryNVS
};

EFI_STATUS
EFIAPI
RustFfiTestEntry (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{
  EFI_STATUS  Status;
  VOID        *TestBuffer;
  UINTN       Idx;

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  for (Idx = 0; Idx < ARRAY_SIZE (ValidMemoryTypes); Idx++) {
    DEBUG ((DEBUG_INFO, "[%a] Testing AllocatePool for memory type %d\n", __FUNCTION__, ValidMemoryTypes[Idx]));
    TestBuffer = NULL;
    Status     = gBS->AllocatePool (ValidMemoryTypes[Idx], 0x1234, &TestBuffer);

    ASSERT_EFI_ERROR (Status);
    ASSERT (TestBuffer != NULL);
    ASSERT (((UINTN)TestBuffer & 0x03) == 0); // Pool allocations are 8-byte aligned.

    DEBUG ((DEBUG_INFO, "[%a]   Allocated 0x1234 bytes at 0x%p\n", __FUNCTION__, TestBuffer));

    DEBUG ((DEBUG_INFO, "[%a] Testing FreePool for memory type %d\n", __FUNCTION__, ValidMemoryTypes[Idx]));

    Status = gBS->FreePool (TestBuffer);
    ASSERT_EFI_ERROR (Status);

    DEBUG ((DEBUG_INFO, "[%a] Testing AllocatePages for memory type %d\n", __FUNCTION__, ValidMemoryTypes[Idx]));
    TestBuffer = NULL;
    Status     = gBS->AllocatePages (AllocateAnyPages, ValidMemoryTypes[Idx], 0x123, (EFI_PHYSICAL_ADDRESS *)&TestBuffer);

    ASSERT_EFI_ERROR (Status);
    ASSERT (TestBuffer != NULL);
    ASSERT (((UINTN)TestBuffer & 0xFFF) == 0); // Page allocations are page aligned.

    DEBUG ((DEBUG_INFO, "[%a]   Allocated 0x123 pages at 0x%p\n", __FUNCTION__, TestBuffer));

    DEBUG ((DEBUG_INFO, "[%a] Testing FreePages for memory type %d\n", __FUNCTION__, ValidMemoryTypes[Idx]));
    Status = gBS->FreePages ((EFI_PHYSICAL_ADDRESS)TestBuffer, 0x123);
    ASSERT_EFI_ERROR (Status);
  }

  // Negative tests

  DEBUG ((DEBUG_INFO, "[%a] Attempt massive pool allocation that should fail.\n", __FUNCTION__));

  TestBuffer = AllocatePool (0x10000000000); // Allocate a Terabyte

  ASSERT (TestBuffer == NULL);

  DEBUG ((DEBUG_INFO, "[%a] Attempt massive page allocation that should fail.\n", __FUNCTION__));

  TestBuffer = AllocatePages (1 << 28); // Allocate a Terabyte of pages

  ASSERT (TestBuffer == NULL);

  DEBUG ((DEBUG_INFO, "[%a] Attempt AllocatePool with NULL buffer.\n", __FUNCTION__));

  Status = gBS->AllocatePool (EfiBootServicesData, 0x1234, NULL);
  ASSERT (Status == EFI_INVALID_PARAMETER);

  DEBUG ((DEBUG_INFO, "[%a] Attempt AllocatePool with bad memory type.\n", __FUNCTION__));

  TestBuffer = NULL;
  Status     = gBS->AllocatePool (EfiReservedMemoryType, 0x1234, &TestBuffer);
  ASSERT (Status == EFI_INVALID_PARAMETER);

  DEBUG ((DEBUG_INFO, "[%a] Attempt AllocatePages with NULL buffer.\n", __FUNCTION__));
  Status = gBS->AllocatePages (AllocateAnyPages, EfiBootServicesData, 0x123, NULL);
  ASSERT (Status == EFI_INVALID_PARAMETER);

  DEBUG ((DEBUG_INFO, "[%a] Attempt AllocatePages with bad allocation type.\n", __FUNCTION__));

  TestBuffer = NULL;
  Status     = gBS->AllocatePages (MaxAllocateType, EfiBootServicesData, 0x123, (EFI_PHYSICAL_ADDRESS *)&TestBuffer);
  ASSERT (Status == EFI_UNSUPPORTED);

  DEBUG ((DEBUG_INFO, "[%a] Attempt AllocatePages with bad memory type.\n", __FUNCTION__));
  TestBuffer = NULL;
  Status     = gBS->AllocatePages (AllocateAnyPages, EfiReservedMemoryType, 0x123, (EFI_PHYSICAL_ADDRESS *)&TestBuffer);
  ASSERT (Status == EFI_INVALID_PARAMETER);

  DEBUG ((DEBUG_INFO, "[%a] Attempt FreePool with NULL pointer.\n", __FUNCTION__));
  Status = gBS->FreePool (NULL);
  ASSERT (Status == EFI_INVALID_PARAMETER);

  DEBUG ((DEBUG_INFO, "[%a] Attempt FreePages with bad address that overflows.\n", __FUNCTION__));
  Status = gBS->FreePages (MAX_UINT64, 0x123);
  ASSERT (Status == EFI_INVALID_PARAMETER);

  DEBUG ((DEBUG_INFO, "[%a] Attempt FreePages with bad address that doesn't overflow.\n", __FUNCTION__));
  Status = gBS->FreePages (MAX_UINT64 - 0x2000, 1);
  ASSERT (Status == EFI_NOT_FOUND);

  DEBUG ((DEBUG_INFO, "[%a] Testing Complete\n", __FUNCTION__));

  return EFI_SUCCESS;
}
