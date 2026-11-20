# @file TestAppSupport.py
#
# Helpers to run pre-compiled EFI test apps from a UEFI Shell startup script and to read back the JUnit results the
# apps write next to themselves. This module only depends on the standard library so it can be used from
# build_and_run_rust_binary.py without an edk2 build environment.
#
# The startup script protocol matches VirtualDriveManager.add_tests in QemuPkg/Plugins/VirtualDriveManager:
#
#   1. An app only runs when its <App>_JUNIT.XML does not exist. The file appears once the app has finished, which
#      lets apps that need a reset resume after the reboot instead of restarting.
#   2. After all apps have finished, stale <App>_JUNIT_RESULT.XML files and all *_Cache.dat files are removed.
#   3. Each <App>_JUNIT.XML is renamed to <App>_JUNIT_RESULT.XML so the results can be read after QEMU exits.
#
# Copyright (c) Microsoft Corporation. All rights reserved.
# SPDX-License-Identifier: BSD-2-Clause-Patent
##

import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Dict, List

RESULT_SUFFIX = "_JUNIT_RESULT.XML"
SCHEMA_VERSION = 1

# Locates the file system that holds startup.nsh since the shell does not always start on it.
_FS_FINDER = r"""#!/bin/nsh
echo -off
for %a run (0 10)
    if exist fs%a:\startup.nsh then
        fs%a:
        goto FOUND_IT
    endif
endfor

:FOUND_IT
"""


def build_startup_script(test_apps: List[str], shutdown: bool = True) -> str:
    """Builds the startup.nsh text that runs `test_apps` and optionally shuts the system down.

    Args:
        test_apps: File names (for example "ExampleTestApp.efi") present in the root of the drive.
        shutdown: Whether to append `reset -s` after the apps have finished.
    """
    lines = [_FS_FINDER.rstrip("\n")]

    for app in test_apps:
        stem = Path(app).stem
        lines += [f"if not exist {stem}_JUNIT.XML then", f"    {app}", "endif"]

    lines += ["rm *_JUNIT_RESULT.XML", "rm *_Cache.dat"]

    for app in test_apps:
        stem = Path(app).stem
        lines += [f"if exist {stem}_JUNIT.XML then", f"    mv {stem}_JUNIT.XML {stem}{RESULT_SUFFIX}", "endif"]

    if shutdown:
        lines.append("reset -s")

    return "\n".join(lines) + "\n"


def collect_results(drive_dir: Path, test_apps: List[str]) -> Dict:
    """Reads the JUnit results for `test_apps` from `drive_dir` and returns a JSON serializable summary.

    An app with a missing or unparsable result file is reported with `"result_found": False` and counts as a failure.
    """
    apps = []
    for app in test_apps:
        entry = {"name": app, "result_found": False, "cases": []}
        result_file = drive_dir / (Path(app).stem + RESULT_SUFFIX)

        if result_file.is_file():
            try:
                root = ET.fromstring(result_file.read_text(encoding="utf-8", errors="replace"))
                for suite in root:
                    for case in suite:
                        failure = next((child for child in case if child.tag == "failure"), None)
                        entry["cases"].append(
                            {
                                "suite": suite.attrib.get("name", ""),
                                "name": case.attrib.get("classname", case.attrib.get("name", "")),
                                "result": "fail" if failure is not None else "pass",
                                "message": failure.attrib.get("message", "") if failure is not None else "",
                            }
                        )
                entry["result_found"] = True
            except ET.ParseError:
                entry["cases"] = []

        apps.append(entry)

    return {"schema_version": SCHEMA_VERSION, "apps": apps}


def count_failures(summary: Dict) -> int:
    """Returns the number of failed cases plus apps that produced no usable result."""
    failures = 0
    for app in summary["apps"]:
        if not app["result_found"]:
            failures += 1
        failures += sum(1 for case in app["cases"] if case["result"] == "fail")
    return failures
