# Goal 14 validation — 2026-11-12

Branch `fix/goal12-20261111`; local commits only, no push. Work in progress.

## Request and correction

Eight items: visible corpse-grip gap; hand jitter while holding; hands during general time slowdown;
left-ear + physical left L3 scanner default; windshield minimap over wheel; money-transfer HUD;
mirror-triggered appearance editing inside SAO with headwear hidden; handgun hip/thigh placement down/forward.
The appearance entry must remain the **existing mirror interaction**, not a new CHARACTER footer button.
The superseded footer/native-panel handoff was removed after the clarification; do not claim it implements SAO editing.

## Evidence so far

- **General slowdown reproduced:** named temporary time dilation at 0.1 gave 19 cm hand lag; 0.01 gave 17 cm,
  with scanner Vision=0. At 0.5 the sampled hand shifts had almost no lag. Fix publishes actual world time scale
  and suspends hand physics during slowdown, preserving tracking and explicit reload-owned stops. Scanner and
  world slowdown have independent reason bits, so ending one cannot resume while the other is active.
  Fixed live matrix 1 / 0.5 / 0.1 / 0.01 / 1: sampled errors <=0.1 mm, collision mask 111/0/0/0/111.
  This is a tracking-preserving fallback, not a rescaled real-time PhysX simulation. Dynamic corpse holds release
  when suspension begins. Live reload under Sandevistan has not yet been exercised; ownership preservation is unit tested.
- **Scanner default verified live:** grip alone does not activate; fresh physical left L3 at left ear activates,
  next click closes; holding L3 before entering the ear zone does not activate; release/reclick does.
  Raw click is claimed before D-pad/cyberware/deferred sprint conversion. Offline tests include left-handed bindings.
- **Corpse gap:** old uniform forearm profile overestimates the distal forearm, and 6 mm wrap allowance enlarges it
  again. Added taper toward wrist and reduced allowance to 1.5 mm; live distal-radius trial visibly fits more tightly.
  Exact live capsule can now be fed to the compiled C++ real-hand-mesh harness. Collision/solver surface contact is
  not proof of contact with the visible NPC mesh; no universal mesh-exact claim.
- **Jitter:** initial fixed-controller 90-sample / 9.9 s baseline: wrist range 1.27 mm, max step 0.41 mm/0.273 deg.
  Higher-rate final test (~1776 samples/8 s, 363 drawn changes): stationary wrist max step 1.65 mm/1.075 deg,
  p95 0.882 mm/0.609 deg. Smooth C++-generated motion: max wrist step 8.21 mm/4.00 deg versus controller
  10.79 mm/4.29 deg; wrist p95 3.88 mm/1.53 deg. Small physics tremor remains; this is not zero-jitter.
  `smooth-motion.mp4` and extracted frames checked; sampled visual evidence is limited by body occlusion.
- **Minimap verified in parked Porsche:** single map above actual wheel, head turn +/-20 deg and lean 15 cm
  preserve cabin placement. Ordinary minimap restored after unmount. Width 40 cm, wheel-relative offset
  +35 cm forward/+40 cm up. Bikes, stale/changed vehicle, recenter and fallback have unit coverage; their live
  transitions and moving-road test remain outstanding.
- **Money notifications:** UI-only +123/-123 calls (actual money unmodified) show one native localized transfer widget
  in HUD. Real entry `items_notification_queue`; authored crop retained, moved through separate-window path.
- **Holsters:** handgun hip/thigh 3 cm down +2 cm forward. Copy/shell/reach transforms share the placement.
  Live Overture hip_r `[0.14,-0.08,0.23]` (bone axes), copy/shell present. Offline both-hand/body/multiple-gun tests pass;
  melee/knife/underarm/back unchanged. Thigh-shell routing now exists and does not alias hip shell keys.

## Tests

- Shared Release DLL builds and deployments passed.
- Scanner input/slowtime C++ production hand-stop regression passed.
- HUD 39 tests + WARP submit/vehicle-anchor test: 40/40 passed.
- Holster Lua tests passed. `py -3.13 -I` failed here because lupa is user-installed; normal project Python run passed.
- Corpse driver/profile/whole-Lua syntax tests passed.
- Appearance tests for the superseded flow are not acceptance evidence for the replacement SAO flow.

## Mirror collision follow-up

