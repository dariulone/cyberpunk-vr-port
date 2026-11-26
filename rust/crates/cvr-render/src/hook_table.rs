//! The hooks whose install (and gate) functions are Rust's, registered from Rust (formerly each one's
//! `CVR_HOOK(...)` line in its C++ file, include/Hooks/Hook.hpp): the C++ half's Registry.cpp calls
//! cvr_hooks_register_rust from its static initialization, beside the hooks still registered by C++ constructors. The
//! install sequence is cvr-engine's stages.rs's (an explicit one), not the registration order.

use cvr_engine::stages::{register, EnabledFn, InstallFn};
use cvr_settings::lc;

// Plain calls without arguments: safe to call as the C++ registry did.
unsafe extern "C" {
    safe fn cvr_hook_xinput_install() -> bool;
    safe fn cvr_hook_job_timeline_install() -> bool;
    safe fn cvr_srt_install_partitions() -> bool;
    safe fn cvr_hud_natives_install_hooks() -> bool;
    safe fn cvr_world_markers_install() -> bool;
    safe fn cvr_weapon_aim_install() -> bool;
    safe fn cvr_hook_aftermath_install() -> bool;
    safe fn cvr_hook_aftermath_wanted() -> bool;
    safe fn cvr_hook_background_pad_install() -> bool;
    safe fn cvr_hook_body_heading_install() -> bool;
    safe fn cvr_hook_body_yaw_census_install() -> bool;
    safe fn cvr_hook_camera_fov_install() -> bool;
    safe fn cvr_hook_camera_pitch_install() -> bool;
    safe fn cvr_hook_dlss_res_install() -> bool;
    safe fn cvr_hook_final_camera_install() -> bool;
    safe fn cvr_hook_force_heading_update_install() -> bool;
    safe fn cvr_hook_free_delta_head_install() -> bool;
    safe fn cvr_hook_keypad_cursor_install() -> bool;
    safe fn cvr_hook_left_handed_animation_install() -> bool;
    safe fn cvr_hook_locate_camera_install() -> bool;
    safe fn cvr_hook_lod_fov_install() -> bool;
    safe fn cvr_hook_lod_fov_threshold_install() -> bool;
    safe fn cvr_hook_menu_mode_install() -> bool;
    safe fn cvr_hook_menu_surface_install() -> bool;
    safe fn cvr_hook_on_foot_delta_head_install() -> bool;
    safe fn cvr_hook_on_foot_move_xy_install() -> bool;
    safe fn cvr_hook_panzer_steering_install() -> bool;
    safe fn cvr_hook_panzer_weapon_aim_install() -> bool;
    safe fn cvr_hook_patch_buffer_install() -> bool;
    safe fn cvr_hook_patch_buffer_wanted() -> bool;
    safe fn cvr_hook_patch_camera_install() -> bool;
    safe fn cvr_hook_photo_mode_capture_install() -> bool;
    safe fn cvr_hook_physical_optics_surface_install() -> bool;
    safe fn cvr_hook_pose_transport_install() -> bool;
    safe fn cvr_hook_proj_aspect_call_install() -> bool;
    safe fn cvr_hook_proj_aspect_call_wanted() -> bool;
    safe fn cvr_hook_proj_aspect_copy_install() -> bool;
    safe fn cvr_hook_proj_aspect_copy_wanted() -> bool;
    safe fn cvr_hook_proj_stage_install() -> bool;
    safe fn cvr_hook_projection_commit_install() -> bool;
    safe fn cvr_hook_prompt_icons_install() -> bool;
    safe fn cvr_hook_rebind_census_install() -> bool;
    safe fn cvr_hook_reflex_install() -> bool;
    safe fn cvr_hook_roomscale_install() -> bool;
    safe fn cvr_hook_scope_graph_dag_install() -> bool;
    safe fn cvr_hook_settings_res_install() -> bool;
    safe fn cvr_hook_story_attention_install() -> bool;
    safe fn cvr_hook_surveillance_body_install() -> bool;
    safe fn cvr_hook_swimming_actions_install() -> bool;
    safe fn cvr_hook_vendor_route_install() -> bool;
    safe fn cvr_hook_menu_tooltip_install() -> bool;
    safe fn cvr_hook_view_key_install() -> bool;
    safe fn cvr_hook_view_key_wanted() -> bool;
    safe fn cvr_hook_world_blend_install() -> bool;
    safe fn cvr_hook_world_touch_install() -> bool;
    safe fn cvr_hook_held_body_install() -> bool;
    safe fn cvr_hook_physx_step_install() -> bool;
    safe fn cvr_hook_dynamic_texture_origins_install() -> bool;
}

