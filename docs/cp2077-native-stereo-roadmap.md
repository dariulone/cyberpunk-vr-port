---
name: cp2077-native-stereo-roadmap
description: "synthesis 2026-06-09 — why 35+ sessions failed (3 closed families) + ranked remaining vectors (1: spawn fresh entity w/ in-world 0x307D690 RTT cam; 2: render-thread attach-piggyback; 3: Sync Sequential fallback)"
metadata: 
  node_type: memory
  type: project
  originSessionId: a4faea66-21d0-4884-a9b4-10c3d5515440
---

**Synthesis (2026-06-09) of the whole native-stereo effort. Why nothing worked yet — 3 closed families, one root cause:** REDengine carries exactly ONE main view/frame at every accessible level, and per-frame state is non-idempotent + globally arena-slotted, so a 2nd main view can only be created by the engine's own registration path.

Closed families (all DECISIVE, do not retry):
- A "replay/double": node WRITE AV, driver READ AV, view no-op, FG-build base−5, producer-double mgr-tear ([[cp2077-inline-replay-dead-all-levels]], [[cp2077-producer-double-mutates-mgr]]).
- B "hand-build 2nd view": byte-clone = global arena slot collision; registry insert needs engine hash+insert; call-site patch tears shared mgr ([[cp2077-clone-crash-global-arena-slot]], [[cp2077-registry-injection-feasibility]]).
- C "RTT handle shortcuts": every callable path consumes handle@+0x1E8, never allocates; resref is runtime token; script append != attach; isEnabled insufficient ([[cp2077-rtt-view-create-fn]], [[cp2077-resref-autoload-dead]], [[cp2077-spawn-test-array-append-negative]]).

**USER PRIORITIES (2026-06-09, explicit):** maintainability/patch-durability is IRRELEVANT ("вообще не важна"). The ONLY goals are (1) it actually WORKS and (2) optimization/performance. => stop weighing fragility; do whatever hacky thing works. "Optimization" within genuine-simultaneous = let the ENGINE share view-independent passes (shadows/CSM/distant ~12% per [[cp2077-pass-census]]) + native per-view FG-build GPU-backing (attach-hook gets this for free, vs hand-rolled 2× view inject).

**USER DECISION (2026-06-09):** committed to **vector 2 = render-thread attach-piggyback hook** for genuine simultaneous stereo, accepting invasiveness (rejected the AER V2 / Sync-Sequential fallbacks and the spawn-entity cheap test).

**UPDATE (run #1 of the probe, [[cp2077-proxy-attach-probe]]):** the "per-frame attach iterator to piggyback" premise is FALSIFIED — sub_140912294 (proxy attach) is a ONE-TIME setup event (fired 0× across ~900 frames of live builds/frame=2). So vector 2 is NOT "piggyback a per-frame loop"; it's "catch the one-time attach (probe armed before a FRESH RTT attach) → learn the attach trigger/dispatcher → fire it once for our right-eye cam; the fan-out then renders it every frame for free." Critical path is still the one-time GPU-backed handle (the 35-session wall), now with a clean catch-tool. Execution target = find the per-frame render-scene attach iterator that virtually dispatches sub_140912294 per RTT cam, hook it safely (PatchCodeSafely thread-suspend per [[viewsched-hook-crash]], NOT prologue patch), inject our player-attached right-eye RTT cam into that attach iteration so engine GPU-backs handle@+0x1E8 + enrolls its view for free on the correct thread/phase. Then per-frame drive cam pose=right-eye + route its RT into OpenXR submit.

**Ranked remaining vectors:**
1. **Spawn a FRESH entity whose template carries the RTT cam** (entity-spawner runs full native assembly+attach — the exact lifecycle script-append skips). KEY twist: don't fight the preview producer (0x2B03A70, resref-walled) — target the in-world producer **0x307D690** (self-allocating: handles@+0x200/+0x230, view-pair@+0x268/+0x270, NO resref). In-game surveillance-camera/monitor device entities are the live exemplars. Step 1 = dump a live in-world camera feed component via rpm.ps1 to learn WHICH property selects producer 0x307D690 (unknown per [[cp2077-spawn-test-array-append-negative]]); step 2 = spawn that entity template (CET/DynamicEntitySystem) near player; success = handle/view-pair non-null; step 3 = per-frame drive its transform/res/mode to right-eye, grab its RT, submit as right eye. Both eyes then render in ONE engine tick via native fan-out (inventory proves 2 builds/frame works, [[cp2077-per-view-independent-fgbuild]]).
2. **Render-thread attach-piggyback hook**: find dispatcher of sub_140912294 (render-proxy attach, pure virtual), inject our cam into the engine's own attach iteration ([[cp2077-attach-is-render-thread-virtual]]). Higher risk: hot-patch history = base−5 AVs.
3. **Fallback = Synchronized Sequential**: freeze sim/time between eye-0/eye-1 engine frames, submit as synced pair ([[redengine-job-render-arch]]). Honest rationale: [[cp2077-pass-census]] shows true-simultaneous saves only ~6–12% GPU vs sequential; the real goal is "both eyes from one sim tick", which freeze gives without breaking any engine wall. User rejected AER ghosting — Sync Sequential is NOT AER (eyes identical sim state); pair rate fps/2 is its cost.

Perf reality either way: 2 full views ≈ 188–200% GPU (per-eye share 88%). Quality parity for vector 1 requires renderingMode=Shaded ([[cp2077-rtt-shaded-mode-equal-quality]]) + features bits checked live.
