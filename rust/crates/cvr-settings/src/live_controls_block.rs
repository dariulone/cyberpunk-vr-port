//! The live settings block (include/Core/LiveControls.hpp `LiveControls`, the C++ half's `g_liveControls`): vrport.ini
//! re-read whenever its timestamp changes, every field a 4-byte scalar the poll thread writes while the render, present
//! and game threads read it -- volatile on both sides. Mirrored field for field under the C++ names (a size check in
//! VrCore.cpp keeps the two in step). The plugin's block is defined here under the C++ symbol (formerly VrCore.cpp's),
//! with the C++ struct's default member initializers; a test program attaches its own.
use core::sync::atomic::{AtomicPtr, Ordering};

#[repr(C)]
#[allow(non_snake_case)]
pub struct LiveControls {
    pub xrHeadOffsetX: f32,
    pub xrHeadOffsetY: f32,
    pub xrHeadOffsetZ: f32,
    pub xrRecenter: i32,
    pub xrMonoSubmit: i32,
    pub xrForceFov: f32,
    pub xrMenuRect: i32,
    pub xrMenuFov: f32,
    /// head-vs-panel yaw offset (deg) that starts the lazy menu re-center
    pub xrMenuFollowDeg: f32,
    pub xrHudPanel: i32,
    pub xrHudFollowDeg: f32,
    pub xrInteractionPanel: i32,
    /// 0 = head-locked
    pub xrInteractionFollowDeg: f32,
    pub xrLootFollowDeg: f32,
    pub xrInteractionDistance: f32,
    pub xrInteractionFov: f32,
    pub xrHudFov: f32,
    pub xrHudDistance: f32,
    /// 0=head cone, 1=physical body
    pub xrHudFollowMode: i32,
    /// 0=same angular position for both eyes
    pub xrHudStereoDepth: i32,
    pub xrHudBrightness: f32,
    pub xrHudShadow: f32,
    pub xrHudGlow: f32,
    pub xr3DofMovement: i32,
    pub xrFirstLaunch: i32,
    pub xrMotionPredictMs: f32,
    pub xrStereoScale: f32,
    /// uniform world scale (1.0 = default, <1 = world bigger)
    pub xrWorldScale: f32,
    /// eye-separation multiplier on runtime IPD
    pub xrIpdScale: f32,
    /// CAS Emphasis (0..1; zero still sharpens)
    pub xrSharpness: f32,
    /// CAS Sharpening (0..1 -> shader mix 0..4)
    pub xrSharpmix: f32,
    /// 1 = reuse last clean frame on stale ticks
    pub xrReuseLastFrame: i32,
    /// 1 = freeze tracked pose per stereo pair (anti-tear). 0 = live pose every locate.
    pub xrPairLock: i32,
    pub xrRenderPoseSubmit: i32,
    pub xrPoseLag: i32,
    pub xrRuntime: i32,
    pub xrDepthSubmit: i32,
    /// 0 = Game heading, 1 = HMD head-oriented locomotion (legacy mirror of xrMovementSource)
    pub xrMovementControl: i32,
    /// 1 = suppress mouse pitch (CET VRIK mod applies it)
    pub xrDisableMouseY: i32,
    /// 1 = merge VR controller into XInput gamepad 0
    pub xrXInputHook: i32,
    /// 1 = discrete snap turn from right-stick X
    pub xrSnapTurn: i32,
    /// degrees per snap pulse
    pub xrSnapTurnAngleDeg: f32,
    /// 0 = Game, 1 = HMD, 2 = LeftHand, 3 = RightHand
    pub xrMovementSource: i32,
    /// 0=current fixed speed, 1=analog on foot
    pub xrMovementSpeedMode: i32,
    pub xrSprintMode: i32,
    pub xrLeftStickDeadzone: f32,
    pub xrRightStickDeadzone: f32,
    pub xrMaxInputThreshold: f32,
    /// 1 = install the XInput entry-point detour at startup (default 1, set 0 in vrport.ini to fully bypass)
    pub xrXInputInstall: i32,
    /// 1 = create gameplay XrActions (thumbstick/trigger/buttons). 0 = pose-only legacy behaviour
    pub xrInputActions: i32,
    /// 1 = mono path inserts cross-queue Wait before depth capture (legacy). 0 = skip it -- avoids CP2077 async-compute Wait cycle that froze present thread.
    pub xrMonoXQueueWait: i32,
    /// duration of the discrete snap turn pulse pushed into the right stick (ms)
    pub xrSnapTurnPulseMs: i32,
    /// 1 (default) = mono scene-depth for XR_KHR_composition_layer_depth. The resolve reads the game depth as an SRV WITHOUT transitioning it (D3D12 state is global -> barriering the game's resource device-removes CP2077), on our own capture queue (FIFO before the submit's depth copy, no cross-queue Wait), and only once the scene depth has been a stable shader-readable resource with menus closed for a warmup window (skips the intro/menu-load transient). 0 = no depth in mono.
    pub xrMonoDepthCapture: i32,
    /// which float index in deltaHead[] gets the snap yaw. Default 1.
    pub xrSnapTurnYawIndex: i32,
    /// 1 = visual-holster equip (default), 0 = simple slot mapping (back=Slot1, R hip=Slot2, L hip=Slot3). Published to shared[23] for the CET Holster mod.
    pub xrImmersiveHolsters: i32,
    /// 1 = draw only the hands of the player's body (the rest of the body and every garment hidden, the belt and the holsters kept). Published to shared[173] for the CET HandsOnly mod; the neck cap stays off.
    pub xrBodyHandsOnly: i32,
    pub xrReloadGlow: i32,
    pub xrCutsceneSuspendTier: i32,
    pub xrVehHeadOffsetX: f32,
    pub xrVehHeadOffsetY: f32,
    pub xrVehHeadOffsetZ: f32,
    pub xrBikeHeadOffsetX: f32,
    pub xrBikeHeadOffsetY: f32,
    pub xrBikeHeadOffsetZ: f32,
    /// 1 (default) = while DRIVING, a grip squeezed with the hand on the animated wheel pose hands that arm back to the driving animation. Per hand.
    pub xrWheelGrab: i32,
    /// how near the animated hand the controller must be for the grip to mean "grab", metres. Default 0.28.
    pub xrWheelRadius: f32,
    /// bike/other mount hand travel to full lock per side (30..120, else 90); a car's is its cabin wheel's (WheelRange.hpp, 1:1)
    pub xrWheelSteerMaxDeg: f32,
    pub xrBikeBarLockDeg: f32,
    pub xrBikeBodyLean: f32,
    /// steering deadzone around centre, degrees. 0 (default): a real wheel has none; every degree here is a degree of dead wheel.
    pub xrWheelSteerDeadDeg: f32,
    /// optional conservative history-based steering lead
    pub xrWheelPrediction: i32,
    /// 0..8 ms, additionally limited to half a degree
    pub xrWheelPredictionMs: f32,
    /// 1 (default) = a hand laid on the wheel HUB holds the horn (pad X = Vehicle_Horn) for as long as it stays there.
    pub xrWheelHorn: i32,
    /// how near the wheel centre counts as "on the hub", metres. Default 0.12.
    pub xrWheelHornRadius: f32,
    /// 1 (default) = with a weapon out in the driver seat the right trigger FIRES (pad RB) and the throttle is latched.
    pub xrVehicleGunTrigger: i32,
    /// how much of the throttle's full travel the left stick adds or removes per second while a weapon is out. Default 0.5.
    pub xrVehicleThrottleTrim: f32,
    /// 1 = physical body rotation (avatar body follows HMD/aim heading). 0 (default) = classic stick/snap heading. Gates the aiming/weapon body-turn paths; vehicles unaffected.
    pub xrPhysicalBodyRotation: i32,
    /// HMD + controllers, replaces head/cone follow while enabled
    pub xrTrackedBodyRotation: i32,
    /// upright head cone; tracked body yaw on looking down / bending
    pub xrHybridBodyRotation: i32,
    pub xrHeadLedBodyRotation: i32,
    /// derived single-scalar publication; never serialize separately
    pub xrBodyRotationMode: i32,
    /// horizontal physical displacement through the native CCT
    pub xrRoomscaleMovement: i32,
    pub xrPhysicalCrouch: i32,
    /// height loss after measured hinge contribution, metres (default: the tuned value, 2026-10-17)
    pub xrPhysicalCrouchDepth: f32,
    pub xrBodyFreeLookDeg: f32,
    /// metres; roomscale movement starts outside this radius
    pub xrBodyMoveRadius: f32,
    /// scene camera turn the view absorbs (Camera/SceneCameraComfort.hpp); 0 = off
    pub xrSceneCamComfortDeg: f32,
    pub xrBodyFreeLookDownDeg: f32,
    pub xrBodyFreeLookSwimDeg: f32,
    pub xrVehicleCutsceneSuspendTier: i32,
    pub xrBreaststrokeSwim: i32,
    pub xrLadderGripClimb: i32,
    /// xr_left_handed: the controllers' roles traded (Hooks/Handedness.hpp)
    pub xrLeftHanded: i32,
    pub xrLeftHandedBindings: i32,
    pub xrLadderAutoFinish: i32,
    pub xrLadderFinishDistance: f32,
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
}
const _: () = assert!(core::mem::size_of::<LiveControls>() == 440);

