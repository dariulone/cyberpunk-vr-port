//! The port's settings panel in VR: its placement in the world, the controller pointer on it, and the L3 + R3 hold
//! that asks for the VR menu (formerly src/Overlay/VrOverlay.cpp with the pose policy of
//! include/Overlay/VrInteraction.hpp; the file's `cvr::vrui` functions forward here, the header keeps its types).
//!
//! THE PANEL is opened by F10 / INSERT (Toggle) and shown once the Lua bridge reports ready in game; while it is asked
//! for, open, or held (both sticks clicked), and until the inputs that opened or closed it are let go, the controllers
//! are captured away from the game. A grip that stays above half (a resting hand on Index controllers) is only waited
//! for 1.5 s, and not at all with the grip read from the fingers.
//!
//! THE PLACEMENT follows the head's heading (a free-look cone, or fixed ahead) at the set distance and offsets, and the
//! GRIP CARRIES THE PANEL ON THE CONTROLLER: its centre is held in the controller's own frame at the moment of the grab
//! and carried rigidly (turning the wrist swings it on the full lever, the stick pushes it away and pulls it in);
//! whatever the drag produces is stored as distance and offsets in the heading's frame, clamped, and the panel stays
//! upright, turned to face the head.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::hud::{cvr_hud_head_yaw, DisplayClock, Follow};
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, Ordering::*};
use crate::cmath::{atan2, cos, sin};
use crate::xr::{conjugate_quat, rotate_vector, v3, Quat, Vec3};
use std::sync::Mutex;

pub const CANVAS_WIDTH: u32 = 1440;
pub const CANVAS_HEIGHT: u32 = 1080;

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}
fn now() -> u64 {
    // SAFETY: plain Win32 call.
    unsafe { GetTickCount64() }
}

/// XrPosef.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub orientation: Quat,
    pub position: Vec3,
}
const IDENTITY_POSE: Pose = Pose {
    orientation: Quat::IDENTITY,
    position: v3(0.0, 0.0, 0.0),
};

/// cvr::vrui::Settings.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub distance: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub width: f32,
    pub cone: f32,
    pub scale: f32,
    pub font_scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub depth_speed: f32,
    /// 0 left, 1 right, 2 automatic.
    pub hand: i32,
    pub follow: bool,
}
const _: () = assert!(core::mem::size_of::<Settings>() == 48);
const DEFAULT_SETTINGS: Settings = Settings {
    distance: 1.4,
    min_distance: 0.6,
    max_distance: 3.0,
    width: 1.55,
    cone: 60.0,
    scale: 1.0,
    font_scale: 1.0,
    offset_x: 0.0,
    offset_y: 0.0,
    depth_speed: 0.75,
    hand: 1,
    follow: true,
};
impl Default for Settings {
    fn default() -> Self {
        DEFAULT_SETTINGS
    }
}
/// std::clamp.
fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}
pub fn sanitize(mut s: Settings) -> Settings {
    let c = |v: f32, lo: f32, hi: f32, def: f32| {
        if v.is_finite() {
            clampf(v, lo, hi)
        } else {
            def
        }
    };
    s.min_distance = c(s.min_distance, 0.35, 2.0, 0.6);
    s.max_distance = c(s.max_distance, s.min_distance + 0.1, 5.0, 3.0);
    s.distance = c(s.distance, s.min_distance, s.max_distance, 1.4);
    s.width = c(s.width, 0.6, 2.4, 1.55);
    s.scale = c(s.scale, 0.6, 1.5, 1.0);
    s.font_scale = c(s.font_scale, 0.8, 1.4, 1.0);
    s.cone = c(s.cone, 5.0, 90.0, 60.0);
    s.offset_x = c(s.offset_x, -1.5, 1.5, 0.0);
    s.offset_y = c(s.offset_y, -1.0, 1.0, 0.0);
    s.depth_speed = c(s.depth_speed, 0.1, 2.0, 0.75);
    s.hand = s.hand.clamp(0, 2);
    s
}

