# Demonstration of Patina in a QEMU UEFI Platform Build

[![CI](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/platform-ci.yml/badge.svg)](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/platform-ci.yml)
[![Nightly Regression Tests](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/nightly-os-boot.yml/badge.svg?event=schedule)](https://github.com/OpenDevicePartnership/patina-qemu/actions/workflows/nightly-os-boot.yml)

The primary purpose of this repository is to demonstrate integrating code from the Open Device Partnership's Patina project
into a UEFI platform build and is meant to be a "first stop" for developers exploring ODP and the Patina Boot Firmware. It
contains a permanent fork of [OvmfPkg](https://github.com/tianocore/edk2/tree/HEAD/OvmfPkg) from EDK II with changes based
on the following:

- Documentation
  - [Open Device Partnership (ODP) documentation](https://opendevicepartnership.org/)
  - [Patina project documentation](https://opendevicepartnership.github.io/patina/)
- GitHub Links
  - [ODP GitHub organization](https://github.com/OpenDevicePartnership)
  - [Patina GitHub repository](https://github.com/OpenDevicePartnership/patina)
  - [Patina DXE Core QEMU repository](https://github.com/OpenDevicePartnership/patina-dxe-core-qemu) showcasing the sample
Patina DXE core .efi binary used by this repository

As Rust adoption increases, it is important for each user to determine best way to incorporate changes during the transition
away from code written in C toward code written in Rust.  UEFI inherently supports dynamic integration, so at a high level
there are two basic approaches:

1. Build the code using Rust tools in a stand-alone workspace to produce a .efi binary that is later integrated into the
Firmware Device (FD) image
2. Add support to the EDK II build infrastructure to compile the Rust source code alongside the C source code when processing
each module specified in a DSC file

This 2nd approach is a viable solution, but the Patina project and the following documentation are focused primarily on the
first approach since it allows for a more natural Rust development experience using only Rust tools and processes, and also
greatly simplifies the integration by not requiring modifications to EDK II build scripts.  However, both options are discussed
in the [Rust Integration](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/docs/Rust_Integration.md) documentation
to help each end-user determine what best fits their usage model.

## Compiling this Repository

There are two platform projects currently supported in this repository:

- [QEMU Q35](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/Docs/Q35/QemuQ35_ReadMe.md) supports
an Intel Q35 chipset
- [QEMU SBSA](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/Platforms/Docs/SBSA/QemuSbsa_ReadMe.md) supports
an ARM System Architecture

Both packages can be built in either a Windows or Linux environment as outlined in the
[Build Details](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/docs/Build_Details.md) document.  But for
simplicity, it is recommended to start by using the environment in the [Dev Container](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/.devcontainer/devcontainer.json)
used by this repository's CI build.  It provides an Ubuntu command line prompt with all of the proper tools and environment
settings necessary with minimal changes to the development platform.

### Install WSL for Windows

If compiling in Linux, this step can be skipped.  If compiling on Windows, the container is most stable running in WSL
(Windows subsystem for Linux) which needs to be [installed](https://learn.microsoft.com/windows/wsl/install) before
proceeding. The default distribution, Ubuntu, is what is used for this demo and the following steps assume the user has
opened a command box and is at the WSL command prompt.

**Hint:** Files can be shared between the Windows file system and WSL by using `\\wsl.localhost\Ubuntu\home\<user name>`
in file explorer to see into WSL, and the paths `/mnt/c`, `/mnt/d`, etc can be used in WSL to see the Windows drives.
But the translation layer can cause significant delays and line ending errors during compilation if code is cloned in the
Windows file system and attempted to be built in WSL or the container environment.

### Install a Container Manager

A manager needs to be installed to load the dev container's environment.  This example is using [podman](https://podman.io/)
which is open source, but other applications such as Docker can be used for more advanced options such as
[Developing inside a Container](https://code.visualstudio.com/docs/devcontainers/containers).

At the Linux command prompt, type the following to download podman and test the installation.

``` bash
  sudo apt-get update
  sudo apt-get install -y podman
  podman --version
```

The podman `run` command is then used to download the container and load its Ubuntu environment.  If using a different manager,
the parameters were created using the data in the [devcontainer.json](https://github.com/OpenDevicePartnership/patina-qemu/blob/refs/heads/main/.devcontainer/devcontainer.json)
file.

``` bash
  podman run -it \
    --privileged \
    --name "patina-dev" \
    -v "$PWD:/workspace" \
    -p 5005-5008:5005-5008 \
    -v /tmp/.X11-unix:/tmp/.X11-unix \
    -e DISPLAY="${DISPLAY:-:0}" \
    "ghcr.io/microsoft/mu_devops/ubuntu-24-dev:latest" \
    /bin/bash
```

At this point, the command prompt is an Ubuntu operating environment with all repository and build tools necessary to compile.

**Hint1:** Any files created inside the container will not be accessible outside the container except for files created in
the `workspace` directory.  That path was created by the `-v` command line parameter and is a virtual mapping to the working
directory podman was executed from.  For instance, if it was launched from `~/`, the directory `/workspace` will give
access to the user root.  It is recommended to do all work in that workspace directory while in the container.

``` bash
  cd workspace
```

**Hint2:** The name `patina-dev` was used to tell podman to log the parameters so next time the container needs to be run,
the command can be shortened.

``` bash
  podman start -ai "patina-dev"
```

### Clone and build

Git was a clean install in the container config, so the first time entering the container, the user information needs to
be set.

``` bash
  git config --global user.email <your email address>
  git config --global user.name "<your user name>"
```

The repository can now be cloned normally and Git needs to be told it is a safe repo.

``` bash
  git clone https://github.com/OpenDevicePartnership/patina-qemu.git
  git config --global --add safe.directory '*'
```

Since this is inside the container with proper tools/environment available and away from the host environment, it is
safe to install any global pip requirements and execute the Stuart commands without any specific toolchain tags.

```shell
  pip install --upgrade -r pip-requirements.txt
  stuart_setup -c Platforms/QemuSbsaPkg/PlatformBuild.py
  stuart_update -c Platforms/QemuSbsaPkg/PlatformBuild.py
  stuart_build -c Platforms/QemuSbsaPkg/PlatformBuild.py --flashrom
```

The final stuart_build command will compile the ARM support code, launch QEMU, and boot into the UEFI shell to demonstrate
the loading of the pre-built Patina DXE Core.  Switching the path from `QemuSbsaPkg` to `QemuQ35Pkg` will compile the Q35
X86 architecture platform package.

For more options or details about building in your native environment or integrating changes, please refer to
[Rust Integration](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/docs/Rust_Integration.md)
or [Build Details](https://github.com/OpenDevicePartnership/patina-qemu/blob/main/docs/Build_Details.md).

## Platform Validation Testing

This repository contains scheduled github workflows to detect regressions in firmware. See [Platforms/Docs/Common/regression-testing.md](https://github.com/OpenDevicePartnership/patina-qemu/tree/main/Platforms/Docs/Common/regression-testing.md).
