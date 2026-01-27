# AcpiSdtDxe Driver

Patina produces an ACPI component which allows consumers to interface with tables through a Rust service. This driver provides the C AML implementation that utilizes the Rust ACPI table implementation.

For more information on the ACPI SDT protocol in general, see:

- [ACPI System Description Protocol](https://uefi.org/specs/PI/1.8/V5_ACPI_System_Desc_Table_Protocol.html) - PI Spec 1.8 Vol 9
- [ACPI Spec 6.5](https://uefi.org/sites/default/files/resources/ACPI_Spec_6_5_Aug29.pdf)

## Capabilities

- Produces `EFI_ACPI_SDT_PROTOCOL`.
- Uses the Rust-implemented `ACPI_GET_PROTOCOL` to perform AML parsing and manipulation.

## Structure

### ACPI Table Protocol

The equivalent of `EFI_ACPI_TABLE_PROTOCOL` (install, uninstall) is implemented entirely in Rust.

### `ACPI_GET_PROTOCOL`

The Patina ACPI component produces a custom protocol, `ACPI_GET_PROTOCOL`, which includes `GetAcpiTable` and `RegisterNotify`. These are normally part of `EFI_ACPI_SDT_PROTOCOL`.

The Patina ACPI component implements table management in Rust, but does not implement AML parsing and manipulation. These are included in the modified `AcpiSdtDxe` driver, which provides the remaining parts of `EFI_ACPI_SDT_PROTOCOL` (`RegisterNotify`, `Open/OpenSdt`, `Close`, `GetChild`, `Get/SetOption`, `FindPath`). To interface with the Rust-side tables, these implementations use the custom `EFI_ACPI_GET_PROTOCOL` to retrieve tables installed by the Patina ACPI component, instead of directly retrieving tables through a global data structure in the ACPI driver.

<table>
<tr>
<th>Before: Global Private ACPI Data</th>
<th>After: Uses ACPI_GET_PROTOCOL</th>
</tr>
<tr>
<td>
  
```C
EFI_ACPI_TABLE_LIST *
FindTableByBuffer (
  IN VOID  *Buffer
  )
{
  EFI_ACPI_TABLE_INSTANCE  *AcpiTableInstance;
  LIST_ENTRY               *CurrentLink;
  EFI_ACPI_TABLE_LIST      *CurrentTableList;
  LIST_ENTRY               *StartLink;

  // GLOBAL ACPI DATA INSTANCE
  AcpiTableInstance = SdtGetAcpiTableInstance ();

  StartLink   = &AcpiTableInstance->TableList;
  CurrentLink = StartLink->ForwardLink;

  while (CurrentLink != StartLink) {
    CurrentTableList = EFI_ACPI_TABLE_LIST_FROM_LINK (CurrentLink);
    if (((UINTN)CurrentTableList->Table <= (UINTN)Buffer) &&
        ((UINTN)CurrentTableList->Table + CurrentTableList->TableSize > (UINTN)Buffer))
    {
        // ...
}
```
  
</td>
<td>

```C
EFI_STATUS
FindTableByBuffer (
  IN  VOID                 *Buffer,
  OUT EFI_ACPI_SDT_HEADER  **OutTable
  )
{
  UINTN  Index = 0;

  EFI_ACPI_SDT_HEADER     *Table  = NULL;
  EFI_ACPI_TABLE_VERSION  Version = 0;
  UINTN                   Key     = 0;
  EFI_STATUS              Status;

  for ( ; ;) {
    // GET PROTOCOL
    Status = mAcpiGetProtocol->GetAcpiTable (Index, &Table, &Version, &Key);
    if (EFI_ERROR (Status)) {
      return Status;
    }

    if (((UINTN)Table <= ((UINTN)Buffer)) &&
        ((UINTN)Buffer < ((UINTN)Table + Table->Length)))
    {
      // ...
  }
}

```

</td>
</tr>
</table>

## Platform Integration

### Component Initialization

To enable the Patina ACPI component during Patina boot, call `AcpiProviderManager::new(...)` with the correct `oem_id`, `oem_table_id`, `oem_revision`, `creator_id`, and `creator_revision`. These values will vary based on platform.

Then, implement `component` for the core and add the Patina ACPI component:

```rust
add.component(patina_acpi::component::AcpiProviderManager::new( /* Platform config. */ ));
```

### Build Integration

The Patina ACPI component along with the custom `AcpiSdtDxe` C driver replaces the existing EDKII `AcpiTableDxe` implementation. To integrate the Rust table implementation into your platform, replace all instances of `AcpiTableDxe` with `AcpiSdtDxe`. `AcpiSdtDxe` should already be included in `patina-qemu` and relevant Intel platforms.

```inf
MdeModulePkg/Universal/Acpi/AcpiTableDxe/AcpiTableDxe.inf -> QemuPkg/AcpiSdtDxe/AcpiSdtDxe.inf
```
