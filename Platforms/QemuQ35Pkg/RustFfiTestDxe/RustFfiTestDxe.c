/** @file
*  This driver is a test driver for DxeRust FFI interfaces.
*
*  Copyright (c) Microsoft Corporation. All rights reserved.
*
**/

#include <Uefi.h>
#include <Protocol/Timer.h>
#include <Protocol/DevicePath.h>
#include <Library/DebugLib.h>
#include <Library/DevicePathLib.h>
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
  EFI_GUID  Protocol3 =  {
    0xef6d39fe, 0x02f3, 0x4daf, { 0xa8, 0xab, 0x0e, 0xe5, 0x9e, 0xe8, 0x1e, 0x05 }
  };

  UINTN  Data1 = 0x0415;
  UINTN  Data2 = 0x1980;
  UINTN  Data3 = 0x4A4F484E;
  VOID   *Interface1;
  VOID   *Interface2;
  VOID   *Interface3;
  VOID   *TestInterface1;
  VOID   *TestInterface2;
  VOID   *TestInterface3;

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  Interface1 = &Data1;
  Interface2 = &Data2;
  Interface3 = &Data3;

  DEBUG ((DEBUG_INFO, "[%a] Verify that protocol interfaces can be installed and located.\n", __FUNCTION__));
  Handle1 = NULL;
  Status  = gBS->InstallMultipleProtocolInterfaces (
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
  Status  = gBS->InstallProtocolInterface (&Handle2, &Protocol3, EFI_NATIVE_INTERFACE, Interface3);
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
                  &Protocol1,
                  Interface1,
                  NULL
                  );
  ASSERT_EFI_ERROR (Status);

  Status = gBS->UninstallProtocolInterface (Handle2, &Protocol3, Interface3);
  ASSERT_EFI_ERROR (Status);

  Status = gBS->LocateProtocol (&Protocol1, NULL, &TestInterface1);
  ASSERT (Status == EFI_NOT_FOUND);

  Status = gBS->LocateProtocol (&Protocol2, NULL, &TestInterface2);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestInterface2 == &Data2);

  Status = gBS->LocateProtocol (&Protocol3, NULL, &TestInterface3);
  ASSERT (Status == EFI_NOT_FOUND);

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
  EFI_STATUS  Status;
  EFI_HANDLE  Handles[10];
  // {c08d4d5d-08b4-47a0-996b-48514feb1d56}
  EFI_GUID  Protocol1 = {
    0xc08d4d5d, 0x08b4, 0x47a0, { 0x99, 0x6b, 0x48, 0x51, 0x4f, 0xeb, 0x1d, 0x56 }
  };
  // {7e61a702-1a98-4275-83d7-d2962f9d8f74}
  EFI_GUID  Protocol2 = {
    0x7e61a702, 0x1a98, 0x4275, { 0x83, 0xd7, 0xd2, 0x96, 0x2f, 0x9d, 0x8f, 0x74 }
  };

  VOID  *Interface;
  VOID  *Interface2;

  UINTN  Data[10];
  UINTN  Data2[10];

  UINTN       BufferSize;
  UINTN       HandleCount;
  EFI_HANDLE  *Buffer;

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  // Install protocol interfaces on all the handles
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Data[i]    = i;
    Data2[i]   = i+10;
    Interface  = &Data[i];
    Interface2 = &Data2[i];
    Handles[i] = NULL;
    Status     = gBS->InstallMultipleProtocolInterfaces (
                        &Handles[i],
                        &Protocol1,
                        Interface,
                        &Protocol2,
                        Interface2,
                        NULL
                        );
    ASSERT_EFI_ERROR (Status);
  }

  DEBUG ((DEBUG_INFO, "[%a] Test that LocateHandle returns a buffer with the expected handles in it.\n", __FUNCTION__));
  BufferSize = 0;
  Status     = gBS->LocateHandle (AllHandles, NULL, NULL, &BufferSize, NULL);
  ASSERT (Status == EFI_BUFFER_TOO_SMALL);

  Buffer = AllocatePool (BufferSize);
  Status = gBS->LocateHandle (AllHandles, NULL, NULL, &BufferSize, Buffer);
  ASSERT_EFI_ERROR (Status);
  // Check that all the handles are returned.
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    BOOLEAN  found = FALSE;
    for (UINTN j = 0; j < BufferSize/sizeof (EFI_HANDLE); j++) {
      if (Handles[i] == Buffer[j]) {
        found = TRUE;
        break;
      }
    }

    if (!found) {
      DEBUG ((DEBUG_ERROR, "[%a] Failed to find Handle %d in the returned handle buffer.\n", __FUNCTION__, Handles[i]));
      ASSERT (FALSE);
    }
  }

  FreePool (Buffer);

  DEBUG ((DEBUG_INFO, "[%a] Test that LocateHandleBuffer returns a buffer with the expected handles in it.\n", __FUNCTION__));
  Status = gBS->LocateHandleBuffer (AllHandles, NULL, NULL, &HandleCount, &Buffer);
  ASSERT_EFI_ERROR (Status);
  // Check that all the handles are returned.
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    BOOLEAN  found = FALSE;
    for (UINTN j = 0; j < HandleCount; j++) {
      if (Handles[i] == Buffer[j]) {
        found = TRUE;
        break;
      }
    }

    if (!found) {
      DEBUG ((DEBUG_ERROR, "[%a] Failed to find Handle %d in the returned handle buffer.\n", __FUNCTION__, Handles[i]));
      ASSERT (FALSE);
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
    EFI_GUID  **ProtocolBuffer;
    UINTN     Count;
    Status = gBS->ProtocolsPerHandle (Handles[i], &ProtocolBuffer, &Count);
    ASSERT_EFI_ERROR (Status);
    ASSERT (Count == 2);
    if (CompareGuid (&Protocol1, ProtocolBuffer[0])) {
      ASSERT (CompareGuid (&Protocol2, ProtocolBuffer[1]));
    } else if (CompareGuid (&Protocol2, ProtocolBuffer[0])) {
      ASSERT (CompareGuid (&Protocol1, ProtocolBuffer[1]));
    } else {
      DEBUG ((DEBUG_ERROR, "[%a] Unrecognized guid %g\n", __FUNCTION__, ProtocolBuffer[0]));
      ASSERT (FALSE);
    }

    FreePool (ProtocolBuffer);
  }

  DEBUG ((DEBUG_INFO, "[%a] Testing Complete\n", __FUNCTION__));
}

