# @file build_and_run_rust_binary.py
#
# Builds the Rust DXE Core (if it is selected), patches a EFI into QEMU
# platform firmware, and runs QEMU. This script is meant to be focused and fast
# for patching specific reference firmware images with a new Rust DXE Core. It
# is not meant to be a general purpose firmware patching tool.
#
# Copyright (c) Microsoft Corporation. All rights reserved.
# SPDX-License-Identifier: BSD-2-Clause-Patent
##

import argparse
import logging
import os
import shutil
import subprocess
import sys
import timeit
from datetime import datetime
from pathlib import Path
from typing import Dict

SCRIPT_DIR = Path(__file__).resolve().parent


def _parse_arguments() -> argparse.Namespace:
    """
    Parses command-line arguments for building and running Rust DXE Core.

    Args:
        --patina-dxe-core-repo (Path): Path to the QEMU Rust bin repository. Default is "C:/src/patina-dxe-core-qemu".
        --fw-patch-repo (Path): Path to the firmware patch repository. Default is "C:/src/patina-fw-patcher".
        --build-target (str): Build target, either DEBUG or RELEASE. Default is "DEBUG".
        --platform (str): QEMU platform such as Q35. Default is "Q35".
        --toolchain (str): Toolchain to use for building. Default is "VS2022".

    Returns:
        argparse.Namespace: Parsed command-line arguments.
    """
    parser = argparse.ArgumentParser(
        description="Build and run Rust DXE Core.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--patina-dxe-core-repo",
        type=Path,
        default=Path("C:/src/patina-dxe-core-qemu"),
        help="Path to the QEMU Rust bin repository.",
    )
    parser.add_argument(
        "--crate-patch",
        action='append',
        type=Path,
        help="Additional repositories to patch the Patina DXE Core Repo with.",
        default=[],
    )
    parser.add_argument(
        "--config-file",
        "-c",
        type=Path,
        default=None,
        help="Path to a configuration file to use for patching. If not specified, "
        "the script will use the default configuration file for the selected platform.",
    )
    parser.add_argument(
        "--pre-compiled-rom",
        "-r",
        type=Path,
        default=None,
        help="A UEFI ROM file to patch. If not specified, the script will use the default "
        "UEFI ROM image for the platform target in the Build directory.",
    )
    parser.add_argument(
        "--custom-efi",
        "-e",
        type=Path,
        help="Path to a custom EFI to patch (instead of the Rust DXE Core).",
    )
    parser.add_argument(
        "--fw-patch-repo",
        type=Path,
        default=Path("C:/src/patina-fw-patcher"),
        help="Path to the firmware patch repository.",
    )
    parser.add_argument(
        "--qemu-path",
        "-q",
        type=Path,
        default=None,
        help="Path to the bin directory of the QEMU installation to use. If not specified, "
        "the script will use the default QEMU installation in the repo.",
    )
    parser.add_argument(
        "--build-target",
        "-b",
        choices=["DEBUG", "RELEASE"],
        default="DEBUG",
        help="Build target, either DEBUG or RELEASE.",
    )
    parser.add_argument(
        "--platform",
        "-p",
        choices=["Q35", "SBSA"],
        default="Q35",
        help="QEMU platform such as Q35 or SBSA.",
    )
    parser.add_argument(
        "--toolchain",
        "-t",
        type=str,
        default="VS2022",
        help="Toolchain to use for building. "
        "Q35 default: VS2022. SBSA default: CLANGPDB.",
    )
    parser.add_argument(
        "--os",
        type=Path,
        default=None,
        help="Path to OS image to boot in QEMU.",
    )
    parser.add_argument(
        "--serial-port",
        "-s",
        type=int,
        default=None,
        help="Port to use for serial communication.",
    )
    parser.add_argument(
        "--gdb-port",
        "-g",
        type=int,
        default=None,
        help="Port to use for GDB communication.",
    )

    args = parser.parse_args()
    if args.platform == "SBSA" and args.toolchain == "VS2022":
        args.toolchain = "CLANGPDB"

    if args.os:
        file_extension = args.os.suffix.lower().replace('"', "")

        storage_format = {
            ".vhd": "raw",
            ".qcow2": "qcow2",
            ".iso": "iso",
        }.get(file_extension, None)

        if storage_format is None:
            raise Exception(f"Unknown OS file type: {args.os}")

        os_arg = []

        if storage_format == "iso":
            os_arg = ["-cdrom", f" {args.os}"]
        else:
            if args.platform == "Q35":
                # Q35 uses NVMe
                os_arg = [
                    "-drive",
                    f"file={args.os},format={storage_format},if=none,id=os_nvme",
                    "-device",
                    "nvme,serial=nvme-1,drive=os_nvme",
                ]
            else:
                # There is a bug in Windows for NVMe on AARCH64, so use AHCI instead
                os_arg = [
                    "-drive",
                    f"file={args.os},format={storage_format},if=none,id=os_disk",
                    "-device",
                    "ahci,id=ahci",
                    "-device",
                    "ide-hd,drive=os_disk,bus=ahci.0",
                ]
        args.os = os_arg
    return args


