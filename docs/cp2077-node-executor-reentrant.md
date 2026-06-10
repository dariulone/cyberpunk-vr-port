---
name: cp2077-node-executor-reentrant
description: option-C ACTIVE test PASSED — re-executing a scene node via sub_1401EC404 a 2nd time survives (no crash); reentrancy confirmed
metadata:
  type: project
---

CP2077 option-C true-stereo, 2026-06-07: ACTIVE double-execute test. Built a clean call-original thunk for sub_1401EC404 (displaced 'mov [rsp+18h],rbx' + jmp found+5, bypasses our E9 patch => no recursion). In the executor pre-hook callback, for one scene-cluster node (rtId 32..79) per scene-active frame, called the original a 2nd time. Budget=60 re-executions. **Result: 60 re-executions survived, no crash.** => sub_1401EC404 is reentrant at the node level; the FrameGraph barrier/state machine tolerates a node executing twice. This is the M3 test done at the CORRECT layer (M3 hit the idempotent scheduler; this hits the real executor).

CAVEAT (not yet proven): re-exec used the SAME args (same rtId/matrices) so it renders the same draws again — survivability is proven but extra-GPU-work was NOT measured (could still be partially deduped like M3). Also NOT proven: full scene-cluster doubling (730 nodes) survives, coherent 2nd image, RT redirection, matrix swap.

Hook stability note: install MUST be deferred to steady state (present thread, frame ~550). Installing at module-init races spawning job workers => base-5 torn-patch AV (one menu-load crash confirmed this). Deferred install = crash-free loads. Builds on [[cp2077-framegraph-node-executor-hookable]]; matrices swap point renderer+0x4658 via [[aer-render-pose-submit]] DLSS hook. Next: escalate to full scene-cluster re-exec in a window (measures real GPU work via RT-diag binds + FPS, tests at-scale survivability — higher crash risk).
