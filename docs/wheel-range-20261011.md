# Car steering travel — 2026-10-11

The user requested twice the hand travel to full steering lock, then clarified
that the cabin wheel animation also needs twice its native range. Road-wheel
angle and vehicle handling must keep their native scale.

Car input now reaches full lock at **180 degrees per side** instead of 90.
`WheelRange.hpp` provides one range/default policy for the solver, prediction,
INI load/save and headset slider. The physical angle remains unscaled and
unclamped in the accumulator, preserving neutral after overtravel and handoff.
Native player-state refresh classifies the mounted `vehicleCarBaseObject` under
a strong handle; motorcycles and other mounts keep the old range/fallback.
The installed INI is `xr_wheel_steer_max_deg=180.0`.

Cabin animation is a separate archive edit: `mulFactor` changes from 1 to 2 only
on `animAnimNode_FloatTrackDirectConnConstraint` nodes targeting
`steering_wheel_turn`, reading `steer_turn_rate`, on channel `RotY`. The shared
`base/vehicles/common/gameplay/default_car_anim_graph.animgraph` is essential:
the Porsche's `deformation_rig` uses it, while its `vehicle_rig` uses a base-rig
graph for suspension/road wheels. Altering only the car-specific legacy graph
does not affect this active rig.

The archive contains that common graph and 39 road-car-specific graph variants.
Bikes, aircraft, tanks, trains and yacht-specific graphs were excluded. Every
other graph property is verified unchanged after binary serialization, including
road-wheel and suspension transforms. Native physics drive-model limits and
handling settings are not modified by this change. The preceding 593 archive
resources (including firearm FX) are byte-identical in the final 633-entry archive.

Reproduction: `tools/driving_tests/build_wheel_graphs.py` takes the original
WolvenKit JSON exports in `vehicle-graphs-json` and binary originals in
`vehicle-graphs`. `prepare` writes patched JSON and the manifest; after WolvenKit
JSON-to-CR2W and a fresh CR2W-to-JSON export, `verify` checks exactly the listed
scalar edits. The 40 built resources are under `mods/archive/build`, and the
manifest is `tools/driving_tests/wheel-graphs-manifest.json`.

## Verification

- Release DLL build passed; all 21 driving CTests passed, including extended
  travel, +/-230-degree excursions, one/two-hand handoff, neutral return,
  prediction beyond 120 degrees, legacy bike input and stale-camera ownership.
- Live OpenXR simulator session used one stationary Porsche and both grips.
  Input at 90 degrees was 0.495798; at 180 it was 1.0. Both grips remained owned
  throughout (control mask 7). Return to center produced zero output with less
  than 0.00005 degrees accumulated residual. Prediction retained its existing
  half-degree maximum lead and the user's saved setting.
- The actual loaded common graph was changed through Codeware's resource API
  for a same-session A/B: at full native stick input, cabin-wheel slot rotation
  was **64.99935 degrees** with gain 1 and **129.99952 degrees** with gain 2.
  The two-hand sweep observed +/-129.2 degrees after 0.5 seconds of settling;
  held-hand positions and screenshots were inspected. Native damping remains.
- The identical scalar edit was then serialized into the final archive, decoded
  and verified before installation. The final packed archive was not tested in
  another restart; its contents match the live-verified resource change.

Evidence: `build/wheel-range-20261011/`, especially `axis-compare.log`,
`axis-gain-1.json`, `axis-gain-2.json`, `live-results.json`, `turns-final.log`,
`tests.log`, `wheel-graphs-manifest.json` and the captured PNGs.

Early scripted setup attempts failed on CET struct/class aliases and native
road summoning; a moved roof test point also let the player fall. The final
session stayed open for individual commands. A nine-point static trace verified
flat ground around (-726.28656, 845.5736, 32.512), with 1.7 cm height variation;
the car settled before the player was mounted. The player was returned to the
original safe roof position, the temporary car was deleted, simulator poses
and settings were restored, and the game was closed after verification.

## 2026-11-05: one to one, no play

Players felt a large play after the doubled cabin animation. Three things made it, all measured live (Porsche 911 and
Thorton Mackinaw, OpenXR simulator, the cabin wheel read off the car's `steering_wheel` slot in the car's frame):

