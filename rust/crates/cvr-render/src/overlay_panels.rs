//! THE SETTINGS PANELS (formerly src/Overlay/OverlayPanels.cpp), written against the panels' widgets (`ui`): ImGui
//! in the overlay, the recorder in the VR menu's pages. So far: the VR hands panel -- hand tracking and the diagnostic
//! dump, the body settings persisted through the live controls' snapshot (hands only, roomscale, the body rotation
//! mode, VRIK's cutscene suspension on foot and in a vehicle, the scene camera's smoothing, the free-look cones, the
//! body's free zone), the arm IK calibration kept two-way with the manager's (sliders follow the loaded or
//! auto-calibrated values whenever nothing is being dragged, edits applied at once and saved when released), the
//! T-pose auto-calibration, the camera mount and its trims.
use core::sync::atomic::{AtomicPtr, Ordering::*};
use std::sync::Mutex;

use cvr_crt::Arg;
use cvr_settings::live_controls_ui_state::LiveControlsUiState;

use crate::ui;

/// What the hands panel asks of the C++ half (OpenXRManager's calibration).
#[repr(C)]
pub struct HandsRuntime {
    set_vr_hand_tracking_mode: extern "C" fn(i32),
    request_vr_diag: extern "C" fn(),
    get_vr_hand_calib: extern "C" fn(*mut f32),
    set_vr_hand_calib: extern "C" fn(*const f32),
    get_calibration_state: extern "C" fn() -> i32,
    get_calibration_progress: extern "C" fn() -> f32,
    start_auto_calibration: extern "C" fn(f32),
    get_camera_offset: extern "C" fn(*mut f32),
    bake_camera_offset: extern "C" fn(),
    clear_camera_offset: extern "C" fn(),
    save_calibration: extern "C" fn(),
    load_calibration: extern "C" fn(),
    set_shoulder_anatomical: extern "C" fn(f32, f32, f32, f32, f32, f32),
}
// SAFETY: the table holds plain functions.
unsafe impl Sync for HandsRuntime {}
/// The table, built here: the manager's calibration is cvr-xr's (switches.rs, calibration.rs; formerly a C++ table of
/// OpenXRManager calls).
static HANDS_RT: HandsRuntime = HandsRuntime {
    set_vr_hand_tracking_mode: xr::set_vr_hand_tracking_mode,
    request_vr_diag: xr::request_vr_diag,
    get_vr_hand_calib: xr::get_vr_hand_calib,
    set_vr_hand_calib: xr::set_vr_hand_calib,
    get_calibration_state: xr::get_calibration_state,
    get_calibration_progress: xr::get_calibration_progress,
    start_auto_calibration: xr::start_auto_calibration,
    get_camera_offset: xr::get_camera_offset,
    bake_camera_offset: xr::bake_camera_offset,
    clear_camera_offset: xr::clear_camera_offset,
    save_calibration: xr::save_calibration,
    load_calibration: xr::load_calibration,
    set_shoulder_anatomical: xr::set_shoulder_anatomical,
};
fn hands_rt() -> &'static HandsRuntime {
    &HANDS_RT
}

/// OpenXRManager's calibration and switches, as its inline members did them (include/Runtimes/OpenXRManager.hpp,
/// src/Runtimes/OpenXRCalibration.cpp): the state is cvr-xr's.
pub(crate) mod xr {
    use core::sync::atomic::Ordering::Relaxed;
    use cvr_camera::anchor::AnchorVector;
    use cvr_settings::live_controls_ui_state::LiveControlsUiState;
    use cvr_xr::calibration::{
        cvr_xr_calibration, cvr_xr_load_calibration, cvr_xr_save_calibration, cvr_xr_start_auto_calibration,
        CalibrationShared,
    };
    use cvr_xr::switches::SWITCHES;

    extern "C" {
        /// cvr::camera::ReadNeckCameraMount (cvr-rig's rig.rs).
        fn cvr_vrik_read_neck_camera_mount(offset: *mut AnchorVector) -> bool;
        /// cvr-ffi's host.rs: the plugin's main log.
        fn cvr_log_write(text: *const core::ffi::c_char, len: usize);
    }
    fn cal() -> &'static CalibrationShared {
        // SAFETY: a static block, alive for the process.
        unsafe { &*cvr_xr_calibration() }
    }
    pub extern "C" fn set_vr_hand_tracking_mode(mode: i32) {
        SWITCHES.hand_tracking_mode.store(mode, Relaxed);
    }
    pub extern "C" fn request_vr_diag() {
        SWITCHES.log_diag_request.fetch_add(1, Relaxed);
    }
    pub extern "C" fn get_vr_hand_calib(out: *mut f32) {
        for (i, v) in cal().calib.iter().enumerate() {
            // SAFETY: the caller's 14 floats.
            unsafe { *out.add(i) = v.load() };
        }
    }
    /// SetVRHandCalib: the 14 values in the [33..47] slots' order, then valid.
    pub extern "C" fn set_vr_hand_calib(values: *const f32) {
        for (i, v) in cal().calib.iter().enumerate() {
            // SAFETY: the caller's 14 floats.
            v.store(unsafe { *values.add(i) });
        }
        cal().calib_valid.store(1, Relaxed);
    }
    pub extern "C" fn get_calibration_state() -> i32 {
        cal().calib_state.load(Relaxed)
    }
    pub extern "C" fn get_calibration_progress() -> f32 {
        cal().calib_progress.load()
    }
    pub extern "C" fn start_auto_calibration(seconds: f32) {
        cvr_xr_start_auto_calibration(seconds);
    }
    /// The effective offset: the neutral-neck mount plus the stored camera trim.
    pub extern "C" fn get_camera_offset(out: *mut f32) {
        let mut mount = AnchorVector::default();
        // SAFETY: a mount to fill.
        unsafe { cvr_vrik_read_neck_camera_mount(&mut mount) };
        let trim = &cal().cam_bake_offset;
        // SAFETY: the caller's three floats.
        unsafe {
            *out = mount.x + trim[0].load();
            *out.add(1) = mount.y + trim[1].load();
            *out.add(2) = mount.z + trim[2].load();
        }
    }
    pub extern "C" fn clear_camera_offset() {
        for v in &cal().cam_bake_offset {
            v.store(0.0);
        }
    }
    /// BakeCameraOffset: reset to the rig's neutral neck mount, never sample an animated stance.
    pub extern "C" fn bake_camera_offset() {
        clear_camera_offset();
        // SAFETY: plain numbers and flags, assigned whole by the getter.
        let mut controls: LiveControlsUiState = unsafe { core::mem::zeroed() };
        // SAFETY: a snapshot to fill, then applied and saved whole.
        unsafe {
            super::GetLiveControlsUiState(&mut controls);
            controls.xrHeadOffsetX = 0.0;
            controls.xrHeadOffsetY = 0.0;
            controls.xrHeadOffsetZ = 0.0;
            crate::live_controls_ui::SetLiveControlsUiState(&controls, 1);
        }
        cvr_xr_save_calibration();
        let line = "Camera mount reset: neutral neck + 12cm forward (a woman's body 10cm), reference eyes + 10cm up.\n";
        // SAFETY: the text and its length.
        unsafe { cvr_log_write(line.as_ptr().cast(), line.len()) };
    }
    pub extern "C" fn save_calibration() {
        cvr_xr_save_calibration();
    }
    pub extern "C" fn load_calibration() {
        cvr_xr_load_calibration();
    }
    /// SetShoulderAnatomical: the anatomical HMD -> shoulder offsets, then valid.
    pub extern "C" fn set_shoulder_anatomical(rx: f32, ry: f32, rz: f32, lx: f32, ly: f32, lz: f32) {
        for (v, x) in cal().calib_ext.iter().zip([rx, ry, rz, lx, ly, lz]) {
            v.store(x);
        }
        cal().calib_ext_valid.store(1, Relaxed);
    }
    /// GetSharedSlot: the shared slots' block the present keeps, 0 outside it.
    pub extern "C" fn shared_slot(i: i32) -> f32 {
        let p = SWITCHES.shared_hands.load(Relaxed);
        if p.is_null() || !(0..256).contains(&i) {
            return 0.0;
        }
        // SAFETY: the present's 256-slot block.
        unsafe { *p.add(i as usize) }
    }
    pub extern "C" fn set_weapon_aim_enable(enable: i32) {
        SWITCHES.weapon_aim_enable.store(enable, Relaxed);
    }
}

// The overlay's own state and the C++ half's services the panels' tables hand on.
#[allow(non_upper_case_globals)]
extern "C" {
    /// ImGuiOverlay.cpp's: the compact ADS telemetry panel's switch and place, the hand gizmos' switches and scale.
    #[link_name = "?g_showCompactAdsTelemetry@overlay@@3_NA"]
    static mut g_showCompactAdsTelemetry: bool;
    #[link_name = "?g_compactAdsTelemetryX@overlay@@3MA"]
    static mut g_compactAdsTelemetryX: f32;
    #[link_name = "?g_compactAdsTelemetryY@overlay@@3MA"]
    static mut g_compactAdsTelemetryY: f32;
    #[link_name = "?g_drawHandLocator@overlay@@3_NA"]
    static mut g_drawHandLocator: bool;
    #[link_name = "?g_drawHandProxy3D@overlay@@3_NA"]
    static mut g_drawHandProxy3D: bool;
    #[link_name = "?g_drawHandDebugAxes@overlay@@3_NA"]
    static mut g_drawHandDebugAxes: bool;
    #[link_name = "?g_handLocatorScale@overlay@@3MA"]
    static mut g_handLocatorScale: f32;
    /// VrCore.cpp's: the verbose channel, the vehicle's state.
    #[link_name = "?g_verboseLog@@3HC"]
    static mut g_verboseLog: i32;
    #[link_name = "?g_isDriving@@3U?$atomic@_N@std@@A"]
    static g_isDriving: core::sync::atomic::AtomicBool;
    #[link_name = "?g_vehicleState@@3U?$atomic@H@std@@A"]
    static g_vehicleState: core::sync::atomic::AtomicI32;
}
unsafe extern "C" {
    /// The game upscaler's CAS block (xr_manager_services.rs).
    safe fn cvr_cpp_blocks_cas() -> bool;
}

extern "C" {
    /// cvr-rig wheel_grab.rs: the vehicle driven is a motorcycle.
    fn cvr_anim_wheel_mounted_bike() -> bool;
    /// cvr-rig haptics.rs: a haptic event (1 the belt touch) for a hand mask (1 left, 2 right).
    fn cvr_haptics_play(event: i32, hand_mask: i32);
    fn GetLiveControlsUiState(out: *mut LiveControlsUiState);
    fn cvr_rotation_mode_from_flags(physical: i32, tracked: i32, hybrid: i32, head_led: i32) -> i32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_BodyYawFollowDeadDeg: f32;
    static CyberpunkVR_DebugBodyFollowOffsetDeg: f32;
    static CyberpunkVR_DebugBodyFollowErrDeg: f32;
}
macro_rules! g {
    ($name:ident) => {{
        // SAFETY: an export of the C++ half, alive for the process.
        #[allow(unused_unsafe)]
        let value = unsafe { core::ptr::read_volatile(&raw const $name) };
        value
    }};
}

/// The overlay's snapshot of the live controls, taken now.
fn ui_state() -> LiveControlsUiState {
    // SAFETY: plain numbers and flags; the C++ half assigns the whole snapshot.
    let mut st: LiveControlsUiState = unsafe { core::mem::zeroed() };
    // SAFETY: a snapshot to fill.
    unsafe { GetLiveControlsUiState(&mut st) };
    st
}
/// Applied and saved, as the panel saves.
fn persist(st: &LiveControlsUiState) {
    // SAFETY: a whole snapshot.
    unsafe { crate::live_controls_ui::SetLiveControlsUiState(st, 1) };
}

/// The baked wrist euler base (Runtimes/WristBase.hpp): the calibration's wrist sliders are offsets from it.
const WRIST_BASE: [f32; 6] = [0.0, -90.0, -21.0, -180.0, -90.0, -15.0];

/// The hands panel's own state (statics of the C++ panel): the tracking switch and the calibration sliders.
struct Hands {
    tracking: bool,
    scale_r: f32,
    scale_l: f32,
    height_r: f32,
    height_l: f32,
    swing_r: f32,
    swing_l: f32,
    pole_r: f32,
    pole_l: f32,
    w_rp: f32,
    w_ry: f32,
    w_rr: f32,
    w_lp: f32,
    w_ly: f32,
    w_lr: f32,
    cal_dirty: bool,
}
static HANDS: Mutex<Hands> = Mutex::new(Hands {
    tracking: true, // default ON -- the backend's hand tracking mode also starts at 4
    scale_r: 1.05,
    scale_l: 1.06,
    height_r: 0.0,
    height_l: 0.0,
    swing_r: 1.0,
    swing_l: 1.0,
    pole_r: 0.0,
    pole_l: 0.0,
    w_rp: 0.0,
    w_ry: 0.0,
    w_rr: 0.0,
    w_lp: 0.0,
    w_ly: 0.0,
    w_lr: 0.0,
    cal_dirty: false,
});

/// Combo index -> stored minimum tier: Never (-1), Tier 2+ (1), Tier 3+ (2), Tier 4+ (3), Tier 5 (4).
const TIER_VALUES: [i32; 5] = [-1, 1, 2, 3, 4];

/// One cutscene-suspension combo; `vehicle` picks the field.
fn scene_suspend_control(
    st: &mut LiveControlsUiState,
    vehicle: bool,
    label: &str,
    id: &str,
    labels: &[&core::ffi::CStr],
    fallback: i32,
    help: &core::ffi::CStr,
) {
    let tier = if vehicle {
        st.xrVehicleCutsceneSuspendTier
    } else {
        st.xrCutsceneSuspendTier
    };
    let mut idx = TIER_VALUES
        .iter()
        .position(|&v| v == tier)
        .map_or(fallback, |i| i as i32);
    if ui::combo_str(&format!("{label}{id}"), &mut idx, labels) {
        let value = TIER_VALUES[idx as usize];
        if vehicle {
            st.xrVehicleCutsceneSuspendTier = value;
        } else {
            st.xrCutsceneSuspendTier = value;
        }
        persist(st);
    }
    if ui::is_item_hovered() {
        ui::set_tooltip(c"%s", &[Arg::S(help.as_ptr())]);
    }
}

