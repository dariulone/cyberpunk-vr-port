@echo off
rem stereo_check.exe: left/right agreement of a captured frame (see stereo_check.cpp). Run from a shell with the VS x64
rem tools, or let this find them.
setlocal
if not defined VSINSTALLDIR (
  for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath`) do call "%%i\VC\Auxiliary\Build\vcvars64.bat" >nul
)
cd /d "%~dp0"
cl /nologo /O2 /fp:fast /EHsc /W3 /std:c++17 /D_CRT_SECURE_NO_WARNINGS /DUNICODE /D_UNICODE stereo_check.cpp /Fe:stereo_check.exe /link /SUBSYSTEM:CONSOLE
set rc=%errorlevel%
del /q stereo_check.obj 2>nul
exit /b %rc%
