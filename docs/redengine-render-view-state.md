---
name: redengine-render-view-state
description: CP2077 centralized render-view matrix state at renderer+0x4658, built by sub_140788A9C — the render-level eye-injection point
metadata:
  type: project
---

CP2077 render-level view state (the structure the scene-render reads for the main view):

- **Renderer global:** `qword_143427C00`. Render-view state lives at `renderer + 18008` (0x4658) — call it `v6` in `sub_140788A9C`.
- **Builder:** `sub_140788A9C` (render-thread, per-frame) computes the full matrix set from the camera object (`a2[3]`) and writes into `v6`. The DLSSMatrices hook site (`0x1407892CE`, AOB `48 8B CB 8B 90 A0 01 00 00 E8`) is inside this function.
- **Layout of v6 (render camera CB):**
  - `+32..95` view-projection 4x4
  - `+96..159` proj matrix
  - `+160..223` / `+224..287` view + inverse
  - `+288..415` prev-frame matrices (TAA/motion)
  - `+416/+420/+424` camera world position (fixed-point * 1/131072 -> float)
  - `+464/+468` render resolution W/H

**Why it matters for True Simultaneous Stereo (option C):** all main-view matrices are centralized in ONE slot (not an array) and are fully parameterizable — injecting right-eye projection is trivial (rebuild v6). BUT this function only *builds* v6; it does not dispatch scene-render or bind the RT. C therefore still = run scene-render twice (v6=left->RT-A, rebuild v6=right->re-dispatch->RT-B, Present both). Missing link = the scene-render DISPATCH (who reads v6 and draws; is it re-invokable). See [[redengine-job-render-arch]].

**How to apply:** to find the dispatch, trace callers/callees of sub_140788A9C downstream toward the job-graph scene render. The render is job-graph (redJobs2), so re-dispatch is the hard, risky part — confirm re-invokability before committing.

## Dispatch resolved (runtime stack capture from DLSS callback, 3 identical dumps)

Render is a **FrameGraph**: redJobs2 -> recursive render-graph executor. Key functions:
- `sub_1401EC404` (caller of view-builder region) = **render-graph node executor**: calls each pass's polymorphic `execute()` via vtable `(*a1+8)(a1,a2,a3)`, iterates a resource bitmask (`a1[1]`, `_BitScanForward64`), reads frame resource tables at `renderer+19536` / `renderer+16908`.
- `sub_1401F41F8` = render-graph **resource-state / barrier tracker** (hash table of resources, `0x80000000`=unknown state, `sub_1401F483C`=emit transition).
- Outer frames = job system cluster (`0x142xxxx`, near job-loop `0x140143060`).

**Final verdict on option C (true simultaneous stereo):** C = re-run the entire per-frame render-graph (build+compile+execute) a 2nd time with right-eye v6 + right-eye RT. It does the SAME double GPU work as Synchronized Sequential (two full scene renders) but is far harder/riskier: the barrier tracker and transient resources assume ONE execution per frame, the graph is not re-entrant, and it runs inside redJobs2. C is NOT cheaper at runtime. Its only real win over a good Sync-Sequential is sharing view-independent passes (shadows/GI) between eyes — which needs even MORE surgery (split graph into shared vs per-eye). Recommendation: Synchronized Sequential is the pragmatic path; C only if willing to do deep frame-graph surgery. See [[redengine-job-render-arch]] and [[aer-render-pose-submit]].
