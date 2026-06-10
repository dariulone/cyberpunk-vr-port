---
name: cp2077-producer-addview-callsite
description: option-C simultaneous — 0x212843 insert proven BENIGN (cull visitor); real producer = sub_140293978 @0x292d06/0x29a7a0, double-call test built
metadata:
  type: project
---

Native simultaneous attack: drive the engine's OWN view-producer instead of cloning handles.

**NEGATIVE result on first target (0x212843):** call-site double at `0x212843` inside `sub_140212800` (`call sub_140212780`, per-view insert into builder inline array @+0x48 / count @+0xD0) ran clean — **25310 inserts, NO crash, but builds/frame stayed 1**. So `sub_140212800` is the generic cull/visibility visitor (vtable 0x2AC27F8, transient, 6+ worker threads), NOT the build fan-out driver. Benign no-op for build count. Hook left in code (`InstallViewInsertCallSiteHook`, g_viewInsHookDone) but de-wired from F7.

**Real main-scene (Type-B) producer = `sub_140293978`** — constructs a fresh view-item via factory `sub_140293C7C` (own 0xFF0 alloc → normal registration → own arena slot). Called from exactly TWO code sites (IDA xrefs):
- `0x292d06` (in `sub_140292A54`)
- `0x29a7a0` (in `sub_14029A5B0`)
Both identical ABI: `rcx=[rsi+0x320]` (mgr), `rdx`=lea local (view-source/out), `r8`=lea local. 3-arg __fastcall.

**Mechanism:** mid-body CALL patch (`.pdata`-safe — unlike unwind-fatal prologue patches EB4/FGB → base−5). `InstallProducerCallSiteHook(siteRva)` validates target==sub_140293978, trampoline preserves rbx/rsi/rdi: eye-1 producer call always; when `g_dualView` armed (F11), `g_dualViewCount`++ and a 2nd identical producer call → fresh factory-built natively-registered view → DISTINCT arena slot (dodges byte-clone global-arena collision stride 0x5E00 [[cp2077-clone-crash-global-arena-slot]]).

**Both sites patched** (don't know which runs in gameplay; whichever is active gets the double). Risk: an early per-frame guard (`call [rax+0x100]; jg`) inside the producer may no-op the 2nd call.

**Build/deploy:** dxgi.dll 15:33. F7 installs FgBuild (builds/frame metric) + Producer call-site hooks #1(0x292d06) + #2(0x29a7a0); sets g_producerHookDone + g_viewMgrHookDone (F11 arms, reuses g_dualView window + g_dualViewCount).

**Test:** F7 (gameplay, not inventory) → wait 3s baseline → F11 → ~4s. Verdict:
- builds/frame 1→2 + DUAL-VIEW DONE injections>0 + SURVIVED = drivable native 2nd view (the win).
- injections>0 but builds/frame=1 = producer's per-frame guard dedups the 2nd call.
- crash → fault addr shows what the 2nd factory-built view collides on downstream.
