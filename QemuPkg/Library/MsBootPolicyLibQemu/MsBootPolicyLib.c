/*++
  This module will provide access to platform information needed to implement
  the MsBootPolicy.

  Copyright (c) Microsoft Corporation.
  SPDX-License-Identifier: BSD-2-Clause-Patent

**/

#include <Uefi.h>                                     // UEFI base types

#include <Protocol/LoadFile.h>
#include <Protocol/SimpleFileSystem.h>
#include <Library/UefiBootServicesTableLib.h>         // gBS
#include <Library/UefiRuntimeServicesTableLib.h>      // gRT
#include <Library/DebugLib.h>                         // DEBUG tracing
#include <Library/BaseMemoryLib.h>
#include <Library/DevicePathLib.h>
#include <Library/MemoryAllocationLib.h>
#include <Library/MsBootPolicyLib.h>                  // Current runtime mode.
#include <Library/DevicePathLib.h>
#include <Library/DeviceBootManagerLib.h>
#include <Library/MsPlatformDevicesLib.h>

#include <Settings/BootMenuSettings.h>

static BOOT_SEQUENCE  BootSequenceUPH[] = {
  MsBootUSB,
  MsBootPXE4,
  MsBootPXE6,
  MsBootHDD,
  MsBootDone
};

static BOOT_SEQUENCE  BootSequenceHUP[] = {
  // HddUsbPxe
  MsBootHDD,
  MsBootUSB,
  MsBootPXE4,
  MsBootPXE6,
  MsBootDone
};

static EFI_IMAGE_LOAD  gSystemLoadImage;

EFI_STATUS
EFIAPI
LocalLoadImage (
  IN  BOOLEAN                   BootPolicy,
  IN  EFI_HANDLE                ParentImageHandle,
  IN  EFI_DEVICE_PATH_PROTOCOL  *DevicePath,
  IN  VOID                      *SourceBuffer OPTIONAL,
  IN  UINTN                     SourceSize,
  OUT EFI_HANDLE                *ImageHandle
  )
{
  if (NULL != DevicePath) {
    if (!MsBootPolicyLibIsDevicePathBootable (DevicePath)) {
      return EFI_ACCESS_DENIED;
    }
  }

  // Pass LoadImage call to system LoadImage;
  return gSystemLoadImage (
           BootPolicy,
           ParentImageHandle,
           DevicePath,
           SourceBuffer,
           SourceSize,
           ImageHandle
           );
}

/**
  Constructor
*/
EFI_STATUS
EFIAPI
MsBootPolicyLibConstructor (
  IN EFI_HANDLE        ImageHandle,
  IN EFI_SYSTEM_TABLE  *SystemTable
  )
{
  UINT32   Crc;
  EFI_TPL  OldTpl;

  //
  // If linked with BDS, take over gBS->LoadImage.
  // The current design doesn't allow for BDS to be terminated.
  //
  if (PcdGetBool (PcdBdsBootPolicy)) {
    // If linked with BDS, take over gBS->LoadImage.  BDS can never
    OldTpl           = gBS->RaiseTPL (TPL_HIGH_LEVEL);
    gSystemLoadImage = gBS->LoadImage;
    gBS->LoadImage   = LocalLoadImage;
    gBS->Hdr.CRC32   = 0;
    Crc              = 0;
    gBS->CalculateCrc32 ((UINT8 *)&gBS->Hdr, gBS->Hdr.HeaderSize, &Crc);
    gBS->Hdr.CRC32 = Crc;
    gBS->RestoreTPL (OldTpl);
  }

  return EFI_SUCCESS;    // Library constructors have to return EFI_SUCCESS.
}

/**
  Print the device path
*/
static VOID
PrintDevicePath (
  IN EFI_DEVICE_PATH_PROTOCOL  *DevicePath
  )
{
  CHAR16  *ToText = NULL;

  if (DevicePath != NULL) {
    ToText = ConvertDevicePathToText (DevicePath, TRUE, TRUE);
  }

  DEBUG ((DEBUG_INFO, "%s", ToText)); // Output NewLine separately in case string is too long
  DEBUG ((DEBUG_INFO, "\n"));

  if (NULL != ToText) {
    FreePool (ToText);
  }

  return;
}

/**
 *Ask if the platform is requesting Settings Change

 *@retval TRUE     System is requesting Settings Change
 *@retval FALSE    System is not requesting Changes.
**/
BOOLEAN
EFIAPI
MsBootPolicyLibIsSettingsBoot (
  VOID
  )
{
  return FALSE;
}

/**
 *Ask if the platform is requesting an Alternate Boot

 *@retval TRUE     System is requesting Alternate Boot
 *@retval FALSE    System is not requesting AltBoot.
**/
BOOLEAN
EFIAPI
MsBootPolicyLibIsAltBoot (
  VOID
  )
{
  return FALSE;
}