/// cvr::vrui::Hand.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Hand {
    pub aim: Pose,
    pub valid: bool,
    pub stick_click: bool,
    pub trigger: f32,
    pub grip: f32,
    pub stick_y: f32,
}
/// cvr::vrui::Tracking.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Tracking {
    pub head: Pose,
    pub hands: [Hand; 2],
    pub valid: bool,
    pub origin: u64,
    pub time: i64,
}
const _: () =
    assert!(core::mem::size_of::<Hand>() == 44 && core::mem::size_of::<Tracking>() == 136);

fn add(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x + b.x, a.y + b.y, a.z + b.z)
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x - b.x, a.y - b.y, a.z - b.z)
}
fn mul(a: Vec3, s: f32) -> Vec3 {
    v3(a.x * s, a.y * s, a.z * s)
}

/// cvr::vrui::Hit.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub valid: bool,
    pub x: f32,
    pub y: f32,
    pub distance: f32,
    pub point: Vec3,
}
const NO_HIT: Hit = Hit {
    valid: false,
    x: 0.0,
    y: 0.0,
    distance: 0.0,
    point: v3(0.0, 0.0, 0.0),
};
/// Where a controller's aim ray meets the panel's front, in canvas pixels.
pub fn intersect(ray: &Pose, panel: &Pose, width: f32, height: f32) -> Hit {
    let inv = conjugate_quat(panel.orientation);
    let origin = rotate_vector(inv, sub(ray.position, panel.position));
    let ahead = rotate_vector(ray.orientation, v3(0.0, 0.0, -1.0));
    let direction = rotate_vector(inv, ahead);
    if !direction.z.is_finite() || direction.z >= -0.0001 || origin.z <= 0.0 {
        return NO_HIT;
    }
    let t = -origin.z / direction.z;
    if !t.is_finite() || t <= 0.0 || t > 10.0 || width <= 0.0 || height <= 0.0 {
        return NO_HIT;
    }
    let p = add(origin, mul(direction, t));
    if p.x.abs() > width * 0.5 || p.y.abs() > height * 0.5 {
        return NO_HIT;
    }
    Hit {
        valid: true,
        x: (p.x / width + 0.5) * CANVAS_WIDTH as f32,
        y: (0.5 - p.y / height) * CANVAS_HEIGHT as f32,
        distance: t,
        point: add(ray.position, mul(ahead, t)),
    }
}

/// Both sticks clicked and held: fires once per hold, after half a second.
#[derive(Clone, Copy, Debug, Default)]
pub struct HoldChord {
    pub held: f64,
    pub latched: bool,
}
impl HoldChord {
    pub fn update(&mut self, both: bool, dt: f32) -> bool {
        if !both {
            self.held = 0.0;
            self.latched = false;
            return false;
        }
        if self.latched {
            return false;
        }
        if dt.is_finite() && dt > 0.0 && dt <= 0.25 {
            self.held += dt as f64;
        } else {
            self.held = 0.0;
        }
        if self.held >= 0.5 {
            self.latched = true;
            return true;
        }
        false
    }
}

