@echo off
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
cd /d %~dp0
cl /nologo /std:c++20 /O2 /EHsc /W4 /D_CRT_SECURE_NO_WARNINGS hand_trim.cpp /Fe:hand_trim.exe /link /nologo > build.log 2>&1
type build.log
