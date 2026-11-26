# Hand physics at 90 Hz on AutoSave-7

PID 464 ran one eight-second motion warmup with physics off and one with physics on, followed by three ABBA blocks in the same disposable OpenXR Simulator session. Each window used the same HMD yaw and two controller reaches. The operator enabled the FPS overlay for the real-frame rings (`xr_framegen_overlay=1`, normally 0 on this installation).

Block 1 failed the predeclared motion checks. Six head keys in its third window missed their 100 ms delivery interval, and paired p95 head delivery skew was 102.2 ms. Blocks 2 and 3 passed with 11.0 ms and 12.6 ms skew. In those eight windows the maximum player displacement from the initial position was 1.6 cm and the median sampled GPU clock was 2925 MHz in each arm.

| Measurement | Physics off | Physics on |
|---|---:|---:|
| CPU / GPU ring samples | 1435 / 1438 | 1439 / 1439 |
| Real FPS | 44.892 | 44.951 |
| Mean real-frame interval, ms | 22.276 | 22.247 |
| P95 real-frame interval, ms | 24.544 | 24.598 |
| Mean GPU frame time, ms | 22.263 | 22.240 |
| Repeated / total XR display cycles | 1443 / 2880 | 1443 / 2881 |
| Before-simulate work, µs per counted step | 9.47 | 95.13 |
| After-fetch work, µs per counted step | 8.26 | 45.65 |
| Finger worker, µs per counted solve | No solves | 37.93 |

Frame statistics are means of the four valid windows per arm. Physics timings divide summed microseconds by summed counts over the retained intervals. Three off intervals and two on intervals crossed a counter reset and were omitted. The enabled worker recorded 1417 solves and 62 counted calls per solve. Its timing is on a separate thread.

The per-block FPS changes with physics on were +0.125 and −0.008. P95 real-frame time changed by −0.219 and +0.328 ms. The two measured physics callbacks together cost about 0.123 ms more per counted step. Both arms ran near half the display rate with roughly half their XR cycles repeating, so this run establishes a callback cost but gives no consistent direction for an FPS change.

