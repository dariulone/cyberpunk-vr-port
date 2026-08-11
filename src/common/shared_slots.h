#pragma once
// ============================================================================
// CyberpunkVR_Hands_Shared -- THE shared-memory float block (single source of
// truth for slot numbering). All three C++ modules map the SAME named mapping:
//   vr_core.cpp      (GetShotShared)        -- render/present threads
//   openxr_manager.cpp  (SetSharedSlot/sShared) -- OpenXR present thread
//   red4ext_plugin      (g_pSharedHands)        -- game/anim threads + Lua natives
// Size: 1024 bytes = 256 floats (mapped as 1024 in every module).
//
// NUMBERING IS FROZEN. CET Lua reads RAW indices through GetVRSharedSlot
// (Holster mod: [20..23], [49]) -- renumbering breaks shipped mods. New data
// takes the lowest free slot from the GRAVEYARD below or fresh space >= [151].
//
// Cross-thread rules:
//  * [127] seqlock (odd = write in progress) brackets the OpenXR hand/HMD
//    publish; the plugin latches a whole coherent frame (RefreshHandsSnapshot).
//  * [143] seqlock brackets the dxgi render-view packet [104..111] + [141..142].
//  * Everything else is single-writer / relaxed (float-atomic on x64).
//
// ---------------------------------------------------------------------------
// LIVE SLOTS (writer -> readers)
// ---------------------------------------------------------------------------
//  [0]        left hand valid          openxr -> plugin, Lua
//  [1..3]     left hand pos (HMD-local)  openxr -> plugin, Lua
//  [4..7]     left hand quat             openxr -> plugin, Lua
//  [8]        right hand valid           openxr -> plugin, Lua
//  [9..11]    right hand pos (HMD-local) openxr -> plugin, Lua
//  [12..15]   right hand quat            openxr -> plugin, Lua
//  [16..19]   HMD orientation quat       openxr -> plugin, Lua
//  [20..22]   hand->holster distances    plugin (hook) -> Holster Lua (RAW idx!)
//  [23]       holster mode simple/immersive  openxr -> Holster Lua (RAW idx!)
//  [24..26]   muzzle forward             plugin (SetVRMuzzleQuat) -> overlay
//  [27]       muzzle valid               plugin -> overlay
//  [28]       zoom level                 plugin -> overlay
//  [29]       melee impulse              dxgi reads/decrements (CET weapon mod
//             pulses it on a VR hand swing; the plugin's kick detector pulses
//             it on a foot strike -- fix15, gated by [140])
//  [30]       right trigger held (bool)  dxgi -> plugin native
//  [31]       in-vehicle flag            dxgi -> plugin hook (arms-only VRIK in vehicles)
//  [32]       hand-tracking / VRIK bind request   openxr -> plugin
//  [33..48]   calibration valid/values/diag req   openxr -> plugin
//  [49]       right grip analog          dxgi -> Holster Lua (RAW idx!)
//  [50..56]   weapon/shot bridge         dxgi
//  [57]       shot frame flag            weapon_aim_hook -> dxgi
//  [58]       weapon-aim enable          openxr -> dxgi
//  [59]       weapon-aim mode            openxr -> dxgi
//  [60..66]   weapon/shot bridge         dxgi
//  [67..69]   -- free (graveyard)
//  [70..76]   shoulder calibration       openxr -> plugin
//  [77..80]   arm length / eye height    openxr -> plugin
//  [81]       menu / world-map flag      plugin (redscript bridge) -> dxgi
//  [82]       fppCamera chain max rot deviation from rest (deg)  plugin -> dxgi [RENDERCAM]
//  [83]       [82]'s joint: 0=Control_GRP 1=Aim_JNT 2=Target_JNT 3=UpOffset_GRP 4=Up_GRP (-1 none)
//  [84]       [CAMWRITE] mode flag (1 = engine-native camera write)  dxgi -> VRIK Lua
//  [85..88]   camera->head bake offset + valid   plugin (hook) -> openxr
//  [89..90]   physical height / neck pivot       openxr -> plugin
//  [91..93]   active-cam bake offset             openxr -> plugin (hook)
//  [94..95]   render eye / half IPD              dxgi
//  [96..98]   entity world pos                   plugin (Lua push) -> dxgi, hook
//  [99]       entity push seq                    plugin -> dxgi, hook (tick clock)
//  [100..103] [CAMWRITE] desired world camera quat  dxgi -> VRIK Lua (torn-read
//             guarded by double-reading [151] around the quat, no seqlock)
//  [104..107] render view quat            dxgi -> hook   (seqlock [143])
//  [108..110] world translation delta     dxgi -> hook   (seqlock [143])
//  [111]      view-pose semantics flag (2.0 = delta v2)  dxgi -> hook
//  [112..114] coherent hand anchor (this-sample head pos)  openxr -> hook
//  [115]      hand-anchor valid           openxr -> hook
//             (LIVE since the arm-shake fix -- the graveyard note below
//             claiming these were the removed view stabilizer was stale;
//             verified by grep in fix10. Do NOT reclaim.)
//  [116..119] eye-view offset + valid     plugin (hook) -> dxgi
//  [120..123] total view offset + valid   dxgi -> hook
//  [124..126] HMD position                openxr -> hook ([126] is HMD Z!)
//  [127]      hands seqlock               openxr writer; hook/native readers
//  [128..130] clean camera pair (local)   plugin -> dxgi
//  [131]      clean-pair seq              plugin -> dxgi (pairs with [99])
//  [141]      render-fresh game heading (rad)   dxgi -> hook  (seqlock [143])
//  [142]      heading valid                     dxgi -> hook
//  [143]      view-packet seqlock               dxgi writer
//  [144]      weapon-equipped flag              dxgi -> overlay laser gate
//  [146]      snap yaw delta (rad)              dxgi -> hook (packet rotation)
//  [147]      snap event counter                dxgi -> hook (replay break)
//  [148]      pre-snap heading                  dxgi -> hook (double-apply guard)
//  [149]      snap consumed ack (= last [147] the solve processed)  hook -> dxgi
//             (diagnostic only since the ONE-TICK VIEW HOLD: release is tick-driven.)
//  [150]      snap event tick stamp (= [99] at publish)  dxgi -> hook
//  [151]      [CAMWRITE] publish seq (written LAST after [100..103])  dxgi -> VRIK Lua
//  [152]      [CAMWRITE] Lua ack (= last [151] applied via SetVRCamAck)  Lua -> dxgi
//  [153]      [CAMWRITE] entity world yaw (deg)  plugin (SetVRPlayerYaw batch) -> dxgi
//             (mode-1 heading source: the camera quat can't serve once WE compose it)
//             ONE-TICK VIEW HOLD protocol (v3, trace-proven mechanism): the entity/
//             puppet world yaw applies one TICK after the camera turns; sprint locks
//             puppet yaw to the heading, so the animated body+arms rendered one frame
//             in the old orientation = the sprint-only snap ghost. dxgi holds the view
//             (and the published [141]) one snap-delta back for the locates of the
//             snap tick, releasing when [99] advances; the hook DEFERS the packet
//             rotation while [99] == [150] so the held frame keeps the pre-snap pose.
//
// ---------------------------------------------------------------------------
// GRAVEYARD (dead -- reclaim before growing past [150])
// ---------------------------------------------------------------------------
//  [69]        never used (it was [67..69] once: a brief LT-inject melee-guard experiment, removed
//              the same session — the VR guard went STAT-driven, IsBlocking/IsDeflecting set
//              directly by the CET weapon mod, no PSM Block state, no debuffs — so the input
//              channel died unused).
//              [67] AND [68] ARE NOT FREE and this line used to say they were. [67] is the
//              hand-sample stamp written inside the hands seqlock, [68] a QPC millisecond
//              timestamp in the view packet. Both are large numbers. The smoking mod's CET bridge
//              read them as "left trigger" and "left grip" and got a lighter that ignited by
//              itself and a grip that was never released.
//  [84]        reclaimed by [CAMWRITE] mode flag (was: never used)
//  [100..103]  reclaimed by [CAMWRITE] desired quat (was: never used)
//  [112..115]  NOT FREE -- the graveyard note here was stale: this is the
//             coherent hand anchor (see the LIVE list above), written every
//             frame by openxr since the arm-shake fix. (Was: old view
//             stabilizer delta+valid, removed session 3, then reused.)
//  [132..136]  entity velocity/timestamp extrapolation (writer exists in
//              main.cpp, NO consumer; the snap-puppet-pre-rotation speed gate
//              consumed [132..134] briefly -- removed after live test)
//  [137..139]  RECLAIMED (fix10): ACTIVE left foot mount quat, hemisphere-
//             packed x,y,z (w = +sqrt(1-|xyz|^2) at the reader)  openxr -> plugin
//             (was: located camera entity-local, writer removed)
//  [140]       RECLAIMED (fix15): kick-damage enable (vrport.ini
//             xr_leg_kick_damage, F10 checkbox)  openxr -> plugin
//             The plugin's kick detector reads it and pulses the melee RT
//             impulse [29] on a fast foot strike (empty hands, on foot only).
//  [145]       reclaimed by the tracker debug gizmo (L dorsal z; was: FinalCamera poison
//              test counter, removed session 3)
//  [154..255]  mostly unused, but NOT a blank cheque -- [200..202] carry a right-hand debug
//              position read by the overlay, [227..230] an XR pose quaternion read by
//              vrik_hook, [203..207]/[250..255] the tracker debug gizmo and [233..249] the
//              mount-calibration diagnostics. Check with a grep, not with this comment.
//  [154]       left trigger analog (0..1)     plugin -> Smoking CET bridge
//  [155]       left grip pressed (0/1)        plugin -> Smoking CET bridge
//  [156]       DEBUG logging on (0/1)         plugin -> every CET bridge
//  [157..189]  BODY TRACKERS (XR_HTCX_vive_tracker_interaction; works with real
//             Vive Trackers and with trackers that emulate them).
//             Written by openxr INSIDE the hands seqlock [127] (same bracket as
//             [0..126]), latched by the plugin together with the hands block --
//             the legs block of RefreshHandsSnapshot reads these, SharedLeg(i)
//             is the accessor (i = absolute slot 157..189).
//  [157]      left foot tracker valid (0/1)   openxr -> plugin, overlay
//  [158..160] left foot pos (HMD-local, same convention as [1..3])
//  [161..164] left foot quat
//  [165]      right foot tracker valid (0/1)
//  [166..168] right foot pos (HMD-local)
//  [169..172] right foot quat
//  [173]      body-tracker enable (vrport.ini xr_leg_trackers)  openxr -> plugin
//  [174]      T-pose measured leg length, hip->ankle (m)        openxr -> plugin
//  [175]      tracker->ankle vertical offset (m)                openxr -> plugin
//  [176]      leg calibration valid (0/1)
//  [177]      connected trackers with a body role (0..3)        openxr -> overlay
//  [178..180] ACTIVE right foot mount quat, hemisphere-packed x,y,z
//             (fix10; was: foot mount euler sliders, dead since fix6)  openxr -> plugin
//  [181]      waist tracker valid (0/1)       openxr -> plugin, overlay
//  [182..184] waist pos (HMD-local, same convention as feet)
//  [185..188] waist quat
//  [189]      waist tracker enable (vrport.ini xr_waist_tracker) openxr -> plugin
//  [190]      T-pose mount-calibration sampling flag (1 = window open)  openxr -> plugin
//  [191..194] T-pose SOLVED left foot mount quat    plugin -> openxr
//             (fix11: yaw-solve result post-composed with the boot-mesh visual
//              fix from vrik_footfix.ini, folded through the tracker frame)
//  [195..198] T-pose SOLVED right foot mount quat   plugin -> openxr
//  [199]      solved-mount publish seq (incremented each solve)  plugin -> openxr
//  [208..217] + [221..223] LEGACY VRIK SOLVE STATS  plugin (hook) -> dxgi present
//             (predates the tracker work; openxr_present.cpp logs them
//             periodically and zeroes the peaks [213]/[216]/[217]):
//             [208] AnimPoseMatch calls, [209] fresh solves, [210] replays,
//             [211] pose-age sum ms, [213] pose-age peak, [214] hand seqs
//             consumed, [215] view-age sum ms, [216] view-age peak, [217]
//             head-turn peak deg, [221..222] anchor-gap sum/peak mm,
//             [223] coherent-frame count.
//             WARNING (fix10): the tracker ACTIVE mounts were first assigned
//             to [208..215] on top of this block -- the stats stomped the
//             mounts every frame and the feet ran uncorrected. Mounts moved
//             to [137..139]/[178..180]; do NOT reuse [208..223].
//  [233..249] T-pose mount-calibration DIAGNOSTICS  plugin -> openxr
//             (written on the same falling edge as [191..199]; read at adoption)
//  [233..236] averaged TARGET L foot orientation (model quat; fix10: the
//             tracker's own orientation yaw-rotated so the toe lands on
//             body forward -- pitch/roll stay with the tracker)
//  [237..240] averaged TARGET R foot orientation
//  [241..243] measured animation TOE DIRECTION L (model space unit vector)
//  [244..246] measured animation TOE DIRECTION R
//  [247]      signed yaw correction L (deg about +Z; 0 with toe (0,0,0)
//             means no toe bone resolved -> raw animation target was used)
//  [248]      signed yaw correction R (deg)
//  [249]      diag block valid for the last [199] seq (1.0)
//             ([231..232] stay the legacy hand-shake slots zeroed in present)
//  [203..207] + [145], [250..255] TRACKER ORIENTATION DEBUG GIZMO  plugin -> overlay
//             (published every frame a foot tracker drives the foot; the F10
//             overlay's "tracker orientation debug axes" checkbox draws them)
//  [203..205] SOLVED left foot VISUAL TOE direction (model space unit vector)
//  [206..207] + [145] SOLVED left foot VISUAL DORSAL direction (sole normal)
//  [250..252] SOLVED right foot VISUAL TOE direction
//  [253..255] SOLVED right foot VISUAL DORSAL direction
//             ([203..207] were free; [145] and [250..255] reclaimed graveyard --
//             [145] was the removed FinalCamera poison counter, [250..255] never
//             used. Visual axes = footTrackRot * bone-local toe/dorsal constants;
//             the overlay draws them next to the ground-truth target frame.)
//             ([190..199] and [137..139]/[178..180] ride the tracker feature;
//             read raw, they change at calibration time only. The plugin solves
//             each mount during the T-pose window -- replaces the old shared
//             manual euler sliders, dead since fix6. [200..202] stay the
//             overlay debug pos.)
// ============================================================================