/// The panel's pose policy.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub follow: Follow,
    pub pose: Pose,
    pub origin: u64,
    pub valid: bool,
    pub dragging: i32,
    /// The follow heading the distance and offsets are measured in.
    pub frame: Quat,
    /// The panel's centre in the dragging controller's own frame.
    pub grab_local: Vec3,
}
impl Default for Placement {
    fn default() -> Self {
        Placement {
            follow: Follow::default(),
            pose: IDENTITY_POSE,
            origin: 0,
            valid: false,
            dragging: -1,
            frame: Quat::IDENTITY,
            grab_local: v3(0.0, 0.0, 0.0),
        }
    }
}
fn yaw_quat(yaw: f32) -> Quat {
    Quat {
        x: 0.0,
        y: sin(yaw * 0.5),
        z: 0.0,
        w: cos(yaw * 0.5),
    }
}
impl Placement {
    pub fn reset(&mut self) {
        self.follow.valid = false;
        self.follow.following = false;
        self.follow.reset_delay();
        self.valid = false;
        self.dragging = -1;
    }
    fn face(&mut self, head: Vec3) {
        let (dx, dz) = (self.pose.position.x - head.x, self.pose.position.z - head.z);
        if dx * dx + dz * dz < 1e-4 {
            self.pose.orientation = self.frame;
            return;
        }
        self.pose.orientation = yaw_quat(atan2(-dx, -dz));
    }
    fn hold(&mut self, hand: &Hand) {
        self.grab_local = rotate_vector(
            conjugate_quat(hand.aim.orientation),
            sub(self.pose.position, hand.aim.position),
        );
    }
    pub fn update(&mut self, t: &Tracking, s: &mut Settings, dt: f32) {
        if !t.valid {
            self.dragging = -1;
            self.follow.reset_delay();
            return;
        }
        if self.origin != t.origin {
            self.reset();
            self.origin = t.origin;
        }
        let q = t.head.orientation;
        let head = cvr_hud_head_yaw(q.x, q.y, q.z, q.w, self.follow.yaw);
        if self.dragging >= 0 {
            let hand = &t.hands[self.dragging as usize];
            if !hand.valid || hand.grip < 0.5 {
                self.dragging = -1;
            }
        }
        if self.dragging < 0 {
            if !s.follow {
                self.follow.yaw = head;
                self.follow.valid = true;
                self.follow.following = false;
                self.follow.reset_delay();
            }
            let yaw = if s.follow {
                self.follow.update(head, s.cone * 0.01745329252, dt, true)
            } else {
                head
            };
            self.frame = yaw_quat(yaw);
        } else {
            let hand = t.hands[self.dragging as usize];
            let target = add(
                hand.aim.position,
                rotate_vector(hand.aim.orientation, self.grab_local),
            );
            let local = rotate_vector(conjugate_quat(self.frame), sub(target, t.head.position));
            let axis = if hand.stick_y.abs() > 0.2 {
                hand.stick_y
            } else {
                0.0
            };
            s.distance = clampf(
                -local.z + axis * s.depth_speed * clampf(dt, 0.0, 0.05),
                s.min_distance,
                s.max_distance,
            );
            s.offset_x = clampf(local.x, -1.5, 1.5);
            s.offset_y = clampf(local.y, -1.0, 1.0);
            self.follow.reset_delay();
        }
        self.pose.position = add(
            t.head.position,
            rotate_vector(self.frame, v3(s.offset_x, s.offset_y, -s.distance)),
        );
        self.face(t.head.position);
        // Re-held on the clamped result, so a bound or the stick is not undone by the next frame.
        if self.dragging >= 0 {
            let hand = t.hands[self.dragging as usize];
            self.hold(&hand);
        }
        self.valid = true;
    }
    pub fn grab(&mut self, hand: usize, t: &Tracking) {
        self.dragging = hand as i32;
        self.hold(&t.hands[hand]);
        self.follow.reset_delay();
    }
}

/// cvr::vrui::PointerEvent.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PointerEvent {
    pub x: f32,
    pub y: f32,
    pub wheel: f32,
    pub down: bool,
}
const NO_EVENT: PointerEvent = PointerEvent {
    x: -1.0,
    y: -1.0,
    wheel: 0.0,
    down: false,
};
/// cvr::vrui::View.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub pose: Pose,
    pub ray_start: Vec3,
    pub ray_end: Vec3,
    pub head_position: Vec3,
    pub width: f32,
    pub height: f32,
    pub hold: f32,
    pub hit: Hit,
    pub valid: bool,
    pub ray: bool,
    pub dragging: bool,
}
const _: () =
    assert!(core::mem::size_of::<View>() == 108 && core::mem::offset_of!(View, hit) == 76);
