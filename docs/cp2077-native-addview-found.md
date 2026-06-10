---
name: cp2077-native-addview-found
description: BREAKTHROUGH — native AddView path found (sub_140212780 appends a view to the builder's inline array); calling it for a right-eye desc = simultaneous via ONE natural FG-build, no re-invoke
metadata:
  type: project
---

Captured the view-registry **populator** live via the populate hook (sub_1402251F4 entry-subobject ctor, F7, 24 stable captures, no crash). Caller chain (tid=worker, bottom = job plate 0x14324C/0x16D50E/0x1A3ABF4 — same as FG-build) led to the producer cluster. Decoded it:

- **`sub_140212800`** = populate loop: iterates a view-source list (fields src+0x18/+0x20/+0x28/+0x30), builds a small descriptor on stack, calls the add-view fn per source. `rcx`=builder, `rdx`=source-node.
- **`sub_140212780`** = **native AddView** (`rcx`=builder, `rdx`=&descriptor). Checks via `sub_1402127B4` whether to add, then appends.
- **`sub_1402127B4`** = the append/bookkeeping: builder has an **inline view array at +0x48/+0x50** (stride 8, element[idx] at +0x48+idx*8; +0x48 dword=flags set to 1, +0x4C dword cleared), **count at +0xD0** (incremented). This is the value-inline storage that made entries unreferenced (refs=0).

**Why this is the path to simultaneous (vs every dead lever):** calling `sub_140212780(builder, &rightEyeDesc)` during populate appends a 2nd view through the engine's OWN add path (maintains all invariants), so the SINGLE natural FG-build (no re-invoke → dodges node WRITE AV / driver READ AV / view no-op / FG-build base−5 in [[cp2077-inline-replay-dead-all-levels]]) iterates count+1 views and renders the right eye into auto-allocated RTs.

Open items before driving it: (1) live builder address + the descriptor layout (rdx) — capture by hooking sub_140212780/sub_140212800; (2) clone main-view descriptor with right-eye pose (AER LocateCamera mechanism); (3) RT routing of the 2nd view to the right-eye swapchain. Builder may be the FrameGraph builder (FG-build arg0). Supersedes fragile hashmap injection ([[cp2077-registry-injection-feasibility]]).

**LIVE UPDATE (ViewSrc hook on sub_140212800, F7, 12 captures, no crash) — tempers the optimism:** builder is a **stack-transient object (vtable exe+0x2AC27F8, 3 ctors = generic registered-type)**, and sub_140212800 runs **massively parallel** (6+ worker tids simultaneously), each with its own stack builder, **count@+0xD0 = 2** everywhere. builder+0x40 = scaleXY(1.0,1.0) like view entries. So sub_140212800 is a GENERIC parallel collection builder (culling/visibility/view lists), NOT a single stable main-view registry. Captured src node had +0x18..+0x38 = 0 (add branches skipped → count=2 was pre-existing). Driving it for a right-eye MAIN view is only possible by an IN-HOOK append to the specific main-view builder instance (identify by a view descriptor whose camera vtRva==0x2B72EE8), then call sub_140212780 with a cloned+IPD-shifted descriptor — uncertain (must isolate the right builder among parallel calls, resolve the 16-byte descriptor format, survive downstream build). Same generic/transient/parallel wall in a new form.
