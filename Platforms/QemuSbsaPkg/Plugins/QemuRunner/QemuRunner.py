##
# This plugin runs the QEMU command and monitors for asserts.
# It can also possibly run tests and parse the results
#
# Copyright (c) Microsoft Corporation
# SPDX-License-Identifier: BSD-2-Clause-Patent
##

import logging
import io
import os
import re
import threading
from edk2toolext.environment.plugintypes import uefi_helper_plugin
from edk2toollib import utility_functions

from QemuCommandBuilder import QemuCommandBuilder
from QemuCommandBuilder import QemuArchitecture


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
    def RunThread(env):
        """Runs TPM in a separate thread"""
        tpm_path = env.GetValue("TPM_DEV")
        if tpm_path is None:
            logging.critical("TPM Path Invalid")
            return

        tpm_cmd = "swtpm"
        tpm_args = f"socket --tpmstate dir={'/'.join(tpm_path.rsplit('/', 1)[:-1])} --ctrl type=unixio,path={tpm_path} --tpm2 --log level=20"

        # Start the TPM emulator in a separate thread
        ret = utility_functions.RunCmd(tpm_cmd, tpm_args)
        if ret != 0:
            logging.critical("Failed to start TPM emulator.")
            return

    @staticmethod
    def Runner(env):
        """Runs QEMU"""

        alt_boot_enable = QemuRunner.GetBool(env, "ALT_BOOT_ENABLE", False)
        boot_to_front_page = QemuRunner.GetBool(env, "BOOT_TO_FRONT_PAGE", False)
        core_count = QemuRunner.GetBuildStr(env, "QEMU_CORE_NUM")
        cpu_model = QemuRunner.GetStr(env, "CPU_MODEL")
        executable = QemuRunner.GetStr(env, "QEMU_PATH")
        gdb_server_port = QemuRunner.GetStr(env, "GDB_SERVER")
        headless = QemuRunner.GetBool(env, "QEMU_HEADLESS", False)
        monitor_port = QemuRunner.GetStr(env, "MONITOR_PORT")
        output_path = QemuRunner.GetStr(env, "BUILD_OUTPUT_BASE")
        path_to_os = QemuRunner.GetStr(env, "PATH_TO_OS")
        path_to_seed = QemuRunner.GetStr(env, "PATH_TO_SEED")
        qemu_accelerator = QemuRunner.GetStr(env, "QEMU_ACCEL")
        qemu_executable_path = QemuRunner.GetStr(env, "QEMU_PATH")
        qemu_ext_dep_dir = QemuRunner.GetStr(env, "QEMU_DIR")
        repo_version = QemuRunner.GetStr(env, "VERSION", "Unknown")
        serial_port = QemuRunner.GetStr(env, "SERIAL_PORT")
        smm_enabled = QemuRunner.GetBuildBool(env, "SMM_ENABLED", True)
        tpm_dev = QemuRunner.GetStr(env, "TPM_DEV")
        virtual_drive = QemuRunner.GetStr(env, "VIRTUAL_DRIVE_PATH")

        code_fd = os.path.join(output_path, "FV", "QEMU_EFI.fd")
        var_store = os.path.join(output_path, "FV", "SECURE_FLASH0.fd")

        # Use a provided QEMU path. Otherwise use what is provided through the extdep
        if not qemu_executable_path:
            if qemu_ext_dep_dir:
                qemu_executable_path = os.path.join(qemu_ext_dep_dir, "qemu-system-aarch64")
            else:
                qemu_executable_path = "qemu-system-aarch64"

        # If we are using the QEMU external dependency, we need to tell it
        # where to look for roms
        rom_path = None
        if qemu_ext_dep_dir:
            rom_path = os.path.join(qemu_ext_dep_dir, "share")

        boot_selection = ""
        if boot_to_front_page:
            boot_selection += "Vol+"

        if alt_boot_enable:
            boot_selection += "Vol-"

        qemu_version = QemuRunner.QueryQemuVersion(qemu_executable_path)
        qemu_cmd_builder = (
            QemuCommandBuilder(qemu_executable_path, QemuArchitecture.SBSA)
            .with_cpu(cpu_model, core_count)
            .with_machine(smm_enabled, qemu_accelerator)
            .with_memory(8192 if path_to_os else 2048)
            .with_firmware(code_fd, var_store)
            .with_rom_path(rom_path)
            .with_os_storage(path_to_os)
            .with_usb_controller()
            .with_usb_mouse()
            .with_usb_keyboard()
            .with_virtual_drive(None if path_to_os else virtual_drive)
            .with_display(not headless)
            .with_network(False)
            .with_smbios(
                smbios_values={
                    # Type 0 (BIOS Information)
                    "smbios0_vendor": "Patina",
                    "smbios0_version": repo_version,
                    # Type 1 (System Information)
                    "smbios1_manufacturer": "OpenDevicePartnership",
                    "smbios1_product": "QEMU SBSA",
                    "smbios1_family": "QEMU",
                    "smbios1_version": str.join(".", qemu_version),
                    "smbios1_serial": "42-42-42-42",
                    "smbios1_uuid": "99fb60e2-181c-413a-a3cf-0a5fea8d87b0",
                    # Type 3 (Chassis Information)
                    "smbios3_manufacturer": "OpenDevicePartnership",
                    "smbios3_serial": "42-42-42-42",
                    "smbios3_asset": "SBSA",
                    "smbios3_sku": "SBSA",
                    "smbios3_version": boot_selection,
                }
            )
            .with_tpm(tpm_dev)
            .with_gdb_server(gdb_server_port)
            .with_serial_port(serial_port) # ["secure.log", "secure_mm.log"]
            .with_monitor_port(monitor_port)
        )
        
        if path_to_seed:
            qemu_cmd_builder = qemu_cmd_builder.with_custom("-drive", f"file=\"{path_to_seed}\",format=raw,if=virtio")

        (executable, args) = qemu_cmd_builder.build()
        logging.info(f"Running QEMU: {executable} {args}")

        thread = None
        if tpm_dev:
            # also spawn the TPM emulator on a different thread
            logging.critical("Starting TPM emulator in a different thread.")
            thread = threading.Thread(target=QemuRunner.RunThread, args=(env,))
            thread.start()

        ## TODO: Save the console mode. The original issue comes from: https://gitlab.com/qemu-project/qemu/-/issues/1674
        if os.name == "nt" and qemu_version[0] >= "8":
            import win32console

            std_handle = win32console.GetStdHandle(win32console.STD_INPUT_HANDLE)
            try:
                console_mode = std_handle.GetConsoleMode()
            except Exception:
                std_handle = None

        # Run QEMU
        ret = utility_functions.RunCmd(executable, str.join(" ", args))

        ## TODO: restore the customized RunCmd once unit tests with asserts are figured out
        if ret == 0xC0000005:
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

        if thread is not None:
            logging.critical("Terminate TPM emulator by using Crtl + C now!")
            thread.join()

        return ret