1. **Range.** The hands reached full lock after 180 degrees while the cabin wheel turns 130 (65 native x 2).
2. **Curve.** The common graph turns the cabin wheel by a curve of the turn input, not in proportion: at the game's
   post-deadzone input n it stands at **89.9 n + 40.1 n^3 degrees** (both cars, within 0.2 degrees at every tenth;
   0.5 -> 50, 1.0 -> 130). With the 180-degree range a 90-degree turn of the hands put the wheel at 50.
3. **Lag.** The chain feeding `steer_turn_rate` is DampFloat (300 units/s) then CriticalSpringDamp (smoothTime 0.1 s).
   The hands on the wheel are handed to that animation, so they trailed the controllers by ~0.1 s -- about 30 degrees
   at a brisk 300 deg/s turn. Step to full input: 40 degrees after 0.11 s, 95 after 0.22 s, full after ~0.4 s.

Fixes. A car's full lock is the cabin wheel's own (`CarCabinWheelLockDeg` 130, whatever `xr_wheel_steer_max_deg` says:
that setting applied to bikes until 2026-11-09, see "Motorcycle handlebars"; it now only serves non-car, non-bike mounts, 30..120, else 90); the hands' share of it is turned into the input that stands the
cabin wheel at the hands' angle (`CarInputForCabinAngle`, the inverse of the curve, WheelRange.hpp); the deadzone
defaults to 0 (a saved 1.5, the old default, reads as 0). In `default_car_anim_graph.animgraph` both `steer_turn_rate`
chains get smoothTime 0.01 and DampFloat 3000 (`tools/driving_tests/wheel_feel_graph.py`; the rebuilt resource differs
from the shipped one in exactly those six fields, and the repacked archive holds the same 1724 files with only that one
changed). The car-specific graphs' chains do not drive the cabin wheel (both cars follow the common curve) and stay.

Verified 2026-11-05 with both hands gripping through the simulator: hands 5/10/20/30/45/65/90/110/130/-30/-90 degrees ->
cabin wheel 5.0/10.0/20.0/30.0/45.0/65.0/90.0/110.0/130.0/-30.0/-90.0. The stick step now reads the final angle at the
first sample (it read 2.6 degrees there before), steady with no oscillation. Road-wheel lock and the drive model are
unchanged (hands 90 -> road wheels 30.8 degrees, full lock 50 on the Porsche).

## Motorcycle handlebars (2026-11-09)

User's decision: a bike is steered by pushing and pulling the bars, not by turning a wheel. `vehicleBikeBaseObject`
mounts now measure the same signed turn in the physical right/forward plane instead of right/up
(`TrackedWheelSteering::Update`, `handlebar`). Right hand forward with left hand back steers left.

- Two hands use the line between them, so its yaw does not depend on which way the room faces and moving both hands
  together does not steer. One hand uses a pivot half the animated span to its side, which assumes recenter-space
  forward roughly matches the bike's.
- Full lock is the new `xr_bike_bar_lock_deg` (15..60, default 25), shown as the overlay's "Bike full lock at" slider.
  The old 30..120 range could not work: 90 degrees of yaw is out of reach by pushing and pulling. 25 degrees is
  computed from hands 50 cm apart, about 12 cm forward and 12 cm back; it has not been tuned in game yet.
- The grab, the arm handover and the horn are unchanged: the arm goes to the bike's animation, as before.
- Body lean (user's request, same day): the game has no sideways-lean input, the bike's lean follows its steering, so
  leaning adds degrees to the arms' turn while the bars are held. `xr_bike_body_lean` (0..1, default 0.5, 0 off) is the
  share of full lock at a 15 cm lean, with 3 cm deadzone; centre is the body position at the grab. It is measured along
  recenter-space right at an estimated neck point (10 cm below, 10 cm behind the eyes), because the headset itself swings
  about 9 cm sideways when looking 60 degrees aside. These values are estimates and have not been tuned in game.
- Unit test: `driving_handlebar` (`tools/driving_tests/main.cpp`). In-game feel is not verified by it.
