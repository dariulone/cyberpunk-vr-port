@echo off
rem Builds wheel_art.exe into build\wheel-art (MSVC, VS 18 Community).
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
if not exist "%~dp0..\..\build\wheel-art" mkdir "%~dp0..\..\build\wheel-art"
cl /nologo /O2 /EHsc /std:c++17 /utf-8 "%~dp0wheel_art.cpp" /Fo"%~dp0..\..\build\wheel-art\\" /Fe"%~dp0..\..\build\wheel-art\wheel_art.exe"
exit /b %ERRORLEVEL%
