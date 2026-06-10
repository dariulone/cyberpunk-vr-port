---
name: cp2077-spawn-test-array-append-negative
description: DECISIVE negative (live) — appending a configured isEnabled RTT cam into player->components does NOT make the engine tick/render-attach it (no handle@1E8, no view-pair@268, no flag88 base-state); array membership != attach
metadata:
  type: project
---

Live spawn test (2026-06-09, SpawnRTTCamOnPlayer native, user ran in-world): created entRenderToTextureCameraComponent via CreateInstance+InitializeProperties, set owner=player, renderingMode=Shaded, res 2048², isEnabled@0x8B=1, resref=0, PushBack into player->components (size 169->170, capacity 240 => appended WITHOUT realloc, no race; capacity-guard worked).

RESULT after several frames of PollRTTHandle:
- our comp=0x1b5cb1c7860: +0x1E8=0 (no handle), +0x268/0x270=0/0 (no view-pair), flag88=0x1000000 UNCHANGED (only our own isEnabled byte@0x8B; engine never set the base 0x3 state bits that live active cams carry as 0x1000003), subVt@0x120=0x2B03A70.
- (poll "[flag88 ATTACHED]" tag is misleading: it triggers on our own isEnabled bit, not engine attach.)

DECISIVE NEGATIVE: the engine does NOT tick/render-attach a component merely appended to a live entity's components DynArray, even on the render-active player with isEnabled=1. Array membership != attach. Empirically confirms (not just static) [[cp2077-attach-is-render-thread-virtual]] + [[cp2077-isenabled-necessary-not-sufficient]]: render-attach enroll happens through the engine's own component-attach lifecycle (render-thread virtual dispatch), which a script-thread append never invokes. Producer-independent signals (+0x268, flag88 base-state) both absent => negative is sound despite the resref=0 confound on the preview-producer handle arm.

NEW DATUM: a freshly CreateInstance'd entRenderToTextureCameraComponent defaults to the PREVIEW producer (subVt 0x2B03A70, needs resref@0x1E0->handle), NOT the in-world self-allocating producer (0x307D690). To get the in-world Shaded producer that self-allocates its RT (+0x268/0x270, no resref), some property/config selects it — unknown which; worth finding for any next attempt.

Remaining escalation vehicles (append-to-existing-entity is now dead): (1) force native re-attach of the player entity (appearance-change / re-stream) so engine re-runs assembly+attach over the new component — risky (player visuals); (2) engine-thread piggyback: hook the per-frame engine point that attaches/ticks RTT components, inject ours into that iteration; (3) SPAWN A FRESH ENTITY carrying the cam so the entity-spawner's assembly attaches all components natively (original option-B "cam in template") — sidesteps the assembled-entity problem. Linchpin unchanged: invoke the engine's own component render-attach; only (3) does it without render-thread injection or player-state risk.
