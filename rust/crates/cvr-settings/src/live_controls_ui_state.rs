//! The overlay's snapshot of the live controls (include/Overlay/LiveControlsUi.hpp `LiveControlsUiState`, 1420 bytes):
//! the overlay edits a copy and hands it back, the save writes it to vrport.ini. Mirrored member for member at the
//! offsets MSVC gives them (each checked below; the size checked in src/Core/LiveControlsPoll.cpp); the subsystems'
//! settings are the C++ half's structs, carried here as opaque words of their size.
#![allow(non_snake_case)]

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LiveControlsUiState {
    pub xrWeaponLaserDot: bool,
    pub xrHeadOffsetX: f32,
    pub xrHeadOffsetY: f32,
    pub xrHeadOffsetZ: f32,
    pub xrMovementControl: i32,
    pub xrDisableMouseY: i32,
    pub xrRecenter: i32,
    pub xrMonoSubmit: i32,
    pub xrForceFov: f32,
    pub xrMenuRect: i32,
    pub xrMenuFov: f32,
    pub xrMenuFollowDeg: f32,
    pub xrHudPanel: i32,
    pub xrHudFollowDeg: f32,
    pub xrInteractionPanel: i32,
    pub xrInteractionFollowDeg: f32,
    pub xrLootFollowDeg: f32,
    pub xrInteractionDistance: f32,
    pub xrInteractionFov: f32,
    pub xrHudFov: f32,
    pub xrHudDistance: f32,
    pub xrHudFollowMode: i32,
    pub xrHudStereoDepth: i32,
    pub xrHudClearance: i32,
    pub xrHudClearanceGrow: f32,
    pub xrHudClearanceHands: i32,
    pub xrHudClearanceHandsScale: f32,
    pub xrInteractionAnchor: i32,
    pub xrHudBrightness: f32,
    pub xrHudShadow: f32,
    pub xrHudGlow: f32,
    pub xrFingerBones: i32,
    pub xrGripFromFingers: i32,
    pub xrFingerSpreadOn: i32,
    pub xrFingerSpread: f32,
    /// cvr::hud::LayoutSettings (41 elements of 20 bytes: 820 bytes).
    pub hudElements: [u32; 205],
    /// cvr::framegen::Settings (52 bytes).
    pub framegen: [u32; 13],
    /// cvr::foveated::Settings (28 bytes).
    pub foveated: [u32; 7],
    /// cvr::vrs::Settings (20 bytes).
    pub vrs: [u32; 5],
    /// cvr::hidden_area::Settings (8 bytes).
    pub hiddenArea: [u32; 2],
    pub monoCascades: i32,
    pub monoMode: i32,
    pub afw: i32,
    pub osr: i32,
    pub scopeStabilize: i32,
    pub nvidiaReflex: i32,
    pub fpsCap: i32,
    pub lobby: i32,
    pub lobbyAmbient: i32,
    /// cvr::vrui::Settings (48 bytes).
    pub overlay: [u32; 12],
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
    pub xrWeaponPitch: f32,
    pub xrWeaponYaw: f32,
    pub xrWeaponRoll: f32,
    pub xrWeaponOffsetX: f32,
    pub xrWeaponOffsetY: f32,
    pub xrWeaponOffsetZ: f32,
    pub xrXInputHook: i32,
    pub xrSnapTurn: i32,
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
    pub xrMonoDepthCapture: i32,
    pub xrSnapTurnPulseMs: i32,
    pub xrImmersiveHolsters: i32,
    pub xrBodyHandsOnly: i32,
    pub xrReloadGlow: i32,
    pub xrPhysicalBodyRotation: i32,
    pub xrTrackedBodyRotation: i32,
    pub xrHybridBodyRotation: i32,
    pub xrHeadLedBodyRotation: i32,
    pub xrRoomscaleMovement: i32,
    pub xrPhysicalCrouch: i32,
    pub xrPhysicalCrouchDepth: f32,
    pub xrAsymProjection: i32,
    pub xrPsvr2HapticBackend: i32,
    pub xrOpenXrHapticGain: f32,
    pub xrOpenXrVehicleHapticGain: f32,
    pub xrVehicleAudioChannels: i32,
    pub xrVehicleAudioRequireGrip: bool,
    pub xrHaptics: i32,
    pub xrHapticScale: f32,
    pub xrCutsceneSuspendTier: i32,
    pub xrVehHeadOffsetX: f32,
    pub xrVehHeadOffsetY: f32,
    pub xrVehHeadOffsetZ: f32,
    pub xrBikeHeadOffsetX: f32,
    pub xrBikeHeadOffsetY: f32,
    pub xrBikeHeadOffsetZ: f32,
    pub xrWheelGrab: i32,
    pub xrWheelRadius: f32,
    pub xrMenuGesture: i32,
    pub xrVrInterface: i32,
    pub xrHandPhysics: i32,
    pub xrVrWeaponWheel: i32,
    pub xrHandCollision: i32,
    pub xrHandCollisionVehicle: i32,
    pub xrHandPush: i32,
    pub xrFingerContact: i32,
    pub xrPropGrab: i32,
    pub xrDebugView: i32,
    pub xrCarryPicture: i32,
    pub xrCarryLead: f32,
    pub xrContactMargin: f32,
    pub xrWheelAnywhere: i32,
    pub xrWheelSteerMaxDeg: f32,
    pub xrBikeBarLockDeg: f32,
    pub xrBikeBodyLean: f32,
    pub xrWheelSteerDeadDeg: f32,
    pub xrWheelPrediction: i32,
    pub xrWheelPredictionMs: f32,
    pub xrWheelHorn: i32,
    pub xrWheelHornRadius: f32,
    pub xrVehicleGunTrigger: i32,
    pub xrVehicleThrottleTrim: f32,
    pub xrBodyFreeLookDeg: f32,
    pub xrBodyMoveRadius: f32,
    pub xrSceneCamComfortDeg: f32,
    pub xrBodyFreeLookDownDeg: f32,
    pub xrBodyFreeLookSwimDeg: f32,
    pub xrVehicleCutsceneSuspendTier: i32,
    pub xrBreaststrokeSwim: i32,
    pub xrLadderGripClimb: i32,
    pub xrLeftHanded: i32,
    pub xrLeftHandedBindings: i32,
    pub xrLadderAutoFinish: i32,
    pub xrLadderFinishDistance: f32,
}
const _: () = {
    assert!(core::mem::size_of::<LiveControlsUiState>() == 1536);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWeaponLaserDot) == 0);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHeadOffsetX) == 4);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHeadOffsetY) == 8);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHeadOffsetZ) == 12);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMovementControl) == 16);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrDisableMouseY) == 20);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrRecenter) == 24);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMonoSubmit) == 28);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrForceFov) == 32);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMenuRect) == 36);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMenuFov) == 40);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMenuFollowDeg) == 44);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudPanel) == 48);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudFollowDeg) == 52);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrInteractionPanel) == 56);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrInteractionFollowDeg) == 60);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrLootFollowDeg) == 64);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrInteractionDistance) == 68);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrInteractionFov) == 72);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudFov) == 76);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudDistance) == 80);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudFollowMode) == 84);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudStereoDepth) == 88);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudClearance) == 92);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudClearanceGrow) == 96);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudClearanceHands) == 100);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudClearanceHandsScale) == 104);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrInteractionAnchor) == 108);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudBrightness) == 112);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudShadow) == 116);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHudGlow) == 120);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrFingerBones) == 124);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrGripFromFingers) == 128);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrFingerSpreadOn) == 132);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrFingerSpread) == 136);
    assert!(core::mem::offset_of!(LiveControlsUiState, hudElements) == 140);
    assert!(core::mem::offset_of!(LiveControlsUiState, framegen) == 960);
    assert!(core::mem::offset_of!(LiveControlsUiState, foveated) == 1012);
    assert!(core::mem::offset_of!(LiveControlsUiState, vrs) == 1040);
    assert!(core::mem::offset_of!(LiveControlsUiState, hiddenArea) == 1060);
    assert!(core::mem::offset_of!(LiveControlsUiState, monoCascades) == 1068);
    assert!(core::mem::offset_of!(LiveControlsUiState, monoMode) == 1072);
    assert!(core::mem::offset_of!(LiveControlsUiState, afw) == 1076);
    assert!(core::mem::offset_of!(LiveControlsUiState, osr) == 1080);
    assert!(core::mem::offset_of!(LiveControlsUiState, scopeStabilize) == 1084);
    assert!(core::mem::offset_of!(LiveControlsUiState, nvidiaReflex) == 1088);
    assert!(core::mem::offset_of!(LiveControlsUiState, fpsCap) == 1092);
    assert!(core::mem::offset_of!(LiveControlsUiState, lobby) == 1096);
    assert!(core::mem::offset_of!(LiveControlsUiState, lobbyAmbient) == 1100);
    assert!(core::mem::offset_of!(LiveControlsUiState, overlay) == 1104);
    assert!(core::mem::offset_of!(LiveControlsUiState, xr3DofMovement) == 1152);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrFirstLaunch) == 1156);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMotionPredictMs) == 1160);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrStereoScale) == 1164);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWorldScale) == 1168);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrIpdScale) == 1172);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrSharpness) == 1176);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrSharpmix) == 1180);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrReuseLastFrame) == 1184);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrPairLock) == 1188);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrRenderPoseSubmit) == 1192);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrPoseLag) == 1196);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrRuntime) == 1200);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWeaponPitch) == 1204);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWeaponYaw) == 1208);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWeaponRoll) == 1212);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWeaponOffsetX) == 1216);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWeaponOffsetY) == 1220);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWeaponOffsetZ) == 1224);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrXInputHook) == 1228);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrSnapTurn) == 1232);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrSnapTurnAngleDeg) == 1236);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMovementSource) == 1240);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMovementSpeedMode) == 1244);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrSprintMode) == 1248);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrLeftStickDeadzone) == 1252);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrRightStickDeadzone) == 1256);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMaxInputThreshold) == 1260);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrXInputInstall) == 1264);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrInputActions) == 1268);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMonoXQueueWait) == 1272);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMonoDepthCapture) == 1276);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrSnapTurnPulseMs) == 1280);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrImmersiveHolsters) == 1284);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBodyHandsOnly) == 1288);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrPhysicalBodyRotation) == 1296);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrTrackedBodyRotation) == 1300);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHybridBodyRotation) == 1304);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrRoomscaleMovement) == 1312);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrPhysicalCrouch) == 1316);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrPhysicalCrouchDepth) == 1320);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrAsymProjection) == 1324);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHaptics) == 1348);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHapticScale) == 1352);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrCutsceneSuspendTier) == 1356);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVehHeadOffsetX) == 1360);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVehHeadOffsetY) == 1364);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVehHeadOffsetZ) == 1368);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelGrab) == 1384);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelRadius) == 1388);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrMenuGesture) == 1392);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVrWeaponWheel) == 1404);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelSteerMaxDeg) == 1448);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelSteerDeadDeg) == 1460);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelPrediction) == 1464);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelPredictionMs) == 1468);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelHorn) == 1472);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelHornRadius) == 1476);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVehicleGunTrigger) == 1480);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVehicleThrottleTrim) == 1484);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBodyFreeLookDeg) == 1488);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBodyMoveRadius) == 1492);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrSceneCamComfortDeg) == 1496);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBodyFreeLookDownDeg) == 1500);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBodyFreeLookSwimDeg) == 1504);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVehicleCutsceneSuspendTier) == 1508);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBreaststrokeSwim) == 1512);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrLadderGripClimb) == 1516);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrLeftHanded) == 1520);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrLeftHandedBindings) == 1524);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrLadderAutoFinish) == 1528);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrLadderFinishDistance) == 1532);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrReloadGlow) == 1292);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHeadLedBodyRotation) == 1308);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrPsvr2HapticBackend) == 1328);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrOpenXrHapticGain) == 1332);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrOpenXrVehicleHapticGain) == 1336);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVehicleAudioChannels) == 1340);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVehicleAudioRequireGrip) == 1344);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBikeHeadOffsetX) == 1372);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBikeHeadOffsetY) == 1376);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBikeHeadOffsetZ) == 1380);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrVrInterface) == 1396);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHandPhysics) == 1400);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHandCollision) == 1408);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHandCollisionVehicle) == 1412);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrHandPush) == 1416);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrFingerContact) == 1420);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrPropGrab) == 1424);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrDebugView) == 1428);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrCarryPicture) == 1432);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrCarryLead) == 1436);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrContactMargin) == 1440);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrWheelAnywhere) == 1444);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBikeBarLockDeg) == 1452);
    assert!(core::mem::offset_of!(LiveControlsUiState, xrBikeBodyLean) == 1456);
};
