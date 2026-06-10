---
name: cp2077-eye2-clone-shared-subobj
description: eye-2 handle-clone gets the 2nd FG-build to RUN but crashes on shared mutable sub-objects (FrameGraph scratch); full byte-clone not enough
metadata:
  type: project
---

CP2077 native simultaneous, eye-2 via handle-CLONE (the furthest the project has reached).

## Setup that works
- FGB call-site hook (patch the `E8 call sub_140293568` inside sub_140293AF4 at rva 0x293BCE, NOT the prologue) is the only stable submit-region hook, BUT it is intermittently base-5 unwind-fatal at install when MANY worker-thread diagnostic hooks are co-installed. Fix: F7 installs ONLY SceneDrv (pass-count metric) + the call-site hook for the clone test. With just those two, F7 survives reliably.
- eye-2 = `Eye2CloneSubmit`: shallow byte-clone of `*a5` (the view-handle, vtable rva 0x2AC8688) into a fresh HeapAlloc, refcount pinned high (clone+8 = 0x100000) so engine dec never frees our memory, then call submitter sub_140293568 with a5'=&clone. Gives eye-2 a DISTINCT identity.

## Progress (real, milestone)
A distinct-identity clone gets the 2nd FG-build to actually RUN and progress through build stages the same-handle re-invoke never reached:
1. First crash was OOB READ at sub_140219730+0x346 (rva 0x219A76): `[clone+0xF88]` past a too-small 0xB00 clone => the view-handle object extends to >=0xF8C. Fixed by cloning 0x2000 bytes (page-safe chunked copy, full coverage confirmed: copiedBytes=0x2000 lastGood=0x2000, NOT truncation).
2. Next crash (current wall): WRITE AV at sub_14024CAC8+? (rva 0x24D4B2) `mov [rcx+8], r14`, rcx = torn value 0xAF000000B7 (two small u32 counters, not a pointer). eye-1 (orig) does NOT crash on the same code path => the field feeding rcx is valid for orig but stale/torn for the clone.

## Diagnosis
The view-handle's sub-objects (h+0x40..0x68 etc.) are SHARED by the byte-clone (copied as pointers). The FrameGraph build writes per-build SCRATCH state into these shared sub-objects; running two builds (eye-1 on orig + eye-2 on clone) concurrently corrupts/torn-writes the shared scratch. RT itself is auto-allocated per distinct handle identity (good), but the mutable sub-object graph is not independent.

## Wall (the factory wall, final form)
Fully solving = deep-clone the engine-owned, interlinked view sub-object graph (RT-pool refs, pass scratch, counters) — each crash reveals another mutated sub-object. This is what the engine itself does when it creates a 2nd view (inventory preview, builds/frame=2), via generic registered-type/job machinery with no callable entry. A manual clone cannot cheaply reproduce a fully-independent view object graph.

Sub-object class at h+0x40/h+0x60 = vtRva 0x2E92718 (generic, 8+ ctors). h+0x48/0x58/0x68 = small refcount nodes {1,1}. View-handle ctor = sub_140294C94 (shell only; sub-objects wired externally).

Relates: [[cp2077-eb4-unhookable-fc8-lever]] [[cp2077-view-fanout-submit-chain]] [[cp2077-rtt-no-drivable-api]] [[cp2077-per-view-independent-fgbuild]]
