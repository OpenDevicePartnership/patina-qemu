// FF-A (Firmware Framework for Arm) ACPI device node
//
// SPDX-License-Identifier: MIT
//

DefinitionBlock ("SsdtFfa.aml", "SSDT", 2, "QEMUAR", "FFA     ", 1) {

Device(\_SB_.FFA0) {
  Name(_HID, "ARML0002")

  OperationRegion(AFFH, FFixedHw, 2, 144)
  Field(AFFH, BufferAcc, NoLock, Preserve) { AccessAs(BufferAcc, 0x1), FFAC, 1152 }

  Name(_DSD, Package() {
    ToUUID("c08c3233-b316-4723-a9d7-e21b7ac0fb6a"), //Device Prop UUID
    Package() {
      Package(2) {
        "arm-arml0002-ffa-ntf-bind",
        Package() {
          0x00010000, // Revision
          1, // Count of following NTF Packages
          Package () {
            ToUUID("330c1273-fde5-4757-9819-5b6539037502"), // UUID
            Package () {
              0x01,     // Cookie1
            }
          },
        }
      }
    }
  }) // _DSD()

  Method(_DSM, 0x4, NotSerialized)
  {
    // Arg0 - UUID
    // Arg1 - Revision
    // Arg2: Function Index
    //         0 - Query
    //         1 - Notify
    //         2 - binding failure
    //         3 - infra failure
    // Arg3 - Data

    //
    // Device specific method used to query
    // configuration data. See ACPI 5.0 specification
    // for further details.
    //
    If(LEqual(Arg0, Buffer(0x10) {
        //
        // UUID: {7681541E-8827-4239-8D9D-36BE7FE12542}
        //
        0x1e, 0x54, 0x81, 0x76, 0x27, 0x88, 0x39, 0x42, 0x8d, 0x9d, 0x36, 0xbe, 0x7f, 0xe1, 0x25, 0x42
      }))
    {
      // Query Function
      If(LEqual(Arg2, Zero))
      {
        Return(Buffer(One) { 0x03 }) // Bitmask Query + Notify
      }

      // Notify Function
      If(LEqual(Arg2, One))
      {
        // Arg3 - Package {UUID, Cookie}
        // Store(DeRefOf(Index(Arg3,1)), \_SB.ECT0.NEVT )
        Return(Zero)
      }
      Return(Buffer(One) { 0x00 })
    } Else {
      Return(Buffer(One) { 0x00 })
    }
  }

}

} // DefinitionBlock
