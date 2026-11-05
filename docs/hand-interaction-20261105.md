# Hand interaction: what was built, measured and ruled out (game 2.5, 2026-11)

Notes for whoever works on this next. Tested with Valve Index controllers on SteamVR, RTX 4080, no ray tracing, at 90 Hz
alternate eye. Nothing here has been tried on other headsets or by other players. Addresses are RVAs of game 2.5, read
from the exe with `dumpbin /disasm /range` and a small string and lea finder; Ghidra through REA timed out on the 66 MB exe.

Words used below: **measured** came from a log or a dump, **confirmed** was seen in game by the tester, **inferred** was
read from the code or reasoned and not proven at run time, **open** is not known.

## Where things are

| What | Where |
| --- | --- |
| Touch, carry, gun, people (Lua) | `mods/cet/CyberpunkVRPort_HandCollision/init.lua` |
| Loot, food and drink, gravity gloves | `mods/cet/CyberpunkVRPort_Reach/modules/world.lua` |
| Bodies, traces, cones, corpses | `src/Natives/HandBody.cpp` |
| PhysX access, collision view, gun sweeps | `src/Natives/DebugView.cpp` |
| Carried thing's picture on the hand | `src/Natives/HeldPicture.cpp` |
| A living person's limb, head, chest | `src/Anim/NpcArmGrip.cpp` |
| Finger limits and bends | `src/Anim/HandGestures.cpp` |
| Hand capsules in the player templates | `tools/ent/add_hand_physics.py` |
| Gun shapes from the meshes | `tools/emit_weapon_shapes.py`, generating `weapon_shapes.lua` |
| Offline tests | `tools/hand_tests/` |

`init.lua` sits at Lua's limit of 200 locals in its wrapper function: new code goes into the table `K` as `K.fn`.
`tools/emit_*.py` are matched by `.gitignore`, so they need `git add -f`.

## Logs

In `red4ext/plugins/cyberpunkvrport/`:

- `log/cyberpunkvrport.log`: `[lua] onUpdate` (ticks, p50, p99, max per 30 s), `[gunshape]`, `[picture]`, `[proxy]`, `[npcgrip]`, `[corpse]`, `[pick]`, `[debugview]`
- `lua/CyberpunkVRPort_HandCollision/CyberpunkVRPort_HandCollision.log`: `[HandColl]`, `[cost]` (per section every 10 s, and every frame over 40 ms)
- `log/lua/scripting.log`: `[VRReach]`

Minidumps are in `%LOCALAPPDATA%\REDEngine\ReportQueue`. A null read at Exe+0x1c2f8f (+0x250) appears in dumps back to before
the hand work, right after the plugin unloads at quit: not caused by it.

`onUpdate` runs twice a frame, 2 ms and 21 ms apart (measured). 44-45 ticks a second is the cap at 90 Hz alternate eye, not a slowdown.

## Decisions that came from a failure

**The hand's filter.** The capsules' own filter is type Dynamic, colliding with Dynamic and Debris, answering no query.
"World Dynamic" made outstretched arms shake (the rays found the capsule), broke props and shoved cars. Type Debris passed
through most small props. See the docstring of `add_hand_physics.py`. Bodies named `VRPortBody_` get grabbed by the basketball mod.

**Carrying.** Tried in order, and rejected:

| Tried | Result |
| --- | --- |
| A body put in the hand each frame (SetPos) | Took effect briefly, then the thing stayed in the world: PhysX sleep, inferred |
| Wake impulses each frame | Floated and spun |
| Impulses bounded by read-back speed | Jitter and flinging; the speed read is a step old, inferred |
| Writing speed each frame, kept | Mean error 1-2 cm at slow hand speed, about a frame behind over 3 m/s (mean 12 cm, most 28 cm), measured |
| Picture override on top, kept | Confirmed nearly perfect |

Direct PhysX would save one step at best, so it was not built: over 300 samples the game's position equals PhysX's, and a told speed shows on the next call.

The picture override is a detour on `sub_1405C3454` (`worldInstancedDestructibleMeshNodeInstance` update) plus a re-send at the frame submit, from the existing hook on `sub_1403D6D84`. The body stays real for collisions while the mesh is drawn on the hand. Script `SetLinearVelocity` and `SetTransform` (0x142998948, 0x142998C1C) only queue into the proxy's command buffer (`sub_140613320`, at most 4 per batch).

Other things that mattered:

- Spin is capped at 7 rad/s until raised per body (`RaiseSpin`, PhysX slot 60). Measured up to 35 afterwards.
- A carried body has to stop being queryable, or the player cannot walk forward with it (`VRBodySetQueryable`).
- Speed is taken from the target itself. Hand speed is relative to the player, so walking had no feed-forward.
- Crushing by grip force was removed: a normal grip reaches force 0.7-1.0.
- A re-grip within 3 cm stays where it closed. 15 cm left things floating in front of the hand.

**The gun.** 943 firearm meshes have one physics box between them, and 1003 firearm entities have no collider, so there
was nothing to reuse: `tools/emit_weapon_shapes.py` fits about 3 boxes per family from the mesh vertices. They are swept by
`PxGeometryQuery::sweep` from the last free position, sliding up to 3 times.

Sweeps alone are not enough. The static near list holds 13-17 shapes where the view finds 42-49 within 2 m, and rays found
the gun past a surface over 1600 times in one session (measured). So `K.gunWalls` adds 2 dynamic and 2 static rays a call
from the drawn wrist, and a hit that is not loose becomes a plane for 0.25 s. This is what made the gun confirmed good. The cause of
the missing shapes in the near list is open: `most of an actor 2` ruled out the 64-shape cap.

A ray starting inside a collider answers with the normal pointing back along the ray, so gun rays start at the drawn wrist. Updating the gun's loose collision every other turn made it worse: `onUpdate` runs twice a frame, so that was one phase only.

