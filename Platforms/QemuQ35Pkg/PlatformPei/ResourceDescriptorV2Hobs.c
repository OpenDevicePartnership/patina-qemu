/**@file
  V2 HOB production for QEMU Q35 resources.

  Copyright (c) Microsoft Corporation

  SPDX-License-Identifier: BSD-2-Clause-Patent

**/

#include <PiPei.h>
#include <Uefi.h>
#include <Library/HobLib.h>
#include <Library/DebugLib.h>

//
// Ranges Below 1MB
//
#define REAL_MODE_IVT_BEGIN            0x00000000
#define REAL_MODE_IVT_END              0x000003FF
#define BIOS_DATA_AREA_BEGIN           0x00000400
#define BIOS_DATA_AREA_END             0x000004FF
#define CONVENTIONAL_MEMORY_BEGIN      0x00000500
#define CONVENTIONAL_MEMORY_END        0x0007FFFF
#define EXTENDED_BIOS_DATA_AREA_BEGIN  0x00080000
#define EXTENDED_BIOS_DATA_AREA_END    0x0009FFFF
#define VIDEO_DISPLAY_BEGIN            0x000A0000
#define VIDEO_DISPLAY_END              0x000BFFFF
#define VIDEO_ROM_BEGIN                0x000C0000
#define VIDEO_ROM_END                  0x000C7FFF
#define BIOS_EXTENSIONS_BEGIN          0x000C8000
#define BIOS_EXTENSIONS_END            0x000EFFFF
#define MOTHERBOARD_BIOS_BEGIN         0x000F0000
#define MOTHERBOARD_BIOS_END           0x000FFFFF

/**
  Produces V2 Resource HOBs for legacy BIOS resources below 1MB.

**/
VOID
PublishV2ResourceHobsBelow1MB (
  VOID
  )
{
  EFI_PHYSICAL_ADDRESS  Start;
  EFI_PHYSICAL_ADDRESS  End;
  UINT64                Length;
  EFI_PEI_HOB_POINTERS  Hob;

  Hob.Raw = GetHobList ();
  while ((Hob.Raw = GetNextHob (EFI_HOB_TYPE_RESOURCE_DESCRIPTOR, Hob.Raw)) != NULL) {
    Start  = Hob.ResourceDescriptor->PhysicalStart;
    Length = Hob.ResourceDescriptor->ResourceLength;
    End    = Start + Length - 1;

    if (End < SIZE_1MB) {
      // Patina DXE Core doesn't work if both V1 and V2 resource HOBs are present.
      Hob.Header->HobType = EFI_HOB_TYPE_UNUSED;

      // Do not set any atttributes on IO ranges below 1MB
      if (Hob.ResourceDescriptor->ResourceType == EFI_RESOURCE_IO) {
        BuildResourceDescriptorV2 (
          Hob.ResourceDescriptor->ResourceType,
          Hob.ResourceDescriptor->ResourceAttribute,
          Start,
          Length,
          0,
          &Hob.ResourceDescriptor->Owner
          );
        DEBUG ((DEBUG_INFO, "Published V2 Resource HOB for Legacy BIOS IO range: Start=0x%lx, Length=0x%lx\n", Start, Length));
        continue;
      }

      // Check that the resource type is expected for this range
      if ((Hob.ResourceDescriptor->ResourceType != EFI_RESOURCE_SYSTEM_MEMORY) &&
          (Hob.ResourceDescriptor->ResourceType != EFI_RESOURCE_MEMORY_MAPPED_IO) &&
          (Hob.ResourceDescriptor->ResourceType != EFI_RESOURCE_MEMORY_RESERVED))
      {
        DEBUG ((DEBUG_ERROR, "Unexpected resource type 0x%x for HOB below 1MB\n", Hob.ResourceDescriptor->ResourceType));
        Hob.Raw = GET_NEXT_HOB (Hob);
        continue;
      }

      if (End <= EXTENDED_BIOS_DATA_AREA_END) {
        // These are considered conventional memory ranges
        BuildResourceDescriptorV2 (
          Hob.ResourceDescriptor->ResourceType,
          Hob.ResourceDescriptor->ResourceAttribute,
          Start,
          Length,
          EFI_MEMORY_WB,
          &Hob.ResourceDescriptor->Owner
          );
      } else if ((VIDEO_DISPLAY_BEGIN <= Start) && (End <= VIDEO_DISPLAY_END)) {
        // Video display is uncacheable
        BuildResourceDescriptorV2 (
          Hob.ResourceDescriptor->ResourceType,
          Hob.ResourceDescriptor->ResourceAttribute,
          Start,
          Length,
          EFI_MEMORY_UC,
          &Hob.ResourceDescriptor->Owner
          );
      } else if ((VIDEO_ROM_BEGIN <= Start) && (End <= MOTHERBOARD_BIOS_END)) {
        // Everything else is a BIOS/ROM range, mark uncacheable and write protected
        BuildResourceDescriptorV2 (
          Hob.ResourceDescriptor->ResourceType,
          Hob.ResourceDescriptor->ResourceAttribute,
          Start,
          Length,
          EFI_MEMORY_UC | EFI_MEMORY_WP,
          &Hob.ResourceDescriptor->Owner
          );
      }

      DEBUG ((DEBUG_INFO, "Published V2 Resource HOB for Legacy BIOS memory range: Start=0x%lx, Length=0x%lx\n", Start, Length));
    }

    Hob.Raw = GET_NEXT_HOB (Hob);
  }
}

