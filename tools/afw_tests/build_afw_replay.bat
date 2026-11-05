@echo off
rem afw_replay.exe: PureDark's AFW plugin run on captured frames and scored (see afw_replay.cpp). Run from a shell
rem with the VS x64 tools, or let this find them.
setlocal
if not defined VSINSTALLDIR (
  for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath`) do call "%%i\VC\Auxiliary\Build\vcvars64.bat" >nul
)
cd /d "%~dp0"
cl /nologo /O2 /EHsc /W3 /std:c++17 /D_CRT_SECURE_NO_WARNINGS /I..\..\include afw_replay.cpp /Fe:afw_replay.exe /link d3d12.lib dxgi.lib /SUBSYSTEM:CONSOLE
del /q afw_replay.obj 2>nul
