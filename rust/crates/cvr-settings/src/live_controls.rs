//! PollLiveControls' ini parse (formerly the loop in src/Core/LiveControlsPoll.cpp; the C++ reference is
//! src/Core/LiveControlsParse.cpp): every line of vrport.ini read as the C runtime reads it -- `_fsopen` in text mode,
//! `fgets` 128 bytes at a time, `sscanf_s` per key -- into the variables PollLiveControls then applies. Generated with
//! the split from the C++ loop, statement for statement; the subsystems' own keys go to their C++ parsers in place.
#![allow(non_snake_case, unused_parens, clippy::all)]
use crate::crt::{clamp_runtime_mode, cpp_clamp, normalize_reflex_mode, ptr, scan, try_parse_int_key};
use core::ffi::{c_char, c_void};

/// include/Core/LiveControlsParse.hpp LiveControlsValues.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LiveControlsValues {
    pub xrWeaponLaserDot: bool,
    pub xrHeadOffsetX: f32,
    pub xrHeadOffsetY: f32,
    pub xrHeadOffsetZ: f32,
    pub xrRecenter: i32,
    pub xrMonoSubmit: i32,
    pub xrThreadedSubmit: i32,
    pub xrWindowWidth: i32,
    pub xrWindowHeight: i32,
    pub xrForceFov: f32,
    pub xrMenuRect: i32,
    pub xrMenuFov: f32,
    pub xrMenuFollowDeg: f32,
    pub xrHudPanel: i32,
    pub xrHudFollowDeg: f32,
    pub xrHudFov: f32,
    pub xrHudDistance: f32,
    pub xrInteractionPanel: i32,
    pub xrInteractionFollowDeg: f32,
    pub xrInteractionDistance: f32,
    pub xrInteractionFov: f32,
    pub xrLootFollowDeg: f32,
    pub xrHudFollowMode: i32,
    pub xrHudStereoDepth: i32,
    pub xrHudBrightness: f32,
    pub xrHudShadow: f32,
    pub xrHudGlow: f32,
    pub monoCascades: u32,
    pub monoMode: i32,
    pub afw: i32,
    pub osr: i32,
    pub scopeStabilize: i32,
    pub nvidiaReflex: i32,
    pub fpsCap: i32,
    pub lobby: i32,
    pub lobbyAmbient: i32,
    pub xrPitchSign: f32,
    pub xrPitchScale: f32,
    pub xrSyncSequential: i32,
    pub xr3DofMovement: i32,
    pub xrFirstLaunch: i32,
    pub xrMotionPredictMs: f32,
    pub xrStereoScale: f32,
    pub xrWorldScale: f32,
    pub xrIpdScale: f32,
    pub xrSharpness: f32,
    pub xrSharpmix: f32,
    pub xrReuseLastFrame: i32,
    pub xrPairLock: i32,
    pub xrRenderPoseSubmit: i32,
    pub xrPoseLag: i32,
    pub xrRuntime: i32,
    pub xrDepthSubmit: i32,
    pub xrMovementControl: i32,
    pub xrDisableMouseY: i32,
    pub xrXInputHook: i32,
    pub xrSnapTurn: i32,
    pub xrHmdSmooth: f32,
    pub xrHandLerp: f32,
    pub xrLerpPistol: f32,
    pub xrLerpRifle: f32,
    pub xrLerpShotgun: f32,
    pub xrLerpSniper: f32,
    pub xrLerpMelee: f32,
    pub xrLerpFramegen: f32,
    pub xrGradeMirrorMask: u32,
    pub xrEnvExtraMask: u32,
    pub xrWaterMirror: u32,
    pub xrWaterAssign: u32,
    pub xrViewDataDiff: i32,
    pub xrLutSrcLend: u32,
    pub xrSceneYawDps: f32,
    pub xrSceneYawTier: i32,
    pub xrBlockDiff: i32,
    pub xrBlockDiffNode: u32,
    pub xrBlockDiffSize: u32,
    pub xrRenderMaskGrant: u32,
    pub xrStageReplay: i32,
    pub xrGbufferPartitions: i32,
    pub xrScopeGbufferPartitions: i32,
    pub xrScopeGraphDag: i32,
    pub xrSubmitChain: i32,
    pub xrDeferPresent: i32,
    pub xrGtSkipRenderQueue: i32,
    pub xrSubmitChainRev: i32,
    pub xrViewDataFixMask: u32,
    pub xrEnvMirrorMask: u32,
    pub xrBdCamDirty: i32,
    pub xrBdPushTransform: i32,
    pub xrBdMainPosScene: i32,
    pub xrBdPushBase: i32,
    pub xrBdOneComp: i32,
    pub xrBdEditorAlign: i32,
    pub xrBdBaseFromLocate: i32,
    pub xrCompLendSet: u32,
    pub xrFinalGrab: i32,
    pub xrStableSrgbView: i32,
    pub xrBdQuatWriteSite: i32,
    pub xrVrcamPosFromMain: i32,
    pub xrDevCamInLocate: i32,
    pub xrLensHeadWrite: i32,
    pub xrInputDefaultInUi: i32,
    pub xrPopupMagBlockMs: i32,
    pub xrDevCamAnyName: i32,
    pub xrDevCamTolM: f32,
    pub xrBdIpdInLocate: i32,
    pub xrBdQuatFromBuffer: i32,
    pub xrCapGrant: u32,
    pub xrForceVrcamCam: u32,
    pub xrVrcamCamFields: u32,
    pub xrRunViewParams: u32,
    pub xrExpoProbe: i32,
    pub xrIndirectCensus: i32,
    pub xrNoStateLies: i32,
    pub xrStableCopy: i32,
    pub xrDistantReuse: u32,
    pub xrLocalShadowReuse: u32,
    pub xrGiReuse: u32,
    pub xrProbeReuse: u32,
    pub xrExpoMirror: i32,
    pub xrExpoFieldMask: u32,
    pub xrCompLendScoped: u32,
    pub xrLightContent: i32,
    pub xrFixLod: i32,
    pub xrHudTo2: i32,
    pub xrInteractionAnchor: i32,
    pub xrHudClearance: i32,
    pub xrHudClearanceGrow: f32,
    pub xrHudClearanceHands: i32,
    pub xrHudClearanceHandsScale: f32,
    pub xrAutoRecenter: i32,
    pub xrFingerBones: i32,
    pub xrGripFromFingers: i32,
    pub xrFingerSpread: f32,
    pub xrFingerSpreadOn: i32,
    pub xrAsym: i32,
    pub xrAsymGuard: f32,
    pub xrVisionTo2: i32,
    pub xrHudBd: i32,
    pub xrCompGroup: u32,
    pub xrTwoHandRadius: f32,
    pub xrTwoHandAim: i32,
    pub xrTwoHandBreak: f32,
    pub xrTwoHandAimRelease: f32,
    pub xrCarryRadius: f32,
    pub xrRifleRise: f32,
    pub xrRifleBack: f32,
    pub xrShotgunRise: f32,
    pub xrShotgunBack: f32,
    pub xrDownPistol: f32,
    pub xrDownRifle: f32,
    pub xrDownShotgun: f32,
    pub xrBackCm: f32,
    pub xrBackMaxCm: f32,
    pub xrTwoHandBack: f32,
    pub xrSniperRise: f32,
    pub xrSniperBack: f32,
    pub xrRecoilScale: f32,
    pub xrHaptics: i32,
    pub xrHapticScale: f32,
    pub xrPistolBack: f32,
    pub xrClimbFrac: f32,
    pub xrClimbMax: f32,
    pub xrClimbMs: f32,
    pub xrReturnMs: f32,
    pub xrReturnPow: f32,
    pub xrScannerSlots: [f32; 21],
    pub xrHandRelFiltered: i32,
    pub xrHandPerFrame: i32,
    pub xrSnapTurnAngleDeg: f32,
    pub xrMovementSource: i32,
    pub xrMovementSpeedMode: i32,
    pub xrSprintMode: i32,
    pub xrLeftStickDeadzone: f32,
    pub xrRightStickDeadzone: f32,
    pub xrMaxInputThreshold: f32,
    pub xrXInputInstall: i32,
    pub xrInputActions: i32,
    pub xrMonoXQueueWait: i32,
    pub xrSnapTurnPulseMs: i32,
    pub xrMonoDepthCapture: i32,
    pub xrSnapTurnYawIndex: i32,
    pub xrImmersiveHolsters: i32,
    pub xrBodyHandsOnly: i32,
    pub xrPhysicalBodyRotation: i32,
    pub xrTrackedBodyRotation: i32,
    pub xrHybridBodyRotation: i32,
    pub xrRoomscaleMovement: i32,
    pub xrPhysicalCrouch: i32,
    pub xrPhysicalCrouchDepth: f32,
    pub xrBreaststrokeSwim: i32,
    pub xrLadderGripClimb: i32,
    pub xrLeftHanded: i32,
    pub xrLeftHandedBindings: i32,
    pub xrLadderAutoFinish: i32,
    pub xrLadderFinishDistance: f32,
    pub xrBodyFreeLookDeg: f32,
    pub xrBodyFreeLookDownDeg: f32,
    pub xrBodyFreeLookSwimDeg: f32,
    pub xrBodyMoveRadius: f32,
    pub xrSceneCamComfortDeg: f32,
    pub xrCutsceneSuspendTier: i32,
    pub xrVehicleCutsceneSuspendTier: i32,
    pub vehicleCutsceneTierConfigured: bool,
    pub xrVehHeadOffsetX: f32,
    pub xrVehHeadOffsetY: f32,
    pub xrVehHeadOffsetZ: f32,
    pub xrWheelGrab: i32,
    pub xrMenuGesture: i32,
    pub xrVrWeaponWheel: i32,
    pub xrWheelRadius: f32,
    pub xrWheelSteerMaxDeg: f32,
    pub xrWheelSteerDeadDeg: f32,
    pub xrWheelPrediction: i32,
    pub xrWheelPredictionMs: f32,
    pub xrWheelHorn: i32,
    pub xrWheelHornRadius: f32,
    pub xrVehicleGunTrigger: i32,
    pub xrVehicleThrottleTrim: f32,
    pub g_viveTrackersSetting: i32,
    pub CyberpunkVR_VisionCsNode: u32,
    pub CyberpunkVR_VisionCsOrd: i32,
    pub CyberpunkVR_VisionCsDim: i32,
    pub CyberpunkVR_VisionCsSwap: i32,
    pub CyberpunkVR_WeaponPlaneAsScene: i32,
    pub CyberpunkVR_HighlightAnyPlane: i32,
    pub CyberpunkVR_PlaneBucketScene: i32,
    pub xrScopeRrFullResolution: i32,
    pub xrPsvr2HapticBackend: i32,
    pub xrOpenXrHapticGain: f32,
    pub xrOpenXrVehicleHapticGain: f32,
    pub xrVehicleAudioChannels: i32,
    pub xrVehicleAudioRequireGrip: i32,
    pub xrReloadGlow: i32,
    pub xrHeadLedBodyRotation: i32,
    pub xrBikeHeadOffsetX: f32,
    pub xrBikeHeadOffsetY: f32,
    pub xrBikeHeadOffsetZ: f32,
    pub xrVrInterface: i32,
    pub xrHandPhysics: i32,
    pub xrHandCollision: i32,
    pub xrHandCollisionVehicle: i32,
    pub xrHandPush: i32,
    pub xrFingerContact: i32,
    pub xrPropGrab: i32,
    pub xrCarryPicture: i32,
    pub xrCarryLead: f32,
    pub xrContactMargin: f32,
    pub xrWheelAnywhere: i32,
    pub xrBikeBarLockDeg: f32,
    pub xrBikeBodyLean: f32,
    pub savedPlaneAsScene: i32,
}
const _: () = assert!(core::mem::size_of::<LiveControlsValues>() == 1088); // static_assert in src/Core/LiveControlsPoll.cpp

