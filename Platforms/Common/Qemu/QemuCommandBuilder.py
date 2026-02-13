##
# QEMU Command Builder for Q35 and SBSA architectures.
# Provides a builder API for constructing QEMU command line arguments.
#
# Copyright (c) Microsoft Corporation
# SPDX-License-Identifier: BSD-2-Clause-Patent
##

"""QEMU Command Builder for Q35 and SBSA architectures"""

import os
import logging
import datetime
from pathlib import Path
from enum import Enum


class QemuArchitecture(Enum):
    """Supported QEMU architectures"""

    Q35 = "q35"
    SBSA = "sbsa"


class QemuCommandBuilder:
    """Builder class for constructing QEMU command arguments"""

    def __init__(self, executable, architecture=QemuArchitecture.Q35):
        self._logger = logging.getLogger(__name__)
        self._executable = executable
        self._architecture = architecture
        self._args = []

        # Idempotent tracking flags
        self._rom_path_added = False
        self._machine_added = False
        self._cpu_added = False
        self._firmware_added = False
        self._usb_controller_added = False
        self._usb_mouse_added = False
        self._usb_keyboard_added = False
        self._usb_storage_index = 0
        self._memory_added = False
        self._network_added = False
        self._smbios_added = False
        self._tpm_added = False
        self._display_added = False
        self._gdb_server_added = False
        self._serial_port_added = False
        self._monitor_port_added = False

        # Common initial arguments
        if self._architecture == QemuArchitecture.Q35:
            self._args.extend(["-debugcon", "stdio"])  # enable debug console
            self._args.extend(
                ["-global", "ICH9-LPC.disable_s3=1"]
            )  # disable S3 sleep state
            self._args.extend(
                ["-global", "isa-debugcon.iobase=0x402"]
            )  # debug console
            self._args.extend(
                ["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]
            )  # debug exit device
        elif self._architecture == QemuArchitecture.SBSA:
            self._args.extend(["-semihosting"])

    def with_rom_path(self, rom_dir):
        """Set ROM path for QEMU external dependency"""
        if self._rom_path_added:
            self._logger.debug("ROM path already added, skipping")
            return self

        if not rom_dir:
            return self

        self._rom_path_added = True
        self._logger.debug(f"Setting ROM path to: {rom_dir}")
        self._args.extend(["-L", f"{str(Path(rom_dir))}"])
        return self

    def with_machine(self, accel=None):
        """Configure machine type with SMM and acceleration"""
        if self._machine_added:
            self._logger.debug("Machine already configured, skipping")
            return self

        self._machine_added = True
        if self._architecture == QemuArchitecture.Q35:
            machine_config = f"q35,smm=on"

            if accel:
                accel_lower = accel.lower()
                if accel_lower in ["kvm", "tcg", "whpx"]:
                    machine_config += f",accel={accel_lower}"

            self._args.extend(["-machine", machine_config])
        elif self._architecture == QemuArchitecture.SBSA:
            self._args.extend(["-machine", "sbsa-ref"])

        self._args.extend(
                ["-global", "driver=cfi.pflash01,property=secure,value=on"]
            )

        return self

    def with_cpu(self, model=None, core_count=None):
        """Configure CPU model and core count"""
        if self._cpu_added:
            self._logger.debug("CPU already configured, skipping")
            return self

        self._cpu_added = True
        if self._architecture == QemuArchitecture.Q35:
            cpu_model = model or "qemu64"
            cpu_features = f"{cpu_model},+rdrand,+umip,+smep,+pdpe1gb,+popcnt,+sse,+sse2,+sse3,+ssse3,+sse4.2,+sse4.1"
            self._args.extend(["-cpu", cpu_features])
        elif self._architecture == QemuArchitecture.SBSA:
            self._args.extend(["-cpu", "max,sve=off,sme=off"])

        if core_count:
            self._args.extend(["-smp", str(core_count)])

        return self

    def with_firmware(self, code_fd, vars_fd=None):
        """Configure firmware (CODE and VARS)"""
        if self._firmware_added:
            self._logger.debug("Firmware already configured, skipping")
            return self

        if not code_fd:
            return self

        self._firmware_added = True
        self._logger.debug(
            "Configuring firmware - CODE: %s, VARS: %s", code_fd, vars_fd
        )

        if self._architecture == QemuArchitecture.Q35:
            self._args.extend(
                [
                    "-drive",
                    f"if=pflash,format=raw,unit=0,file={str(code_fd)},readonly=on",
                    "-drive",
                    f"if=pflash,format=raw,unit=1,file={str(vars_fd)}",
                ]
            )
        elif self._architecture == QemuArchitecture.SBSA:
            # SBSA has different firmware layout
            # Unit 0: SECURE_FLASH0.fd (writable)
            # Unit 1: QEMU_EFI.fd (readonly)
            if vars_fd:
                self._args.extend(
                    ["-drive", f"if=pflash,format=raw,unit=0,file={str(vars_fd)}"]
                )
            self._args.extend(
                [
                    "-drive",
                    f"if=pflash,format=raw,unit=1,file={str(code_fd)},readonly=on",
                ]
            )

        return self

    def with_usb_controller(self):
        """Add USB controller"""
        if self._usb_controller_added:
            self._logger.debug("USB controller already added, skipping")
            return self

        self._usb_controller_added = True
        self._args.extend(
            [
                "-device",
                "qemu-xhci,id=usb",
            ]
        )

        return self

    def with_usb_mouse(self):
        """Add USB mouse device"""
        if self._usb_mouse_added:
            self._logger.debug("USB mouse already added, skipping")
            return self
        
        if not self._usb_controller_added:
            self = self.with_usb_controller()

        self._usb_mouse_added = True
        self._args.extend(["-device", "usb-mouse,id=input0,bus=usb.0,port=1"])
        return self

    def with_usb_keyboard(self):
        """Add USB keyboard device"""
        if self._usb_keyboard_added:
            self._logger.debug("USB keyboard already added, skipping")
            return self
        
        if not self._usb_controller_added:
            self = self.with_usb_controller()

        self._usb_keyboard_added = True
        self._args.extend(["-device", "usb-kbd,id=input1,bus=usb.0,port=2"])
        return self

    def with_usb_storage(self, drive_file, drive_id=None, drive_format="raw"):
        """Add USB storage device"""
        if not drive_file:
            return self
        
        if not self._usb_controller_added:
            self = self.with_usb_controller()

        # Auto-generate unique ID if not provided
        if not drive_id:
            drive_id = f"usb_storage_{self._usb_storage_index}"
            self._usb_storage_index += 1

        self._logger.debug(
            f"Adding USB storage device: {drive_file} (id={drive_id}, format={drive_format})"
        )

        if os.path.isfile(drive_file):
            self._args.extend(
                [
                    "-drive",
                    f"file={drive_file},format={drive_format},media=disk,if=none,id={drive_id}",
                    "-device",
                    f"usb-storage,bus=usb.0,drive={drive_id}",
                ]
            )
        elif os.path.isdir(drive_file):
            self._args.extend(
                [
                    "-drive",
                    f"file=fat:rw:{drive_file},format={drive_format},media=disk,if=none,id={drive_id}",
                    "-device",
                    f"usb-storage,bus=usb.0,drive={drive_id}",
                ]
            )

        return self

    def with_virtual_drive(self, virtual_drive):
        """Mount virtual drive(Vhd, qcow2 img or directory)
        Args:
            virtual_drive: Path to virtual drive. Can be either:
                - A file path: Mounts the file as a virtio drive
                - A directory path: Mounts the directory as a FAT filesystem with read/write access
                - None/empty: No virtual drive will be mounted
        """

        if not virtual_drive:
            return self

        if os.path.isfile(virtual_drive):
            self._logger.debug(f"Mounting virtual drive file: {virtual_drive}")
            self._args.extend(["-drive", f"file={virtual_drive},if=virtio"])
        elif os.path.isdir(virtual_drive):
            self._logger.debug(
                "Mounting virtual drive directory as FAT filesystem: %s", virtual_drive
            )
            self._args.extend(
                ["-drive", f"file=fat:rw:{virtual_drive},format=raw,media=disk"]
            )
        else:
            self._logger.error(
                "Virtual drive path is invalid (not a file or directory): %s",
                virtual_drive,
            )

        return self

    def with_memory(self, size_mb):
        """Set memory size in MB"""
        if self._memory_added:
            self._logger.debug("Memory already configured, skipping")
            return self

        self._memory_added = True
        self._args.extend(["-m", str(size_mb)])
        return self

    def with_storage(self, path: os.PathLike, device: str):
        """Attaches storage to a device to be accessible by QEMU.
        
        The `path` suffix must be `.vhd`, `.qcow2`, or `.iso`.
        The `device` must be `cdrom`, `ssd`, `hdd`, or `usb`.
        
        Note: SSD is only supported on Q35.
        Note: HDD is only supported on SBSA.

        Args:
            path (os.PathLike): Path to the storage.
            device (str): The device to attach this storage to.

        Raises:
            Exception: The combination of architecture, path, and device are not supported.
        """
        if not path:
            return self
        
        self._logger.debug(f"Configuring storage: {path}")
        
        path = Path(path)
        device = device.lower()
        
        format = {
            ".vhd": "raw",
            ".qcow2": "qcow2",
            ".iso": "iso"
        }.get(path.suffix)
        
        if device == "cdrom" and format == "iso":
            self._args.extend(["-cdrom", f"{str(path)}"])
        elif device == "usb":
            self = self.with_usb_storage(path, drive_format = format)
        elif device == "ssd" and self._architecture == QemuArchitecture.Q35:
            self._args.extend([
                "-drive",
                f"file={path},format={format},if=none,id=os_nvme",
                "-device",
                "nvme,serial=nvme-1,drive=os_nvme",
            ])
        elif device == "hdd" and self._architecture == QemuArchitecture.SBSA:
            self._args.extend([
                "-drive",
                f"file={path},format={format},if=none,id=os_disk",
                "-device",
                "ahci,id=ahci",
                "-device",
                "ide-hd,drive=os_disk,bus=ahci.0",
            ])
        else:
            raise Exception(f"Unsupported storage combination: {device} && {format} && {self._architecture}")
            
        return self

    def with_network(self, enabled=True, forward_ports=None, use_virtio=False):
        """Configure network device with user mode networking

        Args:
            enabled (bool): Enable or disable networking.
                - True: Configures user mode networking (default)
                - False: Disables all networking (-net none)
            forward_ports (list): List of port numbers to forward from host to guest.
                Each port is forwarded as host:port -> guest:port.
                Example: [8270, 8271] forwards host:8270->guest:8270 and host:8271->guest:8271
                None: No port forwarding (ignored when enabled=False)
            use_virtio (bool): Use virtio network device instead of e1000.
                - True: Uses virtio-net-pci device (better performance, requires virtio drivers)
                - False: Uses e1000 device (broader compatibility, standard Ethernet emulation)
                (ignored when enabled=False)
        """
        if self._network_added:
            self._logger.debug("Network already configured, skipping")
            return self

        self._network_added = True
        if not enabled:
            self._logger.debug("Networking disabled")
            self._args.extend(["-net", "none"])
            return self

        netdev_config = "user,id=net0"

        if forward_ports:
            self._logger.debug(f"Configuring port forwarding: {forward_ports}")
            for port in forward_ports:
                netdev_config += f",hostfwd=tcp::{port}-:{port}"

        self._args.extend(["-netdev", netdev_config])

        if use_virtio:
            # Booting to UEFI, use virtio-net-pci
            self._args.extend(["-device", "virtio-net-pci,netdev=net0"])
        else:
            # Booting to Windows, use a PCI nic
            self._args.extend(["-device", "e1000,netdev=net0"])

        return self

    def with_smbios(self, smbios_values=None):
        """Configure SMBIOS information (idempotent - only adds once)

        Args:
            smbios_values: Dictionary of key-value pairs to customize SMBIOS fields.
                Supported keys (prefixed by SMBIOS type for clarity):

                Type 0 (BIOS Information):
                - 'smbios0_vendor': Vendor name (default: 'Patina')
                - 'smbios0_version': BIOS version (default: 'patina-q35-patched' for Q35, 'patina-sbsa-patched' for SBSA)
                - 'smbios0_date': Release date in MM/DD/YYYY format (default: current date)

                Type 1 (System Information):
                - 'smbios1_manufacturer': System manufacturer (default: 'OpenDevicePartnership')
                - 'smbios1_product': Product name (default: 'QEMU Q35' for Q35, 'QEMU SBSA' for SBSA)
                - 'smbios1_family': Product family (default: 'QEMU')
                - 'smbios1_version': System version (default: '10.0.0')
                - 'smbios1_serial': Serial number (default: '42-42-42-42')
                - 'smbios1_uuid': System UUID (default: '99fb60e2-181c-413a-a3cf-0a5fea8d87b0')

                Type 3 (Chassis Information):
                - 'smbios3_manufacturer': Chassis manufacturer (default: 'OpenDevicePartnership')
                - 'smbios3_serial': Serial number (default: '40-41-42-43' for Q35, '42-42-42-42' for SBSA)
                - 'smbios3_asset': Asset tag (default: 'Q35' for Q35, 'SBSA' for SBSA)
                - 'smbios3_sku': SKU number (default: 'Q35' for Q35, 'SBSA' for SBSA)
                - 'smbios3_version': Chassis version (default: '')
        """
        if self._smbios_added:
            self._logger.debug("SMBIOS already configured, skipping")
            return self

        self._smbios_added = True
        if self._architecture == QemuArchitecture.Q35:
            defaults = {
                # Type 0 (BIOS Information)
                "smbios0_vendor": "Patina",
                "smbios0_version": "patina-q35-patched",
                "smbios0_date": datetime.datetime.now().strftime("%m/%d/%Y"),
                # Type 1 (System Information)
                "smbios1_manufacturer": "OpenDevicePartnership",
                "smbios1_product": "QEMU Q35",
                "smbios1_family": "QEMU",
                "smbios1_version": "10.0.0",
                "smbios1_serial": "42-42-42-42",
                "smbios1_uuid": "99fb60e2-181c-413a-a3cf-0a5fea8d87b0",
                # Type 3 (Chassis Information)
                "smbios3_manufacturer": "OpenDevicePartnership",
                "smbios3_serial": "40-41-42-43",
                "smbios3_asset": "Q35",
                "smbios3_sku": "Q35",
                "smbios3_version": "",
            }
        else:
            defaults = {
                # Type 0 (BIOS Information)
                "smbios0_vendor": "Patina",
                "smbios0_version": "patina-sbsa-patched",
                "smbios0_date": datetime.datetime.now().strftime("%m/%d/%Y"),
                # Type 1 (System Information)
                "smbios1_manufacturer": "OpenDevicePartnership",
                "smbios1_product": "QEMU SBSA",
                "smbios1_family": "QEMU",
                "smbios1_version": "10.0.0",
                "smbios1_serial": "42-42-42-42",
                "smbios1_uuid": "99fb60e2-181c-413a-a3cf-0a5fea8d87b0",
                # Type 3 (Chassis Information)
                "smbios3_manufacturer": "OpenDevicePartnership",
                "smbios3_serial": "42-42-42-42",
                "smbios3_asset": "SBSA",
                "smbios3_sku": "SBSA",
                "smbios3_version": "",
            }

        if smbios_values:
            defaults.update(smbios_values)

        values = defaults

        self._args.extend(
            [
                "-smbios",
                f"type=0,vendor=\"{values['smbios0_vendor']}\",version=\"{values['smbios0_version']}\",date=\"{values['smbios0_date']}\",uefi=on",
                "-smbios",
                f"type=1,manufacturer=\"{values['smbios1_manufacturer']}\",product=\"{values['smbios1_product']}\",family=\"{values['smbios1_family']}\",version=\"{values['smbios1_version']}\",serial=\"{values['smbios1_serial']}\",uuid={values['smbios1_uuid']}",
                "-smbios",
                f"type=3,manufacturer=\"{values['smbios3_manufacturer']}\",serial=\"{values['smbios3_serial']}\",asset=\"{values['smbios3_asset']}\",sku=\"{values['smbios3_sku']}\",version=\"{values['smbios3_version']}\"",
            ]
        )

        return self

    def with_tpm(self, tpm_dev):
        """Configure TPM device"""
        if self._tpm_added:
            self._logger.debug("TPM already configured, skipping")
            return self

        if tpm_dev is None:
            return self

        self._tpm_added = True
        self._args.extend(
            [
                "-chardev",
                f"socket,id=chrtpm,path={tpm_dev}",
                "-tpmdev",
                "emulator,id=tpm0,chardev=chrtpm",
            ]
        )

        # Q35 uses tpm-tis, SBSA would use tpm-tis-device (not added here as original doesn't have it)
        if self._architecture == QemuArchitecture.Q35:
            self._args.extend(["-device", "tpm-tis,tpmdev=tpm0"])

        return self

    def with_display(self, enabled=True):
        """Configure display output

        Args:
            enabled (bool): Enable or disable display.
                - True: Configures display (VGA cirrus for Q35, default for SBSA)
                - False: Disables display (headless mode, -display none)
        """
        if self._display_added:
            self._logger.debug("Display already configured, skipping")
            return self

        self._display_added = True
        if not enabled:
            self._logger.debug("Display disabled (headless mode)")
            self._args.extend(["-display", "none"])
        elif self._architecture == QemuArchitecture.Q35:
            self._args.extend(["-vga", "cirrus"])

        return self

    def with_gdb_server(self, port, ip="127.0.0.1"):
        """Enable GDB server

        Args:
            port: Port number for GDB server
            ip: IP address to bind to (default: 127.0.0.1)
        """
        if self._gdb_server_added:
            self._logger.debug("GDB server already configured, skipping")
            return self

        if port:
            self._gdb_server_added = True
            self._logger.info(f"Enabling GDB server on tcp:{ip}:{port}")
            self._args.extend(["-gdb", f"tcp:{ip}:{port}"])
        return self

    def with_serial_port(self, port=None, log_files=None, ip="127.0.0.1"):
        """Configure serial port for console output

        Args:
            port: Port number for TCP serial connection (None for stdio)
            log_files: List of log files to write serial output to (only used when port is None)
            ip: IP address to bind to (default: 127.0.0.1)
        """
        if self._serial_port_added:
            self._logger.debug("Serial port already configured, skipping")
            return self

        self._serial_port_added = True
        if port:
            self._args.extend(["-serial", f"tcp:{ip}:{port},server,nowait"])
        else:
            self._args.extend(["-serial", "stdio"])
            if log_files:
                for log_file in log_files:
                    self._args.extend(["-serial", f"file:{log_file}"])
        return self

    def with_monitor_port(self, port, ip="127.0.0.1"):
        """Configure monitor port

        Args:
            port: Port number for monitor connection
            ip: IP address to bind to (default: 127.0.0.1)
        """
        if self._monitor_port_added:
            self._logger.debug("Monitor port already configured, skipping")
            return self

        if port:
            self._monitor_port_added = True
            self._args.extend(["-monitor", f"tcp:{ip}:{port},server,nowait"])
        return self

    def with_custom(self, *args):
        """Add custom QEMU arguments (can be called multiple times)

        Use this method to pass any QEMU command-line arguments not covered
        by the existing builder methods.

        Args:
            *args: Variable number of arguments to add to the command line.
                Can be passed as individual strings or as key-value pairs.

        Examples:
            # Single flag
            .with_custom("-snapshot")

            # Option with value
            .with_custom("-rtc", "base=utc")

            # Multiple arguments at once
            .with_custom("-no-reboot", "-no-shutdown")

            # Device configuration
            .with_custom("-device", "nvme,drive=nvme0,serial=deadbeef")
        """
        if args:
            self._args.extend(args)
        return self

    def get_executable(self):
        """Get the QEMU executable path"""
        return self._executable

    def build(self):
        """Build and return the executable and arguments as a tuple"""
        return (self._executable, self._args)

    def __str__(self):
        """Return the full command line as a string"""
        return f"{self._executable} {' '.join(self._args)}"
