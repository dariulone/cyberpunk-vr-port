---
name: cp2077-rtt-view-create-fn
description: VERIFIED native view-create sub_1404FBAFC(rttCameraComponent); ENTIRE cluster (wrapper+init+gate+view-create) CONSUMES the handle, never allocates it — so view-create sits BEHIND the handle-alloc wall (DECISIVE, byte-proven); wrapper real vtable=0x142B03A70 slot8/off0x40 (NOT 0x142B03A80 slot6); calling wrapper from CET/script thread DEADLOCKS
metadata:
  type: project
---

**Native engine fn that creates a full offscreen scene-view from an RTT camera component.** Byte-verified this session (read_memory_bytes, no decompile — IDA crashes on decompile AND disassemble; only graph/byte reads are safe). Matches prior memory exactly.

**`sub_1404FBAFC(rcx = entRenderToTextureCameraComponent)`** (0x3ef bytes):
```
mov rbx,rcx                  ; arg0 = RTT camera component
call sub_140665054           ; GATE (render-active); test al; jz tail (no-op on fail)
mov eax,[rbx+0x258]          ; width
mov rcx,[rbx+0x1E8]          ; HANDLE (GPU render-texture)
mov eax,[rbx+0x25C]          ; height (companion to 0x258)
... mov rcx,[rip+0x2F2BE19](renderer singleton); mov rax,[rcx]; call [rax+0x450]  ; engine registers the view
```
Builds a Type-A view-item via ctor `sub_140294C94` (vtable 0x2AC8688). Confirmed via get_callees: gate sub_140665054 + ctor sub_140294C94 both present.

**Native entry = wrapper `sub_140AC316C` (NOT raw fn).** CORRECTED vtable (live-scan + byte-confirmed 2026-06-09): real producer vtable = **`0x142B03A70`, the wrapper is at offset 0x40 = slot 8** (prior "0x142B03A80 slot6" was off by 0x10/2 slots; arithmetic still consistent: 0x142B03A80+6*8 == 0x142B03A70+0x40). Read vtable bytes @0x142B03A70: slot0/1=0x14014A700, s2=0x1412BAAA0, s3=0x140664F98, s4=0x140127F58, s5=0x1418F3060, s6=0x1418E78D0, s7=0x14014A700, **s8(off0x40)=0x140AC316C(wrapper)**. The code safety-check compared subVt against +0x2B03A80 (wrong) and rejected every cam — must use +0x2B03A70. Logic:
```
mov rdi,rcx; call sub_140128690
lock cmpxchg [rdi+0x8E0],ecx ; ONE-TIME-INIT guard
lea rbx,[rdi-0x120]          ; comp = rdi - 0x120  (engine holds ptr to comp+0x120 sub-obj)
if first-time: call sub_140AC2BA4(...)   ; one-time init (runs BEFORE gate)
call sub_140AC31C4           ; leaf helper
mov rcx,rbx; jmp sub_1404FBAFC           ; -> view-create with rcx=comp
```
So faithful native call = `subobj->vtable[0x40](subobj)` (slot 8) where subobj=comp+0x120, *(subobj)==0x142B03A70. No static callers (pure virtual dispatch). The wrapper→view-create link is a TAIL-JMP (e9 @0x140AC31BD → sub_1404FBAFC), confirmed via get_xrefs_to(0x1404FBAFC) code-xref inside sub_140AC316C — that's why get_callees doesn't list it.

**CORRECTION to prior optimism — gate `sub_140665054` (0x5d bytes) decoded:** returns TRUE iff ANY of:
1. `[comp+0x1E8] != 0` (HANDLE), OR
2/3/4. `[comp+0x200]`/`[comp+0x218]`/`[comp+0x230]` non-null AND `sub_14014B208(it)`==true (handle-valid predicate; 0x200/0x218/0x230 likely MRT G-buffer handles).