VOID
TestOpenCloseProtocolInterface (
  VOID
  )
{
  EFI_STATUS  Status;
  EFI_HANDLE  Handles[10];
  EFI_HANDLE  AgentHandles[10];
  EFI_HANDLE  ControllerHandles[10];
  // {c08d4d5d-08b4-47a0-996b-48514feb1d56}
  EFI_GUID  Protocol1 = {
    0xc08d4d5d, 0x08b4, 0x47a0, { 0x99, 0x6b, 0x48, 0x51, 0x4f, 0xeb, 0x1d, 0x56 }
  };
  // {7e61a702-1a98-4275-83d7-d2962f9d8f74}
  EFI_GUID  Protocol2 = {
    0x7e61a702, 0x1a98, 0x4275, { 0x83, 0xd7, 0xd2, 0x96, 0x2f, 0x9d, 0x8f, 0x74 }
  };
  // {273a0747-1c00-4b9b-9ee1-1a73bf12e9b7}
  EFI_GUID  AgentProtocol = {
    0x273a0747, 0x1c00, 0x4b9b, { 0x9e, 0xe1, 0x1a, 0x73, 0xbf, 0x12, 0xe9, 0xb7 }
  };
  // {dd39fddb-eeae-41a7-b52b-5486162142aa}
  EFI_GUID  ControllerProtocol = {
    0xdd39fddb, 0xeeae, 0x41a7, { 0xb5, 0x2b, 0x54, 0x86, 0x16, 0x21, 0x42, 0xaa }
  };

  VOID  *Interface;
  VOID  *Interface2;

  UINTN  Data[10];
  UINTN  Data2[10];

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  // Install protocol interfaces on all the handles
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Data[i]    = i;
    Data2[i]   = i+10;
    Interface  = &Data[i];
    Interface2 = &Data2[i];
    Handles[i] = NULL;
    Status     = gBS->InstallMultipleProtocolInterfaces (
                        &Handles[i],
                        &Protocol1,
                        Interface,
                        &Protocol2,
                        Interface2,
                        NULL
                        );
    ASSERT_EFI_ERROR (Status);

    AgentHandles[i] = NULL;
    Status          = gBS->InstallProtocolInterface (&AgentHandles[i], &AgentProtocol, EFI_NATIVE_INTERFACE, Interface);
    ASSERT_EFI_ERROR (Status);

    ControllerHandles[i] = NULL;
    Status               = gBS->InstallProtocolInterface (&ControllerHandles[i], &ControllerProtocol, EFI_NATIVE_INTERFACE, Interface);
    ASSERT_EFI_ERROR (Status);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol BY_DRIVER by the same agent on all handles succeeds\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
                    Handles[i],
                    &Protocol1,
                    &Interface,
                    AgentHandles[0],
                    ControllerHandles[i],
                    EFI_OPEN_PROTOCOL_BY_DRIVER
                    );
    ASSERT_EFI_ERROR (Status);
    ASSERT (*((UINTN *)Interface) == Data[i]);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol BY_DRIVER by the same agent again on all handles returns ALREADY_STARTED\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
                    Handles[i],
                    &Protocol1,
                    &Interface,
                    AgentHandles[0],
                    ControllerHandles[i],
                    EFI_OPEN_PROTOCOL_BY_DRIVER
                    );
    ASSERT (Status == EFI_ALREADY_STARTED);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol BY_DRIVER by a different agent on all handles returns ACCESS_DENIED\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
                    Handles[i],
                    &Protocol1,
                    &Interface,
                    AgentHandles[1],
                    ControllerHandles[i],
                    EFI_OPEN_PROTOCOL_BY_DRIVER
                    );
    ASSERT (Status == EFI_ACCESS_DENIED);
  }

  DEBUG ((DEBUG_INFO, "[%a] CloseProtocol of the first agent on all handles succeeds\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->CloseProtocol (
                    Handles[i],
                    &Protocol1,
                    AgentHandles[0],
                    ControllerHandles[i]
                    );
    ASSERT_EFI_ERROR (Status);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol BY_DRIVER by a different agent on all handles succeeds\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
                    Handles[i],
                    &Protocol1,
                    &Interface,
                    AgentHandles[1],
                    ControllerHandles[i],
                    EFI_OPEN_PROTOCOL_BY_DRIVER
                    );
    ASSERT_EFI_ERROR (Status);
    ASSERT (*((UINTN *)Interface) == Data[i]);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocol of a different interface BY_DRIVER by a different agent on all handles succeeds\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    Status = gBS->OpenProtocol (
                    Handles[i],
                    &Protocol2,
                    &Interface,
                    AgentHandles[2],
                    ControllerHandles[i],
                    EFI_OPEN_PROTOCOL_BY_DRIVER
                    );
    ASSERT_EFI_ERROR (Status);
    ASSERT (*((UINTN *)Interface) == Data2[i]);
  }

  DEBUG ((DEBUG_INFO, "[%a] OpenProtocolInformation returns correct information.\n", __FUNCTION__));
  for (UINTN i = 0; i < ARRAY_SIZE (Handles); i++) {
    EFI_OPEN_PROTOCOL_INFORMATION_ENTRY  *ProtocolInformation;
    UINTN                                ProtocolCount;
    Status = gBS->OpenProtocolInformation (
                    Handles[i],
                    &Protocol1,
                    &ProtocolInformation,
                    &ProtocolCount
                    );
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
                    &ProtocolCount
                    );
    ASSERT_EFI_ERROR (Status);
    ASSERT (ProtocolCount == 1);
    ASSERT (ProtocolInformation[0].AgentHandle == AgentHandles[2]);
    ASSERT (ProtocolInformation[0].ControllerHandle == ControllerHandles[i]);
    ASSERT (ProtocolInformation[0].Attributes == EFI_OPEN_PROTOCOL_BY_DRIVER);
    FreePool (ProtocolInformation);
  }

  DEBUG ((DEBUG_INFO, "[%a] Testing Complete\n", __FUNCTION__));
}

