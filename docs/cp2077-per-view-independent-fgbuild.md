---
name: cp2077-per-view-independent-fgbuild
description: DECISIVE positive - engine renders each view as its own independent FG-build job w/ fresh ctx; gameplay=1 build/frame, inventory=2; simultaneous needs triggering a 2nd per-view build, NOT re-invoke
metadata:
  type: project
---

Measured live (FG-build hook builds/frame counter, F7): **gameplay = 1 FG-build/frame, inventory open = 2 FG-build/frame** (clean toggle correlated with inventory open/close).

**Meaning (overturns the "simultaneous via injection is dead" conclusion):**
- Engine renders EACH view as its OWN independent FrameGraph-build job (`sub_140AA3904`), each with a FRESH per-build ctx (arg0=FG-context, arg1=job-ctx, both stack/transient, allocated per build by producer on tid=10776).
- The F9 crash ([[cp2077-node-replay-not-viable]] family) was self-inflicted: re-invoking `sub_140AA3904` with the SAME ctx hit single-use cursor OOB. The engine never re-invokes - it enqueues a SECOND build with its OWN ctx. So 2nd-eye is NOT a replay problem.
- Inline re-invoke being dead at all 4 levels stays true; but the real native path is different: **cause a 2nd natural per-view FG-build for the right eye** (main camera + IPD), exactly how a render-active 2nd view (inventory preview) spawns build #2.
- Connects to RTT-camera findings: a render-active 2nd view => engine auto-enqueues its FG-build => its own RTs. The "render-active 2nd view" is the linchpin, now with a concrete mechanism (per-view build job).

**Next:** capture BOTH builds' arg0/arg1 during inventory (modify one-shot FG hook to dump 2 distinct builds/frame), diff to find the per-build view reference => learn how a build is parameterized to a view => construct/enqueue a 2nd build for right eye. Producer enqueue path = FG-build caller chain (tid=10776). Relates [[cp2077-framegraph-build-chain]] [[cp2077-view-registry-live]] [[redengine-job-render-arch]].
