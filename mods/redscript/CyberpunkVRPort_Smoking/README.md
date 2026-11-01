# CyberpunkVRPort_Smoking

Cigarette and lighter as VR props: spawn the vanilla item entities into the hands, drive their FX,
and gate everything on real world-space distances (cig tip ↔ lighter flame, cig ↔ head) rather than
on controller proximity. `init.lua` is only an input bridge — both analog triggers and both grips out of
shared slots 154/160/49/155 into `PlayerPuppet.VRSmokeTick`; every decision is in the `.reds`.

Left-handed (`xr_left_handed`) the props swap hands: the cigarette goes to the left hand with its own left
grip, the lighter to the right hand, struck by the right trigger. The right hand's lighter hold is the left
hand's mirrored through the rig's twins (`src/Anim/LeftHanded.cpp`), not a second recording.

## It stopped loading on 2026-08-01, and why

RED4ext refused the script at startup:

```
Script validation error: Missing native global function 'SetVRSmokeFingers'
  at r6\scripts\CyberpunkVRPort_Smoking\vrport_smoking.reds:31      (and eight more, through :50)
```

Not a redscript problem and not the RED4ext move: the natives simply were not in the binary that
provides them, `CyberpunkVR_Hands.dll`. Reading the name tables out of the two builds on disk:

| build | date | `VR*` names |
|---|---|---|
| `src/red4ext_plugin/build/RelWithDebInfo/` | 2026-07-11 | 106 |
| installed | 2026-07-31 | 64 |

The July 31 rebuild had lost 42 natives — 21 `VRSmoke*`, the 16 `VRSettings*` from
`settings_bridge.cpp` (a file no longer in the tree), `SetVRStartMenuState` and `SetVRWheelActive`
— and gained one, `SetVRMuzzlePos`, which `CyberpunkVRPort_Weapon/init.lua` calls. So reinstalling
the older DLL was never an option: it would have traded this mod for the weapon mod.

The source was not merely misplaced. Every committed revision of `src/red4ext_plugin/main.cpp`,
back to `6f54d09`, contains **zero** occurrences of `VRSmoke`; `git log --all -S` finds the string
only in a stash, and only in the `.reds`. The implementation lived in a working copy that was built
on July 11 and later overwritten.

## How it was recovered

Not from a backup — neither `CyberpunkVRPort_backup_20260727-1330` nor
`CPVR_AER_session_backup_20260621-113136` has it. It came out of the **opencode session store**
(`~/.local/share/opencode/opencode.db`), which had recorded every `edit` the plugin ever received.

The reconstruction is a replay, not a reading:

1. Baseline `e0f8154:src/red4ext_plugin/main.cpp` — committed 07-09 14:28, four hours before the
   smoking work started at 18:39.
2. Every completed `edit` to that path after that timestamp, in order: **87 of them**, ending
   07-11 14:17:32 — one minute before the DLL that carries the natives was linked.
3. Each `oldString` had to occur exactly once. **86 applied cleanly, 0 ambiguous, 1 not found** —
   and that one adds `SetVRWheelActive`, which nothing calls, so it was left out.

Result: 7659 lines, verified field-by-field against the symbol table in the July 11 PDB —
**24 of 24 `VRSmoke*` functions, 56 of 56 `g_VRSmoke*` globals, 24 registrations**, nothing missing.

That file is the July 11 state, so it was not installed wholesale. Diffing it against HEAD gave ten
hunks each way, cleanly separated: 783 lines of smoking (plus 44 lines of finger-bone resolution
and three `#include`s) present only in July 11, and ~75 lines of muzzle work present only in HEAD.
The merge is the union — smoking grafted in, muzzle kept.

One header had to follow: `g_VRBoneParent` grew `[256] → [800]` on 07-10 17:41, in `main.cpp` and
`vrik/vrik_hook.h` seven seconds apart. Only the `main.cpp` half survived, so the rebuild failed
with *"redefinition; different subscripts"* until the header was grown to match. The smoking
gesture resolves finger bones by name across the whole metaRig, which runs past 256 entries.

The rebuilt DLL carries 88 `VR*` names against the installed build's 64: all 15 natives the script
declares, `SetVRMuzzlePos` kept, and **no regressions** — nothing the July 31 build had is missing.
