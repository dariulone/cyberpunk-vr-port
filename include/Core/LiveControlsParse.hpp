#pragma once
// PollLiveControls' ini parse (src/Core/LiveControlsPoll.cpp), shared by its C++ loop (src/Core/LiveControlsParse.cpp) and
// its Rust port (rust/crates/cvr-settings/src/live_controls.rs). Generated with the split; the field order IS the
// layout the Rust struct repeats.
#include "Framegen/Framegen.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Render/FixedVrs.hpp"
#include "Render/FoveatedDlss.hpp"
#include "Render/HiddenArea.hpp"
#include "Runtimes/HudLayout.hpp"
#include <cstdint>
#include <cstdio>

// The loop's variables, seeded from the live state, then overwritten by the lines the ini holds; the last eight
// stand in for the globals the loop writes directly, written back after the parse.
struct LiveControlsValues {
    bool xrWeaponLaserDot;
    float xrHeadOffsetX;
    float xrHeadOffsetY;
    float xrHeadOffsetZ;
    int xrRecenter;
    int xrMonoSubmit;
    int xrThreadedSubmit;
    int xrWindowWidth;
    int xrWindowHeight;
    float xrForceFov;
    int xrMenuRect;
    float xrMenuFov;
    float xrMenuFollowDeg;
    int xrHudPanel;
    float xrHudFollowDeg;
    float xrHudFov;
    float xrHudDistance;
    int xrInteractionPanel;
    float xrInteractionFollowDeg;
    float xrInteractionDistance;
    float xrInteractionFov;
    float xrLootFollowDeg;
    int xrHudFollowMode;
    int xrHudStereoDepth;
    float xrHudBrightness;
    float xrHudShadow;
    float xrHudGlow;
    uint32_t monoCascades;
    int monoMode;
    int afw;
    int osr;
    int scopeStabilize;
    int nvidiaReflex;
    int fpsCap;
    int lobby;
    int lobbyAmbient;
    float xrPitchSign;
    float xrPitchScale;
    int xrSyncSequential;
    int xr3DofMovement;
    int xrFirstLaunch;
    float xrMotionPredictMs;
    float xrStereoScale;
    float xrWorldScale;
    float xrIpdScale;
    float xrSharpness;
    float xrSharpmix;
    int xrReuseLastFrame;
    int xrPairLock;
    int xrRenderPoseSubmit;
    int xrPoseLag;
    int xrRuntime;
    int xrDepthSubmit;
    int xrMovementControl;
    int xrDisableMouseY;
    int xrXInputHook;
    int xrSnapTurn;
    float xrHmdSmooth;
    float xrHandLerp;
    float xrLerpPistol;
    float xrLerpRifle;
    float xrLerpShotgun;
    float xrLerpSniper;
    float xrLerpMelee;
    float xrLerpFramegen;
    uint32_t xrGradeMirrorMask;
    uint32_t xrEnvExtraMask;
    uint32_t xrWaterMirror;
    uint32_t xrWaterAssign;
    int32_t xrViewDataDiff;
    uint32_t xrLutSrcLend;
    float xrSceneYawDps;
    int xrSceneYawTier;
    int32_t xrBlockDiff;
    uint32_t xrBlockDiffNode;
    uint32_t xrBlockDiffSize;
    uint32_t xrRenderMaskGrant;
    int xrStageReplay;
    int xrGbufferPartitions;
    int xrScopeGbufferPartitions;
    int xrScopeGraphDag;
    int xrSubmitChain;
    int xrDeferPresent;
    int xrGtSkipRenderQueue;
    int xrSubmitChainRev;
    uint32_t xrViewDataFixMask;
    uint32_t xrEnvMirrorMask;
    int32_t xrBdCamDirty;
    int32_t xrBdPushTransform;
    int32_t xrBdMainPosScene;
    int32_t xrBdPushBase;
    int32_t xrBdOneComp;
    int32_t xrBdEditorAlign;
    int32_t xrBdBaseFromLocate;
    uint32_t xrCompLendSet;
    int32_t xrFinalGrab;
    int32_t xrStableSrgbView;
    int32_t xrBdQuatWriteSite;
    int32_t xrVrcamPosFromMain;
    int32_t xrDevCamInLocate;
    int32_t xrLensHeadWrite;
    int32_t xrInputDefaultInUi;
    int32_t xrPopupMagBlockMs;
    int32_t xrDevCamAnyName;
    float xrDevCamTolM;
    int32_t xrBdIpdInLocate;
    int32_t xrBdQuatFromBuffer;
    uint32_t xrCapGrant;
    uint32_t xrForceVrcamCam;
    uint32_t xrVrcamCamFields;
    uint32_t xrRunViewParams;
    int32_t xrExpoProbe;
    int32_t xrIndirectCensus;
    int32_t xrNoStateLies;
    int32_t xrStableCopy;
    uint32_t xrDistantReuse;
    uint32_t xrLocalShadowReuse;
    uint32_t xrGiReuse;
    uint32_t xrProbeReuse;
    int32_t xrExpoMirror;
    uint32_t xrExpoFieldMask;
    uint32_t xrCompLendScoped;
    int xrLightContent;
    int32_t xrFixLod;
    int xrHudTo2;
    int xrInteractionAnchor;
    int xrHudClearance;
    float xrHudClearanceGrow;
    int xrHudClearanceHands;
    float xrHudClearanceHandsScale;
    int xrAutoRecenter;
    int xrFingerBones;
    int xrGripFromFingers;
    float xrFingerSpread;
    int xrFingerSpreadOn;
    int32_t xrAsym;
    float xrAsymGuard;
    int xrVisionTo2;
    int xrHudBd;
    uint32_t xrCompGroup;
    float xrTwoHandRadius;
    int xrTwoHandAim;
    float xrTwoHandBreak;
    float xrTwoHandAimRelease;
    float xrCarryRadius;
    float xrRifleRise;
    float xrRifleBack;
    float xrShotgunRise;
    float xrShotgunBack;
    float xrDownPistol;
    float xrDownRifle;
    float xrDownShotgun;
    float xrBackCm;
    float xrBackMaxCm;
    float xrTwoHandBack;
    float xrSniperRise;
    float xrSniperBack;
    float xrRecoilScale;
    int xrHaptics;
    float xrHapticScale;
    float xrPistolBack;
    float xrClimbFrac;
    float xrClimbMax;
    float xrClimbMs;
    float xrReturnMs;
    float xrReturnPow;
    float xrScannerSlots[21];
    int xrHandRelFiltered;
    int xrHandPerFrame;
    float xrSnapTurnAngleDeg;
    int xrMovementSource;
    int xrMovementSpeedMode;
    int xrSprintMode;
    float xrLeftStickDeadzone;
    float xrRightStickDeadzone;
    float xrMaxInputThreshold;
    int xrXInputInstall;
    int xrInputActions;
    int xrMonoXQueueWait;
    int xrSnapTurnPulseMs;
    int xrMonoDepthCapture;
    int xrSnapTurnYawIndex;
    int xrImmersiveHolsters;
    int xrBodyHandsOnly;
    int xrPhysicalBodyRotation;
    int xrTrackedBodyRotation;
    int xrHybridBodyRotation;
    int xrRoomscaleMovement;
    int xrPhysicalCrouch;
    float xrPhysicalCrouchDepth;
    int xrBreaststrokeSwim;
    int xrLadderGripClimb;
    int xrLeftHanded;
    int xrLeftHandedBindings;
    int xrLadderAutoFinish;
    float xrLadderFinishDistance;
    float xrBodyFreeLookDeg;
    float xrBodyFreeLookDownDeg;
    float xrBodyFreeLookSwimDeg;
    float xrBodyMoveRadius;
    float xrSceneCamComfortDeg;
    int xrCutsceneSuspendTier;
    int xrVehicleCutsceneSuspendTier;
    bool vehicleCutsceneTierConfigured;
    float xrVehHeadOffsetX;
    float xrVehHeadOffsetY;
    float xrVehHeadOffsetZ;
    int xrWheelGrab;
    int xrMenuGesture;
    int xrVrWeaponWheel;
    float xrWheelRadius;
    float xrWheelSteerMaxDeg;
    float xrWheelSteerDeadDeg;
    int xrWheelPrediction;
    float xrWheelPredictionMs;
    int xrWheelHorn;
    float xrWheelHornRadius;
    int xrVehicleGunTrigger;
    float xrVehicleThrottleTrim;
    int g_viveTrackersSetting;
    uint32_t CyberpunkVR_VisionCsNode;
    int32_t CyberpunkVR_VisionCsOrd;
    int32_t CyberpunkVR_VisionCsDim;
    int32_t CyberpunkVR_VisionCsSwap;
    int32_t CyberpunkVR_WeaponPlaneAsScene;
    int32_t CyberpunkVR_HighlightAnyPlane;
    int32_t CyberpunkVR_PlaneBucketScene;
    // main's keys (2026-11): the scope RR, the haptic routing and gains, the reload highlight, the head-led body
    // turn, the bike, the VR interface and hand physics; savedPlaneAsScene is -1 until the ini names it.
    int xrScopeRrFullResolution;
    int xrPsvr2HapticBackend;
    float xrOpenXrHapticGain;
    float xrOpenXrVehicleHapticGain;
    int xrVehicleAudioChannels;
    int xrVehicleAudioRequireGrip;
    int xrReloadGlow;
    int xrHeadLedBodyRotation;
    float xrBikeHeadOffsetX;
    float xrBikeHeadOffsetY;
    float xrBikeHeadOffsetZ;
    int xrVrInterface;
    int xrHandPhysics;
    int xrHandCollision;
    int xrHandCollisionVehicle;
    int xrHandPush;
    int xrFingerContact;
    int xrPropGrab;
    int xrCarryPicture;
    float xrCarryLead;
    float xrContactMargin;
    int xrWheelAnywhere;
    float xrBikeBarLockDeg;
    float xrBikeBodyLean;
    int savedPlaneAsScene;
};
// The subsystems' own settings, parsed by their own ParseSetting from the same lines.
struct LiveControlsSubsystems {
    cvr::hud::LayoutSettings hudElements{};
    cvr::framegen::Settings framegen{};
    cvr::foveated::Settings foveated{};
    cvr::vrs::Settings vrs{};
    cvr::hidden_area::Settings hiddenArea{};
    cvr::vrui::Settings overlay{};
};
// The loop's names for the fields (the locals, and the aliased globals).
#define LIVE_CONTROLS_BIND_LOCALS(v) \
    auto& xrWeaponLaserDot = (v).xrWeaponLaserDot; \
    auto& xrHeadOffsetX = (v).xrHeadOffsetX; \
    auto& xrHeadOffsetY = (v).xrHeadOffsetY; \
    auto& xrHeadOffsetZ = (v).xrHeadOffsetZ; \
    auto& xrRecenter = (v).xrRecenter; \
    auto& xrMonoSubmit = (v).xrMonoSubmit; \
    auto& xrThreadedSubmit = (v).xrThreadedSubmit; \
    auto& xrWindowWidth = (v).xrWindowWidth; \
    auto& xrWindowHeight = (v).xrWindowHeight; \
    auto& xrForceFov = (v).xrForceFov; \
    auto& xrMenuRect = (v).xrMenuRect; \
    auto& xrMenuFov = (v).xrMenuFov; \
    auto& xrMenuFollowDeg = (v).xrMenuFollowDeg; \
    auto& xrHudPanel = (v).xrHudPanel; \
    auto& xrHudFollowDeg = (v).xrHudFollowDeg; \
    auto& xrHudFov = (v).xrHudFov; \
    auto& xrHudDistance = (v).xrHudDistance; \
    auto& xrInteractionPanel = (v).xrInteractionPanel; \
    auto& xrInteractionFollowDeg = (v).xrInteractionFollowDeg; \
    auto& xrInteractionDistance = (v).xrInteractionDistance; \
    auto& xrInteractionFov = (v).xrInteractionFov; \
    auto& xrLootFollowDeg = (v).xrLootFollowDeg; \
    auto& xrHudFollowMode = (v).xrHudFollowMode; \
    auto& xrHudStereoDepth = (v).xrHudStereoDepth; \
    auto& xrHudBrightness = (v).xrHudBrightness; \
    auto& xrHudShadow = (v).xrHudShadow; \
    auto& xrHudGlow = (v).xrHudGlow; \
    auto& monoCascades = (v).monoCascades; \
    auto& monoMode = (v).monoMode; \
    auto& afw = (v).afw; \
    auto& osr = (v).osr; \
    auto& scopeStabilize = (v).scopeStabilize; \
    auto& nvidiaReflex = (v).nvidiaReflex; \
    auto& fpsCap = (v).fpsCap; \
    auto& lobby = (v).lobby; \
    auto& lobbyAmbient = (v).lobbyAmbient; \
    auto& xrPitchSign = (v).xrPitchSign; \
    auto& xrPitchScale = (v).xrPitchScale; \
    auto& xrSyncSequential = (v).xrSyncSequential; \
    auto& xr3DofMovement = (v).xr3DofMovement; \
    auto& xrFirstLaunch = (v).xrFirstLaunch; \
    auto& xrMotionPredictMs = (v).xrMotionPredictMs; \
    auto& xrStereoScale = (v).xrStereoScale; \
    auto& xrWorldScale = (v).xrWorldScale; \
    auto& xrIpdScale = (v).xrIpdScale; \
    auto& xrSharpness = (v).xrSharpness; \
    auto& xrSharpmix = (v).xrSharpmix; \
    auto& xrReuseLastFrame = (v).xrReuseLastFrame; \
    auto& xrPairLock = (v).xrPairLock; \
    auto& xrRenderPoseSubmit = (v).xrRenderPoseSubmit; \
    auto& xrPoseLag = (v).xrPoseLag; \
    auto& xrRuntime = (v).xrRuntime; \
    auto& xrDepthSubmit = (v).xrDepthSubmit; \
    auto& xrMovementControl = (v).xrMovementControl; \
    auto& xrDisableMouseY = (v).xrDisableMouseY; \
    auto& xrXInputHook = (v).xrXInputHook; \
    auto& xrSnapTurn = (v).xrSnapTurn; \
    auto& xrHmdSmooth = (v).xrHmdSmooth; \
    auto& xrHandLerp = (v).xrHandLerp; \
    auto& xrLerpPistol = (v).xrLerpPistol; \
    auto& xrLerpRifle = (v).xrLerpRifle; \
    auto& xrLerpShotgun = (v).xrLerpShotgun; \
    auto& xrLerpSniper = (v).xrLerpSniper; \
    auto& xrLerpMelee = (v).xrLerpMelee; \
    auto& xrLerpFramegen = (v).xrLerpFramegen; \
    auto& xrGradeMirrorMask = (v).xrGradeMirrorMask; \
    auto& xrEnvExtraMask = (v).xrEnvExtraMask; \
    auto& xrWaterMirror = (v).xrWaterMirror; \
    auto& xrWaterAssign = (v).xrWaterAssign; \
    auto& xrViewDataDiff = (v).xrViewDataDiff; \
    auto& xrLutSrcLend = (v).xrLutSrcLend; \
    auto& xrSceneYawDps = (v).xrSceneYawDps; \
    auto& xrSceneYawTier = (v).xrSceneYawTier; \
    auto& xrBlockDiff = (v).xrBlockDiff; \
    auto& xrBlockDiffNode = (v).xrBlockDiffNode; \
    auto& xrBlockDiffSize = (v).xrBlockDiffSize; \
    auto& xrRenderMaskGrant = (v).xrRenderMaskGrant; \
    auto& xrStageReplay = (v).xrStageReplay; \
    auto& xrGbufferPartitions = (v).xrGbufferPartitions; \
    auto& xrScopeGbufferPartitions = (v).xrScopeGbufferPartitions; \
    auto& xrScopeGraphDag = (v).xrScopeGraphDag; \
    auto& xrSubmitChain = (v).xrSubmitChain; \
    auto& xrDeferPresent = (v).xrDeferPresent; \
    auto& xrGtSkipRenderQueue = (v).xrGtSkipRenderQueue; \
    auto& xrSubmitChainRev = (v).xrSubmitChainRev; \
    auto& xrViewDataFixMask = (v).xrViewDataFixMask; \
    auto& xrEnvMirrorMask = (v).xrEnvMirrorMask; \
    auto& xrBdCamDirty = (v).xrBdCamDirty; \
    auto& xrBdPushTransform = (v).xrBdPushTransform; \
    auto& xrBdMainPosScene = (v).xrBdMainPosScene; \
    auto& xrBdPushBase = (v).xrBdPushBase; \
    auto& xrBdOneComp = (v).xrBdOneComp; \
    auto& xrBdEditorAlign = (v).xrBdEditorAlign; \
    auto& xrBdBaseFromLocate = (v).xrBdBaseFromLocate; \
    auto& xrCompLendSet = (v).xrCompLendSet; \
    auto& xrFinalGrab = (v).xrFinalGrab; \
    auto& xrStableSrgbView = (v).xrStableSrgbView; \
    auto& xrBdQuatWriteSite = (v).xrBdQuatWriteSite; \
    auto& xrVrcamPosFromMain = (v).xrVrcamPosFromMain; \
    auto& xrDevCamInLocate = (v).xrDevCamInLocate; \
    auto& xrLensHeadWrite = (v).xrLensHeadWrite; \
    auto& xrInputDefaultInUi = (v).xrInputDefaultInUi; \
    auto& xrPopupMagBlockMs = (v).xrPopupMagBlockMs; \
    auto& xrDevCamAnyName = (v).xrDevCamAnyName; \
    auto& xrDevCamTolM = (v).xrDevCamTolM; \
    auto& xrBdIpdInLocate = (v).xrBdIpdInLocate; \
    auto& xrBdQuatFromBuffer = (v).xrBdQuatFromBuffer; \
    auto& xrCapGrant = (v).xrCapGrant; \
    auto& xrForceVrcamCam = (v).xrForceVrcamCam; \
    auto& xrVrcamCamFields = (v).xrVrcamCamFields; \
    auto& xrRunViewParams = (v).xrRunViewParams; \
    auto& xrExpoProbe = (v).xrExpoProbe; \
    auto& xrIndirectCensus = (v).xrIndirectCensus; \
    auto& xrNoStateLies = (v).xrNoStateLies; \
    auto& xrStableCopy = (v).xrStableCopy; \
    auto& xrDistantReuse = (v).xrDistantReuse; \
    auto& xrLocalShadowReuse = (v).xrLocalShadowReuse; \
    auto& xrGiReuse = (v).xrGiReuse; \
    auto& xrProbeReuse = (v).xrProbeReuse; \
    auto& xrExpoMirror = (v).xrExpoMirror; \
    auto& xrExpoFieldMask = (v).xrExpoFieldMask; \
    auto& xrCompLendScoped = (v).xrCompLendScoped; \
    auto& xrLightContent = (v).xrLightContent; \
    auto& xrFixLod = (v).xrFixLod; \
    auto& xrHudTo2 = (v).xrHudTo2; \
    auto& xrInteractionAnchor = (v).xrInteractionAnchor; \
    auto& xrHudClearance = (v).xrHudClearance; \
    auto& xrHudClearanceGrow = (v).xrHudClearanceGrow; \
    auto& xrHudClearanceHands = (v).xrHudClearanceHands; \
    auto& xrHudClearanceHandsScale = (v).xrHudClearanceHandsScale; \
    auto& xrAutoRecenter = (v).xrAutoRecenter; \
    auto& xrFingerBones = (v).xrFingerBones; \
    auto& xrGripFromFingers = (v).xrGripFromFingers; \
    auto& xrFingerSpread = (v).xrFingerSpread; \
    auto& xrFingerSpreadOn = (v).xrFingerSpreadOn; \
    auto& xrAsym = (v).xrAsym; \
    auto& xrAsymGuard = (v).xrAsymGuard; \
    auto& xrVisionTo2 = (v).xrVisionTo2; \
    auto& xrHudBd = (v).xrHudBd; \
    auto& xrCompGroup = (v).xrCompGroup; \
    auto& xrTwoHandRadius = (v).xrTwoHandRadius; \
    auto& xrTwoHandAim = (v).xrTwoHandAim; \
    auto& xrTwoHandBreak = (v).xrTwoHandBreak; \
    auto& xrTwoHandAimRelease = (v).xrTwoHandAimRelease; \
    auto& xrCarryRadius = (v).xrCarryRadius; \
    auto& xrRifleRise = (v).xrRifleRise; \
    auto& xrRifleBack = (v).xrRifleBack; \
    auto& xrShotgunRise = (v).xrShotgunRise; \
    auto& xrShotgunBack = (v).xrShotgunBack; \
    auto& xrDownPistol = (v).xrDownPistol; \
    auto& xrDownRifle = (v).xrDownRifle; \
    auto& xrDownShotgun = (v).xrDownShotgun; \
    auto& xrBackCm = (v).xrBackCm; \
    auto& xrBackMaxCm = (v).xrBackMaxCm; \
    auto& xrTwoHandBack = (v).xrTwoHandBack; \
    auto& xrSniperRise = (v).xrSniperRise; \
    auto& xrSniperBack = (v).xrSniperBack; \
    auto& xrRecoilScale = (v).xrRecoilScale; \
    auto& xrHaptics = (v).xrHaptics; \
    auto& xrHapticScale = (v).xrHapticScale; \
    auto& xrPistolBack = (v).xrPistolBack; \
    auto& xrClimbFrac = (v).xrClimbFrac; \
    auto& xrClimbMax = (v).xrClimbMax; \
    auto& xrClimbMs = (v).xrClimbMs; \
    auto& xrReturnMs = (v).xrReturnMs; \
    auto& xrReturnPow = (v).xrReturnPow; \
    auto& xrScannerSlots = (v).xrScannerSlots; \
    auto& xrHandRelFiltered = (v).xrHandRelFiltered; \
    auto& xrHandPerFrame = (v).xrHandPerFrame; \
    auto& xrSnapTurnAngleDeg = (v).xrSnapTurnAngleDeg; \
    auto& xrMovementSource = (v).xrMovementSource; \
    auto& xrMovementSpeedMode = (v).xrMovementSpeedMode; \
    auto& xrSprintMode = (v).xrSprintMode; \
    auto& xrLeftStickDeadzone = (v).xrLeftStickDeadzone; \
    auto& xrRightStickDeadzone = (v).xrRightStickDeadzone; \
    auto& xrMaxInputThreshold = (v).xrMaxInputThreshold; \
    auto& xrXInputInstall = (v).xrXInputInstall; \
    auto& xrInputActions = (v).xrInputActions; \
    auto& xrMonoXQueueWait = (v).xrMonoXQueueWait; \
    auto& xrSnapTurnPulseMs = (v).xrSnapTurnPulseMs; \
    auto& xrMonoDepthCapture = (v).xrMonoDepthCapture; \
    auto& xrSnapTurnYawIndex = (v).xrSnapTurnYawIndex; \
    auto& xrImmersiveHolsters = (v).xrImmersiveHolsters; \
    auto& xrBodyHandsOnly = (v).xrBodyHandsOnly; \
    auto& xrPhysicalBodyRotation = (v).xrPhysicalBodyRotation; \
    auto& xrTrackedBodyRotation = (v).xrTrackedBodyRotation; \
    auto& xrHybridBodyRotation = (v).xrHybridBodyRotation; \
    auto& xrRoomscaleMovement = (v).xrRoomscaleMovement; \
    auto& xrPhysicalCrouch = (v).xrPhysicalCrouch; \
    auto& xrPhysicalCrouchDepth = (v).xrPhysicalCrouchDepth; \
    auto& xrBreaststrokeSwim = (v).xrBreaststrokeSwim; \
    auto& xrLadderGripClimb = (v).xrLadderGripClimb; \
    auto& xrLeftHanded = (v).xrLeftHanded; \
    auto& xrLeftHandedBindings = (v).xrLeftHandedBindings; \
    auto& xrLadderAutoFinish = (v).xrLadderAutoFinish; \
    auto& xrLadderFinishDistance = (v).xrLadderFinishDistance; \
    auto& xrBodyFreeLookDeg = (v).xrBodyFreeLookDeg; \
    auto& xrBodyFreeLookDownDeg = (v).xrBodyFreeLookDownDeg; \
    auto& xrBodyFreeLookSwimDeg = (v).xrBodyFreeLookSwimDeg; \
    auto& xrBodyMoveRadius = (v).xrBodyMoveRadius; \
    auto& xrSceneCamComfortDeg = (v).xrSceneCamComfortDeg; \
    auto& xrCutsceneSuspendTier = (v).xrCutsceneSuspendTier; \
    auto& xrVehicleCutsceneSuspendTier = (v).xrVehicleCutsceneSuspendTier; \
    auto& vehicleCutsceneTierConfigured = (v).vehicleCutsceneTierConfigured; \
    auto& xrVehHeadOffsetX = (v).xrVehHeadOffsetX; \
    auto& xrVehHeadOffsetY = (v).xrVehHeadOffsetY; \
    auto& xrVehHeadOffsetZ = (v).xrVehHeadOffsetZ; \
    auto& xrWheelGrab = (v).xrWheelGrab; \
    auto& xrMenuGesture = (v).xrMenuGesture; \
    auto& xrVrWeaponWheel = (v).xrVrWeaponWheel; \
    auto& xrWheelRadius = (v).xrWheelRadius; \
    auto& xrWheelSteerMaxDeg = (v).xrWheelSteerMaxDeg; \
    auto& xrWheelSteerDeadDeg = (v).xrWheelSteerDeadDeg; \
    auto& xrWheelPrediction = (v).xrWheelPrediction; \
    auto& xrWheelPredictionMs = (v).xrWheelPredictionMs; \
    auto& xrWheelHorn = (v).xrWheelHorn; \
    auto& xrWheelHornRadius = (v).xrWheelHornRadius; \
    auto& xrVehicleGunTrigger = (v).xrVehicleGunTrigger; \
    auto& xrVehicleThrottleTrim = (v).xrVehicleThrottleTrim; \
    static_cast<void>(0)
