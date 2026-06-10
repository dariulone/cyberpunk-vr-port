---
name: cp2077-clone-crash-downstream-collection
description: F12 clone-submit crash root-caused = concurrent-append race on a downstream registered-type collection DynArray, unreachable from handle => handle-cloning exhausted
metadata:
  type: project
---

DECISIVE static decode of the F12 eye-2 clone crash (full 0x2000 clone, builds/frame doubling lever).

Fault = WRITE AV at game rva 0x24D4B2, inside sub_14024CAC8 (0xA0E). The block `mov [rcx+8],r14` is the out-of-line tail of an append loop at ~0x24CE40:
```
mov eax,[rdi+0x0C]   ; DynArray count/cap
mov r8,[rsi+0x10]    ; entry.ptr source
lea rcx,[rax+rax*2]  ; idx*3 -> stride 0x18
mov rax,[rdi]        ; rax = DynArray buffer base
mov [rax+rcx*8+0],r8 / +8,r14 / +0x10,dl  ; entry {ptr,backptr,flag}
```
So it appends `{ptr,backptr,flag}` (stride 0x18) into a DynArray (`[rdi]`=base, `[rdi+0x0C]`=count). rcx torn (0xAF000000B7 = {0xB7,0xAF}) = element addr OOB from torn count/base = concurrent append by eye-1 + eye-2.

`this`(rdi) = arg0 of the only direct caller sub_1401EDA84 (rcx->rdi; rdx=arg2). sub_1401EDA84 is itself registered-type (data-refs from descriptor tables 0x14320295c / 0x14494dd9c -> function cluster 0x24Cxxx).

Crucially this collection is NOT:
- the view-handle (h+0 = vtable 0x2AC8688), NOT its sub-objects ([[cp2077-view-item-layout]] 0x2E92718 at h+0x40/+0x60),
- NOT the view registry (vtable 0x2B57F58 — its slots dumped, sub_1401EDA84 absent).

It is downstream per-build collection-builder state, shared between the two concurrent builds because our submit-clone shortcut reuses the same downstream dispatch/keying. **Unreachable from the handle => handle-deep-cloning cannot fix this fault.** The engine's own 2 builds (inventory builds/frame=2) coexist because the real per-view producer allocates each build its own collection; our clone-submit bypasses that. Confirms [[cp2077-eye2-clone-shared-subobj]] at a deeper, precise level. Native simultaneous now converges (again) on driving the engine's own per-view producer, not cloning. See [[cp2077-view-fanout-submit-chain]] [[cp2077-eb4-unhookable-fc8-lever]].
