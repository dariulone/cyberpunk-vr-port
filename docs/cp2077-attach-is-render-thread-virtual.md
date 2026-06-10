---
name: cp2077-attach-is-render-thread-virtual
description: GPU-backing/handle path is entirely render-thread + virtual-dispatch; no callable allocator exists; only true-stereo path left = render-thread attach-piggyback hook
metadata:
  type: project
---

Fresh xref re-confirmation (this session) that the RTT-camera GPU-backing path has NO callable shortcut, approached from the GPU-render-texture vtable downward:

- vtable `exe+0x2B1BAA0` constructed by 3 ctors (`sub_1412A3CC0`, `sub_140821538`, `sub_140821620`) -- ALL build only the CPU descriptor (each calls only `sub_14022413C`); none does DX12/GPU allocation. Confirms [[cp2077-rendertex-needs-generator]].
- GPU backing comes from init-driver `sub_14012F190` (the puppet-preview init from [[cp2077-rendertex-init-driver-stack]]), whose callers are all render-side: `sub_140912A18`, `sub_140913588`, and `sub_142211654` (create+register dyn-RT in region 0x2211xxx).
- `sub_142211654` <- `sub_140912294` (render-proxy init/attach). `sub_140912294` has ZERO direct callers = pure virtual dispatch via render-scene attach.

CONCLUSION: there is no "call this to get a GPU-backed handle" function. The handle is produced only when the engine runs its render-scene attach for an RTT camera, on the render thread, via virtual dispatch, gated by TLS job-state (gs:[0x58]). Every callable-from-outside attempt across ~35 sessions dies here (deadlock or gate no-op).

THEREFORE the only path consistent with simultaneous-sync stereo + full quality + maintainability is NOT calling engine fns from outside, but a render-thread attach-piggyback hook: hook the engine's per-frame point where it iterates RTT cameras and runs their attach/init (the dispatcher of `sub_140912294`), and inject our player-attached right-eye camera into that same pass so it gets GPU-back + view-enroll for free, on the correct thread/phase. Next reverse step: find the render-scene attach iterator that virtually dispatches `sub_140912294` per RTT cam.

User decision (2026-06-09): rejected AER (ghosting) and reprojection; requires genuine simultaneous second view. Deferred renderer => simultaneous stereo MUST be 2 full views (~88% per-eye [[cp2077-pass-census]]); no geometry-doubling shortcut. Links: [[cp2077-rtt-view-create-fn]] [[cp2077-rtt-handle-allocator-found]] [[cp2077-isenabled-necessary-not-sufficient]] [[cp2077-rtt-proxy-is-registered-type]].
