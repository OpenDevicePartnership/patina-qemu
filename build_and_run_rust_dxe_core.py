# @file build_and_run_rust_dxe_core.py
#
# Builds the Rust DXE Core binary, patches it into a QEMU platform firmware,
# and runs QEMU. This script is meant to be focused and fast for patching
# specific reference firmware images with a new Rust DXE Core. It is not meant
# to be a general purpose firmware patching tool.
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
        --qemu-rust-bin-repo (Path): Path to the QEMU Rust bin repository. Default is "C:/src/qemu_rust_bins".
        --fw-patch-repo (Path): Path to the firmware patch repository. Default is "C:/src/fw_rust_patcher".
        --build-target (str): Build target, either DEBUG or RELEASE. Default is "DEBUG".
        --platform (str): QEMU platform such as Q35. Default is "Q35".
        --toolchain (str): Toolchain to use for building. Default is "VS2022".

    Returns:
        argparse.Namespace: Parsed command-line arguments.
    """
    parser = argparse.ArgumentParser(description="Build and run Rust DXE Core.")
    parser.add_argument(
        "--qemu-rust-bin-repo",
        type=Path,
        default=Path("C:/src/qemu_rust_bins"),
        help="Path to the QEMU Rust bin repository.",
    )
    parser.add_argument(
        "--fw-patch-repo",
        type=Path,
        default=Path("C:/src/fw_rust_patcher"),
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
    args = parser.parse_args()
    if args.platform == "SBSA" and args.toolchain == "VS2022":
        args.toolchain = "CLANGPDB"
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
            - dxe_core: The path to the DXE core .efi binary.
            - fw_patch_repo: The path to the fw_rust_patcher repo.
            - patch_cmd: The command to patch the firmware.
            - qemu_cmd: The command to run QEMU with the specified settings.
            - qemu_rust_bin_repo: The path to the qemu_rust_bins repo.
            - ref_fd: The path to the file to use as a reference for patching.
            - toolchain: The toolchain used for building (e.g. VS2022).
    """
    uefi_rust_dir = Path(__file__).parent

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
        dxe_core = (
            args.qemu_rust_bin_repo
            / "target"
            / "x86_64-unknown-uefi"
            / ("release" if args.build_target.lower() == "release" else "debug")
            / "qemu_q35_dxe_core.efi"
        )

        build_cmd = [
            "cargo",
            "-Zunstable-options",
            "-C",
            str(args.qemu_rust_bin_repo),
            "build_q35",
        ]
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
            "-m",
            "2048",
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
            f"type=0,vendor='Project Mu',version='mu_tiano_platforms-v9.0.0',date={datetime.now().strftime('%m/%d/%Y')},uefi=on",
            "-smbios",
            "type=1,manufacturer=Palindrome,product='QEMU Q35',family=QEMU,version='9.0.0',serial=42-42-42-42,uuid=9de555c0-05d7-4aa1-84ab-bb511e3a8bef",
            "-smbios",
            "type=3,manufacturer=Palindrome,serial=40-41-42-43",
            "-vga",
            "cirrus",
            "-serial",
            "tcp:127.0.0.1:50001,server,nowait",
        ]
        patch_cmd = [
            "python",
            "patch.py",
            "-c",
            str(config_file),
            "-i",
            str(dxe_core),
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
        dxe_core = (
            args.qemu_rust_bin_repo
            / "target"
            / "aarch64-unknown-uefi"
            / ("release" if args.build_target.lower() == "release" else "debug")
            / "qemu_sbsa_dxe_core.efi"
        )

        build_cmd = [
            "cargo",
            "-Zunstable-options",
            "-C",
            str(args.qemu_rust_bin_repo),
            "build_sbsa",
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
            "-m",
            "2048",
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
            f"type=0,vendor='Project Mu',version='mu_tiano_platforms-v9.0.0',date={datetime.now().strftime('%m/%d/%Y')},uefi=on",
            "-smbios",
            "type=1,manufacturer=Palindrome,product='QEMU SBSA',family=QEMU,version='9.0.0',serial=42-42-42-42",
            "-smbios",
            "type=3,manufacturer=Palindrome,serial=42-42-42-42,asset=SBSA,sku=SBSA",
            "-serial",
            "stdio",
        ]
        patch_cmd = [
            "python",
            "patch.py",
            "-c",
            str(config_file),
            "-i",
            str(dxe_core),
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
        "dxe_core": dxe_core,
        "fw_patch_repo": args.fw_patch_repo,
        "patch_cmd": patch_cmd,
        "qemu_cmd": qemu_cmd,
        "qemu_rust_bin_repo": args.qemu_rust_bin_repo,
        "ref_fd": ref_fd,
        "toolchain": args.toolchain,
    }


def _print_configuration(settings: Dict[str, Path]) -> None:
    """
    Prints the current configuration settings.

    Args:
        settings (Dict[str, Path]): A dictionary containing configuration settings.
            - 'qemu_rust_bin_repo': Path to the qemu_rust_bins repo.
            - 'dxe_core': Path to the DXE Core .efi file.
            - 'fw_patch_repo': Path to the fw_rust_patcher repo.
            - 'build_target': The build target.
            - 'toolchain': The toolchain being used.
    """
    print("==Current Configuration==")
    print(f"QEMU Rust Bin Repo (qemu_rust_bins): {settings['qemu_rust_bin_repo']}")
    print(f"DXE Core EFI File: {settings['dxe_core']}")
    print(f"FW Patch Repo: {settings['fw_patch_repo']}")
    print(f"Build Target: {settings['build_target']}")
    print(f"Toolchain: {settings['toolchain']}\n")


def _build_rust_dxe_core(settings: Dict[str, Path]) -> None:
    """
    Build the Rust DXE Core based on the provided settings.

    Args:
        settings (Dict[str, Path]): A dictionary containing the build settings.
            - 'build_target' (str): The target build type.
            - 'build_cmd' (Path): The command to execute for building the Rust DXE Core.
    """
    print("[1]. Building Rust DXE Core...\n")
    if settings["build_target"] == "RELEASE":
        subprocess.run(settings["build_cmd"] + ["--profile", "release"], check=True)
    else:
        subprocess.run(settings["build_cmd"], check=True)


def _patch_rust_dxe_core(settings: Dict[str, Path]) -> None:
    """
    Patches the Rust DXE Core by copying the reference firmware directory if it does not exist
    and running the specified patch command.

    Args:
        settings (Dict[str, Path]): A dictionary containing the following keys:
            - 'ref_fd': Path to patch input (reference) FD file.
            - 'code_fd': Path to patch output FD file.
            - 'patch_cmd': Command to run for patching.
            - 'fw_patch_repo': Path to the fw_rust_patcher repo.
    """
    print("[2]. Patching Rust DXE Core...\n")

    if not settings["ref_fd"].exists():
        shutil.copy(settings["code_fd"], settings["ref_fd"])

    subprocess.run(settings["patch_cmd"], cwd=settings["fw_patch_repo"], check=True)


def _run_qemu(settings: Dict[str, Path]) -> None:
    """
    Runs QEMU with the specified settings.

    """
    print("[3]. Running QEMU with Rust DXE Core Build...\n")
    if os.name == 'nt':
        import win32console
        std_handle = win32console.GetStdHandle(win32console.STD_INPUT_HANDLE)
        try:
            console_mode = std_handle.GetConsoleMode()
        except Exception:
            std_handle = None
    try:
        subprocess.run(settings["qemu_cmd"], check=True)
    finally:
        if os.name == 'nt' and std_handle is not None:
            # Restore the console mode for Windows as QEMU garbles it
            std_handle.SetConsoleMode(console_mode)


def main() -> None:
    """
    Main function to build, patch, and run the Rust DXE core.

    """
    start_time = timeit.default_timer()

    print("Rust DXE Core Build and Runner\n")

    args = _parse_arguments()

    try:
        settings = _configure_settings(args)
    except ValueError as e:
        print(f"Error: {e}")
        exit(1)

    _print_configuration(settings)

    try:
        build_start_time = timeit.default_timer()
        _build_rust_dxe_core(settings)
        build_end_time = timeit.default_timer()
        print(
            f"Rust DXE Core Build Time: {build_end_time - build_start_time:.2f} seconds.\n"
        )
        _patch_rust_dxe_core(settings)
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
