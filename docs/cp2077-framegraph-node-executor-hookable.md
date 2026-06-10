---
name: cp2077-framegraph-node-executor-hookable
description: option-C make-or-break PASSED — FrameGraph node executor sub_1401EC404 is hot-hookable & stable; gives node-vtable -> RT-id map
metadata:
  type: project
---

CP2077 option-C true-stereo, 2026-06-07: hooked **sub_1401EC404** (FrameGraph per-node executor, rva 0x1EC404, hottest renderer fn, 40+/frame across worker threads) via PatchCodeSafely + displaced-prologue trampoline. **Stable, no crash, overflow=0** — make-or-break PASSED (this hot hook is survivable; earlier crashes were the non-atomic patch + r11 clobber, both fixed).

Args: rcx=a1 (node, *a1=vtable), rdx=a2 (descriptor). a2+0x38 = output RT-id byte (0..79, dense FrameGraph transient-slot index = the RT-routing lever found nowhere on view/camera/spec layers). a2+0x30 = flags (bit1 0x02 = has-output-RT; specials 0x42/0x13/0x12). vtable[8] (off 0x40) = execute(). Node clears its per-frame state at end (a2+58/62/64/65=0) => nodes are reentrant by design; barrier tracker holds ~40 views/frame => graph is NOT single-pass.

Steady-state map (frame 6006): total=730 node-execs/frame, 66 distinct RT-ids. Dominant node class vtable rva 0x0312E210 (generic pass node); special pass classes cluster 0x0312D0A0..0x0312E210. RT-id histogram is deterministic frame-to-frame.

DESIGN now concrete: true stereo = re-execute the scene-color node subset inline at the executor with renderer+0x4658 swapped to right-eye matrices + redirected output RT-id (the engine won't emit a 2nd view by itself — that's the FRONT wall, no script API per [[cp2077-rtt-no-drivable-api]]). Open: (1) which rtId(s) = main scene-color 3072^2 target (correlate rtId->renderer+19536/0x2F1D8 resource), (2) active double-execute feasibility (call vtable[8] twice — risky). Anchors: matrices renderer+0x4658 via [[aer-render-pose-submit]] DLSS hook; barrier/resource tables [[cp2077-framegraph-rt-routing]].
