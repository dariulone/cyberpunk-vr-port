# Fingers of an empty hand from the controller's finger sensing (2026-11-05)

Reads `XR_EXT_hand_tracking` and drives each finger of an empty hand on its own (`src/Runtimes/HandTracking.cpp`,
`FingerClosed`, `FromFingers`, `PublishFingers` in `Anim/HandGestures`). Setting `xr_finger_tracking`, checkbox in the Avatar tab. A runtime without the extension, or the setting off, leaves the hand on grip and trigger. A hand holding something keeps its grip.

## Measured on Index controllers with SteamVR

- The extension works, data source 2 (controller), all five fingers individually.
- Bend is about 20-30 degrees open and up to about 280 degrees closed for the four fingers. The thumb is about 30-120 degrees, in coarse steps.
- The outer three fingers' sensors call a finger open about a centimetre off the handle. So they open at a set pace and stay a little bent.
- A thumb off the controller is the thumbs-up on both hands now.

One tester's right controller reports its thumb on the stick nearly all the time, and SteamVR's own binding screen shows no touch on that controller's buttons either. That is a hardware or SteamVR problem, not this code.

## Words

The two new strings are added to every table in `modules/lang/settings`. The generator named in those files (`tools/menu_locale/build_lang.py`) is not in the repository, so they were added to the tables directly. Translated without native speakers checking them.

## Not tried

Any other headset or runtime.
