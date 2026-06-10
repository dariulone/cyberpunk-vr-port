---
name: cp2077-producer-double-mutates-mgr
description: DECISIVE NEGATIVE — Type-B producer sub_140293978 mutates shared per-frame mgr state; double-call tears it (black screen + async null-crash). Producer-double dead.
metadata:
  type: project
---

**Producer-double (call sub_140293978 twice per frame) is DEAD — confirmed by disasm + live test.**

Live test (F11, both call-sites 0x292d06 + 0x29a7a0 patched, eye-2 via SEH wrapper Eye2ProducerCall):
- **black screen then crash.** No `[EYE2-PROD-FAULT]` line => fault is NOT in the synchronous producer call (SEH didn't catch it) — it's in an ASYNC build-job on another thread.
- First fault: `sub_140A9B0F4` rva 0xA9B1B7 `cmp [rax+0xC], r10d` with **rax=null** (READ target=0xC). sub_140A9B0F4 is a per-thread async render job (TLS gs:[0x58], slot 0x1A8). Then base−5 unwind noise masks the rest.

**Why (disasm of sub_140293978):**
```
mov r13, rcx                 ; r13 = mgr (shared per-frame manager, = [rsi+0x320] at call-site)
mov rcx,[rcx+8]; mov rax,[rcx]
call [rax+0x100]; test eax; jg far   ; per-frame GUARD (skips body if >0)
call sub_140293C9C           ; build view
mov [r13+0x2B8], rcx         ; *** WRITES shared mgr+0x2B8 ***
cmp [r13+0x1C0], sil ; mov edi,[r13+0xD0]   ; reads mgr+0x1C0 / +0xD0
```
The producer MUTATES shared mgr (`+0x2B8`) and is gated by a per-frame guard. Two calls on the same mgr tear its per-frame state => eye-1's own main view corrupts (black screen) + async build reads torn/null mgr-derived field => crash. It does NOT build an independent 2nd view.

**Convergence:** non-idempotent shared per-frame state kills the "double an existing call" family at EVERY level now: node WRITE AV, driver READ AV, view re-invoke no-op, FG-build base−5, byte-clone arena-slot collision, and producer-double mgr-tear. The engine never natively does 2 main-scene (Type-B) builds in one frame.

**Key reframe for next direction:** even inventory's builds/frame=2 is Type-A(preview/FC8 sub_140167FC8) + Type-B(main sub_140293568), NOT two Type-B. The ONLY native vehicle that renders a full 2nd scene-view to an offscreen RT every frame is the **Type-A / preview pipeline**, which is merely EMPTY in gameplay (FC8 doesn't fire). Driving a right-eye view into the Type-A view collection (so the engine renders it via its working preview/RTT path) is the remaining native lever — vs forcing a 2nd Type-B which corrupts shared state.

Hooks left in code (InstallProducerCallSiteHook, Eye2ProducerCall) but F11 path now known-dead; F7-install itself is stable (eye-1 passthrough harmless), only F11 crashes.
