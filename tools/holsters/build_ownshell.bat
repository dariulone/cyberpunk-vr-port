@echo off
rem ownshell.exe: the port's own holster shells' geometry (see ownshell.cpp).
setlocal
if not defined VSINSTALLDIR call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
cd /d "%~dp0"
cl /nologo /O2 /EHsc /W3 /std:c++17 /D_CRT_SECURE_NO_WARNINGS ownshell.cpp /Fe:ownshell.exe /link /SUBSYSTEM:CONSOLE > ownshell_build.log 2>&1
del /q ownshell.obj 2>nul
type ownshell_build.log
