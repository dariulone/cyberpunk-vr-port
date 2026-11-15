@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install_cyberfinger.ps1" -ConciseErrors %*
exit /b %ERRORLEVEL%