#define EVENT_TEST_CONTEXT_SIG  SIGNATURE_32('e','t','s','t')
typedef enum {
  NotifySignal,
  NotifyWait,
  ProtocolNotify,
  TimerNotify,
} EVENT_TEST_TYPE;

typedef struct {
  UINT32             Signature;
  EVENT_TEST_TYPE    TestType;
  BOOLEAN            Signalled;
  BOOLEAN            Handled;
  EFI_EVENT          EventOrder[2];
  UINTN              WaitCycles;
  EFI_EVENT          WaitEventToSignal;
  EFI_GUID           *TestProtocol;
  VOID               *RegistrationKey;
  EFI_HANDLE         Handle;
} EVENT_TEST_CONTEXT;

EVENT_TEST_CONTEXT  mTestContext = {
  .Signature = EVENT_TEST_CONTEXT_SIG
};
EFI_EVENT           mTestEvent;
EFI_EVENT           mTestEvent2;
EFI_EVENT           mTestEvent3;

VOID
EFIAPI
EventNotifyCallback (
  IN EFI_EVENT  Event,
  VOID          *Context
  )
{
  EFI_STATUS          Status;
  EVENT_TEST_CONTEXT  *TestContext;
  UINTN               Idx;
  UINTN               HandleCount;
  EFI_HANDLE          *HandleBuffer;

  ASSERT (Context != NULL);
  TestContext = (EVENT_TEST_CONTEXT *)Context;
  ASSERT (TestContext == &mTestContext);
  ASSERT (TestContext->Signature == EVENT_TEST_CONTEXT_SIG);
  TestContext->Handled = TRUE;

  switch (TestContext->TestType) {
    case NotifySignal:
      for (Idx = 0; Idx < ARRAY_SIZE (TestContext->EventOrder); Idx++) {
        if (TestContext->EventOrder[Idx] == 0) {
          TestContext->EventOrder[Idx] = Event;
          break;
        }
      }

      ASSERT (Idx < ARRAY_SIZE (TestContext->EventOrder));
      break;
    case NotifyWait:
      if (TestContext->WaitCycles == 0) {
        Status = gBS->SignalEvent (TestContext->WaitEventToSignal);
        ASSERT_EFI_ERROR (Status);
      } else {
        TestContext->WaitCycles--;
      }

      break;
    case ProtocolNotify:
      Status = gBS->LocateHandleBuffer (ByRegisterNotify, TestContext->TestProtocol, TestContext->RegistrationKey, &HandleCount, &HandleBuffer);
      ASSERT_EFI_ERROR (Status);
      ASSERT (HandleCount == 1);
      TestContext->Handle = HandleBuffer[0];
      break;
    case TimerNotify:
      break;
  }
}

