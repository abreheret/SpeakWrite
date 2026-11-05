@echo off
rem Build the Kyutai STT spike on Windows.
rem Uses CUDA when nvcc is on PATH, CPU otherwise. Extra args go to cargo.
setlocal

rem CMake 4 dropped support for cmake_minimum_required < 3.5 (old opus in audiopus_sys).
set "CMAKE_POLICY_VERSION_MINIMUM=3.5"

rem cargo (cc/cmake) and nvcc need the MSVC toolchain: load it when cl.exe is missing.
where cl.exe >nul 2>&1 && goto :have_msvc
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto :no_msvc
for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSDIR=%%i"
if not defined VSDIR goto :no_msvc
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat" >nul
:have_msvc

rem vcvars may change the current directory: go back to the crate.
cd /d "%~dp0"

where nvcc.exe >nul 2>&1 && goto :cuda
echo nvcc not found: building CPU-only
cargo build --release %*
exit /b %errorlevel%

:cuda
echo Building with --features cuda
cargo build --release --features cuda %*
exit /b %errorlevel%

:no_msvc
echo Visual Studio C++ build tools not found (required by cargo and nvcc).
exit /b 1
