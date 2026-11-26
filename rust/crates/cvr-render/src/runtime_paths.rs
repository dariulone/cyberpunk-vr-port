//! WHERE THIS BUILD'S FILES ARE (formerly src/Core/LauncherConfig.cpp: InitRuntimePaths and
//! EnsureLiveControlFileExists): the game's folder from the loaded module, the settings files in the plugin's config
//! folder, the VRIK mod's bridge files in its Lua folder, the live controls' launch defaults, the CET recenter
//! counter's baseline captured before CET can write it; and vrport.ini written with its defaults when it is missing --
//! the overlay and the launcher both read it, and a file that exists with defaults in it is inspectable where an
//! absent one is not.
//!
//! The paths stay the C++ half's char arrays (dozens of readers), filled here through their MSVC names.
use core::ffi::{c_char, c_void};

use cvr_crt::{format, Arg};
use cvr_settings::lc_set;

const MAX_PATH: usize = 260;

/// The C++ half's paths and what it keeps of them.
#[repr(C)]
pub struct Runtime {
    game_dir: *mut c_char,
    live_control_path: *mut c_char,
    launcher_config_path: *mut c_char,
    vrik_settings_path: *mut c_char,
    vrik_recenter_path: *mut c_char,
    last_vrik_recenter_write: *mut [u32; 2],
    last_vrik_recenter_counter: *mut i32,
    /// foveated, VRS and hidden area: their default settings written into the new file.
    write_default_subsystems: extern "C" fn(*mut c_void),
}
#[allow(non_upper_case_globals)]
unsafe extern "C" {
    // src/Core/VrCore.cpp's arrays and the recenter baseline
    #[link_name = "?g_gameDir@@3PADA"]
    static mut g_gameDir: [c_char; MAX_PATH];
    #[link_name = "?g_liveControlPath@@3PADA"]
    static mut g_liveControlPath: [c_char; MAX_PATH];
    #[link_name = "?g_launcherConfigPath@@3PADA"]
    static mut g_launcherConfigPath: [c_char; MAX_PATH];
    #[link_name = "?g_vrikSettingsPath@@3PADA"]
    static mut g_vrikSettingsPath: [c_char; MAX_PATH];
    #[link_name = "?g_vrikRecenterPath@@3PADA"]
    static mut g_vrikRecenterPath: [c_char; MAX_PATH];
    #[link_name = "?g_lastVrikRecenterWrite@@3U_FILETIME@@A"]
    static mut g_lastVrikRecenterWrite: [u32; 2];
    #[link_name = "?g_lastVrikRecenterCounter@@3HA"]
    static mut g_lastVrikRecenterCounter: i32;
}
// SAFETY: the C++ half's arrays, alive for the process; written only by the init below, before any reader.
unsafe impl Sync for Runtime {}
static RT: Runtime = Runtime {
    game_dir: (&raw mut g_gameDir).cast(),
    live_control_path: (&raw mut g_liveControlPath).cast(),
    launcher_config_path: (&raw mut g_launcherConfigPath).cast(),
    vrik_settings_path: (&raw mut g_vrikSettingsPath).cast(),
    vrik_recenter_path: (&raw mut g_vrikRecenterPath).cast(),
    last_vrik_recenter_write: &raw mut g_lastVrikRecenterWrite,
    last_vrik_recenter_counter: &raw mut g_lastVrikRecenterCounter,
    write_default_subsystems,
};
/// Foveated DLSS, VRS and the hidden area write their default settings into the new file (formerly
/// LauncherConfig.cpp's, over the C++ names' `WriteSettings(file, {})`).
extern "C" fn write_default_subsystems(file: *mut c_void) {
    // SAFETY: the caller's open FILE.
    unsafe {
        crate::foveated::cvr_foveated_write_settings(file, crate::foveated::Settings::default());
        crate::vrs::cvr_vrs_write_settings(file, crate::vrs::Settings::default());
        crate::hidden_area::cvr_hidden_area_write_settings(
            file,
            crate::hidden_area::Settings { enabled: 0, radius: crate::hidden_area::DEFAULT_RADIUS },
        );
    }
}
/// g_liveControlPath (filled by cvr_init_runtime_paths).
pub fn live_control_path() -> *const c_char {
    rt().live_control_path
}
fn rt() -> &'static Runtime {
    &RT
}