impl LiveControls {
    /// `LiveControls{}`: the default member initializers of include/Core/LiveControls.hpp, zero elsewhere.
    pub const DEFAULT: LiveControls = LiveControls {
            xrHeadOffsetX: 0.0,
            xrHeadOffsetY: 0.0,
            xrHeadOffsetZ: 0.0,
            xrRecenter: 0,
            xrMonoSubmit: 0,
            xrForceFov: 0.0,
            xrMenuRect: 0,
            xrMenuFov: 0.0,
            xrMenuFollowDeg: 0.0,
            xrHudPanel: 1,
            xrHudFollowDeg: 60.0,
            xrInteractionPanel: 1,
            xrInteractionFollowDeg: 0.0,
            xrLootFollowDeg: 0.0,
            xrInteractionDistance: 2.0,
            xrInteractionFov: 75.0,
            xrHudFov: 65.4,
            xrHudDistance: 1.5,
            xrHudFollowMode: 0,
            xrHudStereoDepth: 1,
            xrHudBrightness: 1.0,
            xrHudShadow: 1.0,
            xrHudGlow: 1.0,
            xr3DofMovement: 0,
            xrFirstLaunch: 0,
            xrMotionPredictMs: 0.0,
            xrStereoScale: 0.0,
            xrWorldScale: 0.0,
            xrIpdScale: 0.0,
            xrSharpness: 0.0,
            xrSharpmix: 0.0,
            xrReuseLastFrame: 0,
            xrPairLock: 0,
            xrRenderPoseSubmit: 0,
            xrPoseLag: 0,
            xrRuntime: 0,
            xrDepthSubmit: 0,
            xrMovementControl: 0,
            xrDisableMouseY: 0,
            xrXInputHook: 0,
            xrSnapTurn: 0,
            xrSnapTurnAngleDeg: 0.0,
            xrMovementSource: 0,
            xrMovementSpeedMode: 0,
            xrSprintMode: 0,
            xrLeftStickDeadzone: 0.15,
            xrRightStickDeadzone: 0.15,
            xrMaxInputThreshold: 0.90,
            xrXInputInstall: 0,
            xrInputActions: 0,
            xrMonoXQueueWait: 0,
            xrSnapTurnPulseMs: 0,
            xrMonoDepthCapture: 0,
            xrSnapTurnYawIndex: 0,
            xrImmersiveHolsters: 0,
            xrBodyHandsOnly: 0,
            xrReloadGlow: 0,
            xrCutsceneSuspendTier: 0,
            xrVehHeadOffsetX: 0.0,
            xrVehHeadOffsetY: 0.0,
            xrVehHeadOffsetZ: 0.0,
            xrBikeHeadOffsetX: 0.0,
            xrBikeHeadOffsetY: 0.0,
            xrBikeHeadOffsetZ: 0.0,
            xrWheelGrab: 0,
            xrWheelRadius: 0.0,
            xrWheelSteerMaxDeg: 0.0,
            xrBikeBarLockDeg: 0.0,
            xrBikeBodyLean: 0.0,
            xrWheelSteerDeadDeg: 0.0,
            xrWheelPrediction: 0,
            xrWheelPredictionMs: 8.0,
            xrWheelHorn: 0,
            xrWheelHornRadius: 0.0,
            xrVehicleGunTrigger: 0,
            xrVehicleThrottleTrim: 0.0,
            xrPhysicalBodyRotation: 0,
            xrTrackedBodyRotation: 0,
            xrHybridBodyRotation: 0,
            xrHeadLedBodyRotation: 1,
            xrBodyRotationMode: 4,
            xrRoomscaleMovement: 1,
            xrPhysicalCrouch: 1,
            xrPhysicalCrouchDepth: 0.45,
            xrBodyFreeLookDeg: 10.0,
            xrBodyMoveRadius: 0.10,
            xrSceneCamComfortDeg: 30.0,
            xrBodyFreeLookDownDeg: 30.0,
            xrBodyFreeLookSwimDeg: 5.0,
            xrVehicleCutsceneSuspendTier: 3,
            xrBreaststrokeSwim: 1,
            xrLadderGripClimb: 1,
            xrLeftHanded: 0,
            xrLeftHandedBindings: 2,
            xrLadderAutoFinish: 1,
            xrLadderFinishDistance: 1.0,
            xrMenuGesture: 1,
            xrVrInterface: 1,
            xrHandPhysics: 1,
            xrVrWeaponWheel: 1,
            xrHandCollision: 1,
            xrHandCollisionVehicle: 1,
            xrHandPush: 1,
            xrFingerContact: 0,
            xrPropGrab: 1,
            xrDebugView: 0,
            xrCarryPicture: 1,
            xrCarryLead: 0.03,
            xrContactMargin: 0.01,
            xrWheelAnywhere: 1,
    };
}