def _configure_settings(args: argparse.Namespace) -> Dict[str, Path]:
    """
    Configures the settings based on the provided command-line arguments.

    Args:
        args (argparse.Namespace): The command-line arguments provided to the script.

    Returns:
        Dict[str, Path]: A dictionary containing the configuration settings, including:
            - build_cmd: The command to build the Rust DXE core.
            - build_target: The build target (e.g., RELEASE or DEBUG).
            - code_fd: The path to the QEMU platform code FD file to patch.
            - config_file: The path to the configuration file for patching (None uses default).
            - custom_efi: Whether a custom EFI file was provided.
            - efi_file: The path to the EFI file to patch.
            - fw_patch_repo: The path to the patina-fw-patcher repo.
            - patch_cmd: The command to patch the firmware.
            - pre_compiled_rom: The path to the pre-compiled ROM file (if provided).
            - qemu_cmd: The command to run QEMU with the specified settings.
            - qemu_path: The path to the QEMU installation (None uses default).
            - patina_dxe_core_repo: The path to the patina-dxe-core-qemu repo.
            - ref_fd: The path to the file to use as a reference for patching.
            - toolchain: The toolchain used for building (e.g. VS2022).
    """
    if args.platform == "Q35":
        if args.pre_compiled_rom:
            code_fd = args.pre_compiled_rom
        else:
            code_fd = (
                SCRIPT_DIR
                / "Build"
                / "QemuQ35Pkg"
                / f"{args.build_target.upper()}_{args.toolchain.upper()}"
                / "FV"
                / "QEMUQ35_CODE.fd"
            )
        ref_fd = code_fd.with_suffix(".ref.fd")
        if args.config_file:
            config_file = args.config_file
        else:
            config_file = args.fw_patch_repo / "Configs" / "QemuQ35.json"

        if args.custom_efi:
            efi_file = args.custom_efi
        else:
            efi_file = (
                args.patina_dxe_core_repo
                / "target"
                / "x86_64-unknown-uefi"
                / ("release" if args.build_target.lower() == "release" else "debug")
                / "qemu_q35_dxe_core.efi"
            )

        build_cmd = [
            "cargo",
            "-Zunstable-options",
            "-C",
            str(args.patina_dxe_core_repo),
            "make",
            "q35",
        ]
        build_cmd.extend([str(p) for p in args.crate_patch])
        # if a serial port wasn't specified, use the default port so a debugger can be retroactively attached
        if args.serial_port is None:
            args.serial_port = 50001

        if args.qemu_path:
            qemu_exec = args.qemu_path
        else:
            qemu_exec = str(
                SCRIPT_DIR
                / "QemuPkg"
                / "Binaries"
                / "qemu-win_extdep"
                / "qemu-system-x86_64"
            )
        qemu_cmd = [
            qemu_exec,
            "-debugcon",
            "stdio",
            "-L",
            SCRIPT_DIR / "QemuPkg" / "Binaries" / "qemu-win_extdep" / "share",
            "-global",
            "isa-debugcon.iobase=0x402",
            "-global",
            "ICH9-LPC.disable_s3=1",
            "-machine",
            "q35,smm=on",
            "-cpu",
            "qemu64,rdrand=on,umip=on,smep=on,pdpe1gb=on,popcnt=on,+sse,+sse2,+sse3,+ssse3,+sse4.2,+sse4.1,invtsc",
            "-smp",
            "4",
            "-global",
            "driver=cfi.pflash01,property=secure,value=on",
            "-drive",
            f"if=pflash,format=raw,unit=0,file={str(code_fd)},readonly=on",
            "-drive",
            "if=pflash,format=raw,unit=1,file="
            + str(code_fd.parent / "QEMUQ35_VARS.fd"),
            "-device",
            "qemu-xhci,id=usb",
            "-device",
            "usb-tablet,id=input0,bus=usb.0,port=1",
            "-net",
            "none",
            "-smbios",
            f"type=0,vendor='Patina',version='patina-q35-patched',date={datetime.now().strftime('%m/%d/%Y')},uefi=on",
            "-smbios",
            "type=1,manufacturer=OpenDevicePartnership,product='QEMU Q35',family=QEMU,version='9.0.0',serial=42-42-42-42,uuid=99fb60e2-181c-413a-a3cf-0a5fea8d87b0",
            "-smbios",
            "type=3,manufacturer=OpenDevicePartnership,serial=40-41-42-43",
            "-vga",
            "cirrus",
            "-serial",
            f"tcp:127.0.0.1:{args.serial_port},server,nowait",
        ]
        if args.gdb_port:
            qemu_cmd += ["-gdb", f"tcp::{args.gdb_port}"]

        if args.os:
            qemu_cmd += args.os
            qemu_cmd += ["-m", "8192"]
        else:
            qemu_cmd += ["-m", "2048"]

        patch_cmd = [
            "python",
            "patch.py",
            "-c",
            str(config_file),
            "-i",
            str(efi_file),
            "-r",
            str(ref_fd),
            "-o",
            str(code_fd),
        ]
    elif args.platform == "SBSA":
        if args.pre_compiled_rom:
            code_fd = args.pre_compiled_rom
        else:
            code_fd = (
                SCRIPT_DIR
                / "Build"
                / "QemuSbsaPkg"
                / f"{args.build_target.upper()}_{args.toolchain.upper()}"
                / "FV"
                / "QEMU_EFI.fd"
            )
        ref_fd = code_fd.with_suffix(".ref.fd")
        if args.config_file:
            config_file = args.config_file
        else:
            config_file = args.fw_patch_repo / "Configs" / "QemuSbsa.json"
        if args.custom_efi:
            efi_file = args.custom_efi
        else:
            efi_file = (
                args.patina_dxe_core_repo
                / "target"
                / "aarch64-unknown-uefi"
                / ("release" if args.build_target.lower() == "release" else "debug")
                / "qemu_sbsa_dxe_core.efi"
            )

        build_cmd = [
            "cargo",
            "-Zunstable-options",
            "-C",
            str(args.patina_dxe_core_repo),
            "make",
            "sbsa",
        ]
        build_cmd.extend([str(p) for p in args.crate_patch])
        if args.qemu_path:
            qemu_exec = args.qemu_path, "qemu-system-aarch64"
        else:
            qemu_exec = str(
                SCRIPT_DIR
                / "QemuPkg"
                / "Binaries"
                / "qemu-win_extdep"
                / "qemu-system-aarch64"
            )
        qemu_cmd = [
            qemu_exec,
            "-net",
            "none",
            "-L",
            str(SCRIPT_DIR / "QemuPkg" / "Binaries" / "qemu-win_extdep" / "share"),
            "-machine",
            "sbsa-ref",
            "-cpu",
            "max,sve=off,sme=off",
            "-smp",
            "4",
            "-global",
            "driver=cfi.pflash01,property=secure,value=on",
            "-drive",
            f"if=pflash,format=raw,unit=0,file={str(code_fd.parent / 'SECURE_FLASH0.fd')}",
            "-drive",
            f"if=pflash,format=raw,unit=1,file={str(code_fd)},readonly=on",
            "-device",
            "qemu-xhci,id=usb",
            "-device",
            "usb-tablet,id=input0,bus=usb.0,port=1",
            "-device",
            "usb-kbd,id=input1,bus=usb.0,port=2",
            "-smbios",
            f"type=0,vendor='Patina',version='patina-sbsa-patched',date={datetime.now().strftime('%m/%d/%Y')},uefi=on",
            "-smbios",
            "type=1,manufacturer=OpenDevicePartnership,product='QEMU SBSA',family=QEMU,version='9.0.0',serial=42-42-42-42",
            "-smbios",
            "type=3,manufacturer=OpenDevicePartnership,serial=42-42-42-42,asset=SBSA,sku=SBSA",
        ]
        if args.serial_port:
            qemu_cmd += ["-serial", f"tcp:127.0.0.1:{args.serial_port},server,nowait"]
        else:
            qemu_cmd += ["-serial", "stdio"]

        if args.gdb_port:
            qemu_cmd += ["-gdb", f"tcp::{args.gdb_port}"]

        if args.os:
            qemu_cmd += args.os
            qemu_cmd += ["-m", "8192"]
        else:
            qemu_cmd += ["-m", "2048"]
        patch_cmd = [
            "python",
            "patch.py",
            "-c",
            str(config_file),
            "-i",
            str(efi_file),
            "-r",
            str(ref_fd),
            "-o",
            str(code_fd),
        ]
    else:
        raise ValueError(f"Unsupported platform: {args.platform}")

    return {
        "build_cmd": build_cmd,
        "build_target": args.build_target,
        "code_fd": code_fd,
        "custom_efi": args.custom_efi is not None,
        "efi_file": efi_file,
        "fw_patch_repo": args.fw_patch_repo,
        "patch_cmd": patch_cmd,
        "qemu_cmd": qemu_cmd,
        "patina_dxe_core_repo": args.patina_dxe_core_repo,
        "ref_fd": ref_fd,
        "toolchain": args.toolchain,
    }


