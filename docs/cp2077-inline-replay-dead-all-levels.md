---
name: cp2077-inline-replay-dead-all-levels
description: DECISIVE — inline re-invocation of the render pipeline is dead at ALL 4 levels (node/driver/view/FG-build); 2nd eye needs a genuinely fresh ctx+view-set, not a replay
metadata:
  type: project
---

Tested inline re-invocation of the REDengine render pipeline at every altitude to get a 2nd (right-eye) render. **All four are dead:**

| Level | Hook | Result |
|---|---|---|
| per-node | sub_1401EC404 | WRITE AV ([[cp2077-node-replay-not-viable]]) |
| scene-pass driver | sub_1401EC1D0 | READ AV (OOB on one-shot per-frame index) |
| render-view | sub_140126D4C | no-op (passes/frame NOT doubled — setup only, real render is async) |
| **FrameGraph-build** | **sub_140AA3904 (F9, 2nd build/frame)** | **READ AV base−5** inside the build (backtrace through 0x140AA39A7/0x140AA3957 → job plate 0x14324C/0x16D50E/0x1A3ABF4); crashed on the FIRST armed frame |

FG-build crash signature: `code=0xC0000005 READ target=gameBase−5`, rip in extern module = engine unwind/state machinery. Cause: re-running the build with the **same job-ctx (a2)** advances that ctx's single-use cursor/arena twice → invalid relative read.

**Conclusion (definitive):** the engine's per-frame render is non-idempotent at every level — a 2nd eye CANNOT be a replay of the same call. It needs a **separately constructed, freshly-initialized view-set + job-ctx** (its own registry [[cp2077-view-registry-live]], its own ctx, its own RTs), submitted as a real 2nd view-job through redJobs2 ([[redengine-job-render-arch]]). That producer is behind the job queue + virtual dispatch ([[cp2077-simultaneous-producer-virtual-wall]]) — the genuine wall.

Remaining native-simultaneous path = reverse the producer that POPULATES the view registry (vtable exe+0x2B57F58) + enqueues the view-job, and drive it to build a 2nd right-eye view-set. No cheap inline lever remains.
