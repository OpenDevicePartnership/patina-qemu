# @file workspace_setup.py
#
# A console-based wizard to help the user set up their Patina QEMU workspace.
#
# This is an optional script for users new to the Patina QEMU workspace.
#
# First, it will help setup the pre-requisites to build UEFI code. Then, it will check if the user has built
# code before in the workspace. If not, it will run through steps to build the code. Then, it will guide the user
# in setting up the configuration needed to patch the UEFI code with a Patina EFI binary in the future.
#
# Note: To simplify user actions prior to running this module, it should only use built-in libraries. This generally
#       includes edk2-pytools. If after, pytools is installed, and it must be used, it can be, but the user should not
#       be expecteed to manually install it first.
#
# The sript currently makes some assumptions to simplify setup. For example, it does not pass the target to the
# build and always uses the default (usually DEBUG)
#
# Copyright (c) Microsoft Corporation. All rights reserved.
# SPDX-License-Identifier: BSD-2-Clause-Patent
##

import argparse
import glob
import json
import logging
import os
import platform
import re
import shutil
import subprocess
import sys
import textwrap
import time
from pathlib import Path, PurePath
from typing import List
from collections import namedtuple


PROGRAM_NAME = "Patina QEMU Workspace Setup Wizard"

_HANDLER_NAME = "wizard_handler"
_LOGGER = logging.getLogger()
_LOGGER_NAME = "wizard_logger"
_MAX_PYTHON_MINOR_VERSION_SUPPORTED = 13
_PLATFORM_PACKAGE_RELATIVE_DIR = "Platforms"
_WIZARD_VENV_DIR = "venv_wizard"


_PythonInstallation = namedtuple("PythonInsatllation", ["path", "is_venv"])


class TipFilter(logging.Filter):
    """A logging filter that suppresses 'Tip' messages."""

    def filter(self, record: logging.LogRecord) -> bool:
        """Tip filter method.

        Args:
            record (logging.LogRecord): A log record object that the filter is applied to.

        Returns:
            bool: True if the message is not a 'Tip' message. Otherwise, False.
        """
        return not bool(re.match(r"^\s*Tip:", record.getMessage()))


class MessageWrapFilter(logging.Filter):
    """A logging filter that wraps messages to a column."""

    def filter(self, record: logging.LogRecord) -> bool:
        """Message wrap filter method.

        Args:
            record (logging.LogRecord): A log record object that the filter is applied to.

        Returns:
            bool: Always returns True.
        """
        record.msg = _Utils.wrap_text(record.getMessage())
        return True


class _BuildDirState:
    """
    Tracks the state of the build directory and related log files.
    """

    def __init__(self):
        self.missing = False
        self.setup_log_present = False
        self.update_log_present = False
        self.build_log_present = False

    def any_missing_logs(self) -> bool:
        """
        Checks if any of the log files are missing.
        """
        return not (
            self.setup_log_present
            and self.update_log_present
            and self.build_log_present
        )


class _PatchConfig:
    """
    Manages patch configuration for the wizard.
    """

    def __init__(
        self, path: Path, compression_guid: str, ffs_guid: str, fv_layout: str = None
    ):
        self.path = path
        self.compression_guid = compression_guid
        self.ffs_guid = ffs_guid
        self.fv_layout = fv_layout

        self.input = None
        self.input_patch_paths = []
        self.reference_fw = None
        self.output = None
        self.patch_repo_path = None
        self.qemu_path = None

        if self.path.exists():
            with open(self.path, "r") as f:
                config = json.load(f)
                self.fv_layout = config.get("Paths", {}).get("FvLayout")
                self.reference_fw = config.get("Paths", {}).get("ReferenceFw")
                self.output = config.get("Paths", {}).get("Output")

    def save(self) -> None:
        """
        Saves the patch configuration to a JSON file.
        """
        config = {
            "DxeCore": {
                "CompressionGuid": self.compression_guid,
                "FfsGuid": self.ffs_guid,
            },
            "Paths": {
                "FvLayout": self.fv_layout,
                "ReferenceFw": self.reference_fw,
                "Output": self.output,
                **(
                    {"Input": self.input}
                    if hasattr(self, "input") and self.input is not None
                    else {}
                ),
                **(
                    {"FwPatchRepoPath": self.patch_repo_path}
                    if hasattr(self, "patch_repo_path")
                    and self.patch_repo_path is not None
                    else {}
                ),
                **(
                    {"QemuPath": self.qemu_path}
                    if hasattr(self, "qemu_path") and self.qemu_path is not None
                    else {}
                ),
            },
            "Patches": self.input_patch_paths,
        }

        self.path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.path, "w") as f:
            json.dump(config, f, indent=2)

        _LOGGER.debug(f"Patch config written to {self.path}\n")

    def load(self) -> None:
        """
        Loads the patch configuration from a JSON file.
        """
        if not self.path.exists():
            _LOGGER.error(f"Patch config file {self.path} does not exist.")
            return

        with open(self.path, "r") as f:
            config = json.load(f)
            self.fv_layout = config.get("Paths", {}).get("FvLayout")
            self.reference_fw = config.get("Paths", {}).get("ReferenceFw")
            self.output = config.get("Paths", {}).get("Output")
            self.input = config.get("Paths", {}).get("Input")
            self.input_patch_paths = config.get("Patches", [])
            self.qemu_path = config.get("Paths", {}).get("QemuPath")
            self.patch_repo_path = config.get("Paths", {}).get("FwPatchRepoPath")

    def __str__(self):
        """
        Returns a string representation of the patch configuration.
        """
        if len(self.input_patch_paths) > 0:
            extra_patch_paths = "\t[Patch Paths]"
            for path in self.input_patch_paths:
                extra_patch_paths += f"\n\t{path}"
            extra_patch_paths += "\n"
        else:
            extra_patch_paths = " "
        return (
            f"Patch Configuration:\n\n"
            f"  - QEMU ROM Image to Patch:\n      {self.output}\n"
            + (
                f"\n    - Note: {self.output} is a copy of the original ROM file\n\n"
                if self.output and self.output.endswith(".ref.fd")
                else "\n"
            )
            + f"  - Patina EFI Binary Repo:\n      {self.input}\n"
            f"{extra_patch_paths}\n"
            f"  - QEMU Path:\n      {self.qemu_path}\n\n"
            f"  - Patch Repo Path:\n      {self.patch_repo_path}\n\n"
            f"  Internal Patch Details:\n"
            f"    - Compression GUID: {self.compression_guid}\n"
            f"    - FFS GUID: {self.ffs_guid}\n"
            f"    - FV Layout: {self.fv_layout}\n"
        )