/// DrawVRHandsControls: everything that used to live only in the desktop CET window; values reach the arm IK through
/// the manager. The defaults mirror the plugin's baked calibration, so the rig behaves the same before anything is
/// touched.
#[no_mangle]
pub extern "C" fn cvr_draw_vr_hands_controls() {
    let rt = hands_rt();
    let mut h = HANDS.lock().unwrap_or_else(|p| p.into_inner());
    // Must be 4 = full-arm IK (what the CET "Start VR Tracking" button uses).
    if ui::checkbox(c"Start VR hand tracking", &mut h.tracking) {
        (rt.set_vr_hand_tracking_mode)(if h.tracking { 4 } else { 0 });
    }
    ui::same_line();
    if ui::button(c"Log VR Diag") {
        (rt.request_vr_diag)();
    }
    ui::tooltip(c"Dumps gizmo vs. bone poses to vrik_diag.txt (next to dxgi.dll)\nfor tuning the arm IK. Same as the CET 'Log VR Diag' button.");

    ui::separator();
    // One body-yaw policy; the compatible ini flags persisted together.
    {
        let mut st = ui_state();
        let mut hands_only = st.xrBodyHandsOnly != 0;
        if ui::checkbox(c"Hands only (hide the body)", &mut hands_only) {
            st.xrBodyHandsOnly = hands_only as i32;
            persist(&st);
        }
        ui::tooltip(c"Draw only your hands: the rest of the body and all clothing are hidden,\nthe arms are cut at the wrists. The tactical belt and the weapons\nhanging in their holsters stay visible.");
        // main's 0.2.4: the physical hands' switches (the VR menu has them too)
        let mut collision = st.xrHandCollision != 0;
        if ui::checkbox(c"Hand collision", &mut collision) {
            st.xrHandCollision = collision as i32;
            persist(&st);
        }
        ui::tooltip(c"Hands and arms stop at the body and at each other.\nEmpty hands also stop at walls and furniture (static world geometry).\nOnly the drawn arm stops; your real hand does not.");
        let mut collision_vehicle = st.xrHandCollisionVehicle != 0;
        if ui::checkbox(c"Hand collision in vehicles", &mut collision_vehicle) {
            st.xrHandCollisionVehicle = collision_vehicle as i32;
            persist(&st);
        }
        ui::tooltip(c"Seated, hands stop at the car's cabin, the body and each other.\nA hand on the wheel is never stopped.");
        let mut hand_push = st.xrHandPush != 0;
        if ui::checkbox(c"Hands push loose objects", &mut hand_push) {
            st.xrHandPush = hand_push as i32;
            persist(&st);
        }
        ui::tooltip(c"An empty hand clear of the body shoves bottles, crates and the like, and a hand pressed\nagainst a light prop knocks that one prop loose. Never in a vehicle.\nExperimental: turn it off if the player is ever thrown about.");
        let mut prop_grab = st.xrPropGrab != 0;
        if ui::checkbox(c"Pick up small objects", &mut prop_grab) {
            st.xrPropGrab = prop_grab as i32;
            persist(&st);
        }
        ui::tooltip(c"Squeeze the grip at a loose object to carry it, let go to drop or throw it. One hand lifts\nabout 6 kg, two about 30, and the other hand can take it over.\nExperimental: turn it off if the player is ever thrown about.");
        let mut debug_view = st.xrDebugView != 0;
        if ui::checkbox(c"Show collision shapes and rays", &mut debug_view) {
            st.xrDebugView = debug_view as i32;
            persist(&st);
        }
        ui::tooltip(c"For finding faults, and it costs frame rate. Stays off after a restart.\nBlue the hands, cyan the fingers, magenta people. From PhysX within 2 m: grey the world, yellow what moves.\nRays: green free, red up to what they hit. In a carried thing: orange the place taken, white the palm.");
        let mut carry_picture = st.xrCarryPicture != 0;
        if ui::checkbox(c"Carried objects stick to the hand", &mut carry_picture) {
            st.xrCarryPicture = carry_picture as i32;
            persist(&st);
        }
        ui::tooltip(c"Draws a carried thing on the hand in the same frame, while its body follows in the physics.\nAgainst a wall the picture goes back to the body. Off: the picture is on the body, and trails a swing.");
        let mut lead_ms = st.xrCarryLead * 1000.0;
        if ui::slider_float(c"Carried objects lead the hand (ms)", &mut lead_ms, 0.0, 100.0, c"%.0f") {
            st.xrCarryLead = lead_ms * 0.001;
            persist(&st);
        }
        ui::tooltip(c"Physics reaches the screen after the hand does. Raise it until a thing swung fast stops trailing the hand,\nlower it if it runs ahead when the hand stops.");
        let mut contact_cm = st.xrContactMargin * 100.0;
        if ui::slider_float(c"Distance kept from surfaces (cm)", &mut contact_cm, 0.0, 5.0, c"%.1f") {
            st.xrContactMargin = contact_cm * 0.01;
            persist(&st);
        }
        ui::tooltip(c"The game's collision is simpler and often smaller than what is drawn. More distance keeps\nhands and fingers out of the mesh, and makes them stop a little before they touch.");
        let mut roomscale = st.xrRoomscaleMovement != 0;
        if ui::checkbox(c"Roomscale movement", &mut roomscale) {
            st.xrRoomscaleMovement = roomscale as i32;
            persist(&st);
        }
        ui::tooltip(c"Move the player capsule with physical horizontal head movement.\nUses native collision/steps and preserves stick or WASD movement.\nSuspended in menus, vehicles and scripted movement.");
        // cvr::body::RotationMode: Off, HeadCone, Tracked, Hybrid, HeadLed.
        // SAFETY: Rust's own function under its C name.
        let mut mode = unsafe {
            cvr_rotation_mode_from_flags(
                st.xrPhysicalBodyRotation,
                st.xrTrackedBodyRotation,
                st.xrHybridBodyRotation,
                st.xrHeadLedBodyRotation,
            )
        };
        ui::set_next_item_width(410.0);
        if ui::combo(
            c"Body rotation",
            &mut mode,
            &[
                c"Stick / snap only",
                c"Head + free-look cone",
                c"HMD + controllers",
                c"Hybrid",
                c"Head-led (default)",
            ],
        ) {
            st.xrPhysicalBodyRotation = (mode == 1) as i32;
            st.xrTrackedBodyRotation = (mode == 2) as i32;
            st.xrHybridBodyRotation = (mode == 3) as i32;
            st.xrHeadLedBodyRotation = (mode == 4) as i32;
            persist(&st);
        }
        ui::tooltip(c"Head-led: the body stays while the head looks within 10 degrees, then turns\nto the head at 300 deg/s; looking down (past 30 degrees) it stays until 70 degrees. No controllers.\nHybrid: the head cone upright, HMD + controllers looking down or bent.\nHMD + controllers always disables the body cones. Missing tracking holds heading.");
        let tracked_body = mode == 2;
        let hybrid_body = mode == 3;
        let body_rot = mode != 0;
        // VRIK's cutscene suspension: each combo marks its own default (on foot Tier 3+, in a vehicle Tier 4+).
        scene_suspend_control(
            &mut st,
            false,
            "Suspend VRIK in cutscenes (on foot)",
            "##cutsceneSuspend",
            &[
                c"Never (VRIK always on)",
                c"Staged scenes and up (Tier 2+)",
                c"Tier 3 and up  [default]",
                c"Cinematics (Tier 4+)",
                c"Full cinematics only (Tier 5)",
            ],
            2,
            c"Applies only on foot. VRIK yields to the scene animation at or above this tier.\nThe vehicle threshold is configured separately below.",
        );
        scene_suspend_control(
            &mut st,
            true,
            "Suspend VRIK in cutscenes (vehicle)",
            "##cutsceneSuspendVehicle",
            &[
                c"Never (VRIK always on)",
                c"Staged scenes and up (Tier 2+)",
                c"Tier 3 and up",
                c"Cinematics (Tier 4+)  [default]",
                c"Full cinematics only (Tier 5)",
            ],
            3,
            c"Applies in vehicles, including passenger window combat.\nVRIK yields to the scene animation at or above this tier. Never keeps VRIK enabled.",
        );
        {
            let mut comfort = st.xrSceneCamComfortDeg;
            if ui::slider_float(
                c"Scene camera smoothing",
                &mut comfort,
                0.0,
                45.0,
                c"%.0f deg",
            ) {
                st.xrSceneCamComfortDeg = comfort;
                persist(&st);
            }
            ui::tooltip(c"While a scene drives the camera, its turning within this angle does not\nturn the view; past it the view eases after the scene camera. Cuts still switch at once.\n0 = the scene camera exactly as the game moves it. Default: 30 degrees.");
        }
        ui::begin_disabled(!body_rot || tracked_body);
        let mut cone = st.xrBodyFreeLookDeg;
        if ui::slider_float(c"Free-look cone", &mut cone, 0.0, 60.0, c"%.0f deg") {
            st.xrBodyFreeLookDeg = cone;
            persist(&st);
        }
        ui::tooltip(c"How far your head may turn before the body starts coming around.\nThe body settles toward the centre, then restores the full free-look zone.\nDefault: 10 degrees. Hybrid uses this value while looking ahead.");
        ui::end_disabled();
        ui::begin_disabled(mode != 1);
        let mut down_cone = st.xrBodyFreeLookDownDeg;
        if ui::slider_float(
            c"Free-look cone when looking down",
            &mut down_cone,
            0.0,
            90.0,
            c"%.0f deg",
        ) {
            st.xrBodyFreeLookDownDeg = down_cone;
            persist(&st);
        }
        ui::tooltip(c"Extra room to reach chest-mounted items.\nBlends in from 10 to 30 degrees of downward head tilt.\nNever narrows the normal cone; set the same value to disable widening.");
        let mut swim_cone = st.xrBodyFreeLookSwimDeg;
        if ui::slider_float(
            c"Free-look cone in water",
            &mut swim_cone,
            0.0,
            90.0,
            c"%.0f deg",
        ) {
            st.xrBodyFreeLookSwimDeg = swim_cone;
            persist(&st);
        }
        ui::tooltip(c"One fixed cone on the surface and underwater.\nLooking down and physical bending do not widen it. Default: 5 degrees.");
        ui::end_disabled();
        if tracked_body {
            ui::text_disabled(
                c"Body free-look cones are disabled in HMD + controllers mode.",
                &[],
            );
        }
        if hybrid_body {
            ui::text_disabled(
                c"Hybrid: head cone upright; HMD + controllers when looking down or bending.",
                &[],
            );
        }
        let mut radius_cm = st.xrBodyMoveRadius * 100.0;
        if ui::slider_float(
            c"Body movement free zone",
            &mut radius_cm,
            0.0,
            15.0,
            c"%.1f cm",
        ) {
            st.xrBodyMoveRadius = radius_cm * 0.01;
            persist(&st);
        }
        ui::tooltip(c"Small physical head movements leave the body in place.\nOutside this zone the body follows smoothly; camera tracking remains immediate.");
        if body_rot && !tracked_body && g!(CyberpunkVR_RuntimeDiagnostics) != 0 {
            ui::text(
                c"active cone %.1f deg | realign %+.1f deg | head-vs-body %+.1f deg",
                &[
                    Arg::F(g!(CyberpunkVR_BodyYawFollowDeadDeg) as f64),
                    Arg::F(g!(CyberpunkVR_DebugBodyFollowOffsetDeg) as f64),
                    Arg::F(g!(CyberpunkVR_DebugBodyFollowErrDeg) as f64),
                ],
            );
        }
    }

    ui::separator();
    ui::text_unformatted(c"Hand IK Calibration (per hand: R = right, L = left)");

    // TWO-WAY SYNC: the sliders follow the manager's live values (loaded from vrik_calibration.ini or auto-calibrated)
    // whenever nothing is being dragged -- one-way, the first touch pushed the defaults over the real calibration.
    let wb = WRIST_BASE;
    if !ui::is_any_item_active() {
        let mut c = [0.0f32; 14];
        (rt.get_vr_hand_calib)(c.as_mut_ptr());
        (h.scale_r, h.scale_l, h.height_r, h.height_l) = (c[0], c[1], c[2], c[3]);
        (h.swing_r, h.swing_l, h.pole_r, h.pole_l) = (c[4], c[5], c[6], c[7]);
        (h.w_rp, h.w_ry, h.w_rr) = (c[8] - wb[0], c[9] - wb[1], c[10] - wb[2]);
        (h.w_lp, h.w_ly, h.w_lr) = (c[11] - wb[3], c[12] - wb[4], c[13] - wb[5]);
    }

    let mut cal_changed = false;
    cal_changed |= ui::slider_float(c"Reach scale R", &mut h.scale_r, 0.80, 1.30, c"%.3f");
    cal_changed |= ui::slider_float(c"Reach scale L", &mut h.scale_l, 0.80, 1.30, c"%.3f");
    cal_changed |= ui::slider_float(c"Height R", &mut h.height_r, -0.20, 0.50, c"%.3f m");
    cal_changed |= ui::slider_float(c"Height L", &mut h.height_l, -0.20, 0.50, c"%.3f m");
    cal_changed |= ui::slider_float(c"Elbow swing R", &mut h.swing_r, -3.0, 3.0, c"%.2f");
    cal_changed |= ui::slider_float(c"Elbow swing L", &mut h.swing_l, -3.0, 3.0, c"%.2f");
    cal_changed |= ui::slider_float(c"Elbow pole R", &mut h.pole_r, -180.0, 180.0, c"%.1f deg");
    cal_changed |= ui::slider_float(c"Elbow pole L", &mut h.pole_l, -180.0, 180.0, c"%.1f deg");

    ui::separator();
    ui::text_unformatted(c"Wrist rotation offset (palm/finger alignment, deg)");
    cal_changed |= ui::slider_float(c"Wrist R pitch", &mut h.w_rp, -180.0, 180.0, c"%.1f");
    cal_changed |= ui::slider_float(c"Wrist R yaw", &mut h.w_ry, -180.0, 180.0, c"%.1f");
    cal_changed |= ui::slider_float(c"Wrist R roll", &mut h.w_rr, -180.0, 180.0, c"%.1f");
    cal_changed |= ui::slider_float(c"Wrist L pitch", &mut h.w_lp, -180.0, 180.0, c"%.1f");
    cal_changed |= ui::slider_float(c"Wrist L yaw", &mut h.w_ly, -180.0, 180.0, c"%.1f");
    cal_changed |= ui::slider_float(c"Wrist L roll", &mut h.w_lr, -180.0, 180.0, c"%.1f");

    ui::separator();
    // Auto-calibration: a T-pose sampled from the same controller poses that draw the gizmo hands.
    let c_state = (rt.get_calibration_state)();
    if c_state == 1 {
        let prog = (rt.get_calibration_progress)();
        ui::text_colored(
            [1.0, 0.9, 0.3, 1.0],
            c"AUTO-CALIBRATING: stretch arms STRAIGHT OUT to the sides,",
            &[],
        );
        ui::text_colored(
            [1.0, 0.9, 0.3, 1.0],
            c"stand straight facing forward. Hold for %.1fs (%.0f%%).",
            &[
                Arg::F(((1.0 - prog) * 4.0) as f64),
                Arg::F((prog * 100.0) as f64),
            ],
        );
        ui::progress_bar(prog, 280.0, 0.0);
    } else {
        if ui::button_sized(c"Start Auto-Calibration (T-pose, 4s)", 280.0, 0.0) {
            (rt.start_auto_calibration)(4.0);
        }
        ui::tooltip(c"Press once, then stretch BOTH gizmo hands OUT to the sides at shoulder height\nand stand straight facing forward. The mod measures the visible controller\npositions, computes shoulder pivots + arm scale, and saves the result\nto vrik_calibration.ini.");
        if c_state == 2 {
            ui::same_line();
            ui::text_colored([0.4, 1.0, 0.4, 1.0], c"Saved.", &[]);
        }
    }

    ui::separator();
    // Neutral rig alignment is automatic; this button only clears optional trims.
    {
        let mut cb = [0.0f32; 3];
        (rt.get_camera_offset)(cb.as_mut_ptr());
        ui::text_unformatted(c"Camera: neck + 12/10 cm forward, eyes + 10 cm up");
        if ui::button_sized(c"Reset camera to neck", 180.0, 0.0) {
            (rt.bake_camera_offset)();
        }
        ui::tooltip(c"Uses the neutral neck, 12 cm forward (10 for a woman's body) and 10 cm above reference eyes.\nResets camera trims; no stance, head direction or Bake measurement is needed.");
        ui::same_line();
        if ui::button_sized(c"Clear trim##cambake", 90.0, 0.0) {
            (rt.clear_camera_offset)();
            (rt.save_calibration)();
        }
        ui::text(
            c"camera mount: R %.3f  Fwd %.3f  Up %.3f",
            &[
                Arg::F(cb[0] as f64),
                Arg::F(cb[1] as f64),
                Arg::F(cb[2] as f64),
            ],
        );
    }

    ui::separator();
    let apply = ui::button(c"Apply Calibration");
    ui::same_line();
    let save = ui::button(c"Save");
    ui::same_line();
    let load = ui::button(c"Load");
    ui::same_line();
    if ui::button(c"Reset Defaults") {
        (h.scale_r, h.scale_l, h.height_r, h.height_l) = (1.05, 1.06, 0.0, 0.0);
        (h.swing_r, h.swing_l, h.pole_r, h.pole_l) = (1.0, 1.0, 0.0, 0.0);
        (h.w_rp, h.w_ry, h.w_rr, h.w_lp, h.w_ly, h.w_lr) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        (rt.set_shoulder_anatomical)(0.14, -0.17, 0.05, -0.14, -0.17, 0.05);
        cal_changed = true;
    }

    if cal_changed || apply {
        let calib = [
            h.scale_r,
            h.scale_l,
            h.height_r,
            h.height_l,
            h.swing_r,
            h.swing_l,
            h.pole_r,
            h.pole_l,
            wb[0] + h.w_rp,
            wb[1] + h.w_ry,
            wb[2] + h.w_rr,
            wb[3] + h.w_lp,
            wb[4] + h.w_ly,
            wb[5] + h.w_lr,
        ];
        (rt.set_vr_hand_calib)(calib.as_ptr());
    }
    // AUTOSAVE: edits persisted once the drag or press is released.
    if cal_changed {
        h.cal_dirty = true;
    }
    if h.cal_dirty && !ui::is_any_item_active() {
        (rt.save_calibration)();
        h.cal_dirty = false;
    }
    if save {
        (rt.save_calibration)();
    }
    if load {
        // The two-way sync above pulls the loaded values into the sliders next frame.
        (rt.load_calibration)();
    }
}

// ---- the stereo panel ----------------------------------------------------------------------------------------------

/// What the stereo panel keeps of the overlay's own (the compact ADS telemetry panel's switch and place).
#[repr(C)]
pub struct StereoRuntime {
    show_compact_ads_telemetry: *mut bool,
    compact_ads_telemetry_x: *mut f32,
    compact_ads_telemetry_y: *mut f32,
}
// SAFETY: the table holds addresses of the overlay's statics.
unsafe impl Sync for StereoRuntime {}
static STEREO_RT: StereoRuntime = StereoRuntime {
    show_compact_ads_telemetry: &raw mut g_showCompactAdsTelemetry,
    compact_ads_telemetry_x: &raw mut g_compactAdsTelemetryX,
    compact_ads_telemetry_y: &raw mut g_compactAdsTelemetryY,
};

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_StereoModuleLoaded: i32;
    static mut CyberpunkVR_StereoLog: i32;
    static mut CyberpunkVR_StereoSubmit: i32;
    static mut CyberpunkVR_StereoEyeMaxAgeMs: u32;
    static CyberpunkVR_DebugVrcamEyeAgeMs: u32;
    static CyberpunkVR_DebugStereoEyeSubmits: u64;
    static mut CyberpunkVR_StableCopy: i32;
    static mut CyberpunkVR_StableFromTonemap: i32;
    static CyberpunkVR_DebugStableCopies: u64;
    static CyberpunkVR_DebugStableSkips: u64;
    static CyberpunkVR_VrcamDlss: i32;
    static mut CyberpunkVR_ForceVrcamCam: u32;
    static CyberpunkVR_VrcamEnabled: u32;
    static mut CyberpunkVR_MirrorOutput: u32;
    static CyberpunkVR_DebugVrcamNodeHits: u64;
    static CyberpunkVR_DebugMirrorRtvHits: u64;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_DebugMainProjYY: f32;
    static CyberpunkVR_DebugMainCamFov: f32;
    static CyberpunkVR_MainAdsZoomFactor: f32;
    static CyberpunkVR_DebugVrcamWantFov: f32;
    static CyberpunkVR_DebugVrcamBaseFov: f32;
    static mut CyberpunkVR_ProfEnable: i32;
    static CyberpunkVR_ProfFrameMs: f64;
    static CyberpunkVR_ProfDispMainMs: f64;
    static CyberpunkVR_ProfDispVrcamMs: f64;
    static CyberpunkVR_ProfDispMainNodes: u32;
    static CyberpunkVR_ProfDispVrcamNodes: u32;
    static CyberpunkVR_DebugViewKeyMainNodes: u64;
    static CyberpunkVR_DebugViewKeyOtherNodes: u64;
    fn CyberpunkVR_SetVrcamEnabled(on: u32);
    fn CyberpunkVR_VrcamComponentName() -> *const core::ffi::c_char;
    fn CyberpunkVR_VrcamCameraName() -> *const core::ffi::c_char;
    fn CyberpunkVR_ProfDumpNodes();
    fn CyberpunkVR_ProfSnapshotNodes(
        rva: *mut u32,
        msv: *mut f64,
        msm: *mut f64,
        cv: *mut u32,
        cm: *mut u32,
        maxn: i32,
    ) -> i32;
    fn CyberpunkVR_ProfNodeName(rva: u32) -> *const core::ffi::c_char;
}
macro_rules! s {
    ($name:ident, $value:expr) => {{
        let value = $value;
        // SAFETY: as g!.
        #[allow(unused_unsafe)]
        unsafe {
            core::ptr::write_volatile(&raw mut $name, value)
        }
    }};
}
fn diagnostics() -> bool {
    g!(CyberpunkVR_RuntimeDiagnostics) != 0
}

