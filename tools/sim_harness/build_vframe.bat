@echo off
rem vframe.exe: one decoded frame of a recorded clip (see vframe.cpp). Run from a shell with the VS x64 tools, or
rem let this find them.
setlocal
if not defined VSINSTALLDIR (
  for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath`) do call "%%i\VC\Auxiliary\Build\vcvars64.bat" >nul
)
cd /d "%~dp0"
cl /nologo /O2 /EHsc /W3 /std:c++17 /DUNICODE /D_UNICODE vframe.cpp /Fe:vframe.exe /link /SUBSYSTEM:CONSOLE
del /q vframe.obj 2>nul
