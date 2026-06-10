---
name: cp2077-view-fanout-submit-chain
description: NATIVE per-view FG-build fan-out chain found (static, callable) - sub_140167E28->EB4 loop->FC8 submit; viewArray=[mgr+8], 1 build per item; 2 items => 2 native builds = simultaneous seam
metadata:
  type: project
---

Decoded the Type-A FG-build producer (the main-scene per-view fan-out). All STATIC + callable (corrects the earlier "producer is virtual-dispatch only" conclusion):

- **sub_140167E28(mgr, spec)**: viewArray = `[mgr+0x08]`; builds a stack spec; calls the loop with rcx=`[mgr+0]`, rdx=&spec, r8=viewArray.
- **sub_140167EB4(rcx, rdx=spec, r8=viewArray)**: PER-VIEW LOOP. For each item in viewArray (stride 8, until null/count): `lock inc [item+8]` (refcount), sets arg5=&item, calls submit. Each build job carries ITS OWN view handle (moved from arg5 into payload) - NOT a global. This is the native fan-out: 1 FG-build submitted per view item.
- **sub_140167FC8(rcx, dl=flag, r8, r9, arg5=&item)**: lazy-inits static job-desc (Type-A job-ctx vtable=rva 0x2ABC878), allocates payload, MOVES `[arg5]` (view handle) into payload, enqueues via generic `sub_14014292C(rcx=scheduler-global, rdx=&desc, ...)`.
- **Type-B build** = separate submit `sub_140293568` (job-ctx vtable rva 0x2AC8668), runs in inventory (preview/UI/secondary), NOT the main scene loop.

Runtime confirmation: FG-DUMP of a1(job-ctx)+0x00 vtable alternates Type-A(0x...2ABC878)/Type-B(0x...2AC8668) per inventory frame (2 builds/frame); gameplay=1. Runtime base derived from "FgBuild hook installed at 0x7FF6E1623904 (rva=0xAA3904)" => base 0x7FF6E0B80000.

**Simultaneous lever (next):** make the Type-A viewArray hold 2 items (2nd = right-eye view, IPD-shifted) so the natural loop submits a 2nd native build with its own view+ctx (no re-invoke => dodges all 4 crash levels). Need view-item layout (refcount@+8, vtable@+0, camera/pose ref) - capture via read-only hook on sub_140167EB4. Relates [[cp2077-per-view-independent-fgbuild]] [[cp2077-framegraph-build-chain]] [[redengine-render-view-state]].