/// DrawStereoControls: three questions reported separately, because each fails on its own -- is the second view
/// rendering, did a colour frame come from it, did it reach the headset; then the switches.
#[no_mangle]
pub extern "C" fn cvr_draw_stereo_controls() {
    if g!(CyberpunkVR_StereoModuleLoaded) == 0 {
        ui::text_colored([1.0, 0.55, 0.35, 1.0], c"Stereo module not installed.", &[]);
        ui::text_wrapped(
            c"The engine hooks did not load, so everything below is inert and the headset is on the plain mono path. Either CyberpunkVR_StereoModuleEnable is 0, or bin\\x64\\vrport_nostereo.txt exists. See the \"Stereo:\" line in cyberpunkvrport.log.",
            &[],
        );
        return;
    }
    let eye_age = g!(CyberpunkVR_DebugVrcamEyeAgeMs);
    let eye_ever_produced = eye_age != 0xFFFF_FFFF;
    let eye_fresh = eye_ever_produced && eye_age <= g!(CyberpunkVR_StereoEyeMaxAgeMs);
    let submit_on = g!(CyberpunkVR_StereoSubmit) != 0;
    // Which eye VRCAM lands in is CyberpunkVR_MainIsRightEye's to decide, so the wording is derived.
    let (vrcam_eye, main_eye) = if g!(CyberpunkVR_MainIsRightEye) != 0 {
        (c"LEFT", c"RIGHT")
    } else {
        (c"RIGHT", c"LEFT")
    };

    if submit_on && eye_fresh {
        ui::text_colored(
            [0.45, 0.9, 0.5, 1.0],
            c"Stereo active  (%s = VRCAM, %s = MAIN)",
            &[Arg::S(vrcam_eye.as_ptr()), Arg::S(main_eye.as_ptr())],
        );
    } else if submit_on {
        ui::text_colored(
            [1.0, 0.8, 0.35, 1.0],
            c"No second eye right now -> both eyes get MAIN (mono)",
            &[],
        );
        ui::text_disabled(c"Expected in menus and while loading. If it stays here in gameplay: the VRCAM component is off, or vrcam.json names a camera the player entity does not carry.", &[]);
    } else {
        ui::text_disabled(c"Stereo submit off -> both eyes get MAIN (mono)", &[]);
    }
    if diagnostics() && eye_ever_produced {
        ui::text_disabled(
            c"vrcam eye age %u ms  (stale over %u)   eye submits %llu",
            &[
                Arg::U(eye_age as u64),
                Arg::U(g!(CyberpunkVR_StereoEyeMaxAgeMs) as u64),
                Arg::U(g!(CyberpunkVR_DebugStereoEyeSubmits)),
            ],
        );
    } else if diagnostics() {
        ui::text_disabled(c"vrcam eye: never produced a frame", &[]);
    }
    ui::separator();

    let mut submit = submit_on;
    let submit_label = cvr_crt::format(c"Send VRCAM to the %s EYE", &[Arg::S(vrcam_eye.as_ptr())]);
    if ui::checkbox(&submit_label, &mut submit) {
        s!(CyberpunkVR_StereoSubmit, submit as i32);
    }
    if ui::is_item_hovered() {
        ui::set_tooltip(
            c"Off = the old behaviour, MAIN duplicated into both eyes.\nOn = the %s eye gets the VRCAM view's own final colour --\nthe same image the desktop mirror shows -- re-encoded to the\nswapchain's sRGB format and scaled to the eye size.",
            &[Arg::S(vrcam_eye.as_ptr())],
        );
    }
    // No separate capture switch: the submit path drives the eye capture from the checkbox above.
    if diagnostics() {
        ui::text_disabled(
            c"snapshot copies %llu   skips %llu   vrcam RTV hits %llu",
            &[
                Arg::U(g!(CyberpunkVR_DebugStableCopies)),
                Arg::U(g!(CyberpunkVR_DebugStableSkips)),
                Arg::U(g!(CyberpunkVR_DebugMirrorRtvHits)),
            ],
        );
    }

    let mut max_age = g!(CyberpunkVR_StereoEyeMaxAgeMs) as i32;
    if ui::slider_int(c"Eye staleness limit (ms)", &mut max_age, 33, 1000) {
        s!(CyberpunkVR_StereoEyeMaxAgeMs, max_age as u32);
    }
    if ui::is_item_hovered() {
        ui::set_tooltip(
            c"How long the last VRCAM frame stays usable. Past this the %s\neye falls back to MAIN -- one eye frozen while the other moves\nis far worse to look at than mono.",
            &[Arg::S(vrcam_eye.as_ptr())],
        );
    }

    ui::separator();
    // Read-only on purpose: VRCAM's upscaler mirrors MAIN's, decided at graph build from MAIN's own flags.
    ui::text_disabled(
        c"VRCAM DLSS: %s  (follows MAIN's upscaler, no switch)",
        &[Arg::S(if g!(CyberpunkVR_VrcamDlss) != 0 {
            c"ON".as_ptr()
        } else {
            c"off".as_ptr()
        })],
    );
    ui::tooltip(c"On whenever MAIN is upscaling with DLSS: vrcam gets its own\nStreamline viewport and renders below its target.\nOff for DLAA / TAA / no upscaler. Change it in the game's\ngraphics settings, not here.");
    let mut force_cam = g!(CyberpunkVR_ForceVrcamCam) != 0;
    if ui::checkbox(
        c"Match VRCAM projection to MAIN  (fov / zoom / near / far)",
        &mut force_cam,
    ) {
        s!(CyberpunkVR_ForceVrcamCam, force_cam as u32);
    }

    ui::separator();
    // The RTT component's isEnabled through the game's RTTI: off stops the engine rendering the second view at all.
    let mut vrcam_on = g!(CyberpunkVR_VrcamEnabled) != 0;
    if ui::checkbox(
        c"VRCAM component  (RTTI Toggle: force ON/OFF)",
        &mut vrcam_on,
    ) {
        // SAFETY: the C++ half's switch.
        unsafe { CyberpunkVR_SetVrcamEnabled(vrcam_on as u32) };
    }
    // SAFETY: names kept for the process.
    let (component, camera) = unsafe {
        (
            CyberpunkVR_VrcamComponentName(),
            CyberpunkVR_VrcamCameraName(),
        )
    };
    ui::text_disabled(
        c"component %s   camera %s",
        &[Arg::S(component), Arg::S(camera)],
    );
    if diagnostics() {
        ui::text_disabled(
            c"view nodes: main %llu   other %llu   vrcam %llu",
            &[
                Arg::U(g!(CyberpunkVR_DebugViewKeyMainNodes)),
                Arg::U(g!(CyberpunkVR_DebugViewKeyOtherNodes)),
                Arg::U(g!(CyberpunkVR_DebugVrcamNodeHits)),
            ],
        );
    }

    // A second swapchain and window mirroring the VRCAM eye (for OBS / desktop preview); a copy per frame.
    let mut mirror_on = g!(CyberpunkVR_MirrorOutput) != 0;
    if ui::checkbox(
        c"VRCAM Mirror  (separate window, for capture)",
        &mut mirror_on,
    ) {
        s!(CyberpunkVR_MirrorOutput, mirror_on as u32);
    }

    // Weapon ADS is not a toggle: the vrcam eye always follows MAIN's vertical fov, narrowed by the aim zoom.
    ui::text_disabled(
        c"vrcam fov %.2f  (asset %.2f)   main yy %.5f  fov %.2f   ADS x%.3f",
        &[
            Arg::F(g!(CyberpunkVR_DebugVrcamWantFov) as f64),
            Arg::F(g!(CyberpunkVR_DebugVrcamBaseFov) as f64),
            Arg::F(g!(CyberpunkVR_DebugMainProjYY) as f64),
            Arg::F(g!(CyberpunkVR_DebugMainCamFov) as f64),
            Arg::F(g!(CyberpunkVR_MainAdsZoomFactor) as f64),
        ],
    );

    if ui::collapsing_header(c"Diagnostics", 0) {
        // SAFETY: attached at the C++ half's static initialisation; the overlay's own globals.
        let r = &STEREO_RT;
        // SAFETY: as above.
        unsafe {
            ui::checkbox(
                c"Compact ADS camera telemetry (in-headset)",
                &mut *r.show_compact_ads_telemetry,
            );
        }
        ui::tooltip(c"A small panel that stays up after F10 is closed, reporting what the ENGINE did to the camera when the sights came up:\ndelta and peak in centimetres, right/forward/up in the heading's own basis, against a baseline captured while hip firing.\nStereo submission is unaffected.");
        // SAFETY: as above.
        if unsafe { *r.show_compact_ads_telemetry } {
            ui::indent();
            // SAFETY: as above.
            unsafe {
                ui::slider_float(
                    c"Telemetry X",
                    &mut *r.compact_ads_telemetry_x,
                    0.10,
                    0.90,
                    c"%.2f",
                );
                ui::slider_float(
                    c"Telemetry Y",
                    &mut *r.compact_ads_telemetry_y,
                    0.10,
                    0.90,
                    c"%.2f",
                );
            }
            ui::text_disabled(c"Normalised position in the eye image", &[]);
            ui::unindent();
        }
        ui::separator();

        let mut slog = g!(CyberpunkVR_StereoLog) != 0;
        if ui::checkbox(c"Stereo logging -> cyberpunkvrport.log", &mut slog) {
            s!(CyberpunkVR_StereoLog, slog as i32);
        }
        let mut stable = g!(CyberpunkVR_StableCopy) != 0;
        if ui::checkbox(c"Committed snapshot of the VRCAM final", &mut stable) {
            s!(CyberpunkVR_StableCopy, stable as i32);
        }
        ui::tooltip(c"Off = read the engine's transient directly. That target is a\nframe-graph allocation which a later pass aliases, which is what\nmade the image alternate bright/dark. Leave on.");
        let mut from_tonemap = g!(CyberpunkVR_StableFromTonemap) != 0;
        if ui::checkbox(
            c"Snapshot at the tonemap node instead of RenderFinal2D",
            &mut from_tonemap,
        ) {
            s!(CyberpunkVR_StableFromTonemap, from_tonemap as i32);
        }

        let mut prof = g!(CyberpunkVR_ProfEnable) != 0;
        if ui::checkbox(c"Node CPU profiler  (per-node self+incl ms)", &mut prof) {
            s!(CyberpunkVR_ProfEnable, prof as i32);
        }
        ui::text_disabled(
            c"frame %.2f ms   dispatch: main %.2f (%u)  vrcam %.2f (%u)",
            &[
                Arg::F(g!(CyberpunkVR_ProfFrameMs)),
                Arg::F(g!(CyberpunkVR_ProfDispMainMs)),
                Arg::U(g!(CyberpunkVR_ProfDispMainNodes) as u64),
                Arg::F(g!(CyberpunkVR_ProfDispVrcamMs)),
                Arg::U(g!(CyberpunkVR_ProfDispVrcamNodes) as u64),
            ],
        );
        if ui::button(c"Dump node audit -> log  (resets window)") {
            // SAFETY: the C++ half's dump.
            unsafe { CyberpunkVR_ProfDumpNodes() };
        }
        if prof {
            // Top 15 by SELF time: inclusive double-counts, SceneDrv contains every scene pass it dispatches.
            let (mut rva, mut msv, mut msm, mut cv, mut cm) =
                ([0u32; 15], [0f64; 15], [0f64; 15], [0u32; 15], [0u32; 15]);
            // SAFETY: arrays of 15.
            let n = unsafe {
                CyberpunkVR_ProfSnapshotNodes(
                    rva.as_mut_ptr(),
                    msv.as_mut_ptr(),
                    msm.as_mut_ptr(),
                    cv.as_mut_ptr(),
                    cm.as_mut_ptr(),
                    15,
                )
            };
            ui::text_disabled(c"top %d by self ms/frame   (main | vrcam)", &[Arg::I(n)]);
            for i in 0..n.clamp(0, 15) as usize {
                // SAFETY: a name kept for the process, or null.
                let nm = unsafe { CyberpunkVR_ProfNodeName(rva[i]) };
                // SAFETY: as above.
                let named = !nm.is_null() && unsafe { *nm } != 0;
                ui::text(
                    c"%-30s %6.3f | %6.3f",
                    &[
                        Arg::S(if named { nm } else { c"?".as_ptr() }),
                        Arg::F(msm[i]),
                        Arg::F(msv[i]),
                    ],
                );
            }
        }
    }
}

// ---- the driving panel ---------------------------------------------------------------------------------------------

/// What the driving panel reads of the C++ half (the live state under diagnostics, the seat).
#[repr(C)]
pub struct DrivingRuntime {
    /// OpenXRManager::GetSharedSlot.
    shared_slot: extern "C" fn(i32) -> f32,
    is_driving: *const core::sync::atomic::AtomicBool,
    vehicle_state: *const core::sync::atomic::AtomicI32,
}
// SAFETY: the table holds a plain function and addresses of statics.
unsafe impl Sync for DrivingRuntime {}
static DRIVING_RT: DrivingRuntime = DrivingRuntime {
    shared_slot: xr::shared_slot,
    is_driving: &raw const g_isDriving,
    vehicle_state: &raw const g_vehicleState,
};

#[allow(non_upper_case_globals)]
extern "C" {
    static g_wheelBlendRight: core::sync::atomic::AtomicU32;
    static g_wheelBlendLeft: core::sync::atomic::AtomicU32;
    static g_wheelSteer: core::sync::atomic::AtomicU32;
    static g_wheelSteerDeg: core::sync::atomic::AtomicU32;
    static g_wheelHornMask: core::sync::atomic::AtomicI32;
    static g_isInVehicle: bool;
}
/// vrshared::kWheelArmedMask and its bits.
const WHEEL_ARMED_MASK_SLOT: i32 = 163;
const WHEEL_ARMED_RIGHT_BIT: i32 = 1;
const WHEEL_ARMED_LEFT_BIT: i32 = 2;
/// RED4ext::game::PSMVehicle::Combat: a passenger's window combat (cvr::anim::IsPassengerWindowCombat).
const PSM_VEHICLE_COMBAT: i32 = 2;

/// CheckboxInt.
fn checkbox_int(label: &core::ffi::CStr, value: &mut i32) -> bool {
    let mut checked = *value != 0;
    let changed = ui::checkbox(label, &mut checked);
    if changed {
        *value = checked as i32;
    }
    changed
}
fn atomic_f32(a: &core::sync::atomic::AtomicU32) -> f32 {
    f32::from_bits(a.load(core::sync::atomic::Ordering::Relaxed))
}