extern "C" {
    /// The plugin's folder `folder` (Core/Paths.hpp order: 4 config, 8 lua), narrow, with its trailing separator.
    fn cvr_paths_dir_a(folder: u32) -> *const c_char;
}
const CONFIG: u32 = 4;
const LUA: u32 = 8;

/// strcpy_s into a MAX_PATH array of the C++ half (a longer path is cut short rather than overrunning it).
fn copy_path(dst: *mut c_char, folder: u32, name: &str) {
    // SAFETY: a NUL-terminated folder kept for the process.
    let mut bytes = unsafe { core::ffi::CStr::from_ptr(cvr_paths_dir_a(folder)) }
        .to_bytes()
        .to_vec();
    bytes.extend_from_slice(name.as_bytes());
    bytes.truncate(MAX_PATH - 1);
    // SAFETY: the C++ half's MAX_PATH array.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, dst, bytes.len());
        *dst.add(bytes.len()) = 0;
    }
}

/// InitRuntimePaths: once, the RED4ext plugin entry calling it in place of DllMain.
#[no_mangle]
pub extern "C" fn cvr_init_runtime_paths() {
    let r = rt();
    // SAFETY: the C++ half's array.
    if unsafe { *r.game_dir } != 0 {
        return;
    }
    // SAFETY: the exe's own path into the C++ half's MAX_PATH array, cut at its last separator.
    unsafe {
        use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameA;
        GetModuleFileNameA(
            core::ptr::null_mut(),
            r.game_dir as *mut u8,
            MAX_PATH as u32,
        );
        let path = core::slice::from_raw_parts(r.game_dir as *const u8, MAX_PATH);
        let len = path.iter().position(|&b| b == 0).unwrap_or(MAX_PATH);
        if let Some(slash) = path[..len].iter().rposition(|&b| b == b'\\') {
            *r.game_dir.add(slash) = 0;
        }
    }
    // The settings in the plugin's config folder; the VRIK mod's own files in its folder under the plugin's Lua mods
    // (the host sandboxes its IO there).
    copy_path(r.live_control_path, CONFIG, "vrport.ini");
    copy_path(r.launcher_config_path, CONFIG, "vrport-launcher.ini");
    copy_path(
        r.vrik_settings_path,
        LUA,
        "CyberpunkVRPort_VRIK\\vrik_settings.ini",
    );
    copy_path(
        r.vrik_recenter_path,
        LUA,
        "CyberpunkVRPort_VRIK\\vrik_recenter.ini",
    );

    // The launch defaults an ini without the key leaves in place: mouse pitch suppressed (the HMD has pitch), the
    // immersive holsters, no in-vehicle head offset of the user's (a car's view is CAR_CAMERA_AHEAD forward by
    // itself), the wheel grab with a hand-sized reach, full lock at the widest, no deadzone, no prediction, the horn by a
    // hand on the wheel's hub (the car's own ToggleHorn), driving with a gun and the throttle trim, VRIK suspended from
    // Tier 3 scenes on foot and true cinematics in a vehicle, the controller pipeline on, the sticks' deadzones.
    lc_set!(xrDisableMouseY, 1);
    lc_set!(xrImmersiveHolsters, 1);
    // The physical reload highlights the magazine and the bolt to rack.
    lc_set!(xrReloadGlow, 1);
    lc_set!(xrVehHeadOffsetX, 0.0);
    lc_set!(xrVehHeadOffsetY, 0.0);
    lc_set!(xrVehHeadOffsetZ, 0.0);
    lc_set!(xrBikeHeadOffsetX, 0.0);
    lc_set!(xrBikeHeadOffsetY, 0.0);
    lc_set!(xrBikeHeadOffsetZ, 0.0);
    lc_set!(xrWheelGrab, 1);
    lc_set!(xrWheelRadius, 0.28);
    lc_set!(xrWheelSteerMaxDeg, cvr_settings::wheel_range::WHEEL_FULL_LOCK_DEFAULT);
    lc_set!(xrBikeBarLockDeg, cvr_settings::wheel_range::BIKE_BAR_LOCK_DEFAULT);
    lc_set!(xrBikeBodyLean, cvr_settings::wheel_range::BIKE_BODY_LEAN_DEFAULT);
    lc_set!(xrWheelSteerDeadDeg, 0.0);
    lc_set!(xrWheelPrediction, 0);
    lc_set!(xrWheelPredictionMs, 8.0);
    lc_set!(xrWheelHorn, 1);
    lc_set!(xrWheelHornRadius, 0.12);
    lc_set!(xrVehicleGunTrigger, 1);
    lc_set!(xrVehicleThrottleTrim, 0.5);
    lc_set!(xrCutsceneSuspendTier, 2);
    lc_set!(xrVehicleCutsceneSuspendTier, 3);
    lc_set!(xrXInputInstall, 1);
    lc_set!(xrInputActions, 1);
    lc_set!(xrMovementSpeedMode, 0);
    lc_set!(xrSprintMode, 0);
    lc_set!(xrLeftStickDeadzone, 0.15);
    lc_set!(xrRightStickDeadzone, 0.15);
    lc_set!(xrMaxInputThreshold, 0.90);

    // The recenter request's baseline NOW, before CET could write: the first OnGameAttached this session is a change
    // and recenters, a stale counter from a previous session does not.
    if let Some(time) = crate::live_controls_poll::file_time(r.vrik_recenter_path) {
        // SAFETY: the C++ half's FILETIME.
        unsafe { *r.last_vrik_recenter_write = time };
        // SAFETY: the C++ half's path.
        if let Some(mut file) = unsafe { cvr_crt::File::open_shared(r.vrik_recenter_path) } {
            let mut line = [0 as c_char; 64];
            while file.gets_into(&mut line) {
                let mut v = 0i32;
                // SAFETY: a NUL-terminated line and the int written.
                if unsafe { cvr_crt::scan(line.as_ptr(), c"recenter=%d", &[cvr_crt::ptr(&mut v)]) }
                    == 1
                {
                    // SAFETY: the C++ half's int.
                    unsafe { *r.last_vrik_recenter_counter = v };
                    break;
                }
            }
        }
    }
}

