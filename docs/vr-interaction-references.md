# VR interaction references (2026-11-17)

These are observations about presentation and contact events, not imported implementations. No reference binaries, artwork or decompiled source are required to build or run the port.

## Recoil presentation

The Quest installation is a Unity IL2CPP build. `classes.dex` is not where the gameplay bodies live. In metadata version 39 the `Recoil` type belongs to `Footlong.dll`; its method tokens resolve through that codegen module into the ARM64 `libil2cpp.so`. Two presentation methods were statically decompiled in isolation:

- `GetRecoilRecenterTarget` (`0x4e1dc60` in this build) reads two separate three-component states and adds a partial contribution to the second target. The call outside the extracted routine was not resolved; this is not a measurement of the response of a particular item.
- `OffsetByRecoilWorldSpace` (`0x4e1fae0`) uses a different path to transform a supplied point around a pose and return a world-space offset. The metadata also distinguishes linear and angular impulses from recovery. A visual pose offset and the time until the next shot should not be the same control.

For CPVR, use the existing per-hand impulse and recovery on the weapon pose; never apply a recoil offset to the tracked head. Tune timing against CP2077's own trigger modes, perks and item records. A subsequent inspection resolved the actual M1911A1 recoil component and its dimensionless preset multipliers, not a peak kick angle. See the [detailed reference study](vr-contact-and-recoil-study-ru.md) for the impulse path, item asset, and the distinction between a transient effect and a skinned material mask. CPVR tuning numbers are game design values, not measured equivalents.

## Surface effects

`ThunderRoad.dll` in the PC installation is managed. ILSpy decompilation of `CollisionInstance`, `EffectModuleDecal` and `EffectDecal` shows separate responsibilities:

- A collision event carries a point, normal, source and target collider groups and material context.
- The visual module can reject a target, borrow an effect from a bounded pool, parent it, and set its contact transform. The effect updates its intensity, fades and returns to the pool; it does not determine damage.

For CPVR, retain the native game's permissions, damage and effect assets. Reuse a validated local contact for feedback, rather than doing a second broad target lookup or treating the velocity of an air swing as proof of a hit. Do not infer that the native game has a persistent cut decal merely because it emitted an impact effect.

## Physical holds in CPVR

The pre-baked prop grasps and the world-climb classifier are different paths. The baked table selects a hand pose for a portable mesh. World holds use nearby PhysX shapes: palm/finger penetration, candidate surface normals, and a finger-wrap hint. Improving the prop SDF does not by itself improve a ledge.

Useful fixtures for world holds: a single box corner, two adjacent shapes forming an outside corner, a narrow rail, a plain wall with a loose object against it (must not anchor the player), and a stale candidate after a scene change. Compare eligibility and anchor stability first; keep pull speed, grip windows and the two-hand locomotion blend constant during that comparison.