namespace vrshared {
constexpr int kSlotCount = 256;         // mapped bytes / sizeof(float)
constexpr int kMappingBytes = 1024;

// Seqlocks
constexpr int kHandsSeqlock      = 127;
constexpr int kViewPacketSeqlock = 143;

// Frequently used anchors (adopt in new code; existing numeric uses are legacy)
constexpr int kEntityPosX   = 96;
constexpr int kEntitySeq    = 99;
constexpr int kViewQuat     = 104;   // ..107
constexpr int kViewDelta    = 108;   // ..110
constexpr int kViewFlag     = 111;
constexpr int kCleanPair    = 128;   // ..130
constexpr int kCleanPairSeq = 131;
constexpr int kHeading      = 141;
constexpr int kHeadingValid = 142;
constexpr int kWeaponFlag   = 144;
constexpr int kSnapDelta    = 146;
constexpr int kSnapCounter  = 147;
constexpr int kSnapPreHeading = 148;
// Left-hand inputs. The right grip lives at the legacy [49]; these two had no channel until the
// smoking mod needed them, so they are named rather than numbered -- picking a slot off the map's
// word alone is exactly what put the lighter on a millisecond timestamp.
constexpr int kLeftTriggerAnalog = 154;
constexpr int kLeftGripPressed   = 155;
// The launcher's DEBUG checkbox, republished so the Lua side obeys the same switch as the plugin.
// Without it every CET bridge logged per frame unconditionally: 26 449 lines and 5 MB from the
// smoking one alone in a single session, and as much again from the weapon one. A log nobody can
// open is a log nobody reads.
constexpr int kDebugLog          = 156;
// Body trackers (see the [157..189] block above).
constexpr int kLegTrackL         = 157;   // ..164: valid, pos(3), quat(4)
constexpr int kLegTrackR         = 165;   // ..172: valid, pos(3), quat(4)
constexpr int kLegTrackEnable    = 173;
constexpr int kLegLen            = 174;
constexpr int kLegAnkleOffset    = 175;
constexpr int kLegCalibValid     = 176;
constexpr int kViveTrackerCount  = 177;
constexpr int kLegMountQuatR     = 178;   // ..180: ACTIVE R mount quat, hemisphere-packed xyz (was kLegMountEuler)
constexpr int kWaistTrack        = 181;   // ..188: valid, pos(3), quat(4)
constexpr int kWaistTrackEnable  = 189;
constexpr int kMountCalibSampling = 190;  // T-pose window flag
constexpr int kMountSolveL       = 191;   // ..194: plugin-solved L mount quat
constexpr int kMountSolveR       = 195;   // ..198: plugin-solved R mount quat
constexpr int kMountSolveSeq     = 199;
constexpr int kLegMountQuatL     = 137;   // ..139: ACTIVE L mount quat, hemisphere-packed xyz (fix10: was 208, slot collision)
constexpr int kKickDamageEnable  = 140;   // kick-damage checkbox (openxr -> plugin, fix15)
constexpr int kMountDiagTgtL     = 233;   // ..236: averaged straightened target L
constexpr int kMountDiagTgtR     = 237;   // ..240: averaged straightened target R
constexpr int kMountDiagToeL     = 241;   // ..243: animation toe direction L
constexpr int kMountDiagToeR     = 244;   // ..246: animation toe direction R
constexpr int kMountDiagYawL     = 247;   // animation->target correction L (deg)
constexpr int kMountDiagYawR     = 248;   // animation->target correction R (deg)
constexpr int kMountDiagValid    = 249;
// Tracker orientation debug gizmo (plugin -> overlay, every tracked frame).
constexpr int kGizmoToeL         = 203;   // ..205: solved L visual toe dir
constexpr int kGizmoDorsL        = 206;   // ..207 + [145]: solved L visual dorsal dir
constexpr int kGizmoDorsLz       = 145;
constexpr int kGizmoToeR         = 250;   // ..252: solved R visual toe dir
constexpr int kGizmoDorsR        = 253;   // ..255: solved R visual dorsal dir
} // namespace vrshared
