#!/usr/bin/env python3
# A script to generate shields.io endpoint badge JSON and consolidated data files for the Patina
# Compatibility Matrix from metadata artifacts produced by repo test workflows.
#
# Usage:
#   python3 generate_compat_badges.py --cells-dir <dir> --images-dir <dir> --output-dir <dir>
##
# Copyright (c) Microsoft Corporation.
#
# SPDX-License-Identifier: BSD-2-Clause-Patent
##

import argparse
import json
import os
import sys
from pathlib import Path

# Colors representing each image/distro, used for OS-version and bootloader badges. Add an entry
# here when a new OS family (e.g. Fedora) is introduced. Unknown families fall back to DEFAULT_COLOR.
DISTRO_COLORS = {
    "ubuntu": "77216F",
    "windows": "0078D4",
}

DEFAULT_COLOR = "blue"


def load_json_files(directory: Path) -> list:
    """Loads every *.json file directly under `directory` (non-recursive), skipping a missing directory."""
    items = []
    if not directory.is_dir():
        return items
    for path in sorted(directory.glob("*.json")):
        with path.open(encoding="utf-8") as handle:
            items.append(json.load(handle))
    return items


def write_json(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def write_badge(path: Path, label: str, message: str, color: str) -> None:
    write_json(path, {"schemaVersion": 1, "label": label, "message": message, "color": color})


def result_badge_fields(result: str) -> tuple:
    """Returns (message, color) for a shields.io badge based on a pass/fail cell result."""
    if result == "success":
        return "passing", "success"
    return "failing", "critical"


def build_environment(out_dir: Path) -> None:
    environment = {
        "schema_version": 1,
        "patina_sdk_version": os.environ["PATINA_SDK_VERSION"],
        "patina_dxe_core_qemu_version": os.environ["PATINA_DXE_CORE_QEMU_VERSION"],
        "patina_qemu_commit": os.environ["PATINA_QEMU_COMMIT"],
        "run_id": os.environ["RUN_ID"],
        "run_url": os.environ["RUN_URL"],
        "timestamp": os.environ["TIMESTAMP"],
    }
    write_json(out_dir / "data" / "environment.json", environment)

    write_badge(
        out_dir / "badges" / "env" / "patina-sdk.json",
        "patina",
        f"v{environment['patina_sdk_version']}",
        DEFAULT_COLOR,
    )
    write_badge(
        out_dir / "badges" / "env" / "patina-dxe-core-qemu.json",
        "patina-dxe-core-qemu",
        f"v{environment['patina_dxe_core_qemu_version']}",
        DEFAULT_COLOR,
    )
    write_badge(
        out_dir / "badges" / "env" / "patina-qemu-commit.json",
        "patina-qemu",
        environment["patina_qemu_commit"][:7],
        "lightgrey",
    )
    write_badge(
        out_dir / "badges" / "env" / "last-run.json",
        "last run",
        environment["timestamp"].replace("T", " ").rstrip("Z") + " UTC",
        "lightgrey",
    )


def build_cells(out_dir: Path, cells: list) -> None:
    for cell in cells:
        cell_id = cell["cell_id"]
        write_json(out_dir / "data" / "cells" / f"{cell_id}.json", cell)

        message, color = result_badge_fields(cell["result"])
        write_badge(out_dir / "badges" / "result" / f"{cell_id}.json", cell["target"].lower(), message, color)


def build_images(out_dir: Path, images: list) -> None:
    for image in images:
        os_artifact = image["os_artifact"]
        write_json(out_dir / "data" / "images" / f"{os_artifact}.json", image)

        color = DISTRO_COLORS.get(image.get("os_family", ""), DEFAULT_COLOR)
        label = os_artifact.replace("-image", "").replace("-", " ")

        write_badge(
            out_dir / "badges" / "images" / f"{os_artifact}-os-version.json",
            label,
            image["os_version"],
            color,
        )

        bootloader = image.get("bootloader")
        if bootloader:
            write_badge(
                out_dir / "badges" / "images" / f"{os_artifact}-bootloader.json",
                "bootloader",
                f"{bootloader['name']} {bootloader['version']}",
                color,
            )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cells-dir", required=True, type=Path)
    parser.add_argument("--images-dir", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()

    cells = load_json_files(args.cells_dir)
    images = load_json_files(args.images_dir)

    if not cells:
        print(f"Warning: no compatibility cell metadata found under {args.cells_dir}", file=sys.stderr)
    if not images:
        print(f"Warning: no OS image metadata found under {args.images_dir}", file=sys.stderr)

    build_environment(args.output_dir)
    build_cells(args.output_dir, cells)
    build_images(args.output_dir, images)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
