# Into the Radius 2 and BONELAB: how the body turns and follows the head (2026-10-31)

Players reported "roomscale needs a lot of work: the body gets twisted pretty fast just walking around". This is what
the two reference VR games do, read from their own data, and what the port took from it.

## Into the Radius 2 (Unreal Engine 5, VRExpansionPlugin)

How it was read: the reflection names in `IntoTheRadius2-Win64-Shipping.exe` (UTF-8 property and function names) give
the class layout. The values are in `/Game/ITR2/Configurations/DA_PlayerConfig` in `pakchunk0_s4-Windows.utoc/.ucas`.
That container is unencrypted, indexed and Oodle-compressed (`tools/ue5_study/iostore_read.py` lists and extracts it
through the oo2ext DLL Cyberpunk ships). The asset is a zen package with unversioned properties. The class's
property names in declaration order follow `HealthSoundThresholdInPercent` in the exe; the schema has 3 parent
properties first, and one property past `MaxCameraHeight` is missing from that string run. The two curve references
(`CurveDangerousWaterDamage` = import -2, `CurveWaterDrag` = import -3) anchor the mapping, and the two
`BodyTurningConfig` structs show as nested headers `0x0900` followed by four floats each.

Values (UE units: cm, degrees, degrees/s; absent properties keep the C++ defaults):

| property | value | meaning |
|---|---|---|
| `BodyTurnConfig` | upper 1 deg / lower 70 deg, 150 / 150 deg/s | standing: the upper body follows the head within 1 deg; the legs wait for 70 deg; both turn at 150 deg/s |
| `BodyFollowHeadTurnConfig` | 1 / 1 deg, 500 / 500 deg/s | used while the player moves (`Is Player Moving`) or with "body follows head": the whole body tracks the head |
| `UpperHorizontalDeadZoneRadius` | 9 | the upper body follows the head after 9 cm of horizontal offset |
| `LowerHorizontalDeadZoneRadius` / `LowerHorizontalDeadZoneHalfAngle` | 30 / 60 deg | the legs step after 30 cm inside a forward +-60 deg sector (leaning) |
| `LowerInnerDeadZoneRadius` | 12 | ...and after 12 cm elsewhere |
| `LowerVerticalDeadZoneSize` | 3 | vertical dead zone |
| `CameraShiftUp` | -30 | camera-to-body vertical shift |
| `CrouchAnimationDuration` | 0.3 s | |
| `DistanceGrip*` | speed coeff 0.01, radius 70, dead zone 10, length 150 | the force-grab |

`BPC_BodyController` (blueprint) drives it: `Update Turning Speed` picks one of the two configs from `Is Player Moving`
and `IsBodyFollowHeadRotation`; `Is Looking Down` (`LookDownAngle`), `IsLeaning`, `Is In Horizontal/Vertical Dead
Zone`, `Is Inner Lower Dead Zone Breached`, `Get Shifted Back Camera Location`, `Update Player Rotation After Move`.

## BONELAB (Unity IL2CPP, SLZ.Marrow)

From `BONELAB_RE/names` (see `bonelab-roomscale-twohand-20261030.md`). The body is physical. `PhysTorso` holds the
servos `_headChestServo`, `_neckChestServo`, `_chestSpineServo` and `_chestPelServo`: head, then chest, then pelvis,
each pulled by a joint drive. `OpenControllerRig` smooths the tracked chest and pelvis with gimbals (`_chestGimbalVel`,
`_chestGimbleAccel`, `_pelvisGimbal*`) and keeps a `_twistDebt` with its own velocity. The idea is the same two-tier
chain as Into the Radius 2: the pelvis lags the chest, the chest lags the head, all of them soft.

## The port before

One tier: the entity (legs and the whole skeleton) turned toward the head once the head left the free-look cone
(user ini 10 deg), with a 25 ms exponential capped at 720 deg/s. VRIK kept the spine square to the hips. Any glance
past 10 deg whipped the whole body round with the head.

## What the port does now

- Lower tier (`Runtimes/BodyFollowZone.hpp`): the legs turn at most 150 deg/s (Into the Radius 2's lower-body rate)
  and settle over 80 ms. Measured: 148-149 deg/s mean on 90 deg and 25 deg head turns, 95% of a 90 deg turn in
  0.63 s. Tracked (controller-estimated) body yaw keeps its own smoothing.
- Upper tier (`VRIK_PlaceBodyUnderHMD`, `ChestTwist`): the spine turns the chest toward the head's yaw over the hips,
  at most 40 deg (the trunk's own range; the neck shares out the rest), at most 150 deg/s with an 80 ms settle, and
  fades out between 25 and 45 deg of looking down. Measured: on a 90 deg turn the chest leads by up to 38 deg and
  returns to 0 when the legs arrive. A/B `CyberpunkVR_ChestFollowsHead`, diag `CyberpunkVR_DebugChestTwist`.
- Not taken: the 70 deg leg tolerance is the user's free-look cone (ini `xr_body_free_look_deg`). With the chest now
  following the head, a wider cone (30-70 deg) behaves like Into the Radius 2's standing body. The 500 deg/s
  follow-head-while-moving config is also not taken: here the body, not the gaze, is the movement reference.
- The horizontal dead zones correspond to `xr_body_move_radius` (0.10 m) and the head's lead over a blocked capsule
  (`roomscale-native-implementation.md`, item 4).
