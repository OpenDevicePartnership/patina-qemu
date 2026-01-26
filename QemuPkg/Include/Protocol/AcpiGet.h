/** @file
 * TODO
 */
#ifndef __ACPI_GET_H___
#define __ACPI_GET_H___

#include <Protocol/AcpiSystemDescriptionTable.h>

#define ACPI_GET_PROTOCOL_GUID \
  { 0x7f3c1a92, 0x8b4e, 0x4d2f, { 0xa6, 0xc9, 0x3e, 0x12, 0xf4, 0xb8, 0xd7, 0xc1 } }

typedef struct _ACPI_GET_PROTOCOL {
    EFI_ACPI_TABLE_VERSION      AcpiVersion;
    EFI_ACPI_GET_ACPI_TABLE2    GetAcpiTable;
    EFI_ACPI_REGISTER_NOTIFY    RegisterNotify;
} ACPI_GET_PROTOCOL;

extern EFI_GUID  gAcpiGetProtocolGuid;

#endif // __ACPI_GET_H___