/// XInputWanted: the live controls' xrXInputInstall.
extern "C" fn xinput_wanted() -> bool {
    lc!(xrXInputInstall) as i64 != 0
}
/// JobTimelineWanted: CVR_JOB_TIMELINE set.
extern "C" fn job_timeline_wanted() -> bool {
    std::env::var_os("CVR_JOB_TIMELINE").is_some()
}
/// The weapon aim family's KILL SWITCH, kept because it earned its place (formerly WeaponAim.cpp's
/// kEnableWeaponAimFamily): 0.1.2 shipped a frame-rate collapse near security cameras and turrets isolated by setting it
/// to 0; the cause was the check order in the physical ray origin hook, fixed there. Leave it at 1: at 0 the log says
/// "[hooks] boot WeaponAim skipped (disabled)", at the cost of bullets from the muzzle and the hitscan hand recoil.
const ENABLE_WEAPON_AIM_FAMILY: i32 = 1;
extern "C" fn weapon_aim_wanted() -> bool {
    ENABLE_WEAPON_AIM_FAMILY != 0
}

/// Every Rust hook's registration (name, stage, order, install, gate).
#[no_mangle]
pub extern "C" fn cvr_hooks_register_rust() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let hooks: [(&'static str, i32, i32, InstallFn, Option<EnabledFn>); 56] = [
        ("AftermathDiagnostics", 0, 5, cvr_hook_aftermath_install as InstallFn, Some(cvr_hook_aftermath_wanted as EnabledFn)),
        ("ViewKey", 2, 10, cvr_hook_view_key_install as InstallFn, Some(cvr_hook_view_key_wanted as EnabledFn)),
        ("LocateCamera", 1, 10, cvr_hook_locate_camera_install as InstallFn, None),
        ("PatchCamera", 1, 12, cvr_hook_patch_camera_install as InstallFn, None),
        ("FinalCamera", 1, 14, cvr_hook_final_camera_install as InstallFn, None),
        ("CameraPitch", 1, 30, cvr_hook_camera_pitch_install as InstallFn, None),
        ("CameraFov", 1, 35, cvr_hook_camera_fov_install as InstallFn, None),
        ("ProjectionCommit", 1, 45, cvr_hook_projection_commit_install as InstallFn, None),
        ("ScopeGraphDag", 1, 50, cvr_hook_scope_graph_dag_install as InstallFn, None),
        ("ProjAspectCopy", 1, 50, cvr_hook_proj_aspect_copy_install as InstallFn, Some(cvr_hook_proj_aspect_copy_wanted as EnabledFn)),
        ("ProjAspectCall", 1, 51, cvr_hook_proj_aspect_call_install as InstallFn, Some(cvr_hook_proj_aspect_call_wanted as EnabledFn)),
        ("ProjStage", 1, 55, cvr_hook_proj_stage_install as InstallFn, None),
        ("RebindCensus", 1, 60, cvr_hook_rebind_census_install as InstallFn, None),
        ("LodFov", 1, 60, cvr_hook_lod_fov_install as InstallFn, None),
        ("LodFovThreshold", 1, 61, cvr_hook_lod_fov_threshold_install as InstallFn, None),
        ("BodyYawCensus", 1, 61, cvr_hook_body_yaw_census_install as InstallFn, None),
        ("MenuMode", 1, 70, cvr_hook_menu_mode_install as InstallFn, None),
        ("ForceHeadingUpdate", 1, 75, cvr_hook_force_heading_update_install as InstallFn, None),
        ("OnFootDeltaHead", 1, 76, cvr_hook_on_foot_delta_head_install as InstallFn, None),
        ("RoomscaleMovement", 1, 77, cvr_hook_roomscale_install as InstallFn, None),
        ("FreeDeltaHead", 1, 77, cvr_hook_free_delta_head_install as InstallFn, None),
        ("OnFootMoveXY", 1, 78, cvr_hook_on_foot_move_xy_install as InstallFn, None),
        ("BodyHeading", 1, 79, cvr_hook_body_heading_install as InstallFn, None),
        ("PatchBuffer", 1, 80, cvr_hook_patch_buffer_install as InstallFn, Some(cvr_hook_patch_buffer_wanted as EnabledFn)),
        ("CameraPoseTransport", 1, 81, cvr_hook_pose_transport_install as InstallFn, None),
        ("WorldSpaceBlendCamera", 1, 82, cvr_hook_world_blend_install as InstallFn, None),
        ("PhysicalOpticsSurface", 1, 83, cvr_hook_physical_optics_surface_install as InstallFn, None),
        ("MenuSurface", 1, 83, cvr_hook_menu_surface_install as InstallFn, None),
        ("PanzerSteering", 1, 83, cvr_hook_panzer_steering_install as InstallFn, None),
        ("VendorRoute", 1, 84, cvr_hook_vendor_route_install as InstallFn, None),
        ("MenuTooltip", 1, 84, cvr_hook_menu_tooltip_install as InstallFn, None),
        ("PromptIcons", 1, 84, cvr_hook_prompt_icons_install as InstallFn, None),
        ("PanzerWeaponAim", 1, 84, cvr_hook_panzer_weapon_aim_install as InstallFn, None),
        // main's 730f5d78a: after MenuSurface and PhysicalOpticsSurface (83) checked the factory's own prologue
        ("DynamicTextureOrigins", 1, 84, cvr_hook_dynamic_texture_origins_install as InstallFn, None),
        ("SettingsRes", 1, 85, cvr_hook_settings_res_install as InstallFn, None),
        // main's physics hands: the carried things' pictures (the instances' records) and the PhysX step
        ("HeldBody", 1, 84, cvr_hook_held_body_install as InstallFn, None),
        ("PhysXStep", 1, 85, cvr_hook_physx_step_install as InstallFn, None),
        ("PhotoModeCapture", 1, 86, cvr_hook_photo_mode_capture_install as InstallFn, None),
        ("BackgroundPad", 1, 86, cvr_hook_background_pad_install as InstallFn, None),
        ("KeypadCursor", 1, 87, cvr_hook_keypad_cursor_install as InstallFn, None),
        ("WorldTouch", 1, 88, cvr_hook_world_touch_install as InstallFn, None),
        ("SurveillanceBody", 1, 88, cvr_hook_surveillance_body_install as InstallFn, None),
        ("StoryAttention", 1, 89, cvr_hook_story_attention_install as InstallFn, None),
        ("NvidiaReflex", 1, 89, cvr_hook_reflex_install as InstallFn, None),
        ("DlssRes", 1, 90, cvr_hook_dlss_res_install as InstallFn, None),
        ("LeftHandedAnimation", 1, 91, cvr_hook_left_handed_animation_install as InstallFn, None),
        ("SwimmingActions", 1, 101, cvr_hook_swimming_actions_install as InstallFn, None),
            ("JobTimeline", 1, 5, cvr_hook_job_timeline_install as InstallFn, Some(job_timeline_wanted as EnabledFn)),
            ("VrcamGbufferPartitions", 1, 50, cvr_srt_install_partitions as InstallFn, None),
            ("WeaponAim", 1, 80, cvr_weapon_aim_install as InstallFn, Some(weapon_aim_wanted as EnabledFn)),
            ("HudPanel", 1, 82, cvr_hud_natives_install_hooks as InstallFn, None),
            ("WorldMarkers", 1, 83, cvr_world_markers_install as InstallFn, None),
            ("XInput", 1, 100, cvr_hook_xinput_install as InstallFn, Some(xinput_wanted as EnabledFn)),
            ("NativeSetterMetaWrite", 1, 95, crate::setter_tracers::install_meta_write as InstallFn, Some(crate::setter_tracers::tracers_wanted as EnabledFn)),
            ("NativeSetterMetaConsume", 1, 96, crate::setter_tracers::install_meta_consume as InstallFn, Some(crate::setter_tracers::tracers_wanted as EnabledFn)),
            ("NativeSetterClear", 1, 97, crate::setter_tracers::install_clear as InstallFn, Some(crate::setter_tracers::tracers_wanted as EnabledFn)),
        ];
        for (name, stage, order, install, enabled) in hooks {
            register(name, stage, order, Some(install), enabled);
        }
    });
}
