@echo off
rem Builds holster_icons.exe into build\holster-icons (MSVC, VS 18 Community).
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
if not exist "%~dp0..\..\build\holster-icons" mkdir "%~dp0..\..\build\holster-icons"
cl /nologo /O2 /EHsc /std:c++17 /utf-8 "%~dp0holster_icons.cpp" /Fo"%~dp0..\..\build\holster-icons\\" /Fe"%~dp0..\..\build\holster-icons\holster_icons.exe"
exit /b %ERRORLEVEL%
