---
name: cp2077-framegraph-build-chain
description: full static trace from SceneDrv rtId up to the FrameGraph-build job + its submit sites — the RT-routing source and simultaneous-stereo lever
metadata:
  type: project
---

CP2077 simultaneous true-stereo, RT-routing static trace (2026-06-09, IDA).

Traced where the per-pass output RT (rtId) is ASSIGNED, from the SceneDrv driver up to the source:
- driver sub_1401EC1D0 reads rtId = byte ctx+0x38 [[cp2077-scene-pass-driver-rtid]].
- pass/ctx built by sub_1409853B4: allocs 0x38 ctx, sets ctx[0]=driver descriptor 0x142B3A008,
  receives **rtId as arg1 (dl)**, stores built passObj at [arg0+0xF0]. => rtId chosen by CALLER.
- caller = sub_141D43040 (~0x456f bytes, HUGE) — the FrameGraph builder: ~50 calls to the pass-builder,
  each with a hardcoded per-pass rtId. This is where all scene passes + their output RT slots are defined.
- single-caller chain up: sub_141D43040 <- sub_140219730 (0xa00) <- sub_140AA3904 (0xb4, called
  INDIRECTLY = a job work-fn).
- sub_140AA3904 addr stored (data) in sub_140167FC8 (0x107), sub_140293568 (0x103), and table
  0x1449c1bf0 => the FrameGraph-build JOB is submitted from sub_140167FC8 / sub_140293568.

INTERPRETATION (matches [[cp2077-framegraph-rt-routing]]): rtIds are logical indices; the FrameGraph
auto-allocates physical pooled RTs per build. So the simultaneous lever = make the engine run a 2nd
FrameGraph build for a right-eye view => fresh RTs auto-allocated, native 2nd-eye render. Submit sites
sub_140167FC8 / sub_140293568 are CONCRETE functions (not pure virtual dispatch like the render-view
producer [[cp2077-simultaneous-producer-virtual-wall]]) => candidate hook points.

OPEN (needs a game run): is the FG build per-FRAME (all views) or per-VIEW? how many times/frame does
sub_140AA3904 / the submit site run? is re-submitting a 2nd build viable (vs the inline-reexec which is
dead [[cp2077-renderview-orchestrator-reexec]])? Next: read-only hook on sub_140AA3904 (or submit site),
log hits/frame + args, + stack to confirm producer thread.