#define LIVE_CONTROLS_BIND_ALIASES(v) \
    auto& g_viveTrackersSetting = (v).g_viveTrackersSetting; \
    auto& CyberpunkVR_VisionCsNode = (v).CyberpunkVR_VisionCsNode; \
    auto& CyberpunkVR_VisionCsOrd = (v).CyberpunkVR_VisionCsOrd; \
    auto& CyberpunkVR_VisionCsDim = (v).CyberpunkVR_VisionCsDim; \
    auto& CyberpunkVR_VisionCsSwap = (v).CyberpunkVR_VisionCsSwap; \
    auto& CyberpunkVR_WeaponPlaneAsScene = (v).CyberpunkVR_WeaponPlaneAsScene; \
    auto& CyberpunkVR_HighlightAnyPlane = (v).CyberpunkVR_HighlightAnyPlane; \
    auto& CyberpunkVR_PlaneBucketScene = (v).CyberpunkVR_PlaneBucketScene; \
    static_cast<void>(0)
#define LIVE_CONTROLS_BIND_SUBSYSTEMS(s) \
    auto& hudElements = (s).hudElements; auto& framegen = (s).framegen; auto& foveated = (s).foveated; \
    auto& vrs = (s).vrs; auto& hiddenArea = (s).hiddenArea; auto& overlay = (s).overlay

// The C++ loop over an open ini (src/Core/LiveControlsParse.cpp).
void ParseLiveControlsLines(FILE* file, LiveControlsValues& v, LiveControlsSubsystems& s);
extern "C" {
// The Rust loop over the ini at `path`; false when it cannot be opened (rust/crates/cvr-settings).
bool cvr_settings_parse_live_controls(const char* path, LiveControlsValues* v, LiveControlsSubsystems* s);
// One subsystem's ParseSetting, for the Rust loop: 0 hud, 1 framegen, 2 foveated, 3 vrs, 4 hidden area, 5 overlay.
bool cvr_cpp_settings_parse_subsystem(int which, const char* line, LiveControlsSubsystems* s);
}