VOID
TestEventing (
  VOID
  )
{
  EFI_STATUS  Status;

  // {07bad930-66f4-4442-80d5-59b21410a3fa}
  EFI_GUID  EventGroup = {
    0x07bad930, 0x66f4, 0x4442, { 0x80, 0xd5, 0x59, 0xb2, 0x14, 0x10, 0xa3, 0xfa }
  };

  // {8e5b5f58-5545-4790-818b-2a288f99567f}
  EFI_GUID  TestProtocol = {
    0x8e5b5f58, 0x5545, 0x4790, { 0x81, 0x8b, 0x2a, 0x28, 0x8f, 0x99, 0x56, 0x7f }
  };

  VOID        *Registration;
  EFI_HANDLE  Handle;

  DEBUG ((DEBUG_INFO, "[%a] Entry\n", __FUNCTION__));

  DEBUG ((DEBUG_INFO, "[%a] CreateEvent creates an event.\n", __FUNCTION__));

  Status = gBS->CreateEvent (EVT_NOTIFY_SIGNAL, TPL_CALLBACK, EventNotifyCallback, &mTestContext, &mTestEvent);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent != 0);

  DEBUG ((DEBUG_INFO, "[%a] SignalEvent signals an event.\n", __FUNCTION__));
  mTestContext.Handled   = FALSE;
  mTestContext.Signalled = TRUE;
  mTestContext.TestType  = NotifySignal;
  Status                 = gBS->SignalEvent (mTestEvent);
  ASSERT_EFI_ERROR (Status);

  // SignalEvent ensures signalled events dispatched before return (respecting current TPL).
  // This is not a spec requirement; if we were strict here, a raise/restore tpl or timer would be needed
  // to ensure pending event notifies are dispatched.

  ASSERT (mTestContext.Signature == EVENT_TEST_CONTEXT_SIG);
  ASSERT (mTestContext.Signalled == TRUE);
  ASSERT (mTestContext.Handled == TRUE);

  DEBUG ((DEBUG_INFO, "[%a] CloseEvent prevents an event from being signalled.\n", __FUNCTION__));
  Status = gBS->CloseEvent (mTestEvent);
  ASSERT_EFI_ERROR (Status);

  mTestContext.Handled   = FALSE;
  mTestContext.Signalled = TRUE;
  Status                 = gBS->SignalEvent (mTestEvent);
  ASSERT (EFI_ERROR (Status));

  ASSERT (mTestContext.Signature == EVENT_TEST_CONTEXT_SIG);
  ASSERT (mTestContext.Signalled == TRUE);
  ASSERT (mTestContext.Handled == FALSE);

  DEBUG ((DEBUG_INFO, "[%a] EventGroups should be notified and dispatched in TPL order when signalled.\n", __FUNCTION__));
  mTestEvent = 0;
  Status     = gBS->CreateEventEx (EVT_NOTIFY_SIGNAL, TPL_CALLBACK, EventNotifyCallback, &mTestContext, &EventGroup, &mTestEvent);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent != 0);

  mTestEvent2 = 0;
  Status      = gBS->CreateEventEx (EVT_NOTIFY_SIGNAL, TPL_NOTIFY, EventNotifyCallback, &mTestContext, &EventGroup, &mTestEvent2);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent2 != 0);
  ASSERT (mTestEvent != mTestEvent2);

  mTestContext.Handled       = FALSE;
  mTestContext.Signalled     = TRUE;
  mTestContext.EventOrder[0] = 0;
  mTestContext.EventOrder[1] = 0;
  Status                     = gBS->SignalEvent (mTestEvent);
  ASSERT_EFI_ERROR (Status);

  ASSERT (mTestContext.Signature == EVENT_TEST_CONTEXT_SIG);
  ASSERT (mTestContext.Signalled == TRUE);
  ASSERT (mTestContext.Handled == TRUE);
  ASSERT (mTestContext.EventOrder[0] == mTestEvent2); // TPL_NOTIFY first
  ASSERT (mTestContext.EventOrder[1] == mTestEvent);  // TPL_CALLBACK second.

  Status = gBS->CloseEvent (mTestEvent);
  ASSERT_EFI_ERROR (Status);
  Status = gBS->CloseEvent (mTestEvent2);
  ASSERT_EFI_ERROR (Status);

  DEBUG ((DEBUG_INFO, "[%a] Test Wait For Event loop\n", __FUNCTION__));
  mTestEvent = 0;
  Status     = gBS->CreateEventEx (EVT_NOTIFY_WAIT, TPL_CALLBACK, EventNotifyCallback, &mTestContext, NULL, &mTestEvent);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent != 0);

  mTestEvent2 = 0;
  Status      = gBS->CreateEventEx (EVT_NOTIFY_WAIT, TPL_NOTIFY, EventNotifyCallback, &mTestContext, NULL, &mTestEvent2);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent2 != 0);
  ASSERT (mTestEvent != mTestEvent2);

  mTestEvent3 = 0;
  Status      = gBS->CreateEventEx (EVT_NOTIFY_WAIT, TPL_NOTIFY, EventNotifyCallback, &mTestContext, NULL, &mTestEvent3);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent3 != 0);
  ASSERT (mTestEvent != mTestEvent3);

  EFI_HANDLE  HandleList[] = {
    mTestEvent,
    mTestEvent2,
    mTestEvent3,
  };
  UINTN       Index = 0;

  mTestContext.Signalled = TRUE;
  mTestContext.TestType  = NotifyWait;

  mTestContext.WaitCycles        = 15;
  mTestContext.WaitEventToSignal = mTestEvent2;

  Status = gBS->WaitForEvent (3, HandleList, &Index);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestContext.WaitCycles == 0);
  ASSERT (Index == 1);

  Status = gBS->CloseEvent (mTestEvent);
  ASSERT_EFI_ERROR (Status);
  Status = gBS->CloseEvent (mTestEvent2);
  ASSERT_EFI_ERROR (Status);
  Status = gBS->CloseEvent (mTestEvent3);
  ASSERT_EFI_ERROR (Status);

  DEBUG ((DEBUG_INFO, "[%a] Test RegisterProtocolNotify\n", __FUNCTION__));
  Status = gBS->CreateEvent (EVT_NOTIFY_SIGNAL, TPL_CALLBACK, EventNotifyCallback, &mTestContext, &mTestEvent);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent != 0);

  Status = gBS->RegisterProtocolNotify (&TestProtocol, mTestEvent, &Registration);
  ASSERT_EFI_ERROR (Status);

  mTestContext.Signalled       = TRUE;
  mTestContext.Handled         = FALSE;
  mTestContext.TestType        = ProtocolNotify;
  mTestContext.TestProtocol    = &TestProtocol;
  mTestContext.RegistrationKey = Registration;

  Handle = NULL;
  Status = gBS->InstallProtocolInterface (&Handle, &TestProtocol, EFI_NATIVE_INTERFACE, NULL);
  ASSERT_EFI_ERROR (Status);

  ASSERT (mTestContext.Handled == TRUE);
  ASSERT (mTestContext.Handle == Handle);

  Status = gBS->CloseEvent (mTestEvent);

  DEBUG ((DEBUG_INFO, "[%a] Testing Complete\n", __FUNCTION__));
}

