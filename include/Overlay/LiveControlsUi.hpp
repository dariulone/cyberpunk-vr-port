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
    // Laser dot projection mode. 0 = steady projection, 1 = real world point, 2 = surface raycast.
    int xrLaserDotMode;
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
    // Cutscene VRIK suspend (PR #40). The minimum GameplayTier at which the plugin fully suspends
    // the body+arm solve (leaving the engine authored cinematic pose): -1 = never suspend, 0..4 =
    // Tier1..Tier5. Default 3 (Tier4_FPPCinematic = true cinematics). F10 -> VRIK tab.
    int xrCutsceneSuspendTier;
    // In-vehicle head offset, metres, ADDED to the xrHeadOffset* trio while seated and zero by
    // default. The on-foot trio is a standing calibration; seated, the vehicle camera is already in
    // place, so that calibration carries the view off the seat. F10 -> Tracking / Camera.
    float xrVehHeadOffsetX;
    float xrVehHeadOffsetY;
    float xrVehHeadOffsetZ;
    // ---- DRIVING: hands on the wheel ----------------------------------------------------------
    // 1 (default) = while driving, bringing a hand to where the driving animation holds the wheel and
    // squeezing that grip hands the arm back to the animation (hand on the wheel, native finger curl);
    // releasing the grip returns it to the controller. Per hand, independent. xrWheelRadius is how
    // close the hand has to be, in metres, before the grip means "grab" instead of its normal action.
    int xrWheelGrab;
    float xrWheelRadius;
    // Controller tilt that means full lock, degrees. 90 (default) = hands vertical, the 1:1 reading of
    // a real wheel; lower = the same wrist movement steers more. The deadzone is the tilt around
    // centre that steers nothing: 1.5 (default) only swallows tremor.
    float xrWheelSteerMaxDeg;
    float xrWheelSteerDeadDeg;
    // HORN. 1 (default) = laying a hand on the middle of the wheel presses the game's horn button for
    // as long as it stays there. xrWheelHornRadius is how near the wheel centre, in metres, counts as
    // "on the hub".
    int xrWheelHorn;
    float xrWheelHornRadius;
    // DRIVING WITH A GUN. 1 (default) = with a weapon equipped in the driver seat the right trigger
    // fires it (pad RB) and the throttle is latched at what it was, trimmed by the left stick's Y at
    // xrVehicleThrottleTrim of full travel per second.
    int xrVehicleGunTrigger;
    float xrVehicleThrottleTrim;
};

// Read-only snapshot for the compact ADS-camera diagnostic (dabinn, TofuExpress f8a827eb).
// Positions are the ENGINE camera's residual relative to the entity + clean-pair anchor -- i.e.
// what the game itself moved the camera by, before VR adds head translation -- expressed in the
// camera heading's local right/forward/up basis. Metres.
struct AdsCameraTelemetryUiState {
    int available;
    int aiming;
    int baselineValid;
    unsigned int samples;
    float residualRight;
    float residualForward;
    float residualUp;
    float deltaRight;
    float deltaForward;
    float deltaUp;
    float peakRight;
    float peakForward;
    float peakUp;
};

extern "C" void GetAdsCameraTelemetryUiState(AdsCameraTelemetryUiState* outState);

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState);
extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile);
extern "C" void RequestLiveControlsRecenter();
