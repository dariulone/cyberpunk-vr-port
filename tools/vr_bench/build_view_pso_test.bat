@echo off
rem view_pso_test.exe: the single-pass shader rewrite over a folder of shaders (see view_pso_test.cpp).
setlocal
if not defined VSINSTALLDIR (
  for /f "usebackq delims=" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath`) do call "%%i\VC\Auxiliary\Build\vcvars64.bat" >nul
)
cd /d "%~dp0"
cl /nologo /O2 /EHsc /W3 /std:c++20 /DNOMINMAX /DWIN32_LEAN_AND_MEAN /DUNICODE /D_UNICODE /I..\..\include view_pso_test.cpp ../../src/Render/ViewInstancedPipeline.cpp /Fe:view_pso_test.exe /link /SUBSYSTEM:CONSOLE ole32.lib
set rc=%errorlevel%
del /q view_pso_test.obj view_pso_test.exp view_pso_test.lib ViewInstancedPipeline.obj 2>nul
exit /b %rc%