EFI_STATUS
EFIAPI
MsBootPolicyLibClearBootRequests (
  VOID
  )
{
  return EFI_SUCCESS;
}

/**

 *Ask if the platform allows booting this controller

 *@retval TRUE     System is requesting Alternate Boot
 *@retval FALSE    System is not requesting AltBoot.
**/
BOOLEAN
EFIAPI
MsBootPolicyLibIsDeviceBootable (
  EFI_HANDLE  ControllerHandle
  )
{
  return MsBootPolicyLibIsDevicePathBootable (DevicePathFromHandle (ControllerHandle));
}

/**

 *Ask if the platform allows booting this controller

 *@retval TRUE     Device is not excluded from booting
 *@retval FALSE    Device is excluded from booting.
**/
BOOLEAN
EFIAPI
MsBootPolicyLibIsDevicePathBootable (
  EFI_DEVICE_PATH_PROTOCOL  *DevicePath
  )
{
  BOOLEAN                   rc = TRUE;
  EFI_DEVICE_PATH_PROTOCOL  *SdCardDevicePath;
  UINTN                     Size;
  UINTN                     SdSize;

  //    SdCards are not bootable if the platform
  //    returns an SdCard device path

  DEBUG ((DEBUG_INFO, "%a Checking if the following device path is permitted to boot:\n", __func__));

  if (NULL == DevicePath) {
    DEBUG ((DEBUG_ERROR, "NULL device path\n"));
    return TRUE;            // Don't know where this device is, so it is not "excluded"
  }

 #ifdef EFI_DEBUG
  #define MAX_DEVICE_PATH_SIZE  0x100000// Arbitrary 1 Meg max device path size.
 #else
  #define MAX_DEVICE_PATH_SIZE  0      // Don't check length on retail builds.
 #endif

  PrintDevicePath (DevicePath);
  if (!IsDevicePathValid (DevicePath, MAX_DEVICE_PATH_SIZE)) {
    // Arbitrary 1 Meg max device path size
    DEBUG ((DEBUG_ERROR, "Invalid device path\n"));
    return FALSE;
  }

  Size = GetDevicePathSize (DevicePath);

  SdCardDevicePath = GetSdCardDevicePath ();

  if (NULL != SdCardDevicePath) {
    PrintDevicePath (SdCardDevicePath);
    SdSize =  GetDevicePathSize (SdCardDevicePath);
    if (Size > SdSize) {
      // Compare the first part of the device path to the known path of the SdCard.
      if (0 == CompareMem (DevicePath, SdCardDevicePath, SdSize - END_DEVICE_PATH_LENGTH)) {
        DEBUG ((DEBUG_ERROR, "Boot from SD Card is not allowed.\n"));
        rc = FALSE;
      }
    }
  } else {
    DEBUG ((DEBUG_INFO, "No SD Card check enabled.\n"));
  }

  if (rc) {
    DEBUG ((DEBUG_INFO, "Boot from this device is enabled\n"));
  } else {
    DEBUG ((DEBUG_ERROR, "Boot from this device has been prevented\n"));
  }

  return rc;
}

/**
  Asks the platform if the DevicePath provided is a valid bootable 'USB' device.
  USB here indicates the physical port connection type not the device protocol.
  With TBT or USB4 support PCIe storage devices are valid 'USB' boot options.

  Default implementation:
    The platform alone determines if DevicePath is valid for USB boot support.

  @param DevicePath Pointer to DevicePath to check

  @retval TRUE     Device is a valid USB boot option
  @retval FALSE    Device is not a valid USB boot option
 **/
BOOLEAN
EFIAPI
MsBootPolicyLibIsDevicePathUsb (
  EFI_DEVICE_PATH_PROTOCOL  *DevicePath
  )
{
  return PlatformIsDevicePathUsb (DevicePath);
}

/**
 *Ask if the platform for the boot sequence

 *@retval EFI_SUCCESS  BootSequence pointer returned
 *@retval Other        Error getting boot sequence

 BootSequence is assumed to be a pointer to constant data, and
 is not freed by the caller.

**/
EFI_STATUS
EFIAPI
MsBootPolicyLibGetBootSequence (
  BOOT_SEQUENCE  **BootSequence,
  BOOLEAN        AltBootRequest
  )
{
  if (BootSequence == NULL) {
    return EFI_INVALID_PARAMETER;
  }

  if (AltBootRequest) {
    *BootSequence = BootSequenceUPH;
    DEBUG ((DEBUG_INFO, "%a - returing alt boot sequence\n", __func__));
  } else {
    DEBUG ((DEBUG_INFO, "%a - returing normal sequence\n", __func__));
    *BootSequence = BootSequenceHUP;
  }

  return EFI_SUCCESS;
}
