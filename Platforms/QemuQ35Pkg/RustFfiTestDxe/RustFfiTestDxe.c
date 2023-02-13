/** @file
*  This driver is a test driver for DxeRust FFI interfaces.
*
*  Copyright (c) Microsoft Corporation. All rights reserved.
*
**/

#include <Uefi.h>
#include <Library/DebugLib.h>
#include <Library/BaseMemoryLib.h>
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

VOID
TestMemoryInterface (
  VOID
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
}

VOID
TestProtocolInstallUninstallInterface (
  VOID
  )
{
  EFI_STATUS  Status;
  EFI_HANDLE  Handle1;
  EFI_HANDLE  Handle2;
  // {d4c1cc54-bf4c-44ca-8d59-dfe5c85d81f9}
  EFI_GUID  Protocol1 = {
    0xd4c1cc54, 0xbf4c, 0x44ca, { 0x8d, 0x59, 0xdf, 0xe5, 0xc8, 0x5d, 0x81, 0xf9 }
  };
  // {a007d8b1-a498-42a0-9860-555da0d7f42d}
  EFI_GUID  Protocol2 = {
    0xa007d8b1, 0xa498, 0x42a0, { 0x98, 0x60, 0x55, 0x5d, 0xa0, 0xd7, 0xf4, 0x2d }
  };
  // {ef6d39fe-02f3-4daf-a8ab-0ee59ee81e05}
  EFI_GUID  Protocol3 =  {0xef6d39fe, 0x02f3, 0x4daf, {0xa8, 0xab, 0x0e, 0xe5, 0x9e, 0xe8, 0x1e, 0x05}};

  UINTN     Data1 = 0x0415;
  UINTN     Data2 = 0x1980;
  UINTN     Data3 = 0x4A4F484E;
  VOID      *Interface1;
  VOID      *Interface2;
  VOID      *Interface3;
  VOID      *TestInterface1;
  VOID      *TestInterface2;
  VOID      *TestInterface3;

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  Interface1 = &Data1;
  Interface2 = &Data2;
  Interface3 = &Data3;

  DEBUG ((DEBUG_INFO, "[%a] Verify that protocol interfaces can be installed and located.\n", __FUNCTION__));
  Handle1     = NULL;
  Status     = gBS->InstallMultipleProtocolInterfaces (
                      &Handle1,
                      &Protocol1,
                      Interface1,
                      &Protocol2,
                      Interface2,
                      NULL
                      );
  ASSERT_EFI_ERROR (Status);
  ASSERT (Handle1 != NULL);

  Handle2 = NULL;
  Status = gBS->InstallProtocolInterface (&Handle2, &Protocol3, EFI_NATIVE_INTERFACE, Interface3);
  ASSERT_EFI_ERROR (Status);
  ASSERT (Handle2 != NULL);

  Status = gBS->LocateProtocol (&Protocol1, NULL, &TestInterface1);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestInterface1 == &Data1);

  Status = gBS->LocateProtocol (&Protocol2, NULL, &TestInterface2);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestInterface2 == &Data2);

  Status = gBS->LocateProtocol (&Protocol3, NULL, &TestInterface3);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestInterface3 == &Data3);


  DEBUG ((DEBUG_INFO, "[%a] Verify that protocol interfaces can be uninstalled.\n", __FUNCTION__));

  Status = gBS->UninstallMultipleProtocolInterfaces (
    Handle1,
    &Protocol1, Interface1,
    NULL);
  ASSERT_EFI_ERROR(Status);

  Status = gBS->UninstallProtocolInterface (Handle2, &Protocol3, Interface3);
  ASSERT_EFI_ERROR(Status);

  Status = gBS->LocateProtocol (&Protocol1, NULL, &TestInterface1);
  ASSERT(Status == EFI_NOT_FOUND);

  Status = gBS->LocateProtocol (&Protocol2, NULL, &TestInterface2);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestInterface2 == &Data2);

  Status = gBS->LocateProtocol (&Protocol3, NULL, &TestInterface3);
  ASSERT(Status == EFI_NOT_FOUND);

  DEBUG ((DEBUG_INFO, "[%a] Verify that protocol interfaces can be re-installed.\n", __FUNCTION__));

  Status = gBS->ReinstallProtocolInterface (Handle1, &Protocol2, Interface2, Interface3);
  ASSERT_EFI_ERROR (Status);

  Status = gBS->LocateProtocol (&Protocol2, NULL, &TestInterface2);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestInterface2 == &Data3);

  DEBUG ((DEBUG_INFO, "[%a] Testing Complete\n", __FUNCTION__));
}