def _print_configuration(settings: Dict[str, Path]) -> None:
    """
    Prints the current configuration settings.

    Args:
        settings (Dict[str, Path]): A dictionary containing configuration settings.
            - 'build_target': The build target.
            - 'custom_efi': Path to a custom EFI file to patch (instead of the Rust DXE Core).
            - 'efi_file': Path to the .efi file to patch.
            - 'fw_patch_repo': Path to the patina-fw-patcher repo.
            - 'qemu_cmd': The command to run QEMU with the specified settings.
            - 'patina_dxe_core_repo': Path to the patina-dxe-core-qemu repo.
            - 'toolchain': The toolchain being used.
    """
    logging.info("== Current Configuration ==")
    logging.info(
        f" - QEMU Rust Bin Repo (patina-dxe-core-qemu): {settings['patina_dxe_core_repo']}"
    )
    logging.info(
        f" - {'Custom EFI' if settings['custom_efi'] else 'DXE Core'}: {settings['efi_file']}"
    )
    logging.info(f" - Code FD File: {settings['code_fd']}")
    logging.info(f" - FW Patch Repo: {settings['fw_patch_repo']}")
    logging.info(f" - Build Target: {settings['build_target']}")
    logging.info(f" - Toolchain: {settings['toolchain']}")
    logging.info(f" - QEMU Command Line: {settings['qemu_cmd']}")