const NO_VIEW: View = View {
    pose: IDENTITY_POSE,
    ray_start: v3(0.0, 0.0, 0.0),
    ray_end: v3(0.0, 0.0, 0.0),
    head_position: v3(0.0, 0.0, 0.0),
    width: 0.0,
    height: 0.0,
    hold: 0.0,
    hit: NO_HIT,
    valid: false,
    ray: false,
    dragging: false,
};

struct State {
    settings: Settings,
    placement: Placement,
    chord: HoldChord,
    clock: DisplayClock,
    command: i32,
    wanted: bool,
    button_down: bool,
    trigger_armed: bool,
    release_pending: bool,
    grip_held: [bool; 2],
    menu_summon: bool,
    bridge_stamp: u64,
    request_stamp: u64,
    items: Vec<Vec<u8>>,
    events: Vec<PointerEvent>,
    view: View,
    placement_save: bool,
    pointer_hand: usize,
    previous_trigger: bool,
    pending_since: u64,
}
static VISIBLE: AtomicBool = AtomicBool::new(false);
static CAPTURE: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| State {
        settings: DEFAULT_SETTINGS,
        placement: Placement::default(),
        chord: HoldChord::default(),
        clock: DisplayClock::default(),
        command: 0,
        wanted: false,
        button_down: false,
        trigger_armed: false,
        release_pending: false,
        grip_held: [false; 2],
        menu_summon: false,
        bridge_stamp: 0,
        request_stamp: 0,
        items: Vec::new(),
        events: Vec::new(),
        view: NO_VIEW,
        placement_save: false,
        pointer_hand: 1,
        previous_trigger: false,
        pending_since: 0,
    });
    f(s)
}
fn visible() -> bool {
    VISIBLE.load(Relaxed)
}
impl State {
    fn release_pointer(&mut self) {
        self.events.clear();
        self.events.push(NO_EVENT);
        self.button_down = false;
        self.trigger_armed = false;
    }
    fn toggle(&mut self) {
        self.wanted = !self.wanted;
        self.command = if self.wanted { 1 } else { 2 };
        self.request_stamp = now();
        VISIBLE.store(false, SeqCst);
        CAPTURE.store(true, SeqCst);
        self.release_pending = true;
        self.placement.reset();
        self.release_pointer();
    }
    fn update_tracking(&mut self, t: &Tracking, grip_from_fingers: bool) {
        self.clock.step(t.time);
        let elapsed = self.clock.elapsed;
        let both = t.valid
            && t.hands[0].valid
            && t.hands[1].valid
            && t.hands[0].stick_click
            && t.hands[1].stick_click;
        // The sticks are held back from the game until they are let go, as after a panel toggle.
        if self.chord.update(both, elapsed) {
            self.menu_summon = true;
            self.release_pending = true;
        }
        let now = now();
        // The idle Lua bridge only polls commands, so its last acknowledgement may be arbitrarily old while closed:
        // each new request gets its opening window, a live heartbeat is required once it made the panel visible.
        if self.wanted
            && ((visible() && self.bridge_stamp != 0 && now.wrapping_sub(self.bridge_stamp) > 1500)
                || (!visible() && now.wrapping_sub(self.request_stamp) > 3000))
        {
            self.wanted = false;
            VISIBLE.store(false, SeqCst);
            self.release_pointer();
        }
        if !self.release_pending {
            self.pending_since = 0;
        } else if self.pending_since == 0 {
            self.pending_since = now;
        }
        let grips_free = grip_from_fingers
            || (t.hands[0].grip < 0.5 && t.hands[1].grip < 0.5)
            || (self.pending_since != 0 && now.wrapping_sub(self.pending_since) > 1500);
        if self.release_pending
            && !both
            && t.hands[0].trigger < 0.3
            && t.hands[1].trigger < 0.3
            && grips_free
        {
            self.release_pending = false;
        }
        CAPTURE.store(
            visible() || self.wanted || both || self.release_pending,
            SeqCst,
        );
        self.view.hold = self.chord.held as f32;
        if !visible() {
            self.view.valid = false;
            return;
        }
        let was_dragging = self.placement.dragging >= 0;
        self.placement.update(t, &mut self.settings, elapsed);
        if was_dragging && self.placement.dragging < 0 {
            self.placement_save = true;
        }
        let v = &mut self.view;
        v.pose = self.placement.pose;
        v.width = self.settings.width * self.settings.scale;
        v.height = v.width * CANVAS_HEIGHT as f32 / CANVAS_WIDTH as f32;
        v.head_position = t.head.position;
        v.valid = self.placement.valid;
        v.hit = NO_HIT;
        v.ray = false;
        v.dragging = self.placement.dragging >= 0;
        let mut hand = if self.placement.dragging >= 0 {
            self.placement.dragging as usize
        } else if self.settings.hand == 2 {
            self.pointer_hand
        } else {
            self.settings.hand as usize
        };
        let (pose, width, height) = (v.pose, v.width, v.height);
        let hit = |h: usize| {
            if t.valid && t.hands[h].valid {
                intersect(&t.hands[h].aim, &pose, width, height)
            } else {
                NO_HIT
            }
        };
        self.view.hit = hit(hand);
        if self.settings.hand == 2
            && self.placement.dragging < 0
            && !self.button_down
            && !self.view.hit.valid
            && hit(1 - hand).valid
        {
            hand = 1 - hand;
            self.view.hit = hit(hand);
        }
        if hand != self.pointer_hand {
            self.release_pointer();
            self.previous_trigger = false;
            self.pointer_hand = hand;
        }
        let input = t.hands[hand];
        if !t.valid || !input.valid {
            self.release_pointer();
            self.events.push(NO_EVENT);
            return;
        }
        if input.trigger < 0.3 {
            self.trigger_armed = true;
        }
        let pressed = input.trigger > if self.previous_trigger { 0.35 } else { 0.65 };
        let down = self.trigger_armed
            && pressed
            && (self.button_down || (!self.previous_trigger && self.view.hit.valid))
            && self.placement.dragging < 0;
        self.previous_trigger = pressed;
        let mut wheel = 0.0f32;
        if input.grip > 0.7 && !self.grip_held[hand] && self.view.hit.valid && !self.button_down {
            self.placement.grab(hand, t);
            self.view.dragging = true;
        }
        if self.view.hit.valid
            && !down
            && input.grip < 0.5
            && self.placement.dragging < 0
            && input.stick_y.abs() > 0.2
        {
            wheel = input.stick_y * clampf(elapsed, 0.0, 0.05) * 5.0;
        }
        for h in 0..2 {
            self.grip_held[h] = t.hands[h].grip > 0.5;
        }
        self.button_down = down;
        if self.events.len() < 64 {
            let hit = self.view.hit;
            self.events.push(PointerEvent {
                x: if hit.valid { hit.x } else { -1.0 },
                y: if hit.valid { hit.y } else { -1.0 },
                wheel,
                down,
            });
        } else {
            self.events.clear();
            self.release_pointer();
            self.events.push(NO_EVENT);
        }
        self.view.ray = self.view.hit.valid;
        self.view.ray_start = input.aim.position;
        self.view.ray_end = self.view.hit.point;
    }
}

