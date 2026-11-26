//! The gameplay action set (formerly the second half of OpenXRManager::Init): the hands' poses, the sticks, triggers,
//! grips and buttons, the haptic output, the Steam Frame's own buttons and the PSVR2 Triangle touch, and the bindings
//! suggested for every interaction profile the port knows. xr_input_actions=0 keeps only the grip poses (the
//! kill-switch for a runtime that chokes on the larger set).
//!
//! The Vive trackers' actions are created by their C++ owner in the same set (`cvr_cpp_xr_create_tracker_actions`).
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::{c_char, CStr};
use cvr_host::log;
use openxr_sys as xr;
use xr::Handle as _;

#[cfg(not(test))]
extern "system" {
    fn xrCreateActionSet(instance: xr::Instance, info: *const xr::ActionSetCreateInfo, set: *mut xr::ActionSet) -> xr::Result;
    fn xrCreateAction(set: xr::ActionSet, info: *const xr::ActionCreateInfo, action: *mut xr::Action) -> xr::Result;
    fn xrStringToPath(instance: xr::Instance, text: *const c_char, path: *mut xr::Path) -> xr::Result;
    fn xrSuggestInteractionProfileBindings(instance: xr::Instance, bindings: *const xr::InteractionProfileSuggestedBinding) -> xr::Result;
}
#[cfg(not(test))]
extern "C" {
    fn cvr_cpp_xr_create_tracker_actions(instance: u64, set: u64);
}

/// include/Runtimes/SteamFrameInput.hpp SteamFrameProfile.
const STEAM_FRAME_PROFILE: &CStr = c"/interaction_profiles/valve/frame_controller_valve";
const TOUCH: &CStr = c"/interaction_profiles/oculus/touch_controller";

/// The handles OpenXRManager keeps (each 0 when not created).
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct ActionSetup {
    pub action_set: u64,
    pub hand_paths: [u64; 2],
    pub hand_pose: u64,
    pub hand_aim_pose: u64,
    pub thumbstick: u64,
    pub trigger: u64,
    pub grip: u64,
    /// The squeeze's force past closed (Valve Index only).
    pub grip_force: u64,
    pub thumbstick_click: u64,
    pub primary_button: u64,
    pub secondary_button: u64,
    pub menu_button: u64,
    pub psvr2_triangle_touch: u64,
    pub haptic: u64,
    /// SteamFrameActions: x, y, up, down, left, right, view, bumper.
    pub steam_frame: [u64; 8],
    /// The Touch controllers' thumbrest touch (Quest 3: the D-pad's and the menu swipe's modifier).
    pub thumbrest_touch: u64,
}

impl ActionSetup {
    pub const NONE: ActionSetup = ActionSetup {
        action_set: 0,
        hand_paths: [0; 2],
        hand_pose: 0,
        hand_aim_pose: 0,
        thumbstick: 0,
        trigger: 0,
        grip: 0,
        grip_force: 0,
        thumbstick_click: 0,
        primary_button: 0,
        secondary_button: 0,
        menu_button: 0,
        psvr2_triangle_touch: 0,
        haptic: 0,
        steam_frame: [0; 8],
        thumbrest_touch: 0,
    };
}
/// The gameplay actions as created, for the frame thread (cleared with the action set at teardown).
pub static ACTIONS: std::sync::Mutex<ActionSetup> = std::sync::Mutex::new(ActionSetup::NONE);
/// The actions the frame thread works with.
pub fn actions() -> ActionSetup {
    *ACTIONS.lock().unwrap_or_else(|p| p.into_inner())
}

fn copy_name(dst: &mut [c_char], src: &str) {
    for (d, s) in dst.iter_mut().zip(src.bytes()) {
        *d = s as c_char;
    }
}

