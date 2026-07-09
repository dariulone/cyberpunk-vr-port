#pragma once
// ============================================================================
// CyberpunkVR_Hands_Shared -- THE shared-memory float block (single source of
// truth for slot numbering). All three C++ modules map the SAME named mapping:
//   dxgi_proxy.cpp      (GetShotShared)        -- render/present threads
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
//  [29]       melee impulse              dxgi reads/decrements
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
//  [67..69]    never used (except a brief LT-inject melee-guard experiment, removed same session:
//              the VR guard went STAT-driven — IsBlocking/IsDeflecting set directly by the CET
//              weapon mod, no PSM Block state, no debuffs — so the input channel died unused)
//  [84]        reclaimed by [CAMWRITE] mode flag (was: never used)
//  [100..103]  reclaimed by [CAMWRITE] desired quat (was: never used)
//  [112..115]  old view stabilizer delta+valid (removed session 3)
//  [132..136]  entity velocity/timestamp extrapolation (writer exists in
//              main.cpp, NO consumer; the snap-puppet-pre-rotation speed gate
//              consumed [132..134] briefly -- removed after live test)
//  [137..140]  located camera entity-local (writer removed)
//  [145]       FinalCamera poison-test counter (removed session 3)
//  [154..255]  never used ([151..153] taken by [CAMWRITE] seq/ack/entity-yaw)
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
} // namespace vrshared