=> view-create passes ONLY if a render-texture handle already exists. So **`sub_1404FBAFC` does NOT bypass the handle-allocation wall — it sits BEHIND it.** Prior note "script comps CAN pass gate" is imprecise: idle isEnabled cams (handle=0, [[cp2077-isenabled-necessary-not-sufficient]]) FAIL the gate (no-op, no crash); only already-render-active cams pass. The long-standing linchpin (allocate/back the handle via render-ATTACH, [[cp2077-resref-autoload-dead]]) is unchanged.

**RESOLVED NEGATIVE (2026-06-09, byte-decoded):** the one-time-init `sub_140AC2BA4` (0x5c7) does NOT allocate the handle. Scanned its full body for writes to the gate slots on comp(=rbx): the ONLY reference to comp+0x1E8 is `mov rcx,[rbx+0x1E8]` (READ/consume), and there are NO writes to comp+0x200/+0x218/+0x230 (all `..30 02 00 00` hits are rbp+0x230 = local 0xC50 frame, not rbx). Init reads the existing handle + comp+0x258/0x25C (resolution) and builds view structures in its local frame; it calls comp's own vmethods ([rax+0x288]/+0x290/+0x250/+0x450 where rax=[comp]) but the early READ of comp+0x1E8 shows the handle is expected to pre-exist. Likewise sub_1404A04AC (camera-update/view-spec emit, 0x406) only READS comp+0x268/+0x270 (copies them to an output struct w/ lock-inc refcount), no +0x1E8 write. => **The whole view-create cluster CONSUMES the handle; none of {wrapper, init sub_140AC2BA4, gate, view-create sub_1404FBAFC, activation sub_1404A04AC} creates it.** Handle allocation is genuinely upstream in render-ATTACH only. Calling the wrapper on an idle cam (handle=0) is now PROVEN futile (gate no-ops). The real & only linchpin = render-attach handle creation, unchanged.

**ACTIVE TEST RESULT (2026-06-09): calling the wrapper from the CET/script thread DEADLOCKS the game (froze, no crash).** First impl (`DriveRTTViewCreate` in CyberpunkVRPort main.cpp) called `sub_140AC316C(comp+0x120)` synchronously on an already-active cam (mode 1) from the CET native thread -> permanent hang. Mechanism (theory, unconfirmed by stack): view registration via renderer[0x450] enqueues a render command and waits; the render thread is itself waiting on the blocked main thread -> deadlock. (Also the 2TB heap scan ran on the main thread = frame freeze on its own.) => **the wrapper canNOT be called from an arbitrary thread / mid-frame phase.** It must be issued from an engine-thread hook in the engine's own safe activation phase. Consistent with the project-wide rule that producer-side ops need engine dispatch context.

`DriveRTTViewCreate(mode)` was then made READ-ONLY + off-thread (logs candidate cams: cp, flag88, prodVt match at comp+0x120, handles; NO wrapper call). Safe to re-run; writes rttcam_viewcreate.txt next to Cyberpunk2077.exe (bin\x64). mode 0=all,1=active-only,2=idle-only.

**Next (redirected by the negative result):** the view-create lever is exhausted as a handle-allocation path — stop testing wrapper-on-idle-cam (statically futile) and don't bother live-firing the wrapper for handle purposes. Refocus entirely on the handle WRITER = render-attach allocation site (who writes comp+0x1E8). Two ways to find it: (a) IDA needle-search for the render-texture ctor (sub_1412A3CC0, class vtable 0x2B1BAA0) call-site that stores into a component +0x1E8 — hard via MCP, no struct/field xrefs; or (b) live HW watchpoint on comp+0x1E8 of the active inventory-preview cam (cp=0x22945cc4900 had handle=0x2253306d360) to catch the writer during attach — the method that worked before ([[cp2077-gpu-backing-allocator-found]] caught +0xB0 via HW watchpoint). The handle writer IS the render-attach we've chased for many sessions.