/// DrawDrivingControls: the hands on the wheel, the horn on its hub, the gun in the driver's seat, the seated camera
/// offset.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_driving_controls(state: *mut LiveControlsUiState) -> bool {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return false;
    };
    let mut changed = false;
    ui::separator();
    ui::text_unformatted(c"Driving -- hands on the wheel");
    changed |= checkbox_int(c"Grab the wheel with the grips", &mut st.xrWheelGrab);
    ui::tooltip(c"While DRIVING, bring a hand to where the driving animation holds the\nwheel (or the handlebars) and squeeze that grip: the arm is handed back\nto the game's own animation -- hand on the wheel, fingers wrapped around\nit -- instead of following the controller. Release the grip and it goes\nback to your hand.\n\nEach hand is independent: hold the wheel with one and keep the other on\na gun. While a hand is at the wheel that grip does nothing else (no\nholster equip, no magazine grab).");
    changed |= checkbox_int(c"Hold a car's wheel anywhere on the rim", &mut st.xrWheelAnywhere);
    ui::tooltip(c"Your hand stays where you took hold and slides round the rim with your own hand, so\nyou can steer one-handed from the top and change grip in a hard turn.\nOff: a grab puts the hand where the driving animation holds it. Bikes always do.");
    {
        let mut r = if st.xrWheelRadius > 0.0 {
            st.xrWheelRadius
        } else {
            0.28
        };
        if ui::slider_float(c"Grab radius (m)", &mut r, 0.08, 0.60, c"%.2f") {
            st.xrWheelRadius = r;
            changed = true;
        }
        ui::tooltip(c"How close your hand has to be to the animated hand before the\ngrip counts as grabbing the wheel. Bigger = easier to catch a\nwheel you cannot see; too big and every grip in a car grabs.");
        use cvr_settings::wheel_range::{bike_bar_lock_degrees, bike_body_lean_share, BIKE_BAR_LOCK_MAX, BIKE_BAR_LOCK_MIN};
        let mut m = bike_bar_lock_degrees(st.xrBikeBarLockDeg);
        if ui::slider_float(c"Bike full lock at (deg)", &mut m, BIKE_BAR_LOCK_MIN, BIKE_BAR_LOCK_MAX, c"%.0f") {
            st.xrBikeBarLockDeg = m;
            changed = true;
        }
        ui::tooltip(c"Motorcycle handlebars: push one hand forward and pull the other back.\nThis is how far the bars turn, seen from above, to reach full lock. Lower = more sensitive.\nTwo hands use the line between them; one hand turns about a point half the bars' width to its side.\nAdding or releasing a hand keeps the current turn.");
        let mut lean = bike_body_lean_share(st.xrBikeBodyLean);
        if ui::slider_float(c"Bike body lean", &mut lean, 0.0, 1.0, c"%.2f") {
            st.xrBikeBodyLean = lean;
            changed = true;
        }
        ui::tooltip(c"Lean your body sideways to steer a motorcycle, added to your arms while holding the bars.\nThe value is the share of full lock from a 15 cm lean; 0 turns it off.\nCentre is where your body was when you grabbed the bars.");
        let mut d = if st.xrWheelSteerDeadDeg >= 0.0 {
            st.xrWheelSteerDeadDeg
        } else {
            0.0
        };
        if ui::slider_float(c"Steering deadzone (deg)", &mut d, 0.0, 20.0, c"%.1f") {
            st.xrWheelSteerDeadDeg = d;
            changed = true;
        }
        ui::tooltip(c"Tilt around centre that steers nothing at all.\nRaise it if the car drifts while you hold the wheel straight;\nevery degree here is a degree of dead wheel off centre.\nThe full range still ends at 'Full lock at', so widening the\ndeadzone does not make the steering jump.");

        changed |= checkbox_int(c"Steering prediction", &mut st.xrWheelPrediction);
        ui::tooltip(c"Gently anticipate a continuing turn from recent hand movement.\nLimited to 8 ms and 0.5 degrees; neutral stays unchanged.\nPrediction stops on a pause, reversal or grip change.\nOff by default. It cannot anticipate the start of a turn.");
        if st.xrWheelPrediction != 0 {
            changed |= ui::slider_float(
                c"Prediction horizon (ms)",
                &mut st.xrWheelPredictionMs,
                0.0,
                8.0,
                c"%.1f",
            );
        }
        changed |= checkbox_int(c"Horn -- hand on the wheel hub", &mut st.xrWheelHorn);
        ui::tooltip(c"While DRIVING, put a hand on the MIDDLE of the wheel -- where you would\nslap a real horn -- and the car honks for as long as it stays there.\nNo grip needed; a hand that is GRABBING the wheel never honks.");
        let mut hr = if st.xrWheelHornRadius > 0.0 {
            st.xrWheelHornRadius
        } else {
            0.12
        };
        if ui::slider_float(c"Horn hub radius (m)", &mut hr, 0.04, 0.30, c"%.2f") {
            st.xrWheelHornRadius = hr;
            changed = true;
        }
        ui::tooltip(c"How near the wheel centre the hand counts as on the hub.\nBigger = easier to find the horn without seeing it; too big\nand it reaches the rim, so every grab honks.");

        ui::spacing();
        changed |= checkbox_int(
            c"Trigger fires the gun while driving",
            &mut st.xrVehicleGunTrigger,
        );
        // main's fc750f1ed: the latched throttle and its trim slider gone, the left stick drives while the gun is out
        ui::tooltip(c"Draw a weapon in the driver seat and the RIGHT TRIGGER stops being the\nthrottle and becomes the gun: drive with the left hand on the wheel and\nshoot with the right.\n\nThe LEFT STICK drives meanwhile: forward = throttle, back = brake and\nreverse, sideways = steering. With a hand on the wheel the wheel steers\nand the stick only goes forward and back. Holster the weapon and the\ntrigger is the throttle again.\n\nWhile the weapon is out the left stick's forward/back is the throttle:\nno lean / rock until you holster.");

        // LIVE STATE: the armed mask raised by proximity, the blends by the grab itself, the angle the stick is driven
        // from -- so "it did not grab" and "it steers the wrong way" are answerable without a log.
        if diagnostics() {
            // SAFETY: attached at the C++ half's static initialisation; its atomics and the Rust half's under C names.
            unsafe {
                let r = &DRIVING_RT;
                let mask = (r.shared_slot)(WHEEL_ARMED_MASK_SLOT) as i32;
                let horn = g_wheelHornMask.load(core::sync::atomic::Ordering::Relaxed);
                ui::text(
                    c"driving %d   at wheel  L %d R %d   hold  L %.2f R %.2f",
                    &[
                        Arg::I((*r.is_driving).load(core::sync::atomic::Ordering::Relaxed) as i32),
                        Arg::I((mask & WHEEL_ARMED_LEFT_BIT != 0) as i32),
                        Arg::I((mask & WHEEL_ARMED_RIGHT_BIT != 0) as i32),
                        Arg::F(atomic_f32(&g_wheelBlendLeft) as f64),
                        Arg::F(atomic_f32(&g_wheelBlendRight) as f64),
                    ],
                );
                ui::text(
                    c"steer  %+.1f deg  ->  stick %+.2f     horn  L %d R %d",
                    &[
                        Arg::F(atomic_f32(&g_wheelSteerDeg) as f64),
                        Arg::F(atomic_f32(&g_wheelSteer) as f64),
                        Arg::I((horn & WHEEL_ARMED_LEFT_BIT != 0) as i32),
                        Arg::I((horn & WHEEL_ARMED_RIGHT_BIT != 0) as i32),
                    ],
                );
            }
        }
    }

    ui::separator();
    ui::text_unformatted(c"SEATED CAMERA OFFSET");
    ui::tooltip(c"Head offsets in GENERAL are a standing calibration. Seated, the game's own\nvehicle camera is already where it should be -- which is why the port drops\nits two automatic bakes in a vehicle -- so a standing offset carries the view\noff the seat instead of correcting it.\n\nThese three are added to the Head sliders while you are in a vehicle and\nignored the moment you step out, so the car and the street can be tuned\nseparately. Zero = the car keeps exactly the offset it has today.");
    // SAFETY: attached at the C++ half's static initialisation; its atomic and its C-linkage bool.
    let (in_vehicle, vehicle_state) = unsafe {
        let r = &DRIVING_RT;
        (
            core::ptr::read_volatile(&raw const g_isInVehicle),
            (*r.vehicle_state).load(core::sync::atomic::Ordering::Relaxed),
        )
    };
    let passenger_combat = in_vehicle && vehicle_state == PSM_VEHICLE_COMBAT;
    ui::begin_disabled(passenger_combat);
    changed |= ui::slider_float(
        c"Car Head X right",
        &mut st.xrVehHeadOffsetX,
        -0.50,
        0.50,
        c"%.3f m",
    );
    changed |= ui::slider_float(
        c"Car Head Y forward",
        &mut st.xrVehHeadOffsetY,
        -0.50,
        0.50,
        c"%.3f m",
    );
    changed |= ui::slider_float(
        c"Car Head Z up",
        &mut st.xrVehHeadOffsetZ,
        -0.50,
        0.50,
        c"%.3f m",
    );
    ui::end_disabled();
    // SAFETY: cvr-rig's flag under its C name.
    let on_bike = in_vehicle && unsafe { cvr_anim_wheel_mounted_bike() };
    // Live, so a slider that is doing nothing says so instead of being blamed.
    ui::text_disabled(
        if passenger_combat {
            c"   (window combat: use GENERAL > Head offsets)"
        } else if on_bike {
            c"   (on a motorcycle: these are ignored)"
        } else if in_vehicle {
            c"   (in a vehicle: these are live)"
        } else {
            c"   (on foot: these are ignored)"
        },
        &[],
    );
    // A motorcycle's own three, used instead of the car's while one is ridden (main's 0.2.4).
    changed |= ui::slider_float(c"Bike Head X right", &mut st.xrBikeHeadOffsetX, -0.50, 0.50, c"%.3f m");
    ui::tooltip(c"A motorcycle's own seated offset, used instead of the Car Head sliders\nwhile you ride one and ignored everywhere else. Zero = no offset.");
    changed |= ui::slider_float(c"Bike Head Y forward", &mut st.xrBikeHeadOffsetY, -0.50, 0.50, c"%.3f m");
    changed |= ui::slider_float(c"Bike Head Z up", &mut st.xrBikeHeadOffsetZ, -0.50, 0.50, c"%.3f m");
    ui::text_disabled(
        if on_bike {
            c"   (on a motorcycle: these are live)"
        } else {
            c"   (not on a motorcycle: these are ignored)"
        },
        &[],
    );
    changed
}

// ---- the FOV control and the FPS overlay panel ------------------------------------------------------------------

/// cvr::framegen::Settings (include/Framegen/Framegen.hpp), as MSVC lays it out inside the snapshot.
#[repr(C)]
pub struct FramegenSettings {
    pub enabled: bool,
    pub overlay: bool,
    pub auto_pace: bool,
    /// The switches (cvr_gpu::framegen_core::FSR3_NO_OPTICAL_FLOW, FSR3_HIDE, CADENCE_LOCK).
    pub flags: u8,
    pub backend: i32,
    pub quality: i32,
    pub flow_scale: i32,
    pub overlay_corner: i32,
    pub overlay_layout: i32,
    pub overlay_follow: i32,
    pub history_seconds: i32,
    pub overlay_cone: f32,
    pub overlay_scale: f32,
    pub overlay_distance: f32,
    pub overlay_x: f32,
    pub overlay_y: f32,
}
const _: () = {
    assert!(core::mem::size_of::<FramegenSettings>() == 52);
    assert!(core::mem::offset_of!(FramegenSettings, backend) == 4);
    assert!(core::mem::offset_of!(FramegenSettings, overlay_corner) == 16);
    assert!(core::mem::offset_of!(FramegenSettings, history_seconds) == 28);
    assert!(core::mem::offset_of!(FramegenSettings, overlay_y) == 48);
};
fn framegen_settings(st: &mut LiveControlsUiState) -> &mut FramegenSettings {
    // SAFETY: the snapshot's framegen member is a cvr::framegen::Settings of this layout (52 bytes, 4-aligned).
    unsafe { &mut *(st.framegen.as_mut_ptr() as *mut FramegenSettings) }
}

/// cvr::framegen::GetStatistics, the fields the panel shows, flattened by the C++ half.
#[repr(C)]
pub struct FpsStats {
    real_fps: f64,
    generated_fps: f64,
    output_fps: f64,
    average_fps: f64,
    minimum_fps: f64,
    maximum_fps: f64,
    low1_fps: f64,
    low01_fps: f64,
    budget_ms: f64,
    cpu_ms: f64,
    cpu_peak_ms: f64,
    gpu_ms: f64,
    gpu_peak_ms: f64,
    generation_ms: f64,
    cpu_usage: f64,
    core_maximum: f64,
    gpu_usage: f64,
    gpu_temperature: f64,
    vram_bytes: u64,
    skipped: u64,
    ram_used: u64,
    ram_total: u64,
    app_working_set: u64,
    gpu_memory_used: u64,
    gpu_memory_total: u64,
    gpu_process_used: u64,
    gpu_process_budget: u64,
    cpu_graph: *const f32,
    gpu_graph: *const f32,
    graph_count: i32,
    gpu_valid: bool,
    inputs_ready: bool,
    gpu_memory_valid: bool,
    gpu_process_memory_valid: bool,
    cpu_name: *const core::ffi::c_char,
    gpu_name: *const core::ffi::c_char,
}

/// What these panels ask of the C++ half.
#[repr(C)]
pub struct FpsRuntime {
    is_runtime_psvr2: extern "C" fn() -> bool,
    reset_statistics: extern "C" fn(),
    /// The statistics into `out`; the graphs and names stay valid until the next call (render thread).
    statistics: extern "C" fn(*mut FpsStats),
}
/// The FPS panel's table, built here (formerly src/Overlay/OverlayPanels.cpp's).
mod fps_table {
    use super::{FpsRuntime, FpsStats};
    use cvr_gpu::framegen_core::{cvr_framegen_reset_statistics, cvr_framegen_statistics, Statistics, GRAPH_POINTS};
    unsafe extern "C" {
        /// OpenXRManager::IsRuntimePsvr2 (src/Overlay/OverlayPanels.cpp).
        safe fn cvr_cpp_xr_runtime_psvr2() -> bool;
    }
    /// The last statistics (render thread): the graphs and names the panel reads stay valid until the next call.
    static mut STATS: Statistics = unsafe { core::mem::zeroed() };
    extern "C" fn statistics(o: *mut FpsStats) {
        // SAFETY: the render thread's statistics and the caller's output.
        unsafe {
            let s = &mut *(&raw mut STATS);
            cvr_framegen_statistics(s);
            let hw = &s.hardware;
            *o = FpsStats {
                real_fps: s.real_fps, generated_fps: s.generated_fps, output_fps: s.output_fps,
                average_fps: s.average_fps, minimum_fps: s.minimum_fps, maximum_fps: s.maximum_fps,
                low1_fps: s.low1_fps, low01_fps: s.low01_fps, budget_ms: s.budget_ms, cpu_ms: s.cpu_ms,
                cpu_peak_ms: s.cpu_peak_ms, gpu_ms: s.gpu_ms, gpu_peak_ms: s.gpu_peak_ms,
                generation_ms: s.generation_ms, cpu_usage: hw.cpu_usage, core_maximum: hw.core_maximum,
                gpu_usage: hw.gpu_usage, gpu_temperature: hw.gpu_temperature, vram_bytes: s.vram_bytes,
                skipped: s.skipped, ram_used: hw.ram_used, ram_total: hw.ram_total,
                app_working_set: hw.app_working_set, gpu_memory_used: hw.gpu_memory_used,
                gpu_memory_total: hw.gpu_memory_total, gpu_process_used: hw.gpu_process_used,
                gpu_process_budget: hw.gpu_process_budget, cpu_graph: s.cpu_graph.as_ptr(),
                gpu_graph: s.gpu_graph.as_ptr(), graph_count: GRAPH_POINTS as i32, gpu_valid: s.gpu_valid,
                inputs_ready: s.inputs_ready, gpu_memory_valid: hw.gpu_memory_valid,
                gpu_process_memory_valid: hw.gpu_process_memory_valid, cpu_name: hw.cpu_name.as_ptr().cast(),
                gpu_name: hw.gpu_name.as_ptr().cast(),
            };
        }
    }
    pub(super) static RT: FpsRuntime = FpsRuntime {
        is_runtime_psvr2: cvr_cpp_xr_runtime_psvr2,
        reset_statistics: cvr_framegen_reset_statistics,
        statistics,
    };
}
static FPS_RUNTIME: AtomicPtr<FpsRuntime> = AtomicPtr::new((&raw const fps_table::RT).cast_mut());
#[no_mangle]
pub extern "C" fn cvr_overlay_fps_attach(runtime: *const FpsRuntime) {
    FPS_RUNTIME.store(runtime as *mut FpsRuntime, Release);
}
fn fps_rt() -> &'static FpsRuntime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*FPS_RUNTIME.load(Acquire) }
}

extern "C" {
    fn GetGameRenderFovDeg() -> f32;
    fn GetGameRenderVerticalFovDeg() -> f32;
}

/// DrawFovControl: the override, its presets and the render / projection fov, and what the camera renders now.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_fov_control(state: *mut LiveControlsUiState) -> bool {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return false;
    };
    let mut changed = false;
    let psvr2 = (fps_rt().is_runtime_psvr2)();
    let default_fov = if psvr2 { 104.5 } else { 112.0 };
    let mut override_enabled = st.xrForceFov > 0.0;
    if ui::checkbox(c"Enable FOV override", &mut override_enabled) {
        st.xrForceFov = if override_enabled { default_fov } else { 0.0 };
        changed = true;
    }
    if !override_enabled {
        ui::begin_disabled(true);
    }
    let mut preset = 0;
    if (st.xrForceFov - 104.5).abs() < 0.01 {
        preset = 1;
    } else if (st.xrForceFov - 94.0).abs() < 0.01 {
        preset = 2;
    } else if (st.xrForceFov - 104.0).abs() < 0.01 {
        preset = 3;
    }
    if ui::combo_z(
        c"FOV preset",
        &mut preset,
        b"Custom\0PSVR2 - 104.5 deg\0Quest 3 legacy - 94 deg (test)\0Pico 4 - 104 deg\0\0",
    ) && preset > 0
    {
        st.xrForceFov = [0.0, 104.5, 94.0, 104.0][preset as usize];
        changed = true;
    }
    let mut fov = if override_enabled {
        st.xrForceFov
    } else {
        default_fov
    };
    if ui::slider_float(
        c"Horizontal render / projection FOV",
        &mut fov,
        80.0,
        140.0,
        c"%.1f deg",
    ) {
        st.xrForceFov = fov;
        changed = true;
    }
    if !override_enabled {
        ui::end_disabled();
    }
    ui::text_wrapped(c"Override changes BOTH the game VR camera and OpenXR projection. Off uses automatic panel-edge coverage, which can be wider than the per-eye FOV span.", &[]);
    if psvr2 {
        ui::text_wrapped(c"PSVR2: 104.5 deg restores the previous override. Automatic coverage measured 123 deg on this setup.", &[]);
    }
    ui::text_wrapped(c"Narrower coverage can improve central detail at the same resolution, but may reveal black borders. Quest 3 legacy is an optional comparison, not a universal correction; keep the matching headset resolution preset.", &[]);
    // SAFETY: Rust's own functions under their C names.
    let (actual, vertical) = unsafe { (GetGameRenderFovDeg(), GetGameRenderVerticalFovDeg()) };
    if actual > 1.0 && vertical > 1.0 {
        ui::text(
            c"Camera render FOV: %.1f H / %.1f V deg",
            &[Arg::F(actual as f64), Arg::F(vertical as f64)],
        );
    }
    changed
}

