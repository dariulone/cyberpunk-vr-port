---
name: cp2077-simultaneous-producer-virtual-wall
description: simultaneous true-stereo — all hookable per-view points are consumer-side; producer/view-submit is generic virtual dispatch; isolated blocker = FrameGraph RT-routing
metadata:
  type: project
---

CP2077 simultaneous true-stereo, producer-search result (2026-06-09).

Inline doubling proven DEAD at every altitude [[cp2077-renderview-orchestrator-reexec]]. Hunted the
PRODUCER (who submits the per-frame render-view job so we can submit a 2nd right-eye one):
- render-view job work-fn = thunk sub_140D69018 -> its addr lives in job type-descriptor 0x143334780.
- 0x143334780 has ONE static xref: sub_141317EAC, which is a magic-static ACCESSOR ("get render-view
  task descriptor"), and is itself a VIRTUAL method of the per-view camera class (its addr sits in the
  camera vtable 0x2B72EE8 region at 0x142b72ff0, i.e. vtable+0x108) + table 0x144a43da8.
- camera object factory sub_140BE3660 also called indirectly (registered factory); camera object is
  persistent/reused per frame, not re-created.
- LocateCamera hook (where AER shifts the camera) runs SYNCHRONOUSLY BELOW the render-view task =>
  CONSUMER side, not producer.

CONCLUSION: every statically-reachable / hookable per-view point (sub_140126D4C, LocateCamera,
sub_141317EAC) is CONSUMER-side (inside the per-view render job). The per-frame VIEW-LIST assembly +
job submit is a generic loop over a view collection via virtual dispatch — not statically reachable.
Even if we duplicate the view-job at submit (runtime hook), the 2nd view routes to the SAME scene-color
RT (RT routing is downstream in FrameGraph, keyed by view identity, not in any view-level struct
[[cp2077-perview-camera-layout]]). So the ISOLATED, single remaining blocker for native simultaneous =
**FrameGraph RT-routing**: make a 2nd view render to a 2nd RT. SceneDrv driver carries the output slot as
byte ctx+0x38 (rtId) [[cp2077-scene-pass-driver-rtid]] — the rtId is assigned when the FrameGraph is
built; that assignment is the reverse target. This is the multi-session FrameGraph wall
[[cp2077-framegraph-rt-routing]] [[cp2077-framegraph-node-executor-hookable]].
