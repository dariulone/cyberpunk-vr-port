//! Swimming by the arms (formerly src/Hooks/SwimmingInput.cpp): the player's swim state the scripts publish, both
//! hands' strokes turned into the native locomotion actions (MoveY, ToggleSprint, Jump, Dive) at the game's shared
//! CName action reader -- only this player's four reads, never a table entry or an input event -- and the heading the
//! roomscale move steers the stroke by. The reader's leaf contract is kept by the asm thunk
//! (src/Hooks/SwimmingActionLeafThunk.asm, CvrSwimmingActionDetour), which calls CvrSwimmingActionAfter here. The C++
//! API (cvr::swimming::*) forwards here from src/Hooks/WaterLadderRust.cpp.
use super::camera_link::HeadPose;
use super::env::g_isInVehicle;
use super::ladder_input::{cvr_ladder_active, cvr_ladder_mix_move};
use super::roomscale_move::{cvr_roomscale_player_identity, now_us};
use super::xinput::{SwimInput, VrController};
use core::ffi::c_void;
use cvr_body::cmath::max;
use cvr_body::swim::{forward_action, hand_in_tracking_space, in_water, presence_fresh, state_fresh, AscendStroke, Breaststroke, Sample, SprintButton};
use cvr_body::xr::{rotate_vector, Quat, Vec3};
use cvr_engine::code::{expect, Pattern};
use cvr_engine::hooks::{install_chain, Detour};
use std::sync::{LazyLock, Mutex};

extern "C" {
    static mut CvrSwimmingActionOriginal: *mut c_void;
    fn CvrSwimmingActionDetour();
    fn cvr_cpp_pose_scene_tier() -> i32;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_braindance_active() -> bool;
    fn cvr_cpp_ui_popup_open() -> i32;
    fn cvr_cpp_overlay_visible() -> bool;
    fn cvr_cpp_swimming_player_input_object(locomotion: *mut c_void) -> usize;
    fn cvr_cpp_swimming_script_hooks_active() -> bool;
}

const FORWARD: i32 = 0;
const BOOST: i32 = 1;
const ASCEND: i32 = 2;
const DIVE: i32 = 3;

const fn hash(text: &[u8]) -> u64 {
    let mut h = 14695981039346656037u64;
    let mut i = 0;
    while i < text.len() {
        h ^= text[i] as u64;
        h = h.wrapping_mul(1099511628211);
        i += 1;
    }
    h
}
const MOVE_X: u64 = hash(b"MoveX");
const MOVE_Y: u64 = hash(b"MoveY");
const TOGGLE_SPRINT: u64 = hash(b"ToggleSprint");
const SPRINT: u64 = hash(b"Sprint");
const JUMP: u64 = hash(b"Jump");
const DIVE_NAME: u64 = hash(b"Dive");

struct State {
    player: usize,
    stamp: u64,
    strokes: u64,
    state: i32,
    fast: i32,
    gesture: Breaststroke,
    ascent: AscendStroke,
    sprint: SprintButton,
    input: SwimInput,
    button: bool,
    tracking: bool,
    automatic: bool,
    paused: bool,
    water_context: bool,
    manual: bool,
    manual_sprint: bool,
    direction: Vec3,
    input_stamp: u64,
    ascents: u64,
    input_object: usize,
    input_owner: usize,
    input_owner_stamp: u64,
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        player: 0,
        stamp: 0,
        strokes: 0,
        state: -1,
        fast: -1,
        gesture: Breaststroke::default(),
        ascent: AscendStroke::default(),
        sprint: SprintButton::default(),
        input: SwimInput::default(),
        button: false,
        tracking: false,
        automatic: false,
        paused: false,
        water_context: false,
        manual: false,
        manual_sprint: false,
        direction: Vec3::default(),
        input_stamp: 0,
        ascents: 0,
        input_object: 0,
        input_owner: 0,
        input_owner_stamp: 0,
    })
});
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
impl State {
    fn fresh(&self, now: u64) -> bool {
        state_fresh(self.player, cvr_roomscale_player_identity(), self.stamp, now, self.state)
    }
    fn present(&self, now: u64) -> bool {
        presence_fresh(self.player, cvr_roomscale_player_identity(), self.stamp, now, self.state, self.water_context)
    }
    fn clear(&mut self) {
        self.gesture.reset();
        self.ascent.reset();
        self.sprint.reset();
        self.input = SwimInput::default();
        self.button = false;
        self.tracking = false;
        self.automatic = false;
        self.manual = false;
        self.manual_sprint = false;
        self.input_stamp = 0;
    }
}
unsafe fn context_allowed() -> bool {
    let tier = cvr_cpp_pose_scene_tier();
    cvr_cpp_menu_mode() == 0
        && !cvr_ladder_active()
        && !g_isInVehicle
        && !cvr_cpp_braindance_active()
        && !cvr_cpp_device_cam_active()
        && cvr_cpp_ui_popup_open() == 0
        && !cvr_cpp_overlay_visible()
        && tier > 0
        && tier < 4
}