/// EnsureLiveControlFileExists: vrport.ini with its defaults, only when there is none.
#[no_mangle]
pub extern "C" fn cvr_ensure_live_control_file_exists() {
    cvr_init_runtime_paths();
    let r = rt();
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesA, INVALID_FILE_ATTRIBUTES};
    // SAFETY: the C++ half's path.
    if unsafe { GetFileAttributesA(r.live_control_path as *const u8) } != INVALID_FILE_ATTRIBUTES {
        return;
    }
    // SAFETY: as above.
    let Some(mut file) = (unsafe { cvr_crt::File::open_shared_mode(r.live_control_path, c"w") })
    else {
        return;
    };
    let mut w = |text: &core::ffi::CStr| file.puts(text);
    w(c"xr_head_offset_x=0.000\n");
    w(c"xr_head_offset_y=0.000\n");
    w(c"xr_head_offset_z=0.000\n");
    w(c"xr_recenter=0\n");
    w(c"xr_mono_submit=1\n");
    w(c"xr_weapon_plane_as_scene=1\n");
    // Who owns the XR frame loop: -1 auto (the submit thread on SteamVR, the inline pump on Virtual Desktop), 0 force
    // inline, 1 force the thread.
    w(c"xr_threaded_submit=-1\n");
    w(c"xr_stage_replay=0\nxr_submit_chain=1\nxr_defer_present=1\nxr_gt_skip_render_queue=0\nxr_submit_chain_rev=1\n");
    w(c"xr_force_fov=0\n");
    w(c"xr_menu_rect=0\n");
    w(c"xr_menu_fov=65.0\n");
    w(c"xr_menu_follow_deg=60.0\n");
    w(c"xr_hud_stereo_depth=1\n");
    w(c"xr_3dof_movement=0\n");
    w(c"xr_roomscale_movement=1\n");
    w(c"xr_tracked_body_rotation=0\n");
    w(c"xr_hybrid_body_rotation=0\n");
    w(c"xr_head_led_body_rotation=1\n");
    w(c"xr_body_free_look_deg=10.0\n");
    w(c"xr_body_free_look_down_deg=30.0\n");
    w(c"xr_body_move_radius=0.10\n");
    w(c"xr_scene_cam_comfort_deg=30.0\n");
    w(c"xr_breaststroke_swim=1\n");
    w(c"xr_ladder_grip_climb=1\n");
    w(c"xr_left_handed=0\n");
    w(c"xr_left_handed_bindings=2\n");
    w(c"xr_ladder_auto_finish=1\nxr_ladder_finish_distance=1.0\n");
    w(c"xr_body_free_look_swim_deg=5\n");
    w(c"first_launch=1\n");
    w(c"xr_motion_predict_ms=0.0\n");
    w(c"xr_stereo_scale=1.0\n");
    w(c"xr_world_scale=1.0\n");
    w(c"xr_ipd_scale=1.0\n");
    w(c"xr_sharpness=0.0\n");
    w(&format(c"xr_sharpmix=%.1f\n", &[Arg::F(0.0)])); // cvr::sharpen::DefaultAmount
    (r.write_default_subsystems)(file.as_ptr());
    let mut w = |text: &core::ffi::CStr| file.puts(text);
    w(c"xr_mono_cascades=1\n");
    w(c"xr_scope_stabilize=80\n");
    // The game 2.5 stereo path (Stereo/EngineStereo.hpp); 0 = the port's own second view.
    w(c"xr_engine_stereo=1\n");
    w(c"xr_reuse_last_frame=0\n");
    w(c"xr_hmd_smooth=0.35\n");
    // UEVR's own default, in their units: follow-per-second times delta time where it is used. Per-class rates of
    // zero use this base rate.
    w(c"xr_hand_lerp_speed=25.0\n");
    w(c"xr_hand_lerp_melee=35.0\n");
    w(c"xr_hand_lerp_framegen_scale=1.10\n");
    w(c"xr_pair_lock=0\n");
    w(c"xr_render_pose_submit=1\n");
    w(c"xr_pose_lag=1\n");
    w(c"xr_runtime=0\n");
    w(&format(c"xr_nvidia_reflex=%d\n", &[Arg::I(0)])); // cvr::reflex::DefaultMode
    w(c"xr_local_shadow_reuse=0\n");
    // Only frame-matched, per-eye depth is submitted; synthetic frames omit it until their interpolation backend
    // supplies corresponding depth.
    w(c"xr_depth_submit=1\n");
    w(c"xr_movement_control=0\n");
    w(c"xr_disable_mouse_y=1\n");
    w(c"xr_vr_interface=1\n");
    w(c"xr_hand_physics=1\n");
    w(c"xr_xinput_hook=1\n");
    w(c"xr_snap_turn=0\n");
    w(c"xr_snap_turn_angle_deg=30\n");
    w(c"xr_movement_source=0\n");
    w(c"xr_sprint_mode=0\n");
    w(c"xr_movement_speed_mode=0\nxr_left_stick_deadzone=0.15\nxr_right_stick_deadzone=0.15\nxr_max_input_threshold=0.90\n");
    // Both flags on: the VR controller reaches the game as an XInput pad only with both.
    w(c"xr_xinput_install=1\n");
    w(c"xr_input_actions=1\n");
    w(c"xr_mono_xqueue_wait=0\n");
    w(c"xr_snap_turn_pulse_ms=30\n");
    w(c"xr_mono_depth_capture=1\n");
}