def _build_rust_dxe_core(settings: Dict[str, Path]) -> None:
    """
    Build the Rust DXE Core based on the provided settings.

    Args:
        settings (Dict[str, Path]): A dictionary containing the build settings.
            - 'build_cmd' (Path): The command to execute for building the Rust DXE Core.
            - 'build_target' (str): The target build type.
    """
    logging.info("[1]. Building Rust DXE Core...\n")

    env = os.environ.copy()
    if "-Zunstable-options" in settings["build_cmd"]:
        env["RUSTC_BOOTSTRAP"] = "1"

    try:
        if settings["build_target"] == "RELEASE":
            subprocess.run(
                settings["build_cmd"] + ["--profile", "release"], check=True, env=env
            )
        else:
            subprocess.run(settings["build_cmd"], check=True, env=env)
    except subprocess.CalledProcessError as e:
        logging.error(f"Build failed with error #{e.returncode}.")
        sys.exit(e.returncode)


def _patch_rust_binary(settings: Dict[str, Path]) -> None:
    """
    Patches the binary by copying the reference firmware directory if it does not exist
    and running the specified patch command.

    Args:
        settings (Dict[str, Path]): A dictionary containing the following keys:
            - 'code_fd': Path to patch output FD file.
            - 'custom_efi': Whether a custom EFI file was provided.
            - 'fw_patch_repo': Path to the patina-fw-patcher repo.
            - 'patch_cmd': Command to run for patching.
            - 'ref_fd': Path to patch input (reference) FD file.
    """
    logging.info(
        f"[2]. Patching {'Custom EFI' if settings['custom_efi'] else 'Rust DXE Core'}...\n"
    )

    shutil.copy(settings["code_fd"], settings["ref_fd"])

    subprocess.run(settings["patch_cmd"], cwd=settings["fw_patch_repo"], check=True)
    settings["ref_fd"].unlink()


