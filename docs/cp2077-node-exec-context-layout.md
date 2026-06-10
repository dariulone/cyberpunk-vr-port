---
name: cp2077-node-exec-context-layout
description: option-C node-executor re-grounded; node-exec-context layout (RT-id byte @+0x38, state @0x3a/0x3e/0x41, flags @0x30), view is GLOBAL (renderer+0x4658) - right-eye lever = 2nd pass w/ different +0x38 RT byte + swapped global view
metadata:
  type: project
---

Re-entry to option-C node-executor ([[cp2077-framegraph-node-executor-hookable]] / [[cp2077-node-replay-not-viable]]) after RTT path proven dead ([[cp2077-isenabled-necessary-not-sufficient]]) and mirror view-injection already dead ([[cp2077-952xx-is-generic-task-scheduler]]). IDA safe-read decode (NO decompile/disassemble — they crash IDA; use read_memory_bytes + hand-decode).

**Node executor `sub_1401EC404(rcx=executor-obj, rdx=exec-context, r8=arg2)`:**
- reads `[rdx+0x30]` flags (tests bit1), calls `executor.vtable[0x28]` then `vtable[0x10]`.
- resets context state each call: `[rdx+0x3a]=0`, `[rdx+0x3e]=0`, `[rdx+0x41]&=0xFE`.

**Pass driver `sub_1401EC1D0(rsi=pass-descriptor, rdi/rdx=exec-context, r8=arg2)`** (calls executor x2, count-branched):
- `[arg0+0x20]` = node count, `[arg0+0x34]` = flag/gate.
- `r8b = [arg1+0x38]` => **RT-id is a single BYTE (render-target slot index) at exec-context+0x38**, not a pointer.

**Key architecture fact:** the output render-target is selected by the `context+0x38` byte; the VIEW matrices are GLOBAL (centralized render-view-state at renderer+0x4658, sub_140788A9C, [[redengine-render-view-state]]) — all nodes in a pass share it.

**Right-eye lever (concrete):** re-execute the scene pass nodes a 2nd time through the proven-stable hook (sub_1401EC404, reentrancy confirmed [[cp2077-node-executor-reentrant]]) with (a) a DIFFERENT context+0x38 RT-id byte (separate output), and (b) the global view-state swapped to right-eye view/proj ([[cp2077-perview-camera-layout]]: per-view cam quat@0x70/fov@0x80/view@0xB0/proj@0x1B0). The non-idempotent-recording crash of blind replay ([[cp2077-node-replay-not-viable]]) should be avoided because a distinct RT-id + reset state fields (0x3a/0x3e/0x41) give the 2nd eye its own output, not an in-place overwrite.

NEXT: determine valid RT-id byte values (which slot = a free/2nd-eye target) and how the global view-state is swapped per-pass; then prototype a 2nd-pass injection in the existing node-executor hook.
