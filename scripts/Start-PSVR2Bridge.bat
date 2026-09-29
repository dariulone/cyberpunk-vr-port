@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Start-PSVR2Bridge.ps1" %*
set "RC=%ERRORLEVEL%"
pause
exit /b %RC%
