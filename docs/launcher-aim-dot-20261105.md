# A steady aim dot for the arm launcher (2026-11-05)

While the projectile launcher cyberware is selected, the barrel dot marks its shot line, whatever the laser dot option says.
With the launcher selected no gun may ever have been drawn, so the direction cannot come from a latched muzzle direction.

`cvr::cyberware::LauncherLine` gives origin and direction, used in `Hooks/FinalCamera.cpp` and `Overlay/OverlayDebugDraw.cpp`.

## Which bone

The line follows `launcher_base` +Z. `launcher_fragment_fire` and its marker sway through the idle animation, and they carried both
the dot and the rocket with them.

## Tested

Confirmed in game on Index controllers with SteamVR.

`tools/vrik_tests` has a new case for the launcher pose. That suite did not link as shipped on this machine (8 missing symbols), and `launcher_pose`
failed both before and after this change, so it says nothing about the change either way.
