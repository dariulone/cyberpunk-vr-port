@echo off
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
cd /d %~dp0
cl /nologo /std:c++20 /O2 /EHsc /W3 /D_CRT_SECURE_NO_WARNINGS loopback_meter.cpp /Fe:loopback_meter.exe /link /nologo > build.log 2>&1
cl /nologo /std:c++20 /O2 /EHsc /W0 /D_CRT_SECURE_NO_WARNINGS /I..\..\externals\stb ogg_check.cpp /Fe:ogg_check.exe /link /nologo >> build.log 2>&1
cl /nologo /std:c++20 /O2 /EHsc /W3 /D_CRT_SECURE_NO_WARNINGS window_grab.cpp /Fe:window_grab.exe /link /nologo >> build.log 2>&1
type build.log
