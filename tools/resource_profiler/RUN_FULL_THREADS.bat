@echo off
setlocal
"%~dp0CyberpunkVRProfiler.exe" --threads all %*
echo.
echo Recording finished. Send the session folder shown above.
pause
