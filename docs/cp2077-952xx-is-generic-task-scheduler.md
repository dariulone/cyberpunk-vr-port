---
name: cp2077-952xx-is-generic-task-scheduler
description: CORRECTION — sub_140952CF4/952C28 is the GENERIC frame TASK scheduler, not the view collection; elem+0x60 is a profiling NAME-string ptr (audio/AI/etc), not a view-stage vtable. "main=idx0/mirror=idx18" was a misidentification.
metadata:
  type: project
---

Hard correction after IDA check. The values captured at elem+0x60 of the 952xx
scheduler elements — 0x7FF6_5482ECC0 / 0x7FF6_5482ED30 (RVA 0x2C5ECC0 / 0x2C5ED30,
IDA 0x142C5ECC0 / 0x142C5ED30) — are NOT view-stage vtables. They resolve to .rdata
profiling strings: `'Audio/FinishAsyncJobs'` and `'Audio/Components'`. So elem+0x60 is a
**task debug-name pointer**, and sub_140952CF4 / sub_140952C28 / sub_140952B30 are the
**generic per-frame TASK scheduler** (audio, AI, material, view tasks all mixed, each tagged
with a name string + sector tag like "03_night_city").

Consequences — DO NOT rebuild on this:
- "main view = idx0, mirror = idx18" (in [[cp2077-viewfamily-iterator]] /
  [[cp2077-true-stereo-mirror-mechanism]]) was a MISIDENTIFICATION — those were
  name-tagged generic tasks, not view stages.
- Explains M3 null result: re-scheduling idx0 did nothing because idx0 was an AUDIO task,
  not a render view. The 952xx injection model is a dead end for option C.

The ONLY reliable render-view handle is the correlation-found one:
**sub_140D69018 (taskWork=0xD69018) -> sub_140126D4C(camObj, *(spec+8))** = builds ONE
render view. Main + mirror both run through it per frame. RT-routing is automatic per view
(see [[cp2077-framegraph-rt-routing]]). FRONT half of C (create a distinct right-eye view)
should be re-anchored OFF the 952xx scheduler — prefer the reflection/mirror system at the
RTTI/component level (red4ext has full RTTI) as the maintainable native lever.
