---
name: cp2077-clone-crash-global-arena-slot
description: eye-2 clone-submit crash root-caused to GLOBAL per-frame arena slot collision; deep-clone path exhausted, distinct registered view-slot is the singular requirement
metadata:
  type: project
---

DECISIVE root-cause of FC8/FGB clone-submit eye-2 crashes. Iterated deep-clone 3 crashes deep:
1. handle+0xF88 read -> fixed by 0x2000-byte clone
2. sub-objects +0x40/+0x60 (class vtRva 0x2E92718) torn DynArray append -> fixed by deep-cloning those subs
3. FINAL wall: sub_1401F0F80 `mov [rsi+rax*8+0x514950],rcx` WRITE AV.

sub_1401F0F80 = generic per-view-slot accessor of a GLOBAL per-frame render arena (HUNDREDS of callers across renderer). args: arg0(rcx)=arena base, arg2(r8b)=slot-id; arena indexed by arg2*0x5E00 (slot stride 0x5E00) + global table @+0x514950 => arena holds ~224 view-slots. Plenty of capacity for eye-2.

CRASH CAUSE: eye-2 (cloned submit) reuses eye-1's slot-id because slot-id is assigned at VIEW REGISTRATION (producer), not carried in the handle. Two builds -> same arena slot -> collision. Deep-cloning the handle graph cannot fix this: the arena is global engine state keyed by registration-assigned slot.

CONCLUSION: clone-submit path proves the singular requirement (consistent with all prior walls [[cp2077-registry-injection-feasibility]] [[cp2077-rtt-no-drivable-api]]): eye-2 needs a REGISTERED, distinct view-slot from the producer's AddView. Inventory preview works because its view IS registered (gets its own slot). 

NEW LEVER (untried combo): the call-site-patch technique is PROVEN stable (FGB call-site hook at 0x293BCE survives, unlike prologue patches). Apply same call-site patch to the producer view-insert CALL (sub_140212780 inside fan-out loop sub_140212800) to add a 2nd view with distinct slot natively -> engine assigns it a real slot -> build won't collide. See [[cp2077-view-fanout-submit-chain]] [[cp2077-view-item-layout]].