class _WizardSettings:
    """
    Stores settings and constants used by the Patina QEMU patching wizard.
    """

    _Q35_DXE_CORE_COMPRESSION_GUID = "ee4e5898-3914-4259-9d6e-dc7bd79403cf"
    _Q35_DXE_CORE_FFS_GUID = "fb5947af-7cb5-413e-8c1a-38167fcbe3ea"
    _SBSA_DXE_CORE_COMPRESSION_GUID = "ee4e5898-3914-4259-9d6e-dc7bd79403cf"
    _SBSA_DXE_CORE_FFS_GUID = "7bb6c4a8-fecd-4f0d-9f5a-2e03add35b96"

    def __init__(self, workspace_dir: Path, package: str):
        self.default_prompt_choices = False
        self.package = package
        self.package_name = "Qemu" + self.package.title() + "Pkg"
        self.patch_config = None
        self.py = _PythonInstallation(path=None, is_venv=False)
        self.show_build_output = False
        self.workspace_dir = workspace_dir

        self.config_dir = self.workspace_dir / "PatinaPatching" / "Configs"
        self.build_dir = self.workspace_dir / "Build"
        self.build_dir_state = _Utils.get_build_dir_state(self.build_dir)
        self.package_path = (
            self.workspace_dir / _PLATFORM_PACKAGE_RELATIVE_DIR / self.package_name
        )

        if self.package.upper() == "Q35":
            self.config_path = self.config_dir / "Q35WizardConfig.json"
            self.pre_compiled_rom = (
                self.workspace_dir
                / "PatinaPatching"
                / "Reference"
                / "Binaries"
                / "Q35"
                / "QEMUQ35_CODE.fd"
            )
            self.patch_config = _PatchConfig(
                self.config_path,
                self._Q35_DXE_CORE_COMPRESSION_GUID,
                self._Q35_DXE_CORE_FFS_GUID,
                "./Reference/Layouts/qemu_q35.inf",  # Use the built-in layout
            )
        elif self.package.upper() == "SBSA":
            self.config_path = self.config_dir / "SbsaWizardConfig.json"
            self.pre_compiled_rom = (
                self.workspace_dir
                / "PatinaPatching"
                / "Reference"
                / "Binaries"
                / "SBSA"
                / "QEMUSBSA_CODE.fd"
            )
            self.patch_config = _PatchConfig(
                self.config_path,
                self._SBSA_DXE_CORE_COMPRESSION_GUID,
                self._SBSA_DXE_CORE_FFS_GUID,
                "./Reference/Layouts/qemu_sbsa.inf",  # Use the built-in layout
            )
        else:
            raise ValueError(f"Unknown package: {package}")


