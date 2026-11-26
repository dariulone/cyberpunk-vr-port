@echo off
setlocal
"%~dp0CyberpunkVRProfiler.exe" --duration 300 %*
echo.
echo Recording finished. Send the session folder shown above.
pause
