# @file RustPackageHelper.py
# HelperFucntion used to share the RustPackage
# class to the rest of the build system.
##
# Copyright (c) Microsoft Corporation.
# SPDX-License-Identifier: BSD-2-Clause-Patent
##
from edk2toolext.environment.plugintypes.uefi_helper_plugin import IUefiHelperPlugin
from edk2toollib.utility_functions import RunCmd

from pathlib import Path
import io
import os
import time


class RustPackage:
    def __init__(self, path: Path):
        self.path = path
        self.name = path.name

    def clean(self):
        """Cleans any build artifacts from the directory.
        
        Ensures tests are freshly built and executed.
        """
        command = "cargo"
        parameters = "clean"

        RunCmd(command, parameters, workingdir=self.path)
        time.sleep(1) # Cargo clean returns immediately, wait for it to finish
    
    def test(self, ws):
        """Runs any tests located within the library.
        
        Returns:
            (dict): dict containing all test results
        """
        command = "cargo"
        params = f"make -e TEST_FLAGS=\"-- -Z unstable-options --format json\" test {self.name}"
        output = io.StringIO()

        ret = RunCmd(command, params, workingdir=ws, outstream=output)
        output.seek(0)
        return self.__clean_output(output)
       
    def __clean_output(self, output: io.StringIO):
        """Searches the output only for json lines to return."""
        out = []
        for line in output.readlines():
            line = line.strip()
            if line.startswith('{') and line.endswith('}'):
                entry = eval(line)  # Transform line into dict
                out.append(entry)

        return out


class RustPackageHelper(IUefiHelperPlugin):
    def RegisterHelpers(self, obj):
        fp = os.path.abspath(__file__)
        obj.Register("RustPackage", RustPackage, fp)