/// DrawFpsOverlayControls: the FPS overlay's switch, layout, place and free look, the statistics' window, and the
/// statistics themselves while it is on.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_fps_overlay_controls(state: *mut LiveControlsUiState) -> bool {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return false;
    };
    let fg = framegen_settings(st);
    let mut changed = false;
    changed |= ui::checkbox(c"FPS overlay", &mut fg.overlay);
    changed |= ui::combo_z(
        c"Overlay layout",
        &mut fg.overlay_layout,
        b"Detailed\0Compact\0Minimal\0\0",
    );
    changed |= ui::combo_z(
        c"Overlay position",
        &mut fg.overlay_corner,
        b"Top left\0Top right\0Bottom left\0Bottom right\0Top center\0Bottom center\0Left center\0Right center\0Center\0\0",
    );
    changed |= ui::slider_float(c"Overlay size", &mut fg.overlay_scale, 0.5, 2.0, c"%.2fx");
    changed |= ui::slider_float(
        c"Overlay distance",
        &mut fg.overlay_distance,
        0.5,
        5.0,
        c"%.2f m",
    );
    changed |= ui::slider_float(
        c"Overlay horizontal offset",
        &mut fg.overlay_x,
        -60.0,
        60.0,
        c"%.1f deg",
    );
    changed |= ui::slider_float(
        c"Overlay vertical offset",
        &mut fg.overlay_y,
        -45.0,
        45.0,
        c"%.1f deg",
    );
    changed |= checkbox_int(c"Overlay free look", &mut fg.overlay_follow);
    ui::begin_disabled(fg.overlay_follow == 0);
    changed |= ui::slider_float(
        c"Overlay free-look cone",
        &mut fg.overlay_cone,
        5.0,
        90.0,
        c"%.1f deg",
    );
    ui::tooltip(c"Hold the panel heading inside this yaw range; catch up beyond it.");
    ui::end_disabled();
    let mut history = match fg.history_seconds {
        10 => 0,
        60 => 2,
        120 => 3,
        _ => 1,
    };
    if ui::combo_z(
        c"History window",
        &mut history,
        b"10 seconds\0 30 seconds\0 60 seconds\0 120 seconds\0\0",
    ) {
        fg.history_seconds = [10, 30, 60, 120][history as usize];
        changed = true;
    }
    if !fg.overlay {
        ui::text_disabled(c"FPS overlay and statistics are off.", &[]);
        return changed;
    }
    let rt = fps_rt();
    if ui::button(c"Reset statistics") {
        (rt.reset_statistics)();
    }
    // SAFETY: plain numbers and pointers, all written by the C++ half.
    let mut s: FpsStats = unsafe { core::mem::zeroed() };
    (rt.statistics)(&mut s);
    ui::text(
        c"Real %.1f FPS   Framegen %.1f FPS   Output %.1f FPS",
        &[
            Arg::F(s.real_fps),
            Arg::F(s.generated_fps),
            Arg::F(s.output_fps),
        ],
    );
    ui::text(
        c"Average %.1f FPS   Min %.1f   Max %.1f",
        &[
            Arg::F(s.average_fps),
            Arg::F(s.minimum_fps),
            Arg::F(s.maximum_fps),
        ],
    );
    if s.low1_fps > 0.0 {
        ui::text(c"1%% low %.1f FPS", &[Arg::F(s.low1_fps)]);
    } else {
        ui::text_disabled(c"1%% low: collecting 100 real frame intervals", &[]);
    }
    if s.low01_fps > 0.0 {
        ui::text(c"0.1%% low %.1f FPS", &[Arg::F(s.low01_fps)]);
    } else {
        ui::text_disabled(c"0.1%% low: collecting 1000 real frame intervals", &[]);
    }
    let graph_max = 33.34f64.max(s.budget_ms * 2.0) as f32;
    ui::plot_lines(
        c"CPU history (ms)",
        s.cpu_graph,
        s.graph_count,
        0.0,
        graph_max,
        0.0,
        55.0,
    );
    ui::plot_lines(
        c"GPU history (ms)",
        s.gpu_graph,
        s.graph_count,
        0.0,
        graph_max,
        0.0,
        55.0,
    );
    ui::text(
        c"CPU frame %.2f ms   Peak %.2f ms",
        &[Arg::F(s.cpu_ms), Arg::F(s.cpu_peak_ms)],
    );
    if s.gpu_valid {
        ui::text(
            c"GPU frame %.2f ms   Peak %.2f ms",
            &[Arg::F(s.gpu_ms), Arg::F(s.gpu_peak_ms)],
        );
    } else {
        ui::text_disabled(c"GPU frame timing: waiting for completed queries", &[]);
    }
    ui::text(
        c"Generation %.2f ms   Tracked VRAM %.1f MiB",
        &[
            Arg::F(s.generation_ms),
            Arg::F(s.vram_bytes as f64 / (1024.0 * 1024.0)),
        ],
    );
    let gib = 1024.0f64 * 1024.0 * 1024.0;
    // SAFETY: NUL-terminated names the C++ half keeps until the next call.
    let named = |p: *const core::ffi::c_char| {
        if !p.is_null() && unsafe { *p } != 0 {
            p
        } else {
            c"--".as_ptr()
        }
    };
    ui::text(c"CPU: %s", &[Arg::S(named(s.cpu_name))]);
    ui::text(c"GPU: %s", &[Arg::S(named(s.gpu_name))]);
    if s.cpu_usage >= 0.0 {
        ui::text(
            c"CPU load %.1f%%   Busiest logical core %.1f%%",
            &[Arg::F(s.cpu_usage), Arg::F(s.core_maximum)],
        );
    }
    if s.gpu_usage >= 0.0 {
        ui::text(
            c"GPU load %.1f%%   Temperature %.0f C",
            &[Arg::F(s.gpu_usage), Arg::F(s.gpu_temperature)],
        );
    }
    if s.gpu_memory_valid {
        ui::text(
            c"GPU memory %.2f / %.2f GiB",
            &[
                Arg::F(s.gpu_memory_used as f64 / gib),
                Arg::F(s.gpu_memory_total as f64 / gib),
            ],
        );
    }
    if s.gpu_process_memory_valid {
        ui::text(
            c"Game GPU memory %.2f GiB   Budget %.2f GiB",
            &[
                Arg::F(s.gpu_process_used as f64 / gib),
                Arg::F(s.gpu_process_budget as f64 / gib),
            ],
        );
    }
    ui::text(
        c"RAM %.2f / %.2f GiB   Game working set %.2f GiB",
        &[
            Arg::F(s.ram_used as f64 / gib),
            Arg::F(s.ram_total as f64 / gib),
            Arg::F(s.app_working_set as f64 / gib),
        ],
    );
    ui::text(
        c"Depth + motion: %s   Skipped %llu",
        &[
            Arg::S(if s.inputs_ready {
                c"both eyes observed".as_ptr()
            } else {
                c"waiting".as_ptr()
            }),
            Arg::U(s.skipped),
        ],
    );
    ui::text_disabled(
        c"CPU: captured frame cadence. GPU: graphics-queue frame interval. Peaks: last 2 seconds.",
        &[],
    );
    ui::text_disabled(
        c"Output FPS excludes repeated frames; it is not a headset scanout measurement.",
        &[],
    );
    ui::text_disabled(c"Lows use the mean of the slowest 1%% / 0.1%% of real-frame intervals. Hardware polling: 1 Hz.", &[]);
    changed
}

// ---- the live controls: GENERAL ------------------------------------------------------------------------------------

/// What the GENERAL section asks of the C++ half: the game upscaler's CAS block, the hidden area's status, the
/// overlay's gizmo switches and the verbose channel.
#[repr(C)]
pub struct GeneralRuntime {
    blocks_cas: extern "C" fn() -> bool,
    /// cvr::hidden_area::GetStatus into a HiddenAreaStatus.
    hidden_area_status: extern "C" fn(*mut HiddenAreaStatus),
    draw_hand_locator: *mut bool,
    draw_hand_proxy_3d: *mut bool,
    draw_hand_debug_axes: *mut bool,
    hand_locator_scale: *mut f32,
    verbose_log: *mut i32,
}
// SAFETY: the table holds plain functions and addresses of statics.
unsafe impl Sync for GeneralRuntime {}
static GENERAL_RT: GeneralRuntime = GeneralRuntime {
    blocks_cas: cvr_cpp_blocks_cas,
    hidden_area_status,
    draw_hand_locator: &raw mut g_drawHandLocator,
    draw_hand_proxy_3d: &raw mut g_drawHandProxy3D,
    draw_hand_debug_axes: &raw mut g_drawHandDebugAxes,
    hand_locator_scale: &raw mut g_handLocatorScale,
    verbose_log: &raw mut g_verboseLog,
};

/// cvr::hidden_area::Status.
/// hidden_area::GetStatus (formerly OverlayPanels.cpp's service over HiddenArea.cpp's): the last masked clear's
/// rectangles, covered share and size per view, and each view's masked clears.
extern "C" fn hidden_area_status(out: *mut HiddenAreaStatus) {
    let mut s = HiddenAreaStatus::default();
    // SAFETY: room for two of each.
    unsafe {
        crate::hidden_area_runtime::cvr_hidden_area_last(
            s.rects.as_mut_ptr(),
            s.hidden_percent.as_mut_ptr(),
            s.width.as_mut_ptr(),
            s.height.as_mut_ptr(),
        )
    };
    for (m, v) in s.masked.iter_mut().zip(&crate::used_moved_exports_rest::CyberpunkVR_HiddenAreaMasked) {
        *m = v.load(core::sync::atomic::Ordering::Relaxed);
    }
    // SAFETY: the caller's output.
    unsafe { *out = s };
}
#[repr(C)]
#[derive(Default)]
pub struct HiddenAreaStatus {
    masked: [u64; 2],
    rects: [u32; 2],
    hidden_percent: [f32; 2],
    width: [u32; 2],
    height: [u32; 2],
}
const _: () = {
    assert!(core::mem::size_of::<crate::foveated::Settings>() == 28);
    assert!(core::mem::size_of::<crate::vrs::Settings>() == 20);
    assert!(core::mem::size_of::<crate::hidden_area::Settings>() == 8);
    assert!(core::mem::size_of::<HiddenAreaStatus>() == 48);
};
/// ImGuiTreeNodeFlags_DefaultOpen.
const DEFAULT_OPEN: i32 = 1 << 5;
/// ImGuiHoveredFlags_AllowWhenDisabled.
const ALLOW_WHEN_DISABLED: i32 = 1 << 10;

/// DrawLiveControls' GENERAL section: CAS sharpening, fixed foveated rendering, shared sun shadows, the scope lens's
/// stabilisation, variable rate shading, the hidden area mask, the game menus' quad, tracking and camera offsets, the
/// debug gizmos, the verbose log.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState; `foveated_editing` is null or a writable bool.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_live_controls_general(
    state: *mut LiveControlsUiState,
    foveated_editing: *mut bool,
) -> bool {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return false;
    };
    // SAFETY: attached at the C++ half's static initialisation.
    let rt = &GENERAL_RT;
    let mut changed = false;
    let cas_blocked = (rt.blocks_cas)();
    let mut cas_amount = if cas_blocked { 0.0 } else { st.xrSharpmix };
    ui::begin_disabled(cas_blocked);
    if ui::slider_float(c"CAS Sharpening", &mut cas_amount, 0.0, 1.0, c"%.3f") && !cas_blocked {
        st.xrSharpmix = cas_amount;
        changed = true;
    }
    if ui::is_item_hovered_flags(ALLOW_WHEN_DISABLED) {
        ui::set_tooltip(c"Contrast-adaptive sharpening for both eyes. 0 = off, 1 = full strength. Disabled automatically with game DLAA.", &[]);
    }
    ui::end_disabled();
    if ui::collapsing_header(c"FIXED FOVEATED RENDERING", DEFAULT_OPEN) {
        // SAFETY: the snapshot's foveated member is a cvr::foveated::Settings (28 bytes, 4-aligned).
        let foveated =
            unsafe { &mut *(st.foveated.as_mut_ptr() as *mut crate::foveated::Settings) };
        changed |= ui::combo_z(
            c"Peripheral quality",
            &mut foveated.mode,
            b"Off\0Quality\0Balanced\0Performance\0\0",
        );
        ui::tooltip(c"The center follows game DLSS/DLAA. Peripheral DLAA uses 100%% / 75%% / 50%% of the game's render resolution.");
        ui::begin_disabled(foveated.mode == 0);
        changed |= ui::slider_float(
            c"Center width",
            &mut foveated.width_percent,
            20.0,
            100.0,
            c"%.1f %%",
        );
        let mut editing = ui::is_item_active();
        ui::tooltip(c"Full-quality region width as a percentage of each eye image.");
        changed |= ui::slider_float(
            c"Center height",
            &mut foveated.height_percent,
            20.0,
            100.0,
            c"%.1f %%",
        );
        editing |= ui::is_item_active();
        ui::tooltip(c"Full-quality region height as a percentage of each eye image.");
        *foveated = crate::foveated::cvr_foveated_normalize(*foveated);
        let max_x = (100.0 - foveated.width_percent) * 0.5;
        let max_y = (100.0 - foveated.height_percent) * 0.5;
        ui::begin_disabled(max_x == 0.0);
        changed |= ui::slider_float(
            c"Center offset X",
            &mut foveated.offset_x_percent,
            -max_x,
            max_x,
            c"%.1f %%",
        );
        editing |= ui::is_item_active();
        ui::tooltip(c"Percentage of eye-image width. Negative = left, positive = right.");
        ui::end_disabled();
        ui::begin_disabled(max_y == 0.0);
        changed |= ui::slider_float(
            c"Center offset Y",
            &mut foveated.offset_y_percent,
            -max_y,
            max_y,
            c"%.1f %%",
        );
        editing |= ui::is_item_active();
        ui::tooltip(
            c"Percentage of eye-image height. Negative = up, positive = down toward the hands.",
        );
        ui::end_disabled();
        if !foveated_editing.is_null() {
            // SAFETY: the caller's bool.
            unsafe { *foveated_editing = editing };
        }
        changed |= ui::checkbox(c"Show region boundary", &mut foveated.show_boundary);
        ui::end_disabled();
        // SAFETY: plain numbers and a C string, all written by the call.
        let mut status: crate::foveated_dlss::Status = unsafe { core::mem::zeroed() };
        // SAFETY: a status to fill.
        unsafe { crate::foveated_dlss::cvr_foveated_dlss_status(&mut status) };
        ui::text_disabled(
            c"%s",
            &[Arg::S(status.text.as_ptr() as *const core::ffi::c_char)],
        );
        if foveated.mode != 0 && status.evaluations != 0 {
            ui::text(
                c"DLSS GPU: native %.2f ms / foveated %.2f ms",
                &[
                    Arg::F(status.native_ms as f64),
                    Arg::F(status.total_ms as f64),
                ],
            );
        }
    }
    changed |= checkbox_int(c"Shared sun shadows", &mut st.monoCascades);
    ui::tooltip(c"Render sun cascades once for both eyes. Shadow quality and distance remain controlled by the game.");
    changed |= slider_int_clamped(c"Scope lens stabilization", &mut st.scopeStabilize, 0, 100);
    ui::tooltip(c"Magnifying scope lenses while the view moves: share of the stabilised image (percent). Higher: no blinking of lights and grates, softer. 0 = off. Standing still the lens is untouched.");
    if ui::collapsing_header(c"VARIABLE RATE SHADING", DEFAULT_OPEN) {
        ui::push_id(c"fixedVrs");
        // SAFETY: the snapshot's vrs member is a cvr::vrs::Settings (20 bytes, 4-aligned).
        let v = unsafe { &mut *(st.vrs.as_mut_ptr() as *mut crate::vrs::Settings) };
        changed |= ui::combo_z(
            c"Peripheral shading",
            &mut v.mode,
            b"Off\0 2 x 2\0 4 x 4\0\0",
        );
        ui::tooltip(c"Opaque scene materials in both eyes. The center stays 1 x 1. Unsupported 4 x 4 falls back to 2 x 2.");
        ui::begin_disabled(v.mode == 0);
        changed |= ui::slider_float(c"Center width", &mut v.width, 20.0, 100.0, c"%.1f %%");
        changed |= ui::slider_float(c"Center height", &mut v.height, 20.0, 100.0, c"%.1f %%");
        *v = crate::vrs::cvr_vrs_normalize(*v);
        let x = (100.0 - v.width) * 0.5;
        let y = (100.0 - v.height) * 0.5;
        ui::begin_disabled(x == 0.0);
        changed |= ui::slider_float(c"Center offset X", &mut v.offset_x, -x, x, c"%.1f %%");
        ui::end_disabled();
        ui::begin_disabled(y == 0.0);
        changed |= ui::slider_float(c"Center offset Y", &mut v.offset_y, -y, y, c"%.1f %%");
        ui::end_disabled();
        ui::end_disabled();
        // SAFETY: plain numbers and flags, written by the call.
        let mut s: crate::fixed_vrs::Status = unsafe { core::mem::zeroed() };
        // SAFETY: a status to fill.
        unsafe { crate::fixed_vrs::cvr_vrs_get_status(&mut s) };
        if v.mode != 0 {
            if s.failed {
                ui::text_disabled(c"Unavailable: GPU resource error", &[]);
            } else if s.checked && !s.supported {
                ui::text_disabled(c"VRS Tier 2 unavailable", &[]);
            } else if s.ready_maps == 0 {
                ui::text_disabled(c"Preparing", &[]);
            } else {
                ui::text_disabled(
                    c"%s | %u px tiles",
                    &[
                        Arg::S(if v.mode == 2 && s.additional_rates {
                            c"4 x 4".as_ptr()
                        } else {
                            c"2 x 2".as_ptr()
                        }),
                        Arg::U(s.tile_size as u64),
                    ],
                );
            }
        }
        ui::pop_id();
    }
    if ui::collapsing_header(c"HIDDEN AREA MASK", DEFAULT_OPEN) {
        ui::push_id(c"hiddenArea");
        // SAFETY: the snapshot's hiddenArea member is a cvr::hidden_area::Settings (8 bytes, 4-aligned).
        let h = unsafe { &mut *(st.hiddenArea.as_mut_ptr() as *mut crate::hidden_area::Settings) };
        changed |= checkbox_int(c"Mask lens edges", &mut h.enabled);
        ui::tooltip(c"Skips shading the parts of each eye image the lens never shows. The visible area is unchanged.");
        ui::begin_disabled(h.enabled == 0);
        changed |= ui::slider_float(
            c"Visible radius",
            &mut h.radius,
            crate::hidden_area::MIN_RADIUS,
            crate::hidden_area::MAX_RADIUS,
            c"%.0f %%",
        );
        ui::tooltip(c"Size of the shaded area as a percentage of the eye's field of view: 100%% touches its edges, 141%% masks nothing inside it.\nLower it until a dark border appears at the edge of the lens, then raise it until the border is gone.");
        ui::end_disabled();
        *h = crate::hidden_area::cvr_hidden_area_normalize(*h);
        if h.enabled != 0 {
            let mut s = HiddenAreaStatus::default();
            (rt.hidden_area_status)(&mut s);
            if s.masked[0] == 0 && s.masked[1] == 0 {
                ui::text_disabled(c"Waiting for the scene depth", &[]);
            } else {
                ui::text_disabled(
                    c"Masked %.1f %% / %.1f %% of pixels (MAIN / VRCAM)",
                    &[
                        Arg::F(s.hidden_percent[0] as f64),
                        Arg::F(s.hidden_percent[1] as f64),
                    ],
                );
            }
        }
        ui::pop_id();
    }
    if ui::collapsing_header(c"GAME MENUS", DEFAULT_OPEN) {
        changed |= checkbox_int(c"VR menu quad", &mut st.xrMenuRect);
        changed |= ui::slider_float(c"VR menu FOV", &mut st.xrMenuFov, 30.0, 120.0, c"%.1f deg");
    }

    if ui::collapsing_header(c"Tracking / Camera", 0) {
        ui::text_unformatted(c"Locomotion direction is set in the Controls tab.");
        changed |= checkbox_int(c"Disable Mouse Y (Pitch)", &mut st.xrDisableMouseY);
        ui::tooltip(c"Suppress mouse/right-stick pitch so only the HMD controls\nvertical look. Applied by the CET VRIK mod and the\nXInput merge. On by default.");
        // The offsets are always live and reach BOTH eyes ("Fix Head", which dropped them, is gone).
        changed |= ui::slider_float(
            c"Head X right",
            &mut st.xrHeadOffsetX,
            -0.50,
            0.50,
            c"%.3f m",
        );
        changed |= ui::slider_float(
            c"Head Y forward",
            &mut st.xrHeadOffsetY,
            -0.50,
            0.50,
            c"%.3f m",
        );
        changed |= ui::slider_float(c"Head Z up", &mut st.xrHeadOffsetZ, -0.50, 0.50, c"%.3f m");
    }

    if ui::collapsing_header(c"Debug Gizmos", 0) {
        ui::text_unformatted(c"Raw hand overlay / debug gizmos:");
        // SAFETY: the overlay's own globals, render thread.
        unsafe {
            ui::checkbox(c"Enable hand overlay", &mut *rt.draw_hand_locator);
            ui::checkbox(c"Draw 3D hand proxy", &mut *rt.draw_hand_proxy_3d);
            ui::checkbox(c"Draw debug wire/axes", &mut *rt.draw_hand_debug_axes);
            ui::slider_float(
                c"Locator scale",
                &mut *rt.hand_locator_scale,
                0.50,
                2.00,
                c"%.2f",
            );
        }
    }

    if ui::collapsing_header(c"Diagnostics", 0) {
        // SAFETY: the C++ half's volatile int.
        let mut vl = unsafe { core::ptr::read_volatile(rt.verbose_log) };
        if checkbox_int(c"Verbose log (spammy diag)", &mut vl) {
            // SAFETY: as above.
            unsafe { core::ptr::write_volatile(rt.verbose_log, vl) };
        }
        ui::tooltip(c"Off by default for a clean cyberpunkvrport.log. Enable only\nwhen capturing ClipCursor / depth / hook diagnostics.");
    }
    changed
}