def _run_qemu(settings: Dict[str, Path]) -> None:
    """
    Runs QEMU with the specified settings.

    """
    logging.info("[3]. Running QEMU with Patched Binary...\n")
    if os.name == "nt":
        import ctypes

        kernel32 = ctypes.windll.kernel32
        STD_INPUT_HANDLE = -10
        std_handle = kernel32.GetStdHandle(STD_INPUT_HANDLE)
        original_mode = ctypes.c_uint()
        if std_handle != 0:
            if not kernel32.GetConsoleMode(std_handle, ctypes.byref(original_mode)):
                std_handle = None
    try:
        subprocess.run(settings["qemu_cmd"], check=True)
    finally:
        if os.name == "nt" and std_handle and original_mode.value:
            # Restore the console mode for Windows as QEMU garbles it
            kernel32.SetConsoleMode(std_handle, original_mode.value)


def main() -> None:
    """
    Main function to build, patch, and run the Rust DXE core.

    """
    start_time = timeit.default_timer()

    root_logger = logging.getLogger()
    root_logger.setLevel(logging.DEBUG)

    stdout_logger_handler = logging.StreamHandler(sys.stdout)
    stdout_logger_handler.set_name("stdout_logger_handler")
    stdout_logger_handler.setLevel(logging.INFO)
    stdout_logger_handler.setFormatter(logging.Formatter("%(message)s"))
    root_logger.addHandler(stdout_logger_handler)

    logging.info("Patina Rust Binary Build and Runner")

    args = _parse_arguments()

    try:
        settings = _configure_settings(args)
    except ValueError as e:
        logging.error(f"Error: {e}")
        exit(1)

    _print_configuration(settings)

    try:
        if not settings["custom_efi"]:
            build_start_time = timeit.default_timer()
            _build_rust_dxe_core(settings)
            build_end_time = timeit.default_timer()
            logging.info(
                f"Rust DXE Core Build Time: {build_end_time - build_start_time:.2f} seconds.\n"
            )

        _patch_rust_binary(settings)
        end_time = timeit.default_timer()
        logging.info(
            f"Total time to get to kick off QEMU: {end_time - start_time:.2f} seconds.\n"
        )
        _run_qemu(settings)
    except subprocess.CalledProcessError as e:
        logging.error(f"Failed with error #{e.returncode}.")
        exit(e.returncode)


if __name__ == "__main__":
    main()
