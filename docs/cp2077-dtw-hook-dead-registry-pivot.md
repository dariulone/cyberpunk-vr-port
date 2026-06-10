---
name: cp2077-dtw-hook-dead-registry-pivot
description: dtw-hook sub_1412BAACC confirmed dead (4x base-5 AV); pivot to no-hook read-only renderer-registry walk to capture preview generator
metadata:
  type: project
---

The DynTexWire hook on `sub_1412BAACC` (intended to catch the engine's live puppet-preview
RTT component on a worker thread to read its generator@comp+0x88) is a **confirmed dead-end**:
4 instrumentation attempts (E9 ×2, INT3 ×2) all crash at inventory open with a `base-5` AV
(`0xC0000005`, rip in a system module) — the fn runs concurrently on render job-workers in an
unwind-sensitive context and is not safely hookable. See [[viewsched-hook-crash]].

**Build made stable (2026-06-08):** in `src/dxgi_proxy.cpp` the arm flags are commented out
(`g_dynTexHookPending`/`g_ctorHookPending`, ~lines 5470/5476) so neither dtw nor ctor hook installs
→ inventory no longer crashes. Rebuilt `dxgi` + redeployed to game `bin\x64`.

**Pivot = NO-HOOK read-only renderer-registry walk.** New global fn `DumpRenderTexRegistry`
(red4ext plugin `main.cpp`, callable `Game.DumpRenderTexRegistry()` from CET): resolves
`renderer=*(exe+0x3427C00)`, `texMgr=*(renderer+0x70)` (see [[cp2077-render-texmgr-handle]]),
dumps renderer[0..0x200] + texMgr[0..0x600] as qwords, and scans both for array triples
(ptr,count,cap) — following each candidate pointer-array and dumping element vtable RVAs. Goal:
find the dynamic-texture/generator list (a cluster of same-vtable heap objects) ENTITY-AGNOSTICALLY,
so we capture the engine's live preview generator without any code patch. Pure SEH-guarded reads →
cannot crash. Run with inventory puppet-preview OPEN; writes `rendertex_registry.txt`.

**CORRECTION after Ghidra disasm (2026-06-08):** `texMgr+0x20..0xC38` is **not** an inline render-
texture/generate-list. It is a family of **render command queues** (0x18-byte queue slots). Each slot
is roughly `{headChunk, currentChunk, sync/lock}`; helpers pass `R8=texMgr+slot`, use `[R8+8]` as the
current 0x4010-byte command chunk, append packets at chunk cursor `+0x4000`, and link chunks through
`+0x4008`. Packet layout: `uint16 size`, `qword typeTag` at `pos+2`, payload at `pos+0x0A`. The
apparent `obj+0x10 == rva 0x3133338` was a command packet type tag inside a chunk, not a texture
object field. OPEN/CLOSED changes still prove inventory preview affects texMgr command traffic, but
the previous “58 textures in generate-list” interpretation was wrong.

**The dump fn does NOT crash in-body** — `DumpRenderTexRegistry` ran to completion and the crash hit
AFTER it returned. `DumpRenderTexEntries` 2nd run also completed (file written) but the game FROZE:
root cause = `out.flush()` per line × 3152 lines, executed on the game MAIN thread (CET console runs
in-frame) → multi-second frame stall → hang/TDR. The reads complete fine; only I/O cost mattered.
The freeze happens REGARDLESS of I/O: even buffered single-write FROZE the game mid-read and the
final write never ran (file unchanged) — so the stall isn't disk, it's the **game main thread**
(CET console runs in-frame) blocked while reading the live render-thread-mutated registry → GPU TDR
/ freeze. FINAL FIX (applied): `DumpRenderTexEntries` now launches `RenderTexEntriesWorker()` on a
**detached `std::thread`** and returns instantly; main thread keeps presenting → no freeze. Worker
buffers + writes once; reads stay `VirtualQuery`+SEH-guarded against concurrent entry teardown.
Lesson: heavy game-memory reads from CET globals must run OFF the main thread.

New diagnostic: `Game.DumpTexMgrQueues()` (off-thread/read-only) decodes the real queue chunks/tags.
Diff `texmgr_queues_CLOSED.txt` vs `texmgr_queues_OPEN.txt` to identify which command tag appears for
inventory preview, then map that tag to a texMgr wrapper/helper from Ghidra disasm. Static anchors:
`ResizeDynamicTexture` is vtable[80] `0x14291A4D4`, command vtable `0x1431D8548`, submit
`0x142910480`. Queue helper examples: `0x1429109A8`, `0x1429129C8`, `0x1429152A4`; they write generic
tag `0x143133338` then final specific tags (`0x143133468`, `0x1431333E8`, `0x1431333F8`, etc.).

**New RTT-specific caller chain (2026-06-08):** static disasm around the packet wrapper found a live
inventory/RTT-looking path:
- `0x140CAF780` reads `R9+0x1E0` and `R9+0x252`, compares `R10+0x40==2`, then calls `0x140CAF7D4`
- `0x140CAF7D4` checks `RCX+0x23 & 1`, obtains `renderer = *qword_143427C00`, calls `renderer->vtable[0x450]`,
  then invokes a function pointer at **`[obj+0x98]`** with ABI:
  `fn(rendererV450Ret, &obj+0x08, &obj+0x10, R9B=modeFlag, [rsp+20]=bool)`
- That callee signature matches `0x140CAF834` exactly (the mapped `tag=0x143133418`, `p0=0x142B89280`
  texMgr wrapper). This is the first concrete static chain from an RTT-ish object to the enroll packet.

This is strong evidence that the real crux is not “construct generator@comp+0x88 manually”, but
reach the same wrapper path that uses the object/function-pointer at `+0x98` to emit the RTT command.

New follow-up diagnostic deployed: `Game.DumpTexMgrRttTargets()` (off-thread/read-only). It scans
decoded texMgr queue packets, filters RTT packets `(tagRva in {0x3133338,0x3133418}, p0=0x2B89280)`,
and dumps each packet's `p1` target object (`0x00..0xC0`, plus `p1+0x98`). Goal: determine whether
`p1` is the GPU handle, a wrapper object, or a descriptor carrying the callable pointer into
`0x140CAF834`.

Existing RTTI walks (`DumpRTTCamVtable` etc.) only iterate the **player** entity's components, so
they never reach the preview-puppet's render-attached component — that gap is why a hook was used,
and why the registry walk (renderer-side, owner-agnostic) is the right no-hook substitute.
Continues [[cp2077-resref-autoload-dead]].

Build/deploy: plugin = `src/red4ext_plugin/build` (Release) → `red4ext/plugins/CyberpunkVR_Hands/`;
dxgi = `build-vs` (Release, target dxgi) → game `bin\x64` via `tools/Install-ToCyberpunk.ps1`.
cmake at VS2022 Community `Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe`.