EFI_TIMER_NOTIFY  mTimerNotifyFunction = NULL;

EFI_STATUS
EFIAPI
TimerRegisterHandler (
  IN EFI_TIMER_ARCH_PROTOCOL  *This,
  IN EFI_TIMER_NOTIFY         NotifyFunction
  )
{
  mTimerNotifyFunction = NotifyFunction;
  DEBUG ((DEBUG_INFO, "[%a] registered notify function %p\n", __FUNCTION__, NotifyFunction));
  return EFI_SUCCESS;
}

EFI_STATUS
EFIAPI
SetTimerPeriod (
  IN EFI_TIMER_ARCH_PROTOCOL  *This,
  IN UINT64                   TimerPeriod
  )
{
  return EFI_UNSUPPORTED;
}

EFI_STATUS
EFIAPI
GetTimerPeriod (
  IN EFI_TIMER_ARCH_PROTOCOL  *This,
  OUT UINT64                  *TimerPeriod
  )
{
  return EFI_UNSUPPORTED;
}

EFI_STATUS
EFIAPI
GenerateSoftInterrupt (
  IN EFI_TIMER_ARCH_PROTOCOL  *This
  )
{
  return EFI_UNSUPPORTED;
}

EFI_TIMER_ARCH_PROTOCOL  MockTimer = {
  TimerRegisterHandler,
  SetTimerPeriod,
  GetTimerPeriod,
  GenerateSoftInterrupt
};

