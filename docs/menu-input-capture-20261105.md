# A resting grip no longer leaves the controllers without input (2026-11-05)

Two pieces around the native overlay's input capture, which zeroes all controller input for the game while it holds.

## The capture never let go

`releasePending` in `Overlay/VrOverlay.cpp` waits for sticks, triggers and both grips to be released after the overlay closes. A grip staying above one half, as a hand resting on Index controllers does, or a grip held through the whole menu, kept it pending forever, and with it the capture. The game then had no input at all.

Now grips have to let go first only for 1.5 s. Sticks and triggers wait as before.

## The VR menu read through the capture

`VRMenuPointer` read its trigger through the same cleared state, so a lingering capture also left the menu itself without input. It now reads `GetRawControllerState`, the snapshot taken just before the capture clears it in the frame loop.

## Tested

Used in game on Index controllers with SteamVR since 2026-10, and the menu has not gone dead since. The diagnosis came from logging capture state and raw grips at the time; that logging is not part of this change.
