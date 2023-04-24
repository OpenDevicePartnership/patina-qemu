# @file RustCoverageCheck.py
# CiBuildPlugin compute coverage information for each
# rust package in a given edk2 package.
##
# Copyright (c) Microsoft Corporation.
# SPDX-License-Identifier: BSD-2-Clause-Patent
##
import logging
from pathlib import Path
from edk2toolext.environment.plugintypes.ci_build_plugin import ICiBuildPlugin
from edk2toollib.utility_functions import GetHostInfo
import re


class RustCoverageCheck(ICiBuildPlugin):
    def GetTestName(self, packagename: str, environment: object) -> tuple[str, str]:
        return (f'Code Coverage in {packagename}', f'{packagename}.RustCoverageCheck')

    def RunBuildPlugin(self, packagename, Edk2pathObj, pkgconfig, environment, PLM, PLMHelper, tc, output_stream):
        if GetHostInfo().os == "Windows":
            tc.SetSkipped()
            # TODO: works with 1.70 nightly. Try again when 1.70 is released.
            tc.LogStdOut("Rust Coverage unsupported on Windows.")
            return 0

        failed = 0
        workspace = PLMHelper.RustWorkspace(Edk2pathObj.WorkspacePath)
        default_coverage = pkgconfig.get("Default", 0.75)

        # Run coverage on the workspace
        try:
            self.run_workspace_coverage(workspace)
        except RuntimeError as e:
            tc.LogStdError(str(e))
            logging.error(str(e))
            failed += 1

        # Run coverage on the individual packages
        for rust_pkg in workspace.members:
            coverage_req = pkgconfig["PackageOverrides"].get(rust_pkg.name, default_coverage)

            # Run the coverage and filter for only files within the rust package
            try:
                output = rust_pkg.coverage(Edk2pathObj.WorkspacePath)
            except RuntimeError as e:
                tc.LogStdError(str(e))
                logging.error(str(e))
                failed += 1
            filtered_output = [x for x in output if x.get("package") in x.get('path')]

            # No tests for this rust package, skip
            if len(filtered_output) == 0:
                continue

            # Log and calculate overall coverage for a package.
            total = 0.0
            covered = 0.0
            for entry in filtered_output:
                total += entry.get("total")
                covered += entry.get("covered")
                tc.LogStdOut(f'{entry.get("package")}@{entry.get("path")}  [{entry.get("covered")}/{entry.get("total")} lines covered]')
                logging.debug(f'{entry.get("package")}@{entry.get("path")}  [{entry.get("covered")}/{entry.get("total")} lines covered]')

            if covered / total < coverage_req:
                tc.LogStdError(f'Coverage for {rust_pkg.name} is below {coverage_req}')
                logging.error(f'Coverage for {rust_pkg.name} is below {coverage_req}')
                failed += 1

        if failed > 0:
            tc.SetFailed(f'RustCoverageCheck failed. Errors {failed}', "CHECK_FAILED")
        else:
            tc.SetSuccess()
        return failed

    def run_workspace_coverage(self, workspace: 'RustWorkspace'):
        """Runs coverage on the workspace and places the output at the output path."""
        try:
            workspace.coverage(report_type = "xml")
        except RuntimeError as e:
            logging.error(f'Coverage failed for workspace.')
            return -1

        xml = Path(workspace.path) / "target" / "cobertura.xml"
        out = Path(workspace.path) / "Build"
        xml = xml.rename(out / "coverage.xml")

        with open(xml, 'r') as f:
            contents = f.read()
            contents = re.sub(r'<source>(.*?)</source>', r'<source>.</source>', contents)
        
        with open (xml, "w") as f:
            f.write(contents)

    def run_package_coverage(self, package: 'RustPackage'):
        """Runs coverage on the package and places the output at the output path."""

