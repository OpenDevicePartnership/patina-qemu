# Regression Testing

This repository performs comprehensive scheduled testing against the default branch to detect regressions for common
paths in firmware. The below sections will contain different tests (nightly, weekly, etc.) being performed. Not all
tests have been implemented yet. See below to know if a test is implemented or not:

- [x] Means that the regression tests are implemented and enabled.
- [ ] Means that the regression tests are **not** implemented or enabled.

## OS Boot Validation

OS boot is critical to a platform being considered "correct", however the validation and continuous integration steps
performed when validating a pull request from merge only boots to the UEFI shell, rather than any given operating
system. Due to this, a nightly OS boot test exists to test common ways of booting an operating system. This includes
ways such as booting from an internal drive, from a USB drive, and from a PXE server.

- [x] Q35 Internal Drive Windows Validation OS boot
- [x] Q35 Internal Drive Ubuntu 24.04 Server OS boot
- [x] SBSA Internal Drive Windows Validation OS boot
- [x] SBSA Internal Drive Ubuntu 24.04 Server OS boot

- [x] Q35 USB Drive Windows Validation OS boot
- [x] Q35 USB Drive Ubuntu 24.04 Server OS boot
- [x] SBSA USB Drive Windows Validation OS boot
- [x] SBSA USB Drive Ubuntu 24.04 Server OS boot

- [ ] Q35 PXE Windows Validation OS boot
- [ ] Q35 PXE Ubuntu 24.04 Server OS boot
- [ ] SBSA PXE Windows Validation OS boot
- [ ] SBSA PXE Ubuntu 24.04 Server OS boot

The [.github/workflows/nightly-os-boot.yml](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/.github/workflows/nightly-os-boot.yml)
is responsible for the nightly OS boot testing mentioned above. This workflow works by first downloading and preparing
the OS images that will be booted so that once booted, they automatically shut down. From there the firmware is
compiled and flashed to our two virtual platforms and BDS automatically detects the operating system and boots.

## Advanced Logger Log Gathering

The advanced logger is a logger implementation for UEFI firmware that writes its log files to a special partition
during boot. Since it is not common to need to pull logs off of a device (Thanks to flashing / booting under a
debugger), it can take time to notice if this functionality is broken. This nightly test ensures that the platform boot
logs exist and are retrievable from windows systems once booted to the operating system.

- [x] Q35 advanced logger boot log gathering via UEFI application
- [x] SBSA advanced logger boot log gathering via UEFI application
- [ ] Q35 advanced logger boot log gathering via Windows Validation OS
- [ ] SBSA advanced logger boot log gathering via Windows Validation OS

## Debugger Connection

This flow is exercised fairly frequently, however it is important enough that any regression in this functionality
should be detected immediately. Due to this, nightly tests exist that boot with the debugger and initial breakpoint
enabled, and a script sends GDB remote serial protocol communication to ensure we successfully connect and continue
from the initial breakpoint.

- [ ] Q35 debugger connection boot
- [ ] SBSA debugger connection boot

## Hibernation

While hibernation is not strictly a UEFI feature, its implementation is highly dependent on the memory layout provided
by firmware via the UEFI memory map. Successful hibernation requires that runtime memory regions remain stable across
boots. Historically, even minor firmware changes have broken hibernation, demonstrating how sensitive and error-prone
this path can be.

To validate hibernation support, tests are implemented that repeatedly boot the operating system, extract the UEFI
memory map from logs, and compare it against the previous boot to ensure memory layout stability. Each iteration then
performs a hibernate-resume cycle. This process is repeated multiple times to detect nondeterministic firmware behavior
that could cause resume failures.

- [ ] Q35 hibernate resume
- [ ] SBSA hibernate resume

## Self-Certification Tests (SCTs)

SCTs are the de-facto way to validate a platform's firmware implementation is compliant with the UEFI specification.
Due to this, it is important to monitor and stay compliant with these tests. Unlike other tests documented here, these
are run on a weekly basis due to the time it takes to run these tests.

- [ ] Q35 self-certification tests
- [ ] SBSA self-certification tests

## Performance Monitoring

Boot performance is an important aspect of platform firmware and thus must be monitored to ensure that boot
performance does not degrade over time. Performance tracking is performed by the EDKII performance measurement protocol
and can be retrieved via the operating system via the FBPT. Due to this, we can boot to the operating system, retrieve
the boot measurements and publish the results.

Performance monitoring happens in two parts. On a weekly schedule, a workflow runs to gather the current performance
measurements and produce them as an artifact. A second workflow runs when a pull request is created that updates the
Patina QEMU DXE Core external dependency. This workflow will perform the same performance measurements, but will
additionally compare it against the most recent performance measurements from the scheduled run and comment on the
pull request with the performance changes.

- [ ] Q35 Release Performance Measurement tracking
- [ ] SBSA Release Performance Measurement tracking