User observed body displacement at the mirror, then Johnny appeared. Confirmed live `johnny_mirror=1`,
`Character.johnny_mirror` present and ordinary VRIK mirror binding inactive; no quest facts were changed.
Source found mirror clones were not excluded from HandCollision's nearby living NPC capsule list. Exclude only
`Character.q110_tpp_male_v`, `Character.q110_tpp_female_v` and `Character.johnny_mirror`; ordinary `Character.Silverhand`
and other NPCs remain physical. After clean load, normal mirror double and player held identical positions with
no observed drift while idle; native double collision was not disabled. Four repeated hand positions at chest/body
also produced **0.0 m player drift** (`mirror-sao/drift-active.json`). This isolates the port's NPC capsule inclusion;
no native NPC collision disable was retained.

## Appearance route constraints

The user explicitly rejected a HUD/XR layer: appearance must remain a **world SAO menu**, using the existing vendor
interception strategy so the native fullscreen menu never opens. Temporary XR-channel edits were fully removed
before compilation/deployment. The earlier observer/reparent attempt failed: the native mirror editor freezes world
time to 1e-6 (world widgets cannot spawn), then sends only its fullscreen image to XR (a prepared world panel still
is not visible). Neither `phase=active` nor a prepared texture proves visual success.

Current implementation extends the existing native vendor request queue to retain only the mirror scenario's
`MorphMenuUserData`, with the same timeout/fallback. A narrow native loads the normal customization resources from
the disposable mirror NPC (the public signature restricts it to PlayerPuppet, but the verified implementation reads
entity components and TppHead/RightArm slots). Actual player equipment is never replaced. Options use the normal
InitializeOptionsFromFinalizedState/ApplyEditTag/ApplyChangeToOption/ReFinalizeState/CancelFinalizedStateUpdate APIs.

## Appearance route — live results (2026-11-13, H10, simulator)

- **Scene completion.** The mirror scene's UI-manager node (`questSwitchToScenario`, start = character customization,
  end = `MenuScenario_Idle`) waits on the menu instance's scenario listener. A retained request never reaches the
  native editor, so `VRMirrorAppearanceFinish` re-enters `MenuScenario_Idle` on the recorded menu instance (switch
  vtable +0x108, RVA 0xC6EF4C) after the mirror NPC is restored. A queued Idle switch from Lua is filtered out by the
  event handler and does nothing. Verified: hub with leave/change/expression returns after cancel and after accept.
- **Headless reflection (user: "тело исчезает").** The editor head/arms (`CharacterCustomizationMaHead`, `...Arms`)
  are new items every session. `ApplyChangeToOption` returns early when the requested index equals the option's
  current one, and its parts wait for an async resource load (+540 in-flight counter → +536 → tick 0x75A998). On a
  second and later open in one game session, unchanged options therefore put nothing on the new head: no face, no hands.
  Calling the part-apply functions directly (0x2D8FA6C/FAA8) does nothing either, because the resources are not loaded.
  Fix (Lua only): after `InitializeOptionsFromFinalizedState`, every active option has `currIndex` cleared to
  0xFFFFFFFF and its value applied again with `ApplyChangeToOption(o, index, true)`. The game then takes the value as
  new, loads it and removes nothing. Linked options keep their index (checked: none left at 0xFFFFFFFF). Live: 4 opens
  in a row → face, hair, beard and arms visible each time (`mirror-face3/faces-cancel.jpg`). A hair change after the
  reapply swaps cleanly (old `hh_062` parts gone). Cancel → finalized hair 8; accept 8→9, then 9→8. Save folder untouched.
- **Walking away (user: "отходишь на 5 м и оно выключается").** The mirror switches off at ~4.7 m in all cases (plain,
  editor open, after cancel); an open editor cancels and restores first. A separate defect was found and fixed: after a
  far leave the mirror was **never offered again** ("look in the mirror" missing even when walking back facing it).
  Cause: VRIK kept the double on the player, so the leave branch despawned it ~5 m outside the bathroom. Leaving at the
  mirror worked (double despawned there). VRIK now sends the double back to the mirror's start point first and leaves
  only once it is there. Live: three consecutive far leaves (plain, plain, editor open) → "Посмотреть в зеркало"
  offered again on return. Choice 220's shape is narrow: 0.12 m past the start point it is not shown (vanilla shape).
  Rechecked after the close rework below: editor open → walk 6 m out → back → offered at the start point.