VOID
TestHandleProtocolInterface (
  VOID
  )
{
  EFI_STATUS Status;
  EFI_HANDLE Handles[10];
  // {c08d4d5d-08b4-47a0-996b-48514feb1d56}
  EFI_GUID  Protocol1 = {0xc08d4d5d, 0x08b4, 0x47a0, {0x99, 0x6b, 0x48, 0x51, 0x4f, 0xeb, 0x1d, 0x56}};
  // {7e61a702-1a98-4275-83d7-d2962f9d8f74}
  EFI_GUID  Protocol2 = {0x7e61a702, 0x1a98, 0x4275, {0x83, 0xd7, 0xd2, 0x96, 0x2f, 0x9d, 0x8f, 0x74}};

  VOID *Interface;
  VOID *Interface2;

  UINTN Data[10];
  UINTN Data2[10];

  UINTN BufferSize;
  UINTN HandleCount;
  EFI_HANDLE *Buffer;

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  //Install protocol interfaces on all the handles
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Data[i] = i;
    Data2[i] = i+10;
    Interface = &Data[i];
    Interface2 = &Data2[i];
    Handles[i] = NULL;
    Status = gBS->InstallMultipleProtocolInterfaces(
      &Handles[i],
      &Protocol1, Interface,
      &Protocol2, Interface2,
      NULL);
    ASSERT_EFI_ERROR (Status);
  }

  DEBUG ((DEBUG_INFO, "[%a] Test that LocateHandle returns a buffer with the expected handles in it.\n", __FUNCTION__));
  BufferSize = 0;
  Status = gBS->LocateHandle (AllHandles, NULL, NULL, &BufferSize, NULL);
  ASSERT (Status == EFI_BUFFER_TOO_SMALL);

  Buffer = AllocatePool (BufferSize);
  Status = gBS->LocateHandle (AllHandles, NULL, NULL, &BufferSize, Buffer);
  ASSERT_EFI_ERROR (Status);
  //Check that all the handles are returned.
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    BOOLEAN found = FALSE;
    for (UINTN j = 0; j < BufferSize/sizeof (EFI_HANDLE); j++) {
      if (Handles[i] == Buffer[j]) {
        found = TRUE;
        break;
      }
    }
    if (!found) {
      DEBUG ((DEBUG_ERROR, "[%a] Failed to find Handle %d in the returned handle buffer.\n", __FUNCTION__, Handles[i]));
      ASSERT(FALSE);
    }
  }
  FreePool (Buffer);

  DEBUG ((DEBUG_INFO, "[%a] Test that LocateHandleBuffer returns a buffer with the expected handles in it.\n", __FUNCTION__));
  Status = gBS->LocateHandleBuffer(AllHandles, NULL, NULL, &HandleCount, &Buffer);
  ASSERT_EFI_ERROR (Status);
  //Check that all the handles are returned.
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    BOOLEAN found = FALSE;
    for (UINTN j = 0; j < HandleCount; j++) {
      if (Handles[i] == Buffer[j]) {
        found = TRUE;
        break;
      }
    }
    if (!found) {
      DEBUG ((DEBUG_ERROR, "[%a] Failed to find Handle %d in the returned handle buffer.\n", __FUNCTION__, Handles[i]));
      ASSERT(FALSE);
    }
  }
  FreePool (Buffer);

  DEBUG ((DEBUG_INFO, "[%a] Test that HandleProtocol returns the expected protocol instance.\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->HandleProtocol (Handles[i], &Protocol1, &Interface);
    ASSERT_EFI_ERROR (Status);
    ASSERT (*((UINTN *)Interface) == Data[i]);
    Status = gBS->HandleProtocol (Handles[i], &Protocol2, &Interface);
    ASSERT_EFI_ERROR (Status);
    ASSERT (*((UINTN *)Interface) == Data2[i]);
  }

  DEBUG ((DEBUG_INFO, "[%a] Test that ProtocolsPerHandle returns the expected protocol guids.\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    EFI_GUID **ProtocolBuffer;
    UINTN Count;
    Status = gBS->ProtocolsPerHandle (Handles[i], &ProtocolBuffer, &Count);
    ASSERT_EFI_ERROR (Status);
    ASSERT (Count==2);
    if (CompareGuid (&Protocol1, ProtocolBuffer[0])) {
      ASSERT (CompareGuid(&Protocol2, ProtocolBuffer[1]));
    } else if (CompareGuid (&Protocol2, ProtocolBuffer[0])) {
      ASSERT (CompareGuid(&Protocol1, ProtocolBuffer[1]));
    } else {
      DEBUG ((DEBUG_ERROR, "[%a] Unrecognized guid %g\n", __FUNCTION__, ProtocolBuffer[0]));
      ASSERT (FALSE);
    }
    FreePool(ProtocolBuffer);
  }

  DEBUG ((DEBUG_INFO, "[%a] Testing Complete\n", __FUNCTION__));
}

VOID
TestOpenCloseProtocolInterface (
  VOID
  )
{
  EFI_STATUS Status;
  EFI_HANDLE Handles[10];
  EFI_HANDLE AgentHandles[10];
  EFI_HANDLE ControllerHandles[10];
  // {c08d4d5d-08b4-47a0-996b-48514feb1d56}
  EFI_GUID  Protocol1 = {0xc08d4d5d, 0x08b4, 0x47a0, {0x99, 0x6b, 0x48, 0x51, 0x4f, 0xeb, 0x1d, 0x56}};
  // {7e61a702-1a98-4275-83d7-d2962f9d8f74}
  EFI_GUID  Protocol2 = {0x7e61a702, 0x1a98, 0x4275, {0x83, 0xd7, 0xd2, 0x96, 0x2f, 0x9d, 0x8f, 0x74}};
  // {273a0747-1c00-4b9b-9ee1-1a73bf12e9b7}
  EFI_GUID AgentProtocol = {0x273a0747, 0x1c00, 0x4b9b, {0x9e, 0xe1, 0x1a, 0x73, 0xbf, 0x12, 0xe9, 0xb7}};
  // {dd39fddb-eeae-41a7-b52b-5486162142aa}
  EFI_GUID ControllerProtocol = {0xdd39fddb, 0xeeae, 0x41a7, {0xb5, 0x2b, 0x54, 0x86, 0x16, 0x21, 0x42, 0xaa}};

  VOID *Interface;
  VOID *Interface2;


  UINTN Data[10];
  UINTN Data2[10];

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  //Install protocol interfaces on all the handles
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Data[i] = i;
    Data2[i] = i+10;
    Interface = &Data[i];
    Interface2 = &Data2[i];
    Handles[i] = NULL;
    Status = gBS->InstallMultipleProtocolInterfaces(
      &Handles[i],
      &Protocol1, Interface,
      &Protocol2, Interface2,
      NULL);
    ASSERT_EFI_ERROR (Status);

    AgentHandles[i] = NULL;
    Status = gBS->InstallProtocolInterface (&AgentHandles[i], &AgentProtocol, EFI_NATIVE_INTERFACE, Interface);
    ASSERT_EFI_ERROR (Status);

    ControllerHandles[i] = NULL;
    Status = gBS->InstallProtocolInterface (&ControllerHandles[i], &ControllerProtocol, EFI_NATIVE_INTERFACE, Interface);
    ASSERT_EFI_ERROR (Status);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol BY_DRIVER by the same agent on all handles succeeds\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
      Handles[i], &Protocol1,
      &Interface,
      AgentHandles[0],
      ControllerHandles[i],
      EFI_OPEN_PROTOCOL_BY_DRIVER);
    ASSERT_EFI_ERROR (Status);
    ASSERT (*((UINTN *)Interface) == Data[i]);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol BY_DRIVER by the same agent again on all handles returns ALREADY_STARTED\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
      Handles[i], &Protocol1,
      &Interface,
      AgentHandles[0],
      ControllerHandles[i],
      EFI_OPEN_PROTOCOL_BY_DRIVER);
    ASSERT (Status == EFI_ALREADY_STARTED);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol BY_DRIVER by a different agent on all handles returns ACCESS_DENIED\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
      Handles[i], &Protocol1,
      &Interface,
      AgentHandles[1],
      ControllerHandles[i],
      EFI_OPEN_PROTOCOL_BY_DRIVER);
    ASSERT (Status == EFI_ACCESS_DENIED);
  }

  DEBUG ((DEBUG_INFO, "[%a] CloseProtocol of the first agent on all handles succeeds\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->CloseProtocol (
      Handles[i],
      &Protocol1,
      AgentHandles[0],
      ControllerHandles[i]);
    ASSERT_EFI_ERROR (Status);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol BY_DRIVER by a different agent on all handles succeeds\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
      Handles[i], &Protocol1,
      &Interface,
      AgentHandles[1],
      ControllerHandles[i],
      EFI_OPEN_PROTOCOL_BY_DRIVER);
    ASSERT_EFI_ERROR (Status);
    ASSERT (*((UINTN *)Interface) == Data[i]);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol of a different interface BY_DRIVER by a different agent on all handles succeeds\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
      Handles[i], &Protocol2,
      &Interface,
      AgentHandles[2],
      ControllerHandles[i],
      EFI_OPEN_PROTOCOL_BY_DRIVER);
    ASSERT_EFI_ERROR (Status);
    ASSERT (*((UINTN *)Interface) == Data2[i]);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocolInformation returns correct information.\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    EFI_OPEN_PROTOCOL_INFORMATION_ENTRY *ProtocolInformation;
    UINTN ProtocolCount;
    Status = gBS->OpenProtocolInformation (
      Handles[i],
      &Protocol1,
      &ProtocolInformation,
      &ProtocolCount);
    ASSERT_EFI_ERROR (Status);
    ASSERT (ProtocolCount == 1);
    ASSERT (ProtocolInformation[0].AgentHandle == AgentHandles[1]);
    ASSERT (ProtocolInformation[0].ControllerHandle == ControllerHandles[i]);
    ASSERT (ProtocolInformation[0].Attributes == EFI_OPEN_PROTOCOL_BY_DRIVER);
    FreePool (ProtocolInformation);

    Status = gBS->OpenProtocolInformation (
      Handles[i],
      &Protocol2,
      &ProtocolInformation,
      &ProtocolCount);
    ASSERT_EFI_ERROR (Status);
    ASSERT (ProtocolCount == 1);
    ASSERT (ProtocolInformation[0].AgentHandle == AgentHandles[2]);
    ASSERT (ProtocolInformation[0].ControllerHandle == ControllerHandles[i]);
    ASSERT (ProtocolInformation[0].Attributes == EFI_OPEN_PROTOCOL_BY_DRIVER);
    FreePool (ProtocolInformation);
  }

  DEBUG ((DEBUG_INFO, "[%a] Testing Complete\n", __FUNCTION__));
}


EFI_STATUS
EFIAPI
RustFfiTestEntry (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{

  TestMemoryInterface ();
  TestProtocolInstallUninstallInterface ();
  TestHandleProtocolInterface ();
  TestOpenCloseProtocolInterface ();

  return EFI_SUCCESS;
}
