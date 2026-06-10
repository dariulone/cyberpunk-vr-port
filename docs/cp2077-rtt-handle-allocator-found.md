---
name: cp2077-rtt-handle-allocator-found
description: sub_1404A04AC (RTT activation) creates a render-texture into comp+0x268 — BUT live RPM proved +0x268 is a DIFFERENT producer's slot (0x307D690), NOT the gate-checked +0x1E8 used by the preview producer (0x2B03A70) the wrapper drives. So this allocator does NOT unlock the view-create gate. The preview-path handle@+0x1E8 comes from resolving resref@+0x1E0 during render-attach (flag88 0x3->0x1000003). Idle preview cams already carry a valid resref.
metadata:
  type: project
---

**LIVE RPM CORRECTION (2026-06-09) — read FIRST:** probed 3 live RTT cams. There are TWO producer pipelines sharing component vtable exe+0x307BFD0:
- **Preview producer (subobj@comp+0x120 vtable = exe+0x2B03A70)** — the one the wrapper sub_140AC316C drives. Handle lives in **comp+0x1E8**, resolved from a depot resref at **comp+0x1E0**. comp+0x268/+0x270 are ALWAYS NULL for this type.
  - Active preview cam (L19): flag88=0x1000003, resref+0x1E0=0xD55FB557EAEBA05B (hash), +0x1E8=render-texture (vtable exe+0x2B1BAA0, GPU-backed), 720x720, +0x268=0.
  - Idle preview cam (L3): flag88=0x3, resref+0x1E0=0x29103B6637D9D5B4 (a VALID hash — idle cams already carry a resref!), +0x1E8=0, all slots 0.
- **Other/render-target producer (subobj vtable = exe+0x307D690)** — different cam type. Handles in comp+0x200/+0x230, view pair in comp+0x268/+0x270 (populated). THIS is the producer sub_1404A04AC's create-into-+0x268 branch serves.

=> **sub_1404A04AC creates into +0x268 which belongs to the 0x307D690 producer, NOT the +0x1E8 the gate/wrapper need.** So the "callable handle allocator" below does NOT solve the preview-path gate. For the preview path, the handle@+0x1E8 is produced by resolving resref@+0x1E0 during render-attach (the flag88 0x3->0x1000003 transition). Idle preview cams ALREADY have a valid resref@+0x1E0, so the wall is purely "trigger the resref->handle resolution / render-attach", consistent with [[cp2077-resref-autoload-dead]] + [[cp2077-rttcam-isenabled-is-the-flag]]. NEXT LINCHPIN = find the fn that resolves resref@+0x1E0 into handle@+0x1E8.

**RESOLVED (2026-06-09): the resolver is NOT a dedicated callable — it's the generic RED4 async resource loader fulfilling a `raRef<RenderTexture>`.** Proof: the live handle object @comp+0x1E8 stores its OWN resref hash at handle+0x30 (== comp+0x1E0 = 0xD55FB557EAEBA05B) — i.e. comp+0x1E0/+0x1E8 is a standard resource reference (path-hash + resolved Handle), not an RTT-specific slot. The handle fills when the raRef is requested (LoadAsync), which happens during component/entity render-attach => LIFECYCLE-BOUND, not a free call. Moreover the preview cam's resref is NOT a static asset: it's a RUNTIME hash of a dynamic render-texture the UI preview controller creates+registers via the texture manager ([[cp2077-render-texmgr-handle]], [[cp2077-dyntex-live-recipe]]). So "resolve the resref" reduces to the SAME wall as [[cp2077-rendertex-needs-generator]]: the lever is "create+register a dynamic GPU render-texture so our cam's raRef resolves", not a resolver call. This thread is closed as confirming (not breaking) the wall.

--- original finding (still true for the 0x307D690 producer, kept for reference) ---

**Found the native, callable handle-creation site for RTT cameras (2026-06-09, IDA byte-decode, no decompile).** This is the render-attach handle allocation the project has chased for many sessions ([[cp2077-resref-autoload-dead]], [[cp2077-rendertex-needs-generator]], [[cp2077-isenabled-necessary-not-sufficient]]).

