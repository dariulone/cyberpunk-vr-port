# Osprey aperture and reticle investigation

The earlier PID26944 fixture measured the weapon-root-transformed authored mesh
centre. Its numerical gates passed, but this is **not proof of alignment with the
rendered Osprey screen**. No visual reticle PASS was claimed.

## Confirmed coordinate error

The packed `osprey_surface.mesh` and `osprey_reticle_rear.mesh` each have one skin
bone, `scope_hinge`, bound to the `Barrel` component. Their authored centre is in
mesh bind space. Both the old production eyeToAxis/camera code and the initial
fixture transformed it only by the weapon entity, omitting the animated hinge.

The packed appearance already has `FX_slots/vrp_scope_hinge`, with zero relative
translation and identity rotation. The standard GetSlotTransform API therefore
provides its live world transform without a new archive or DLL.

REDengine uses row vectors: `world = meshPoint * inverseBind * liveBoneWorld`.
The existing mesh and part generators document the same convention. The C++
`render_audit_math optics-bind` derives bone-local centre
`[0.0275094006,-0.0267940455,-0.0143638775]`; mesh +Y maps to bone +Z, with local
quaternion approximately `[0.7071067812,0,0,0.7071067812]` (xyzw).
Packed JSON and source SHA256 values are under
`scratch/goal-audit-20261117/osprey-reticle-audit/`.

Production `optics/frames.lua` and independent fixture `osprey-binding.json` came
from that C++ derivation. Optics/init.lua now resolves Osprey's point and rotation
through the live slot for eye-to-axis testing and camera placement. Missing slots
return an unavailable frame instead of the incorrect root-space fallback. Other
fixed scopes retain their previous frame; Rasetsu retains its front sensor branch.
The slot handle clears on restore/unequip. Renderer independent source review
approved matrix order and camera conversion. Animated-frame/visibility/bridge
regressions and C++ tests pass; this source change is not yet live verified.

## Texture and projection diagnostic

Packed composite marker `ImageScale=-716.25`, rear emission32.319, native widget
resource and mesh bindings are present. Component hashes match the shared2048
atlas layout: camera top-left, front top-right, rear bottom-right. The reticle
shader returns zero outside its projected local-UV rectangle, while the camera
image covers a much wider field. The wrong aperture can therefore show a camera
image while missing the marks; this is the concrete next hypothesis to test.

`--optics-probe` records raw textures from existing VRPhysicalOpticsProbe and a
bounded widget tree. C++ `optics_atlas_readback` emits four BMP tiles and statistics.
It was verified on the historical Osprey readback: rear brackets and central dot
are visible. That old readback is not current-session evidence.

`hotload_optics.py` backs up/hash-checks init.lua+frames.lua, replaces the existing
M.update without duplicate event registrations, and retains runtime options and
the existing path publisher. It requires root's causal save guard and inactive,
holstered optics. Its owned `--restore` mode restores only its saved token/files.
An offline test executes the exact payload and verifies callback/rollback ownership.
Actual normal CET `require('optics/frames')` still requires a normal restart check;
the controlled hotload uses the actual source text through a dependency table.

After root GO, resume the owned street scene and verify the actual hinge frame.
Use at most two C++-computed translation corrections (12cm maximum each), retain
the alignment gates, then capture hip/ADS/hip and both atlas readbacks. Blank rear
pixels point to native UI/window generation; populated rear pixels with missing
lens marks point to material/UV projection. Declare visual PASS only after the
reticle, distant target, physical glass/caps and both-eye images are inspected.

Game-camera-derived eyes are not exact submitted XR/capture matrices. Source shows
GetVRMainEyeOffset and both EngineStereo separation getters use the same desired
IPD convention; temporal/submission-matrix agreement remains a separate boundary.

## Controlled live result on PID26944

Hotload succeeded with one existing callback, root save protection and backups in
`scratch/goal-audit-20261117/osprey-hotload-26944/manifest.json`. Only init.lua and
frames.lua changed; installed visibility/profiles matched source hashes before
the operation. The live bone slot was available and both production and independent
C++ measurements resolved it. In the sampled hip pose, its derived frame nearly
coincided with the bind-space frame; this does not prove what it does during ADS.

