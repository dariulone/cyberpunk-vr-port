# vr_bench -- VR rendering benchmark and stereo regression harness

One owned game launch per run, driven through the OpenXR Simulator; the game is closed afterwards.
Details and results: `docs/vr-render-optimization-20261104.md`.

```
py -3.13 -B tools/vr_bench/vr_bench.py deploy                       # install the built DLL (+ its PDB) if it differs
py -3.13 -B tools/vr_bench/vr_bench.py run --label baseline          # trajectories, every frame's times, GPU per node, stereo checks
py -3.13 -B tools/vr_bench/vr_bench.py ab --label x --variant "CyberpunkVR_Foo=0" --variant "CyberpunkVR_Foo=1"
py -3.13 -B tools/vr_bench/vr_bench.py compare build/vr-bench/<A> build/vr-bench/<B>
py -3.13 -B tools/vr_bench/show_profile.py build/vr-bench/<run>
py -3.13 -B -m unittest tools/vr_bench/test_vr_bench.py            # offline tests (no game)
tools\vr_bench\build_stereo_check.bat                               # stereo_check.exe (C++)
```

- `run` options: `--traj` (see `trajectories.py`), `--stereo-on`, `--timeline-on` (GPU queue timeline: busy GPU ms per
  frame vs the frame window, and the idle time split by `tools/hitch_tests/gpu_queue_timeline.py` into starved -- the
  CPU submitted after the GPU went idle -- and held -- a fence or the driver; raw records in `timeline/<traj>.json`),
  `--stereo-sweep` (stereo checks at IPD 58/70/64 mm and a symmetric and a wide asymmetric FOV, via the simulator's live
  ipd/fov commands; the Quest 3 profile restored after), `--node-detail 0xRVA,...` (a node per pipeline), `--draw-occlusion 0xRVA,...` (an occlusion query per draw of a node: per eye and per pipeline the
  draws that left no visible sample), `--pipeline-stats 0xRVA,...` (D3D12 pipeline statistics of a node
  per eye: primitives the clipper drops, VS / PS invocations), `--census` (shader census on the still head), `--diagnostics`,
  `--setting Name=Value` (a game setting for this launch, restored after; `Name=Value#index` for a list the game
  fills at run time, e.g. `DLSS=Ultra Performance#5`), `--etw-cpu <trajectory>` (CPU of the game's process in that
  window by module, thread and port function: `etw_cpu.py`, wpr + tracerpt, elevated shell; the trace is deleted
  after the analysis), `--no-telegram`.
- Every window also reports `repro%`: the share of the XR thread's display cycles that showed the game frame the
  previous cycle had already shown (`CyberpunkVR_XrProjectionCycles` / `CyberpunkVR_XrRepeatCycles`), i.e. what the
  runtime reprojects.
- A window in which the game presented fewer than 5 frames a second is recorded under `stalls` (shown in the
  summary) and measured again, in `run` and `ab`.
- `ab`: variants of live 32-bit exports take turns ABBA in one launch, each measured on one trajectory with a GPU
  profile per node (`--stereo` adds a stereo check per variant, `--etw-cpu` wpr CPU sampling of every window by module).
  `--prime "Name=Value;..."` sets exports for `--prime-seconds` during the warm-up and restores them before the first
  window (single pass: its view-instanced pipelines are built in the background then); `--counters` reads 64-bit
  counters per window, `Name[i]` the i-th of an exported array.
- `run --census --census-modes 3,4`: every draw's vertex / pixel shader per node; the shaders seen are kept in
  `cvr_shaders/` of the run (the input of the offline shader tools below).
- Single pass (`src/Stereo/SinglePass.cpp`): `view_twin_test.exe <dxc dir> <shaders> [out]` rewrites a folder of
  shaders the way the plugin does and checks each twin validates and reads SV_ViewID; `view_pso_test.exe <dxc dir>
  <originals> <twins> [vs [ps]]` creates the view-instanced pipelines offline (`ps`: every pixel shader with a vertex
  shader it links with). Build them with `build_view_twin_test.bat` / `build_view_pso_test.bat`. The plugin loads
  `dxc\dxcompiler.dll` and `dxc\dxil.dll` beside itself; `deploy` copies them from the Windows SDK.
- Head trajectories need the simulator with `head_trajectory.h` (`simulator/`: the header and the patch of its
  `runtime.cpp`; the simulator lives in its own repository).
- Results go to `build/vr-bench/<stamp>-<label>/`; only the newest 10 stereo folders are kept, raw frames are deleted
  after the check. Telegram reports read `%LOCALAPPDATA%\vr_bench\telegram.json` (`{"token": ..., "chat_id": ...}`),
  never the repository.
- `gpu_caps.cpp`: the D3D12 feature queries this work relies on (view instancing tier, shader model, ...).