/// SliderIntClamped: widgets::SliderInt on a copy, clamped back.
/// The PSVR2 Toolkit audio companion's settings (plugins/cyber_engine_tweaks/mods/PSVR2 Haptics/settings.ini beside the
/// game's exe): read once a second, written through a temporary file replaced in one move (main's 0.2.4).
fn toolkit_audio_settings() {
    struct Audio {
        enabled: bool,
        valid: bool,
        strength: i32,
        last_read: u64,
        error: bool,
    }
    static AUDIO: Mutex<Audio> = Mutex::new(Audio { enabled: false, valid: false, strength: 35, last_read: 0, error: false });
    let mut a = AUDIO.lock().unwrap_or_else(|e| e.into_inner());
    let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("plugins/cyber_engine_tweaks/mods/PSVR2 Haptics/settings.ini")))
    else {
        return;
    };
    // SAFETY: a plain Win32 call.
    let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    if now.wrapping_sub(a.last_read) > 1000 {
        a.last_read = now;
        let (mut version, mut enabled, mut strength) = (-1i32, -1i32, -1i32);
        if let Ok(text) = std::fs::read_to_string(&path) {
            for line in text.lines() {
                let number = |key: &str| line.strip_prefix(key).and_then(leading_int);
                if let Some(n) = number("Version=") {
                    version = n;
                }
                if let Some(n) = number("AudioEnabled=") {
                    enabled = n;
                }
                if let Some(n) = number("AudioStrength=") {
                    strength = n;
                }
            }
        }
        a.valid = version == 1 && (enabled == 0 || enabled == 1) && (0..=300).contains(&strength);
        if a.valid {
            a.enabled = enabled != 0;
            a.strength = strength;
        }
    }
    ui::begin_disabled(!a.valid);
    let mut edited = ui::checkbox(c"Toolkit audio haptics", &mut a.enabled);
    edited |= slider_int_clamped(c"Toolkit audio strength (%)", &mut a.strength, 0, 300);
    ui::end_disabled();
    if edited {
        let mut temporary = path.clone().into_os_string();
        temporary.push(".f10.tmp");
        let text = format!("Version=1\nAudioEnabled={}\nAudioStrength={}\n", a.enabled as i32, a.strength);
        let written = std::fs::write(&temporary, text).is_ok()
            // std's rename is MoveFileExW(MOVEFILE_REPLACE_EXISTING)
            && std::fs::rename(&temporary, &path).is_ok();
        a.error = !written;
        a.last_read = 0;
    }
    if !a.valid {
        ui::text_wrapped(c"Toolkit audio companion settings missing or invalid. Install the PSVR2Toolkit audio settings companion.", &[]);
    }
    if a.error {
        ui::text_wrapped(c"%s", &[Arg::S(c"Unable to save Toolkit audio settings.".as_ptr())]);
    }
    ui::text_wrapped(c"Applies live through the updated Toolkit bridge. Captures default playback audio; weapon effects and adaptive triggers are separate. The driving-only channel and grip controls below apply to the separate port road/engine effect.", &[]);
}
/// sscanf's %d: blanks, a sign and the digits that follow; None without a digit.
fn leading_int(v: &str) -> Option<i32> {
    let t = v.trim_start();
    let (sign, digits) = match t.as_bytes().first() {
        Some(b'-') => (-1i64, &t[1..]),
        Some(b'+') => (1, &t[1..]),
        _ => (1, t),
    };
    let end = digits.bytes().take_while(u8::is_ascii_digit).count();
    if end == 0 {
        return None;
    }
    digits[..end].parse::<i64>().ok().map(|n| (sign * n).clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}
fn slider_int_clamped(label: &core::ffi::CStr, value: &mut i32, min: i32, max: i32) -> bool {
    let mut temp = *value;
    let changed = ui::slider_int(label, &mut temp, min, max);
    if changed {
        *value = temp.clamp(min, max);
    }
    changed
}

// ---- the live controls: FRAMEGEN -----------------------------------------------------------------------------------

/// What the FRAMEGEN section asks of the C++ half.
#[repr(C)]
pub struct FramegenRuntime {
    /// cvr::framegen::ActiveBackend: 0 FidelityFX, 1 NVIDIA OFA + FidelityFX, 2 DLSS Frame Generation.
    active_backend: extern "C" fn() -> i32,
    /// cvr::framegen::dlssg::Reason(provider): 0 NVIDIA's DLSS-G, 1 ANR FG.
    dlssg_reason: extern "C" fn(u8) -> *const core::ffi::c_char,
    /// cvr::framegen::GetRuntimeStatus's text into a 160-byte buffer.
    runtime_status: extern "C" fn(*mut core::ffi::c_char),
}
/// The frame generation panel's table, built here (formerly src/Overlay/OverlayPanels.cpp's).
mod framegen_table {
    use super::FramegenRuntime;
    use core::ffi::c_char;
    /// cvr::framegen::GetRuntimeStatus's text into the caller's 160 bytes.
    extern "C" fn runtime_status(out: *mut c_char) {
        let mut text = [0u8; 160];
        cvr_gpu::framegen_core::cvr_framegen_runtime_status(text.as_mut_ptr());
        // SAFETY: the caller's 160 bytes.
        unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), out.cast(), text.len()) };
    }
    extern "C" fn dlssg_reason(provider: u8) -> *const c_char {
        crate::dlssg_native::reason_of(provider)
    }
    pub(super) static RT: FramegenRuntime = FramegenRuntime {
        active_backend: cvr_gpu::framegen_core::cvr_framegen_active_backend,
        dlssg_reason,
        runtime_status,
    };
}
static FRAMEGEN_RUNTIME: AtomicPtr<FramegenRuntime> = AtomicPtr::new((&raw const framegen_table::RT).cast_mut());
#[no_mangle]
pub extern "C" fn cvr_overlay_framegen_attach(runtime: *const FramegenRuntime) {
    FRAMEGEN_RUNTIME.store(runtime as *mut FramegenRuntime, Release);
}
const BACKEND_FIDELITYFX: i32 = 0;
const BACKEND_NVIDIA: i32 = 1;
const BACKEND_DLSSG: i32 = 2;
const BACKEND_ANR: i32 = 3;
const BACKEND_FSR3_INTERPOLATION: i32 = 4;
const BACKEND_FSR3_EXTRAPOLATION: i32 = 5;

/// DrawLiveControls' FRAMEGEN section: the generator, its backend and what the GPU allows of it, the optical flow's
/// scale, adaptive pacing, the runtime's status line.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_live_controls_framegen(state: *mut LiveControlsUiState) -> bool {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return false;
    };
    // SAFETY: attached at the C++ half's static initialisation.
    let rt = unsafe { &*FRAMEGEN_RUNTIME.load(Acquire) };
    let fg = framegen_settings(st);
    let mut changed = false;
    changed |= ui::checkbox(c"Enable frame generation", &mut fg.enabled);
    ui::tooltip(c"While frame generation is enabled, hand smoothing runs at its rate times xr_hand_lerp_framegen_scale (1.10; 0 turns it off).");
    // the combo's rows are the backends' numbers (main's 0.2.4: ANR FG is 3)
    let mut row = fg.backend.clamp(0, 5);
    if ui::combo_z(
        c"Frame generator",
        &mut row,
        b"FidelityFX\0NVIDIA OFA + FidelityFX\0NVIDIA DLSS Frame Generation\0ANR FG (Apex Neural Rendering)\0FSR3 interpolation\0FSR3 extrapolation\0\0",
    ) {
        fg.backend = row.clamp(0, 5);
        changed = true;
    }
    use cvr_gpu::framegen_core::{FSR3_HIDE, FSR3_NO_OPTICAL_FLOW};
    if fg.backend == BACKEND_FSR3_INTERPOLATION {
        let mut flow = fg.flags & FSR3_NO_OPTICAL_FLOW == 0;
        if ui::checkbox(c"FSR3 optical flow", &mut flow) {
            fg.flags = if flow { fg.flags & !FSR3_NO_OPTICAL_FLOW } else { fg.flags | FSR3_NO_OPTICAL_FLOW };
            changed = true;
        }
    }
    if fg.backend == BACKEND_FSR3_INTERPOLATION || fg.backend == BACKEND_FSR3_EXTRAPOLATION {
        let mut show = fg.flags & FSR3_HIDE == 0;
        if ui::checkbox(c"Show FSR3 generated frames", &mut show) {
            fg.flags = if show { fg.flags & !FSR3_HIDE } else { fg.flags | FSR3_HIDE };
            changed = true;
        }
    }
    let active = (rt.active_backend)();
    if fg.backend == BACKEND_DLSSG && fg.enabled && active != BACKEND_DLSSG {
        ui::text_wrapped(
            c"DLSS Frame Generation is not available here (%s); FidelityFX is used.",
            &[Arg::S((rt.dlssg_reason)(0))],
        );
    } else if fg.backend == BACKEND_ANR && fg.enabled && active != BACKEND_ANR {
        ui::text_wrapped(c"ANR FG is not available here (%s); FidelityFX is used.", &[Arg::S((rt.dlssg_reason)(1))]);
    } else if fg.backend == BACKEND_NVIDIA && fg.enabled && active == BACKEND_FIDELITYFX {
        ui::text_wrapped(
            c"NVIDIA optical flow is not available on this GPU; FidelityFX optical flow is used.",
            &[],
        );
    } else if fg.backend == BACKEND_NVIDIA && fg.flow_scale != 0 {
        changed |= ui::combo_z(
            c"NVIDIA quality",
            &mut fg.quality,
            b"Fast (performance)\0Medium (balanced)\0Slow (best)\0\0",
        );
    }
    if fg.backend == BACKEND_FIDELITYFX || fg.backend == BACKEND_NVIDIA {
        let mut scale = match fg.flow_scale {
            100 => 3,
            75 => 2,
            0 => 0,
            _ => 1,
        };
        if ui::combo_z(
            c"Optical flow",
            &mut scale,
            b"Off (game vectors only)\0 50%\0 75%\0 100%\0\0",
        ) {
            fg.flow_scale = match scale {
                3 => 100,
                2 => 75,
                0 => 0,
                _ => 50,
            };
            changed = true;
        }
        ui::tooltip(c"The game's motion vectors already describe almost all motion; optical flow adds GPU time\nand memory for what they miss (some particles and effects).");
    }
    if fg.backend == BACKEND_DLSSG {
        ui::text_wrapped(c"NVIDIA's frame generation model on the game's motion vectors and depth. Highest quality, RTX 40/50.", &[]);
    }
    if fg.backend == BACKEND_ANR {
        ui::text_wrapped(c"Apex Neural Rendering's frame generation model (beta), run as a DLSS-G provider on the game's motion vectors and depth. Any recent NVIDIA, AMD or Intel GPU.", &[]);
    }
    changed |= ui::checkbox(c"Adaptive real-frame pacing", &mut fg.auto_pace);
    ui::tooltip(c"Up to half headset refresh. Leaves more GPU time for generated frames when adjacent real frames are being missed.");
    if fg.backend == BACKEND_FSR3_EXTRAPOLATION {
        ui::text_wrapped(c"The real stereo pair is shown first. A spare display cycle can show a half-frame prediction from its motion vectors.", &[]);
    } else {
        ui::text_wrapped(c"Uses game motion vectors and depth in both eyes. Adds one intermediate frame between real frames.", &[]);
    }
    ui::separator();
    let mut text = [0 as core::ffi::c_char; 160];
    (rt.runtime_status)(text.as_mut_ptr());
    ui::text_wrapped(c"%s", &[Arg::S(text.as_ptr())]);
    changed
}

// ---- the live controls: HUD ----------------------------------------------------------------------------------------

/// The HUD section's element combo: `static int element=10;` (the minimap).
static HUD_ELEMENT: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(10);
/// cvr::hud::Elements' labels as C strings, for the element combo.
fn hud_element_labels() -> &'static [std::ffi::CString] {
    static LABELS: std::sync::OnceLock<Vec<std::ffi::CString>> = std::sync::OnceLock::new();
    LABELS.get_or_init(|| {
        cvr_settings::hud_layout::ELEMENTS
            .iter()
            .map(|e| std::ffi::CString::new(e.label).expect("a label without NUL"))
            .collect()
    })
}

