# BONELAB: roomscale, tracking and the two-handed grip (2026-10-30)

What BONELAB (Stress Level Zero, Unity IL2CPP build, `D:\Games\BONELAB`) does for roomscale and for holding a
weapon in two hands, read from its own code, and what the port takes from it.

## How it was read

No third-party dumper was run. `tools/il2cpp_study/il2cpp_names.py` reads `global-metadata.dat` (version 29) and
`GameAssembly.dll` directly: every type, method (with parameter names) and field of the 142 images, the method
addresses from each image's `Il2CppCodeGenModule` (the data record whose first pointer is the image name; 139 of
142 found) and the field offsets from `Il2CppMetadataRegistration.fieldOffsets` (the record with two counts equal to
the number of type definitions). 136 329 methods, 98 342 fields. `ida_name_methods.py` names them in an IDA
database without auto-analysing the 76 MB binary; `ida_decompile_set.py` decompiles a list of them. The outputs
(names/, idb/, decompiled/) stay outside the repository.

The rig is `SLZ.Marrow`: `OpenControllerRig` (tracked devices), `RemapRig` (locomotion, crouch, jump),
`PhysicsRig` / `PhysTorso` / `PhysHand` (the physical body), `Hand` / `Grip` / `TargetGrip` / `CylinderGrip` /
`HandJointConfiguration` (holding).

## Tracking: a device that loses and regains tracking does not jump

`OpenControllerRig.OnEarlyUpdate` runs each tracked device (headset, both controllers) through
`ProcessTrackedBlend(output, target, tracked, state, offset, blend, velocity, time)`, a four-state machine kept per
device (`_contLfState`, `_contLfOffset`, `_contLfBlend`, `_contLfVelocity`, `_contLfTime`, and the same for the right
hand and the headset):

- tracked: the pose is used as it is;
- lost (the input's rotation has no length, `|q|^2 <= 0.1`): the controller is put at its last pose relative to the
  headset (`_lastContLfLocal` under the head), and that pose is snapshotted (`SnapshotOffset`);
- back: the difference between the held pose and the tracked one is kept as an offset, and `BlendOffset` publishes
  `tracked + offset * blend` (rotation `tracked * Lerp(identity, offsetRot, blend)`), `blend` taken to 0 with
  `Mathf.SmoothDamp(blend, 0, ref velocity, time, +inf, deltaTime)`; under 0.01 the device is tracked again.

So the hand follows the controller from the first frame back, and the jump is spread over a few tenths of a
second. **Taken**: `include/Runtimes/TrackingReacquire.hpp` does the same for the port's hands (head-local, as
published; SmoothDamp, 0.12 s), in `OpenXRManager::FlushHandsToShared` after the raw melee sample and before the
smoothing filter. Two additions: a gap under 0.1 s is no loss (a frame the runtime could not locate must not make a
fast swing lag), and a reacquired pose within 2 mm / 0.6 degrees is published as it is. Tests:
`tools/roomscale_tests/tracking_reacquire.cpp`. The headset is not blended: the port never moves the view off the
runtime's pose.

`OpenControllerRig.KnollFilter(transform, trackedPos, trackedRot, maxMillimeters, maxDegreesSwing)` is a jitter
filter for the body trackers: the fraction the output follows by is `clamp((distance / maxMm)^2, 0, 1)` (and the
same for the angle), so tremor well under the threshold is held still while larger motion passes through with no
lag. It is not used on the controllers. Noted for the port's own hand filter (`Runtimes/OpenXRManager.cpp`, the
UEVR-style exponential follow), which lags all motion alike; not changed now -- the hand rate is tuned to the
weapons and the melee speed measure.

## Roomscale

The player's body is a physical rig: `PhysicsRig` has a pelvis, a knee and a "football" (`_football`, the
locomotion ball) on joints, and steps (`CheckStep`, `HoldStep`, `StepFeet`), climbs (`CheckClimb`) and crouches
(`SetKneePelvisLimit`, `RemapRig.CrouchHold`) by driving those. Real movement in the room reaches it through
`RemapRig`, whose fields keep the tracked, locomotion and physics parts of the motion apart (`_trackedDebt`,
`_locoDebt`, `_physDebt`, `_trackedApplied`, `_locoApplied`, `_physApplied`, `_artificialDelta`,
`_lastTrackedHead`, `_trackedHeadDelta`) -- none of which the update functions decompiled here read; the
working path is the physical body following the head with forces, the camera staying on the headset. The port's
roomscale (`Runtimes/RoomscaleMovement.hpp`: a movement ledger by tracking origin and sample, a free zone the body
follows out of with a critically damped response, the character controller doing the collision) is the same split
for an engine whose body is animated, not simulated; nothing there to take over.

## Two hands on one weapon

BONELAB holds everything with physics. A hand attaches to a grip with a `ConfigurableJoint` configured by
`HandJointConfiguration` (drives, limits, `massScale` / `connectedMassScale`). `PhysHand.RefreshHandStates` gives
each hand a state: 0 empty, 1 holding a dynamic body, **2 both hands on the same body**, 3 holding something static
or kinematic; the arm's drives and the order the solver resolves the two arms in (`FixedUpdateArm(...,
secondToSolveAndStatic, ...)`) follow from it, and the muscle model (`PrepShoulderMuscle`, `MuscleClamp`,
`MuscleMax`) caps the force, so a heavy weapon lags and sags. The aim of a two-handed weapon is whatever the
physics solve of two joints on one body gives: the barrel along the line between the two grips, the roll from the
primary hand's rotation drive.

The port's weapon is animated, so the solve cannot be had for free; `Anim/TwoHandGrip.cpp` gives the same result
kinematically (the barrel turned by how the support hand moves against the primary one once the grip closes, the
roll left to the primary hand), which is what the two joints converge to. What the port does not have is the
weight -- a heavy weapon held in one hand sagging and lagging -- which in BONELAB comes from the muscle cap. That
would be a new feature (per-weapon mass, a lag on the aim when held one-handed), not a fix; left for a decision.