/// The plugin's block (LiveControls g_liveControls, formerly src/Core/VrCore.cpp's), under the C++ symbol the other
/// crates read it by.
#[cfg(feature = "plugin-tables")]
#[allow(non_upper_case_globals)]
#[export_name = "?g_liveControls@@3ULiveControls@@A"]
pub static mut g_liveControls: LiveControls = LiveControls::DEFAULT;
#[cfg(feature = "plugin-tables")]
static BLOCK: AtomicPtr<LiveControls> = AtomicPtr::new(&raw mut g_liveControls);
#[cfg(not(feature = "plugin-tables"))]
static BLOCK: AtomicPtr<LiveControls> = AtomicPtr::new(core::ptr::null_mut());
/// Attached at the C++ half's static initialisation (src/Core/VrCore.cpp, where `g_liveControls` is defined).
#[no_mangle]
pub extern "C" fn cvr_live_controls_block_attach(block: *mut LiveControls) {
    BLOCK.store(block, Ordering::Release);
}
/// The block (null before the C++ half attached it).
pub fn block() -> *mut LiveControls {
    BLOCK.load(Ordering::Acquire)
}

/// A field, read volatile: `lc!(xrForceFov)`.
#[macro_export]
macro_rules! lc {
    ($field:ident) => {{
        let b = $crate::live_controls_block::block();
        // SAFETY: the C++ half's block, alive for the process; a 4-byte aligned scalar.
        unsafe { core::ptr::read_volatile(&raw const (*b).$field) }
    }};
}
/// A field, written volatile: `lc_set!(xrDisableMouseY, 1)`.
#[macro_export]
macro_rules! lc_set {
    ($field:ident, $value:expr) => {{
        let b = $crate::live_controls_block::block();
        // SAFETY: as lc!.
        unsafe { core::ptr::write_volatile(&raw mut (*b).$field, $value) }
    }};
}

