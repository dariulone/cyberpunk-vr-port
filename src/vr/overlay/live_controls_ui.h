#pragma once

struct LiveControlsUiState {
    float xrHeadOffsetX;
    float xrHeadOffsetY;
    float xrHeadOffsetZ;
    int xrMovementControl;
    int xrDisableMouseY;
    int xrRecenter;
    int xrMonoSubmit;
    float xrForceFov;
    int xrMenuRect;
    float xrMenuFov;
    float xrMenuFollowDeg;
    int xr3DofMovement;
    int xrFirstLaunch;      // not a control -- carried so a UI save does not drop the key
    float xrMotionPredictMs;
    float xrStereoScale;
    float xrWorldScale;   // uniform world scale (1.0 = default; <1 = world bigger)
    float xrIpdScale;     // eye-separation multiplier on runtime IPD (1.5 = legacy calib)
    float xrSharpness;    // CAS sharpen strength (0 = off .. 1)
    float xrSharpmix;     // CAS sharpen mix (0..1)
    int xrReuseLastFrame; // 1 = reuse last clean frame on stale ticks
    int xrPairLock;       // 1 = pose-pair-lock on (anti-tear); 0 = live pose (full-rate avatar)
    int xrRenderPoseSubmit;
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
    int xrSnapTurnPulseMs;
    // Weapon holster mode. 1 (default) = immersive: equip is chosen by which visual
    // holster (katana / pistol / back-strap) the hand reaches. 0 = simple slot mapping
    // ignoring visual holsters: over-shoulder = EquipmentSlot1, right hip = Slot2,
    // left hip = Slot3. Read by the CET Holster mod via GetVRSharedSlot(23).
    int xrImmersiveHolsters;
    // Physical body rotation. 1 = the avatar body follows the HMD/aim heading
    // (continuous body-yaw tracking on foot; aiming / holding a weapon switches the
    // camera to full head-look + head-relative movement). 0 (default) = classic
    // stick / snap-turn heading. Vehicles are unaffected either way. F10 -> VRIK tab.
    int xrPhysicalBodyRotation;
    // HTC Vive leg trackers (feet). xrLegTrackers: 1 = foot trackers drive the
    // avatar legs via the VRIK plugin (needs SteamVR + assigned foot roles).
    // xrLegAnkleOffset: tracker-on-shoe -> ankle joint vertical offset (m).
    // xrLegMount*Deg: foot-orientation mount correction euler (deg), tuned live
    // from the F10 -> VRIK tab until the avatar feet point where yours do.
    int xrLegTrackers;
    // Optional 3rd tracking point (waist/belt): 0 (default) = 2-point feet only,
    // 1 = waist tracker also drives the avatar hips (needs the waist role
    // assigned in SteamVR's tracker management).
    int xrWaistTracker;
    float xrLegAnkleOffset;
    float xrLegMountPitchDeg;
    float xrLegMountYawDeg;
    float xrLegMountRollDeg;
    // fix12: per-foot manual rotation trim (deg), stacked on the calibrated
    // mount by the stereo side at publish time, in world space: yaw about
    // world up (+ = toes turn left), roll about body forward (+ = tips the
    // boot's top to your right), pitch about body right (+ = toes up).
    // Zero = calibration result untouched. F10 -> VRIK tab, live.
    float xrLegAdjYawDegL;
    float xrLegAdjRollDegL;
    float xrLegAdjPitchDegL;
    float xrLegAdjYawDegR;
    float xrLegAdjRollDegR;
    float xrLegAdjPitchDegR;
    // fix15: 1 = a fast physical foot strike taps the native melee attack
    // (shared[29] RT impulse) so kicks damage NPCs with the game's own
    // damage/numbers/armor. Empty hands only (the plugin gates it). 0 default.
    int xrLegKickDamage;
};

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState);
extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile);
extern "C" void RequestLiveControlsRecenter();