/// DrawLiveControls' HUD section: the HUD panel, how it follows, its depth and what keeps it in front of the world and
/// the hands, its look; the interactions / dialogs panel; one HUD element's place, size and opacity.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_live_controls_hud(state: *mut LiveControlsUiState) -> bool {
    use cvr_settings::hud_layout::{self, ElementSettings};
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return false;
    };
    let mut changed = false;
    changed |= checkbox_int(c"Enable HUD panel", &mut st.xrHudPanel);
    changed |= ui::combo(
        c"Follow",
        &mut st.xrHudFollowMode,
        &[c"Head / free-look cone", c"Body rotation"],
    );
    ui::begin_disabled(st.xrHudFollowMode != 0);
    changed |= ui::slider_float(
        c"Free-look cone",
        &mut st.xrHudFollowDeg,
        5.0,
        90.0,
        c"%.1f deg",
    );
    ui::tooltip(c"Catch up beyond the cone, or after 3 seconds at rest when the yaw offset is over 10 degrees inside it.");
    ui::end_disabled();
    changed |= ui::slider_float(c"HUD FOV", &mut st.xrHudFov, 30.0, 120.0, c"%.1f deg");
    changed |= checkbox_int(c"Stereo depth", &mut st.xrHudStereoDepth);
    ui::tooltip(c"Off: the HUD has the same direction in both eyes.\nOn: place it at the selected physical depth.");
    ui::begin_disabled(st.xrHudStereoDepth == 0);
    changed |= ui::slider_float(c"Depth", &mut st.xrHudDistance, 0.5, 5.0, c"%.2f m");
    // In front of the world, never inside it (Runtimes/HudClearance.hpp).
    changed |= checkbox_int(c"Keep HUD in front of the world", &mut st.xrHudClearance);
    ui::tooltip(c"The HUD comes closer when a wall or a person is nearer than Depth.\nOff: always at Depth.");
    ui::begin_disabled(st.xrHudClearance == 0);
    changed |= ui::slider_float(
        c"Grow when brought closer",
        &mut st.xrHudClearanceGrow,
        0.0,
        1.0,
        c"%.2f",
    );
    ui::tooltip(c"0: same size on screen. 1: same size in the world.");
    changed |= checkbox_int(
        c"Hands push the HUD out of view",
        &mut st.xrHudClearanceHands,
    );
    ui::tooltip(c"While a hand is in front of the HUD, the HUD grows until its elements leave the view,\nso your hands are not covered by it.");
    ui::begin_disabled(st.xrHudClearanceHands == 0);
    changed |= ui::slider_float(
        c"Out-of-view amount",
        &mut st.xrHudClearanceHandsScale,
        1.0,
        3.0,
        c"%.2fx",
    );
    ui::tooltip(c"How far the HUD moves out while a hand is in front of it.\n1: not at all. 2: to the edge of the view, a small head turn away. 3: fully out of view.");
    ui::end_disabled();
    ui::end_disabled();
    ui::end_disabled();
    changed |= ui::slider_float(c"Brightness", &mut st.xrHudBrightness, 0.25, 3.0, c"%.2fx");
    changed |= ui::slider_float(c"Shadow / outline", &mut st.xrHudShadow, 0.0, 2.0, c"%.2fx");
    changed |= ui::slider_float(c"Glow", &mut st.xrHudGlow, 0.0, 2.0, c"%.2fx");
    ui::separator();
    if ui::collapsing_header(c"INTERACTIONS / DIALOGS", DEFAULT_OPEN) {
        changed |= checkbox_int(
            c"World-space interactions / dialogs",
            &mut st.xrInteractionPanel,
        );
        changed |= ui::slider_float(
            c"Interaction free-look cone",
            &mut st.xrInteractionFollowDeg,
            0.0,
            90.0,
            c"%.1f deg",
        );
        changed |= ui::slider_float(
            c"Loot free-look cone",
            &mut st.xrLootFollowDeg,
            0.0,
            90.0,
            c"%.1f deg",
        );
        ui::tooltip(
            c"Used while the loot panel and item description are visible. Default: 10 degrees.",
        );
        changed |= ui::slider_float(
            c"Interaction distance",
            &mut st.xrInteractionDistance,
            0.5,
            5.0,
            c"%.2f m",
        );
        changed |= checkbox_int(c"Prompt at the object's depth", &mut st.xrInteractionAnchor);
        ui::tooltip(c"The prompt sits 10 cm in front of the object it names.\nInteraction distance is then used only when nothing is targeted.");
        changed |= ui::slider_float(
            c"Interaction FOV",
            &mut st.xrInteractionFov,
            30.0,
            120.0,
            c"%.1f deg",
        );
        ui::text_wrapped(
            c"Interaction prompts and dialog choices share this panel. It follows only beyond the cone; no delayed catch-up at rest.",
            &[],
        );
    }
    ui::separator();
    let labels: Vec<&core::ffi::CStr> = hud_element_labels().iter().map(|s| s.as_c_str()).collect();
    let mut element = HUD_ELEMENT.load(Relaxed);
    ui::combo(c"Element", &mut element, &labels);
    HUD_ELEMENT.store(element, Relaxed);
    let element = element.clamp(0, hud_layout::COUNT as i32 - 1) as usize;
    if !hud_layout::status()[element].available {
        ui::text_disabled(
            c"Not currently visible in gameplay; settings are still saved.",
            &[],
        );
    }
    // SAFETY: cvr::hud::LayoutSettings, 40 ElementSettings of 20 bytes, held as 200 u32 of the same alignment.
    let elements =
        unsafe { &mut *(st.hudElements.as_mut_ptr() as *mut [ElementSettings; hud_layout::COUNT]) };
    let transform = &mut elements[element];
    changed |= checkbox_int(c"Show element", &mut transform.visible);
    changed |= ui::slider_float(
        c"Horizontal offset",
        &mut transform.x,
        -100.0,
        100.0,
        c"%.1f %%",
    );
    changed |= ui::slider_float(
        c"Vertical offset",
        &mut transform.y,
        -100.0,
        100.0,
        c"%.1f %%",
    );
    changed |= ui::slider_float(c"Size", &mut transform.scale, 0.1, 3.0, c"%.2fx");
    changed |= ui::slider_float(c"Opacity", &mut transform.opacity, 0.0, 1.0, c"%.2f");
    if ui::button(c"Reset element") {
        *transform = ElementSettings::DEFAULT;
        changed = true;
    }
    ui::same_line();
    if ui::button(c"Reset all elements") {
        *elements = [ElementSettings::DEFAULT; hud_layout::COUNT];
        changed = true;
    }
    changed
}

// ---- the live controls: CONTROLS -----------------------------------------------------------------------------------

/// What the CONTROLS section asks of the C++ half.
#[repr(C)]
pub struct ControlsRuntime {
    /// The FBT calibration's step, availability, start and cancel (fbt_calibration.rs).
    calibration_step: extern "C" fn() -> i32,
    calibration_available: extern "C" fn() -> bool,
    start_calibration: extern "C" fn(),
    cancel_calibration: extern "C" fn(),
    /// OpenXRManager::SetWeaponAimEnable.
    set_weapon_aim_enable: extern "C" fn(i32),
}
// SAFETY: the table holds plain functions.
unsafe impl Sync for ControlsRuntime {}
static CONTROLS_RT: ControlsRuntime = ControlsRuntime {
    calibration_step: crate::fbt_calibration::cvr_cpp_fbt_calibration_step,
    calibration_available: crate::fbt_calibration::calibration_available,
    start_calibration: crate::fbt_calibration::start_calibration,
    cancel_calibration: crate::fbt_calibration::cvr_cpp_fbt_cancel_calibration,
    set_weapon_aim_enable: xr::set_weapon_aim_enable,
};
/// The FULL-BODY TRACKING section (formerly cvr::fbt::game::DrawSettingsSection, Fbt/Game/Menu.cpp): the calibration's
/// button for its step (in game only), and the three switches, each needing the one before.
fn fbt_settings(rt: &ControlsRuntime) {
    use cvr_settings::fbt_settings::{enabled, set_enabled, ARMS, LEGS, SPINE};
    if !ui::collapsing_header(c"FULL-BODY TRACKING", DEFAULT_OPEN) {
        return;
    }
    match (rt.calibration_step)() {
        0 => {
            let available = (rt.calibration_available)();
            ui::begin_disabled(!available);
            if ui::button(c"Calibrate full-body tracking") {
                (rt.start_calibration)();
            }
            ui::tooltip(c"Three steps: look ahead at the horizon line, press both triggers;\nstand straight, arms along the body, look down, press both triggers;\nthen the T-pose on the cross until the countdown ends.\nNeeds the Vive Trackers on the waist and both feet.");
            ui::end_disabled();
            if !available {
                ui::text_colored([1.0, 0.75, 0.2, 1.0], c"Full-body calibration works in game only: load a save first.", &[]);
            }
        }
        1 => {
            if ui::button(c"Step 1: look ahead at the horizon line, then press both triggers (press to cancel)") {
                (rt.cancel_calibration)();
            }
            ui::tooltip(c"Release both triggers, take the pose, then pull both together.");
        }
        2 => {
            if ui::button(c"Step 2: arms down, look down, then press both triggers (press to cancel)") {
                (rt.cancel_calibration)();
            }
            ui::tooltip(c"Release both triggers, take the pose, then pull both together.");
        }
        _ => {
            if ui::button(c"Step 3: face the cross: head on its top, arms straight out along its arms, until the countdown ends (press to keep V's arms)") {
                (rt.cancel_calibration)();
            }
            ui::tooltip(c"Stand facing the cross; the dots turn green on their marks, then the countdown runs.");
        }
    }
    let mut legs = enabled(LEGS);
    if ui::checkbox(c"V's legs follow the trackers", &mut legs) {
        set_enabled(LEGS, legs);
    }
    ui::tooltip(c"V's hips and legs follow your waist and foot trackers (needs a calibration).\nThe spine, head and arms stay as before.");
    ui::begin_disabled(!legs);
    let mut spine = enabled(SPINE);
    if ui::checkbox(c"V's spine and head follow the headset", &mut spine) {
        set_enabled(SPINE, spine);
    }
    ui::tooltip(c"V's spine bends from your hips to your headset, V's eyes on your view (with the legs on).\nThe arms stay as before.");
    ui::begin_disabled(!spine);
    let mut arms = enabled(ARMS);
    if ui::checkbox(c"V's arms follow the controllers", &mut arms) {
        set_enabled(ARMS, arms);
    }
    ui::tooltip(c"V's arms reach your controllers with V's arm length set by the calibration (with the spine on).\nOff: the arms as before.");
    ui::end_disabled();
    ui::end_disabled();
}
/// The CONTROLS section's page: `static int page=0;` -- GENERAL, DRIVING, BINDINGS.
static CONTROLS_PAGE: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
/// The hand aim switch: `static bool s_weaponAim = true;` -- the backend's m_weaponAimEnable also starts at 1.
static WEAPON_AIM: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// DrawLiveControls' CONTROLS section: its pages -- the driving panel, the bindings, and the general controls:
/// handedness, finger tracking, full-body tracking, the weapon aim, vibration, the menu gesture and the weapon wheel,
/// the XInput merge, holsters, locomotion, swimming and crouch, ladders, turning.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_live_controls_controls(state: *mut LiveControlsUiState) -> bool {
    // SAFETY: attached at the C++ half's static initialisation.
    let rt = &CONTROLS_RT;
    let mut page = CONTROLS_PAGE.load(Relaxed);
    ui::tabs(
        c"controls-pages",
        &mut page,
        &[c"GENERAL", c"DRIVING", c"BINDINGS"],
    );
    CONTROLS_PAGE.store(page, Relaxed);
    if page == 1 {
        // SAFETY: the caller's snapshot.
        return unsafe { cvr_draw_driving_controls(state) };
    }
    if page == 2 {
        ui::draw_bindings();
        return false;
    }
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return false;
    };
    let mut changed = false;
    ui::text_wrapped(
        c"Movement, weapon aiming and physical interaction shortcuts.",
        &[],
    );

    changed |= checkbox_int(c"Left-handed controls", &mut st.xrLeftHanded);
    ui::tooltip(
        c"The two controllers trade roles: the left trigger fires and the right one aims,
the right stick walks and the left one turns, dashes and crouches.
X jumps, Y drops the magazine, A reloads and interacts, B switches weapons,
and the right stick click is the D-pad. Button prompts show these buttons.",
    );
    // How far the controllers trade (Hooks/Handedness.hpp): the triggers and grips always go with the weapon hand.
    if st.xrLeftHanded != 0 {
        changed |= ui::combo_z(
            c"Left-handed bindings",
            &mut st.xrLeftHandedBindings,
            b"Same as right-handed\0Left stick walks, buttons mirrored\0Left stick turns, buttons mirrored\0\0",
        );
        ui::tooltip(
            c"Same as right-handed: the buttons and sticks as right-handed; only the triggers
and grips go with the weapon hand.
Left stick walks: the buttons mirrored (X jumps, A reloads...), the sticks as they were.
Left stick turns: the buttons and the sticks mirrored.",
        );
    }
    ui::separator();
    // Finger tracking from OpenXR hand tracking (Runtimes/FingerGrip.hpp, Anim/HandGestures.hpp).
    if ui::collapsing_header(c"FINGER TRACKING", DEFAULT_OPEN) {
        changed |= checkbox_int(c"Enable finger tracking", &mut st.xrFingerBones);
        ui::tooltip(c"Each of V's fingers follows your own, from hand tracking (finger-tracking gloves, Index controllers).\nTurning it on applies at the next game start. Off: finger poses from the buttons.");
        ui::begin_disabled(st.xrFingerBones == 0);
        changed |= checkbox_int(
            c"Grip input from finger position instead of the sensor",
            &mut st.xrGripFromFingers,
        );
        ui::tooltip(c"The grip is a fist or a thumbs-up, from the fingers' position,\nnot the controller's grip sensor: a relaxed hand never grips.");
        changed |= checkbox_int(c"Enable finger spread", &mut st.xrFingerSpreadOn);
        ui::tooltip(c"V's fingers also spread sideways as yours do.\nOff: the fingers only bend, from the curl.");
        ui::begin_disabled(st.xrFingerSpreadOn == 0);
        changed |= ui::slider_float(c"Finger spread", &mut st.xrFingerSpread, 0.0, 2.0, c"%.2fx");
        ui::tooltip(
            c"V's fingers spread sideways as yours do.\n0: off, the fingers only curl. 1: default.",
        );
        ui::end_disabled();
        ui::end_disabled();
    }

    fbt_settings(rt); // formerly Fbt/Game/Menu.cpp

    // Weapon aim: bullets / projectiles fly down the weapon barrel (controller-pointed) instead of the camera
    // crosshair; the RED4ext plugin applies it.
    {
        let mut weapon_aim = WEAPON_AIM.load(Relaxed);
        if ui::checkbox(c"Hand aim  (off = Decoupled VR Head Aim)", &mut weapon_aim) {
            (rt.set_weapon_aim_enable)(weapon_aim as i32);
        }
        WEAPON_AIM.store(weapon_aim, Relaxed);
        ui::tooltip(
            c"ON (Hand Aim): the controller points the weapon and VRIK drives the arms.
OFF (Decoupled VR Head Aim): the WEAPON follows your head instead, the game
keeps owning its position and its ADS animations, and VRIK stands down for
the weapon arm. Either way the shot leaves the real muzzle, for guns and
projectiles alike, and free-look while aiming is preserved.",
        );
        changed |= ui::checkbox(
            c"Weapon Aim laser dot (where the bullet hits)",
            &mut st.xrWeaponLaserDot,
        );
        ui::tooltip(
            c"Red dot projected from the actual weapon muzzle direction through the
game camera -- marks exactly where the bullet will fly.",
        );
    }
    ui::separator();

    let route = &cvr_settings::haptic_route::HAPTIC_ROUTE;
    if cvr_xr::runtime_info::RUNTIME.psvr2() {
        changed |= ui::combo(
            c"PSVR2 haptics output (restart)",
            &mut st.xrPsvr2HapticBackend,
            &[c"OpenXR (standard vibration)", c"Prefer Toolkit (OpenXR fallback)", c"Auto (detect bridge at startup)"],
        );
        ui::text_wrapped(c"Restart the game after changing output. OpenXR: stop the Toolkit bridge. Toolkit: start the bridge before the game; provides advanced triggers and optional audio haptics.", &[]);
        ui::text(
            c"Active output: %s",
            &[Arg::S(if route.toolkit() { c"Toolkit bridge" } else { c"OpenXR" }.as_ptr())],
        );
        if route.pending(true) {
            ui::text_unformatted(c"Saved output will take effect after restarting the game.");
        }
        if !route.bridge_detected() && st.xrPsvr2HapticBackend != 0 {
            ui::text_wrapped(c"No compatible bridge listener detected at startup: using OpenXR. Start bridge BEFORE launching game for advanced haptics.", &[]);
        }
        if route.toolkit() {
            ui::text_wrapped(c"Vibration controls below affect port events. Toolkit audio is controlled separately below.", &[]);
        } else {
            ui::text_wrapped(c"OpenXR output requires working SteamVR haptic bindings. Toolkit-only trigger effects are not available in this mode.", &[]);
        }
    }
    if route.toolkit() {
        toolkit_audio_settings();
    }
    changed |= checkbox_int(c"Controller vibration", &mut st.xrHaptics);
    ui::tooltip(
        c"Haptics: weapon-strength recoil and physical melee swings,
melee block and parry, bullet block and deflect, reaching a belt element,
pulling a grenade pin, and audio-derived driving rumble.
Two-handed gun grip: weaker support-hand pulse, as in our earlier fork.",
    );
    ui::begin_disabled(st.xrHaptics == 0);
    changed |= ui::slider_float(
        c"Vibration strength",
        &mut st.xrHapticScale,
        0.0,
        2.0,
        c"%.2fx",
    );
    if !route.toolkit() {
        changed |= ui::slider_float(c"OpenXR gun/swing/road gain", &mut st.xrOpenXrHapticGain, 0.0, 2.0, c"%.2fx");
    }
    let mut vehicle_audio = st.xrOpenXrVehicleHapticGain > 0.0;
    if ui::checkbox(c"Driving road/engine haptics", &mut vehicle_audio) {
        st.xrOpenXrVehicleHapticGain = if vehicle_audio { 1.0 } else { 0.0 };
        changed = true;
    }
    changed |= ui::slider_float(
        c"Driving road/engine strength",
        &mut st.xrOpenXrVehicleHapticGain,
        0.0,
        2.0,
        c"%.2fx",
    );
    changed |= ui::combo(
        c"Driving audio channels",
        &mut st.xrVehicleAudioChannels,
        &[c"Stereo", c"Stronger channel to both hands", c"Left audio to both hands", c"Right audio to both hands"],
    );
    changed |= ui::checkbox(c"Driving audio only while each hand holds wheel/handlebar", &mut st.xrVehicleAudioRequireGrip);
    ui::text_wrapped(c"Unchecked: road/engine rumble continues with hands off while driving. Uses the active Toolkit or OpenXR output. Toolkit full-game audio above is independent; leave it off if you only want driving rumble.", &[]);
    ui::tooltip(c"Earlier fork's 28-320 Hz road/engine envelope, only while driving.\nDefault output audio (including music/other apps) can contribute. Zero disables vehicle rumble only.");
    if ui::button(c"Test left vibration") {
        // SAFETY: cvr-rig's haptic event under its C name: the belt touch, the left hand.
        unsafe { cvr_haptics_play(1, 1) };
    }
    ui::same_line();
    if ui::button(c"Test right vibration") {
        // SAFETY: as above, the right hand.
        unsafe { cvr_haptics_play(1, 2) };
    }
    ui::end_disabled();
    ui::separator();

    changed |= checkbox_int(c"Open the VR menu with a grip swipe", &mut st.xrMenuGesture);
    ui::tooltip(
        c"Hold the right grip and sweep the hand straight down to open or close the VR menu.
Off: only L3 + R3 (both sticks pressed) opens it.
A hand holding something (the steering wheel, a ladder, an item) never opens it.",
    );
    changed |= checkbox_int(c"VR weapon wheel on a held Y", &mut st.xrVrWeaponWheel);
    ui::tooltip(
        c"Hold Y: a small wheel opens in front of the body and the game keeps running.
Point at a weapon, the arm cyberware, fists, a grenade or a consumable, then let go of Y.
A tap of Y is still the next weapon. Off: the game's own wheel, which slows time.",
    );
    ui::separator();

    changed |= checkbox_int(c"Enable VR -> XInput merge", &mut st.xrXInputHook);
    ui::tooltip(
        c"OR the VR controller state into XInput gamepad 0 every poll.
Off = the game only sees a physical pad / nothing.",
    );

    ui::separator();
    ui::text_unformatted(c"Weapon holsters (reach + right grip)");
    changed |= checkbox_int(c"Immersive holsters", &mut st.xrImmersiveHolsters);
    ui::tooltip(
        c"ON  - every equipped weapon hangs on the body where it belongs:
      pistols on the right hip, blades and clubs on the left hip,
      rifles and shotguns over the shoulders, knives on the chest.
      Reach for one and squeeze the grip to draw it (a blade at the
      hip is pulled out along its handle); grip at its place to put it away.
OFF - no weapons on the body, fixed slot per zone:
      over-shoulder = EquipmentSlot1, right hip = Slot2, left hip = Slot3.",
    );

    ui::separator();
    ui::text_unformatted(c"Physical reload");
    changed |= checkbox_int(c"Reload highlight", &mut st.xrReloadGlow);
    ui::tooltip(c"On: the gun shows the free hand what it needs - an empty magazine pulses,\nthe place of a removed one stays lit, the bolt or the slide to rack glows.\nOff: no highlight; the reload itself works the same.");

    ui::separator();
    ui::text_unformatted(c"Locomotion direction");
    let mut analog_movement = st.xrMovementSpeedMode == 1;
    if ui::checkbox(c"Analog movement", &mut analog_movement) {
        st.xrMovementSpeedMode = analog_movement as i32;
        changed = true;
    }
    ui::tooltip(c"On: speed follows left-stick travel. Off: the current fixed-speed movement (default).\nVehicle controls keep their existing response.");
    ui::begin_disabled(!analog_movement);
    let mut left_deadzone = (st.xrLeftStickDeadzone * 100.0 + 0.5) as i32;
    let mut right_deadzone = (st.xrRightStickDeadzone * 100.0 + 0.5) as i32;
    let mut full_input = (st.xrMaxInputThreshold * 100.0 + 0.5) as i32;
    if ui::slider_int_fmt(c"Left stick deadzone", &mut left_deadzone, 0, 30, c"%d%%") {
        st.xrLeftStickDeadzone = left_deadzone as f32 * 0.01;
        changed = true;
    }
    if ui::slider_int_fmt(c"Right stick deadzone", &mut right_deadzone, 0, 30, c"%d%%") {
        st.xrRightStickDeadzone = right_deadzone as f32 * 0.01;
        changed = true;
    }
    if ui::slider_int_fmt(c"Full input threshold", &mut full_input, 80, 100, c"%d%%") {
        st.xrMaxInputThreshold = full_input as f32 * 0.01;
        changed = true;
    }
    ui::tooltip(c"Raw travel that produces full analog input and activates sprint, dash and crouch.\nThe usable travel between the deadzone and this threshold maps to 0-100%%.");
    ui::end_disabled();
    // How the sprint starts (Hooks/XInput.cpp, the sprint gesture).
    changed |= ui::combo_z(
        c"Sprint",
        &mut st.xrSprintMode,
        b"Hold the left stick forward\0Click the left stick\0\0",
    );
    ui::tooltip(
        c"Hold the left stick forward: the stick held at the stop for a moment sprints (default).
Click the left stick: a click sprints until the stick is let go.",
    );
    let mut move_source = st.xrMovementSource;
    if !(0..=3).contains(&move_source) {
        move_source = (st.xrMovementControl != 0) as i32;
    }
    if ui::combo(
        c"Move source",
        &mut move_source,
        &[c"Game (camera)", c"HMD (head)", c"Left hand", c"Right hand"],
    ) {
        st.xrMovementSource = move_source;
        st.xrMovementControl = (move_source != 0) as i32;
        changed = true;
    }
    ui::tooltip(
        c"Game     - left stick walks the way the camera faces (vanilla).
HMD      - left stick walks the way the headset faces.
Left/Right hand - walks the way the chosen controller points.
Vehicles always keep game heading.",
    );

    ui::separator();
    changed |= checkbox_int(c"Breaststroke swimming", &mut st.xrBreaststrokeSwim);
    changed |= checkbox_int(c"Physical crouch", &mut st.xrPhysicalCrouch);
    let mut crouch_cm = st.xrPhysicalCrouchDepth * 100.0;
    if ui::ui_slider_float(
        c"Physical crouch depth",
        &mut crouch_cm,
        15.0,
        55.0,
        c"%.0f cm",
    ) {
        st.xrPhysicalCrouchDepth = crouch_cm * 0.01;
        changed = true;
    }
    ui::tooltip(c"Height loss after the measured lean. Standing back up releases physical crouch.");
    // As in C++: the swimming tooltip follows the crouch depth's and replaces it.
    ui::tooltip(
        c"In water: reach forward with both hands, sweep out and pull back.
Movement begins during the pull. Gentle strokes swim normally; fast pulls boost.
Swim where the headset points, including up/down; no stick needed.
To rise, raise separated hands to chest level and push both down.
Repeat strokes to keep moving. Pull the left stick back to stop.",
    );
    ui::separator();
    changed |= checkbox_int(c"Ladder grip climbing", &mut st.xrLadderGripClimb);
    ui::tooltip(
        c"Grip a side rail or rung. Pull the hand down to climb up,
or move it up to climb down. Release to stop. Native pelvis and legs stay on the ladder.",
    );
    changed |= checkbox_int(c"Auto finish near ladder top", &mut st.xrLadderAutoFinish);
    ui::tooltip(
        c"After an upward pull near the top, finish climbing even when grips are released.
Move the stick or push a held hand upward to cancel.",
    );
    if st.xrLadderAutoFinish != 0 {
        changed |= ui::slider_float(
            c"Ladder finish distance",
            &mut st.xrLadderFinishDistance,
            0.2,
            1.2,
            c"%.2f m",
        );
    }
    ui::separator();
    ui::text_unformatted(c"Turning (right stick)");
    changed |= checkbox_int(c"Snap turn", &mut st.xrSnapTurn);
    ui::tooltip(
        c"Convert the right-stick X axis into discrete snap pulses
instead of smooth rotation. Helps with motion sickness.",
    );
    if st.xrSnapTurn != 0 {
        changed |= ui::slider_float(
            c"Snap angle",
            &mut st.xrSnapTurnAngleDeg,
            10.0,
            90.0,
            c"%.0f deg",
        );
    }
    changed
}

