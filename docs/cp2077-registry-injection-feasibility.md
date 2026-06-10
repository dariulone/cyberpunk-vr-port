---
name: cp2077-registry-injection-feasibility
description: live verdict on injecting a 2nd view into the view registry — entries are value-inline in a hashmap (refs=0), so injection requires replicating engine hash+insert; not a cheap append
metadata:
  type: project
---

Tested feasibility of the "inject right-eye entries into the live view registry so the single natural FG-build renders both eyes" lever (the only non-replay path left after [[cp2077-inline-replay-dead-all-levels]]). Live RPM (game open) verdict: **fragile, not a cheap append.**

- Real per-pass registry this session: vtable exe+0x2B57F58 (registry[1]); registry[0] is the parent array of ~16 sibling sub-registries.
- Entry count is **render-state dependent**: ~40 in inventory last session, only **1** (key=0x10000, main camera vtRva 0x2B72EE8) in plain gameplay this session.
- Entry layout this session (0x40): `+0x00 key` / `+0x08 registry back-ptr` / `+0x10 camera` / `+0x18 obj` / `+0x20 embedded sub-object vtable (rva 0x2AC1B90)` / `+0x28 self+0x20` / `+0x30 obj`.
- **Decisive: a full-heap scan for pointers TO the entry returned refs=0.** No pointer references the entry → the map stores entries **value-inline** (entry = slot in a bucket array at its hash position), not by pointer. So injection ≠ alloc-node + add-pointer; it requires writing into the correct hash slot = replicating the engine's hash function + insert + count/capacity bookkeeping, or calling the registry's own insert method (not statically identified; [[cp2077-view-registry-producer-generic]]).

Net: all three native-simultaneous routes converge on the same requirement — replicate or drive the engine's own view-insert/producer, which assumes a single main view (FG-build base−5 on a 2nd ctx). Direct injection is high-risk (map corruption / build asserts) and still needs the insert mechanics + a 2nd constructed camera + RT routing to the right eye.
