# A prompt on Y keeps the hold of Y from the VR weapon wheel (2026-11-05)

The VR weapon wheel took every hold of Y, so the game never saw `Choice2_Hold`. Doors could not be forced, for one.

`modules/wheel.lua` now takes no new press while a prompt sits on Y. A wheel already open stays open.

Confirmed in game on Index controllers with SteamVR, on a door to be forced. The wheel opens as before where no prompt is on Y.
