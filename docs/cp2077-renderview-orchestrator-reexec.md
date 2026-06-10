---
name: cp2077-renderview-orchestrator-reexec
description: sub_140126D4C decoded as per-view render orchestrator; view-level double-invoke is the next-altitude simultaneous-stereo test (F5)
metadata:
  type: project
---

CP2077 simultaneous true-stereo, view-altitude re-invoke test.

`sub_140126D4C` (real render-view, 0x6b bytes) = thin **per-view orchestrator**: with rcx=this
(view/camera object, vtable rva 0x2B72EE8) held in rdi, it sequentially calls render stages —
sub_140126DB8 (FIRST, at entry, before stages — candidate "acquire fresh per-frame view slot"),
sub_140127B88, sub_140127AB8 (gets xmm1=spec[0]), sub_14012752C, sub_140127404, sub_14012701C,
tail-jmp sub_140126F18. rdx=view-spec is only read as one float `[spec+0]`. So the **entire view
render is parameterized by `this`**.

Re-invoke ladder for the 2nd eye (inline duplication):
- node-level (sub_1401EC404) => WRITE AV [[cp2077-node-replay-not-viable]]
- driver-level (sub_1401EC1D0) => READ AV, OOB on single-use per-frame index `qword_143439A28[edx]`
  at fault ida=0x1401F51F5 [[cp2077-driver-reexec-oob]]
- **view-level (sub_140126D4C) = UNTESTED, one altitude up.** Hypothesis: its first call
  sub_140126DB8 acquires a FRESH per-frame view slot, so a 2nd invoke gets fresh indices and
  survives where driver replay didn't.

RESULT (FALSE POSITIVE � RETRACTED): view-level double-invoke "survived" 240x ONLY because it does NO actual rendering. Decisive per-frame measurement: scene-pass driver work baseline=80/frame, in-window=80/frame => NOT doubled. sub_140126D4C re-invoke re-runs idempotent setup; the real render is async redJobs2 jobs enqueued ONCE/frame. 2ND-PASS rtId attribution = 0 (driver not synchronous within the view call). INLINE RE-INVOKE IS DEAD AT ALL ALTITUDES: node=WRITE AV, driver=READ AV, view=no-op. Simultaneous REQUIRES producer-side: enqueue a 2nd (right-eye) view-job. 
Shipped test detail: **F5** arms a gated double-invoke of sub_140126D4C in
OnRenderViewCallback, gated to main-view vtable rva 0x2B72EE8, ~240-frame window. Calls
g_renderViewCallOriginal(a1,a2,a3,a4) a 2nd time (clean thunk, no hook re-entry). Log marker
`RenderView RE-INVOKE window DONE: ... (SURVIVED, no crash)` or a crash report.
Decision: SURVIVED => view-level inline dup viable => next clone `this` IPD-shifted for true eye-2.
CRASH => inline dup dead at every altitude => only path is true producer-side 2nd view-job submit
(submitter behind redJobs2 queue [[redengine-job-render-arch]]). Note: even on survival, RT routing
is downstream in FrameGraph (not in `this`) [[cp2077-perview-camera-layout]] — 2nd view may overwrite
left-eye RT; that's the follow-up sub-question.
