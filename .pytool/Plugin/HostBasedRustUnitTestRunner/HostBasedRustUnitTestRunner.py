# @file RustHostBasedUnitTestRunner.py
# CiBuildPlugin used to run Cargo Test for all host based tests
##
# Copyright (c) Microsoft Corporation.
# SPDX-License-Identifier: BSD-2-Clause-Patent
##
from edk2toolext.environment.plugintypes.ci_build_plugin import ICiBuildPlugin
from edk2toollib.utility_functions import RunCmd
from pathlib import Path

import logging


class HostBasedRustUnitTestRunner(ICiBuildPlugin):
    def GetTestName(self, packagename: str, environment: object) -> tuple[str, str]:
        return (f'Run Host Based Unit Tests in {packagename}', f'{packagename}.HostBasedRustUnitTestRunner')

    def RunBuildPlugin(self, packagename, Edk2pathObj, pkgconfig, environment, PLM, PLMHelper, tc, output_stream):
        
        abs_pkg_path = Edk2pathObj.GetAbsolutePathOnThisSystemFromEdk2RelativePath(packagename)
        rust_pkg_list = [Path(x).parent for x in self.WalkDirectoryForExtension(['.toml'], abs_pkg_path)]

        failed = 0
        # PLMHelper.RustPackage() is a registered Helper Plugin (IUefiHelperPlugin) that points to
        # The RustPackage class defined in RustPackageHelper.py. Calling PLMHelper.RustPackage()
        # will return an instance of RustPackage.
        for rust_pkg in [PLMHelper.RustPackage(x) for x in rust_pkg_list]:
            # Skip any packages specified in CI config
            if rust_pkg.name in pkgconfig.get("IgnoreRustPkg", []):
                logging.info(f'{rust_pkg.name} skipped per ci settings.')
                continue
            
            # Clean and run tests
            rust_pkg.clean()
            output = rust_pkg.test(Edk2pathObj.WorkspacePath)
            failed += self.log_test_output(output, tc, rust_pkg.name)
        
        if failed > 0:
            tc.SetFailed(f'CargoTest failed. Errors {failed}', "CHECK_FAILED")
        else:
            tc.SetSuccess()

        return failed

    def log_test_output(self, output, tc, package):
        """Parses test output, logs necessary output, and keeps track of failures."""
        failed = 0
        for entry in output:
            if entry.get('type') != 'test':
                continue

            if entry.get('event') == 'ok':
                tc.LogStdOut(f'{package}::{entry.get("name")}....ok')

            if entry.get('event') == 'failed':
                logging.error(f'{package}::{entry.get("name")}....failed')
                logging.error(f'    {entry.get("stdout")}')
                tc.LogStdError(f'{package}::{entry.get("name")}....failed')
                tc.LogStdError(f'    {entry.get("stdout")}')
                failed += 1
        
        return failed