# Regression Testing

This repository performs scheduled testing against the default branch to detect regressions for common firmware paths.
The sections below describe the categories of tests (nightly, weekly, etc.) being performed. Not all tests have been
implemented yet:

- ✅ Means the regression tests are implemented and enabled.
- ❌ Means the regression tests are **not** implemented or enabled.

## OS Boot Validation

OS boot is critical to a platform being considered "correct", but the validation and continuous integration steps
performed when validating a pull request only boot to the UEFI shell, rather than any given operating system. A
nightly OS boot test exists to test common ways of booting an operating system. This includes booting from an internal
drive, from a USB drive, and from a PXE server.

- ✅ Q35 Internal Drive Windows Validation OS boot
- ✅ Q35 Internal Drive Ubuntu 24.04 Server OS boot
- ✅ ArmVirt Internal Drive Windows Validation OS boot
- ✅ ArmVirt Internal Drive Ubuntu 24.04 Server OS boot

- ✅ Q35 USB Drive Windows Validation OS boot
- ✅ Q35 USB Drive Ubuntu 24.04 Server OS boot
- ✅ ArmVirt USB Drive Windows Validation OS boot
- ✅ ArmVirt USB Drive Ubuntu 24.04 Server OS boot

- ❌ Q35 PXE Windows Validation OS boot
- ❌ Q35 PXE Ubuntu 24.04 Server OS boot
- ❌ ArmVirt PXE Windows Validation OS boot
- ❌ ArmVirt PXE Ubuntu 24.04 Server OS boot

The [`.github/workflows/nightly-os-boot.yml`](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/.github/workflows/nightly-os-boot.yml)
workflow is responsible for the nightly OS boot testing mentioned above. It first downloads and prepares the OS
images that will be booted so that, once booted, they automatically shut down. The firmware is then compiled and
flashed to the two virtual platforms, and BDS automatically detects the operating system and boots.

## Advanced Logger Log Gathering

The advanced logger is a logger implementation for UEFI firmware that writes its log files to a special partition
during boot. Since it is uncommon to need to pull logs off of a device (thanks to flashing and booting under a
debugger), it can take time to notice if this functionality is broken. This nightly test ensures that platform boot
logs exist and are retrievable from Windows systems once booted to the operating system.

- ✅ Q35 advanced logger boot log gathering via UEFI application
- ✅ ArmVirt advanced logger boot log gathering via UEFI application
- ❌ Q35 advanced logger boot log gathering via Windows Validation OS
- ❌ ArmVirt advanced logger boot log gathering via Windows Validation OS

## Debugger Connection

This flow is exercised fairly frequently, but it is important enough that any regression should be detected
immediately. Nightly tests boot with the debugger and initial breakpoint enabled, and a script sends GDB Remote Serial
Protocol communication to ensure the debugger successfully connects and continues from the initial breakpoint.

- ❌ Q35 debugger connection boot
- ❌ ArmVirt debugger connection boot

## Hibernation

While hibernation is not strictly a UEFI feature, its implementation is highly dependent on the memory layout provided
by firmware via the UEFI memory map. Successful hibernation requires that runtime memory regions remain stable across
boots. Historically, even minor firmware changes have broken hibernation, demonstrating how sensitive and error-prone
this path can be.

To validate hibernation support, tests are implemented that repeatedly boot the operating system, extract the UEFI
memory map from logs, and compare it against the previous boot to ensure memory layout stability. Each iteration then
performs a hibernate-resume cycle. This process is repeated multiple times to detect nondeterministic firmware behavior
that could cause resume failures.

- ❌ Q35 hibernate resume
- ❌ ArmVirt hibernate resume

## Self-Certification Tests (SCTs)

SCTs are the de-facto way to validate a platform's firmware implementation is compliant with the UEFI specification.
It is important to monitor and stay compliant with these tests. Unlike other tests documented here, SCTs are run on a
weekly basis due to the time it takes to run them.

- ❌ Q35 self-certification tests
- ❌ ArmVirt self-certification tests

## Performance Monitoring

Boot performance is an important aspect of platform firmware and must be monitored to ensure it does not degrade over
time. Performance tracking is performed by the EDK II performance measurement protocol and can be retrieved from the
operating system via the FBPT. We can boot to the operating system, retrieve the boot measurements, and publish the
results.

Performance monitoring happens in two parts. On a weekly schedule, a workflow runs to gather the current performance
measurements and produce them as an artifact. A second workflow runs when a pull request is created that updates the
Patina QEMU DXE Core external dependency. This workflow performs the same performance measurements and additionally
compares them against the most recent measurements from the scheduled run, commenting on the pull request with the
performance changes.

- ❌ Q35 Release Performance Measurement tracking
- ❌ ArmVirt Release Performance Measurement tracking