VOID
TestTimerEvents (
  VOID
  )
{
  EFI_STATUS  Status;
  EFI_HANDLE  Handle;

  DEBUG ((DEBUG_INFO, "[%a] Installing Architectural Timer Mock implementation.\n", __FUNCTION__));
  Handle = NULL;
  Status = gBS->InstallProtocolInterface (&Handle, &gEfiTimerArchProtocolGuid, EFI_NATIVE_INTERFACE, &MockTimer);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTimerNotifyFunction != NULL);

  DEBUG ((DEBUG_INFO, "[%a] Verifying TimerRelative Events are fired.\n", __FUNCTION__));

  Status = gBS->CreateEvent (EVT_NOTIFY_SIGNAL | EVT_TIMER, TPL_CALLBACK, EventNotifyCallback, &mTestContext, &mTestEvent);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent != 0);

  Status = gBS->SetTimer (mTestEvent, TimerRelative, 1000);
  ASSERT_EFI_ERROR (Status);

  mTestContext.TestType  = TimerNotify;
  mTestContext.Signalled = TRUE;
  mTestContext.Handled   = FALSE;

  // Tick, but not enough to trigger event.
  mTimerNotifyFunction (100);
  ASSERT (mTestContext.Handled == FALSE);

  // Tick again, enough to trigger event.
  mTimerNotifyFunction (900);
  ASSERT (mTestContext.Handled == TRUE);

  Status = gBS->CloseEvent (mTestEvent);
  ASSERT_EFI_ERROR (Status);

  DEBUG ((DEBUG_INFO, "[%a] Verifying that TimerPeriodic Events are fired.\n", __FUNCTION__));

  Status = gBS->CreateEvent (EVT_NOTIFY_SIGNAL | EVT_TIMER, TPL_CALLBACK, EventNotifyCallback, &mTestContext, &mTestEvent);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent != 0);

  Status = gBS->SetTimer (mTestEvent, TimerPeriodic, 500);
  ASSERT_EFI_ERROR (Status);

  mTestContext.TestType  = TimerNotify;
  mTestContext.Signalled = TRUE;
  mTestContext.Handled   = FALSE;

  // Tick, but not enough to trigger event.
  mTimerNotifyFunction (100);
  ASSERT (mTestContext.Handled == FALSE);

  // Tick again, enough to trigger event.
  mTimerNotifyFunction (400);
  ASSERT (mTestContext.Handled == TRUE);

  mTestContext.Handled = FALSE;

  // tick again, not enough to trigger
  mTimerNotifyFunction (100);
  ASSERT (mTestContext.Handled == FALSE);

  // tick again, enough to trigger
  mTimerNotifyFunction (400);
  ASSERT (mTestContext.Handled == TRUE);

  mTestContext.Handled = FALSE;
  // close the event.
  Status = gBS->CloseEvent (mTestEvent);
  ASSERT_EFI_ERROR (Status);

  // tick again, enough to trigger
  mTimerNotifyFunction (1000);
  ASSERT (mTestContext.Handled == FALSE);

  DEBUG ((DEBUG_INFO, "[%a] Verify that TimerCancel shuts down timers.\n", __FUNCTION__));
  Status = gBS->CreateEvent (EVT_NOTIFY_SIGNAL | EVT_TIMER, TPL_CALLBACK, EventNotifyCallback, &mTestContext, &mTestEvent);
  ASSERT_EFI_ERROR (Status);
  ASSERT (mTestEvent != 0);

  Status = gBS->SetTimer (mTestEvent, TimerPeriodic, 500);
  ASSERT_EFI_ERROR (Status);

  mTestContext.TestType  = TimerNotify;
  mTestContext.Signalled = TRUE;
  mTestContext.Handled   = FALSE;

  // Tick, but not enough to trigger event.
  mTimerNotifyFunction (100);
  ASSERT (mTestContext.Handled == FALSE);

  // Tick again, enough to trigger event.
  mTimerNotifyFunction (400);
  ASSERT (mTestContext.Handled == TRUE);

  mTestContext.Handled = FALSE;

  // Cancel the timer
  Status = gBS->SetTimer (mTestEvent, TimerCancel, 0);
  ASSERT_EFI_ERROR (Status);

  // Tick again, enough to trigger event.
  mTimerNotifyFunction (1000);
  ASSERT (mTestContext.Handled == FALSE);

  Status = gBS->CloseEvent (mTestEvent);
  ASSERT_EFI_ERROR (Status);

  DEBUG ((DEBUG_INFO, "[%a] Testing Complete\n", __FUNCTION__));
}