**People.** There is no switch for one limb of a ragdoll: the `Animated` activation shows bodies and the person carries on
(measured). So the part is solved in the pose hook, identified by track buffer, with hips staying put. Most NPCs ignore being held, one fought it. A Bump stimulus is sent as a touch; living NPCs show no reaction to it (open).

The hook sees a person's pose 4 times a frame: every grip's pass count was a multiple of 4, about 165 a second (measured). The head reached 180 degrees with a limit of 46 in its first form, which is 4 x 46. But `SolveOnce`, which skips a pass finding the bones as the last one left them, skipped only 0-4 of about 1000 passes on heads (582 of 1144 on a leg). So those passes are not simply stale, and **why they differ is open**. What holds the head is the clamp against the chest, which does not depend on the pass count. Confirmed better; a head now gives only about 11 cm, so head and chest grips tear at 25 cm rather than 20.

A bend chain takes its axis from where the tip has to go. Taking it from the direction to the target turned the trunk right round when a head was brought below the neck.

**Dead bodies.** Forced ragdoll on grip works. Limbs in Lua cost about 60 script calls a frame (6 ms), hence `VRCorpseHold`
in C++. Sleep is `entRagdollPutToSleepEvent`, and limbs read 0.000 m/s after it. Capsules visible on corpses in the collision view are the game's ragdoll actors, left in place.

**Food.** Standing decor cans are props, not items, and cannot be eaten. Real ones go through `ItemActionsHelper`. A hip zone
that reached a low table pocketed food half a second after taking it, so it now needs the hand to have been away first.

**Marking things.** Two ways, with different reach:

- `VRThingHighlight` calls the render scene's slot 0x2D8 (`sub_1407A0190(scene, &obj+8, &obj+0x10, on, mask, opacity, 0)`; mask 1 outline, 2 fill, index<<2, 0x40 through walls). Passing a null second handle crashes at Exe+0x67aaff. It works, but on a world node it lights every copy of that node, and it cost 35-62 ms per change (measured). Not in use.
- `entRenderHighlightEvent` queued on an entity is per entity and is what Reach uses now. Fields: fill +0x40, outline +0x41, through walls +0x42, opacity +0x50. Indices 5 and 6 are unverified guesses at colours.

**Gravity gloves** are limited to dropped loot, food and drink (`t.far`, 6 m, 16 degree cone). Loose props are switched off by `K.GLOVES_LOOSE = false`, because of the highlight above and `VRPickCone` (up to 47 ms). With both gone, `onUpdate` p99 fell from 20-30 ms to about 6 ms (measured). Pulling loot other than food from afar has not been seen in a log yet (open).

## PhysX

The game ships PhysX 3.4 but older than the public headers. The tables were read from the DLLs and are checked at run time where the code says so.

- Scene: vtable at RVA 0x20CAF0 has six more virtuals than the headers. `getGravity` 67, raycast 94 (+0x2F0), overlap 96, slots 26/27 aggregates, aggregate `getActors` 11. Overlap returned 0 hits everywhere (open), so `DebugView` walks actors and aggregates by bounds.
- Shape: vtable RVA 0x20E878 starts at index 38 and has one extra virtual. `getGeometryType` 7, `getBox` 10, `getSphere` 11, `getCapsule` 12, `getConvex` 14, `getTriangles` 15, `getActor` 17, **`setLocalPose` 18**, `getLocalPose` 19, `getSimFilter` 21, `getQueryFilter` 23, `getFlags` 34. Slot 18 was once called as `getLocalPose`, which wrote a zero pose over live collision shapes and dropped the player through the world.
- `PxRigidDynamic`: base RVA 0x20B2E8. `setGlobalPose` 21, `setKinematicTarget` 54, `setMaxAngularVelocity` 60, `getMax` 61.
- Actor: `+0x10` proxy id, `+0x14` body index (`userData` owner). PhysX world is game world minus the vector at `+0x134` of the scene object, whose `+0x18` is the PxScene: (-2048, -2048, 0) measured.
- Trace: `physicsTraceResult` +0x48 proxy, +0x4C body, +0x50 shape, +0x54 distance, +0x59 kind (from byte 0x34 of the proxy). These hidden bytes survive CET's copy (measured). `entPhysicalBodyInterface` reads +0x40 proxy and +0x44 body, so an instance filled from a hit is that body.
- Kinds met: 10 aggregate (most props; owner byte 6 inferred `InstancedDestructibleNode`, a world node with no entity), 7 baked world collision (mass 0, refused), 1 and 3, 12 ragdoll.
- Exactly 1 kg of kind 1 is the hand's own capsule: refused in `VRNearBody`, or it gets picked up.

Do not read guessed pointers under `__try` in this process: it caught the same read once and then did not.

## Controllers (Index, SteamVR)

`XR_EXT_hand_tracking` gives all five fingers (data source 2). A saved SteamVR binding for the game overrides the generated default, so any action added later stays unbound until entered in the binding screen: that is why grip force read 0.

## Licences

HIGGS and PLANCK (Skyrim VR) are GPLv3 and the port is MIT, so only their methods were looked at, no code.

## Open

- Why NPC pose passes differ, see above.
- Overlap returning no hits, and what the gun's near list misses.
- Static small props (kind 7) cannot move: they are compiled actors in a DataBuffer with visuals in separate nodes and no link. WolvenKit 9.1 cannot serialize 2.5 sectors. A replacement would need a runtime match of collider to mesh. Set aside.
- Outline colours and whether drops show the outline in every case.
- Fingers jumped on some surfaces; bend rays and easing were added after that report, and it was not asked again.
- Slight DLSS ghosting on a carried thing; not compared with the picture off.