The raw file is `build/physical-melee/physics-ab02/hand-physics.json`, SHA-256 `a32fe9025b9ae1a42ff0ec98ad20bcffe9d27a63cfe300f85dc4bf6e18ca6c64`. The plugin SHA-256 was `7bdf05fa34c74327047571e12910fae458dba4a19b3c9e38f567743d4a8409aa`. [Benchmark driver and procedure](../../../tools/vr_bench/README.md#hand-physics-in-an-existing-simulator-session)

## Frame generation and FPS cap disabled

The operator disabled frame generation and the FPS cap for both arms. The same PID, plugin, simulator, save label, player anchor, trace, hand settings hash and game graphics settings hash were recorded. Both ABBA blocks passed. There were 2031 real-frame intervals with physics off and 2020 with it on. Neither arm reported a repeated XR projection cycle.

| Measurement | Physics off | Physics on | On minus off |
|---|---:|---:|---:|
| Real FPS | 63.516 | 63.072 | −0.444 |
| Mean real-frame interval, ms | 15.790 | 15.859 | +0.069 |
| Mean GPU frame time, ms | 15.777 | 15.846 | +0.068 |
| P95 real-frame interval, ms | 18.844 | 18.817 | −0.027 |
| Before-simulate, µs per counted step | 8.71 | 88.07 | +79.37 |
| After-fetch, µs per counted step | 7.34 | 32.63 | +25.29 |
| Finger worker, µs per counted solve | No solves | 34.67 | |

The enabled worker recorded 1975 solves and 122450 collision calls, exactly 62 calls per counted solve. The disabled worker recorded zero solves. The two callbacks added **104.66 µs per counted step**. About three quarters of that difference was before simulation. The finger timer includes the complete worker solve when it issues at least one collision call. It cannot isolate the time inside any one query. Four reset intervals were omitted per arm.

The block FPS differences were −1.913 and +1.024. Both blocks are included. The last off window fell to 57.77 FPS versus 65.10–65.65 in the other three off windows. Real-frame and GPU intervals moved together in both blocks. These eight windows do not establish a repeatable FPS penalty or identify a particular collision loop to optimize.

The raw file is `build/physical-melee/physics-unpaced01/hand-physics.json`, SHA-256 `9b6461551aa67d59b3c99e8d5c009a6074263f40649a45dbe9d41fb53b63dc35`.

## Unpaced storefront contact

**No systematic FPS drop was observed under the measured contact load.** PID 8608 ran one four-second reach, hold and release per window on AutoSave-7. Frame generation and the FPS cap were disabled, the FPS overlay was enabled, and the simulator display rate was 90 Hz. The driver used one qualified C++ trajectory for both arms. All twelve measured windows passed, giving three complete ABBA blocks.

| Block | Real FPS change, on minus off | P95 real-frame interval change, ms |
|---|---:|---:|
| 1 | +0.434 | +0.020 |
| 2 | −2.395 | +0.203 |
| 3 | +1.481 | −0.736 |
| **Median** | **+0.434** | **+0.020** |

All six physics-on windows sustained contact with the bound static collider. Their longest runs of consecutive qualifying dwell samples were **8, 5, 8, 8, 8, 8** at 10 Hz. Each had eight clear withdrawal samples. Mean inward motor-target-to-wrist refusal during the settled dwell was 7.99–8.42 mm. The native counters recorded 692 right-hand world-contact steps with physics on and zero with it off. At corresponding samples the largest foot-position difference in any AB or BA pair was 1.27 mm. The rings recorded 1539 off and 1536 on real-frame intervals.

| Native timing | Physics off | Physics on |
|---|---:|---:|
| Before simulate, µs per counted step | 9.31 | 130.41 |
| After fetch, µs per counted step | 10.44 | 38.56 |
| Finger worker, µs per counted solve | No solves | 64.55 |

The pre-step and post-fetch callbacks together added **149.22 µs per counted step**. The separate finger worker recorded 1491 solves and 169435 collision calls, or 113.64 calls per counted solve. These ratios omit three counter-reset intervals in A and two in B.

The contact and pose measurements establish that the fixture exercised its intended workload. Confidence in a small FPS effect is limited by the three paired blocks, whose changes ranged from −2.40 to +1.48 FPS. The median is descriptive. The data support neither an FPS gain claim nor a production optimization for this case.

### Drift in the two-press warmup

The earlier `post-ab04` calibration qualified two presses. By the start of warmup B, the neutral foot had moved 7.04 mm outward along the press axis from the first qualifying start. Within warmup B, however, the two presses began at identical recorded foot positions and headings. The neutral wrist differed by only 0.03 mm along that axis. During the second settled dwell, the mean foot and motor-target positions shifted 0.85 mm outward relative to the first dwell, while the solved wrist moved only 0.09 mm outward. Mean inward refusal fell from 5.04 to 4.29 mm, crossing the 5 mm acceptance threshold. Mean static penetration stayed near 25.3 mm in both dwells.

A translation measured before those two presses would have assigned essentially the same correction to each one. The distinguishing displacement developed under load. The final one-press protocol obtained repeatable contact with matched starts and the existing threshold. It did not require a per-trial target adjustment.

The completed run is `build/physical-melee/post-ab05/hand-physics.json`, SHA-256 `46654ab65faa9b8eba95b84635157835ac14b3424a2a9d2a039554550181c51a`. The drift comparison uses `build/physical-melee/post-ab04/hand-physics.json`, SHA-256 `657a8048cbcfdfdf1b5a3e686dd8dd9c8c826104c69cc8badbc78a2f67e66442`. [Storefront fixture and acceptance criteria](../../../tools/vr_bench/README.md#storefront-post-on-autosave-7)
