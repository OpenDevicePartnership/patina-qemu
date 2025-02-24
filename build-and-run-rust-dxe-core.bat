@echo off
setlocal enabledelayedexpansion

rem Customizable options (change these to match your system)
set "QEMU_RUST_BIN_REPO=C:\src\qemu_rust_bins"
set "FW_PATCH_REPO=C:\src\fw_rust_patcher"

rem NOTE: The DEBUG VS2022 build is used by default. Find/replace as needed.
set "UEFI_RUST_DIR=%~dp0"
set "CODE_FD=%UEFI_RUST_DIR%Build\QemuQ35Pkg\DEBUG_VS2022\FV\QEMUQ35_CODE.fd"
set "REF_FD=%CODE_FD:.fd=.ref.fd%"
set "CONFIG_FILE=Configs\QemuQ35.json"

if "%1" == "--help" (
  echo Usage: build-and-run-rust-dxe-core.bat [options]
  echo.
  echo Options:
  echo   --qemu-rust-bin-repo Path to the QEMU Rust bin repository. Default: %QEMU_RUST_BIN_REPO%
  echo   --fw-patch-repo      Path to the firmware patch repository. Default: %FW_PATCH_REPO%
  exit /b 0
)

:parse_args
if "%1" == "--qemu-rust-bin-repo" (
  set "QEMU_RUST_BIN_REPO=%2"
  shift
  shift
  goto :parse_args
)
if "%1" == "--fw-patch-repo" (
  set "FW_PATCH_REPO=%2"
  shift
  shift
  goto :parse_args
)

if "%QEMU_RUST_BIN_REPO:~-1%" == "\" set "QEMU_RUST_BIN_REPO=%QEMU_RUST_BIN_REPO:~0,-1%"
if "%FW_PATCH_REPO:~-1%" == "\" set "FW_PATCH_REPO=%FW_PATCH_REPO:~0,-1%"

rem NOTE: Rust debug build is used by default. Change the profile to release if needed.
set "DXE_CORE=%QEMU_RUST_BIN_REPO%\target\x86_64-unknown-uefi\debug\qemu_q35_dxe_core.efi"

echo "==Current Configuration=="
echo QEMU Rust Bin Repo (qemu_rust_bins): %QEMU_RUST_BIN_REPO%
echo DXE Core EFI File: %DXE_CORE%
echo FW Patch Repo: %FW_PATCH_REPO%

for /f "tokens=2 delims=:" %%a in ('mode con^|more +4') do if not defined width set /a width=%%a
for /l %%a in (1,1,%width%) do set "line=!line!-"

echo Building Rust DXE Core && echo.
cargo -Zunstable-options -C "%QEMU_RUST_BIN_REPO%" build_q35 || goto :error
cargo -Zunstable-options -C "%QEMU_RUST_BIN_REPO%" build_q35 --profile release || goto :error

echo %line%

echo Patching Rust DXE Core && echo.
IF NOT EXIST "%REF_FD%" (
  echo f | xcopy "%CODE_FD%" "%REF_FD%"
)

cd "%FW_PATCH_REPO%"
python patch.py -c "%FW_PATCH_REPO%\%CONFIG_FILE%" -i "%DXE_CORE%" -r "%REF_FD%" -o "%CODE_FD%" || goto :error
cd "%UEFI_RUST_DIR%"

echo %line%

echo Running QEMU with Rust DXE Core Build && echo.

"%UEFI_RUST_DIR%QemuPkg\Binaries\qemu-win_extdep\qemu-system-x86_64" ^
  -debugcon stdio -L %UEFI_RUST_DIR%QemuPkg\Binaries\qemu-win_extdep\share ^
  -global isa-debugcon.iobase=0x402 ^
  -global ICH9-LPC.disable_s3=1 ^
  -machine q35,smm=on ^
  -m 2048 ^
  -cpu qemu64,rdrand=on,umip=on,smep=on,pdpe1gb=on,popcnt=on,+sse,+sse2,+sse3,+ssse3,+sse4.2,+sse4.1,invtsc ^
  -smp 4 ^
  -global driver=cfi.pflash01,property=secure,value=on ^
  -drive if=pflash,format=raw,unit=0,file=%UEFI_RUST_DIR%Build\QemuQ35Pkg\DEBUG_VS2022\FV\QEMUQ35_CODE.fd,readonly=on ^
  -drive if=pflash,format=raw,unit=1,file=%UEFI_RUST_DIR%Build\QemuQ35Pkg\DEBUG_VS2022\FV\QEMUQ35_VARS.fd ^
  -device qemu-xhci,id=usb -device usb-tablet,id=input0,bus=usb.0,port=1 ^
  -net none ^
  -smbios type=0,vendor="Project Mu",version="mu_tiano_platforms-v9.0.0",date=12/19/2023,uefi=on ^
  -smbios type=1,manufacturer=Palindrome,product="QEMU Q35",family=QEMU,version="9.0.0",serial=42-42-42-42,uuid=9de555c0-05d7-4aa1-84ab-bb511e3a8bef ^
  -smbios type=3,manufacturer=Palindrome,serial=40-41-42-43 ^
  -vga cirrus ^
  -serial tcp:127.0.0.1:50001,server,nowait || goto :error

goto :EOF

:error
echo Failed with error #%errorlevel%.
exit /b %errorlevel%
