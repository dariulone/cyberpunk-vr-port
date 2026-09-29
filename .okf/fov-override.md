---
type: Configuration
title: Explicit FOV override and headset comparison presets
description: User-selectable horizontal FOV override with actual camera FOV readout and coverage tradeoffs.
tags: [psvr2, fov, overlay, clarity]
timestamp: 2026-09-29T19:42:00+09:00
---

# Why this exists

The old fork logged 104.946 degrees horizontal at 3584x3584 with automatic FOV.
Upstream 0.1.7 logs 123 degrees on the same PSVR2. Both runs used DLSS Performance
at 1792x1792 internally. Upstream covers asymmetric panel edges, spreading the
same image resolution over a larger field. This is a plausible contributor to
reduced central detail, not a measured sole cause of perceived softness.

User requested restoring the earlier manual **104.5-degree override**. This is
an explicit coverage tradeoff, not a fix to the automatic projection algorithm.
Narrower coverage may expose borders; verify in the headset. It does not change
resolution, DLSS, sharpening, frame generation or other graphics settings.

# Menu

F10 → STEREO → STEREO VIEW:

- **Enable FOV override**: checked uses the saved positive `xr_force_fov`; unchecked
  writes zero and returns to upstream automatic panel coverage (123 on this setup).
  Re-enabling starts at 104.5 on detected PSVR2, 112 on other headsets.
- **FOV preset**: PSVR2 104.5; Quest 3 legacy 94 (optional test, not universal);
  Pico 4 104; custom via the slider. No headset is silently assigned a new default.
- **Horizontal render / projection FOV**: 80–140 degree manual slider.
- **Camera render FOV**: actual hook-published horizontal/vertical camera values.
  This is not an independent compositor measurement and may show the previous
  frame's value immediately after editing.

Quest 3 already has an aspect-aware resolution ladder, but no dedicated override
checkbox existed. Keep the proper headset resolution selection when comparing
presets. This control changes BOTH the camera and submitted projection; the old
UI text claiming projection-only behavior was wrong and has been replaced.

# Deployment checks

Only `xr_force_fov` was changed from 0.000 to 104.500 in the live INI, while the
game was closed. Original: `bin/x64/vrport.ini.before-1045-override-20260929.bak`.
Check next runtime log for `NormalFOV ... targetH=104.500` and check edge coverage.
Do not claim this improves perceived quality until tested in-headset.

MSVC Release build, existing input regression suite and OKF validation passed.
Deployed DLL/PDB at 2026-09-29 19:42:35 KST, preserving the new override and all
other settings. SHA-256:
`36cc4e30d280fbd11f305f374108791b57e260c9e7d5e1ee6ecc39af83d85418`.
Backup/report: parent worktree `build/psvr2-017-msvc/deployment-20260929-194235`.
The game was not launched; runtime readout and visual acceptance remain pending.
