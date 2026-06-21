#pragma once

struct LiveControlsUiState {
    float xrHeadOffsetX;
    float xrHeadOffsetY;
    float xrHeadOffsetZ;
    int xrMovementControl;
    int xrDisableMouseY;
    int xrRecenter;
    int xrMonoSubmit;
    int xrAERSubmit;
    float xrForceFov;
    int xrMenuRect;
    float xrMenuFov;
    int xr3DofMovement;
    int xrDLSSMatrixHook;
    int xrDLSSSlotMode;
    int xrDLSSLogStride;
    int xrAERPairGate;
    int xrAERStartEye;
    int xrAERDebugEye;
    float xrMotionPredictMs;
    float xrStereoScale;
    float xrWorldScale;   // uniform world scale (1.0 = default; <1 = world bigger)
    float xrIpdScale;     // eye-separation multiplier on runtime IPD (1.5 = legacy calib)
    float xrSharpness;    // CAS sharpen strength (0 = off .. 1)
    float xrSharpmix;     // CAS sharpen mix (0..1)
    int xrReuseLastFrame; // 1 = reuse last clean frame on stale AER ticks
    int xrNvofPerf;       // 0=FAST,1=MEDIUM,2=SLOW
    int xrPairLock;       // 1 = pose-pair-lock on (anti-tear); 0 = live pose (full-rate avatar)
    int xrRenderPoseSubmit;
    int xrAERHalfRate;
    int xrAERV2;
    int xrPoseLag;
    int xrRuntime;

    // HUD placement / scale controls expected by imgui_overlay.cpp.
    float xrHudScale;
    float xrHudScaleY;
    float xrHudMinimapQuestScale;

    float xrHudPhone;
    float xrHudPhoneY;
    float xrHudPhoneScale;

    float xrHudTopLeftAlerts;
    float xrHudTopLeftAlertsY;
    float xrHudTopLeftAlertsScale;

    float xrHudTopRight;
    float xrHudTopRightY;
    float xrHudTopRightScale;

    float xrHudBottomLeft;
    float xrHudBottomLeftY;
    float xrHudBottomLeftScale;

    float xrHudBottomLeftTop;
    float xrHudBottomLeftTopY;
    float xrHudBottomLeftTopScale;

    float xrHudRadio;
    float xrHudRadioY;
    float xrHudRadioScale;

    float xrHudBottomRight;
    float xrHudBottomRightY;
    float xrHudBottomRightScale;

    float xrHudRightCenter;
    float xrHudRightCenterY;
    float xrHudRightCenterScale;

    float xrHudJohnnyHint;
    float xrHudActivityLog;
    float xrHudWarning;
    float xrHudBossHealth;
    float xrHudVehicleScan;
    float xrHudProgressBar;
    float xrHudOxygenBar;

    // Weapon proxy controls expected by newer overlay/runtime code.
    float xrWeaponPitch;
    float xrWeaponYaw;
    float xrWeaponRoll;
    float xrWeaponOffsetX;
    float xrWeaponOffsetY;
    float xrWeaponOffsetZ;

    // VR controller -> XInput merge. xrXInputHook: 0 = passthrough only (no VR
    // input), 1 = hook XInputGetState and OR VR state into pad 0. xrSnapTurn:
    // when 1, right-stick X is converted from analog rotation to discrete
    // snap-turn pulses of xrSnapTurnAngleDeg.
    int xrXInputHook;
    int xrSnapTurn;
    float xrSnapTurnAngleDeg;
    // Locomotion direction source extension. xrMovementControl is the legacy 0/1
    // (Game/HMD) value and stays for back-compat; xrMovementSource is the new
    // 0..3 enum (0=Game, 1=HMD, 2=LeftHand, 3=RightHand). The overlay edits the
    // latter and the proxy mirrors it back into the legacy field.
    int xrMovementSource;
    // Kill-switches for the new controller pipeline; default 0 (off) so a stuck
    // OpenXR binding or XInput entry-point patch can't keep CP2077 from booting.
    int xrXInputInstall;
    int xrInputActions;
    // Mono submit safety flags. Defaults 0 keep CP2077 mono mode from hanging on
    // the menu (see cybervrport-controller-bindings memory for the trace).
    int xrMonoXQueueWait;
    int xrMonoDepthCapture;
    // AER equivalent: 1 = AER capture inserts the cross-queue Wait + runs depth
    // capture every present (safe vs save/load depth recycle, but serializes the
    // present queue behind CP2077's async-compute -> depressed sim rate). 0
    // (default) skips both, like mono -- the AER V2 warp falls back to NvOF-only
    // (agreement-gated depth path self-disables), smooth at the cost of depth.
    int xrAerXQueueWait;
    int xrSnapTurnPulseMs;
    // Weapon holster mode. 1 (default) = immersive: equip is chosen by which visual
    // holster (katana / pistol / back-strap) the hand reaches. 0 = simple slot mapping
    // ignoring visual holsters: over-shoulder = EquipmentSlot1, right hip = Slot2,
    // left hip = Slot3. Read by the CET Holster mod via GetVRSharedSlot(23).
    int xrImmersiveHolsters;
};

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState);
extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile);
extern "C" void RequestLiveControlsRecenter();
