# Independent review: Osprey instance-qualified hinge rotation

Source-only review,2026-11-17. No game access, deployment or production build was performed by the reviewer. The review covers new `Anim/OpticHinge.hpp/.cpp`, their WeaponRig/AnimPose call sites, native registration and the Lua hinge request lifecycle. Earlier aperture-frame and visibility work has its separate audit.

**Final corrected source: PASS, no remaining critical ownership/lifetime/race blocker found.** Actual native binding acceptance, application counters and hip→ADS6→hip visual results remain live acceptance gates.

## Native stage and ownership evidence

Existing2.5 exports under `scratch/goal-20261115/npc-pose-order/decompiled/` establish the relevant stage:

- AnimatedObject update1B3E00 saves `object+0x18` as descriptor R13. Its graph context at stack var_560 receives R13 at var_528, i.e. context+0x38/index7, before calling1B5AF4.
- Pose copy1B3C08 reads `a2[7]`, its bone buffer at descriptor[0] and tracks at descriptor[3], then copies48-byte parent-local transforms through the current source→destination remap. Translation is+0, rotation+16, scale+32.
- The final582B3C local-to-model conversion receives that same instance descriptor; the verified return site is1B427D. Existing nested TLS ObjectPoseScope is restored by SEH finally. Adding the object pointer to this existing scope identifies the instance without calling the script VM from an animation worker.
- MetaRig is distinct from the source Rig resource: MetaRig boneNames+0x20/referenceTracks+0x40, Rig boneNames+0x50/referenceTracks+0xF8. Shared reference tracks cannot identify a weapon instance.
- The packed Osprey appearance explicitly binds animated `Barrel.controlBinding` to `Receiver`, enabled. Receiver's further controller binding is to a non-AnimatedComponent; the approved resolution stops at the exact Receiver of the same weapon, rather than following arbitrary component names or asset-wide fallbacks.

## Findings resolved before signoff

The first draft required Barrel itself to be the AnimationSystem bucket driver and required a13-bone MetaRig with hinge at destination8. That could reject the real control-bound/merged receiver pose, although it failed closed. The corrected code retains the exact Barrel source resource, binds its enabled exact controlBinding to the same weapon's Receiver, and permits a bounded merged MetaRig. The source hinge8 is translated through the **current native remap**, with the actual destination name checked as `scope_hinge`. No LOD0 fallback or guessed destination index remains.

The first draft tested WeakHandle expiry without retaining the objects during native pointer reads. The corrected wrapper copies its bounded weak snapshot under RigPoseStateLock, promotes player/weapon/Barrel/Receiver/control outside that lock, then re-locks and rechecks current enabled state, identity, freshness, player/context and legacy conflict. Native pointer traversal/write is isolated in a no-RAII SEH helper. The lock is released before strong handles are destroyed. A completed clear/holster cannot slip a stale snapshot through that second locked check.

## Final checks

- Requests validate the actual ScriptedPuppet.GetActiveWeapon for the current player. Its static one-argument/wref return contract was checked against the script and SDK ExecuteFunction overload.
- Animation workers use the existing acquire-loaded `roomscale::PlayerIdentity()`; they never call FindPlayerEntity or script VM lookup. The older OrientationProvider raw player cache was correctly avoided.
- Binding requires exact weapon/entity/component/control identities, same runtime scene, matching live bucket membership, exact Osprey Rig path and13-bone source signature. Weak controls prevent recycled engine objects from becoming a valid old registration.
- Worker gates require exact TLS AnimatedObject/descriptor, current instance trackB, source Rig pointer, current MetaRig pointer/ID and actual graph context `a2[5]`. Rebuild/rebind/foreign instances are rejected. Fast atomic object/descriptor checks are only filters; all authoritative state is rechecked under the lock.
- The rotation is authored hip/reference `q=(0,0,1,0)` in source parent-local space. Reference orientation accepts its quaternion sign-equivalent form. The code writes exactly16bytes at `48*currentDestination+16`. Parents0/1, translation, scale, other bones, SharedReloadSlots and generic VRRigWrite slots are untouched.
- Missing/duplicate hinge remaps, invalid destination bounds/name and a conflicting existing writer for(1,8) skip the write. Disable leaves a fresh native copy untouched rather than restoring stale cached transforms. Request expiry is200ms; Lua also clears on leaving aim/scanner ownership, disable, holster/weapon change, missing player and shutdown.
- Requests and applies share the existing rig-state synchronization, but engine strong handles are released outside it. The worker's native access helper contains SEH, so an invalid engine read cannot bypass the promoted handles' normal C++ destruction.
- One bounded current registration is used; no per-weapon or per-frame growing cache was introduced.

## Tests and limits

Independently rerun successfully:

```powershell
build/render-runtime-audit/Release/optic_hinge_test.exe
py -3.13 tools/weapon_tests/test_optics_bridge.py
py -3.13 tools/weapon_tests/test_optics_hotload.py
```

The production policy test covers another actor sharing the same rig, owner/Barrel/Receiver/control replacement, expired weak ownership, stale/future timestamps, rig/MetaRig/track replacement, old in-flight contextMeta, disabled/device contexts, conflicting legacy writes, missing/duplicate/out-of-bounds remaps, and a merged22-slot destination with source8→destination17. A whole-buffer comparison verifies only the hinge quaternion changed. Bridge/hotload regressions pass; they do not validate CET's new native API invocation or real animation-bucket acceptance.

The native request validates active-weapon ownership on the permitted request thread and refreshes a200ms registration. Live acceptance must check `requests/accepted/applied` counters and actual bone/frame data, not merely the Lua request bit. Confirm actual ADS6, both-eye aperture/reticle alignment, return to native hip/holster behavior, and no same-asset NPC effect before claiming the visual defect fixed.

Reviewed corrected hashes:

- `OpticHinge.hpp`: `44960B9086D4FD081133A3DE41775210C060F2BBD5C435631F99CB3306C794FE`
- `OpticHinge.cpp`: `DCAF75936CABFEEB6FF0D63CFE33E95277A3FF2A241C6D8CAA605B3FE955D61E`
- `WeaponRig.cpp`: `D883EE03B32F7F7E4AF202CBA28DEC66AC7126D56AC8CD2BCAC5C38BD403E84B`
- `AnimPose.cpp`: `E45EC34CDA482D01076A699C772EE4FF0F69BED4D7D3B1C92A1C2A5981966CE5`
- Optics `init.lua`: `95FED6E69088E8C0AC0612239C3927CA00331B5798DDC54C8DC7F6985B4A1BF1`

Source corrections were made by the weapon agent after coordination. Reviewer changed no shared production source.