- **Panel side and layout (user: "правее", "кнопки меню убрать").** The appearance panel has its own canvas beside
  the mirror on the free hand's side: right of it for a right-handed player (canvas x 2160), left of it in left-handed
  mode (x 220, `VRLeftHanded()`). The SAO menu column and money/weight strip are not built in appearance mode. Live:
  right panel clear of the reflection; left-handed open → panel left of the mirror (`mirror-left/left-mirror_L.jpg`),
  handedness restored to right afterwards.
- **Game's own editor button.** A 130 px icon (`mappin_icons` part `mirror`, the vanilla mirror choice icon) next to
  the panel title. It undoes the preview, rebuilds the double, then hands the **retained** `MorphMenuUserData` to the
  native editor as if it had never been taken (`VRMirrorAppearanceNative`: only when the recorded menu instance is
  still in `MenuScenario_Idle`; refused → the scene is completed here instead). The native editor first came up
  **empty** (no option list): the system's options resource (+160) still held the double's resource, so its
  `SetCustomizationOptionsResource` loaded nothing and never sent `OnInitializeOptionsList`. The system is now reset
  before the hand-over. Live: native "Выбор внешности Ви" with all rows (`mirror-native3/native-flat-small.jpg`);
  its "Назад" + confirm returns the mirror hub (leave/change/expression); the double keeps one copy of each part.
- **Reset without a target.** `ResetCustomizationOptionsResource` first takes the session's parts off the system's
  target -- the mirror double, matched by resource, so its own copies go too. `VRCustomizationReset` clears the
  target (system +0x108), calls the reset (vtable +0x240) and puts the target back. A reset in the same frame as a
  finalize cut the double's customization controllers off (bald reflection), so after a plain close it waits 2 s; a
  new request or the native hand-over flushes it at once.
- **Close strips the double (first edit after a load).** Removing the editor head (lease restore) took the double's
  hair, beard and piercings with it; nothing put them back (stepwise probe on a fresh load: revert → refinalize →
  head back → all customization parts gone). Close is now staged: preview reverted (cancel) or finalized (accept),
  own head/arms/headwear back at once; 1 s later the finalized state is reapplied on the double's own head and
  finalized again (its controllers rebuild it, one copy of each part: the session's reapplied duplicates, ×5 hair
  ×5 beard, go); 1 s later target cleared and the request completed or handed over. Live on a fresh load:
  cancel → hh_062 ×1, beard ×1, earring ×1; accept +1 → hh_085 ×1; accept −1 → hh_062 ×1. Panel changes are applied
  at once (`ApplyChangeToOption(..., true)`) so no deferred change lands after close.
- **Remaining limitations.** Checked only in V's H10 bathroom with a male V; the other copied mirrors and female V
  were not checked in game. Face/body morph rows were not exercised after the close rework (hair is the probe).

Offline: `tools/vr_menu_tests/appearance_test.py` 32/32 (staged close, rebuild on the double's own head, native
hand-over after rebuild, refused hand-over, cancel revert, deferred/flushed reset, reset script fallback, release
while restoring), `vendor_handoff_test.lua` PASS, `tools/grasp_tests/mirror_filter_test.py` PASS.

## Test failures and recovery

- First direct SetTimeDilation test used `CName.new("")` for curve names. Game crashed at exe+0x620095 while reading
  a null curve. Use `CName.new("None")` and all optional arguments (including nil listener). Corrected matrix ran.
- Its queued cleanup command was consumed at the next boot before TimeSystem existed, causing another crash
  (exe+0x7D6588). Both dumps inspected before further launch; the command file was already removed by the bridge.
  No test time-dilation reason survives process exit. Neither crash is reported as a production hand-physics failure.
- Native `Game.OperatorEqual` has no matching Lua overload for MorphMenuUserData; private editor fields read nil.
  Superseded appearance implementation relied on these. Replacement must use public APIs/observed mirror lifecycle.

## Evidence

`build/goal14-20261112/`: `slowtime-baseline-valid/`, `slowtime-fixed/`, `scanner-L3/`,
`grip-jitter-baseline/`, `forearm-taper-trial/`, `grip-taper-final/`, `car-minimap/`, `money-hud/`, `holsters/`,
`mirror-body/`, `mirror-face2/`, `mirror-face3/`, `mirror-walk-final/`, `mirror-native3/`, `mirror-left/`, `re/` (IDA exports of the customization system).
Test actors/cars temporary. Never manually save test state. Runtime remains the user's restored simulator runtime.
