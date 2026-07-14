# Self Certification Tests

The UEFI Self Certification Tests (SCT) are an EFI application suite managed by TianoCore that allow Platform Vendors
and IHVs (Independent Hardware Vendors) to test their platform's conformance to the UEFI specification.

The core managed repository is located at [edk2-test](https://github.com/tianocore/edk2-test) and the releases section
contains the associated binaries for testing. Simply pick the application type and necessary architecture.

Here is a quick rundown of the different test types provided by the SCTs. However, the focus of these instructions is
to prepare the system for the UEFI SCTs.

- **UEFI SCT**: Focuses on Platform / System conformance to the UEFI specification, both in terms of functionality
  and expected interfaces (such as return values under specific circumstances).
- **IHV SCT**: Focuses on Device and Driver conformance to the UEFI specification.
- **SCRT**: Focuses on Runtime conformance to the UEFI specification.
- **EMS**: Focuses on network stack conformance.

There are also two execution modes available to users wishing to run the SCTs: Native mode (host only) and Passive mode
(host & target).

For detailed information, refer to the [User Guide](https://github.com/tianocore/edk2-test/tree/master/uefi-sct/Doc/UserGuide).

## Running the SCTs

These instructions run the SCTs in Native mode, which is to say, directly on the machine. The first thing to note is
that the SCTs expect at least 100MB of free space on the disk where they are installed. The typical workflow for a
physical platform is to load a USB drive with the installer, then install the SCTs to a local drive. Since this is a
virtual platform, the installer is instead placed on the virtual drive and the SCTs are installed to that same location.

### Build the platform

The first step is to build the platform with enough space on the virtual drive. **The commands shown throughout this
document are for the Q35 platform, but similar steps apply to `QemuArmVirtPkg`**. By default, Q35 only creates a virtual
drive with 60MB of space, so that default must be overridden via a build variable provided on the command line.

```bash
stuart_build -c Platforms/QemuQ35Pkg/PlatformBuild.py TARGET=RELEASE BLD_*_MEMORY_PROTECTION=FALSE VIRTUAL_DRIVE_SIZE=200 EMPTY_DRIVE=TRUE --FlashRom
```

```admonish tip
Building in `RELEASE` mode, as shown above, is suggested, but not required.
```

```admonish warning title="Memory protections disabled"
Memory protections are disabled because the current compiled SCTs do not have the NX-compatibility flag set, so the
system will not run them with memory protections enabled.

A GitHub issue has been opened to track this: [edk2-test#364](https://github.com/tianocore/edk2-test/issues/364)
```

```admonish note
- An existing virtual drive is **not** overwritten unless `EMPTY_DRIVE=TRUE` is set.
- The virtual drive contents are only written out when QEMU actually flashes and runs, so `--FlashRom` is used here
  (instead of just compiling) to ensure the drive is created.
```

### Loading the SCTs on the drive

If the above command was run, QEMU should have opened, loaded to the UEFI shell, and shut down (assuming the default
`startup.nsh` was not cancelled). If it did not close, simply close QEMU. All that mattered was that the platform was
built and the drive was created.

The next step is to download the SCT binaries from the releases section of the
[edk2-test](https://github.com/tianocore/edk2-test/releases/) GitHub repository, specifically:

- For `QemuQ35Pkg`: `SctPackageX64.7z`
- For `QemuArmVirtPkg`: `SctPackageAARCH64.tar.gz`

Unzip the archive and place everything from the appropriate package at the root of the virtual hard drive created in the
previous step. For Q35, this is `SctPackageX64`. At the root of the drive, there should now be 3 items: `X64`,
`InstallX64.efi`, `SctStartup.nsh`. Delete `STARTUP.NSH` if it exists.

```admonish note
- The virtual drive is located in the `Build` output directory.
- Don't forget to unmount the virtual drive once finished with it.
```

## Install the SCTs

Now that the files are placed and the virtual drive is unmounted, load back into QEMU:

```bash
stuart_build -c Platforms/QemuQ35Pkg/PlatformBuild.py TARGET=RELEASE SHUTDOWN_AFTER_RUN=FALSE --FlashOnly
```

From the UEFI shell:

1. `FS0:`
2. `InstallX64.efi`

Follow the prompts to install the SCTs to the only drive space available.

```admonish note
- `SHUTDOWN_AFTER_RUN=FALSE` ensures a `startup.nsh` script does not override the one provided by the SCT installer.
- `--FlashOnly` is used here (instead of `--FlashRom`) to reuse the existing build and avoid wasting time rebuilding.
```

### Running

It is easiest to run the SCTs in user mode (a GUI). However, it is also possible to run them through the shell command
line only.

The SCTs provide a [User Guide](https://github.com/tianocore/edk2-test/tree/master/uefi-sct/Doc/UserGuide) covering
advanced usage. A simple test run is described below.

From the root of `FS0:`:

1. `cd SCT`
2. `SCT -u`
3. Press enter on "Test Case Management".
4. Select / deselect test groups to run by pressing space.
5. Each test group has either one or two levels of drill down. Tests or test groups can be selected / deselected at any
   level.
6. Press `F9` to run the tests.

The below commands show how to generate a report after the tests have completed.

1. `FS0:`
2. `cd SCT`
3. `SCT -u`
4. `Test Report Generator`
5. `F2`
6. Type the name of the file and press enter.
7. Close QEMU.
8. Open the VHD and navigate to `SCT/Report`, then open the report.
9. (Optional) Logs for each test are located at `SCT/Log`.
