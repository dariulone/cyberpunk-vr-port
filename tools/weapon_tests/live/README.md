# Attach-only weapon audit

Run only after the root grants an exclusive live lease and checks the current
location has a clear upward firing direction. The root owns game launch/exit.

```
py -3.13 tools/weapon_tests/live/run.py --pid NEW_PID --out scratch/goal-audit-20261117/weapons-repeat --root-exclusive-lease --expected-inventory 747 --expected-money 806112 --case same --case different
```

The harness never launches, terminates, restarts, loads, or requests a manual save. It
uses the existing CVRBridge and OpenXR simulator. It creates no NPCs. Controller
poses are literal constants; it does not generate numerical pose data.

Automatic saves must also be blocked. The initial observer test did not observe
an ACK using `NameToString`; a new dynamic CName can stringify to an empty name,
so that test does not establish whether the callback was dispatched. Retain token
names as plain strings and compare CName values by hash equality if an observer is
used. Root instead proved its unique session guard by
causal REMOVE → engine/manager false, then ADD → both true on PID 20040. Before
any fixture mutation the harness requires `GoalSaveGuard.verified=true`, the
`causal-remove-readd` verification kind, the same valid root lock name, and current
authoritative `Game.IsSavingLocked()==true`. A bare true engine check or an
unverified/stale token is refused; standalone use without this root proof is refused.

The harness additionally queues its own unique `cpvr_weapon_audit_<uuid>` ADD.
After restoration is verified twice across a drain interval, it invalidates
autosave requests, queues REMOVE for **only its own reason**, allows processing,
checks that the verified root protection remains, and invalidates autosaves again.
Its own requests are reported as queued, never falsely described as acknowledged.

The optical case uses the existing `render_audit_math` C++ tool's `optics` mode.
Build `build/render-runtime-audit` before attaching. Its Lua adapter exports the
actual camera position/axes, measured MAIN-eye offset/IPD, weapon position/quaternion,
and the current authored scope offset. It makes no synthetic alignment claim.

`--case optics --scope-yaw 0` tries a bounded fixed yaw list. A native static-only
ray must hit world geometry 20–150 m away; sky/no-hit and a close wall are refused.
The wide scene screenshot still needs visual review for a clear contrasting target.
Both controllers use legacy head-relative orientation, avoiding the sideways
body-quaternion pose from the previous run. Five literal yaw/pitch candidates
calibrate Osprey's authored grip offset using measured optical-axis angle. At most
two C++-computed translation corrections follow (12cm maximum each);
an eligible pose requires bore/camera angle <=2 degrees, an actual eye within 15 mm
of the scope axis, and rendered eye relief of 6–30 cm. Missing measurements fail;
there is no fallback to the first pose. This fixture currently requires the reviewed
right-handed baseline. It never fires or spawns a target.

The selected pose remains fixed for hip→ADS→hip L/R/SBS captures and a short video.
Axis/relief and static target depth are checked again in all three states. Those
checks establish fixture validity only: readable reticle, target image, glass and
caps must still be inspected in the actual images before claiming visual PASS.
C++ mode self-tests, Python/Lua syntax checks and `test_optics_bridge.py` cover the
geometry and real Lua array-call serialization offline. The fixture ran in CET on
PID 26944: ypr=(11,-6,0) gave bore error 0.082 degrees, eye-axis distance 11.53 mm,
relief 20.35 cm and static target distance 33.72 m. Those original gates used a
weapon-root bind-space point and did NOT prove alignment with the animated screen.
The corrected fixture requires the live vrp_scope_hinge transform and C++-derived
inverse-bind frame. Its native slot query and populated atlas were confirmed on
PID26944, but that session did not enter native ADS6 despite rawLT input; the report
was correctly failed. Actual hip→ADS6→hip remains pending a fresh session. The earlier
fixture did not produce a readable reticle in the inspected images;
aligned glass on/off/on did not alter that result. Visual reticle quality remains
unresolved, separate from the passing physical glass/cap visibility behavior.
Any restoration error retains the lock request and original snapshot. Root's
distinct session lock remains until ExitGame. No persistent save setting changes.

The next root-owned launch explicitly uses AutoSave-13 (747 inventory items,
806112 money). Earlier runs restored their in-memory starting state, but did not
prevent automatic saves; that is not proof of disk-save safety. Root archived the
test-generated autosaves and backed up the save directory. This harness never
modifies or removes save files.

It snapshots all six weapon slots (not only slots 0–2), exact ItemIDs and inventory
quantities, active/selected weapon, active gadget, magazine counts, Double Trouble
rank/unlocked state and primary perk points, `player_fired_a_weapon`, Triple Tequila if present,
FPP camera local offset, holster preferences, simulator head/controllers/body, and
the exact bytes of three touched configuration files. Temporary items are tracked
by exact created ItemIDs and the `cpvr_weapon_audit_20261117` tag where the engine
retains it (weapon archetypes can discard GiveItem's tags). Cleanup refuses an original inventory ItemID.
Force-buy does not spend perk points; force-sell returns points, so cleanup adjusts
only the unspent delta with AddDevelopmentPoints, preserving the spent count.

Cases:

- Two exact Nue instances of one record, then Nue + Overture: two full squeezes
  per hand within 390 ms, both released, observe native ShootState 4/5/6 and
  ValentinosFrenzySE. Sample exported native diagnostics every 5 ms and require
  actual-instance route 4 and additional automatic rays for both hands after
  release + 100 ms. First verify that ordinary trigger holding causes one shot.
  No synthetic effect is applied to trigger the ability. Short stereo video is
  recorded for each pair. A Hurricane-only repeat creates only its three guns.
- Empty the new gun through the production reload ammo API, establish real glow,
  holster through the native equipment request, and record the first empty-slot
  observation versus the first actual disabled glow component. Both-eye pictures
  and per-sample tick/time evidence remain available.
- Osprey hip/ADS: verify UpperBody=6 during ADS, physical glass remains enabled,
  and the three native cap/decal-cap components remain disabled. Save both eyes
  in each state. The default fixed 60-degree view faces the distant city target
  instead of the nearby white column. The pictures still require visual inspection
  of lens/reticle; component enabled flags alone are insufficient.
- Hip holster and frag grenade: native gadget assignment, three fixed camera
  positions (front, close hip, side), both-eye screenshots plus holster/belt state.
  These views validate the active body and right/left handed setting only; they
  do not prove every body/garment combination.

An exception runs restoration and leaves the game running. A bridge timeout
leaves its pending request intact; never delete that request or restart the game
to clear a timeout. The Lua `WeaponAudit.original` remains until restoration is
verified. Inspect `report.json`, `cleanup.json`, `restored.json` and the original
snapshot; restore any reported mismatch before returning the lease.

The first authorized run on PID 6676 verified glow, Osprey component states and
restoration. It caught a real native Hurricane conflict: the ability activated but
the old semi-auto wrapper suppressed every automatic shot. The production guard
was corrected and compiled; full Hurricane after-fix live verification awaits the
next root-owned restart. See docs/goal-weapons-belt-20261117.md for evidence and
remaining visual limitations. A failure is evidence to fix the harness or
implementation, not permission to force a successful result.