class _Utils:
    """
    Static utility functions for use in the wizard.
    """

    @staticmethod
    def print_divider(logger) -> None:
        """
        Prints a divider line of '=' characters across the terminal width using the provided logger.

        Attempts to determine the terminal width dynamically; defaults to 80 columns if unable to retrieve the size.

        The divider is logged as an info message with a preceding newline.

        Args:
            logger: A logging.Logger-like object with an 'info' method for outputting the divider.
        """
        try:
            columns = shutil.get_terminal_size().columns
        except Exception:
            columns = 80
        logger.info("\n" + "=" * columns)

    @staticmethod
    def wrap_text(text: str, columns: int = 0) -> str:
        """
        Wraps the input text to a specified number of columns, preserving existing newlines.

        This function takes a string and wraps each paragraph (separated by newlines) so that
        no line exceeds the specified column width. If columns is set to 0, it attempts to
        detect the terminal width; if detection fails, it defaults to 80 columns.

        Args:
            text (str): The input text to wrap.
            columns (int, optional): The maximum number of columns per line. Defaults to 0.

        Returns:
            str: The wrapped text with preserved paragraph breaks.
        """

        if columns == 0:
            try:
                columns = shutil.get_terminal_size().columns
            except Exception:
                columns = 80
        # Preserve existing newlines, only wrap each paragraph
        return "\n".join(
            "\n".join(textwrap.wrap(line, width=columns)) if line.strip() else ""
            for line in text.splitlines()
        )

    @staticmethod
    def find_python_versions(workspace_dir: Path) -> List[tuple[str, str]]:
        """
        Searches for installed Python interpreters on the system and within the given workspace directory.

        Args:
            workspace_dir (Path): The root directory of the workspace to search for Python virtual environments.

        Returns:
            List[tuple[str, str]]: A list of tuples, each containing the absolute path to a Python executable
            and its version string (e.g., [('/usr/bin/python3.xx', 'Python 3.xx.y'), ...]).
        """

        _LOGGER.info("\nSearching for installed Python versions...")

        python_executables = set()

        # Check common executable names in PATH
        for name in ["py", "python", "python3"]:
            path = shutil.which(name)
            if path:
                python_executables.add(path)

        # Check for pythonX.Y executables in PATH
        for minor in range(0, _MAX_PYTHON_MINOR_VERSION_SUPPORTED + 1):
            bin_name = f"python3.{minor}"
            path = shutil.which(bin_name)
            if path:
                python_executables.add(path)

        # Check using py launcher on Windows
        if platform.system() == "Windows":
            try:
                output = subprocess.check_output(["py", "-0p"], universal_newlines=True)
                for line in output.splitlines():
                    if line.strip():
                        python_executables.add(line.strip().split(maxsplit=1)[-1])
            except Exception:
                pass

        # Check common installation directories
        possible_dirs = []
        if platform.system() == "Windows":
            possible_dirs += [
                r"C:\Python*",
                r"C:\Users\%USERNAME%\AppData\Local\Programs\Python\Python*",
                r"C:\Users\%USERNAME%\AppData\Local\Programs\Python\Launcher*",
                r"C:\Program Files\Python*",
                r"C:\Program Files (x86)\Python*",
            ]
        else:
            possible_dirs += [
                "/usr/bin/python*",
                "/usr/local/bin/python*",
                "/opt/python*",
                str(Path.home() / ".pyenv" / "versions" / "*/bin/python*"),
            ]

        # Include any Python virtual environment in workspace_dir by globbing
        for venv_path in workspace_dir.glob("**/"):
            # Look for typical venv structure
            if platform.system() == "Windows":
                exe = venv_path / "Scripts" / "python.exe"
            else:
                exe = venv_path / "bin" / "python"
            if exe.exists() and os.access(exe, os.X_OK):
                python_executables.add(str(exe))

        for pattern in possible_dirs:
            for exe in glob.glob(os.path.expandvars(pattern)):
                if os.path.isfile(exe) and os.access(exe, os.X_OK):
                    python_executables.add(exe)

        found_versions = []
        for exe in sorted(python_executables):
            try:
                version = subprocess.check_output(
                    [exe, "--version"],
                    stderr=subprocess.STDOUT,
                    universal_newlines=True,
                ).strip()
                found_versions.append((exe, version))
            except Exception:
                continue

        unique_versions = {}
        for exe, version in found_versions:
            exe_norm = str(Path(exe).resolve()).lower()
            if exe_norm not in unique_versions:
                unique_versions[exe_norm] = (exe, version)

        return list(unique_versions.values())

    @staticmethod
    def init_logging(
        hide_tip_messages: bool, wrap_text_to_terminal: bool
    ) -> logging.Logger:
        """
        Initializes and configures a logger instance.

        Args:
            hide_tip_messages (bool): If True, filters out tip messages ("Tip:") from the log output.
            wrap_text_to_terminal (bool): If True, wraps log messages to fit the terminal width.

        Returns:
            logging.Logger: The configured logger instance.
        """
        wiz_logger = logging.getLogger(_LOGGER_NAME)
        wiz_logger.setLevel(logging.DEBUG)
        wiz_logger.propagate = False

        if wiz_logger.hasHandlers():
            wiz_logger.handlers.clear()

        wiz_logger_handler = logging.StreamHandler(sys.stdout)
        wiz_logger_handler.set_name(_HANDLER_NAME)
        wiz_logger_handler.setLevel(logging.INFO)
        if hide_tip_messages:
            wiz_logger_handler.addFilter(TipFilter())
        if wrap_text_to_terminal:
            wiz_logger_handler.addFilter(MessageWrapFilter())
        wiz_logger.addHandler(wiz_logger_handler)

        return wiz_logger

    @staticmethod
    def deinit_logging() -> None:
        """
        Deinitializes logging by removing all handlers from the logger.

        Retrieves the logger specified by _LOGGER_NAME and removes all attached handlers, effectively disabling
        further logging output for that logger.
        """

        wiz_logger = logging.getLogger(_LOGGER_NAME)
        for handler in wiz_logger.handlers[:]:
            wiz_logger.removeHandler(handler)

    @staticmethod
    def run_cmd(
        cmd: list[str], context_msg: str = None, quiet: bool = False, cwd: Path = None
    ) -> None:
        """
        Runs a command in a subprocess.

        This function exits with the command return code if the command fails.

        Args:
            cmd (list[str]): The command to run as a list of strings.
            context_msg (str, optional): A message to display before running the command.
            quiet (bool, optional): If True, suppresses output to stdout and stderr.
            cwd (Path, optional): The working directory to run the command in.
        """
        if context_msg:
            _LOGGER.info(context_msg)

        try:
            if quiet:
                subprocess.run(
                    cmd,
                    check=True,
                    cwd=cwd,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
            else:
                subprocess.run(cmd, check=True, cwd=cwd)
        except subprocess.CalledProcessError as e:
            _LOGGER.error(f"Command failed with error #{e.returncode}.")
            exit(e.returncode)

    @staticmethod
    def check_submodule_state(workspace_dir: Path) -> tuple[bool, bool]:
        """
        Checks if submodules have been initialized and are up-to-date.

        Args:
            workspace_dir (Path): The path to the workspace directory.

        Returns:
            tuple[bool, bool]: A tuple with:
                - A boolean indicating if some submodules are missing or outdated.
                - A boolean indicating if all submodules are missing.
        """
        _LOGGER.info("\nChecking submodule state...")

        gitmodules_path = workspace_dir / ".gitmodules"
        if not gitmodules_path.exists():
            _LOGGER.info("No .gitmodules file found; skipping submodule check.")
            return False, False

        with open(gitmodules_path, "r") as f:
            submodules = [
                line.split("=")[1].strip()
                for line in f
                if line.strip().startswith("path =")
            ]

        missing = []
        outdated = []
        for submodule in submodules:
            submodule_path = workspace_dir / submodule
            if not submodule_path.exists() or not any(submodule_path.iterdir()):
                missing.append(submodule)
            else:
                try:
                    output = subprocess.check_output(
                        ["git", "submodule", "status", submodule],
                        cwd=workspace_dir,
                        universal_newlines=True,
                    ).strip()
                    if output.startswith("-"):
                        outdated.append(submodule)
                except subprocess.CalledProcessError as e:
                    _LOGGER.error(
                        f"Failed to check the status of the submodule {submodule} (exit code {e.returncode})."
                    )
                    exit(e.returncode)

        if missing:
            _LOGGER.warning(
                "The following submodules are not initialized or empty:\n"
                + "\n".join(f"- {submodule}" for submodule in missing)
            )
            return (True, True) if len(missing) == len(submodules) else (True, False)

        if outdated:
            _LOGGER.warning(
                "The following submodules are not up-to-date:\n"
                + "\n".join(f"- {submodule}" for submodule in outdated)
            )
            return True, False

        return False, False

    @staticmethod
    def setup_py_virtual_env(workspace_dir: Path) -> _PythonInstallation:
        """
        Sets up a virtual environment for the script.

        Args:
            workspace_dir (Path): The path to the workspace directory.

        Returns:
            virtual_env_path (Path): The path to the created virtual environment.
                                     None if a virtual environment is not selected.
        """
        wizard_venv_dir = workspace_dir / _WIZARD_VENV_DIR

        py_versions = _Utils.find_python_versions(workspace_dir)
        if not py_versions:
            _LOGGER.error("\nA Python installation was not found or an error occurred.")
            exit(1)

        wizard_venv_bin = None
        _LOGGER.info("\nFound Python versions:")
        for i, (bin, version) in enumerate(py_versions, start=1):
            _LOGGER.info(f" {i}. {version}: {bin}")

            bin_path = Path(bin).resolve()
            if wizard_venv_dir in bin_path.parents:
                wizard_venv_bin = (i, bin, version)

        if wizard_venv_bin:
            _LOGGER.info(
                f"\n* Option {wizard_venv_bin[0]} was previously created by this wizard. It is recommended for use."
            )
            _LOGGER.info(f"  ({wizard_venv_bin[2]}: {wizard_venv_bin[1]})")

        while True:
            choice = input(f"\nSelect an option [1-{len(py_versions)}]: ").strip()
            try:
                choice_int = int(choice)
                if 1 <= choice_int <= len(py_versions):
                    break
                else:
                    _LOGGER.error(
                        "Invalid choice. Please enter a number between 1 and {}.".format(
                            len(py_versions)
                        )
                    )
            except ValueError:
                _LOGGER.error(
                    "Invalid choice. Please enter a number between 1 and {}.".format(
                        len(py_versions)
                    )
                )

        selected_install = py_versions[choice_int - 1][0]
        _LOGGER.debug(f"\nSelected Python executable: {selected_install}")

        # Check if the selected Python executable is already in a virtual environment
        try:
            output = subprocess.check_output(
                [
                    selected_install,
                    "-c",
                    "import sys; print(hasattr(sys, 'real_prefix') or (hasattr(sys, 'base_prefix') and sys.base_prefix != sys.prefix))",
                ],
                universal_newlines=True,
            ).strip()
            in_venv = output == "True"
        except Exception:
            in_venv = False

        if in_venv:
            _LOGGER.info(
                "\nGreat! The Python installation you selected is already in a virtual environment. Moving on..."
            )
            return _PythonInstallation(selected_install, True)
        else:
            if not _Utils.get_yes_no_response(
                "Would you like to create a Python virtual environment using"
                f"\n {selected_install}? (y/n): "
            ):
                _LOGGER.info("Skipping virtual environment creation.")
                return _PythonInstallation(selected_install, False)

            _LOGGER.info(
                f"\nCreating a Python virtual environment at {wizard_venv_dir}..."
            )

            cmd = [selected_install, "-m", "venv", str(wizard_venv_dir)]
            _Utils.run_cmd(
                cmd,
                context_msg=f"\nTip: The command used to create this virtual environment is:\n  {' '.join(cmd)}\n\n",
            )

            _LOGGER.info(
                "\nTo activate the virtual environment, after this script exits, run:"
            )
            if platform.system() == "Windows":
                _LOGGER.info(f"  {wizard_venv_dir}\\Scripts\\activate")
            else:
                _LOGGER.info(f"  source {wizard_venv_dir}/bin/activate")

            _LOGGER.info("\nThen run this script again to continue the setup process.")
            _LOGGER.info("Exiting the script now.")
            exit(0)

    @staticmethod
    def install_pip_modules(
        workspace_dir: Path, py_install: _PythonInstallation
    ) -> _PythonInstallation:
        """
        Installs required pip modules listed in 'pip-requirements.txt' for the given workspace.

        Checks if a Python installation is selected and whether a virtual environment is active.

        If a virtual environment is not active, it prompts the user to create one. It then attempts to install
        the required pip modules specified in 'pip-requirements.txt' located in the workspace directory.

        Args:
            workspace_dir (Path): The path to the workspace directory containing 'pip-requirements.txt'.
            py_install (_PythonInstallation): The current Python installation context.

        Returns:
            _PythonInstallation: The (possibly updated) Python installation context after setup.

        Raises:
            SystemExit: If 'pip-requirements.txt' is not found or pip installation fails.
        """
        if py_install.path is None:
            _LOGGER.error(
                "\nA Python installation is not selected. Please run the script again to select a Python installation."
            )

        if not py_install.is_venv:
            if _Utils.get_yes_no_response(
                "\nA virtual environment is not active. It is highly recommended to "
                "proceeed in a Python virtual environment. Would you like to set one up? "
                "(y/n): "
            ):
                py_install = _Utils.setup_py_virtual_env(workspace_dir)

        _LOGGER.info("\nAttempting to install required pip modules...")

        # Check if pip modules in pip-requirements.txt match installed modules
        requirements_path = workspace_dir / "pip-requirements.txt"
        if not requirements_path.exists():
            _LOGGER.error(f"pip-requirements.txt not found at {requirements_path}")
            exit(1)

        cmd = [str("pip"), "install", "-r", str(requirements_path), "--upgrade"]
        _Utils.run_cmd(
            cmd,
            context_msg=f"\nTip: The command used to install pip modules is:\n  {' '.join(cmd)}\n\n",
        )
        _LOGGER.info("\nPip modules installed successfully!")

        return py_install

    @staticmethod
    def update_submodules() -> None:
        """
        Updates all git submodules for the current repository.
        """

        _LOGGER.info("\nUpdating submodules...")

        cmd = ["git", "submodule", "update", "--init", "--recursive"]
        _Utils.run_cmd(
            cmd,
            context_msg=f"\nTip: The command used to update submodules is:\n  {' '.join(cmd)}\n\n",
        )

    @staticmethod
    def get_build_dir_state(build_dir: Path) -> _BuildDirState:
        """
        Gets the state of the build directory.

        Args:
            workspace_dir (Path): The path to the workspace directory.

        Returns:
            _BuildDirState: An instance of _BuildDirState with the state of the build directory.
        """
        _Utils.print_divider(_LOGGER)
        _LOGGER.info("\nChecking your workspace build state...")

        build_dir_state = _BuildDirState()

        if not build_dir.exists():
            build_dir_state.missing = True
            return build_dir_state

        # Check log file state
        log_files = [
            "BUILDLOG_*.txt",
            "SETUPLOG.txt",
            "UPDATE_LOG.txt",
        ]
        for log_file_pattern in log_files:
            # Use glob to allow wildcards in file names
            matches = list(build_dir.glob(log_file_pattern))

            if matches:
                # Remove wildcard and extension to get the attribute name
                attr_name = (
                    log_file_pattern.split("*")[0].split(".")[0].rstrip("_").lower()
                )
                # Map to correct attribute
                if "buildlog" in attr_name:
                    build_dir_state.build_log_present = True
                elif "setuplog" in attr_name:
                    build_dir_state.setup_log_present = True
                elif "update_log" in attr_name:
                    build_dir_state.update_log_present = True

        return build_dir_state

    @staticmethod
    def find_code_fd(build_dir: Path, package: str) -> List[Path]:
        """
        Finds the QEMU UEFI code file in the build directory.

        Args:
            build_dir (Path): The path to the build directory.
            package (str): The package name (e.g., "QemuQ35Pkg" or "QemuSbsaPkg").

        Returns:
            List[Path]: A list of paths to code files found for the specified package.
        """
        Q35_FD_FILE_NAME = "QEMUQ35_CODE.fd"
        SBSA_FD_FILE_NAME = "QEMU_EFI.fd"

        _LOGGER.info(f"\nSearching for already built {package} UEFI code files...")

        # Check if the build directory exists
        if not build_dir.exists():
            _LOGGER.error(f"Build directory {build_dir} does not exist.")
            return None

        package_build_dir = build_dir / package
        code_fd_pattern = (
            Q35_FD_FILE_NAME if package == "QemuQ35Pkg" else SBSA_FD_FILE_NAME
        )
        return list(package_build_dir.glob(f"**/{code_fd_pattern}"))

    @staticmethod
    def get_yes_no_response(prompt: str) -> bool:
        """
        Prompts the user for a yes/no response.

        Args:
            prompt (str): The prompt message to display.

        Returns:
            bool: True if the user responds with 'y' or 'Y', False otherwise.
        """
        _LOGGER.info(prompt)

        while True:
            response = input().strip().lower()
            if response in ("y", "yes"):
                return True
            elif response in ("n", "no"):
                return False
            else:
                _LOGGER.error("Invalid input. Please enter 'y' or 'n'.")


class _Wizard:
    """
    A wizard for setting up a Patina UEFI workspace.
    """

    def __init__(self, workspace_dir: Path, package: str):
        self._settings = self._init_settings(workspace_dir, package)

    def _init_settings(self, workspace_dir: Path, package: str) -> _WizardSettings:
        """
        Initializes and returns wizard settings based on the specified package.

        Args:
            workspace_dir (Path): The directory where the workspace is located.
            package (str): The name of the package to initialize settings for. Supported values are "Q35" and "SBSA".

        Returns:
            _WizardSettings: An instance of _WizardSettings configured for the specified package.

        Raises:
            ValueError: If the provided package is not recognized.
        """

        if package.upper() == "Q35":
            return _WizardSettings(workspace_dir, "Q35")
        elif package.upper() == "SBSA":
            return _WizardSettings(workspace_dir, "SBSA")
        else:
            raise ValueError(f"Unknown package: {package}")

    def _run_stuart_setup(self) -> None:
        """
        Runs the Stuart Setup script to initialize the workspace.

        Notes:
        - This function does not check if stuart_setup needs to be run, it just runs it.
        - This function assume the edk2-pytool-extensions pip module is already installed.
        """
        _LOGGER.info("\nRunning Stuart Setup... To initialize your workspace.\n")
        _LOGGER.info(f"\nRunning stuart_setup for {self._settings.package_name}...\n")

        cmd = [
            "stuart_setup",
            "-c",
            f"{str(self._settings.package_path / 'PlatformBuild.py')}",
        ]
        _Utils.run_cmd(
            cmd,
            context_msg=f"\nTip: The command used for stuart_setup is:\n  {' '.join(cmd)}\n\n",
        )

    def _run_stuart_build(self) -> None:
        """
        Runs the Stuart Build command to build the workspace.
        """
        _LOGGER.info("\nRunning Stuart Build...\n\n")
        _LOGGER.info(
            f'Tip: If an error occurs, check the "BUILDLOG_" files in {self._settings.build_dir}.\n'
        )

        command = [
            "stuart_build",
            "-c",
            f"{str(self._settings.package_path / 'PlatformBuild.py')}",
            "--FlashRom",
        ]
        if self._settings.package.upper() == "SBSA":
            if platform.system() == "Windows":
                command.append("TOOL_CHAIN_TAG=CLANGPDB")
            else:
                command.append("TOOL_CHAIN_TAG=GCC5")

        if self._settings.show_build_output:
            _Utils.run_cmd(
                command,
                context_msg=f"\nTip: The command used for this build is:\n  {' '.join(command)}\n\n",
            )
        else:
            _LOGGER.info(
                '\nTip: Build output is hidden by default. Use "--show-build-output" for it to be '
                "displayed in the console.\n"
            )
            _Utils.run_cmd(
                command,
                context_msg=f"\nTip: The command used for this build is:\n  {' '.join(command)}\n\n",
                quiet=True,
            )

    def _find_code_fd_file(self) -> Path:
        """
        Finds the QEMU UEFI code file in the build directory.

        Returns:
            Path: The path to the code file found for the specified package. None if not found.
        """
        selected_code_fd_file = None

        pre_existing_code_fd_files = _Utils.find_code_fd(
            self._settings.build_dir, self._settings.package_name
        )
        if pre_existing_code_fd_files:
            _LOGGER.info(
                "\nIt looks like you have already built the QEMU UEFI code before in this workspace."
            )

            pre_existing_code_fd_files_count = len(pre_existing_code_fd_files)
            if pre_existing_code_fd_files_count > 1:
                _LOGGER.info(
                    f"\n{pre_existing_code_fd_files_count} QEMU UEFI code files were found in your build "
                    "directory. Please select one:"
                )
                for i, code_fd_file in enumerate(pre_existing_code_fd_files, start=1):
                    _LOGGER.info(
                        f"  [{i}] {code_fd_file.relative_to(self._settings.build_dir)}"
                    )
                _LOGGER.info(
                    f"\nSelect an option [1-{pre_existing_code_fd_files_count}]: "
                )
                choice = input().strip()
                try:
                    choice_int = int(choice)
                    if 1 <= choice_int <= len(pre_existing_code_fd_files):
                        selected_code_fd_file = pre_existing_code_fd_files[
                            choice_int - 1
                        ]
                        _LOGGER.info(
                            f"\nUsing {selected_code_fd_file} as the UEFI image to patch."
                        )
                    else:
                        raise ValueError
                except ValueError:
                    _LOGGER.error("Invalid choice. Exiting.")
                    exit(1)
            else:
                selected_code_fd_file = pre_existing_code_fd_files[0]
                _LOGGER.info(
                    f"  - Found {selected_code_fd_file} as the UEFI image to patch."
                )

        return selected_code_fd_file

    def qemu_workspace_setup(self) -> None:
        """
        Sets up the QEMU UEFI workspace environment interactively.

        This method performs the following steps:
        1. Initializes and installs the Python virtual environment and required pip modules in the workspace directory.
        2. Checks the state of git submodules and prompts the user to update them if necessary.
        3. Checks if the "Stuart Update" process has been run before, and prompts the user to run it if needed.
        4. Runs "stuart_update" to update external dependencies if the user agrees, or provides instructions to run
           it later.
        """
        _LOGGER.info("\nUEFI Workspace Setup Wizard\n")

        self._settings.py = _Utils.setup_py_virtual_env(self._settings.workspace_dir)
        self._settings.py = _Utils.install_pip_modules(
            self._settings.workspace_dir, self._settings.py
        )

        _Utils.print_divider(_LOGGER)

        need_submodule_update, need_stuart_setup = _Utils.check_submodule_state(
            self._settings.workspace_dir
        )
        if need_stuart_setup:
            self._run_stuart_setup()
        elif need_submodule_update:
            if _Utils.get_yes_no_response(
                "\nSome submodules are missing or out-of-date. Would you like to update them now? (y/n): "
            ):
                _Utils.update_submodules()

        else:
            _LOGGER.info(
                "\nAll submodules are up-to-date. No action needed for submodules!"
            )

        _Utils.print_divider(_LOGGER)

        # This does not accurately indicate iif "stuart_update" needs to be run but it is a simple, good enough check
        if not self._settings.build_dir_state.update_log_present:
            stuart_update_prompt = (
                '\nIt looks like you have not run "Stuart Update" yet.\n\nThis is needed to update external '
                "dependencies in your workspace that are used during the build process. It is required to run "
                "at least once in a new workspace.\n"
                "\nWould you like to run Stuart Update now? (y/n): "
            )
        else:
            stuart_update_prompt = (
                "\nIt looks like you have already run Stuart Update before.\n\nWould you like to run it again to "
                "check for any updates to external dependencies? (y/n): "
            )

        cmd = [
            "stuart_update",
            "-c",
            f"{str(self._settings.package_path / 'PlatformBuild.py')}",
        ]
        if _Utils.get_yes_no_response(stuart_update_prompt):
            _LOGGER.info("\nRunning Stuart Update...\n")
            _Utils.run_cmd(
                cmd,
                context_msg=f"\nTip: The command used for stuart_update is:\n  {' '.join(cmd)}.\n\n"
                "You should run stuart_update periodically to check for updates to external "
                "dependencies.\n\n",
            )
        else:
            _LOGGER.info(
                f"\nYou can run Stuart Update in the future by running the command:\n  {' '.join(cmd)}\n\n"
            )

    def patch_config_setup(self) -> None:
        """
        Sets up configuration for patching.
        """
        Q35_QEMU_EXEC = "qemu-system-x86_64"
        SBSA_QEMU_EXEC = "qemu-system-aarch64"

        _LOGGER.info("\nSetting up patch configuration...")

        _LOGGER.info(
            "\nIn order to patch the QEMU UEFI image, we need make sure some dependencies are present on "
            "your system.\n"
        )

        _Utils.print_divider(_LOGGER)

        _LOGGER.info(
            "\nThe script used for patching is in this repository:\n\n"
            "https://github.com/OpenDevicePartnership/patina-fw-patcher\n"
        )

        _LOGGER.info(
            "\nPlease clone this repo locally if you have not already and give the path to the local "
            "repository directory:\n"
        )
        while True:
            patina_fw_patcher_local_repo_path = input().strip()
            if os.path.isdir(patina_fw_patcher_local_repo_path):
                break
            _LOGGER.error(
                "The provided path does not exist or is not a directory. Please enter a valid directory path:"
            )

        qemu_paths = []

        # Check if the ext dep is available on Windows
        if platform.system() == "Windows":
            qemu_ext_dep_path = Path(
                self._settings.workspace_dir
                / "QemuPkg"
                / "Binaries"
                / "qemu-win_extdep"
            )
            if qemu_ext_dep_path.exists():
                if self._settings.package.upper() == "Q35":
                    if (qemu_ext_dep_path / Q35_QEMU_EXEC).exists():
                        qemu_paths.append(qemu_ext_dep_path / Q35_QEMU_EXEC)
                elif self._settings.package.upper() == "SBSA":
                    if (qemu_ext_dep_path / SBSA_QEMU_EXEC).exists():
                        qemu_paths.append(qemu_ext_dep_path / SBSA_QEMU_EXEC)

        # Check for qemu on the system path
        qemu_sys_path = None
        if self._settings.package.upper() == "Q35":
            qemu_sys_path = shutil.which("qemu-system-x86_64")
        elif self._settings.package.upper() == "SBSA":
            qemu_sys_path = shutil.which("qemu-system-aarch64")

        if qemu_sys_path:
            qemu_paths.append(qemu_sys_path)

        qemu_path = None
        if len(qemu_paths) == 0:
            _LOGGER.info(
                "\nQEMU needs to be installed on your system to run the patched image.\n"
            )
            _LOGGER.info("\nThis script was not able to find QEMU on your system.\n")
            _LOGGER.info(
                "Please install QEMU and/or set the path to the QEMU bin directory here:\n"
            )

            while True:
                qemu_path = input().strip()
                if os.path.isdir(qemu_path):
                    break
                _LOGGER.error(
                    "The provided path does not exist or is not a directory. Please enter a valid directory path:"
                )
        elif len(qemu_paths) > 1:
            _LOGGER.info("\nMultiple QEMU executables were found. Please select one:")
            for i, path in enumerate(qemu_paths, start=1):
                _LOGGER.info(f"  [{i}] {path}")
            _LOGGER.info(f"\nSelect an option [1-{len(qemu_paths)}]: ")
            choice = input().strip()
            try:
                choice_int = int(choice)
                if 1 <= choice_int <= len(qemu_paths):
                    qemu_path = qemu_paths[choice_int - 1]
                else:
                    raise ValueError
            except ValueError:
                _LOGGER.error("Invalid choice. Exiting.")
                exit(1)
        else:
            qemu_path = qemu_paths[0]

        if not qemu_path:
            _LOGGER.error("No QEMU executable found. Exiting.")
            exit(1)

        _Utils.print_divider(_LOGGER)

        _LOGGER.info(
            "\nA binary from a Patina repo needs to be specified to patch. If you are unsure of which binary "
            'to use, you likely want to use the "Patina DXE Core" binary. The Patina DXE Core is built '
            "from this repo:\n\n"
            "https://github.com/OpenDevicePartnership/patina-dxe-core-qemu\n"
        )

        _LOGGER.info(
            "\nPlease clone this repo locally if you have not already and give the path to the local "
            "repository directory:\n"
        )
        while True:
            patina_binary_local_repo_path = input().strip()
            if os.path.isdir(patina_binary_local_repo_path):
                break
            _LOGGER.error(
                "The provided path does not exist or is not a directory. Please enter a valid directory path:"
            )

        _Utils.print_divider(_LOGGER)

        _LOGGER.info(
            "\nThis tool supports patching dependencies used to build the Patina DXE Core binary (you provided above)"
            " with other local versions of the crates.\n\nIf you would like to automatically patch any dependencies,"
            " please provide the path(s) to them below. Providing an empty path will skip this step.\n"
        )

        patch_repo_paths = []
        while True:
            patch_repo_path = input().strip()
            if patch_repo_path == "":
                break
            if os.path.isdir(patch_repo_path):
                patch_repo_paths.append(patch_repo_path)
            else:
                _LOGGER.error(
                    "The provided path does not exist or is not a directory. Please enter a valid directory path:"
                )

        selected_code_fd_file = self._find_code_fd_file()
        if not selected_code_fd_file:
            selected_code_fd_file = self._settings.pre_compiled_rom
            _LOGGER.info(
                "\nIt looks like you have built UEFI firmware in this workspace but an existing UEFI image to "
                "patch was not found."
            )

            _LOGGER.info(
                "\nIf you would like to compile a new UEFI image, run the script again and select "
                "option 1."
            )

            _LOGGER.info(
                "\nFor now, we will attempt to use the pre-compiled QEMU UEFI image that is included "
                "in the repo."
            )
            time.sleep(3)

        self._settings.patch_config.reference_fw = str(selected_code_fd_file)
        self._settings.patch_config.output = str(
            selected_code_fd_file.with_suffix(".ref.fd")
        )
        self._settings.patch_config.input = patina_binary_local_repo_path
        self._settings.patch_config.input_patch_paths = patch_repo_paths
        self._settings.patch_config.patch_repo_path = patina_fw_patcher_local_repo_path
        self._settings.patch_config.qemu_path = qemu_path

    def run_patching_script(self) -> None:
        """
        Runs the patching script using the specified configuration.
        """
        _Utils.print_divider(_LOGGER)
        _LOGGER.info("\nRunning the patching script...\n")

        patch_config = self.get_patch_config()
        output_path = Path(patch_config.output)

        if output_path.name.endswith(".ref.fd"):
            output_path = output_path.with_name(
                output_path.name.replace(".ref.fd", ".fd")
            )

        command = [
            "python",
            str(Path(self._settings.workspace_dir) / "build_and_run_rust_binary.py"),
            "--platform",
            self.get_package()[0],
            "--patina-dxe-core-repo",
            str(Path(patch_config.input)),
            "--pre-compiled-rom",
            str(output_path),
            "--fw-patch-repo",
            str(patch_config.patch_repo_path),
            "--qemu-path",
            patch_config.qemu_path,
            "--config-file",
            str(patch_config.path),
        ]

        for patch in patch_config.input_patch_paths:
            command.extend(["--crate-patch", str(patch)])

        _Utils.run_cmd(
            command,
            context_msg=f"\nTip: The command used for patching is:\n {' '.join(command)}\n\n",
            cwd=self._settings.workspace_dir,
        )

    def is_needed(self) -> bool:
        """
        Returns whether the wizard needs to run.

        Returns:
            bool: True if the wizard needs to run, False otherwise.
        """
        return not self._settings.config_path.exists()

    def get_package(self) -> tuple[str, str]:
        """
        Return the platform name and package name.

        Returns:
            tuple[str, str]: A tuple containing the platform name and package name.
        """
        return self._settings.package, self._settings.package_name

    def get_patch_config(self) -> _PatchConfig:
        """
        Returns the patch config.

        Returns:
            _PatchConfig: The patch config object.
        """
        return self._settings.patch_config

    def use_default_prompt_choices(self) -> None:
        """
        Hides extraneuous prompts by automatically using the default option.
        """
        self._settings.default_prompt_choices = True

    def show_build_output(self) -> None:
        """
        Displays output to the console during "stuart_build".
        """
        self._settings.show_build_output = True

    def start(self) -> None:
        """
        Starts the wizard.
        """
        _LOGGER.info(
            "\nIt looks like you have not setup your workspace for patching Patina binaries yet."
        )
        _LOGGER.debug(
            f"(an existing {self._settings.package} workspace config file was not found in {self._settings.config_path})...\n"
        )

        _LOGGER.info(
            "\nThis script will help you set up your workspace to build UEFI code and patch Patina binaries into "
            "QEMU UEFI images. It saves your configuration so it is easier in the future.\n"
        )

        _Utils.print_divider(_LOGGER)

        if (
            self._settings.build_dir_state.missing
            or self._settings.build_dir_state.any_missing_logs()
        ):
            _LOGGER.info(
                "\nYou have not built the QEMU UEFI code yet. Since this script patches Patina binaries into a QEMU "
                "UEFI image, you should build the QEMU UEFI code first.\n"
            )

            # Do not provide a built-in ROM for SBSA right now due to its size
            if self._settings.package.upper() != "SBSA":
                _LOGGER.info(
                    "\nHowever, we can continue the patching process using a pre-built QEMU UEFI image. This "
                    "will be built from code older than that in your workspace right now, but it will still allow "
                    "you to patch with Patina binaries and run that image on QEMU. You will need to have your own "
                    "QEMU installation to run the image if you choose this option.\n\n"
                )

                _LOGGER.info("Would you like to:")
                _LOGGER.info(
                    "  [1] Build a new QEMU UEFI image from source (recommended)"
                )
                _LOGGER.info(
                    f"  [2] Use a pre-compiled QEMU {self._settings.package} UEFI binary"
                )
                _LOGGER.info("\nSelect an option [1 or 2]: ")
                choice = input().strip()
            else:
                choice = "1"

            if choice == "1":
                _Utils.print_divider(_LOGGER)
                self.qemu_workspace_setup()
                self._run_stuart_build()

                _LOGGER.info(
                    '\nThe UEFI build is complete. For more information and steps to run "stuart_buid" '
                    "without this wizard, visit "
                    "https://github.com/tianocore/tianocore.github.io/wiki/How-to-Build-With-Stuart"
                )

                exit(0)

        # Assume we are attempting to patch from this point on

        if not self._settings.patch_config.path.exists():
            _LOGGER.info(
                "\nYou are attempting to patch a QEMU UEFI image with Patina binaries. This is the fastest "
                "way to test Patina changes on QEMU! After you complete setting up patching config once, you will "
                "not need to do it again.\n"
            )
            if not self._settings.default_prompt_choices:
                _LOGGER.info("\nPress Enter to continue or Ctrl+C to cancel.")
                input()

            self.patch_config_setup()
            self._settings.patch_config.save()
        else:
            _LOGGER.info(
                f"\nTip: A patch config file already exists at {self._settings.patch_config.path}. "
                "You can edit it if you want to change the patching configuration.\n"
            )
            self._settings.patch_config.load()

        _Utils.print_divider(_LOGGER)

        _LOGGER.info("\n" + str(self._settings.patch_config) + "\n")
        _LOGGER.info(
            "\nTip: You can modify this patch config file if you want to change the patching configuration:\n"
            "  " + str(self._settings.patch_config.path)
        )
        if not self._settings.default_prompt_choices:
            _LOGGER.info(
                "\nPress enter to continue and attempt to run the patching script or Ctrl+C to cancel."
            )
            input()

        self.run_patching_script()


def _internal_main(workspace_dir: Path, package: str, args: argparse.Namespace) -> None:
    """
    Internal main function for the wizard. It sets up the workspace for patching Patina binaries.

    Args:
        workspace_dir (Path): The path to the workspace directory.
        package (str): The package for which to set up the workspace (e.g., "Q35" or "SBSA").
        args (argparse.Namespace): The command-line arguments for this invocation.
    """
    wizard = _Wizard(workspace_dir, package)
    if args.default_prompt_choices:
        wizard.use_default_prompt_choices()
    if args.show_build_output:
        wizard.show_build_output()

    if wizard.is_needed():
        wizard.start()
    else:
        _LOGGER.debug(
            f"A local config file already exists at {wizard.get_patch_config().path}"
        )

        patch_config = wizard.get_patch_config()
        patch_config.load()

        _Utils.print_divider(_LOGGER)

        _LOGGER.info("\n" + str(patch_config) + "\n")
        _LOGGER.info(
            "\nTip: You can modify this patch config file if you want to change the patching configuration:\n"
            "  " + str(wizard.get_patch_config().path)
        )
        if not args.default_prompt_choices:
            _LOGGER.info(
                "\nPress enter to continue and attempt to run the patching script or Ctrl+C to cancel."
            )
            input()

        wizard.run_patching_script()


def _parse_args() -> argparse.Namespace:
    """
    Parses command-line arguments.

    Returns:
        argparse.Namespace: The parsed command-line arguments.
    """
    parser = argparse.ArgumentParser(
        prog=PROGRAM_NAME,
        description=(
            "Sets up a Patina workspace to build UEFI code and patch Patina binaries into QEMU "
            "UEFI images. It is recommend to run this script without any arguments to get "
            "started."
        ),
    )

    parser.add_argument(
        "--hide-tips",
        action="store_true",
        default=False,
        help="Hide tip messages in the output.",
    )

    parser.add_argument(
        "--no-wrap-columns-to-terminal",
        action="store_false",
        dest="wrap_columns_to_terminal",
        default=True,
        help="Do not wrap text output to the number of columns in the terminal. Wrapping is enabled by default.",
    )

    parser.add_argument(
        "--default-prompt-choices",
        action="store_true",
        default=False,
        help="Reduces the number of prompts in setup automatically choosing the default option.",
    )

    parser.add_argument(
        "--show-build-output",
        action="store_true",
        default=False,
        help="Displays UEFI build output to the console.",
    )

    return parser.parse_args()


def wizard_main() -> None:
    """
    Main function for the wizard. It sets up the workspace for patching Patina binaries.
    """
    global _LOGGER

    args = _parse_args()
    _LOGGER = _Utils.init_logging(args.hide_tips, args.wrap_columns_to_terminal)

    workspace_dir = Path(__file__).resolve().parent
    package = None

    while package not in ("Q35", "SBSA"):
        print("\nWhich QEMU platform are you setting up?")
        print("  [1] Q35 (x86_64)")
        print("  [2] SBSA (aarch64)")
        choice = input("\nSelect an option [1 or 2]: ").strip()
        if choice == "1":
            package = "Q35"
        elif choice == "2":
            package = "SBSA"
        else:
            print("Invalid choice. Please enter 1 or 2.")

    try:
        _internal_main(workspace_dir, package, args)

    except KeyboardInterrupt:
        _LOGGER.info("\n\nExiting the wizard...")
        _LOGGER.info("You can run the script again to continue the setup process.")

    _Utils.deinit_logging()

    exit(0)


if __name__ == "__main__":
    wizard_main()