/// # Safety
/// `out` points to a cvr::vrui::Settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_get_settings(out: *mut Settings) {
    *out = with(|s| s.settings);
}
/// # Safety
/// `s` points to a cvr::vrui::Settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_set_settings(s: *const Settings) {
    let value = sanitize(*s);
    with(|state| state.settings = value);
}
#[no_mangle]
pub extern "C" fn cvr_vrui_visible() -> bool {
    visible()
}
#[no_mangle]
pub extern "C" fn cvr_vrui_captures_input() -> bool {
    CAPTURE.load(Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_vrui_toggle() {
    with(|s| s.toggle());
}
/// L3 + R3 held half a second: the VR menu is asked to open or close, once per hold. Read and cleared.
#[no_mangle]
pub extern "C" fn cvr_vrui_consume_menu_summon() -> bool {
    with(|s| core::mem::take(&mut s.menu_summon))
}
#[no_mangle]
pub extern "C" fn cvr_vrui_close(resume: bool) {
    with(|s| {
        s.wanted = false;
        VISIBLE.store(false, SeqCst);
        CAPTURE.store(true, SeqCst);
        s.release_pending = true;
        if resume {
            s.command = 2;
        }
        s.release_pointer();
    });
}
#[no_mangle]
pub extern "C" fn cvr_vrui_recenter() {
    with(|s| {
        s.settings.offset_x = 0.0;
        s.settings.offset_y = 0.0;
        s.placement.reset();
    });
}
#[no_mangle]
pub extern "C" fn cvr_vrui_consume_placement_save() -> bool {
    with(|s| core::mem::take(&mut s.placement_save))
}
#[no_mangle]
pub extern "C" fn cvr_vrui_select_menu_item(i: u32) {
    with(|s| {
        if i as usize >= s.items.len() || !visible() {
            return;
        }
        s.command = 100 + i as i32;
        s.wanted = false;
        VISIBLE.store(false, SeqCst);
        CAPTURE.store(true, SeqCst);
        s.release_pending = true;
        s.release_pointer();
    });
}
/// 1 open panel, 2 close panel, 100+ the selected game action; 0 none. Read and cleared.
#[no_mangle]
pub extern "C" fn cvr_vrui_poll_command() -> i32 {
    with(|s| core::mem::take(&mut s.command))
}
/// The Lua bridge's heartbeat: ready, in game, and its menu's items (one per line, at most 16 of 240 bytes).
///
/// # Safety
/// `labels` is a NUL-terminated string or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_bridge_update(ready: bool, in_game: bool, labels: *const c_char) {
    let text: &[u8] = if labels.is_null() {
        b""
    } else {
        core::ffi::CStr::from_ptr(labels).to_bytes()
    };
    with(|s| {
        s.bridge_stamp = now();
        s.items.clear();
        for line in text.split(|&b| b == b'\n') {
            if s.items.len() >= 16 {
                break;
            }
            if !line.is_empty() {
                s.items.push(line[..line.len().min(240)].to_vec());
            }
        }
        if s.wanted && ready && in_game && !s.items.is_empty() {
            VISIBLE.store(true, SeqCst);
            CAPTURE.store(true, SeqCst);
        } else if visible() && (!ready || !in_game) {
            s.wanted = false;
            VISIBLE.store(false, SeqCst);
            CAPTURE.store(false, SeqCst);
            s.release_pointer();
        }
    });
}
/// The menu's items, one per line, into `out` (at most `capacity` bytes); their full length.
///
/// # Safety
/// `out` points to `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_menu_items(out: *mut u8, capacity: usize) -> usize {
    let text = with(|s| s.items.join(&b'\n'));
    core::ptr::copy_nonoverlapping(text.as_ptr(), out, text.len().min(capacity));
    text.len()
}
/// The pointer events since the last call, into `out` (at most `capacity`); their count (all of them: the caller's
/// capacity is the queue's bound).
///
/// # Safety
/// `out` points to `capacity` events.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_consume_pointer_events(
    out: *mut PointerEvent,
    capacity: usize,
) -> usize {
    let events = with(|s| core::mem::take(&mut s.events));
    let n = events.len().min(capacity);
    core::ptr::copy_nonoverlapping(events.as_ptr(), out, n);
    n
}
/// # Safety
/// `out` points to a cvr::vrui::View.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_get_view(out: *mut View) {
    *out = with(|s| s.view);
}
/// One sample of the head and the hands on the display timeline.
///
/// # Safety
/// `t` points to a cvr::vrui::Tracking.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_update_tracking(t: *const Tracking, grip_from_fingers: bool) {
    let t = *t;
    with(|s| s.update_tracking(&t, grip_from_fingers));
}