/// Creates the gameplay action set and its actions in `instance`, then suggests every profile's bindings.
///
/// # Safety
/// `instance` is the live instance; `out` the caller's ActionSetup.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_create_actions(instance: u64, input_actions: bool, psvr2: bool, steam_frame: bool, out: *mut ActionSetup) {
    // SAFETY (whole body): the loader's calls with structures of the types they take, the caller's setup.
    unsafe {
        let Some(out) = out.as_mut() else { return };
        *out = ActionSetup::default();
        let instance = xr::Instance::from_raw(instance);
        let mut info: xr::ActionSetCreateInfo = core::mem::zeroed();
        info.ty = xr::StructureType::ACTION_SET_CREATE_INFO;
        copy_name(&mut info.action_set_name, "gameplay");
        copy_name(&mut info.localized_action_set_name, "Gameplay");
        info.priority = 0;
        let mut set = xr::ActionSet::NULL;
        xrCreateActionSet(instance, &info, &mut set);
        let mut paths = [xr::Path::NULL; 2];
        xrStringToPath(instance, c"/user/hand/left".as_ptr(), &mut paths[0]);
        xrStringToPath(instance, c"/user/hand/right".as_ptr(), &mut paths[1]);
        let make = |kind: xr::ActionType, name: &str, localized: &str, per_hand: bool| -> u64 {
            let mut a: xr::ActionCreateInfo = core::mem::zeroed();
            a.ty = xr::StructureType::ACTION_CREATE_INFO;
            a.action_type = kind;
            copy_name(&mut a.action_name, name);
            copy_name(&mut a.localized_action_name, localized);
            if per_hand {
                a.count_subaction_paths = 2;
                a.subaction_paths = paths.as_ptr();
            }
            let mut action = xr::Action::NULL;
            xrCreateAction(set, &a, &mut action);
            action.into_raw()
        };
        out.hand_pose = make(xr::ActionType::POSE_INPUT, "hand_pose", "Hand Pose", true);
        // The aim pose has a runtime-stable forward (-Z = pointing): the hand locomotion walks where the player points.
        if input_actions {
            out.hand_aim_pose = make(xr::ActionType::POSE_INPUT, "hand_aim_pose", "Hand Aim Pose", true);
        }
        if input_actions {
            out.thumbstick = make(xr::ActionType::VECTOR2F_INPUT, "thumbstick", "Thumbstick", true);
            out.trigger = make(xr::ActionType::FLOAT_INPUT, "trigger", "Trigger", true);
            out.grip = make(xr::ActionType::FLOAT_INPUT, "grip", "Grip", true);
            out.grip_force = make(xr::ActionType::FLOAT_INPUT, "grip_force", "Grip Force", true);
            out.thumbstick_click = make(xr::ActionType::BOOLEAN_INPUT, "thumbstick_click", "Thumbstick Click", true);
            out.primary_button = make(xr::ActionType::BOOLEAN_INPUT, "primary_button", "Primary Button (A/X)", true);
            out.secondary_button = make(xr::ActionType::BOOLEAN_INPUT, "secondary_button", "Secondary Button (B/Y)", true);
            out.menu_button = make(
                xr::ActionType::BOOLEAN_INPUT,
                if psvr2 { "systembutton" } else { "menu" },
                if psvr2 { "SystemButton (Sense Create)" } else { "Menu Button" },
                false,
            );
            if psvr2 {
                out.psvr2_triangle_touch = make(xr::ActionType::BOOLEAN_INPUT, "secondary_button_touch", "Triangle Touch D-pad Shift", true);
            } else {
                out.thumbrest_touch = make(xr::ActionType::BOOLEAN_INPUT, "thumbrest_touch", "Thumbrest Touch (D-pad / menu modifier)", true);
            }
            // HAPTICS: an output action, bound inside each profile's one suggest call (a second call replaces them).
            out.haptic = make(xr::ActionType::VIBRATION_OUTPUT, "haptic", "Haptic Feedback", true);
            if steam_frame {
                let b = xr::ActionType::BOOLEAN_INPUT;
                out.steam_frame = [
                    make(b, "frame_x", "Frame X Button", false),
                    make(b, "frame_y", "Frame Y Button", false),
                    make(b, "frame_dpad_up", "Frame D-pad Up", false),
                    make(b, "frame_dpad_down", "Frame D-pad Down", false),
                    make(b, "frame_dpad_left", "Frame D-pad Left", false),
                    make(b, "frame_dpad_right", "Frame D-pad Right", false),
                    make(b, "frame_view", "Frame View Button", false),
                    make(b, "frame_bumper", "Frame Bumper", true),
                ];
            }
        }
        cvr_cpp_xr_create_tracker_actions(instance.into_raw(), set.into_raw());
        log!(
            "OpenXRManager[Input]: gameplay action set {} (xr_input_actions={})\n",
            if input_actions { "ENABLED" } else { "DISABLED (pose-only)" },
            input_actions as i32
        );
        out.action_set = set.into_raw();
        out.hand_paths = [paths[0].into_raw(), paths[1].into_raw()];

        let suggest = |profile: &CStr, list: &[(u64, &CStr)]| {
            let mut profile_path = xr::Path::NULL;
            if xrStringToPath(instance, profile.as_ptr(), &mut profile_path).into_raw() < 0 {
                return;
            }
            let mut v: Vec<xr::ActionSuggestedBinding> = Vec::with_capacity(list.len());
            for &(action, path) in list {
                let mut p = xr::Path::NULL;
                if xrStringToPath(instance, path.as_ptr(), &mut p).into_raw() >= 0 {
                    v.push(xr::ActionSuggestedBinding { action: xr::Action::from_raw(action), binding: p });
                }
            }
            if psvr2 && input_actions && profile == TOUCH {
                let mut triangle = xr::Path::NULL;
                if out.psvr2_triangle_touch != 0 && xrStringToPath(instance, c"/user/hand/left/input/y/touch".as_ptr(), &mut triangle).into_raw() >= 0 {
                    v.push(xr::ActionSuggestedBinding { action: xr::Action::from_raw(out.psvr2_triangle_touch), binding: triangle });
                }
            }
            let sb = xr::InteractionProfileSuggestedBinding {
                ty: xr::StructureType::INTERACTION_PROFILE_SUGGESTED_BINDING,
                next: core::ptr::null(),
                interaction_profile: profile_path,
                count_suggested_bindings: v.len() as u32,
                suggested_bindings: v.as_ptr(),
            };
            let r = xrSuggestInteractionProfileBindings(instance, &sb);
            log!("OpenXRManager[Input]: suggest bindings {} -> {} (count={})\n", profile.to_str().unwrap_or(""), r.into_raw(), v.len());
        };

        let pose = out.hand_pose;
        if !input_actions {
            // Pose-only: the grip-pose pair, for every profile.
            let pose_only = [(pose, c"/user/hand/left/input/grip/pose"), (pose, c"/user/hand/right/input/grip/pose")];
            for profile in [
                TOUCH,
                c"/interaction_profiles/valve/index_controller",
                c"/interaction_profiles/htc/vive_controller",
                c"/interaction_profiles/microsoft/motion_controller",
                c"/interaction_profiles/khr/simple_controller",
            ] {
                suggest(profile, &pose_only);
            }
            if steam_frame {
                suggest(STEAM_FRAME_PROFILE, &pose_only);
            }
            return;
        }
        let (aim, stick, trigger, grip, click) = (out.hand_aim_pose, out.thumbstick, out.trigger, out.grip, out.thumbstick_click);
        let (primary, secondary, menu, haptic) = (out.primary_button, out.secondary_button, out.menu_button, out.haptic);
        let f = out.steam_frame;
        // Frame: all four face buttons on the right, a left D-pad, separate bumpers; grips stay the VR gestures'.
        if steam_frame {
            suggest(
                STEAM_FRAME_PROFILE,
                &[
                    (pose, c"/user/hand/left/input/grip/pose"),
                    (pose, c"/user/hand/right/input/grip/pose"),
                    (aim, c"/user/hand/left/input/aim/pose"),
                    (aim, c"/user/hand/right/input/aim/pose"),
                    (stick, c"/user/hand/left/input/thumbstick"),
                    (stick, c"/user/hand/right/input/thumbstick"),
                    (click, c"/user/hand/left/input/thumbstick/click"),
                    (click, c"/user/hand/right/input/thumbstick/click"),
                    (trigger, c"/user/hand/left/input/trigger/value"),
                    (trigger, c"/user/hand/right/input/trigger/value"),
                    (grip, c"/user/hand/left/input/squeeze/value"),
                    (grip, c"/user/hand/right/input/squeeze/value"),
                    (primary, c"/user/hand/right/input/a/click"),
                    (secondary, c"/user/hand/right/input/b/click"),
                    (f[0], c"/user/hand/right/input/x/click"),
                    (f[1], c"/user/hand/right/input/y/click"),
                    (menu, c"/user/hand/right/input/menu/click"),
                    (f[2], c"/user/hand/left/input/dpad_up/click"),
                    (f[3], c"/user/hand/left/input/dpad_down/click"),
                    (f[4], c"/user/hand/left/input/dpad_left/click"),
                    (f[5], c"/user/hand/left/input/dpad_right/click"),
                    (f[6], c"/user/hand/left/input/view/click"),
                    (f[7], c"/user/hand/left/input/bumper/click"),
                    (f[7], c"/user/hand/right/input/bumper/click"),
                ],
            );
        }
        // Oculus Touch (Quest/Rift): X/Y on the left, A/B on the right, menu = the left menu button.
        suggest(
            TOUCH,
            &[
                (pose, c"/user/hand/left/input/grip/pose"),
                (pose, c"/user/hand/right/input/grip/pose"),
                (aim, c"/user/hand/left/input/aim/pose"),
                (aim, c"/user/hand/right/input/aim/pose"),
                (stick, c"/user/hand/left/input/thumbstick"),
                (stick, c"/user/hand/right/input/thumbstick"),
                (click, c"/user/hand/left/input/thumbstick/click"),
                (click, c"/user/hand/right/input/thumbstick/click"),
                (trigger, c"/user/hand/left/input/trigger/value"),
                (trigger, c"/user/hand/right/input/trigger/value"),
                (grip, c"/user/hand/left/input/squeeze/value"),
                (grip, c"/user/hand/right/input/squeeze/value"),
                (primary, c"/user/hand/left/input/x/click"),
                (primary, c"/user/hand/right/input/a/click"),
                (secondary, c"/user/hand/left/input/y/click"),
                (secondary, c"/user/hand/right/input/b/click"),
                (menu, c"/user/hand/left/input/menu/click"),
                (haptic, c"/user/hand/left/output/haptic"),
                (haptic, c"/user/hand/right/output/haptic"),
                (out.thumbrest_touch, c"/user/hand/left/input/thumbrest/touch"),
                (out.thumbrest_touch, c"/user/hand/right/input/thumbrest/touch"),
            ],
        );
        // Valve Index: A/B on both hands, system as menu.
        suggest(
            c"/interaction_profiles/valve/index_controller",
            &[
                (pose, c"/user/hand/left/input/grip/pose"),
                (pose, c"/user/hand/right/input/grip/pose"),
                (aim, c"/user/hand/left/input/aim/pose"),
                (aim, c"/user/hand/right/input/aim/pose"),
                (stick, c"/user/hand/left/input/thumbstick"),
                (stick, c"/user/hand/right/input/thumbstick"),
                (click, c"/user/hand/left/input/thumbstick/click"),
                (click, c"/user/hand/right/input/thumbstick/click"),
                (trigger, c"/user/hand/left/input/trigger/value"),
                (trigger, c"/user/hand/right/input/trigger/value"),
                (grip, c"/user/hand/left/input/squeeze/value"),
                (grip, c"/user/hand/right/input/squeeze/value"),
                (out.grip_force, c"/user/hand/left/input/squeeze/force"),
                (out.grip_force, c"/user/hand/right/input/squeeze/force"),
                (primary, c"/user/hand/left/input/a/click"),
                (primary, c"/user/hand/right/input/a/click"),
                (secondary, c"/user/hand/left/input/b/click"),
                (secondary, c"/user/hand/right/input/b/click"),
                (menu, c"/user/hand/left/input/system/click"),
                (haptic, c"/user/hand/left/output/haptic"),
                (haptic, c"/user/hand/right/output/haptic"),
            ],
        );
        // HTC Vive wand: no A/B/X/Y, the touchpad as the stick, the grip a button.
        suggest(
            c"/interaction_profiles/htc/vive_controller",
            &[
                (pose, c"/user/hand/left/input/grip/pose"),
                (pose, c"/user/hand/right/input/grip/pose"),
                (aim, c"/user/hand/left/input/aim/pose"),
                (aim, c"/user/hand/right/input/aim/pose"),
                (stick, c"/user/hand/left/input/trackpad"),
                (stick, c"/user/hand/right/input/trackpad"),
                (click, c"/user/hand/left/input/trackpad/click"),
                (click, c"/user/hand/right/input/trackpad/click"),
                (trigger, c"/user/hand/left/input/trigger/value"),
                (trigger, c"/user/hand/right/input/trigger/value"),
                (menu, c"/user/hand/left/input/menu/click"),
                (haptic, c"/user/hand/left/output/haptic"),
                (haptic, c"/user/hand/right/output/haptic"),
            ],
        );
        // Windows MR motion controller: the trackpad and thumbstick combination.
        suggest(
            c"/interaction_profiles/microsoft/motion_controller",
            &[
                (pose, c"/user/hand/left/input/grip/pose"),
                (pose, c"/user/hand/right/input/grip/pose"),
                (aim, c"/user/hand/left/input/aim/pose"),
                (aim, c"/user/hand/right/input/aim/pose"),
                (stick, c"/user/hand/left/input/thumbstick"),
                (stick, c"/user/hand/right/input/thumbstick"),
                (click, c"/user/hand/left/input/thumbstick/click"),
                (click, c"/user/hand/right/input/thumbstick/click"),
                (trigger, c"/user/hand/left/input/trigger/value"),
                (trigger, c"/user/hand/right/input/trigger/value"),
                (menu, c"/user/hand/left/input/menu/click"),
                (haptic, c"/user/hand/left/output/haptic"),
                (haptic, c"/user/hand/right/output/haptic"),
            ],
        );
        // KHR simple controller (the fallback): select, menu and the grip pose.
        suggest(
            c"/interaction_profiles/khr/simple_controller",
            &[
                (pose, c"/user/hand/left/input/grip/pose"),
                (pose, c"/user/hand/right/input/grip/pose"),
                (primary, c"/user/hand/left/input/select/click"),
                (primary, c"/user/hand/right/input/select/click"),
                (menu, c"/user/hand/left/input/menu/click"),
                (haptic, c"/user/hand/left/output/haptic"),
                (haptic, c"/user/hand/right/output/haptic"),
            ],
        );
        *ACTIONS.lock().unwrap_or_else(|p| p.into_inner()) = *out;
    }
}