**Call chain (bottom-up, byte-verified):**
- render-texture ctor `sub_1412A3CC0` (builds GPU render-texture, class vtable 0x2B1BAA0 — [[cp2077-render-texture-ctor]])
- ← only caller: factory wrapper `sub_14221BF64` (0x5d)
- ← only caller: **`sub_1404A04AC`** (RTT activation / camera-update, 0x406 + cold chunks in far-code 0x141E8xxxx). The factory call is in a COLD branch at **0x141E8ECF7**:
  ```
  call sub_14221BF64          ; create render-texture
  mov  rdx, rax               ; rdx = new handle
  lea  rcx, [rbx+0x268]       ; comp+0x268   (rbx = comp throughout)
  call <smart-ptr assign>     ; comp+0x268 = new render-texture
  ```
  The cold block (start 0x141E8ECD1, xref'd from main body @0x1404A0782 and @0x1404A07A3) is reached when a computed camera EXTENT is zero:
  ```
  0x1404A0755 mov al,[comp+0x240]; cmp al,1; jnz <other cold>   ; precondition comp+0x240==1
  0x1404A0769 call sub_1404A17D8                                ; computes wanted extent into [r12]
  0x1404A076E movss xmm0,[r12+8]; subss xmm0,[r12]; cvttss2si rax; test eax,eax
  0x1404A0782 jz 0x141E8ECD1   ; CREATE RT if delta_x == 0
  0x1404A0788 movss xmm2,[comp+0x248]; movss xmm1,[comp+0x250]; subss; cvttss2si; test
  0x1404A07A3 jz 0x141E8ECD1   ; CREATE RT if delta_y == 0
  ```
  So the create-branch fires when the camera's render extent is 0 on either axis (dimensions uninitialized = a freshly-activated/idle cam's state); the cold block then loads a default-dims const vector and creates the RT. Precondition: comp+0x240==1.
- ← `sub_1404A04AC` is invoked (near-called x2) by **`sub_14049F21C`** (0xaa). `sub_14049F21C` has NO direct callers => pure vtable dispatch. It sets up **TLS job-state** (`mov rcx, gs:[0x58]; mov dword [comp + tls_slot], 8`), guards on `comp+0x290`/`comp+0x240` bytes, then calls the activation. The TLS/job-state strongly implies this runs in the engine's **render/job thread** context, NOT an arbitrary thread.

**Main-body data flow:** after activation, `sub_1404A04AC` reads `comp+0x268`/`comp+0x270` and copies them (with `lock inc` refcount) into an output view-spec struct (rdi). So +0x268/+0x270 = the view/RT pair the activation emits. It also reads `comp+0x258/0x25C` (resolution) and many comp fields for camera matrices (FOV/aspect/near-far).

**IMPORTANT distinction (unresolved):** the gate in the OTHER path `sub_1404FBAFC` (the wrapper/view-create, [[cp2077-rtt-view-create-fn]]) checks `comp+0x1E8/+0x200/+0x218/+0x230`, NOT +0x268. So there appear to be TWO RTT stages: (A) `sub_14049F21C`→`sub_1404A04AC` = activation that CREATES the RT into +0x268 and emits the view-spec; (B) `sub_1404FBAFC` = view-register gated on +0x1E8. Need to confirm whether filling +0x268 (path A) alone makes the camera render, or whether +0x1E8 must also be populated for the engine to pick it up. The live active inventory-preview cam had +0x1E8 populated (0x2253306d360); did NOT yet read its +0x268.

**OPEN QUESTIONS before trusting this as the True-Stereo lever:**
1. ANSWERED (partially): ctor `sub_1412A3CC0` here builds only the CPU-side descriptor, no GPU backing at this instant. Decoded factory `sub_14221BF64`: allocs a 0xA8-byte object, calls the ctor (which calls just ONE fn sub_14022413C, no DX12), then registers it in an owner. So GPU backing + generator@+0x88 still come downstream — SAME as old [[cp2077-rendertex-needs-generator]]/[[cp2077-resize-not-allocator]]. KEY DIFFERENCE: old attempts built the descriptor OUTSIDE the activation/attach lifecycle so nothing enrolled it in the generate-list; here it's created INSIDE the real per-frame activation (sub_1404A04AC, render thread) that then emits the view-spec — so the engine's own downstream machinery would GPU-back it. => the route to a real GPU-backed handle is to make the engine RUN its activation (sub_14049F21C dispatch) for our cam, not to hand-build the descriptor. That re-confirms render-attach as the lever, but now with the exact engine functions mapped.
2. Thread/phase: like the wrapper, this MUST be issued from the engine render/job thread (TLS job-state). A blind script-thread call will deadlock/crash (same lesson as [[cp2077-rtt-view-create-fn]]).
3. Does creating the RT into +0x268 satisfy what the engine needs to actually render the cam, or is +0x1E8 the script-facing handle that something else fills?

**Next step (NOT a blind call):** confirm via (a) more IDA: decode the validity-check fn before the `jnz cold` to nail the trigger condition, and check whether the activation also wires GPU backing/generator@+0x88; (b) live read-only: on the active preview cam read comp+0x268/+0x270 and compare to +0x1E8 to learn the two-slot relationship. Only after that consider hosting a one-shot `sub_14049F21C(comp)` (or `sub_1404A04AC`) call from an engine-render-thread hook on an idle player-attachable cam, then read comp+0x268/+0x1E8 to see if the RT was created.