/// The ini's overlay keys (" = " matches any spacing, as sscanf's whitespace does).
///
/// # Safety
/// `line` is a NUL-terminated string; `s` a cvr::vrui::Settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_parse_setting(line: *const c_char, s: *mut Settings) -> bool {
    use cvr_crt::{ptr, scan};
    let s = &mut *s;
    let (mut v, mut n) = (0.0f32, 0i32);
    macro_rules! float {
        ($fmt:expr, $field:expr) => {
            if scan(line, $fmt, &[ptr(&mut v)]) == 1 {
                $field = v;
                return true;
            }
        };
    }
    float!(c"xr_overlay_distance = %f", s.distance);
    float!(c"xr_overlay_min_distance = %f", s.min_distance);
    float!(c"xr_overlay_max_distance = %f", s.max_distance);
    float!(c"xr_overlay_width = %f", s.width);
    float!(c"xr_overlay_scale = %f", s.scale);
    float!(c"xr_overlay_font_scale = %f", s.font_scale);
    float!(c"xr_overlay_cone = %f", s.cone);
    float!(c"xr_overlay_offset_x = %f", s.offset_x);
    float!(c"xr_overlay_offset_y = %f", s.offset_y);
    float!(c"xr_overlay_depth_speed = %f", s.depth_speed);
    if scan(line, c"xr_overlay_hand = %d", &[ptr(&mut n)]) == 1 {
        s.hand = n;
        return true;
    }
    if scan(line, c"xr_overlay_follow = %d", &[ptr(&mut n)]) == 1 {
        s.follow = n != 0;
        return true;
    }
    false
}
/// The overlay's keys as the ini keeps them.
pub fn settings_text(s: &Settings) -> std::ffi::CString {
    use cvr_crt::Arg::{F, I};
    cvr_crt::format(
        c"xr_overlay_distance=%.3f\nxr_overlay_min_distance=%.3f\nxr_overlay_max_distance=%.3f\nxr_overlay_width=%.3f\nxr_overlay_scale=%.3f\nxr_overlay_font_scale=%.3f\nxr_overlay_cone=%.2f\nxr_overlay_offset_x=%.3f\nxr_overlay_offset_y=%.3f\nxr_overlay_depth_speed=%.3f\nxr_overlay_hand=%d\nxr_overlay_follow=%d\n",
        &[
            F(s.distance as f64),
            F(s.min_distance as f64),
            F(s.max_distance as f64),
            F(s.width as f64),
            F(s.scale as f64),
            F(s.font_scale as f64),
            F(s.cone as f64),
            F(s.offset_x as f64),
            F(s.offset_y as f64),
            F(s.depth_speed as f64),
            I(s.hand),
            I(s.follow as i32),
        ],
    )
}
/// # Safety
/// `file` is a C FILE open for writing; `s` a cvr::vrui::Settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_write_settings(file: *mut c_void, s: *const Settings) {
    cvr_crt::fputs_to(file, &settings_text(&*s));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_ahead_hits_the_centre() {
        let ray = Pose {
            orientation: Quat::IDENTITY,
            position: v3(0.0, 1.7, -0.3),
        };
        let panel = Pose {
            orientation: Quat::IDENTITY,
            position: v3(0.0, 1.7, -1.4),
        };
        let hit = intersect(&ray, &panel, 1.6, 1.2);
        assert!(
            hit.valid
                && (hit.x - 720.0).abs() < 1e-3
                && (hit.y - 540.0).abs() < 1e-3
                && (hit.distance - 1.1).abs() < 1e-3
        );
    }
    #[test]
    fn the_chord_fires_once_per_hold() {
        let mut c = HoldChord::default();
        assert!((0..49).all(|_| !c.update(true, 0.01)));
        assert!((0..3).any(|_| c.update(true, 0.01)));
        assert!((0..300).all(|_| !c.update(true, 0.01)));
    }
    #[test]
    fn sanitize_bounds_the_settings() {
        let s = sanitize(Settings {
            distance: 9.0,
            min_distance: f32::NAN,
            hand: 7,
            ..DEFAULT_SETTINGS
        });
        assert_eq!((s.min_distance, s.distance, s.hand), (0.6, 3.0, 2));
    }
}