`osprey-skin-frame-live-26944` captured both native2048x2048 FP16 atlases. The rear
tile contains readable Osprey brackets and central dot (max0.873, p99≈0.207,
approximately250,000 pixels above0.02), and the widget tree's root is visible.
Thus current native UI generation/binding is established; the reticle is not an
empty or absent native texture. The hip screenshot also contains the marks.

The requested ADS transition did **not** occur: UpperBody remained0. Accordingly,
the fixture report is failed, and files named `osprey-ads` must not be represented
as native ADS6 evidence. A single short follow-up input diagnostic moved the left
hand away from the kiosk/weapon, saw rawLT0/1, weaponFlag1, cyberwareKind0,
vision/vehicle0, and an expired/nonblocking LT deadline in all32 samples. UpperBody
stayed0. No native flags/gates were changed, and the real shot counter did not move.
Root chose the already planned fresh-session repeat rather than resetting PSM state.

All temporary state was restored. `osprey-aim-input-26944/lease-return.json` verifies
747 items,806112 money, holds0/0, no weapon/menu, no audit/command pending, recorder
stopped and three exact configuration byte matches. Root road-scene restore returned
the original terrace and `GoalRoadScene.restored=true`; root save lock remains active.
The approved narrow hotpatch/source files remain installed. Normal CET require loading
and actual hip→ADS6→hip visual validation are reserved for the next fresh session.

## Fresh ADS6 and the specific hinge fix

PID4744 loaded init.lua/frames.lua normally and entered actual UpperBody6. The
single controlled series `osprey-skin-frame-fresh-4744` failed the unchanged
alignment gate: hip eye-axis≈11mm, ADS best eye-axis63.41mm (right88.27mm), while
the optical/bore angle remained≈0.08°. Images show the screen folding aside.
Restore succeeded:747 items/806112 money, no holds/weapon/menu/pending commands,
three exact configuration byte matches and the root-owned road scene restored.

The separate C++ `osprey_hinge_audit` compared the native `idlepose_ironsight`
clip to its rig and the live slot transforms. Parents0/1 do not move in that clip;
bone8 scope_hinge changes only rotation by180°, with translation difference
2e-10m. Bone11 pos_ironsight has its independent1.418mm translation, which is left
untouched. Live weapon-relative hinge pivot changes0.165mm, hinge rotation179.9975°,
weapon root rotation0.0255°, then returns to the hip orientation. The report is
`osprey-reticle-audit/hinge-clip-report.json`; live direct VRRigBone locals were not
recorded, so it distinguishes clip-local proof from live world/weapon-relative proof.

The new native VRPhysicalOpticsHinge adapter preserves all gameplay ADS state and
uses the existing pose-copy/remap site. It writes only the mapped rotation of
source bone8 to authored hip quaternion(0,0,1,0). It never changes parents,
translation, scale, bone11 or reload bones5/7/12, and does not use the shared
VRRigWrite slot table. A conflicting existing writer for(1,8) causes refusal.

Ownership is qualified by the actual current player/active weapon, exact Barrel
component and enabled controlBinding pointer to that weapon's Receiver. Receiver's
AnimatedObject and current instance descriptor/trackB come from the typed animation
system bucket. Shared referenceTracks/asset names alone are never an owner key.
The existing AnimatedObjectUpdate TLS supplies the actual object/descriptor to the
pose callback. The source Rig remains Barrel.rig; the destination MetaRig may be
merged, and native remap8→DST must name scope_hinge there. In-flight context[5]
must match that MetaRig, and all destination bounds are checked.

The apply path promotes weak player/weapon/Barrel/Receiver/control handles to
temporary strong handles after its owner fast check, revalidates the current target
under the rig lock, contains native accesses in an SEH helper, then releases strong
handles outside the lock. Requests expire after200ms; disable/holster/player/device
changes refuse further writes. Missing LOD mapping is skipped without a LOD0
fallback, so the next native mapped copy supplies the restored rotation.

Pure production-policy tests cover equal-rig foreign actors, weak/stale/future
requests, control/Receiver/rig/track/meta replacement, old in-flight context,
merged22-bone mapping, wrong destination names, missing/duplicate/out-of-bounds
maps and unchanged adjacent/T/S data. Production Lua adapter tests cover scope/ADS
gating, refresh, native refusal, inactive cost and holster release. Independent
corpse review approved the corrected source. Full combined build and live proof
of nonzero native applied counts plus stable hip→ADS6→hip images remain required.
