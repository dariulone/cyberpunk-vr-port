#pragma once
#include "Render/SharpenPolicy.hpp"

// ================================================================================================
// The live settings block -- vrport.ini, re-read whenever the file's timestamp changes.
//
// Every field is `volatile` because the poll thread writes them while the render, present and
// game threads read them. That is the whole synchronisation: these are independent scalars, no
// field depends on another being updated in the same pass, and a torn read of one aligned int is
// not a state this code can observe.
//
// Hooks read this directly. That is deliberate rather than lazy -- routing forty settings through
// forty accessors buys nothing here, and the hub already exports accessors for the handful that
// cross the C ABI to the overlay.
// ================================================================================================

struct LiveControls {
    volatile float xrHeadOffsetX;
    volatile float xrHeadOffsetY;
    volatile float xrHeadOffsetZ;
    volatile int xrRecenter;
    volatile int xrMonoSubmit;
    volatile float xrForceFov;
    volatile int xrMenuRect;
    volatile float xrMenuFov;
    volatile float xrMenuFollowDeg; // head-vs-panel yaw offset (deg) that starts the lazy menu re-center
    volatile int xrHudPanel = 1;
    volatile float xrHudFollowDeg = 60.0f;
    volatile int xrInteractionPanel = 1;
    volatile float xrInteractionFollowDeg = 0.0f; // 0 = head-locked
    volatile float xrLootFollowDeg = 0.0f;
    volatile float xrInteractionDistance = 2.0f;
    volatile float xrInteractionFov = 75.0f;
    volatile float xrHudFov = 65.4f;
    volatile float xrHudDistance = 1.5f;
    volatile int xrHudFollowMode = 0; // 0=head cone, 1=physical body
    volatile int xrHudStereoDepth = 1; // 0=same angular position for both eyes
    volatile float xrHudBrightness = 1.0f;
    volatile float xrHudShadow = 1.0f;
    volatile float xrHudGlow = 1.0f;
    volatile int xr3DofMovement;
    // 1 = this is still the first launch, i.e. the shipped UserSettings.json has not been
    // installed yet. ApplyFirstLaunchGameSettings CLEARS it to 0 once it has, so a player's own
    // tuning is never overwritten twice. See the note there.
    volatile int xrFirstLaunch;
    volatile float xrMotionPredictMs;
    volatile float xrStereoScale;
    volatile float xrWorldScale;   // uniform world scale (1.0 = default, <1 = world bigger)
    volatile float xrIpdScale;     // eye-separation multiplier on runtime IPD
    volatile float xrSharpness;    // CAS Emphasis (0..1; zero still sharpens)
    volatile float xrSharpmix = cvr::sharpen::DefaultAmount; // CAS Sharpening (0..1 -> shader mix 0..4)
    volatile int xrReuseLastFrame; // 1 = reuse last clean frame on stale ticks
    volatile int xrPairLock;       // 1 = freeze tracked pose per stereo pair (anti-tear). 0 = live pose every locate.
    volatile int xrRenderPoseSubmit;
    volatile int xrPoseLag;
    volatile int xrRuntime;
    volatile int xrDepthSubmit;
    volatile int xrMovementControl; // 0 = Game heading, 1 = HMD head-oriented locomotion (legacy mirror of xrMovementSource)
    volatile int xrDisableMouseY;   // 1 = suppress mouse pitch (CET VRIK mod applies it)
    volatile int xrXInputHook;      // 1 = merge VR controller into XInput gamepad 0
    volatile int xrSnapTurn;        // 1 = discrete snap turn from right-stick X
    volatile float xrSnapTurnAngleDeg; // degrees per snap pulse
    volatile int xrMovementSource;  // 0 = Game, 1 = HMD, 2 = LeftHand, 3 = RightHand
    volatile int xrMovementSpeedMode = 0; // 0=current fixed speed, 1=analog on foot
    // xr_sprint_mode: 0 (default) the left stick held at the stop sprints, 1 a click of the left stick sprints until
    // the stick is let go (Hooks/XInput.cpp, the sprint gesture)
    volatile int xrSprintMode = 0;
    volatile float xrLeftStickDeadzone = .15f;
    volatile float xrRightStickDeadzone = .15f;
    volatile float xrMaxInputThreshold = .90f;
    volatile int xrXInputInstall;   // 1 = install the XInput entry-point detour at startup (default 1, set 0 in vrport.ini to fully bypass)
    volatile int xrInputActions;    // 1 = create gameplay XrActions (thumbstick/trigger/buttons). 0 = pose-only legacy behaviour
    volatile int xrMonoXQueueWait;  // 1 = mono path inserts cross-queue Wait before depth capture (legacy). 0 = skip it -- avoids CP2077 async-compute Wait cycle that froze present thread.
    volatile int xrSnapTurnPulseMs; // duration of the discrete snap turn pulse pushed into the right stick (ms)
    volatile int xrMonoDepthCapture; // 1 (default) = mono scene-depth for XR_KHR_composition_layer_depth. The resolve reads the game depth as an SRV WITHOUT transitioning it (D3D12 state is global -> barriering the game's resource device-removes CP2077), on our own capture queue (FIFO before the submit's depth copy, no cross-queue Wait), and only once the scene depth has been a stable shader-readable resource with menus closed for a warmup window (skips the intro/menu-load transient). 0 = no depth in mono.
    volatile int xrSnapTurnYawIndex; // which float index in deltaHead[] gets the snap yaw. Default 1.
    volatile int xrImmersiveHolsters; // 1 = visual-holster equip (default), 0 = simple slot mapping (back=Slot1, R hip=Slot2, L hip=Slot3). Published to shared[23] for the CET Holster mod.
    volatile int xrBodyHandsOnly;     // 1 = draw only the hands of the player's body (the rest of the body and every garment hidden, the belt and the holsters kept). Published to shared[173] for the CET HandsOnly mod; the neck cap stays off.
    volatile int xrReloadGlow;        // 1 = the physical reload highlights what the free hand reaches for (the magazine, the bolt to rack; default). Published inverted to shared[174] for the CET Reload mod.
    // Cutscene VRIK suspend (PR #40): minimum GameplayTier at which the pose-apply hook fully
    // suspends the body+arm solve. Stored UI index: -1/0 = never, 1..4 = Tier2..Tier5.
    // Default 3 selects engine Tier4 (enum value 4). Read directly by AnimPose.
    volatile int xrCutsceneSuspendTier;
    // IN-VEHICLE HEAD OFFSET, metres, in the view's own right/forward/up basis. ADDED to the
    // xrHeadOffset* trio while the player is mounted, and 0 by default so it changes nothing until
    // it is moved. It exists because the on-foot trio is a STANDING calibration: seated, the vehicle
    // camera is already in the right place (which is why the two automatic bakes are dropped there --
    // see LocateCamera), and a standing offset then carries the view off the seat.
    volatile float xrVehHeadOffsetX;
    volatile float xrVehHeadOffsetY;
    volatile float xrVehHeadOffsetZ;
    // ---- DRIVING: hands on the wheel (iPowerTech, 425d4262 + 51861118) --------------------------
    // None of these is published to a shared slot: the consumers (src/Anim/WheelGrab.cpp and the
    // XInput merge) are in this DLL and read them straight from here.
    volatile int xrWheelGrab;        // 1 (default) = while DRIVING, a grip squeezed with the hand on the animated wheel pose hands that arm back to the driving animation. Per hand.
    volatile float xrWheelRadius;    // how near the animated hand the controller must be for the grip to mean "grab", metres. Default 0.28.
    volatile float xrWheelSteerMaxDeg;  // bike/other mount hand travel to full lock per side (30..120, else 90); a car's is its cabin wheel's (WheelRange.hpp, 1:1)
    volatile float xrWheelSteerDeadDeg; // steering deadzone around centre, degrees. 0 (default): a real wheel has none; every degree here is a degree of dead wheel.
    volatile int xrWheelPrediction = 0; // optional conservative history-based steering lead
    volatile float xrWheelPredictionMs = 8.0f; // 0..8 ms, additionally limited to half a degree
    volatile int xrWheelHorn;        // 1 (default) = a hand laid on the wheel HUB holds the horn (pad X = Vehicle_Horn) for as long as it stays there.
    volatile float xrWheelHornRadius;   // how near the wheel centre counts as "on the hub", metres. Default 0.12.
    volatile int xrVehicleGunTrigger;   // 1 (default) = with a weapon out in the driver seat the right trigger FIRES (pad RB) and the throttle is latched.
    volatile float xrVehicleThrottleTrim; // how much of the throttle's full travel the left stick adds or removes per second while a weapon is out. Default 0.5.
    volatile int xrPhysicalBodyRotation; // 1 = physical body rotation (avatar body follows HMD/aim heading). 0 (default) = classic stick/snap heading. Gates the aiming/weapon body-turn paths; vehicles unaffected.
    volatile int xrTrackedBodyRotation = 0; // HMD + controllers, replaces head/cone follow while enabled
    volatile int xrHybridBodyRotation = 0; // upright head cone; tracked body yaw on looking down / bending (not the default)
    volatile int xrHeadLedBodyRotation = 1; // the head-led body turn (Runtimes/HeadLedBodyTurn.hpp); the default
    volatile int xrBodyRotationMode = 4; // derived single-scalar publication; never serialize separately
    volatile int xrRoomscaleMovement = 1; // horizontal physical displacement through the native CCT
    volatile int xrPhysicalCrouch = 1;
    volatile float xrPhysicalCrouchDepth = .45f; // height loss after measured hinge contribution, metres (default: the tuned value, 2026-10-17)
    volatile float xrBodyFreeLookDeg = 10.0f;
    volatile float xrBodyMoveRadius = .10f; // metres; roomscale movement starts outside this radius
    volatile float xrSceneCamComfortDeg = 30.0f; // scene camera turn the view absorbs (Camera/SceneCameraComfort.hpp); 0 = off
    volatile float xrBodyFreeLookDownDeg = 30.0f;
    volatile float xrBodyFreeLookSwimDeg = 5.0f;
    // Independent vehicle scene threshold; same stored tier encoding as on foot.
    volatile int xrVehicleCutsceneSuspendTier = 3;
    volatile int xrBreaststrokeSwim = 1;
    volatile int xrLadderGripClimb = 1;
    volatile int xrLeftHanded = 0;     // xr_left_handed: the controllers' roles traded (Hooks/Handedness.hpp)
    // xr_left_handed_bindings, left-handed: 0 the right-handed buttons and sticks (only the triggers and grips go with
    // the weapon hand), 1 the left stick walks with the buttons mirrored, 2 everything mirrored, the left stick turns
    volatile int xrLeftHandedBindings = 2;
    volatile int xrLadderAutoFinish = 1;
    volatile float xrLadderFinishDistance = 1.0f;
    // 1 (default) = the right grip held and swept straight down opens or closes the VR menu (never while that
    // hand holds the steering wheel). 0 = only L3 + R3 does.
    volatile int xrMenuGesture = 1;
    // 1 (default) = holding Y opens the port's weapon wheel in front of the body and the game keeps running; a
    // tap is still the game's next weapon. 0 = the game's own wheel (it slows time). Hooks/WeaponWheelHold.hpp.
    volatile int xrVrWeaponWheel = 1;
};

extern LiveControls g_liveControls;
