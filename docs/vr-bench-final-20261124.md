# The final VR bench: C++ (main) against Rust (rust-main), 2026-11-24

One owned launch per build, the same save (AutoSave-4), the same simulator (OpenXR Simulator, Quest 3 profile, 90 Hz
target), the same head trajectories and the same settings (the Rust install's `vrport.ini` copied into the C++
install; `xr_engine_stereo=1 xr_depth_submit=1 xr_fps_cap=1`, frame generation, OSR, AFW, foveated DLSS, VRS and the
hidden area off in both). `tools/vr_bench/vr_bench.py run`, driven by a script that puts main's package (f7a824dd5,
`build_dist.ps1`) into `red4ext\plugins\cyberpunkvrport` for the C++ run with the port's own loader set aside, and
takes it out again for the Rust run (5e5fcf2f7 + the own loader, 6fa4f539a).

Results: `build/vr-bench/20261124-032423-final-cpp-main`, `build/vr-bench/20261124-032806-final-rust-main`.

## Frame rate (GPU-bound in both: GPU utilisation 98-100 %)

| trajectory | C++ fps | Rust fps | C++ GPU p50 ms | Rust GPU p50 ms | C++ 1% low | Rust 1% low |
|---|---|---|---|---|---|---|
| still | 56.7 | 53.0 | 17.60 | 18.77 | 53.0 | 47.3 |
| yaw_slow | 56.2 | 53.2 | 17.77 | 18.78 | 52.7 | 48.1 |
| yaw_fast | 56.6 | 53.8 | 17.72 | 18.53 | 52.1 | 48.1 |
| snap90 | 57.2 | 55.2 | 17.32 | 18.12 | **28.6** | **50.7** |
| combo3 | 55.6 | 52.8 | 17.94 | 18.96 | **39.6** | **49.9** |
| translate_lr | 55.9 | 52.7 | 17.87 | 18.93 | 51.9 | 47.1 |
| lean_in | 55.8 | 52.6 | 17.94 | 19.06 | 52.6 | 49.6 |

The Rust build renders a frame about 1 ms (5-6 %) slower on the GPU; its 1 % lows are steadier on the sharp head
moves (snap90, combo3), where the C++ build drops to 28.6 / 39.6 fps.

## CPU (per-thread, mean over the trajectories)

| | all threads | game thread | engine workers |
|---|---|---|---|
| C++ | 686 % | 94.3 % | 524 % |
| Rust | 663 % | 96.0 % | 540 % |

The Rust build uses 3 % less CPU in total; the game thread is saturated in both (GPU-bound frames).

## Where the GPU time goes (still head, 12 frames, ms)

| node | C++ | Rust |
|---|---|---|
| DLSS | 5.21 (one pass over both eyes) | 4.79 (2.41 + 2.38, one per eye) + 0.26 |
| RenderElements | 2.80 | 3.32 (1.98 left + 1.34 right) |
| RenderLightBuffers | 1.50 | 1.88 |
| RenderLightsIntegrate | 1.46 | 1.54 |
| DecoupledParticleLighting | 0.21 | 0.49 |
| Present | 0.91 | 0.40 |
| total | 17.96 | 18.38 |

The two builds take different DLSS paths in this configuration (main's single evaluation over the side-by-side frame,
the Rust build's per-eye evaluation); the Rust build's scene passes (elements, light buffers, particle lighting) cost
more, its present less.

## Stereo correctness (Rust; main's DLL does not expose the capture the check reads)

Left-to-right / right-to-left geometric agreement 0.938 / 0.938 (still), 0.950 / 0.943 (yaw slow), 0.991 / 0.992 (yaw
fast), 0.936 / 0.940 (translate); pixels off by more than 32/255: 0.08-0.41 %.

## Beyond the numbers

Rust: one plugin with its own loader (no RED4ext, CET, redscript or Lua needed), the address library read in 0.1 s;
memory-safe ports of the hooks with the engine's faults caught in Rust (seh.rs); every mechanic in one DLL.
C++ (main): a RED4ext plugin with the built-in Lua host and the script compiler; 18.7 MB DLL against 38.6 MB.
Not compared here: frame generation, OSR / NR / FSR3 (main's newest frame paths, 2e123a4a3, are not ported yet) and a
physical headset.

## Re-run after the 0.2.4 port (2026-11-24, 11:11-11:25)

The same driver, save and settings; main's package (0.2.45, f7a824dd5) against the Rust build with main's 0.2.4 now in
it -- the runtime's own D3D12 queue, the frame generation pacing, the scope DLSS isolation, the midpoint depth and the
rest (cb4d234f7..466e86081). Results: `build/vr-bench/20261124-111140-final-cpp-main`,
`build/vr-bench/20261124-112530-final-rust-main` (two earlier attempts stopped on the simulator's "trajectory did not
finish"; `20261124-110724-final-rust-main` ran before 466e86081 and could not take the stereo check: the previous
captured frame went without its inputs then).

| trajectory | C++ fps | Rust fps | C++ GPU p50 ms | Rust GPU p50 ms | C++ 1% low | Rust 1% low | C++ 0.1% low | Rust 0.1% low |
|---|---|---|---|---|---|---|---|---|
| still | 56.3 | 51.6 | 17.77 | 19.34 | 52.6 | 46.2 | 52.4 | 45.9 |
| yaw_slow | 56.0 | 51.7 | 17.90 | 19.30 | 52.2 | 45.8 | 51.6 | 45.4 |
| yaw_fast | 56.6 | 52.3 | 17.54 | 19.11 | **30.8** | **47.5** | **16.8** | **47.1** |
| snap90 | 58.0 | 53.7 | 17.22 | 18.53 | **42.8** | **49.3** | **30.2** | **49.2** |
| combo3 | 55.9 | 51.4 | 17.87 | 19.45 | 52.2 | 47.9 | 51.5 | 47.1 |
| translate_lr | 56.3 | 51.5 | 17.77 | 19.41 | 53.7 | 46.2 | 53.6 | 46.0 |
| lean_in | 56.0 | 51.4 | 17.84 | 19.53 | 52.9 | 48.8 | 52.3 | 48.7 |

CPU (mean over the trajectories): C++ 672 % in all, game thread 94.3 %, engine workers 523 %; Rust 653 %, 96.8 %,
535 %. Both runtimes now hand the session a queue of their own (main's package has it too), so the reprojection column
of the summaries is 0 for both: a cycle whose newest capture is still on the GPU resends the last frame outside the
projection path it counts.

What changed against the morning: the Rust build is still GPU-bound about 1.5 ms a frame slower (19.3 against 17.8 ms
p50), the difference in the engine's own scene passes (RenderElements 3.58 / 2.90, RenderLightBuffers 2.16 / 1.53,
DecoupledParticleLighting 0.66 / 0.22, a RenderFogOverlay of 0.30 the morning's runs did not have -- the scene's
weather moved between the runs) and in the per-eye DLSS (2.43 + 2.42 against one 5.37 over both eyes, nearly even);
its Present is half of main's (0.46 / 0.90). Its sharp-turn lows stay far steadier: yaw_fast 47.5 / 47.1 against
30.8 / 16.8, snap90 49.3 / 49.2 against 42.8 / 30.2. Stereo agreement (Rust) 0.937 / 0.952 / 0.938 / 0.936 for still,
yaw slow, yaw fast and translate (main's DLL has no capture to check).

Frame generation (FidelityFX, NVIDIA OFA, FSR3, ANR FG, DLSS-G) and NR are ported now but not benched here: DLSS-G
needs an RTX 40/50 (this machine has an RTX 3060, where both builds fall back to FidelityFX) and ANR FG its package.