extern "C" {
    fn cvr_cpp_settings_parse_subsystem(which: i32, line: *const c_char, s: *mut c_void) -> bool;
}
unsafe fn subsystem(which: i32, line: *const c_char, s: *mut c_void) -> bool {
    // SAFETY: the C++ parsers read the line and write their own settings.
    unsafe { cvr_cpp_settings_parse_subsystem(which, line, s) }
}

/// One line (a C string, at most 127 bytes and its newline), as the C++ loop body took it.
#[allow(unused_assignments, unused_mut, unused_variables)]
pub unsafe fn parse_line(line: *const c_char, v: &mut LiveControlsValues, s: *mut c_void) {
    // SAFETY (whole body): the caller's line and subsystems.
    unsafe {
        'line: {
            let mut value: f32 = 0.0f32;
            if (scan(line, c"xr_weapon_laser_dot = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrWeaponLaserDot = (value != 0.0f32);
                break 'line;
            }
            if ((scan(line, c"xr_head_offset_x=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_head_offset_x = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHeadOffsetX = value;
                break 'line;
            }
            if ((scan(line, c"xr_head_offset_y=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_head_offset_y = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHeadOffsetY = value;
                break 'line;
            }
            if ((scan(line, c"xr_head_offset_z=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_head_offset_z = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHeadOffsetZ = value;
                break 'line;
            }
            let mut intValue: i32 = 0i32;
            if ((scan(line, c"xr_recenter=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_recenter = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrRecenter = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_mono_submit=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_mono_submit = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrMonoSubmit = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_threaded_submit=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_threaded_submit = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrThreadedSubmit = (if ((intValue < 0i32)) { (-1i32) } else { ((if (intValue > 0i32) { 1i32 } else { 0i32 })) });
                break 'line;
            }
            if ((scan(line, c"xr_window_width=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_window_width = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrWindowWidth = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_window_height=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_window_height = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrWindowHeight = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_force_fov=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_force_fov = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrForceFov = value;
                break 'line;
            }
            if ((scan(line, c"xr_menu_rect=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_menu_rect = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrMenuRect = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_menu_fov=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_menu_fov = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrMenuFov = value;
                break 'line;
            }
            if ((scan(line, c"xr_menu_follow_deg=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_menu_follow_deg = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrMenuFollowDeg = value;
                break 'line;
            }
            if subsystem(0, line, s) {
                break 'line;
            }
            if subsystem(1, line, s) {
                break 'line;
            }
            if subsystem(2, line, s) {
                break 'line;
            }
            if subsystem(3, line, s) {
                break 'line;
            }
            if subsystem(4, line, s) {
                break 'line;
            }
            if (scan(line, c"xr_mono_cascades = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.monoCascades = ((intValue != 0i32) as u32);
                break 'line;
            }
            if (scan(line, c"xr_mono_mode = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.monoMode = ((intValue != 0i32) as i32);
                break 'line;
            }
            if (scan(line, c"xr_afw = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.afw = ((intValue != 0i32) as i32);
                break 'line;
            }
            if (scan(line, c"xr_osr = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.osr = ((intValue != 0i32) as i32);
                break 'line;
            }
            if (scan(line, c"xr_scope_stabilize = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.scopeStabilize = intValue;
                break 'line;
            }
            if try_parse_int_key(line, c"xr_nvidia_reflex", &mut intValue) {
                v.nvidiaReflex = normalize_reflex_mode(intValue);
                break 'line;
            }
            if (scan(line, c"xr_fps_cap = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.fpsCap = ((intValue != 0i32) as i32);
                break 'line;
            }
            if (scan(line, c"xr_lobby = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.lobby = ((intValue != 0i32) as i32);
                break 'line;
            }
            if (scan(line, c"xr_lobby_ambient = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.lobbyAmbient = ((intValue != 0i32) as i32);
                break 'line;
            }
            if subsystem(5, line, s) {
                break 'line;
            }
            if (scan(line, c"xr_hud_follow_mode = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrHudFollowMode = intValue;
                break 'line;
            }
            if (scan(line, c"xr_hud_stereo_depth = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrHudStereoDepth = intValue;
                break 'line;
            }
            if (scan(line, c"xr_hud_brightness = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrHudBrightness = value;
                break 'line;
            }
            if (scan(line, c"xr_hud_shadow = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrHudShadow = value;
                break 'line;
            }
            if (scan(line, c"xr_hud_glow = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrHudGlow = value;
                break 'line;
            }
            if (scan(line, c"xr_hud_panel = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrHudPanel = intValue;
                break 'line;
            }
            if (scan(line, c"xr_hud_follow_deg = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrHudFollowDeg = value;
                break 'line;
            }
            if (scan(line, c"xr_hud_fov = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrHudFov = value;
                break 'line;
            }
            if (scan(line, c"xr_hud_distance = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrHudDistance = value;
                break 'line;
            }
            if (scan(line, c"xr_interaction_panel = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrInteractionPanel = intValue;
                break 'line;
            }
            if (scan(line, c"xr_interaction_follow_deg = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrInteractionFollowDeg = value;
                break 'line;
            }
            if (scan(line, c"xr_loot_follow_deg = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrLootFollowDeg = value;
                break 'line;
            }
            if (scan(line, c"xr_interaction_distance = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrInteractionDistance = value;
                break 'line;
            }
            if (scan(line, c"xr_interaction_fov = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrInteractionFov = value;
                break 'line;
            }
            if ((scan(line, c"xr_pitch_sign=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_pitch_sign = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrPitchSign = (if (value < 0.0f32) { (-1.0f32) } else { 1.0f32 });
                break 'line;
            }
            if ((scan(line, c"xr_pitch_scale=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_pitch_scale = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrPitchScale = (if (value > 0.01f32) { value } else { 1.0f32 });
                break 'line;
            }
            if ((scan(line, c"xr_sync_sequential=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_sync_sequential = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrSyncSequential = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_3dof_movement=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_3dof_movement = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xr3DofMovement = intValue;
                break 'line;
            }
            if ((scan(line, c"first_launch=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"first_launch = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrFirstLaunch = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_motion_predict_ms=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_motion_predict_ms = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrMotionPredictMs = value;
                break 'line;
            }
            if ((scan(line, c"xr_stereo_scale=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_stereo_scale = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrStereoScale = value;
                break 'line;
            }
            if ((scan(line, c"xr_world_scale=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_world_scale = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrWorldScale = value;
                break 'line;
            }
            if ((scan(line, c"xr_ipd_scale=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_ipd_scale = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrIpdScale = value;
                break 'line;
            }
            if ((scan(line, c"xr_sharpness=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_sharpness = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrSharpness = value;
                break 'line;
            }
            if ((scan(line, c"xr_sharpmix=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_sharpmix = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrSharpmix = value;
                break 'line;
            }
            if ((scan(line, c"xr_hmd_smooth=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hmd_smooth = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHmdSmooth = value;
                break 'line;
            }
            if ((scan(line, c"xr_hand_per_frame=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hand_per_frame = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHandPerFrame = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_hand_rel_filtered=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hand_rel_filtered = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHandRelFiltered = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_hand_lerp_pistol=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hand_lerp_pistol = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrLerpPistol = value;
                break 'line;
            }
            if ((scan(line, c"xr_hand_lerp_rifle=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hand_lerp_rifle = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrLerpRifle = value;
                break 'line;
            }
            if ((scan(line, c"xr_hand_lerp_shotgun=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hand_lerp_shotgun = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrLerpShotgun = value;
                break 'line;
            }
            if ((scan(line, c"xr_hand_lerp_sniper=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hand_lerp_sniper = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrLerpSniper = value;
                break 'line;
            }
            if ((scan(line, c"xr_hand_lerp_melee=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hand_lerp_melee = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrLerpMelee = value;
                break 'line;
            }
            if ((scan(line, c"xr_hand_lerp_speed=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hand_lerp_speed = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHandLerp = value;
                break 'line;
            }
            if ((scan(line, c"xr_hand_lerp_framegen_scale=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hand_lerp_framegen_scale = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrLerpFramegen = (if (((value - 1.5f32)).abs() < 0.0001f32) { 1.10f32 } else { value });
                break 'line;
            }
            {
                let mut sx: f32 = 0.0f32;
                let mut sy: f32 = 0.0f32;
                let mut ss: f32 = 0.0f32;
                let mut slot: i32 = (-1i32);
                if (scan(line, c"xr_scanner_frame=%f,%f,%f", &[ptr(&mut sx), ptr(&mut sy), ptr(&mut ss)]) == 3i32) {
                    slot = 0i32;
                } else if (scan(line, c"xr_scanner_details=%f,%f,%f", &[ptr(&mut sx), ptr(&mut sy), ptr(&mut ss)]) == 3i32) {
                    slot = 1i32;
                } else if (scan(line, c"xr_scanner_hacks=%f,%f,%f", &[ptr(&mut sx), ptr(&mut sy), ptr(&mut ss)]) == 3i32) {
                    slot = 2i32;
                } else if (scan(line, c"xr_scanner_hint=%f,%f,%f", &[ptr(&mut sx), ptr(&mut sy), ptr(&mut ss)]) == 3i32) {
                    slot = 3i32;
                } else if (scan(line, c"xr_scanner_memory=%f,%f,%f", &[ptr(&mut sx), ptr(&mut sy), ptr(&mut ss)]) == 3i32) {
                    slot = 4i32;
                } else if (scan(line, c"xr_scanner_scripts=%f,%f,%f", &[ptr(&mut sx), ptr(&mut sy), ptr(&mut ss)]) == 3i32) {
                    slot = 5i32;
                } else if (scan(line, c"xr_scanner_desc=%f,%f,%f", &[ptr(&mut sx), ptr(&mut sy), ptr(&mut ss)]) == 3i32) {
                    slot = 6i32;
                }
                if (slot >= 0i32) {
                    v.xrScannerSlots[((slot * 3i32) + 0i32) as usize] = sx;
                    v.xrScannerSlots[((slot * 3i32) + 1i32) as usize] = sy;
                    v.xrScannerSlots[((slot * 3i32) + 2i32) as usize] = ss;
                    break 'line;
                }
            }
            if ((scan(line, c"xr_vrcam_composition=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vrcam_composition = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrCompGroup = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_auto_recenter=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_auto_recenter = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrAutoRecenter = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_vive_trackers=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vive_trackers = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.g_viveTrackersSetting = (if (intValue == 1i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_grip_from_fingers=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_grip_from_fingers = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrGripFromFingers = (if (intValue == 1i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_finger_spread_enabled=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_finger_spread_enabled = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrFingerSpreadOn = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_finger_spread=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_finger_spread = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrFingerSpread = (if (value).is_finite() { cpp_clamp(value, 0.0f32, 2.0f32) } else { 1.0f32 });
                break 'line;
            }
            if ((scan(line, c"xr_finger_bones=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_finger_bones = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrFingerBones = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_interaction_anchor=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_interaction_anchor = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrInteractionAnchor = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_hud_clearance_hands_scale=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hud_clearance_hands_scale = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHudClearanceHandsScale = (if (value).is_finite() { cpp_clamp(value, 1.0f32, 3.0f32) } else { 2.0f32 });
                break 'line;
            }
            if ((scan(line, c"xr_hud_clearance_hands=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hud_clearance_hands = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHudClearanceHands = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_hud_clearance_grow=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_hud_clearance_grow = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHudClearanceGrow = (if (value).is_finite() { cpp_clamp(value, 0.0f32, 1.0f32) } else { 0.5f32 });
                break 'line;
            }
            if ((scan(line, c"xr_hud_clearance=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hud_clearance = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHudClearance = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_hud_to_second_eye=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hud_to_second_eye = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHudTo2 = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_asym_projection=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_asym_projection = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrAsym = (if (intValue == 1i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            {
                let mut g: f32 = 0.0f32;
                if ((scan(line, c"xr_asym_inner_guard=%f", &[ptr(&mut g)]) == 1i32) || (scan(line, c"xr_asym_inner_guard = %f", &[ptr(&mut g)]) == 1i32)) {
                    v.xrAsymGuard = (if ((g < 0.0f32)) { 0.0f32 } else { ((if ((g > 1.0f32)) { 1.0f32 } else { g })) });
                    break 'line;
                }
            }
            if ((scan(line, c"xr_hud_in_braindance=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hud_in_braindance = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHudBd = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_fix_lod=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_fix_lod = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrFixLod = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_comp_lend_scoped=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_comp_lend_scoped = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrCompLendScoped = ((if (intValue < 0i32) { 0i32 } else { ((intValue & 0xFi32)) }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_expo_mirror=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_expo_mirror = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrExpoMirror = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_expo_field_mask=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_expo_field_mask = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrExpoFieldMask = ((if (intValue < 0i32) { 0i32 } else { ((intValue & 0x7Fi32)) }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_distant_reuse=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_distant_reuse = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrDistantReuse = (if ((intValue < 0i32)) { 0u32 } else { (intValue as u32) });
                break 'line;
            }
            if ((scan(line, c"xr_local_shadow_reuse=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_local_shadow_reuse = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrLocalShadowReuse = (if ((intValue < 0i32)) { 0u32 } else { (intValue as u32) });
                break 'line;
            }
            if ((scan(line, c"xr_gi_reuse=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_gi_reuse = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrGiReuse = (if ((intValue < 0i32)) { 0u32 } else { (intValue as u32) });
                break 'line;
            }
            if ((scan(line, c"xr_probe_reuse=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_probe_reuse = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrProbeReuse = (if ((intValue < 0i32)) { 0u32 } else { (intValue as u32) });
                break 'line;
            }
            if ((scan(line, c"xr_stable_copy=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_stable_copy = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrStableCopy = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_no_state_lies=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_no_state_lies = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrNoStateLies = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_indirect_census=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_indirect_census = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrIndirectCensus = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_expo_probe=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_expo_probe = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrExpoProbe = (if ((intValue < 0i32)) { 0i32 } else { ((if ((intValue > 2i32)) { 2i32 } else { intValue })) });
                break 'line;
            }
            if ((scan(line, c"xr_run_view_params=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_run_view_params = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrRunViewParams = ((if (intValue < 0i32) { 0i32 } else { ((if (intValue > 2i32) { 2i32 } else { intValue })) }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_vrcam_cam_fields=%x", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vrcam_cam_fields = %x", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrVrcamCamFields = ((if (intValue < 0i32) { 0i32 } else { ((intValue & 0xFi32)) }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_force_vrcam_cam=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_force_vrcam_cam = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrForceVrcamCam = (if (intValue != 0) { 1u32 } else { 0u32 });
                break 'line;
            }
            if ((scan(line, c"xr_cap_grant=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_cap_grant = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrCapGrant = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_light_content=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_light_content = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrLightContent = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_bd_quat_from_buffer=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_quat_from_buffer = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBdQuatFromBuffer = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_bd_ipd_in_locate=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_ipd_in_locate = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBdIpdInLocate = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_dev_cam_tol_m=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_dev_cam_tol_m = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrDevCamTolM = value;
                break 'line;
            }
            if ((scan(line, c"xr_dev_cam_any_name=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_dev_cam_any_name = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrDevCamAnyName = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_input_default_in_ui=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_input_default_in_ui = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrInputDefaultInUi = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_popup_mag_block_ms=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_popup_mag_block_ms = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrPopupMagBlockMs = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_lens_head_write=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_lens_head_write = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrLensHeadWrite = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_dev_cam_in_locate=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_dev_cam_in_locate = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrDevCamInLocate = intValue;
                break 'line;
            }
            {
                let mut visCsNode: u32 = (0i32 as u32);
                if ((scan(line, c"xr_vision_cs_node=%x", &[ptr(&mut visCsNode)]) == 1i32) || (scan(line, c"xr_vision_cs_node = %x", &[ptr(&mut visCsNode)]) == 1i32)) {
                    v.CyberpunkVR_VisionCsNode = visCsNode;
                    break 'line;
                }
            }
            if ((scan(line, c"xr_vision_cs_ord=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vision_cs_ord = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.CyberpunkVR_VisionCsOrd = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_vision_cs_dim=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vision_cs_dim = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.CyberpunkVR_VisionCsDim = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_vision_cs_swap=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vision_cs_swap = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.CyberpunkVR_VisionCsSwap = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_vision_to_second_eye=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vision_to_second_eye = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrVisionTo2 = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_weapon_plane_as_scene=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_weapon_plane_as_scene = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.savedPlaneAsScene = (intValue != 0) as i32;
                break 'line;
            }
            if ((scan(line, c"xr_highlight_any_plane=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_highlight_any_plane = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.CyberpunkVR_HighlightAnyPlane = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_plane_bucket_scene=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_plane_bucket_scene = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.CyberpunkVR_PlaneBucketScene = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_vrcam_pos_from_main=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vrcam_pos_from_main = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrVrcamPosFromMain = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_bd_quat_from_write_site=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_quat_from_write_site = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBdQuatWriteSite = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_stable_srgb_view=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_stable_srgb_view = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrStableSrgbView = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_final_grab=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_final_grab = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrFinalGrab = (if (intValue != 0) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_comp_lend_set=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_comp_lend_set = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrCompLendSet = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_bd_base_from_locate=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_base_from_locate = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBdBaseFromLocate = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_bd_editor_align=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_editor_align = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBdEditorAlign = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_bd_one_composition=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_one_composition = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBdOneComp = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_bd_push_base=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_push_base = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBdPushBase = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_bd_main_pos_from_scene=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_main_pos_from_scene = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBdMainPosScene = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_bd_push_transform=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_push_transform = %d", &[ptr(&mut intValue)]) == 1i32)) {
                if ((intValue >= 0i32) && (intValue <= 3i32)) {
                    v.xrBdPushTransform = intValue;
                }
                break 'line;
            }
            if ((scan(line, c"xr_bd_cam_dirty=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_bd_cam_dirty = %d", &[ptr(&mut intValue)]) == 1i32)) {
                if ((intValue >= 0i32) && (intValue <= 3i32)) {
                    v.xrBdCamDirty = intValue;
                }
                break 'line;
            }
            if ((scan(line, c"xr_env_mirror_mask=%x", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_env_mirror_mask = %x", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrEnvMirrorMask = (intValue as u32);
                break 'line;
            }
            if ((scan(line, c"xr_viewdata_fix_mask=%x", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_viewdata_fix_mask = %x", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrViewDataFixMask = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_render_mask_grant=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_render_mask_grant = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrRenderMaskGrant = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_stage_replay=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_stage_replay = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrStageReplay = ((intValue != 0i32) as i32);
                break 'line;
            }
            if ((scan(line, c"xr_vrcam_gbuffer_partitions=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vrcam_gbuffer_partitions = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrGbufferPartitions = ((intValue != 0i32) as i32);
                break 'line;
            }
            if ((scan(line, c"xr_scope_gbuffer_partitions=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_scope_gbuffer_partitions = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrScopeGbufferPartitions = ((intValue != 0i32) as i32);
                break 'line;
            }
            if (scan(line, c"xr_scope_rr_full_resolution = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrScopeRrFullResolution = (intValue != 0) as i32;
                break 'line;
            }
            if ((scan(line, c"xr_scope_graph_dag=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_scope_graph_dag = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrScopeGraphDag = ((intValue != 0i32) as i32);
                break 'line;
            }
            if ((scan(line, c"xr_submit_chain=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_submit_chain = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrSubmitChain = (if (intValue < 0i32) { 0i32 } else { (if (intValue > 2i32) { 2i32 } else { intValue }) });
                break 'line;
            }
            if ((scan(line, c"xr_defer_present=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_defer_present = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrDeferPresent = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_gt_skip_render_queue=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_gt_skip_render_queue = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrGtSkipRenderQueue = (if (intValue != 0i32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_submit_chain_rev=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_submit_chain_rev = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrSubmitChainRev = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_water_mirror=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_water_mirror = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrWaterMirror = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
            }
            if ((scan(line, c"xr_water_assign=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_water_assign = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrWaterAssign = ((if (intValue < 0i32) { 0i32 } else { ((if (intValue > 1i32) { 1i32 } else { intValue })) }) as u32);
            }
            if ((scan(line, c"xr_viewdata_diff=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_viewdata_diff = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrViewDataDiff = (if (intValue < 0i32) { 0i32 } else { ((if (intValue > 2i32) { 2i32 } else { intValue })) });
            }
            if ((scan(line, c"xr_scene_stick_yaw_dps=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_scene_stick_yaw_dps = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrSceneYawDps = (if (value < 0.0f32) { 0.0f32 } else { ((if (value > 360.0f32) { 360.0f32 } else { value })) });
                break 'line;
            }
            if ((scan(line, c"xr_scene_stick_yaw_tier=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_scene_stick_yaw_tier = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrSceneYawTier = (if (intValue < 0i32) { 0i32 } else { ((if (intValue > 4i32) { 4i32 } else { intValue })) });
                break 'line;
            }
            if ((scan(line, c"xr_lut_src_lend=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_lut_src_lend = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrLutSrcLend = ((if (intValue < 0i32) { 0i32 } else { ((if (intValue > 1i32) { 1i32 } else { intValue })) }) as u32);
            }
            if ((scan(line, c"xr_block_diff=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_block_diff = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBlockDiff = (if (intValue < 0i32) { 0i32 } else { ((if (intValue > 2i32) { 2i32 } else { intValue })) });
            }
            if ((scan(line, c"xr_block_diff_node=%x", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_block_diff_node = %x", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBlockDiffNode = (intValue as u32);
            }
            if ((scan(line, c"xr_block_diff_size=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_block_diff_size = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBlockDiffSize = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
            }
            if ((scan(line, c"xr_env_extra_mask=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_env_extra_mask = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrEnvExtraMask = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_grade_mirror_mask=%x", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_grade_mirror_mask = %x", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrGradeMirrorMask = ((if (intValue < 0i32) { 0i32 } else { intValue }) as u32);
                break 'line;
            }
            if ((scan(line, c"xr_two_hand_radius=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_two_hand_radius = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrTwoHandRadius = value;
                break 'line;
            }
            if ((scan(line, c"xr_two_hand_aim=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_two_hand_aim = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrTwoHandAim = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_two_hand_break=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_two_hand_break = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrTwoHandBreak = value;
                break 'line;
            }
            if ((scan(line, c"xr_two_hand_aim_release_ms=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_two_hand_aim_release_ms = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrTwoHandAimRelease = value;
                break 'line;
            }
            if ((scan(line, c"xr_carry_radius=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_carry_radius = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrCarryRadius = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_rifle_rise=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_rifle_rise = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrRifleRise = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_rifle_back=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_rifle_back = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrRifleBack = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_shotgun_rise=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_shotgun_rise = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrShotgunRise = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_shotgun_back=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_shotgun_back = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrShotgunBack = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_down_pistol=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_down_pistol = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrDownPistol = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_down_rifle=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_down_rifle = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrDownRifle = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_down_shotgun=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_down_shotgun = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrDownShotgun = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_back_cm=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_back_cm = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrBackCm = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_back_max_cm=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_back_max_cm = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrBackMaxCm = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_twohand_back=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_twohand_back = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrTwoHandBack = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_sniper_rise=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_sniper_rise = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrSniperRise = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_sniper_back=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_sniper_back = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrSniperBack = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_scale=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_scale = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrRecoilScale = value;
                break 'line;
            }
            if ((scan(line, c"xr_psvr2_haptic_backend=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_psvr2_haptic_backend = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrPsvr2HapticBackend = if value == 2.0f32 { 2 } else if value == 1.0f32 { 1 } else { 0 };
                break 'line;
            }
            if (scan(line, c"xr_openxr_haptic_gain = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrOpenXrHapticGain = value;
                break 'line;
            }
            if (scan(line, c"xr_vehicle_audio_channels = %d", &[ptr(&mut v.xrVehicleAudioChannels)]) == 1i32) {
                break 'line;
            }
            if (scan(line, c"xr_vehicle_audio_require_grip = %d", &[ptr(&mut v.xrVehicleAudioRequireGrip)]) == 1i32) {
                break 'line;
            }
            if (scan(line, c"xr_openxr_vehicle_haptic_gain = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrOpenXrVehicleHapticGain = value;
                break 'line;
            }
            if ((scan(line, c"xr_haptics=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_haptics = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHaptics = (if (value != 0.0f32) { 1i32 } else { 0i32 });
                break 'line;
            }
            if ((scan(line, c"xr_haptic_scale=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_haptic_scale = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrHapticScale = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_pistol_back=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_pistol_back = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrPistolBack = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_climb=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_climb = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrClimbFrac = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_climb_max_deg=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_climb_max_deg = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrClimbMax = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_climb_ms=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_climb_ms = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrClimbMs = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_return_ms=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_return_ms = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrReturnMs = value;
                break 'line;
            }
            if ((scan(line, c"xr_recoil_return_pow=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_recoil_return_pow = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrReturnPow = value;
                break 'line;
            }
            if ((scan(line, c"xr_render_pose_submit=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_render_pose_submit = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrRenderPoseSubmit = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_reuse_last_frame=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_reuse_last_frame = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrReuseLastFrame = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_pair_lock=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_pair_lock = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrPairLock = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_pose_lag=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_pose_lag = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrPoseLag = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_runtime=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_runtime = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrRuntime = clamp_runtime_mode(intValue);
                break 'line;
            }
            if ((scan(line, c"xr_depth_submit=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_depth_submit = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrDepthSubmit = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_movement_control=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_movement_control = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrMovementControl = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_disable_mouse_y=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_disable_mouse_y = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrDisableMouseY = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_xinput_hook=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_xinput_hook = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrXInputHook = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_snap_turn=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_snap_turn = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrSnapTurn = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_snap_turn_angle_deg=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_snap_turn_angle_deg = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrSnapTurnAngleDeg = value;
                break 'line;
            }
            if ((scan(line, c"xr_movement_source=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_movement_source = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrMovementSource = intValue;
                break 'line;
            }
            if (scan(line, c"xr_movement_speed_mode = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrMovementSpeedMode = intValue;
                break 'line;
            }
            if (scan(line, c"xr_sprint_mode = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrSprintMode = intValue;
                break 'line;
            }
            if (scan(line, c"xr_left_stick_deadzone = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrLeftStickDeadzone = value;
                break 'line;
            }
            if (scan(line, c"xr_right_stick_deadzone = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrRightStickDeadzone = value;
                break 'line;
            }
            if (scan(line, c"xr_max_input_threshold = %f", &[ptr(&mut value)]) == 1i32) {
                v.xrMaxInputThreshold = value;
                break 'line;
            }
            if ((scan(line, c"xr_cutscene_suspend_tier=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_cutscene_suspend_tier = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrCutsceneSuspendTier = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_vehicle_cutscene_suspend_tier=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vehicle_cutscene_suspend_tier = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrVehicleCutsceneSuspendTier = intValue;
                v.vehicleCutsceneTierConfigured = true;
                break 'line;
            }
            if (scan(line, c"xr_body_free_look_deg=%f", &[ptr(&mut value)]) == 1i32) {
                v.xrBodyFreeLookDeg = value;
            }
            if ((scan(line, c"xr_body_free_look_down_deg=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_body_free_look_down_deg = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrBodyFreeLookDownDeg = value;
            }
            if (scan(line, c"xr_body_free_look_swim_deg=%f", &[ptr(&mut value)]) == 1i32) {
                v.xrBodyFreeLookSwimDeg = value;
            }
            if (scan(line, c"xr_body_move_radius=%f", &[ptr(&mut value)]) == 1i32) {
                v.xrBodyMoveRadius = value;
            }
            if (scan(line, c"xr_scene_cam_comfort_deg=%f", &[ptr(&mut value)]) == 1i32) {
                v.xrSceneCamComfortDeg = value;
            }
            if (scan(line, c"xr_physical_crouch=%d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrPhysicalCrouch = intValue;
            }
            if (scan(line, c"xr_physical_crouch_depth=%f", &[ptr(&mut value)]) == 1i32) {
                v.xrPhysicalCrouchDepth = value;
            }
            if (scan(line, c"xr_ladder_grip_climb=%d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrLadderGripClimb = intValue;
            }
            if (scan(line, c"xr_left_handed=%d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrLeftHanded = intValue;
            }
            if (scan(line, c"xr_left_handed_bindings=%d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrLeftHandedBindings = intValue;
            }
            if (scan(line, c"xr_ladder_auto_finish=%d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrLadderAutoFinish = intValue;
            }
            if (scan(line, c"xr_ladder_finish_distance=%f", &[ptr(&mut value)]) == 1i32) {
                v.xrLadderFinishDistance = value;
            }
            if (scan(line, c"xr_breaststroke_swim=%d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrBreaststrokeSwim = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_roomscale_movement=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_roomscale_movement = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrRoomscaleMovement = intValue;
            }
            if (scan(line, c"xr_tracked_body_rotation = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrTrackedBodyRotation = intValue;
            }
            if (scan(line, c"xr_hybrid_body_rotation = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrHybridBodyRotation = intValue;
            }
            if (scan(line, c"xr_head_led_body_rotation = %d", &[ptr(&mut intValue)]) == 1i32) {
                v.xrHeadLedBodyRotation = intValue;
            }
            if ((scan(line, c"xr_physical_body_rotation=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_physical_body_rotation = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrPhysicalBodyRotation = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_xinput_install=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_xinput_install = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrXInputInstall = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_input_actions=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_input_actions = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrInputActions = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_mono_xqueue_wait=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_mono_xqueue_wait = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrMonoXQueueWait = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_snap_turn_pulse_ms=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_snap_turn_pulse_ms = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrSnapTurnPulseMs = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_mono_depth_capture=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_mono_depth_capture = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrMonoDepthCapture = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_snap_turn_yaw_index=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_snap_turn_yaw_index = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrSnapTurnYawIndex = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_immersive_holsters=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_immersive_holsters = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrImmersiveHolsters = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_body_hands_only=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_body_hands_only = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrBodyHandsOnly = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_reload_glow=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_reload_glow = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrReloadGlow = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_veh_head_offset_x=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_veh_head_offset_x = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrVehHeadOffsetX = value;
                break 'line;
            }
            if ((scan(line, c"xr_veh_head_offset_y=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_veh_head_offset_y = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrVehHeadOffsetY = value;
                break 'line;
            }
            if ((scan(line, c"xr_veh_head_offset_z=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_veh_head_offset_z = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrVehHeadOffsetZ = value;
                break 'line;
            }
            if ((scan(line, c"xr_bike_head_offset_x=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_bike_head_offset_x = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrBikeHeadOffsetX = value;
                break 'line;
            }
            if ((scan(line, c"xr_bike_head_offset_y=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_bike_head_offset_y = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrBikeHeadOffsetY = value;
                break 'line;
            }
            if ((scan(line, c"xr_bike_head_offset_z=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_bike_head_offset_z = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrBikeHeadOffsetZ = value;
                break 'line;
            }
            if ((scan(line, c"xr_vr_interface=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vr_interface = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrVrInterface = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_hand_physics=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hand_physics = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHandPhysics = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_wheel_grab=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_wheel_grab = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrWheelGrab = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_menu_gesture=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_menu_gesture = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrMenuGesture = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_vr_weapon_wheel=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vr_weapon_wheel = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrVrWeaponWheel = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_hand_collision=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hand_collision = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHandCollision = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_hand_collision_vehicle=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hand_collision_vehicle = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHandCollisionVehicle = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_hand_push=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_hand_push = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrHandPush = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_prop_grab=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_prop_grab = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrPropGrab = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_carry_picture=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_carry_picture = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrCarryPicture = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_carry_lead=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_carry_lead = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrCarryLead = value;
                break 'line;
            }
            if ((scan(line, c"xr_contact_margin=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_contact_margin = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrContactMargin = value;
                break 'line;
            }
            if ((scan(line, c"xr_finger_contact=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_finger_contact = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrFingerContact = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_wheel_anywhere=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_wheel_anywhere = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrWheelAnywhere = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_wheel_radius=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_wheel_radius = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrWheelRadius = value;
                break 'line;
            }
            if ((scan(line, c"xr_wheel_steer_max_deg=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_wheel_steer_max_deg = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrWheelSteerMaxDeg = value;
                break 'line;
            }
            if ((scan(line, c"xr_bike_bar_lock_deg=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_bike_bar_lock_deg = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrBikeBarLockDeg = value;
                break 'line;
            }
            if ((scan(line, c"xr_bike_body_lean=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_bike_body_lean = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrBikeBodyLean = value;
                break 'line;
            }
            if ((scan(line, c"xr_wheel_steer_dead_deg=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_wheel_steer_dead_deg = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrWheelSteerDeadDeg = (if (((value - 1.5f32)).abs() < 0.0001f32) { 0.0f32 } else { value });
                break 'line;
            }
            if ((scan(line, c"xr_wheel_prediction=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_wheel_prediction = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrWheelPrediction = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_wheel_prediction_ms=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_wheel_prediction_ms = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrWheelPredictionMs = value;
                break 'line;
            }
            // xr_wheel_horn2 (2026-11-24): the horn sounds through the car's own ToggleHorn now, on by default; the
            // old xr_wheel_horn (off in every saved ini, while the horn had no button) is no longer read
            if ((scan(line, c"xr_wheel_horn2=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_wheel_horn2 = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrWheelHorn = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_wheel_horn_radius=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_wheel_horn_radius = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrWheelHornRadius = value;
                break 'line;
            }
            if ((scan(line, c"xr_vehicle_gun_trigger=%d", &[ptr(&mut intValue)]) == 1i32) || (scan(line, c"xr_vehicle_gun_trigger = %d", &[ptr(&mut intValue)]) == 1i32)) {
                v.xrVehicleGunTrigger = intValue;
                break 'line;
            }
            if ((scan(line, c"xr_vehicle_throttle_trim=%f", &[ptr(&mut value)]) == 1i32) || (scan(line, c"xr_vehicle_throttle_trim = %f", &[ptr(&mut value)]) == 1i32)) {
                v.xrVehicleThrottleTrim = value;
                break 'line;
            }
        }
    }
}