#[no_mangle]
pub extern "C" fn cvr_swimming_active() -> bool {
    state().present(now_us())
}
/// The context the native locomotion sampler just used, bound when its provider is GetPlayer()'s.
#[no_mangle]
pub unsafe extern "C" fn cvr_swimming_bind_native_input(locomotion: *mut c_void) {
    if !cvr_swimming_active() && !cvr_ladder_active() {
        return;
    }
    let input = cvr_cpp_swimming_player_input_object(locomotion);
    if input == 0 {
        return;
    }
    let mut s = state();
    s.input_object = input;
    s.input_owner = cvr_roomscale_player_identity();
    s.input_owner_stamp = now_us();
}
#[no_mangle]
pub unsafe extern "C" fn cvr_swimming_native_action(action: i32) -> f32 {
    if !context_allowed() {
        return 0.0;
    }
    let s = state();
    let now = now_us();
    if s.paused || !s.fresh(now) || s.input_stamp == 0 || now < s.input_stamp || now - s.input_stamp > 100000 {
        return 0.0;
    }
    if s.manual && action != ASCEND {
        return 0.0;
    }
    match action {
        FORWARD => forward_action(s.input.forward, s.input.boost, s.fast, s.manual_sprint),
        BOOST => {
            if s.input.boost && s.input.forward > 0.8 {
                1.0
            } else {
                0.0
            }
        }
        ASCEND => {
            if s.input.ascend > 0.1 {
                1.0
            } else {
                0.0
            }
        }
        DIVE => s.input.dive as i32 as f32,
        _ => 0.0,
    }
}
type ActionFn = unsafe extern "system" fn(*mut c_void, u64) -> f32;
unsafe fn original_action(object: *mut c_void, name: u64) -> f32 {
    let f: ActionFn = core::mem::transmute(CvrSwimmingActionOriginal);
    f(object, name)
}
/// After the original leaf: this player's MoveY / ToggleSprint / Jump / Dive with the strokes' actions in.
#[no_mangle]
pub unsafe extern "C" fn CvrSwimmingActionAfter(object: *mut c_void, name: u64, original: f32) -> f32 {
    if name != MOVE_Y && name != TOGGLE_SPRINT && name != JUMP && name != DIVE_NAME {
        return original;
    }
    {
        let s = state();
        let now = now_us();
        if object as usize != s.input_object
            || s.input_owner != cvr_roomscale_player_identity()
            || now < s.input_owner_stamp
            || now - s.input_owner_stamp > 250000
        {
            return original;
        }
    }
    let x = original_action(object, MOVE_X);
    let y = original_action(object, MOVE_Y);
    if cvr_ladder_active() {
        return if name == MOVE_Y { cvr_ladder_mix_move(original, x, y) } else { original };
    }
    if y < -0.2 {
        cvr_swimming_reset_input();
        return original;
    }
    let manual = x.abs() > 0.15 || y.abs() > 0.15;
    let manual_sprint = original_action(object, TOGGLE_SPRINT) > 0.0 || original_action(object, SPRINT) > 0.0;
    {
        let mut s = state();
        s.manual = manual;
        s.manual_sprint = manual_sprint;
    }
    let action = if name == MOVE_Y {
        FORWARD
    } else if name == TOGGLE_SPRINT {
        BOOST
    } else if name == JUMP {
        ASCEND
    } else {
        DIVE
    };
    let value = if manual && (action == FORWARD || action == BOOST || action == DIVE) {
        0.0
    } else {
        cvr_swimming_native_action(action)
    };
    if action == FORWARD {
        state().automatic = value > 0.0;
    }
    if value > 0.0 {
        max(original, value)
    } else {
        original
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_swimming_read_steering(direction: *mut Vec3, out_state: *mut i32) -> bool {
    let context = context_allowed();
    let s = state();
    let now = now_us();
    if !context
        || s.paused
        || !s.fresh(now)
        || !s.automatic
        || s.input_stamp == 0
        || now < s.input_stamp
        || now - s.input_stamp > 100000
        || direction.is_null()
        || out_state.is_null()
    {
        return false;
    }
    *direction = s.direction;
    *out_state = s.state;
    true
}
#[no_mangle]
pub extern "C" fn cvr_swimming_publish_state(player: usize, swim_state: i32, fast: i32, paused: bool, water_context: bool) {
    let mut s = state();
    if player != s.player || (!in_water(swim_state) && !water_context) || paused {
        s.clear();
    }
    s.paused = paused;
    s.water_context = water_context;
    s.player = player;
    s.state = swim_state;
    s.fast = fast.clamp(-1, 1);
    s.stamp = now_us();
}
#[no_mangle]
pub extern "C" fn cvr_swimming_reset_input() {
    state().clear();
}
#[no_mangle]
pub unsafe extern "C" fn cvr_swimming_update_input(controllers: *const VrController, manual_back: bool, out: *mut SwimInput) {
    let c = *controllers;
    let context = context_allowed();
    {
        let mut s = state();
        if !context || s.paused || !s.present(now_us()) {
            s.clear();
            *out = SwimInput::default();
            return;
        }
    }
    let mut head = HeadPose::default();
    let mut hands = [HeadPose::default(); 2];
    let (mut sequence, mut stamp) = (0u64, 0u64);
    let tracked = c.left_hand_valid
        && c.right_hand_valid
        && super::xr_calls::gesture_hand_frame(&mut head, hands.as_mut_ptr(), &mut sequence, &mut stamp, false)
        && head.valid
        && hands[0].valid
        && hands[1].valid;
    let mut sample = Sample::default();
    if tracked {
        sample.head = Vec3 {
            x: head.pos[0],
            y: head.pos[1],
            z: head.pos[2],
        };
        sample.orientation = Quat {
            x: head.ori[0],
            y: head.ori[1],
            z: head.ori[2],
            w: head.ori[3],
        };
        for i in 0..2 {
            sample.hands[i] = hand_in_tracking_space(
                sample.head,
                sample.orientation,
                Vec3 {
                    x: hands[i].pos[0],
                    y: hands[i].pos[1],
                    z: hands[i].pos[2],
                },
            );
        }
        sample.sequence = sequence;
        sample.stamp_us = stamp;
        sample.origin = head.origin_serial;
        sample.valid = true;
    }
    let breaststroke = super::live_reads::breaststroke_swim();
    let mut s = state();
    let now = now_us();
    if !s.present(now) {
        s.clear();
        *out = SwimInput::default();
        return;
    }
    let enabled = !manual_back && tracked && breaststroke != 0 && !s.paused;
    let motion = s.gesture.update(&sample, now, enabled);
    let ascent = s.ascent.update(&sample, now, enabled);
    s.tracking = tracked;
    if motion.stroke {
        s.strokes += 1;
    }
    if ascent.stroke {
        s.ascents += 1;
        s.gesture.reset();
    }
    s.direction = if tracked {
        rotate_vector(sample.orientation, Vec3 { x: 0.0, y: 0.0, z: -1.0 })
    } else {
        Vec3::default()
    };
    let ascending = ascent.forward > 0.0 || ascent.stroke;
    s.input = SwimInput {
        forward: if ascending { 0.0 } else { motion.forward },
        water: true,
        ascend: ascent.forward,
        dive: !ascending && s.state == 1 && motion.forward > 0.15 && s.direction.y < -0.25,
        boost: !ascending && motion.boost,
    };
    s.input_stamp = now;
    *out = s.input;
}
#[no_mangle]
pub unsafe extern "C" fn cvr_swimming_sprint_input(wanted: bool) -> bool {
    let context = context_allowed();
    let mut s = state();
    let now = now_us();
    if !context || s.paused || !s.fresh(now) || !s.input.water {
        s.sprint.reset();
        s.button = false;
        return false;
    }
    let fast = s.fast;
    s.button = s.sprint.update(wanted, fast, now);
    s.button
}
#[no_mangle]
pub unsafe extern "C" fn cvr_swimming_script_action(owner: usize, name: u64, just_pressed: bool) -> f32 {
    let action = if name == DIVE_NAME {
        DIVE
    } else if just_pressed {
        return 0.0;
    } else if name == MOVE_Y {
        FORWARD
    } else if name == TOGGLE_SPRINT {
        BOOST
    } else if name == JUMP {
        ASCEND
    } else {
        return 0.0;
    };
    if !cvr_swimming_is_player(owner) {
        return 0.0;
    }
    cvr_swimming_native_action(action)
}
#[no_mangle]
pub extern "C" fn cvr_swimming_is_player(owner: usize) -> bool {
    let s = state();
    owner != 0 && owner == s.player
}
#[no_mangle]
pub unsafe extern "C" fn cvr_swimming_debug_value(index: i32) -> f32 {
    if (20..=23).contains(&index) {
        return cvr_swimming_native_action(index - 20);
    }
    let s = state();
    match index {
        0 => s.state as f32,
        1 => s.fast as f32,
        2 => s.strokes as f32,
        3 => s.gesture.current_phase() as i32 as f32,
        4 => s.input.forward,
        5 => s.button as i32 as f32,
        6 => {
            if s.stamp != 0 {
                now_us().wrapping_sub(s.stamp) as f32 * 0.001
            } else {
                -1.0
            }
        }
        7 => s.input.water as i32 as f32,
        8 => s.tracking as i32 as f32,
        9 => s.input.ascend,
        10 => s.ascents as f32,
        11 => s.automatic as i32 as f32,
        12 => (s.input_object != 0 && s.input_owner == cvr_roomscale_player_identity()) as i32 as f32,
        13 => s.water_context as i32 as f32,
        14 => s.input.boost as i32 as f32,
        15 => s.gesture.speed(),
        16 => cvr_cpp_swimming_script_hooks_active() as i32 as f32,
        _ => -1.0,
    }
}

const ACTION_READER: usize = 0x648ECC;
const ACTION_READER_BYTES: &str = "48 8B 41 08 8B 49 14 48 8D 0C 49 48 C1 E1 04";

/// Installed from src/Hooks/SwimmingInput.cpp's registration (Boot, 101): the shared CName action-table reader.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_swimming_actions_install() -> bool {
    let site = cvr_engine::code::image_base() + ACTION_READER;
    if !Pattern::parse(ACTION_READER_BYTES).is_some_and(|p| expect(site, &p)) {
        return false;
    }
    let detours = [Detour {
        name: "ActionReader",
        target: ACTION_READER,
        absolute: false,
        detour: CvrSwimmingActionDetour as *const c_void,
        original: core::ptr::addr_of_mut!(CvrSwimmingActionOriginal),
        expect: Some(ACTION_READER_BYTES),
    }];
    install_chain("swimming-actions", &detours).is_ok()
}

// The swim module's natives (formerly src/Natives/Swimming.cpp), registered by src/Natives/Registration.cpp.

/// SetVRSwimmingState(player: handle:PlayerPuppet, state: Int32, fast: Int32, paused: Int32, waterContext: Int32)
/// -> Int32 (always 1).
#[no_mangle]
pub unsafe extern "C" fn SetVRSwimmingState(_this: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut i32, _unused: i64) {
    let mut player = red4ext_abi::handle::Handle::null();
    let (mut state, mut fast, mut paused, mut water_context) = (-1i32, -1i32, 0i32, 0i32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut player);
        frame.get_parameter(&mut state);
        frame.get_parameter(&mut fast);
        frame.get_parameter(&mut paused);
        frame.get_parameter(&mut water_context);
        frame.finish_params();
    }
    cvr_swimming_publish_state(player.instance as usize, state, fast, paused != 0, water_context != 0);
    if !out.is_null() {
        *out = 1;
    }
}
/// GetVRSwimmingDebug(index: Int32) -> Float.
#[no_mangle]
pub unsafe extern "C" fn GetVRSwimmingDebug(_this: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut f32, _unused: i64) {
    let mut index = 0i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut index);
        frame.finish_params();
    }
    if !out.is_null() {
        *out = cvr_swimming_debug_value(index);
    }
}
