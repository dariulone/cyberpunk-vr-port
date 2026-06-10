---
name: cp2077-node-replay-not-viable
description: option-C make-or-break NEGATIVE - blind in-place re-exec of scene FrameGraph nodes corrupts state (WRITE AV); 2nd eye must be a separate view-object
metadata:
  type: project
---

CP2077 true-stereo (option C), decisive ACTIVE test result (2026-06-08):

**Blind in-place replay of scene nodes is NOT viable.** Full-cluster double-execute of the
FrameGraph node executor sub_1401EC404 (re-call original via thunk for every scene rtId 32-79)
crashed with **AV op=WRITE** to a heap addr one frame after install. Root cause: graph nodes are
NOT idempotent - the 2nd execute records draw commands into an already-consumed/finalized per-frame
command allocator / advances state past its end => memory corruption. The earlier budget probe
(60 single-node re-execs "survived") was luck: re-running ONE node/frame hit only safe ones; the
full scene cluster includes command-recording/accumulation nodes that cannot be replayed.

Consequence: "render the right eye by re-running the main view's passes" is empirically DEAD.
The 2nd eye MUST be a genuinely separate view-object the engine schedules its OWN nodes for
(mirror / entRenderToTextureCameraComponent / holocall path), not a replay of main-view nodes.
See [[cp2077-framegraph-node-executor-hookable]] [[cp2077-node-executor-reentrant]] (correct that
one: reentrancy holds per-single-node only, NOT for the scene cluster).

**Reusable tooling win:** the hot executor E9 hook installs cleanly when triggered MANUALLY (F8)
in quiescent gameplay. Auto-timing always fails - the main-menu 3D background renders large RTs too,
so any frame/RT-size heuristic installs during the menu and the save-load FrameGraph teardown then
base-5 torn-patch AVs. Manual in-world trigger = quiescent worker pool = clean 5-byte patch.
INT3+VEH would be atomic but adds an exception per node (~730/frame) => slideshow.
