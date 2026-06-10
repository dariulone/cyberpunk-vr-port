---
name: cp2077-view-registry-producer-generic
description: view-registry (0x2B57F58) ctor/factory/dtor are all generic registered-type machinery; per-frame populator (insert) is behind producer virtual dispatch, not statically reachable
metadata:
  type: project
---

Reversed the view/pass registry ([[cp2077-view-registry-live]], vtable exe+0x2B57F58) from the construction side to find the per-frame populator. Dead-ends, all confirming generic dispatch:

- **ctor** `sub_140AAB75C` (only data-xref to vtable): sets vtable, inits sub-objects @+0x88/+0x1B0, stores descriptor @+0x58, zeroes fields to +0x4A8 (registry ~0x4B0 bytes). Just construction.
- **factory thunk** `sub_140AAC30C` = `mov rcx,rdx; jmp ctor`. Reached only via data-ref in a **static init-table** at 0x1449C24E0 (rows of `{typeTag 0x1431Fxxxx, ctorFn, end}`; our row tag=0x1431F32D0). Registry is a member sub-object of a big render-scene-setup, built by generic init-table dispatch.
- **vtable** mostly generic stubs (0x1409C4600, 0x14014A700); slot[3]=`0x140AAC51C` is the scalar **deleting destructor** (tears down sub-objects @+0x2D8..+0x498), not insert.

So the populator that inserts the ~40 camera-entries each frame is NOT reachable from these static xrefs — it's the producer iterating a view collection and calling registry.insert via virtual dispatch (same wall as [[cp2077-simultaneous-producer-virtual-wall]]). Static reverse of the producer is exhausted: construct + submit + populate are all generic registered-type / redJobs2 dispatch.

Next genuine step is RUNTIME: hook the registry insert (or the generic view-job submit) on the producer thread to capture the populator + the view collection it reads — that collection is where a right-eye view-set would be injected for native simultaneous. No static per-frame submit site exists to find.
