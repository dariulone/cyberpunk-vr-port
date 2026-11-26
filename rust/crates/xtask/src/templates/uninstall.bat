@echo off
setlocal EnableDelayedExpansion
cd /d "%~dp0"

echo.
echo   CyberpunkVRPort $Version  --  uninstall
echo   =======================================
echo.

rem ---- refuse to run anywhere but the game root ----------------------------------------------
rem Every path below is relative, so in the wrong folder this would delete nothing rather than
rem something -- but saying so beats a run that reports "already gone" 45 times and looks done.
if not exist "bin\x64\Cyberpunk2077.exe" goto :notgameroot

rem ---- and not while the game is running ------------------------------------------------------
rem A locked DLL fails silently enough to leave half a mod behind, and the settings restore below
rem would be overwritten by the running game on exit.
rem Windows' own find.exe by its path: a Unix find earlier on PATH (Git's, MSYS2's) takes /i for a folder,
rem fails, and the check passes with the game up.
tasklist /fi "imagename eq Cyberpunk2077.exe" 2>nul | "%SystemRoot%\System32\find.exe" /i "Cyberpunk2077.exe" >nul
if not errorlevel 1 goto :stillrunning

echo   Game folder:  %CD%
echo.
echo   This removes every file CyberpunkVRPort installed -- $($ownDirs.Count) folders, $($loose.Count) loose files,
echo   plus the files the plugin writes at runtime.
echo.
echo   See UNINSTALL.txt for the complete removal and settings-restoration instructions.
echo   Your Cyberpunk settings are offered back at the end.
echo.
set "GO="
set /p "GO=  Remove CyberpunkVRPort now? [y/N] "
rem First character only, so y / Y / yes / a stray trailing space all mean yes, and an empty
rem answer (just Enter) does not.
if /i not "!GO:~0,1!"=="y" goto :nothingdone
echo.

set /a GONE=0
set /a ABSENT=0
set /a STUCK=0

$batDirs

$batFiles

call :killfile "bin\x64\vrport.ini"
call :killfile "bin\x64\vrport-launcher.ini"
call :killfile "bin\x64\cyberpunkvrport.log"
rem Written by the plugin for Input Loader (Core/InputConfig.cpp), when that mod is installed.
call :killfile "r6\input\CyberpunkVRPort_ScannerHud.xml"
call :killfile "r6\input\CyberpunkVRPort_PhotoMode.xml"

echo.
echo   Removed !GONE!, already gone !ABSENT!, could not remove !STUCK!.
if !STUCK! GTR 0 (
  echo.
  echo   Something is holding those files. Close the game and Steam and run this again.
)

rem ---- offer the player's own settings back ---------------------------------------------------
rem The newest backup, because the plugin only ever writes one and only on a first launch, but a
rem reinstall makes a second and the newest is the one from the install being removed.
set "CFG=%LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077"
set "BAK="
for /f "delims=" %%F in ('dir /b /o-d "%CFG%\UserSettings.pre-vr-*.json" 2^>nul') do (
  if not defined BAK set "BAK=%%F"
)
if not defined BAK goto :nobackup

echo.
echo   Your own Cyberpunk settings were saved before this mod applied its VR preset:
echo     !BAK!
set "GO2="
set /p "GO2=  Put them back now? [y/N] "
if /i not "!GO2:~0,1!"=="y" (
  echo   Left alone. It is still there whenever you want it.
  goto :done
)
copy /y "%CFG%\!BAK!" "%CFG%\UserSettings.json" >nul
if errorlevel 1 (
  echo   FAILED. Do it by hand: rename
  echo       %CFG%\!BAK!
  echo       over UserSettings.json
) else (
  echo   Restored. Your graphics, comfort and language settings are yours again.
)
goto :done

:nobackup
echo.
echo   No UserSettings.pre-vr-*.json found in
echo     %CFG%
echo   So either the VR preset was never applied, or the backup has been moved.
goto :done

:done
echo.
echo   Done. UNINSTALL.txt is still here if you want to check anything by hand --
echo   delete it, INSTALL.txt and this file whenever you like.
echo.
pause
exit /b 0

:notgameroot
echo   This is not the Cyberpunk 2077 folder:
echo     %CD%
echo.
echo   bin\x64\Cyberpunk2077.exe is not here. UNINSTALL.bat belongs in the game root --
echo   the folder that holds bin\, r6\, red4ext\ and archive\ -- and has to run there.
echo.
pause
exit /b 1

:stillrunning
echo   Cyberpunk 2077 is running.
echo.
echo   Close it first: the plugin DLL is locked while the game is up, and the game
echo   rewrites its settings file on exit, which would undo the restore below.
echo.
pause
exit /b 1

:nothingdone
echo.
echo   Nothing was deleted.
echo.
pause
exit /b 0

rem ---- helpers, past every exit so nothing falls into them ------------------------------------
:killdir
if not exist "%~1\" (
  set /a ABSENT+=1
  exit /b 0
)
rmdir /s /q "%~1" 2>nul
if exist "%~1\" (
  echo   LOCKED      %~1\
  set /a STUCK+=1
) else (
  echo   removed     %~1\
  set /a GONE+=1
)
exit /b 0

:killfile
if not exist "%~1" (
  set /a ABSENT+=1
  exit /b 0
)
del /f /q "%~1" 2>nul
if exist "%~1" (
  echo   LOCKED      %~1
  set /a STUCK+=1
) else (
  echo   removed     %~1
  set /a GONE+=1
)
exit /b 0