VOID
TestDevicePathSupport (
  VOID
  )
{
  EFI_STATUS  Status;

  // {82eea697-4fc9-49db-9e64-e94358e8aab4}
  EFI_GUID  TestProtocol = {
    0x82eea697, 0x4fc9, 0x49db, { 0x9e, 0x64, 0xe9, 0x43, 0x58, 0xe8, 0xaa, 0xb4 }
  };

  CHAR16  DevPathStr1[]  = L"PcieRoot(0x3)";
  CHAR16  DevPathStr2[]  = L"PcieRoot(0x3)/Pci(0x0,0x0)";
  CHAR16  DevPathStr3[]  = L"PcieRoot(0x3)/Pci(0x0,0x0)/Pci(0x0,0x0)";
  CHAR16  BogusPathStr[] = L"/Pci(0x0,0x0)/Pci(0x0,0x0)";

  EFI_DEVICE_PATH_PROTOCOL  *DevPath1;
  EFI_DEVICE_PATH_PROTOCOL  *DevPath2;
  EFI_DEVICE_PATH_PROTOCOL  *DevPath3;
  EFI_DEVICE_PATH_PROTOCOL  *BogusPath;
  EFI_DEVICE_PATH_PROTOCOL  *TestDevicePath;
  EFI_DEVICE_PATH_PROTOCOL  *TestDevicePath2;

  EFI_HANDLE  Handle1         = NULL;
  EFI_HANDLE  Handle2         = NULL;
  EFI_HANDLE  Handle3         = NULL;
  EFI_HANDLE  NoDevPathHandle = NULL;
  EFI_HANDLE  TestHandle      = NULL;

  DEBUG ((DEBUG_INFO, "[%a] Testing Device Path support.\n", __FUNCTION__));

  DevPath1  = ConvertTextToDevicePath (DevPathStr1);
  DevPath2  = ConvertTextToDevicePath (DevPathStr2);
  DevPath3  = ConvertTextToDevicePath (DevPathStr3);
  BogusPath = ConvertTextToDevicePath (BogusPathStr);

  ASSERT ((DevPath1 != NULL) && (DevPath2 != NULL) && (DevPath3 != NULL));

  // Install device path
  Status = gBS->InstallProtocolInterface (&Handle1, &gEfiDevicePathProtocolGuid, EFI_NATIVE_INTERFACE, DevPath1);
  ASSERT_EFI_ERROR (Status);

  Status = gBS->InstallProtocolInterface (&Handle2, &gEfiDevicePathProtocolGuid, EFI_NATIVE_INTERFACE, DevPath2);
  ASSERT_EFI_ERROR (Status);

  Status = gBS->InstallProtocolInterface (&Handle3, &gEfiDevicePathProtocolGuid, EFI_NATIVE_INTERFACE, DevPath3);
  ASSERT_EFI_ERROR (Status);

  // Install a copy of test protocol on a new handle without a device path - this tests that the "No Device Path" handle
  // is not returned below, which would be an error.
  Status = gBS->InstallProtocolInterface (&NoDevPathHandle, &TestProtocol, EFI_NATIVE_INTERFACE, NULL);
  ASSERT_EFI_ERROR (Status);

  DEBUG ((DEBUG_INFO, "[%a] Verify LocateDevicePath returns NOT_FOUND when the desired protocol doesn't exist.\n", __FUNCTION__));
  // Locate Device Path should fail if no handles with both TestProtocol and DevicePathProtocol exist.
  TestDevicePath = DevPath3;
  Status         = gBS->LocateDevicePath (&TestProtocol, &TestDevicePath, &TestHandle);
  ASSERT (Status == EFI_NOT_FOUND); // Test protocol is not installed on any handles.

  DEBUG ((DEBUG_INFO, "[%a] Verify LocateDevicePath returns success with correct handle and remaining device path.\n", __FUNCTION__));

  // TestProtocol only exists on Handle1
  Status = gBS->InstallProtocolInterface (&Handle1, &TestProtocol, EFI_NATIVE_INTERFACE, NULL);
  ASSERT_EFI_ERROR (Status);

  TestDevicePath = DevPath3;
  Status         = gBS->LocateDevicePath (&TestProtocol, &TestDevicePath, &TestHandle);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestHandle == Handle1);
  TestDevicePath2 = NextDevicePathNode (DevPath3);
  ASSERT (TestDevicePath2 != NULL);
  ASSERT (TestDevicePath == TestDevicePath2);

  // TestProtocol exists on Handle1 and Handle2
  Status = gBS->InstallProtocolInterface (&Handle2, &TestProtocol, EFI_NATIVE_INTERFACE, NULL);
  ASSERT_EFI_ERROR (Status);

  TestDevicePath = DevPath3;
  Status         = gBS->LocateDevicePath (&TestProtocol, &TestDevicePath, &TestHandle);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestHandle == Handle2);
  TestDevicePath2 = DevPath3;
  TestDevicePath2 = NextDevicePathNode (TestDevicePath2);
  TestDevicePath2 = NextDevicePathNode (TestDevicePath2);
  ASSERT (TestDevicePath2 != NULL);
  ASSERT (TestDevicePath == TestDevicePath2);

  // TestProtocol exists on Handle1, Handle2, and Handle3.
  Status = gBS->InstallProtocolInterface (&Handle3, &TestProtocol, EFI_NATIVE_INTERFACE, NULL);
  ASSERT_EFI_ERROR (Status);

  TestDevicePath = DevPath3;
  Status         = gBS->LocateDevicePath (&TestProtocol, &TestDevicePath, &TestHandle);
  ASSERT_EFI_ERROR (Status);
  ASSERT (TestHandle == Handle3);
  TestDevicePath2 = DevPath3;
  TestDevicePath2 = NextDevicePathNode (TestDevicePath2);
  TestDevicePath2 = NextDevicePathNode (TestDevicePath2);
  TestDevicePath2 = NextDevicePathNode (TestDevicePath2);
  ASSERT (TestDevicePath2 != NULL);
  ASSERT (TestDevicePath == TestDevicePath2);

  DEBUG ((DEBUG_INFO, "[%a] Verify LocateDevicePath returns NOT_FOUND when the device path used doesn't match any device path.\n", __FUNCTION__));

  TestDevicePath = BogusPath;
  Status         = gBS->LocateDevicePath (&TestProtocol, &TestDevicePath, &TestHandle);
  ASSERT (Status == EFI_NOT_FOUND); // BogusPath is not a sub path of any other path.

  FreePool (BogusPath); // Note: other test device paths are still installed on handles, so to be safe just leave them allocated.
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
  TestEventing ();
  TestTimerEvents ();
  TestDevicePathSupport ();

  return EFI_SUCCESS;
}
