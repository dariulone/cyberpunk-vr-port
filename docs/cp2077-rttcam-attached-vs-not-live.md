---
name: cp2077-rttcam-attached-vs-not-live
description: live RPM diff of attached vs non-attached entRenderToTextureCameraComponent — exact attach gap + handle object layout, corrects the generator@+0x88 myth
metadata:
  type: reference
---

Live capture (RPM, inventory open, 2026-06-08). Heap-scanned for entRenderToTextureCameraComponent **instance vtable = exe+0x307BFD0** → found 20 instances. Exactly ONE was attached (the inventory preview); the other 19 are the same class but inactive.

**CORRECTION:** comp+0x88 is NOT a generator pointer (old [[cp2077-resref-autoload-dead]] / dyntex notes were wrong). It is a FLAG word. attached=0x01000003, non-attached=0x00000003. Bit **0x01000000** = render-attached/has-handle. Low bits 0x3 = enabled/visible.

Attached vs non-attached are structurally IDENTICAL (same +0x30 type-desc exe+0x4402C78, same +0xA0 exe+0x3075268, same +0x120 exe+0x2B03A70, same +0x78=0x100000001, +0x80=1, same near/far +0x168). Only real differences:
- comp+0x88: flag bit 0x01000000 set
- comp+0x1E0: resref hash (both have one; working = 0xD55FB557EAEBA05B)
- **comp+0x1E8: the GPU render-texture HANDLE** (null on non-attached) — THE crux field
- comp+0x1F0: an extra render-state descriptor block (null on non-attached); not a vtable object, just dwords/counters

**Handle object** (comp+0x1E8 → 0x1F89F737740) = the thing our script cam lacks:
- +0x00 vtable = exe+0x2B1BAA0 (GPU render-texture handle class; ctor = sub_1412A3CC0)
- +0x30 / +0x80 = resref hash (same as comp+0x1E0), +0x38 = refcount 1
- +0x40 = 0x2D0,0x2D0 = **720x720** (preview res); +0x48=1
- +0x54 = 0x00010006 (pixel format)
- +0xB0 = exe vtable ptr, +0xB8 = GPU-backing sub-object (0x1F89F7377F0)

Conclusion: render-attach = engine creates handle (class 0x2B1BAA0), GPU-backs it 720x720 fmt 0x10006, writes ptr to comp+0x1E8, allocates +0x1F0 block, sets flag 0x01000000. Happens when the cam becomes active in a rendered view. See [[cp2077-live-rpm-and-attach-pivot]].

**Factory chain reversed (Ghidra, tools\ghidra):**
- handle ctor = 0x1412A3CC0: sets [obj]=vtable 0x142B1BAA0, [+0x54]=0x10006, [+0x40/+0x44]=0x100 (default size, later patched to 0x2D0=720), [+0x98]=0x143009280; zeroes up to +0xA0. **Does NOT set +0xB0/+0xB8 GPU-backing** → GPU backing comes later from enroll/generate pass (the known generator-gap [[cp2077-rendertex-needs-generator]]).
- ctor 0x12A3CC0 has exactly ONE caller: 0x14221BFA3, inside factory ~0x14221BF62 = CreateRenderTextureRef(): allocs Ref RDI, calls ctor, assigns handle via Ref<>::operator= (0x1401456C0, a refcount-assign, NOT a +0x1E8 store), returns Ref.
- So comp+0x1E8 store happens further up, in the caller of the factory (not yet reversed).
- Factory entry = 0x14221BF64 (2 INT3 pad before). Inside: alloc 0xA8 via 0x14102B740, **CALL 0x141908820 = GPU-backing allocator** (adjacent to known +0xB0 writer 0x141908809 [[cp2077-gpu-backing-allocator-found]]) — THIS is why ctor-only bottom-up attempts never GPU-backed. Then ctor 0x12A3CC0, then Ref-assign.
- Factory has exactly ONE caller: 0x141E8ECF7. Decoded raw: `LEA RCX,[RBP+0x20]` (stack-local Ref dst) → CALL factory → `MOV RDX,RAX; LEA RCX,[RBX+0x268]; CALL ...` i.e. the new RT-Ref is then stored into **[RBX+0x268]**, where RBX = a per-camera render-PROXY created on activation (NOT the component). comp+0x1E8 references the same handle. Fits finding B: handle lives in an activation-time render-proxy. Enclosing fn at 0x141E8Exxx is large (no INT3 pad within 0x8000 back) and called INDIRECTLY (vtable) — 0 direct E8 callers. RBX in that fn = the render-PROXY.

**Render-proxy identified LIVE (RPM scan for handle value H):** with inventory open, scanned memory for the handle ptr → besides comp+0x1E8 and H-internal, found proxy X with [X+0x268]=H. Proxy class vtable = **exe+0x3009130** (methods cluster in 0x20Dxxxx = render code); object repeats its vtable at +0x00/+0x10/+0x20/+0x30/+0x40 (render-scene-node, multi-slot); +0x270=aux obj, +0x278=exe+0x30283D0 descriptor. This proxy is created on activation and drives the RT. Next: with full Ghidra analysis (running), query getFunctionContaining(0x141E8ECF7) + xrefs to vtable exe+0x3009130 (where proxies are created/linked to the cam) = the activation trigger. Tools: tools\find_proxy.ps1 (live), tools\ghidra\.

**DECISIVE TEST RESULT (B confirmed):** closing inventory → same cam object 0x1FCCA0459C0 (stable addr+resref 0xD55FB557EAEBA05B) PERSISTED but DETACHED: +0x1E8 handle 0x1F89F737740→0x0, +0x88 0x1000003→0x3. So attach is a lifecycle/visibility-driven state: engine allocs+GPU-backs the handle when the cam is active in a render view, releases it when not. => Manual handle construction is a DEAD END (engine lifecycle would release it). Lever = get our script cam into the "actively rendered" state; engine then auto-attaches+GPU-backs. Next reverse target = the activate/attach fn (caller of factory ~0x14221BF62 that stores comp+0x1E8).

**Strategic fork (RESOLVED → B):** (A) manually build handle via factory + wire comp+0x1E8 + set flag 0x01000000 from plugin, hope engine's generate pass then GPU-backs it; vs (B) the 19 non-attached cams are configured+structurally identical to the attached one, only difference is being actively rendered → attach may be purely visibility/active-view driven (the RTT-driver problem [[cp2077-rtt-no-drivable-api]]). Decisive cheap test: poll comp+0x1E8 via RPM while toggling a cam's on-screen visibility to see if handle appears/disappears with rendering.
