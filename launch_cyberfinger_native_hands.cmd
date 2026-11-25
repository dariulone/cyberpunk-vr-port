@echo off
setlocal
rem Diagnostic launch: bypass the Ultraleap OpenXR layer for THIS game process only.
rem SteamVR/CyberFinger continue to supply hand tracking. No global settings change.
rem https://docs.ultraleap.com/openxr/configuration/disabling-the-layer/index.html
set "DISABLE_XR_APILAYER_ULTRALEAP_HAND_TRACKING_1=1"
set "CYBERFINGER_GAME_BIN=C:\Program Files (x86)\GOG Galaxy\Games\Cyberpunk 2077\bin\x64"
if not exist "%CYBERFINGER_GAME_BIN%\Cyberpunk2077.exe" (
    echo Cyberpunk2077.exe was not found in the configured GOG directory.
    exit /b 1
)
echo Starting Cyberpunk with SteamVR hand tracking; Ultraleap's OpenXR layer is bypassed for this launch.
start "" /D "%CYBERFINGER_GAME_BIN%" "%CYBERFINGER_GAME_BIN%\Cyberpunk2077.exe"
endlocal
