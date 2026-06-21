# Changelog

All notable changes to the **CyberpunkVR Port** are documented here.

## 0.0.7 — 2026-06-21

The biggest update since 0.0.6: a per-eye reprojection pipeline (**AER V2**), a
**full-body VR avatar**, **decoupled weapon aiming**, **VR motion melee**, a
**hand-to-holster** weapon system, and a reworked **VR controller mapping**.

### VR rendering & reprojection
- **AER V2 (Asynchronous Edge Reprojection).** New per-eye reprojection pipeline
  that synthesises the second eye / intermediate frames from the game's mono
  output using NVIDIA Optical Flow (NvOF) motion vectors plus a depth-aware warp,
  driven by a single unified frame producer with late-IPD application (fixes the
  one-eye jitter). Falls back to a D3D12-compute optical-flow path when CUDA/NvOF
  is not available. (`src/aer_v2/*`, `optical_flow_d3d12`, `mv_warp`,
  `stereo_reproject`)
- Depth capture/resolve and colour-blit passes feeding the reprojection inputs.
- CAS **sharpening** pass with overlay-tunable strength/mix.
- **DLSS / NGX** hook (`ngx_hook`) and DLSS matrix-hook handling.
- **Runtime FOV correction** plus world-scale / IPD-scale / stereo-scale controls
  (fixes "world too big").
- **Pose pair-locking** (anti-tear) and **motion prediction** (pose extrapolation).
- Mono / AER submit **safety flags** (cross-queue wait, depth-capture gating) so
  CP2077 no longer hangs on menus or save/load.

### Full-body VR avatar (VRIK / FRIK)
- Full-body IK: upper body placed under the HMD, distributed spine, **arm-length
  calibration**, leg IK with feet on the ground.
- **Real-life squat** (the avatar lowers when you physically crouch), using a
  neck-pivot height so looking *down* no longer triggers a false squat.
- Camera→head offset is **baked on calibration** so the FPP view sits on the
  avatar's head and the body follows.
- 1:1 hands from the controllers with elbow lean, recoil smoothing, and a rigid
  body-under-camera that removes the weapon-draw torso desync.

### Weapons & combat
- **Decoupled VR weapon aim** ("bullet from the barrel"). The shot launches down
  the real weapon-muzzle direction instead of the camera, with an optional barrel
  crosshair dot that scales with scope zoom. (CET `Weapon` + plugin
  `GetOrientation` override)
- **VR motion melee.** A real controller swing fires the game's *native* melee
  attack along the blade (blade segment vs. NPC body), so damage / reaction /
  stamina behave like the flat game; a fast swing reads as power/cleave. (CET
  `Weapon` + redscript `Melee`)
- **Weapon-Up.** Keeps a drawn melee weapon out of the game's auto-lower /
  auto-unequip path so the katana doesn't vanish after a few seconds. (redscript
  `WeaponUp`)
- **No-Anims.** Neutralises the visual animations that fight VR (camera/hand
  drag) while keeping gameplay systems — ADS, recoil, reload — intact. (redscript
  `NoAnims`)

### Hand-to-holster weapon system
Reach a hand to a body zone and squeeze the **right grip** to equip / unequip:
- **Immersive holsters** *(default)* — equip is chosen by the visual holster you
  reach for: over the right shoulder = primary (rifle/sniper), hip with a katana =
  melee, hip with a pistol = sidearm.
- **Simple slot mode** *(new toggle)* — visual holsters ignored; fixed mapping:
  over-shoulder = `EquipmentSlot1`, right hip = `EquipmentSlot2`, left hip =
  `EquipmentSlot3`.

  *(CET `Holster` + redscript `Holster`; toggle in `F10 → Controller → Immersive
  holsters`.)*

### VR controller bindings
- VR controllers are merged into **XInput gamepad 0**, so every native CP2077
  gamepad action works out of the box.
- **Sprint** by pushing the **left stick fully forward** (no more clicking L3);
  a partial push stays jog.
- **Crouch** by pushing the **right stick fully down** (same bind as the R3 click).
- **Snap turn** (configurable angle + pulse length) or smooth turn.
- **Locomotion direction source**: Game (camera) / HMD (head) / Left hand / Right
  hand.
- Optional **HMD-only pitch** (disable right-stick Y).

### HUD
- Per-element HUD **placement & scale** (minimap/quest, phone, alerts, boss
  health, radio, oxygen, progress bars, …) from the F10 overlay, bridged live to
  the CET HUD mod. (CET `HUD` + redscript `HUD`)

### World map
- **World-map head-lock** bridge: while the map is open the HMD stops driving the
  game camera so the map no longer swims. (CET `WorldMap` + redscript `WorldMap`)

### Misc
- F10 overlay reorganised into tabs with **live, persisted settings**
  (`vrport.ini`); quiet-by-default logging with a verbose toggle.

### Baseline
- **0.0.6** — full VRIK hands, tabbed F10 overlay, HUD/locomotion fixes, repo
  cleanup.