// ---- the live controls: STEREO, AVATAR and the panel ---------------------------------------------------------------

extern "C" {
    /// ImGuiOverlay.cpp: 1 skips the overlay's submission when nothing is drawn.
    static mut CyberpunkVR_OverlaySkipEmpty: i32;
    /// Stereo/EngineStereo.cpp: the mono switch and the eyes-take-turns switch, as the live controls set them.
    static mut CyberpunkVR_MonoMode: i32;
    static mut CyberpunkVR_Afw: i32;
    /// Hooks/Reflex: the mode the game's options block was last given; -2 before the first.
    fn cvr_reflex_get_applied_mode() -> i32;
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
/// cvr::engine_stereo::RestartPending: the running launch does not match the switches -- engine stereo, the way out
/// of a mono launch, eyes taking turns on or off need a restart.
fn restart_pending() -> bool {
    let afw = g!(CyberpunkVR_Afw) != 0;
    if afw != crate::afw::cvr_afw_launch() {
        return true;
    }
    if afw {
        return false;
    }
    let mono = g!(CyberpunkVR_MonoMode);
    ((crate::engine_stereo::cvr_engine_stereo_requested() && mono == 0)
        != crate::engine_stereo::cvr_engine_stereo_enabled())
        || (crate::engine_stereo::cvr_engine_stereo_mono_launch() && mono == 0)
}

/// DrawLiveControls' STEREO section: performance -- the empty-overlay pacing, NVIDIA Reflex, the FPS cap; the stereo
/// view -- mono, OSR / AFW and their statistics, the FOV control, prediction, separation, world and IPD scale, frame
/// reuse, the asymmetric projection; then the stereo panel.
fn draw_live_controls_stereo(st: &mut LiveControlsUiState) -> bool {
    let mut changed = false;
    crate::neural_render::draw_controls();
    if ui::collapsing_header(c"PERFORMANCE", DEFAULT_OPEN) {
        let mut stable_empty_pacing = g!(CyberpunkVR_OverlaySkipEmpty) == 0;
        if ui::checkbox(
            c"Stable pacing with no overlay graphics",
            &mut stable_empty_pacing,
        ) {
            s!(
                CyberpunkVR_OverlaySkipEmpty,
                if stable_empty_pacing { 0 } else { 1 }
            );
            let line = cvr_crt::format(
                c"[pacing] stable empty-overlay pacing=%d (skipEmpty=%d)\n",
                &[
                    Arg::I(stable_empty_pacing as i32),
                    Arg::I(g!(CyberpunkVR_OverlaySkipEmpty)),
                ],
            );
            log(&line.to_string_lossy());
        }
        ui::tooltip(
            c"Keep the previous-overlay fence pacing even when no overlay is drawn.
May improve headset delivery when mounting or holstering. Adds a small
empty command submission. Uncheck for upstream behavior; resets ON at launch.",
        );

        let mut reflex = st.nvidiaReflex + 1;
        if ui::combo_z(
            c"NVIDIA Reflex",
            &mut reflex,
            b"Game setting\0Off (higher FPS)\0On (lower latency)\0On + Boost\0\0",
        ) {
            st.nvidiaReflex = reflex - 1;
            changed = true;
        }
        ui::text_wrapped(
            c"Off can improve GPU utilization and FPS, but may increase input latency.",
            &[],
        );
        // SAFETY: a plain query.
        let applied = unsafe { cvr_reflex_get_applied_mode() };
        let applied_name = match applied {
            0 => c"Off",
            1 => c"On",
            2 => c"On + Boost",
            _ => c"Waiting for renderer",
        };
        ui::text_disabled(c"Applied: %s", &[Arg::S(applied_name.as_ptr())]);
        changed |= checkbox_int(c"Limit FPS to the headset refresh rate", &mut st.fpsCap);
        ui::tooltip(
            c"Frames above the headset's refresh rate are never shown: they only load the GPU
and make the picture stutter now and then. Off lets the game run free.",
        );
    }
    if ui::collapsing_header(c"STEREO VIEW", DEFAULT_OPEN) {
        changed |= checkbox_int(c"Mono mode", &mut st.monoMode);
        ui::tooltip(
            c"One picture for both eyes, drawn once from the centre of the head:
no depth, the most frames. Engine stereo and the second view are off.",
        );
        // OSR and AFW (the eyes in turn) are no longer offered here (2026-11-24): their ini keys, xr_osr and xr_afw,
        // still start them for development; the launch's state is shown while one runs.
        if crate::afw::cvr_afw_launch() {
            let source = if crate::afw::cvr_afw_uses_osr() { c"OSR" } else { c"AFW" };
            ui::text_disabled(c"%s on from the ini (xr_osr / xr_afw)", &[Arg::S(source.as_ptr())]);
        }
        // Engine stereo is sized into the window at start-up and a launch in mono has no second view, so those
        // directions wait for the next launch (Stereo/EngineStereo.hpp ApplyMonoMode, RestartPending).
        if restart_pending() {
            ui::text_colored([1.0, 0.8, 0.35, 1.0], c"Restart the game to apply", &[]);
        }
        // SAFETY: the caller's snapshot.
        changed |= unsafe { cvr_draw_fov_control(st) };
        changed |= ui::slider_float(
            c"Motion prediction (ms)",
            &mut st.xrMotionPredictMs,
            0.0,
            60.0,
            c"%.1f ms",
        );
        ui::tooltip(
            c"Forward-predicts the head pose by this many ms using head
velocity, hiding render-to-photon latency. 0 = off.
Tune up until motion feels responsive without overshoot.",
        );
        changed |= ui::slider_float(
            c"Stereo separation x",
            &mut st.xrStereoScale,
            0.25,
            5.0,
            c"%.2fx",
        );
        ui::tooltip(
            c"Personal fine-tune on the auto IPD. 1.0 = calibrated natural
separation, auto-scaled to the headset's runtime IPD. Nudge
0.8-1.2 for taste; crank to 3-5x to exaggerate depth and make
the eye alternation obvious on the flat monitor for testing.",
        );
        // World scale and the honest IPD.
        changed |= ui::slider_float(c"World scale", &mut st.xrWorldScale, 0.20, 3.0, c"%.2f");
        ui::tooltip(
            c"Scales eye separation AND head translation together.
Lower it (e.g. 0.8) to make the world look
BIGGER / yourself smaller; raise to shrink the world. Use this if V
and NPCs feel too large.",
        );
        changed |= ui::slider_float(c"IPD scale", &mut st.xrIpdScale, 0.50, 2.0, c"%.2fx");
        ui::tooltip(
            c"Eye-separation multiplier on the runtime IPD. 1.0 = the neutral
baseline (+-0.033 m on a typical headset).
Affects stereo depth (diorama vs giant), NOT the monocular size.",
        );
        let mut reuse_last_frame = st.xrReuseLastFrame != 0;
        if ui::checkbox(c"Reuse last clean frame", &mut reuse_last_frame) {
            st.xrReuseLastFrame = reuse_last_frame as i32;
            changed = true;
        }
        ui::tooltip(
            c"On stale ticks, re-submit the last clean captured eye and let
the compositor reproject it, instead of warping stale content
again. May lower submit rate
toward the capture rate. Off = always warp the stale eye.",
        );
        // The "Pose pair-lock" checkbox is gone: it toggled a freeze that only meant something under AER's one-camera
        // eye alternation. The ini key still round-trips so old files load.
        changed |= checkbox_int(c"Asymmetric projection", &mut st.xrAsymProjection);
        ui::tooltip(
            c"ON: each eye renders exactly its own off-centre lens window
(more of the panel used, square pixels).
OFF: the symmetric frustum sized to the widest half-angle.",
        );
    }

    cvr_draw_stereo_controls();
    changed
}

/// DrawLiveControls (Overlay/OverlayInternal.hpp): recenter and save, then the sections -- all of them as tabs
/// (`section` < 0, the desktop overlay), or the one the VR menu's page asks for (0 GENERAL, 1 FRAMEGEN, 2 HUD,
/// 3 CONTROLS, 4 STEREO, 5 AVATAR). True when the snapshot changed.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState; `foveated_editing` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_live_controls(
    state: *mut LiveControlsUiState,
    section: i32,
    foveated_editing: *mut bool,
) -> bool {
    let mut changed = false;
    // SAFETY: the caller's flag, or null.
    if let Some(editing) = unsafe { foveated_editing.as_mut() } {
        *editing = false;
    }

    if ui::button(c"Recenter HMD (F7)") {
        crate::live_controls_poll::RequestLiveControlsRecenter();
    }
    ui::same_line();
    if ui::button(c"Save") {
        // SAFETY: the caller's snapshot.
        unsafe { crate::live_controls_ui::SetLiveControlsUiState(state, 1) };
    }

    let tabs = section < 0;
    if !tabs || ui::begin_tab_bar(c"CyberpunkVRPortTabs") {
        let open = |index: i32, label: &core::ffi::CStr| {
            if tabs {
                ui::begin_tab_item(label)
            } else {
                section == index
            }
        };
        let close = || {
            if tabs {
                ui::end_tab_item();
            }
        };
        if open(0, c"GENERAL") {
            // SAFETY: the caller's snapshot and flag.
            changed |= unsafe { cvr_draw_live_controls_general(state, foveated_editing) };
            close();
        }
        if open(1, c"FRAMEGEN") {
            // SAFETY: the caller's snapshot.
            changed |= unsafe { cvr_draw_live_controls_framegen(state) };
            close();
        }
        if open(2, c"HUD") {
            // SAFETY: the caller's snapshot.
            changed |= unsafe { cvr_draw_live_controls_hud(state) };
            close();
        }
        if open(3, c"CONTROLS") {
            // SAFETY: the caller's snapshot.
            changed |= unsafe { cvr_draw_live_controls_controls(state) };
            close();
        }
        if open(4, c"STEREO") {
            // SAFETY: the caller's snapshot.
            if let Some(st) = unsafe { state.as_mut() } {
                changed |= draw_live_controls_stereo(st);
            }
            close();
        }
        if open(5, c"AVATAR") {
            cvr_draw_vr_hands_controls();
            close();
        }
        if tabs {
            ui::end_tab_bar();
        }
    }
    changed
}
