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
import os
import shutil
import subprocess
import timeit
from datetime import datetime
from pathlib import Path
from typing import Dict


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
        "--custom-efi",
        "-c",
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
            - custom_efi: Whether a custom EFI file was provided.
            - efi_file: The path to the EFI file to patch.
            - fw_patch_repo: The path to the patina-fw-patcher repo.
            - patch_cmd: The command to patch the firmware.
            - qemu_cmd: The command to run QEMU with the specified settings.
            - patina_dxe_core_repo: The path to the patina-dxe-core-qemu repo.
            - ref_fd: The path to the file to use as a reference for patching.
            - toolchain: The toolchain used for building (e.g. VS2022).
    """
    uefi_rust_dir = Path(__file__).resolve().parent

    if args.platform == "Q35":
        code_fd = (
            uefi_rust_dir
            / "Build"
            / "QemuQ35Pkg"
            / f"{args.build_target.upper()}_{args.toolchain.upper()}"
            / "FV"
            / "QEMUQ35_CODE.fd"
        )
        ref_fd = code_fd.with_suffix(".ref.fd")
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
        # if a serial port wasn't specified, use the default port so a debugger can be retroactively attached
        if args.serial_port is None:
            args.serial_port = 50001
        qemu_cmd = [
            uefi_rust_dir
            / "QemuPkg"
            / "Binaries"
            / "qemu-win_extdep"
            / "qemu-system-x86_64",
            "-debugcon",
            "stdio",
            "-L",
            uefi_rust_dir / "QemuPkg" / "Binaries" / "qemu-win_extdep" / "share",
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
        code_fd = (
            uefi_rust_dir
            / "Build"
            / "QemuSbsaPkg"
            / f"{args.build_target.upper()}_{args.toolchain.upper()}"
            / "FV"
            / "QEMU_EFI.fd"
        )
        ref_fd = code_fd.with_suffix(".ref.fd")
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
        qemu_cmd = [
            str(
                uefi_rust_dir
                / "QemuPkg"
                / "Binaries"
                / "qemu-win_extdep"
                / "qemu-system-aarch64"
            ),
            "-net",
            "none",
            "-L",
            str(uefi_rust_dir / "QemuPkg" / "Binaries" / "qemu-win_extdep" / "share"),
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
    print("== Current Configuration ==")
    print(f" - QEMU Rust Bin Repo (patina-dxe-core-qemu): {settings['patina_dxe_core_repo']}")
    print(
        f" - {'Custom EFI' if settings['custom_efi'] else 'DXE Core'}: {settings['efi_file']}"
    )
    print(f" - Code FD File: {settings['code_fd']}")
    print(f" - FW Patch Repo: {settings['fw_patch_repo']}")
    print(f" - Build Target: {settings['build_target']}")
    print(f" - Toolchain: {settings['toolchain']}")
    print(f" - QEMU Command Line: {settings['qemu_cmd']}")
    print()


def _build_rust_dxe_core(settings: Dict[str, Path]) -> None:
    """
    Build the Rust DXE Core based on the provided settings.

    Args:
        settings (Dict[str, Path]): A dictionary containing the build settings.
            - 'build_cmd' (Path): The command to execute for building the Rust DXE Core.
            - 'build_target' (str): The target build type.
    """
    print("[1]. Building Rust DXE Core...\n")

    env = os.environ.copy()
    if "-Zunstable-options" in settings["build_cmd"]:
        env["RUSTC_BOOTSTRAP"] = "1"

    if settings["build_target"] == "RELEASE":
        subprocess.run(
            settings["build_cmd"] + ["--profile", "release"], check=True, env=env
        )
    else:
        subprocess.run(settings["build_cmd"], check=True, env=env)


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
    print(
        f"[2]. Patching {'Custom EFI' if settings['custom_efi'] else 'Rust DXE Core'}...\n"
    )

    shutil.copy(settings["code_fd"], settings["ref_fd"])

    subprocess.run(settings["patch_cmd"], cwd=settings["fw_patch_repo"], check=True)
    settings["ref_fd"].unlink()


def _run_qemu(settings: Dict[str, Path]) -> None:
    """
    Runs QEMU with the specified settings.

    """
    print("[3]. Running QEMU with Patched Binary...\n")
    if os.name == "nt":
        import win32console

        std_handle = win32console.GetStdHandle(win32console.STD_INPUT_HANDLE)
        try:
            console_mode = std_handle.GetConsoleMode()
        except Exception:
            std_handle = None
    try:
        subprocess.run(settings["qemu_cmd"], check=True)
    finally:
        if os.name == "nt" and std_handle is not None:
            # Restore the console mode for Windows as QEMU garbles it
            std_handle.SetConsoleMode(console_mode)


def main() -> None:
    """
    Main function to build, patch, and run the Rust DXE core.

    """
    start_time = timeit.default_timer()

    print("Rust Binary Build and Runner\n")

    args = _parse_arguments()

    try:
        settings = _configure_settings(args)
    except ValueError as e:
        print(f"Error: {e}")
        exit(1)

    _print_configuration(settings)

    try:
        if not settings["custom_efi"]:
            build_start_time = timeit.default_timer()
            _build_rust_dxe_core(settings)
            build_end_time = timeit.default_timer()
            print(
                f"Rust DXE Core Build Time: {build_end_time - build_start_time:.2f} seconds.\n"
            )

        _patch_rust_binary(settings)
        end_time = timeit.default_timer()
        print(
            f"Total time to get to kick off QEMU: {end_time - start_time:.2f} seconds.\n"
        )
        _run_qemu(settings)
    except subprocess.CalledProcessError as e:
        print(f"Failed with error #{e.returncode}.")
        exit(e.returncode)


if __name__ == "__main__":
    main()
