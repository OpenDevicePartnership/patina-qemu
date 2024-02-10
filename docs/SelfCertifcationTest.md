# Self Certification Tests

The Self Certification Tests is an an efi application managed by tianocore
that allows for Platform Vendors and IHVs (Independent Hardware vendors) to
easily test.

The core managed repository is located at [edk2-test](https://github.com/tianocore/edk2-test)
and the releases section contains the associated binaries for testing. Simply
pick the application type and necessary architecture.

Here is a quick rundown of the different test types provided by the SCTs,
however the focus of these instructions will be to prepare the system for
the UEFI SCTs.

- UEFI SCT: Focuses on Platform / System conformance to the UEFI specification,
  both in terms of functionality and expected interfaces (such as return values
  under specific circumstances).

- IHV SCT: Focuses on Device and Driver conformance to the UEFI specification

- SCRT: Focuses on Runtime conformance to the UEFI specification

- EMS: Focuses on the network stack conformance

There are also two execution modes available to users wishing to run the SCTs,
that being Native mode (host only) and Passive mode (host & target).

For detailed information, please refer to the [User Guide](https://github.com/tianocore/edk2-test/tree/master/uefi-sct/Doc/UserGuide).

## Running the SCTs

These instructions are to run in Native mode, which is to say, directly on the
host machine. The first thing to note is that the SCTs expect at least 100MB of
space on the disk you plan to install it on. The typical workflow for a
physical platform is to load a USV with the installer, then install it on a
local drive. As this is a virtual platform, we will simply place it on the
virtual drive and install it to the same location.

### Build the platform

The first step is to build the platform with enough space on the virtual
drive. By default, Q35 will only create a virtual drive with 60MB, so that
will need to be overwritten via a build variable provided on the command line.

`stuart_build -c Platforms/QemuQ35Pkg/PlatformBuild.py TARGET=RELEASE BLD_*_MEMORY_PROTECTION=FALSE VIRTUAL_DRIVE_SIZE=150 EMPTY_DRIVE=TRUE --FlashRom`

**Note1**: It is also suggested (but not required) to build in RELEASE mode.

**Note2**: An existing virtual drive will not be overwritten unless
EMPTY_DRIVE=TRUE

**Note3**: We turn off memory protections as the current compiled SCTs do not
have the NX compat flag set, so the system won't run them.

**Note4**: Creating the drive will not actually happen unless flashing, so we
set --FlashRom

### Loading the SCTs on the drive

If you executed the above command, Qemu should have opened, loaded to the UEFI
shell, and shutdown (assuming you did not press a button to cancel the default
startup.nsh). If it did not close, simply close down QEMU; all we cared about
was that the platform was built, and the drive was created.

The next step is to download the SCT binaries from the releases section of the
github repository [edk2-test](https://github.com/tianocore/edk2-test/releases/).
Specifically: UefiSctBinaryX64.zip.

From there, unzip it and place everything in SctPackageX64 at the root of the
virtual hard drive that was created in the previous step. At the root of the
drive, you should now see 3 items: X64, InstallX64.efi, SctStartup.nsh. delete
the STARTUP.NSH if it exists.

**Note1**: The virtual drive is located in the Build output
**Note2**: Don't forget to unmount the virtual drive once you are done.

## Install the SCTs

Now that the files are placed and the virtual drive unmounted, we need to load
back into QEMU

`stuart_build -c Platforms/QemuQ35Pkg/PlatformBuild.py TARGET=RELEASE SHUTDOWN_AFTER_RUN=FALSE --FlashOnly`

From UEFI shell: `fs0:`
From UEFI shell: `InstallX64.efi`
Follow the prompts to install it to the only drive space available.

**Note1**: We use SHUTDOWN_AFTER_RUN=FALSE to ensure a Startup.nsh script does
not override the Sct provided one
**Note2**: We now use --FlashOnly so we don't waste time rebuilding.

### Running

It is easiest to run SCTs in user mode (a gui), however it is possible to run
them through the shell command line only.

SCTs provide a great [User Guide](https://github.com/tianocore/edk2-test/tree/master/uefi-sct/Doc/UserGuide)
on how to use them, so check it out if you want to do anything other then the
simple testing as I describe below:

From the root of fs0:

1. `cd SCT`
2. `SCT -u`
3. press enter on "Test Case Management"
4. Select / deselect test groups you wish to run by pressing space
5. Each test group has either one or two levels of drill down. You can select /
   deselect tests or test groups at any level.
6. `F9` to run the tests

Unfortunately, you cannot generate a report on UefiRust (not looked into). The
best solution is to copy your VHD over to mu_tiano_platforms and open it there.

1. `fs0:`
2. `cd SCT`
3. `SCT -u`
4. `Test Report Generator`
5. `F2`
6. Type name of file and press enter
7. Close Qemu
8. Open the vhd and navigate to SCT/Report, open the report
9. (optional) logs for each test are located at SCT/Log
