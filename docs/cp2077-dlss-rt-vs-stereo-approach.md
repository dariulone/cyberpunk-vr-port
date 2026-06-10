---
name: cp2077-dlss-rt-vs-stereo-approach
description: "DECISIVE direction insight (2026-06-09) — DLSS & RT are temporally-accumulated PER-VIEW, which INVERTS the approach ranking: D3D12 command-replay is HOSTILE to DLSS/RT (shares engine's single history), while the sequential/AER family runs DLSS+RT natively per frame (proven by RealVR). Max-quality + DLSS + RT favors sequential, NOT D3D12 true-stereo."
metadata: 
  node_type: memory
  type: project
  originSessionId: a4faea66-21d0-4884-a9b4-10c3d5515440
---

User asked (2026-06-09): which stereo approach gives MAXIMUM image quality with working DLSS + RayTracing? This is the decisive lens — it inverts the prior ranking.

**Core fact:** DLSS (temporal upscaler) and RT (RTGI/reflections/path-tracing via ReSTIR + denoisers) are BOTH temporally accumulated PER VIEW. Each needs: per-eye jittered matrices, per-eye motion vectors (velocity buffer), and its OWN temporal history buffer. Mixing two viewpoints into one history => ghosting/smearing/denoiser garbage.

**Approach ranking for max-quality WITH DLSS+RT:**
1. **Native simultaneous** — engine manages 2 views = 2 DLSS/RT histories natively = perfect. But DEAD ([[cp2077-native-simultaneous-exhausted]]).
2. **Sequential / AER family (BEST ACHIEVABLE)** — each eye is a COMPLETE NATIVE ENGINE FRAME, so the engine runs its own DLSS jitter/motion-vectors/history + RT denoisers exactly as in flat 2D. This is why RealVR (the reference CP2077 VR mod) runs DLSS and even path tracing: alternate-eye = native frames. Only artifact = temporal history alternating viewpoints by IPD, mitigated by runtime/optical-flow (AER V2). The user already has this infra half-built ([[aer-render-pose-submit]]).
3. **D3D12 command-replay (WORST for DLSS/RT)** — the replayed eye re-executes the engine's recorded commands, which reference the engine's SINGLE NGX/DLSS context + single history buffer + single denoiser history. Replaying with a swapped camera feeds the right-eye G-buffer into the LEFT eye's history => temporal corruption. Clean per-eye DLSS+RT via replay requires cloning the DLSS history + motion-vector + exposure buffers AND redirecting the NGX Evaluate + RT denoiser calls per eye — i.e. the 35-40 RT clone problem PLUS NGX/denoiser-handle surgery. Highest fragility, highest quality risk. ([[cp2077-d3d12-replay-feasibility]])

**Two extra clinchers for the sequential family when RT/DLSS are the priority:**
- RT-ON (esp. path tracing) per-eye cost is so high that genuine-simultaneous (~188-200% GPU) is likely unplayable; sequential spreads the 2 eyes across 2 frames => RT-on essentially FORCES sequential for perf anyway.
- DLSS quality depends on clean per-eye temporal history, which native full-frame rendering gives for free and replay does not.

**DIRECTION CONSEQUENCE:** if the user's true priority is "max picture quality + working DLSS + working RT," the genuine-simultaneous goal (native = dead, D3D12-replay = hostile to DLSS/RT) actively WORKS AGAINST that. The proven quality path in CP2077 specifically is the SEQUENTIAL / AER family (what RealVR ships), polished via AER V2 optical-flow to kill the ghosting the user disliked. The project already has the AER V2 pipeline partially built. This reframes the whole effort: stop fighting for native simultaneous; invest in making the sequential/AER path's quality maximal (per-eye history is native, add optical-flow frame-gen + depth-aware reprojection).