/**
  Produces V2 Resource HOBs for memory types.

**/
VOID
PublishV2MemoryHobs (
  VOID
  )
{
  EFI_PHYSICAL_ADDRESS  End;
  UINT64                Length;
  EFI_PEI_HOB_POINTERS  Hob;

  Hob.Raw = GetHobList ();
  while ((Hob.Raw = GetNextHob (EFI_HOB_TYPE_RESOURCE_DESCRIPTOR, Hob.Raw)) != NULL) {
    End    = Hob.ResourceDescriptor->PhysicalStart + Hob.ResourceDescriptor->ResourceLength - 1;
    Length = Hob.ResourceDescriptor->ResourceLength;

    if (Hob.ResourceDescriptor->ResourceType == EFI_RESOURCE_SYSTEM_MEMORY) {
      Hob.Header->HobType = EFI_HOB_TYPE_UNUSED;

      BuildResourceDescriptorV2 (
        Hob.ResourceDescriptor->ResourceType,
        Hob.ResourceDescriptor->ResourceAttribute,
        Hob.ResourceDescriptor->PhysicalStart,
        Length,
        EFI_MEMORY_WB,
        &Hob.ResourceDescriptor->Owner
        );
    } else if (Hob.ResourceDescriptor->ResourceType == EFI_RESOURCE_MEMORY_MAPPED_IO) {
      Hob.Header->HobType = EFI_HOB_TYPE_UNUSED;

      BuildResourceDescriptorV2 (
        Hob.ResourceDescriptor->ResourceType,
        Hob.ResourceDescriptor->ResourceAttribute,
        Hob.ResourceDescriptor->PhysicalStart,
        Length,
        EFI_MEMORY_UC | EFI_MEMORY_XP,
        &Hob.ResourceDescriptor->Owner
        );
    } else if (Hob.ResourceDescriptor->ResourceType == EFI_RESOURCE_MEMORY_RESERVED) {
      Hob.Header->HobType = EFI_HOB_TYPE_UNUSED;

      BuildResourceDescriptorV2 (
        Hob.ResourceDescriptor->ResourceType,
        Hob.ResourceDescriptor->ResourceAttribute,
        Hob.ResourceDescriptor->PhysicalStart,
        Length,
        EFI_MEMORY_UC,
        &Hob.ResourceDescriptor->Owner
        );
    } else {
      DEBUG ((DEBUG_ERROR, "Unexpected resource type 0x%x for System Memory HOB\n", Hob.ResourceDescriptor->ResourceType));
      Hob.Raw = GET_NEXT_HOB (Hob);
      continue;
    }

    DEBUG ((DEBUG_INFO, "Published V2 System Memory HOB: Start=0x%lx, Length=0x%lx\n", Hob.ResourceDescriptor->PhysicalStart, Length));
    Hob.Raw = GET_NEXT_HOB (Hob);
  }
}

/**
  Produces V2 Resource HOBs.

**/
VOID
PublishV2ResourceHobs (
  VOID
  )
{
  PublishV2ResourceHobsBelow1MB ();
  PublishV2MemoryHobs ();
}
