---
type: Integration
title: PSVR2 Sense controls and advanced-haptics bridge on 0.1.7
description: Restore Triangle touch and Create menus while preserving upstream behavior and safe Toolkit haptic ownership.
tags: [psvr2, controls, haptics, upstream]
timestamp: 2026-09-29T19:27:12+09:00
---

# Branch policy

User instruction: **all future commits stay on `psvr2-tweaks-0.1.7`**. Do not
create a new branch for each feature. This branch starts at official 0.1.7
`a0c23fe110530bb74a73b3adf1ffa7d83050c43d`. Its current worktree is
`F:\ai\GITHUB\cyberpunk-vr-port\build\upstream-0.1.7`; the older parent
`psvr2-tweaks` worktree has unrelated dirty work and must not be overwritten.
Quest/general OpenXR haptics must be a separate commit on this same branch.

# First modification

PSVR2-only Triangle capacitive touch shifts right stick to D-pad with complete
look suppression. Triangle click remains Y. Create uses the global application
SystemButton action: <500 ms release is Start, >=500 ms is one Back event.
Triangle+R3 provides the fallback without claiming upstream's L3+R3 overlay chord.
Timing runs on game polling and cancels on loss of input/focus or overlay capture.
See `docs/PSVR2-TWEAKS-017.md` for controls, manual SteamVR binding and tests.

# Haptic ownership and ABI

The external Toolkit DSX-compatible bridge remains the sole Sense actuator owner;
the DSX app and the mod's UDPClient must not run concurrently. No vibration action
or `xrApplyHapticFeedback` path is added by this commit. Enhanced DualSense Support
continues to produce weapon/adaptive-trigger profiles and the bridge mixes audio
haptics. `scripts/Start-PSVR2Bridge.bat` launches the checked configuration.

Critical: upstream slots 157-160 are right B, left Y, R3 and right trigger, NOT
the old bridge's sequence/hand/amplitude/duration. The new launcher forces
`--no-vr-motion-haptics`. Explicit legacy swing/impact pulses are not supported
until a separate versioned IPC producer/consumer is implemented. Do not remap
upstream slots or use the old shortcut with its motion watcher enabled.

# Validation

`tools/psvr2_input_tests` passed 24 checks with GCC and MSVC Release via CTest.
The full MSVC Release plugin build passed. Installed bridge `-CheckOnly` passed
with no process started and no third-party files edited. OKF validation passed.

Deployed 2026-09-29 19:27:12 KST: plugin DLL/PDB to the existing official 0.1.7
installation and `Start-PSVR2Bridge.bat/.ps1` to
`C:\Users\gthom\Downloads\PSVR2Toolkit-DSX-Bridge`.
DLL SHA-256: `5859ff870f7deced7a8acf89419f056d1303f66c96ad99bbe3a1f71c6b15a1de`.
Backup/report: `build/psvr2-017-msvc/deployment-20260929-192712` relative to the
older parent worktree. Runtime settings, launcher selection, calibration and
game UserSettings were hash-checked unchanged; `xr_force_fov=0` remains set.
Neither game nor bridge was started by deployment. Physical headset acceptance
is still pending. Do not equate action suggestions with working bindings.
