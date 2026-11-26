# Animated hit-shape overlap, Cyberpunk 2077 2.5

OpenXR Simulator, PID 20184, a disposable NPC, save locked. A confirmed native hit selected Belly. Its live component/resource and the hit's user-data handle resolved the two capsule vectors: center (0.10, −0.02, 0), radius 0.20, axis (1, 0, 0), half-height 0.075, in the animated hit slot. Measurements below used collinear fixture segments; they are not headset gestures or a mesh intersection.

| Tip offset (m) | Overlap along finite blade (m) | Resistance coefficient |
|---:|---:|---:|
| −0.34 / −0.25 / −0.18 | 0 | 0 |
| −0.12 | 0.05691 | 1.388 |
| −0.08 | 0.09693 | 1.702 |
| 0 | 0.17691 | 2.330 |
| 0.10 | 0.27688 | 3.114 |
| 0.19 | 0.36687 | 3.820 |

The stale receipt returned zero; stationary and backward motion had zero positive edge alignment. A previous two-ray implementation returned up to 0.70 m on the broad character proxy; it is not used for penetration depth. Contact resistance is a gameplay coefficient, not measured tissue properties or controller force.

A separate trial with simulated XR hand movement produced an initial native hit but no continuation request. This trial does not establish sustained cutting or physical coupling. No per-instance skinned geometry or neck hit shape was changed by this checkpoint.

## Subsequent runtime checks

- PID 15052, retail D6 factory, two disposable equal-mass dynamic actors with no collision shapes. Only A received initial velocity. After five steps: free-axis displacement A/B = 0.134999/0 m, locked-axis displacement A/B = 0.067501/0.067501 m. Native axis modes read back as 2 and 0. This proves bilateral solver response in the game scene, **not** a joint on an animated puppet. The fixture flag was removed.
- PID 9244, disposable Valentinos: the authoritative melee receipt identified the target, shape **Neck**, binding **Head**, and two geometry vectors. A new slot bound directly to Neck1 had previously failed live transform lookup; the confirmed shape instead uses an offset in the existing animated Head slot. It is not an alias for all Head hits.
- PID 18784, Body 20, simulator-controlled katana: the initial Chest receipt measured 0.10057 m of overlap. After hand and NPC movement the blade no longer intersected that capsule. No follow-through was charged; HP stayed at 1476.6764 until withdrawal. Positive continuous XR cutting, a real NPC joint and per-instance mesh splitting remain unverified. The trajectory ribbons are presentation only.

## Tests with a target-owned PhysX actor

| Game PID | Observation |
|---|---|
| 15928 | Three entries bound the D6 to body index 6 on three separately verified NPC proxies (70176, 70575 and 70976). The hand actuator and the NPC body were both dynamic. |
| 2256 | The katana produced a second accepted event from simulator hand movement after fixture entry. The target reached zero HP, idle input left the event count at two, and withdrawal released the contact. The sampled Lua state at the second event reported an inactive joint. |
| 4904 | A knife event with the stealth flag changed Normal HP from 613.89996 to 0 and Elite HP from 1841.69995 to 368.339996. The surface diagnostics recorded Head on both actors. PID 9244 above is the separate authoritative Neck result. |
| 17424 | On the second of three fixture positions, a knife bound to body 6 on proxy 71654. The same joint stayed active at both controller strokes and both idle samples until withdrawal. HP remained 1474.61389 and the serial stayed at one. |

The solver contact has a player-wide ID across weapons. Accepted events on the same chain advance the damage serial independently of that ID. While the solver carries the NPC, the input check uses signed controller travel. Receipt ownership, current overlap, damage eligibility and hit-shape selection still run on the game side.

| Subsequent game PID | Observation |
|---|---|
| 17112 | Both the Normal and Elite receipts named the native Neck shape. The render attachment used the Head slot. Logging the slot as the shape explained the earlier diagnostic. |
| 17076 | Two separate knives produced two paid continuations each. In the third placement, the same body-6 joint on proxy 72135 stayed bound through serials 1, 2 and 3. Neither idle window, the backward movement nor the sideways movement changed HP. Withdrawal released the joint. The first placement in that run produced one entry and zero continuations. |
| 18604 | With load applied to both solver bodies, the two knife cases each advanced 1 → 2 → 3. In the second case HP after entry and the two strokes was 1473.00 → 1425.10 → 1376.53. The same joint was active during the two strokes and the two direction controls. No part notification occurred. The two longer-prop placements lost contact after entry, including one PhysX break flag during an animation reaction. |

## Installed build

The game releases the D6 on tracking loss, excessive separation, withdrawal, a changed actor, or an expired request. The gameplay coefficient damps relative velocity along the solver's sliding axis. The equal and opposite momentum changes use both actors' solver masses. The two transverse axes stay locked.

- PID 16500, Body 20, knife. The first hand case held the joint from serial 1 through serials 2 and 3 on one living NPC. HP and serial were stable during idle, backward and lateral input. Withdrawal cleared the joint. The other knife placement lost contact. In this run the separate stealth fixtures were already in combat before the request.
- PID 27632, Body 20, katana. A distinct test started with the disposable collider near the middle of the visible prop and sampled the solved item's edge after entry. The first case bound to body 6 on proxy 70142. The first controller stroke advanced serial 1 → 2, HP fell from 1452.31 to 0, and the stock part notification was recorded for the confirmed Belly hit. The joint was active on both samples. Idle input produced no further events and withdrawal cleared it. Neither of the other two placements produced a continuation.

The fixture issued the initial contact. Only the simulated XR hand moved for the continuation and control samples. These tests used the OpenXR Simulator. [Sampled events and solver measurements](blade-runtime.json).
