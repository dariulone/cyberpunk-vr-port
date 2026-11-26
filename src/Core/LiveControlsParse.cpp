// PollLiveControls' ini loop (src/Core/LiveControlsPoll.cpp), over LiveControlsValues: the C++ build's parse, and the
// reference rust/crates/cvr-settings is traced against (tools/vrik_tests). The loop is the original, verbatim.
#include "Core/LiveControlsParse.hpp"
#include "Core/CoreInternal.hpp"   // ClampRuntimeMode
#include "Hooks/ReflexOptions.hpp"
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <algorithm>

#if !CVR_RUST_VRIK
// The Rust build parses with rust/crates/cvr-settings (cvr_settings_parse_live_controls); this stays the C++ build's
// parse and the reference the settings trace compares against.
void ParseLiveControlsLines(FILE* file, LiveControlsValues& v, LiveControlsSubsystems& s) {
    LIVE_CONTROLS_BIND_LOCALS(v);
    LIVE_CONTROLS_BIND_ALIASES(v);
    LIVE_CONTROLS_BIND_SUBSYSTEMS(s);
    auto tryParseIntKey = [](const char* text, const char* key, int* outValue) {
        if (!text || !key || !outValue) return false;
        const size_t keyLen = strlen(key);
        if (_strnicmp(text, key, keyLen) != 0) return false;
        const char* cursor = text + keyLen;
        while (*cursor == ' ' || *cursor == '\t') ++cursor;
        if (*cursor != '=') return false;
        ++cursor;
        while (*cursor == ' ' || *cursor == '\t') ++cursor;
        *outValue = atoi(cursor);
        return true;
    };
    char line[128];
    while (fgets(line, sizeof(line), file)) {        float value = 0.0f;
        if (sscanf_s(line, "xr_weapon_laser_dot = %f", &value) == 1) {
            xrWeaponLaserDot = value != 0.0f;
            continue;
        }

        if (sscanf_s(line, "xr_head_offset_x=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_x = %f", &value) == 1) {
            xrHeadOffsetX = value;
            continue;
        }
        if (sscanf_s(line, "xr_head_offset_y=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_y = %f", &value) == 1) {
            xrHeadOffsetY = value;
            continue;
        }
        if (sscanf_s(line, "xr_head_offset_z=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_z = %f", &value) == 1) {
            xrHeadOffsetZ = value;
            continue;
        }
        int intValue = 0;
        if (sscanf_s(line, "xr_recenter=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_recenter = %d", &intValue) == 1) {
            xrRecenter = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_submit = %d", &intValue) == 1) {
            xrMonoSubmit = intValue;
            continue;
        }
        // -1 auto (thread on SteamVR, inline on Virtual Desktop), 0 inline, 1 thread.
        if (sscanf_s(line, "xr_threaded_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_threaded_submit = %d", &intValue) == 1) {
            xrThreadedSubmit = (intValue < 0) ? -1 : (intValue > 0 ? 1 : 0);
            continue;
        }

        if (sscanf_s(line, "xr_window_width=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_window_width = %d", &intValue) == 1) {
            xrWindowWidth = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_window_height=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_window_height = %d", &intValue) == 1) {
            xrWindowHeight = intValue;
            continue;
        }

        if (sscanf_s(line, "xr_force_fov=%f", &value) == 1 ||
            sscanf_s(line, "xr_force_fov = %f", &value) == 1) {
            xrForceFov = value;
            continue;
        }
        if (sscanf_s(line, "xr_menu_rect=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_menu_rect = %d", &intValue) == 1) {
            xrMenuRect = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_menu_fov=%f", &value) == 1 ||
            sscanf_s(line, "xr_menu_fov = %f", &value) == 1) {
            xrMenuFov = value;
            continue;
        }
        if (sscanf_s(line, "xr_menu_follow_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_menu_follow_deg = %f", &value) == 1) {
            xrMenuFollowDeg = value;
            continue;
        }
        if (cvr::hud::ParseLayoutSetting(line,hudElements)) continue;
        if (cvr::framegen::ParseSetting(line,framegen)) continue;
        if (cvr::foveated::ParseSetting(line,foveated)) continue;
        if (cvr::vrs::ParseSetting(line,vrs)) continue;
        if (cvr::hidden_area::ParseSetting(line,hiddenArea)) continue;
        if(sscanf_s(line,"xr_mono_cascades = %d",&intValue)==1){monoCascades=intValue!=0;continue;}
        if(sscanf_s(line,"xr_mono_mode = %d",&intValue)==1){monoMode=intValue!=0;continue;}
        if(sscanf_s(line,"xr_afw = %d",&intValue)==1){afw=intValue!=0;continue;}
        if(sscanf_s(line,"xr_osr = %d",&intValue)==1){osr=intValue!=0;continue;}
        if(sscanf_s(line,"xr_scope_stabilize = %d",&intValue)==1){scopeStabilize=intValue;continue;}
        if (tryParseIntKey(line,"xr_nvidia_reflex",&intValue)) {nvidiaReflex=cvr::reflex::NormalizeMode(intValue);continue;}
        if(sscanf_s(line,"xr_fps_cap = %d",&intValue)==1){fpsCap=intValue!=0;continue;}
        if(sscanf_s(line,"xr_lobby = %d",&intValue)==1){lobby=intValue!=0;continue;}
        if(sscanf_s(line,"xr_lobby_ambient = %d",&intValue)==1){lobbyAmbient=intValue!=0;continue;}
        if (cvr::vrui::ParseSetting(line,overlay)) continue;
        if (sscanf_s(line,"xr_hud_follow_mode = %d",&intValue)==1) { xrHudFollowMode=intValue;continue; }
        if (sscanf_s(line,"xr_hud_stereo_depth = %d",&intValue)==1) { xrHudStereoDepth=intValue;continue; }
        if (sscanf_s(line,"xr_hud_brightness = %f",&value)==1) { xrHudBrightness=value;continue; }
        if (sscanf_s(line,"xr_hud_shadow = %f",&value)==1) { xrHudShadow=value;continue; }
        if (sscanf_s(line,"xr_hud_glow = %f",&value)==1) { xrHudGlow=value;continue; }
        if (sscanf_s(line, "xr_hud_panel = %d", &intValue) == 1) { xrHudPanel=intValue; continue; }
        if (sscanf_s(line, "xr_hud_follow_deg = %f", &value) == 1) { xrHudFollowDeg=value; continue; }
        if (sscanf_s(line, "xr_hud_fov = %f", &value) == 1) { xrHudFov=value; continue; }
        if (sscanf_s(line, "xr_hud_distance = %f", &value) == 1) { xrHudDistance=value; continue; }
        if (sscanf_s(line,"xr_interaction_panel = %d",&intValue)==1){xrInteractionPanel=intValue;continue;}
        if (sscanf_s(line,"xr_interaction_follow_deg = %f",&value)==1){xrInteractionFollowDeg=value;continue;}
        if (sscanf_s(line,"xr_loot_follow_deg = %f",&value)==1){xrLootFollowDeg=value;continue;}
        if (sscanf_s(line,"xr_interaction_distance = %f",&value)==1){xrInteractionDistance=value;continue;}
        if (sscanf_s(line,"xr_interaction_fov = %f",&value)==1){xrInteractionFov=value;continue;}
        if (sscanf_s(line, "xr_pitch_sign=%f", &value) == 1 ||
            sscanf_s(line, "xr_pitch_sign = %f", &value) == 1) {
            xrPitchSign = value < 0.0f ? -1.0f : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_pitch_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_pitch_scale = %f", &value) == 1) {
            xrPitchScale = value > 0.01f ? value : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_sync_sequential=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_sync_sequential = %d", &intValue) == 1) {
            xrSyncSequential = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_3dof_movement=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_3dof_movement = %d", &intValue) == 1) {
            xr3DofMovement = intValue;
            continue;
        }
        if (sscanf_s(line, "first_launch=%d", &intValue) == 1 ||
            sscanf_s(line, "first_launch = %d", &intValue) == 1) {
            xrFirstLaunch = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_motion_predict_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_motion_predict_ms = %f", &value) == 1) {
            xrMotionPredictMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_stereo_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_stereo_scale = %f", &value) == 1) {
            xrStereoScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_world_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_world_scale = %f", &value) == 1) {
            xrWorldScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_ipd_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_ipd_scale = %f", &value) == 1) {
            xrIpdScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_sharpness=%f", &value) == 1 ||
            sscanf_s(line, "xr_sharpness = %f", &value) == 1) {
            xrSharpness = value;
            continue;
        }
        if (sscanf_s(line, "xr_sharpmix=%f", &value) == 1 ||
            sscanf_s(line, "xr_sharpmix = %f", &value) == 1) {
            xrSharpmix = value;
            continue;
        }
        if (sscanf_s(line, "xr_hmd_smooth=%f", &value) == 1 ||
            sscanf_s(line, "xr_hmd_smooth = %f", &value) == 1) {
            xrHmdSmooth = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_per_frame=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hand_per_frame = %d", &intValue) == 1) {
            xrHandPerFrame = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hand_rel_filtered=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hand_rel_filtered = %d", &intValue) == 1) {
            xrHandRelFiltered = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_pistol=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_pistol = %f", &value) == 1) {
            xrLerpPistol = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_rifle=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_rifle = %f", &value) == 1) {
            xrLerpRifle = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_shotgun=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_shotgun = %f", &value) == 1) {
            xrLerpShotgun = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_sniper=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_sniper = %f", &value) == 1) {
            xrLerpSniper = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_melee=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_melee = %f", &value) == 1) {
            xrLerpMelee = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_speed=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_speed = %f", &value) == 1) {
            xrHandLerp = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_framegen_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_framegen_scale = %f", &value) == 1) {
            // 1.5 was the shipped default until 2026-11-05 and every save wrote it back; it reads as the new
            // default, 1.25.
            xrLerpFramegen = std::abs(value - 1.5f) < 0.0001f ? 1.25f : value;
            continue;
        }
        // Three numbers on one line per piece, because twelve keys for one feature is a wall of ini
        // nobody can read. The editor writes them back in exactly this shape.
        {
            float sx = 0.0f, sy = 0.0f, ss = 0.0f;
            int slot = -1;
            if      (sscanf_s(line, "xr_scanner_frame=%f,%f,%f",   &sx, &sy, &ss) == 3) slot = 0;
            else if (sscanf_s(line, "xr_scanner_details=%f,%f,%f", &sx, &sy, &ss) == 3) slot = 1;
            else if (sscanf_s(line, "xr_scanner_hacks=%f,%f,%f",   &sx, &sy, &ss) == 3) slot = 2;
            else if (sscanf_s(line, "xr_scanner_hint=%f,%f,%f",    &sx, &sy, &ss) == 3) slot = 3;
            else if (sscanf_s(line, "xr_scanner_memory=%f,%f,%f",  &sx, &sy, &ss) == 3) slot = 4;
            else if (sscanf_s(line, "xr_scanner_scripts=%f,%f,%f", &sx, &sy, &ss) == 3) slot = 5;
            else if (sscanf_s(line, "xr_scanner_desc=%f,%f,%f",    &sx, &sy, &ss) == 3) slot = 6;
            if (slot >= 0) {
                xrScannerSlots[slot * 3 + 0] = sx;
                xrScannerSlots[slot * 3 + 1] = sy;
                xrScannerSlots[slot * 3 + 2] = ss;
                continue;
            }
        }
        if (sscanf_s(line, "xr_vrcam_composition=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vrcam_composition = %d", &intValue) == 1) {
            xrCompGroup = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_auto_recenter=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_auto_recenter = %d", &intValue) == 1) {
            xrAutoRecenter = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_vive_trackers=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vive_trackers = %d", &intValue) == 1) {
            g_viveTrackersSetting = intValue == 1 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_grip_from_fingers=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_grip_from_fingers = %d", &intValue) == 1) {
            xrGripFromFingers = intValue == 1 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_finger_spread_enabled=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_finger_spread_enabled = %d", &intValue) == 1) {
            xrFingerSpreadOn = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_finger_spread=%f", &value) == 1 ||
            sscanf_s(line, "xr_finger_spread = %f", &value) == 1) {
            xrFingerSpread = std::isfinite(value) ? std::clamp(value, 0.0f, 2.0f) : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_finger_bones=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_finger_bones = %d", &intValue) == 1) {
            xrFingerBones = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_interaction_anchor=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_interaction_anchor = %d", &intValue) == 1) {
            xrInteractionAnchor = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hud_clearance_hands_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_hud_clearance_hands_scale = %f", &value) == 1) {
            xrHudClearanceHandsScale = std::isfinite(value) ? std::clamp(value, 1.0f, 3.0f) : 2.0f;
            continue;
        }
        if (sscanf_s(line, "xr_hud_clearance_hands=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hud_clearance_hands = %d", &intValue) == 1) {
            xrHudClearanceHands = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hud_clearance_grow=%f", &value) == 1 ||
            sscanf_s(line, "xr_hud_clearance_grow = %f", &value) == 1) {
            xrHudClearanceGrow = std::isfinite(value) ? std::clamp(value, 0.0f, 1.0f) : 0.5f;
            continue;
        }
        if (sscanf_s(line, "xr_hud_clearance=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hud_clearance = %d", &intValue) == 1) {
            xrHudClearance = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hud_to_second_eye=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hud_to_second_eye = %d", &intValue) == 1) {
            xrHudTo2 = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_asym_projection=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_asym_projection = %d", &intValue) == 1) {
            xrAsym = intValue == 1 ? 1 : 0;
            continue;
        }
        {
            float g = 0.0f;
            if (sscanf_s(line, "xr_asym_inner_guard=%f", &g) == 1 || sscanf_s(line, "xr_asym_inner_guard = %f", &g) == 1) {
                xrAsymGuard = (g < 0.0f) ? 0.0f : ((g > 1.0f) ? 1.0f : g);
                continue;
            }
        }
        if (sscanf_s(line, "xr_hud_in_braindance=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hud_in_braindance = %d", &intValue) == 1) {
            xrHudBd = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_fix_lod=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_fix_lod = %d", &intValue) == 1) {
            xrFixLod = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_comp_lend_scoped=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_comp_lend_scoped = %d", &intValue) == 1) {
            xrCompLendScoped = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue & 0xF));
            continue;
        }
        if (sscanf_s(line, "xr_expo_mirror=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_expo_mirror = %d", &intValue) == 1) {
            xrExpoMirror = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_expo_field_mask=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_expo_field_mask = %d", &intValue) == 1) {
            xrExpoFieldMask = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue & 0x7F));
            continue;
        }
        if (sscanf_s(line, "xr_distant_reuse=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_distant_reuse = %d", &intValue) == 1) {
            xrDistantReuse = (intValue < 0) ? 0u : static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_local_shadow_reuse=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_local_shadow_reuse = %d", &intValue) == 1) {
            xrLocalShadowReuse = (intValue < 0) ? 0u : static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_gi_reuse=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_gi_reuse = %d", &intValue) == 1) {
            xrGiReuse = (intValue < 0) ? 0u : static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_probe_reuse=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_probe_reuse = %d", &intValue) == 1) {
            xrProbeReuse = (intValue < 0) ? 0u : static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_stable_copy=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_stable_copy = %d", &intValue) == 1) {
            xrStableCopy = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_no_state_lies=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_no_state_lies = %d", &intValue) == 1) {
            xrNoStateLies = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_indirect_census=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_indirect_census = %d", &intValue) == 1) {
            xrIndirectCensus = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_expo_probe=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_expo_probe = %d", &intValue) == 1) {
            xrExpoProbe = (intValue < 0) ? 0 : ((intValue > 2) ? 2 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_run_view_params=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_run_view_params = %d", &intValue) == 1) {
            xrRunViewParams = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue > 2 ? 2 : intValue));
            continue;
        }
        if (sscanf_s(line, "xr_vrcam_cam_fields=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_vrcam_cam_fields = %x", &intValue) == 1) {
            xrVrcamCamFields = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue & 0xF));
            continue;
        }
        if (sscanf_s(line, "xr_force_vrcam_cam=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_force_vrcam_cam = %d", &intValue) == 1) {
            xrForceVrcamCam = intValue ? 1u : 0u;
            continue;
        }
        if (sscanf_s(line, "xr_cap_grant=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_cap_grant = %d", &intValue) == 1) {
            xrCapGrant = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_light_content=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_light_content = %d", &intValue) == 1) {
            xrLightContent = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_quat_from_buffer=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_quat_from_buffer = %d", &intValue) == 1) {
            xrBdQuatFromBuffer = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_bd_ipd_in_locate=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_ipd_in_locate = %d", &intValue) == 1) {
            xrBdIpdInLocate = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_dev_cam_tol_m=%f", &value) == 1 ||
            sscanf_s(line, "xr_dev_cam_tol_m = %f", &value) == 1) {
            xrDevCamTolM = value;
            continue;
        }
        if (sscanf_s(line, "xr_dev_cam_any_name=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_dev_cam_any_name = %d", &intValue) == 1) {
            xrDevCamAnyName = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_input_default_in_ui=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_input_default_in_ui = %d", &intValue) == 1) {
            xrInputDefaultInUi = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_popup_mag_block_ms=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_popup_mag_block_ms = %d", &intValue) == 1) {
            xrPopupMagBlockMs = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_lens_head_write=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_lens_head_write = %d", &intValue) == 1) {
            xrLensHeadWrite = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_dev_cam_in_locate=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_dev_cam_in_locate = %d", &intValue) == 1) {
            xrDevCamInLocate = intValue;
            continue;
        }
        {
            unsigned int visCsNode = 0;
            if (sscanf_s(line, "xr_vision_cs_node=%x", &visCsNode) == 1 ||
                sscanf_s(line, "xr_vision_cs_node = %x", &visCsNode) == 1) {
                CyberpunkVR_VisionCsNode = visCsNode;
                continue;
            }
        }
        if (sscanf_s(line, "xr_vision_cs_ord=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vision_cs_ord = %d", &intValue) == 1) {
            CyberpunkVR_VisionCsOrd = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vision_cs_dim=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vision_cs_dim = %d", &intValue) == 1) {
            CyberpunkVR_VisionCsDim = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vision_cs_swap=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vision_cs_swap = %d", &intValue) == 1) {
            CyberpunkVR_VisionCsSwap = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vision_to_second_eye=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vision_to_second_eye = %d", &intValue) == 1) {
            xrVisionTo2 = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_weapon_plane_as_scene=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_weapon_plane_as_scene = %d", &intValue) == 1) {
            CyberpunkVR_WeaponPlaneAsScene = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_highlight_any_plane=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_highlight_any_plane = %d", &intValue) == 1) {
            CyberpunkVR_HighlightAnyPlane = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_plane_bucket_scene=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_plane_bucket_scene = %d", &intValue) == 1) {
            CyberpunkVR_PlaneBucketScene = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vrcam_pos_from_main=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vrcam_pos_from_main = %d", &intValue) == 1) {
            xrVrcamPosFromMain = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_bd_quat_from_write_site=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_quat_from_write_site = %d", &intValue) == 1) {
            xrBdQuatWriteSite = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_stable_srgb_view=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_stable_srgb_view = %d", &intValue) == 1) {
            xrStableSrgbView = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_final_grab=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_final_grab = %d", &intValue) == 1) {
            xrFinalGrab = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_comp_lend_set=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_comp_lend_set = %d", &intValue) == 1) {
            xrCompLendSet = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_bd_base_from_locate=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_base_from_locate = %d", &intValue) == 1) {
            xrBdBaseFromLocate = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_editor_align=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_editor_align = %d", &intValue) == 1) {
            xrBdEditorAlign = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_one_composition=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_one_composition = %d", &intValue) == 1) {
            xrBdOneComp = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_push_base=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_push_base = %d", &intValue) == 1) {
            xrBdPushBase = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_main_pos_from_scene=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_main_pos_from_scene = %d", &intValue) == 1) {
            xrBdMainPosScene = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_push_transform=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_push_transform = %d", &intValue) == 1) {
            if (intValue >= 0 && intValue <= 3) xrBdPushTransform = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_bd_cam_dirty=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_cam_dirty = %d", &intValue) == 1) {
            if (intValue >= 0 && intValue <= 3) xrBdCamDirty = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_env_mirror_mask=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_env_mirror_mask = %x", &intValue) == 1) {
            xrEnvMirrorMask = static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_viewdata_fix_mask=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_viewdata_fix_mask = %x", &intValue) == 1) {
            xrViewDataFixMask = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_render_mask_grant=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_render_mask_grant = %d", &intValue) == 1) {
            xrRenderMaskGrant = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_stage_replay=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_stage_replay = %d", &intValue) == 1) {
            xrStageReplay = intValue != 0;
            continue;
        }
        if (sscanf_s(line, "xr_vrcam_gbuffer_partitions=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vrcam_gbuffer_partitions = %d", &intValue) == 1) {
            xrGbufferPartitions = intValue != 0;
            continue;
        }
        if (sscanf_s(line, "xr_scope_gbuffer_partitions=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_scope_gbuffer_partitions = %d", &intValue) == 1) {
            xrScopeGbufferPartitions = intValue != 0;
            continue;
        }
        if (sscanf_s(line, "xr_scope_graph_dag=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_scope_graph_dag = %d", &intValue) == 1) {
            xrScopeGraphDag = intValue != 0;
            continue;
        }
        if (sscanf_s(line, "xr_submit_chain=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_submit_chain = %d", &intValue) == 1) {
            xrSubmitChain = intValue < 0 ? 0 : intValue > 2 ? 2 : intValue;
            continue;
        }
        if (sscanf_s(line, "xr_defer_present=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_defer_present = %d", &intValue) == 1) {
            xrDeferPresent = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_gt_skip_render_queue=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_gt_skip_render_queue = %d", &intValue) == 1) {
            xrGtSkipRenderQueue = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_submit_chain_rev=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_submit_chain_rev = %d", &intValue) == 1) {
            xrSubmitChainRev = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_water_mirror=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_water_mirror = %d", &intValue) == 1) {
            xrWaterMirror = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
        }
        if (sscanf_s(line, "xr_water_assign=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_water_assign = %d", &intValue) == 1) {
            xrWaterAssign = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue > 1 ? 1 : intValue));
        }
        if (sscanf_s(line, "xr_viewdata_diff=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_viewdata_diff = %d", &intValue) == 1) {
            xrViewDataDiff = intValue < 0 ? 0 : (intValue > 2 ? 2 : intValue);
        }
        // THE NODE IS HEX, because every RVA in this project is written and read as hex and a node
        // key that silently took decimal would aim the diff at an address nobody meant.
        if (sscanf_s(line, "xr_scene_stick_yaw_dps=%f", &value) == 1 ||
            sscanf_s(line, "xr_scene_stick_yaw_dps = %f", &value) == 1) {
            xrSceneYawDps = value < 0.0f ? 0.0f : (value > 360.0f ? 360.0f : value);
            continue;
        }
        if (sscanf_s(line, "xr_scene_stick_yaw_tier=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_scene_stick_yaw_tier = %d", &intValue) == 1) {
            xrSceneYawTier = intValue < 0 ? 0 : (intValue > 4 ? 4 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_lut_src_lend=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_lut_src_lend = %d", &intValue) == 1) {
            xrLutSrcLend = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue > 1 ? 1 : intValue));
        }
        if (sscanf_s(line, "xr_block_diff=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_block_diff = %d", &intValue) == 1) {
            xrBlockDiff = intValue < 0 ? 0 : (intValue > 2 ? 2 : intValue);
        }
        if (sscanf_s(line, "xr_block_diff_node=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_block_diff_node = %x", &intValue) == 1) {
            xrBlockDiffNode = static_cast<uint32_t>(intValue);
        }
        if (sscanf_s(line, "xr_block_diff_size=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_block_diff_size = %d", &intValue) == 1) {
            xrBlockDiffSize = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
        }
        if (sscanf_s(line, "xr_env_extra_mask=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_env_extra_mask = %d", &intValue) == 1) {
            xrEnvExtraMask = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        // HEX, LIKE ITS SIBLING xr_env_mirror_mask. It was %d while the value is a bit mask that
        // everyone -- the table, the comments, this session -- writes in hex, so "7FE0003" parsed as
        // 7 and stopped at the F. A measurement was reported on the strength of that, and the number
        // that produced it was never the number in the file.
        if (sscanf_s(line, "xr_grade_mirror_mask=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_grade_mirror_mask = %x", &intValue) == 1) {
            xrGradeMirrorMask = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_two_hand_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_two_hand_radius = %f", &value) == 1) {
            xrTwoHandRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_two_hand_aim=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_two_hand_aim = %d", &intValue) == 1) {
            xrTwoHandAim = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_two_hand_break=%f", &value) == 1 ||
            sscanf_s(line, "xr_two_hand_break = %f", &value) == 1) {
            xrTwoHandBreak = value;
            continue;
        }
        if (sscanf_s(line, "xr_two_hand_aim_release_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_two_hand_aim_release_ms = %f", &value) == 1) {
            xrTwoHandAimRelease = value;
            continue;
        }
        if (sscanf_s(line, "xr_carry_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_carry_radius = %f", &value) == 1) {
            xrCarryRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_rifle_rise=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_rifle_rise = %f", &value) == 1) {
            xrRifleRise = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_rifle_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_rifle_back = %f", &value) == 1) {
            xrRifleBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_shotgun_rise=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_shotgun_rise = %f", &value) == 1) {
            xrShotgunRise = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_shotgun_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_shotgun_back = %f", &value) == 1) {
            xrShotgunBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_down_pistol=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_down_pistol = %f", &value) == 1) {
            xrDownPistol = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_down_rifle=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_down_rifle = %f", &value) == 1) {
            xrDownRifle = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_down_shotgun=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_down_shotgun = %f", &value) == 1) {
            xrDownShotgun = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_back_cm=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_back_cm = %f", &value) == 1) {
            xrBackCm = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_back_max_cm=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_back_max_cm = %f", &value) == 1) {
            xrBackMaxCm = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_twohand_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_twohand_back = %f", &value) == 1) {
            xrTwoHandBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_sniper_rise=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_sniper_rise = %f", &value) == 1) {
            xrSniperRise = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_sniper_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_sniper_back = %f", &value) == 1) {
            xrSniperBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_scale = %f", &value) == 1) {
            xrRecoilScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_haptics=%f", &value) == 1 ||
            sscanf_s(line, "xr_haptics = %f", &value) == 1) {
            xrHaptics = value != 0.0f ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_haptic_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_haptic_scale = %f", &value) == 1) {
            xrHapticScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_pistol_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_pistol_back = %f", &value) == 1) {
            xrPistolBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_climb=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_climb = %f", &value) == 1) {
            xrClimbFrac = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_climb_max_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_climb_max_deg = %f", &value) == 1) {
            xrClimbMax = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_climb_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_climb_ms = %f", &value) == 1) {
            xrClimbMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_return_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_return_ms = %f", &value) == 1) {
            xrReturnMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_return_pow=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_return_pow = %f", &value) == 1) {
            xrReturnPow = value;
            continue;
        }
        if (sscanf_s(line, "xr_render_pose_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_render_pose_submit = %d", &intValue) == 1) {
            xrRenderPoseSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_reuse_last_frame=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_reuse_last_frame = %d", &intValue) == 1) {
            xrReuseLastFrame = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_pair_lock=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_pair_lock = %d", &intValue) == 1) {
            xrPairLock = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_pose_lag=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_pose_lag = %d", &intValue) == 1) {
            xrPoseLag = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_runtime=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_runtime = %d", &intValue) == 1) {
            xrRuntime = ClampRuntimeMode(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_depth_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_depth_submit = %d", &intValue) == 1) {
            xrDepthSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_movement_control=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_movement_control = %d", &intValue) == 1) {
            xrMovementControl = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_disable_mouse_y=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_disable_mouse_y = %d", &intValue) == 1) {
            xrDisableMouseY = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_xinput_hook=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_xinput_hook = %d", &intValue) == 1) {
            xrXInputHook = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn = %d", &intValue) == 1) {
            xrSnapTurn = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_angle_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_snap_turn_angle_deg = %f", &value) == 1) {
            xrSnapTurnAngleDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_movement_source=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_movement_source = %d", &intValue) == 1) {
            xrMovementSource = intValue;
            continue;
        }
        if(sscanf_s(line,"xr_movement_speed_mode = %d",&intValue)==1){xrMovementSpeedMode=intValue;continue;}
        if(sscanf_s(line,"xr_sprint_mode = %d",&intValue)==1){xrSprintMode=intValue;continue;}
        if(sscanf_s(line,"xr_left_stick_deadzone = %f",&value)==1){xrLeftStickDeadzone=value;continue;}
        if(sscanf_s(line,"xr_right_stick_deadzone = %f",&value)==1){xrRightStickDeadzone=value;continue;}
        if(sscanf_s(line,"xr_max_input_threshold = %f",&value)==1){xrMaxInputThreshold=value;continue;}
        if (sscanf_s(line, "xr_cutscene_suspend_tier=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_cutscene_suspend_tier = %d", &intValue) == 1) {
            xrCutsceneSuspendTier = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vehicle_cutscene_suspend_tier=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vehicle_cutscene_suspend_tier = %d", &intValue) == 1) {
            xrVehicleCutsceneSuspendTier = intValue;
            vehicleCutsceneTierConfigured = true;
            continue;
        }
        if (sscanf_s(line, "xr_body_free_look_deg=%f", &value) == 1) xrBodyFreeLookDeg=value;
        if (sscanf_s(line, "xr_body_free_look_down_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_body_free_look_down_deg = %f", &value) == 1) xrBodyFreeLookDownDeg=value;
        if (sscanf_s(line, "xr_body_free_look_swim_deg=%f", &value) == 1) xrBodyFreeLookSwimDeg=value;
        if (sscanf_s(line, "xr_body_move_radius=%f", &value) == 1) xrBodyMoveRadius=value;
        if (sscanf_s(line, "xr_scene_cam_comfort_deg=%f", &value) == 1) xrSceneCamComfortDeg=value;
        if (sscanf_s(line, "xr_physical_crouch=%d", &intValue) == 1) xrPhysicalCrouch=intValue;
        if (sscanf_s(line, "xr_physical_crouch_depth=%f", &value) == 1) xrPhysicalCrouchDepth=value;
        if (sscanf_s(line, "xr_ladder_grip_climb=%d", &intValue) == 1) xrLadderGripClimb=intValue;
        if (sscanf_s(line, "xr_left_handed=%d", &intValue) == 1) xrLeftHanded=intValue;
        if (sscanf_s(line, "xr_left_handed_bindings=%d", &intValue) == 1) xrLeftHandedBindings=intValue;
        if (sscanf_s(line, "xr_ladder_auto_finish=%d", &intValue) == 1) xrLadderAutoFinish=intValue;
        if (sscanf_s(line, "xr_ladder_finish_distance=%f", &value) == 1) xrLadderFinishDistance=value;
        if (sscanf_s(line, "xr_breaststroke_swim=%d", &intValue) == 1) {
            xrBreaststrokeSwim=intValue;
            continue;
        }
        if (sscanf_s(line, "xr_roomscale_movement=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_roomscale_movement = %d", &intValue) == 1) {
            xrRoomscaleMovement = intValue;
        }
        if (sscanf_s(line, "xr_tracked_body_rotation = %d", &intValue) == 1) xrTrackedBodyRotation=intValue;
        if (sscanf_s(line, "xr_hybrid_body_rotation = %d", &intValue) == 1) xrHybridBodyRotation=intValue;
        if (sscanf_s(line, "xr_physical_body_rotation=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_physical_body_rotation = %d", &intValue) == 1) {
            xrPhysicalBodyRotation = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_xinput_install=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_xinput_install = %d", &intValue) == 1) {
            xrXInputInstall = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_input_actions=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_input_actions = %d", &intValue) == 1) {
            xrInputActions = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_xqueue_wait=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_xqueue_wait = %d", &intValue) == 1) {
            xrMonoXQueueWait = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_pulse_ms=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn_pulse_ms = %d", &intValue) == 1) {
            xrSnapTurnPulseMs = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_depth_capture=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_depth_capture = %d", &intValue) == 1) {
            xrMonoDepthCapture = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_yaw_index=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn_yaw_index = %d", &intValue) == 1) {
            xrSnapTurnYawIndex = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_immersive_holsters=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_immersive_holsters = %d", &intValue) == 1) {
            xrImmersiveHolsters = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_body_hands_only=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_body_hands_only = %d", &intValue) == 1) {
            xrBodyHandsOnly = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_x=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_x = %f", &value) == 1) {
            xrVehHeadOffsetX = value;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_y=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_y = %f", &value) == 1) {
            xrVehHeadOffsetY = value;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_z=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_z = %f", &value) == 1) {
            xrVehHeadOffsetZ = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_grab=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_wheel_grab = %d", &intValue) == 1) {
            xrWheelGrab = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_menu_gesture=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_menu_gesture = %d", &intValue) == 1) {
            xrMenuGesture = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vr_weapon_wheel=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vr_weapon_wheel = %d", &intValue) == 1) {
            xrVrWeaponWheel = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_radius = %f", &value) == 1) {
            xrWheelRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_steer_max_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_steer_max_deg = %f", &value) == 1) {
            xrWheelSteerMaxDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_steer_dead_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_steer_dead_deg = %f", &value) == 1) {
            // 1.5 was the shipped default until 2026-11-04 (a wheel with play round its centre); it reads as the
            // new default, none.
            xrWheelSteerDeadDeg = std::abs(value - 1.5f) < 0.0001f ? 0.0f : value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_prediction=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_wheel_prediction = %d", &intValue) == 1) {
            xrWheelPrediction = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_prediction_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_prediction_ms = %f", &value) == 1) {
            xrWheelPredictionMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_horn=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_wheel_horn = %d", &intValue) == 1) {
            xrWheelHorn = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_horn_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_horn_radius = %f", &value) == 1) {
            xrWheelHornRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_vehicle_gun_trigger=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vehicle_gun_trigger = %d", &intValue) == 1) {
            xrVehicleGunTrigger = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vehicle_throttle_trim=%f", &value) == 1 ||
            sscanf_s(line, "xr_vehicle_throttle_trim = %f", &value) == 1) {
            xrVehicleThrottleTrim = value;
            continue;
        }

    }
}
#endif

#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's is rust/crates/cvr-render/src/live_controls_poll.rs's
extern "C" bool cvr_cpp_settings_parse_subsystem(int which, const char* line, LiveControlsSubsystems* s) {
    if (!line || !s) return false;
    switch (which) {
    case 0: return cvr::hud::ParseLayoutSetting(line, s->hudElements);
    case 1: return cvr::framegen::ParseSetting(line, s->framegen);
    case 2: return cvr::foveated::ParseSetting(line, s->foveated);
    case 3: return cvr::vrs::ParseSetting(line, s->vrs);
    case 4: return cvr::hidden_area::ParseSetting(line, s->hiddenArea);
    case 5: return cvr::vrui::ParseSetting(line, s->overlay);
    default: return false;
    }
}
#endif
