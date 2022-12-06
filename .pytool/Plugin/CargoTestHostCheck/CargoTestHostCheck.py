# @file CargoTestHostCheck.py
# CiBuildPlugin used to run Cargo Test for all host based tests
##
# Copyright (c) Microsoft Corporation.
# SPDX-License-Identifier: BSD-2-Clause-Patent
##
from edk2toolext.environment.plugintypes.ci_build_plugin import ICiBuildPlugin
from edk2toolext.environment import shell_environment
from edk2toollib.utility_functions import RunCmd
from pathlib import Path
import io
import logging


class RustPackage:
    def __init__(self, path: Path, triple: str = None):
        self.path = path
        self.name = path.name
        self.triple = triple if triple is not None else self.__get_triple()

    def clean(self):
        """Cleans any build artifacts from the directory.
        
        Ensures tests are freshly built and executed.
        """

        command = "cargo"
        parameters = "clean"

        RunCmd(command, parameters, workingdir=self.path)

    def run_tests(self):
        """Runs tests and returns output in json format.

        Returns:
            (dict): dict containing all test results
        """

        command = "cargo"
        parameters = f"test -q --no-fail-fast --target={self.triple} -Z build-std-features -Z build-std -- -Z unstable-options --format json"
        output = io.StringIO()

        RunCmd(command, parameters, workingdir=self.path, outstream=output)

        output.seek(0)
        return self.__clean_test_output(output)

    def verify_lib(self):
        """Verifies if a package has a library, and if the library compiles.

        Returns:
            (int): 0 If a library exists and compiles
            (int): 1 If a library exists but does not compile
            (int): -1 If a library does not exist

        """
        return self.__cargo_build("lib")

    def verify_tests(self):
        """Verifies if a package has tests, and if the tests compile.

        Returns:
            (int): 0 If tests exist and compile
            (int): 1 If tests exist but do not compile
            (int): -1 If a library does not exist
        """
        return self.__cargo_build("tests")

    def __cargo_build(self, target: str = "all-targets", triple: str = None, extra: str = ""):
        """Runs cargo build on the selected target.

        Args:
            target (str, default=all-targets): lib, bins, examples, tests, benches, all-targets
            triple (str, default=host's triple): any triple specified by `rustc --print target-list`
            extra (str): Any extra params to use with cargo check

        Returns:
            (int): 0 if `target` exists and compiles
            (int): 1 if `target` exists but does not compile
            (int): -1 if `target` does not exist
        """
        if triple is None:
            triple = self.triple
        command = "cargo"
        params = f"build --{target} --target={triple} {extra} -Z build-std-features -Z build-std"
        output = io.StringIO()

        RunCmd(command, params, workingdir=self.path, outstream=output)

        output.seek(0)
        for line in output.readlines():
            line = line.strip()
            logging.debug(line)
            if line.startswith('error: no') and 'found in package' in line:
                return -1

            if line.startswith('error: could not compile'):
                return 1

        return 0

    def __get_triple(self):
        """Returns the host triple."""
        command = "rustc"
        params = "-vV"
        output = io.StringIO()

        ret = RunCmd(command, params, workingdir=self.path, outstream=output)

        if ret != 0:
            raise Exception("Failed to get target triple")

        output.seek(0)

        for line in output.readlines():
            if line.startswith('host'):
                return line.split(":")[1].strip()
        raise Exception("Failed to get target triple")

    def __clean_test_output(self, output: io.StringIO):
        """Clean output to only contain result lines

        RunCmd catches all output, including warnings and other debug information.
        This function removes all of those lines and returns an iterable of only the
        test output in the form of dictionaries."""
        out = []

        for line in output.readlines():
            line = line.strip()
            if line.startswith('{') and line.endswith('}'):
                entry = eval(line)  # Transform line into dict
                out.append(entry)

        return out


class CargoTestHostCheck(ICiBuildPlugin):
    def GetTestName(self, packagename: str, environment: object) -> tuple[str, str]:
        return (f'Confirm all cargo packages pass Cargo Test in {packagename}', f'{packagename}.CargoTest_Host')

    def RunBuildPlugin(self, packagename, Edk2pathObj, pkgconfig, environment, PLM, PLMHelper, tc, output_stream):
        shell_env = shell_environment.GetEnvironment()

        # Unless explicitly set, default to RUSTC_BOOTSTRAP=1
        if shell_env.get_shell_var("RUSTC_BOOTSTRAP") is None:
            rustc_bootstrap = environment.GetValue("RUSTC_BOOTSTRAP", "1")
            shell_env.set_shell_var("RUSTC_BOOTSTRAP", rustc_bootstrap)
            logging.info("Override: RUSTC_BOOTSTRAP={}".format(rustc_bootstrap))

        # Get all Rust Packages
        abs_pkg_path = Edk2pathObj.GetAbsolutePathOnThisSystemFromEdk2RelativePath(packagename)
        rust_pkg_list = [Path(x).parent for x in self.WalkDirectoryForExtension(['.toml'], abs_pkg_path)]

        # Test all applicable Rust packages
        failed = 0
        for rust_pkg in rust_pkg_list:

            package = RustPackage(rust_pkg)

            # Clean and build artifacts
            package.clean()

            # Skip any packages specified in CI config
            if package.name in pkgconfig.get("IgnoreRustPkg", []):
                logging.info(f'{rust_pkg.name} skipped per ci settings.')
                continue

            # Verify package has library
            ret = package.verify_lib()
            if ret == 1:
                logging.error(f'Failed to compile library associated with the package {package.name}.')
                tc.SetError(f'Failed to compile library associated with the package {package.name}.',
                            "LIBRARY_COMPILE_ERROR")
                return 1
            if ret == -1:
                logging.warning(f'{package.name} does not have an associated library.')
                logging.warning(f'  HINT: add {package.name} to IgnoreRustPkg section of ci.yaml file.')
                continue

            # Verify package has tests
            ret = package.verify_tests()
            if ret == 1:
                logging.error(f'Failed to compile tests associated with the package {package.name}.')
                tc.SetError(f'Failed to compile tests associated with the package {package.name}.',
                            "TEST_COMPILE_ERROR")
                return 1
            if ret == -1:
                logging.warning(f'{package.name} does not have any associated tests.')
                logging.warning(f'  HINT: {package.name} Should really have tests since it has a library.')
                continue

            # Run tests and parse for test results
            output = package.run_tests()
            for entry in output:
                if entry.get('type') != 'test':
                    continue

                if entry.get('event') == 'ok':
                    tc.LogStdOut(f'{package.name}::{entry.get("name")}....ok')

                if entry.get('event') == 'failed':
                    logging.error(f'{package.name}::{entry.get("name")}....failed')
                    logging.error(f'    {entry.get("stdout")}')
                    tc.LogStdError(f'{package.name}::{entry.get("name")}....failed')
                    tc.LogStdError(f'    {entry.get("stdout")}')
                    failed += 1

        if failed > 0:
            tc.SetFailed(f'CargoTest failed. Errors {failed}', "CHECK_FAILED")
        else:
            tc.SetSuccess()

        return failed