#[cfg(test)]
mod default_tests {
    use super::LiveControls;

    /// The bytes of `LiveControls g_liveControls = {};` as MSVC builds them (dumped by a C++ program that includes
    /// include/Core/LiveControls.hpp).
    #[test]
    fn default_is_the_cpp_value() {
        const CPP: [&str; 10] = [
            "000000000000000000000000000000000000000000000000000000000000000000000000010000000000704201000000",
            "00000000000000000000004000009642cdcc82420000c03f00000000010000000000803f0000803f0000803f00000000",
            "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000000000009a99193e9a99193e6666663f",
            "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000041",
            "00000000000000000000000000000000000000000000000000000000010000000400000001000000010000006666e63e",
            "00002041cdcccc3d0000f0410000f0410000a0400300000001000000010000000000000002000000010000000000803f",
            "01000000010000000100000001000000010000000100000001000000000000000100000000000000010000008fc2f53c",
            "0ad7233c01000000",
        ];
        let expected: Vec<u8> = CPP
            .concat()
            .as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let value = LiveControls::DEFAULT;
        // SAFETY: a plain repr(C) block of 4-byte scalars, no padding.
        let bytes = unsafe { core::slice::from_raw_parts((&raw const value).cast::<u8>(), core::mem::size_of::<LiveControls>()) };
        assert_eq!(bytes, &expected[..]);
    }
}
