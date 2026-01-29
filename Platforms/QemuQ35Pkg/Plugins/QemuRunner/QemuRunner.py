##
# This plugin runs the QEMU command and monitors for asserts.
# It can also possibly run tests and parse the results
#
# Copyright (c) Microsoft Corporation
# SPDX-License-Identifier: BSD-2-Clause-Patent
##

import logging
import os
import re
import io
import shutil
from edk2toolext.environment.plugintypes import uefi_helper_plugin
from edk2toollib import utility_functions

from QemuCommandBuilder import QemuCommandBuilder
from QemuCommandBuilder import QemuArchitecture


# """QEMU Command Builder for Q35 and SBSA architectures"""


class QemuRunner(uefi_helper_plugin.IUefiHelperPlugin):

    def __init__(self):
        self.logger = logging.getLogger(__name__)

    def RegisterHelpers(self, obj):
        fp = os.path.abspath(__file__)
        obj.Register("QemuRun", QemuRunner.Runner, fp)
        return 0

    @staticmethod
    # raw helper function to extract version number from QEMU
    def QueryQemuVersion(exec):
        if exec is None:
            return None

        result = io.StringIO()
        ret = utility_functions.RunCmd(exec, "--version", outstream=result)
        if ret != 0:
            logging.error(result.getvalue())
            logging.error(ret)
            return None

        # expected version string will be "QEMU emulator version maj.min.rev"
        res = result.getvalue()
        ver_str = re.search(r"version\s*([\d.]+)", res).group(1)

        return ver_str.split(".")

    @staticmethod
    def GetBuildBool(env, key: str, default: bool = False) -> bool:
        val = env.GetBuildValue(key)
        if val is None:
            return default
        return val.strip().lower() in ("true", "yes", "y", "1")

    @staticmethod
    def GetBuildStr(env, key: str, default: str | None = None) -> str | None:
        return env.GetBuildValue(key) or default

    @staticmethod
    def GetBool(env, key: str, default: bool = False) -> bool:
        val = env.GetValue(key)
        if val is None:
            return default
        return val.strip().lower() in ("true", "yes", "y", "1")

    @staticmethod
    def GetStr(env, key: str, default: str | None = None) -> str | None:
        return env.GetValue(key) or default

    @staticmethod
    def Runner(env):
        """Runs QEMU"""

        alt_boot_enable = QemuRunner.GetBool(env, "ALT_BOOT_ENABLE", False)
        boot_to_front_page = QemuRunner.GetBool(env, "BOOT_TO_FRONT_PAGE", False)
        core_count = QemuRunner.GetBuildStr(env, "QEMU_CORE_NUM")
        cpu_model = QemuRunner.GetStr(env, "CPU_MODEL")
        dfci_files = QemuRunner.GetStr(env, "DFCI_FILES")
        dfci_var_store = QemuRunner.GetStr(env, "DFCI_VAR_STORE")
        enable_network = QemuRunner.GetBool(env, "ENABLE_NETWORK", False)
        executable = QemuRunner.GetStr(env, "QEMU_PATH")
        gdb_server_port = QemuRunner.GetStr(env, "GDB_SERVER")
        headless = QemuRunner.GetBool(env, "QEMU_HEADLESS", False)
        install_files = QemuRunner.GetStr(env, "INSTALL_FILES")
        monitor_port = QemuRunner.GetStr(env, "MONITOR_PORT")
        output_path = QemuRunner.GetStr(env, "BUILD_OUTPUT_BASE")
        path_to_os = QemuRunner.GetStr(env, "PATH_TO_OS")
        os_boot_device = QemuRunner.GetStr(env, "OS_BOOT_DEVICE", "SSD")
        path_to_seed = QemuRunner.GetStr(env, "PATH_TO_SEED")
        qemu_accelerator = QemuRunner.GetStr(env, "QEMU_ACCEL")
        qemu_executable_path = QemuRunner.GetStr(env, "QEMU_PATH")
        qemu_ext_dep_dir = QemuRunner.GetStr(env, "QEMU_DIR")
        repo_version = QemuRunner.GetStr(env, "VERSION", "Unknown")
        serial_port = QemuRunner.GetStr(env, "SERIAL_PORT", "50001")
        shutdown_after_run = QemuRunner.GetBool(env, "SHUTDOWN_AFTER_RUN", False)
        smm_enabled = QemuRunner.GetBuildBool(env, "SMM_ENABLED", True)
        tpm_dev = QemuRunner.GetStr(env, "TPM_DEV")
        virtual_drive = QemuRunner.GetStr(env, "VIRTUAL_DRIVE_PATH")

        code_fd = os.path.join(output_path, "FV", "QEMUQ35_CODE.fd")
        orig_var_store = os.path.join(output_path, "FV", "QEMUQ35_VARS.fd")

        # Use a provided QEMU path. Otherwise use what is provided through the extdep
        if not qemu_executable_path:
            if qemu_ext_dep_dir:
                qemu_executable_path = os.path.join(qemu_ext_dep_dir, "qemu-system-x86_64")
            else:
                qemu_executable_path = "qemu-system-x86_64"

        # If we are using the QEMU external dependency, we need to tell it
        # where to look for roms
        rom_path = None
        if qemu_ext_dep_dir:
            rom_path = os.path.join(qemu_ext_dep_dir, "share")

        if dfci_var_store is not None:
            if not os.path.isfile(dfci_var_store):
                shutil.copy(orig_var_store, dfci_var_store)
            var_store = dfci_var_store
        else:
            var_store = orig_var_store

        forward_ports = None
        if enable_network:
            if dfci_var_store:
                forward_ports = [8270, 8271]

        boot_selection = ""
        if boot_to_front_page:
            boot_selection += "Vol+"

        if alt_boot_enable:
            boot_selection += "Vol-"

        use_virtio = boot_to_front_page or alt_boot_enable

        qemu_version = QemuRunner.QueryQemuVersion(qemu_executable_path)
        qemu_cmd_builder = (
            QemuCommandBuilder(qemu_executable_path, QemuArchitecture.Q35)
            .with_cpu(cpu_model, core_count)
            .with_machine(smm_enabled, qemu_accelerator)
            .with_memory(8192 if path_to_os else 2048)
            .with_firmware(code_fd, var_store)
            .with_rom_path(rom_path)
            .with_usb_controller()
            .with_usb_mouse()
            .with_usb_storage(dfci_files, "dfci_disk")
            .with_usb_storage(install_files, "install_disk")
            .with_storage(path_to_os, os_boot_device)
            .with_virtual_drive(None if path_to_os else virtual_drive)
            .with_display(not headless)
            .with_network(forward_ports, use_virtio)
            .with_smbios(
                smbios_values={
                    # Type 0 (BIOS Information)
                    "smbios0_vendor": "Patina",
                    "smbios0_version": repo_version,
                    # Type 1 (System Information)
                    "smbios1_manufacturer": "OpenDevicePartnership",
                    "smbios1_product": "QEMU Q35",
                    "smbios1_family": "QEMU",
                    "smbios1_version": str.join(".", qemu_version),
                    "smbios1_serial": "42-42-42-42",
                    "smbios1_uuid": "99fb60e2-181c-413a-a3cf-0a5fea8d87b0",
                    # Type 3 (Chassis Information)
                    "smbios3_manufacturer": "OpenDevicePartnership",
                    "smbios3_serial": "40-41-42-43",
                    "smbios3_asset": "Q35",
                    "smbios3_sku": "Q35",
                    "smbios3_version": boot_selection,
                }
            )
            .with_tpm(tpm_dev)
            .with_gdb_server(gdb_server_port)
            .with_serial_port(serial_port)
            .with_monitor_port(monitor_port)
            .with_shutdown_from_guest(shutdown_after_run)
        )

        if path_to_seed:
            qemu_cmd_builder = qemu_cmd_builder.with_custom("-drive", f"file=\"{path_to_seed}\",format=raw,if=virtio")

        ## TODO: Save the console mode. The original issue comes from: https://gitlab.com/qemu-project/qemu/-/issues/1674
        if os.name == "nt" and qemu_version[0] >= "8":
            import win32console

            std_handle = win32console.GetStdHandle(win32console.STD_INPUT_HANDLE)
            try:
                console_mode = std_handle.GetConsoleMode()
            except Exception:
                std_handle = None

        (executable, args) = qemu_cmd_builder.build()

        # Run QEMU
        ret = utility_functions.RunCmd(executable, str.join(" ", args))

        ## TODO: restore the customized RunCmd once unit tests with asserts are figured out
        if ret == 0xC0000005 or ret == 33:
            ret = 0

        ## TODO: remove this once we upgrade to newer QEMU
        if ret == 0x8B and qemu_version[0] == "4":
            # QEMU v4 will return segmentation fault when shutting down.
            # Tested same FDs on QEMU 6 and 7, not observing the same.
            ret = 0

        if os.name == "nt" and qemu_version[0] >= "8" and std_handle is not None:
            # Restore the console mode for Windows on QEMU v8+.
            std_handle.SetConsoleMode(console_mode)
        elif os.name != "nt":
            # Linux version of QEMU will mess with the print if its run failed, let's just restore it anyway
            utility_functions.RunCmd("stty", "sane", capture=False)
        return ret
