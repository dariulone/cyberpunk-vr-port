---
name: cp2077-producer-selection-structural
description: "IDA xref proof (2026-06-09) — in-world RTT producer vtable 0x307D690 built by ONE ctor sub_141CA0730; preview 0x2B03A70 by TWO others (sub_1406A00B4/sub_1406A03D8); separate construction sites => producer is structurally selected at construction, NOT a settable property. In-world producer co-located with component vtable (same module)."
metadata: 
  node_type: memory
  type: project
  originSessionId: a4faea66-21d0-4884-a9b4-10c3d5515440
---

**Decisive static finding (xref-only, no decompile per [[ida-no-decompile-rule]]):** which RTT producer a component gets is bound to WHICH ctor builds the subobject@comp+0x120, not to a runtime property.

- in-world self-allocating producer vtable **0x14307D690** ← written by exactly ONE site: `sub_141CA0730` @0x141ca07a7 (sub_141CA0730 itself has no direct callers = pure virtual dispatch; installed in a vtable slot @0x144ae4320).
- preview producer vtable **0x142B03A70** ← written by TWO sites: `sub_1406A00B4` @0x1406a00cf and `sub_1406A03D8` @0x1406a040e.
- These are entirely separate construction functions => the preview-vs-in-world choice is made by the construction/attach code path, not by a flag we can set on a CreateInstance'd component. Confirms why [[cp2077-spawn-test-array-append-negative]] saw a bare CreateInstance default to preview (0x2B03A70).

**Address co-location clue:** in-world producer 0x14307D690 sits ~0x16C0 from the component vtable 0x14307BFD0 (both in 0x1430_7xxxx = same translation unit/module), while preview 0x2B03A70 is a far region (0x142Bxxxxx). Suggests 0x307D690 is the component's NATIVE producer wired during real render-attach, and preview is a fallback default for bare/UI-preview construction. => getting the in-world producer == triggering real render-attach == the same wall as every prior session.

**RTTI note:** RED4 vtables carry NO MSVC RTTI COL at vtable-8 (the qword there is code/prev-slot, not a _RTTICompleteObjectLocator). Use RED4 reflection dump (reflection_rtti_dump.txt) + strings for type names. NB: ida-pro-mcp `list_strings_filter` TIMES OUT at 1s on this exe (string list too large to build in the MCP budget) — navigate via xrefs/byte-reads instead.

**Consequence for vector 1 (spawn fresh entity):** you can't configure entRenderToTextureCameraComponent into the in-world producer from script. The value of spawning a FRESH entity is that the entity-assembly attach path may run sub_141CA0730 and wire the in-world producer for us — still the decisive UNTESTED experiment. See [[cp2077-native-stereo-roadmap]].
