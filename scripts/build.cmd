@echo off
rem Release build of SpeakWrite on Windows.
rem Dictation uses the GPU (CUDA) when nvcc is on PATH, the CPU otherwise.
rem Extra arguments are passed to cargo.
setlocal

rem nvcc needs the MSVC host compiler (cl.exe): load the Visual Studio environment if missing.
where cl.exe >nul 2>&1 && goto :have_msvc
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto :no_msvc
for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSDIR=%%i"
if not defined VSDIR goto :no_msvc
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat" >nul
:have_msvc

rem vcvars may change the current directory: go to the workspace root.
cd /d "%~dp0.."

where nvcc.exe >nul 2>&1 && goto :cuda
echo nvcc not found: dictation will run on the CPU
cargo build --release -p speakwrite %*
exit /b %errorlevel%

:cuda
echo Building with --features cuda
cargo build --release -p speakwrite --features cuda %*
exit /b %errorlevel%

:no_msvc
echo Visual Studio C++ build tools not found (required by cargo and nvcc).
exit /b 1
