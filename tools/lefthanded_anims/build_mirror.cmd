@echo off
rem Builds mirror_anims.exe into build\lefthanded_anims (MSVC, VS 18 Community).
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
if not exist "%~dp0..\..\build\lefthanded_anims" mkdir "%~dp0..\..\build\lefthanded_anims"
cl /nologo /O2 /EHsc /std:c++17 /utf-8 "%~dp0mirror_anims.cpp" /Fo"%~dp0..\..\build\lefthanded_anims\\" /Fe"%~dp0..\..\build\lefthanded_anims\mirror_anims.exe"
exit /b %ERRORLEVEL%
