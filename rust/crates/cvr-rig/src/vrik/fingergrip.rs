//! Finger tracking from the hand skeleton (formerly src/Runtimes/FingerGrip.cpp and Runtimes/FingerFilter.hpp):
//! Valve Index controllers and gloves that present themselves as them, through OpenXR XR_EXT_hand_tracking
//! (xr_finger_bones=1 in vrport.ini when the game starts). Per frame and hand the XR thread locates the joints, turns
//! them into each digit's bend and the spread between neighbours, runs the bends through the spike and zero-dropout
//! filters and publishes them, packed with a millisecond stamp, for V's fingers on the animation thread
//! (gestures.rs); the grip from the fingers (xr_grip_from_fingers) comes from the same bends with hysteresis
//! (Runtimes/FingerGrip.hpp's Update, three of the four fingers).
//!
//! The OpenXR calls are made here through the functions the runtime hands out; the instance-creation step that
//! needs the extension list (std::vector) and the loader's xrGetInstanceProcAddr stay in the C++ wrappers
//! (src/Anim/CharacterRigRust.cpp), which also check these structures' layout against openxr.h.
use super::env::{cvr_cpp_vrik_config_file, log};
use super::gestures::now_ms;
use cvr_body::cmath::acos;
use core::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

// ---- OpenXR 1.0, XR_EXT_hand_tracking and XR_EXT_hand_joints_motion_range (openxr.h)

/// An XrInstance / XrSession / XrSpace / XrHandTrackerEXT: a pointer-sized handle.
pub type Handle = u64;
type XrResult = i32;
type VoidFn = Option<unsafe extern "system" fn()>;
/// xrGetInstanceProcAddr.
pub type GetInstanceProcAddr = unsafe extern "system" fn(instance: Handle, name: *const c_char, function: *mut VoidFn) -> XrResult;
type CreateHandTracker = unsafe extern "system" fn(session: Handle, info: *const HandTrackerCreateInfo, tracker: *mut Handle) -> XrResult;
type DestroyHandTracker = unsafe extern "system" fn(tracker: Handle) -> XrResult;
type LocateHandJoints = unsafe extern "system" fn(tracker: Handle, info: *const HandJointsLocateInfo, locations: *mut HandJointLocations) -> XrResult;

pub const TYPE_HAND_TRACKER_CREATE_INFO: i32 = 1000051001;
pub const TYPE_HAND_JOINTS_LOCATE_INFO: i32 = 1000051002;
pub const TYPE_HAND_JOINT_LOCATIONS: i32 = 1000051003;
pub const TYPE_HAND_JOINTS_MOTION_RANGE_INFO: i32 = 1000080000;
/// XR_EXT_hand_tracking_data_source.
pub const TYPE_HAND_TRACKING_DATA_SOURCE_INFO: i32 = 1000428000;
pub const TYPE_HAND_TRACKING_DATA_SOURCE_STATE: i32 = 1000428001;
pub const DATA_SOURCE_UNOBSTRUCTED: i32 = 1;
pub const DATA_SOURCE_CONTROLLER: i32 = 2;
pub const TYPE_SYSTEM_PROPERTIES: i32 = 5;
pub const TYPE_SYSTEM_HAND_TRACKING_PROPERTIES: i32 = 1000051000;
pub const HAND_LEFT: i32 = 1;
pub const HAND_RIGHT: i32 = 2;
pub const HAND_JOINT_SET_DEFAULT: i32 = 0;
pub const MOTION_RANGE_UNOBSTRUCTED: i32 = 1;
pub const POSITION_VALID_BIT: u64 = 0x2;
pub const JOINT_COUNT: usize = 26;
const WRIST: usize = 1;
const THUMB_METACARPAL: usize = 2;
const THUMB_PROXIMAL: usize = 3;
const THUMB_DISTAL: usize = 4;
const THUMB_TIP: usize = 5;
const INDEX_METACARPAL: usize = 6;
const INDEX_PROXIMAL: usize = 7;
const INDEX_INTERMEDIATE: usize = 8;
const MIDDLE_METACARPAL: usize = 11;
const MIDDLE_PROXIMAL: usize = 12;
const MIDDLE_INTERMEDIATE: usize = 13;
const RING_METACARPAL: usize = 16;
const RING_PROXIMAL: usize = 17;
const RING_INTERMEDIATE: usize = 18;
const LITTLE_METACARPAL: usize = 21;
const LITTLE_PROXIMAL: usize = 22;
const LITTLE_INTERMEDIATE: usize = 23;
const LITTLE_TIP: usize = 25;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Posef {
    pub orientation: [f32; 4],
    pub position: Vec3,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct HandJointLocation {
    pub location_flags: u64,
    pub pose: Posef,
    pub radius: f32,
}
#[repr(C)]
pub struct HandJointLocations {
    pub ty: i32,
    pub next: *mut c_void,
    pub is_active: u32,
    pub joint_count: u32,
    pub joint_locations: *mut HandJointLocation,
}
#[repr(C)]
pub struct HandJointsMotionRangeInfo {
    pub ty: i32,
    pub next: *const c_void,
    pub hand_joints_motion_range: i32,
}
#[repr(C)]
pub struct HandJointsLocateInfo {
    pub ty: i32,
    pub next: *const c_void,
    pub base_space: Handle,
    pub time: i64,
}
#[repr(C)]
pub struct HandTrackerCreateInfo {
    pub ty: i32,
    pub next: *const c_void,
    pub hand: i32,
    pub hand_joint_set: i32,
}
#[repr(C)]
pub struct HandTrackingDataSourceInfo {
    pub ty: i32,
    pub next: *const c_void,
    pub requested_data_source_count: u32,
    pub requested_data_sources: *const i32,
}
#[repr(C)]
pub struct HandTrackingDataSourceState {
    pub ty: i32,
    pub next: *mut c_void,
    pub is_active: u32,
    pub data_source: i32,
}
#[repr(C)]
struct SystemHandTrackingProperties {
    ty: i32,
    next: *mut c_void,
    supports_hand_tracking: u32,
}
/// XrSystemProperties: type, next, systemId, vendorId, systemName[256], graphicsProperties (3 u32), trackingProperties
/// (2 XrBool32).
#[repr(C)]
struct SystemProperties {
    ty: i32,
    next: *mut c_void,
    system_id: u64,
    vendor_id: u32,
    system_name: [u8; 256],
    graphics: [u32; 3],
    tracking: [u32; 2],
}
type GetSystemProperties = unsafe extern "system" fn(instance: Handle, system: u64, properties: *mut SystemProperties) -> XrResult;

// ---- the bends and the spread

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}
fn angle(a: Vec3, b: Vec3) -> f32 {
    let la = (a.x * a.x + a.y * a.y + a.z * a.z).sqrt();
    let lb = (b.x * b.x + b.y * b.y + b.z * b.z).sqrt();
    if la < 1e-5 || lb < 1e-5 {
        return 0.0;
    }
    let mut c = (a.x * b.x + a.y * b.y + a.z * b.z) / (la * lb);
    c = if c > 1.0 {
        1.0
    } else if c < -1.0 {
        -1.0
    } else {
        c
    };
    acos(c)
}

/// A finger's bend from its five joints (metacarpal .. tip): the three flexion angles over what a closed finger
/// reaches. A fist sums to about 4 rad, a relaxed hand to 1-2 rad.
const FULL_BEND_RAD: f32 = 3.6;
fn curl(j: &[HandJointLocation; JOINT_COUNT], first: usize) -> f32 {
    let p = |k: usize| j[first + k].pose.position;
    let (b0, b1, b2, b3) = (sub(p(1), p(0)), sub(p(2), p(1)), sub(p(3), p(2)), sub(p(4), p(3)));
    let bend = angle(b0, b1) + angle(b1, b2) + angle(b2, b3);
    let c = bend / FULL_BEND_RAD;
    if c > 1.0 {
        1.0
    } else {
        c
    }
}
/// The thumb has one joint less: two flexion angles, about 1.6 rad bent.
const THUMB_FULL_BEND_RAD: f32 = 1.6;
fn thumb_curl(j: &[HandJointLocation; JOINT_COUNT]) -> f32 {
    let (p0, p1, p2, p3) = (
        j[THUMB_METACARPAL].pose.position,
        j[THUMB_PROXIMAL].pose.position,
        j[THUMB_DISTAL].pose.position,
        j[THUMB_TIP].pose.position,
    );
    let (b0, b1, b2) = (sub(p1, p0), sub(p2, p1), sub(p3, p2));
    let c = (angle(b0, b1) + angle(b1, b2)) / THUMB_FULL_BEND_RAD;
    if c > 1.0 {
        1.0
    } else {
        c
    }
}

/// SPREAD FROM THE JOINTS: the angle between neighbouring digits' first bones (thumb proximal -> distal, fingers
/// proximal -> intermediate), each projected into the palm plane (index knuckle, little knuckle, wrist), over the
/// angle of a pair fully apart: 0 touching .. 1 apart.
const SPLAY_FULL_DEG: [f32; 4] = [60.0, 25.0, 20.0, 25.0];
fn splay_from_joints(j: &[HandJointLocation; JOINT_COUNT], splay: &mut [f32; 4]) -> bool {
    let p = |k: usize| j[k].pose.position;
    let across = sub(p(LITTLE_PROXIMAL), p(INDEX_PROXIMAL));
    let down = sub(p(WRIST), p(INDEX_PROXIMAL));
    let mut n = Vec3 {
        x: across.y * down.z - across.z * down.y,
        y: across.z * down.x - across.x * down.z,
        z: across.x * down.y - across.y * down.x,
    };
    let nl = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
    if nl < 1e-6 {
        return false;
    }
    n = Vec3 { x: n.x / nl, y: n.y / nl, z: n.z / nl };
    let in_plane = |from: usize, to: usize| {
        let d = sub(p(to), p(from));
        let k = d.x * n.x + d.y * n.y + d.z * n.z;
        Vec3 { x: d.x - n.x * k, y: d.y - n.y * k, z: d.z - n.z * k }
    };
    let dir = [
        in_plane(THUMB_PROXIMAL, THUMB_DISTAL),
        in_plane(INDEX_PROXIMAL, INDEX_INTERMEDIATE),
        in_plane(MIDDLE_PROXIMAL, MIDDLE_INTERMEDIATE),
        in_plane(RING_PROXIMAL, RING_INTERMEDIATE),
        in_plane(LITTLE_PROXIMAL, LITTLE_INTERMEDIATE),
    ];
    for k in 0..4 {
        let deg = angle(dir[k], dir[k + 1]) * 57.295_779_5;
        let s = deg / SPLAY_FULL_DEG[k];
        splay[k] = if s < 0.0 {
            0.0
        } else if s > 1.0 {
            1.0
        } else {
            s
        };
    }
    true
}

/// Grip with hysteresis: ON when three of index, middle, ring and little are bent past ON, OFF when fewer than three
/// stay above OFF (Runtimes/FingerGrip.hpp, whose static_asserts pin the cases).
const ON: f32 = 0.72;
const OFF: f32 = 0.58;
pub fn update(gripping: bool, curl: &[f32; 4]) -> bool {
    // The third-highest bend = the least of the best three: one finger may read open.
    let (mut least, mut second) = (curl[0], 2.0f32);
    for &c in &curl[1..] {
        if c < least {
            second = least;
            least = c;
        } else if c < second {
            second = c;
        }
    }
    if gripping {
        second >= OFF
    } else {
        second >= ON
    }
}

// ---- the spike and zero-dropout filters (FingerFilter.hpp)

/// A jump bigger than JUMP from the shown value is held back until the new reading has stayed (within AGREE of
/// itself) for CONFIRM_MS; a spike that comes back sooner is dropped.
const JUMP: f32 = 0.40;
const AGREE: f32 = 0.15;
const CONFIRM_MS: u64 = 60;
/// An exact 0 after a bent finger is a glove's dropout, not a reading: the finger keeps its place for at most
/// ZERO_HOLD_MS.
const ZERO: f32 = 0.005;
const BENT: f32 = 0.30;
const ZERO_HOLD_MS: u64 = 15000;
const NAMES: [&str; 5] = ["thumb", "index", "middle", "ring", "little"];

#[derive(Clone, Copy)]
struct Finger {
    shown: f32,
    candidate: f32,
    /// 0 = nothing held back.
    since: u64,
    /// The value before the jump.
    from: f32,
    dropped: i32,
    taken: i32,
    longest_dropped: u64,
    /// The range of the shown value, for calibration.
    low: f32,
    high: f32,
    /// 0 = not in a zero dropout.
    zero_since: u64,
    zeros: i32,
    longest_zero: u64,
}
const FINGER0: Finger = Finger {
    shown: -1.0,
    candidate: 0.0,
    since: 0,
    from: 0.0,
    dropped: 0,
    taken: 0,
    longest_dropped: 0,
    low: 1.0,
    high: 0.0,
    zero_since: 0,
    zeros: 0,
    longest_zero: 0,
};

struct Filter {
    fingers: [[Finger; 5]; 2],
    event_lines: i32,
}
impl Filter {
    fn note(&mut self, text: String) {
        let n = self.event_lines;
        self.event_lines += 1;
        if n < 300 {
            log(&format!("[finger-filter] {}\n", text));
        }
    }
    fn filter(&mut self, hand: usize, finger: usize, raw: f32, now: u64) -> f32 {
        let side = if hand != 0 { "right" } else { "left" };
        let mut f = self.fingers[hand][finger];
        let mut lines: Vec<String> = Vec::new();
        if f.shown < 0.0 {
            f.shown = raw;
        }
        let mut early = false;
        if raw < ZERO && (f.zero_since != 0 || f.shown > BENT) {
            if f.zero_since == 0 {
                f.zero_since = now;
            }
            if now.wrapping_sub(f.zero_since) < ZERO_HOLD_MS {
                early = true;
            }
        } else if f.zero_since != 0 {
            let held = now.wrapping_sub(f.zero_since);
            f.zeros += 1;
            if held > f.longest_zero {
                f.longest_zero = held;
            }
            lines.push(format!("{} {}: zero dropout held {} ms at {:.2}, back at {:.2}", side, NAMES[finger], held, f.shown, raw));
            f.zero_since = 0;
        }
        if !early {
            if (raw - f.shown).abs() <= JUMP {
                if f.since != 0 {
                    let held = now.wrapping_sub(f.since);
                    f.dropped += 1;
                    if held > f.longest_dropped {
                        f.longest_dropped = held;
                    }
                    lines.push(format!("{} {}: spike {:.2} -> {:.2} dropped after {} ms", side, NAMES[finger], f.from, f.candidate, held));
                    f.since = 0;
                }
                f.shown = raw;
            } else if f.since == 0 {
                f.since = now;
                f.from = f.shown;
                f.candidate = raw;
            } else if (raw - f.candidate).abs() > AGREE {
                f.candidate = raw; // still moving: the new reading has to stay put from now
                f.since = now;
            } else if now.wrapping_sub(f.since) >= CONFIRM_MS {
                f.taken += 1;
                lines.push(format!("{} {}: jump {:.2} -> {:.2} taken after {} ms", side, NAMES[finger], f.from, raw, now.wrapping_sub(f.since)));
                f.shown = raw;
                f.since = 0;
            }
            if f.shown < f.low {
                f.low = f.shown;
            }
            if f.shown > f.high {
                f.high = f.shown;
            }
        }
        self.fingers[hand][finger] = f;
        for text in lines {
            self.note(text);
        }
        f.shown
    }
}

// ---- the published bends and spreads (any thread)

/// Curl: five bytes (thumb .. little); spread: four bytes (thumb-index .. ring-little); both with a 23-bit
/// millisecond stamp at bit 40 and bit 63 set once stored.
static CURL: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static SPLAY: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
const STAMP_MASK: u64 = (1u64 << 23) - 1;

/// `uint64_t(clamped * 255.0f + 0.5f)`: truncation; a NaN converts as MSVC's cvttss2si does (the integer indefinite).
fn byte(v: f32) -> u64 {
    let c = if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    };
    let x = c * 255.0 + 0.5;
    if x.is_nan() {
        0x8000_0000_0000_0000
    } else {
        x as u64
    }
}
unsafe fn store_splay(hand: usize, splay: &[f32; 4]) {
    let now = now_ms();
    SPLAY[hand].store(
        (1u64 << 63) | ((now & STAMP_MASK) << 40) | byte(splay[0]) | (byte(splay[1]) << 8) | (byte(splay[2]) << 16) | (byte(splay[3]) << 24),
        Ordering::Release,
    );
}

// ---- the XR thread's state

struct State {
    enabled: bool,
    hand_tracking: bool,
    motion_range: bool,
    /// XR_EXT_hand_tracking_data_source enabled (main's 0.2.4).
    data_source: bool,
    /// The session the trackers are made in once xr_finger_bones is on (ensure_trackers); one attempt per session.
    session: Handle,
    tried: bool,
    /// A hand's tracker was made with its data sources: only then may the locate ask which source the joints came from
    /// (SteamVR fails xrLocateHandJointsEXT for a source state on a tracker made without them).
    sources: [bool; 2],
    /// What each hand's joints last were, for the log: -1 nothing yet, 0 not active, 1 optical, 2 from the
    /// controller, 3 active with the source unknown.
    joints: [i32; 2],
    locate_error: [i32; 2],
    create: Option<CreateHandTracker>,
    destroy: Option<DestroyHandTracker>,
    locate: Option<LocateHandJoints>,
    tracker: [Handle; 2],
    gripping: [bool; 2],
    /// Once a hand has sent finger data, a pause in it keeps the last state instead of falling back to the squeeze.
    had: [bool; 2],
    filter: Filter,
}
static STATE: Mutex<State> = Mutex::new(State {
    enabled: false,
    hand_tracking: false,
    motion_range: false,
    data_source: false,
    session: 0,
    tried: false,
    sources: [false; 2],
    joints: [-1; 2],
    locate_error: [0; 2],
    create: None,
    destroy: None,
    locate: None,
    tracker: [0; 2],
    gripping: [false; 2],
    had: [false; 2],
    filter: Filter { fingers: [[FINGER0; 5]; 2], event_lines: 0 },
});
static GRIP_FROM_FINGERS: AtomicBool = AtomicBool::new(false);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
fn hand_name(hand: usize) -> &'static str {
    if hand == 0 {
        "left"
    } else {
        "right"
    }
}
impl State {
    fn hold(&self, hand: usize) -> i32 {
        if self.had[hand] {
            self.gripping[hand] as i32
        } else {
            -1
        }
    }
    /// The grip state and the values V's fingers read.
    unsafe fn use_fingers(&mut self, hand: usize, thumb: f32, curl: &[f32; 4]) -> i32 {
        if !self.had[hand] {
            log(&format!("[finger-grip] {} hand: finger data from hand tracking\n", hand_name(hand)));
        }
        self.had[hand] = true;
        let was = self.gripping[hand];
        self.gripping[hand] = update(was, curl);
        let now = now_ms();
        if thumb >= 0.0 {
            CURL[hand].store(
                (1u64 << 63)
                    | ((now & STAMP_MASK) << 40)
                    | byte(thumb)
                    | (byte(curl[0]) << 8)
                    | (byte(curl[1]) << 16)
                    | (byte(curl[2]) << 24)
                    | (byte(curl[3]) << 32),
                Ordering::Release,
            );
        }
        self.gripping[hand] as i32
    }
    fn note_joints(&mut self, hand: usize, state: i32) {
        if self.joints[hand] == state {
            return;
        }
        self.joints[hand] = state;
        const WHAT: [&str; 4] = [
            "not active: the runtime sends no joints for it (a held controller with no controller source, hands not forwarded by the streamer, or the hand out of view)",
            "active, optical hand tracking",
            "active, from the controller held in it",
            "active",
        ];
        log(&format!("[finger-grip] {} hand joints {}\n", hand_name(hand), WHAT[state as usize]));
    }
    /// The trackers, the first frame finger tracking is on in a session -- both data sources asked for, optical first:
    /// a Quest runtime (VDXR, Steam Link) otherwise sends optical joints only, and Quest does not see the hands with its
    /// cameras while they hold the controllers; with the controller source the joints are built from the held
    /// controller, optical joints still winning when the controllers are put down. False while there are none.
    unsafe fn ensure_trackers(&mut self) -> bool {
        if self.tried {
            return self.tracker[0] != 0 || self.tracker[1] != 0;
        }
        let Some(create) = self.create else { return false };
        if self.session == 0 {
            return false;
        }
        self.tried = true;
        let sources = [DATA_SOURCE_UNOBSTRUCTED, DATA_SOURCE_CONTROLLER];
        let data_sources = HandTrackingDataSourceInfo {
            ty: TYPE_HAND_TRACKING_DATA_SOURCE_INFO,
            next: core::ptr::null(),
            requested_data_source_count: 2,
            requested_data_sources: sources.as_ptr(),
        };
        for h in 0..2 {
            let mut ci = HandTrackerCreateInfo {
                ty: TYPE_HAND_TRACKER_CREATE_INFO,
                next: if self.data_source { (&data_sources as *const HandTrackingDataSourceInfo).cast() } else { core::ptr::null() },
                hand: if h == 0 { HAND_LEFT } else { HAND_RIGHT },
                hand_joint_set: HAND_JOINT_SET_DEFAULT,
            };
            let mut r = create(self.session, &ci, &mut self.tracker[h]);
            self.sources[h] = r >= 0 && !ci.next.is_null();
            if r < 0 && !ci.next.is_null() {
                // a runtime that turns the request down: the tracker as before, with its default source
                log(&format!(
                    "[finger-grip] {} hand tracker with both data sources refused ({}): the runtime's default source\n",
                    hand_name(h),
                    r
                ));
                ci.next = core::ptr::null();
                r = create(self.session, &ci, &mut self.tracker[h]);
            }
            if r < 0 {
                log(&format!("[finger-grip] {} hand tracker not created ({})\n", hand_name(h), r));
                self.tracker[h] = 0;
            }
        }
        log(&format!(
            "[finger-grip] finger tracking on: hand trackers made ({})\n",
            if self.sources[0] || self.sources[1] { "optical and controller sources" } else { "the runtime's default source" }
        ));
        self.tracker[0] != 0 || self.tracker[1] != 0
    }
    unsafe fn evaluate(&mut self, hand: usize, base: Handle, time: i64) -> i32 {
        if !self.ensure_trackers() {
            return -1;
        }
        let Some(locate) = self.locate else { return -1 };
        if self.tracker[hand] == 0 || base == 0 {
            return -1;
        }
        let mut joints = [HandJointLocation::default(); JOINT_COUNT];
        let mut source = HandTrackingDataSourceState {
            ty: TYPE_HAND_TRACKING_DATA_SOURCE_STATE,
            next: core::ptr::null_mut(),
            is_active: 0,
            data_source: 0,
        };
        let mut locations = HandJointLocations {
            ty: TYPE_HAND_JOINT_LOCATIONS,
            next: if self.sources[hand] { (&mut source as *mut HandTrackingDataSourceState).cast() } else { core::ptr::null_mut() },
            is_active: 0,
            joint_count: JOINT_COUNT as u32,
            joint_locations: joints.as_mut_ptr(),
        };
        let range = HandJointsMotionRangeInfo {
            ty: TYPE_HAND_JOINTS_MOTION_RANGE_INFO,
            next: core::ptr::null(),
            hand_joints_motion_range: MOTION_RANGE_UNOBSTRUCTED,
        };
        let info = HandJointsLocateInfo {
            ty: TYPE_HAND_JOINTS_LOCATE_INFO,
            next: if self.motion_range { &range as *const _ as *const c_void } else { core::ptr::null() },
            base_space: base,
            time,
        };
        let located = locate(self.tracker[hand], &info, &mut locations);
        if located < 0 {
            if self.locate_error[hand] != located {
                log(&format!("[finger-grip] {} hand: xrLocateHandJointsEXT failed ({})\n", hand_name(hand), located));
            }
            self.locate_error[hand] = located;
            return self.hold(hand);
        }
        self.locate_error[hand] = 0;
        if locations.is_active == 0 {
            self.note_joints(hand, 0);
            return self.hold(hand);
        }
        let joints_from = if !self.sources[hand] || source.is_active == 0 {
            3
        } else if source.data_source == DATA_SOURCE_CONTROLLER {
            2
        } else {
            1
        };
        self.note_joints(hand, joints_from);
        for j in &joints[THUMB_METACARPAL..=LITTLE_TIP] {
            if j.location_flags & POSITION_VALID_BIT == 0 {
                return self.hold(hand);
            }
        }
        let raw = [
            curl(&joints, INDEX_METACARPAL),
            curl(&joints, MIDDLE_METACARPAL),
            curl(&joints, RING_METACARPAL),
            curl(&joints, LITTLE_METACARPAL),
        ];
        let now = now_ms();
        let mut bent = [0.0f32; 4];
        for f in 0..4 {
            bent[f] = self.filter.filter(hand, f + 1, raw[f], now);
        }
        let thumb_raw = thumb_curl(&joints);
        let thumb = self.filter.filter(hand, 0, thumb_raw, now);
        let mut splay = [0.0f32; 4];
        if splay_from_joints(&joints, &mut splay) {
            store_splay(hand, &splay);
        }
        self.use_fingers(hand, thumb, &bent)
    }
}

/// xr_finger_bones from vrport.ini, read as the C++ did: fgets lines, `xr_finger_bones=%d` or `xr_finger_bones = %d`.
unsafe fn read_setting() -> i32 {
    let mut path = [0 as c_char; 1024];
    if !cvr_cpp_vrik_config_file(c"vrport.ini".as_ptr(), path.as_mut_ptr(), path.len()) || path[0] == 0 {
        return 0;
    }
    let Some(path) = super::env::ansi_path(path.as_ptr()) else { return 0 };
    let Ok(bytes) = std::fs::read(path) else { return 0 };
    for line in super::two_hand::text_lines(&bytes) {
        if let Some(v) = setting_value(&line) {
            return v;
        }
    }
    0
}
/// sscanf "xr_finger_bones = %d" (the "=%d" form is a subset): the name at the start, whitespace, '=', then %d.
fn setting_value(line: &[u8]) -> Option<i32> {
    let space = |c: u8| c == b' ' || (b'\t'..=b'\r').contains(&c);
    let mut rest = line.strip_prefix(b"xr_finger_bones")?;
    while rest.first().is_some_and(|&c| space(c)) {
        rest = &rest[1..];
    }
    rest = rest.strip_prefix(b"=")?;
    while rest.first().is_some_and(|&c| space(c)) {
        rest = &rest[1..];
    }
    let (negative, digits) = match rest.first() {
        Some(b'-') => (true, &rest[1..]),
        Some(b'+') => (false, &rest[1..]),
        _ => (false, rest),
    };
    let count = digits.iter().take_while(|c| c.is_ascii_digit()).count();
    if count == 0 {
        return None;
    }
    let mut v: i64 = 0;
    for &c in &digits[..count] {
        v = v.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    Some(if negative { v.wrapping_neg() } else { v } as i32)
}

// ---- exports (src/Anim/CharacterRigRust.cpp forwards cvr::fingergrip::* to them)

/// Instance creation: which hand-tracking extensions to enable -- bit 0 XR_EXT_hand_tracking, bit 1
/// XR_EXT_hand_joints_motion_range, bit 2 XR_EXT_hand_tracking_data_source -- given what the runtime offers. ENABLED
/// WHENEVER OFFERED, whatever xr_finger_bones says (main's 0.2.4): an extension is only chosen when the instance is
/// made, and with the switch off at that moment turning it on in the menu did nothing until the next game start.
/// Enabled and unused, they change nothing; the trackers are made only once the switch is on (ensure_trackers).
#[no_mangle]
pub unsafe extern "C" fn cvr_fingergrip_choose_extensions(has_hand_tracking: bool, has_motion_range: bool, has_data_source: bool) -> i32 {
    let _ = read_setting;
    let mut s = state();
    s.enabled = true;
    s.hand_tracking = has_hand_tracking;
    s.motion_range = has_motion_range && has_hand_tracking;
    s.data_source = has_data_source && has_hand_tracking;
    let mask = (s.hand_tracking as i32) | ((s.motion_range as i32) << 1) | ((s.data_source as i32) << 2);
    log(&format!(
        "[finger-grip] hand tracking {}, unobstructed motion range {}, data sources {}\n",
        if s.hand_tracking { "available" } else { "NOT available (normal grip)" },
        if s.motion_range { "available" } else { "not available" },
        if s.data_source { "available (optical, and the held controller)" } else { "not available (the runtime's default)" }
    ));
    mask
}

/// After xrCreateSession: the runtime's hand-tracking functions and what the system says of hand tracking; the trackers
/// are made by the first evaluation with xr_finger_bones on (ensure_trackers).
#[no_mangle]
pub unsafe extern "C" fn cvr_fingergrip_session_created(instance: Handle, system: u64, session: Handle, get_proc: Option<GetInstanceProcAddr>) {
    let mut s = state();
    if !s.enabled || !s.hand_tracking {
        return;
    }
    let Some(get_proc) = get_proc else { return };
    let mut f: VoidFn = None;
    get_proc(instance, c"xrGetSystemProperties".as_ptr(), &mut f);
    if let Some(f) = f {
        let get_properties: GetSystemProperties = core::mem::transmute(f);
        let mut hand = SystemHandTrackingProperties { ty: TYPE_SYSTEM_HAND_TRACKING_PROPERTIES, next: core::ptr::null_mut(), supports_hand_tracking: 0 };
        let mut props: SystemProperties = core::mem::zeroed();
        props.ty = TYPE_SYSTEM_PROPERTIES;
        props.next = (&mut hand as *mut SystemHandTrackingProperties).cast();
        if get_properties(instance, system, &mut props) >= 0 {
            log(&format!("[finger-grip] the system reports hand tracking supported={}\n", (hand.supports_hand_tracking != 0) as i32));
        }
    }
    s.session = session;
    s.tried = false;
    let mut f: VoidFn = core::mem::transmute(s.create);
    get_proc(instance, c"xrCreateHandTrackerEXT".as_ptr(), &mut f);
    s.create = core::mem::transmute::<VoidFn, Option<CreateHandTracker>>(f);
    let mut f: VoidFn = core::mem::transmute(s.destroy);
    get_proc(instance, c"xrDestroyHandTrackerEXT".as_ptr(), &mut f);
    s.destroy = core::mem::transmute::<VoidFn, Option<DestroyHandTracker>>(f);
    let mut f: VoidFn = core::mem::transmute(s.locate);
    get_proc(instance, c"xrLocateHandJointsEXT".as_ptr(), &mut f);
    s.locate = core::mem::transmute::<VoidFn, Option<LocateHandJoints>>(f);
    if s.create.is_none() || s.destroy.is_none() || s.locate.is_none() {
        log("[finger-grip] hand-tracking functions missing\n");
        s.create = None;
    }
}

/// Before xrDestroySession.
#[no_mangle]
pub unsafe extern "C" fn cvr_fingergrip_session_destroying() {
    let mut s = state();
    for h in 0..2 {
        if s.tracker[h] != 0 {
            if let Some(destroy) = s.destroy {
                destroy(s.tracker[h]);
                s.tracker[h] = 0;
            }
        }
    }
    s.gripping = [false; 2];
    s.had = [false; 2];
    s.joints = [-1; 2];
    s.sources = [false; 2];
    s.locate_error = [0; 2];
    s.session = 0;
    s.tried = false;
    for a in CURL.iter().chain(SPLAY.iter()) {
        a.store(0, Ordering::SeqCst);
    }
}

/// Per frame and hand (0 left, 1 right): -1 finger tracking off (xr_finger_bones, live) or no finger data yet, else the
/// grip from the fingers, 0 or 1.
#[no_mangle]
pub unsafe extern "C" fn cvr_fingergrip_evaluate(hand: i32, base: Handle, time: i64) -> i32 {
    extern "C" {
        static CyberpunkVR_FingerBones: i32;
    }
    if !(0..=1).contains(&hand) || core::ptr::read_volatile(&raw const CyberpunkVR_FingerBones) == 0 {
        return -1;
    }
    state().evaluate(hand as usize, base, time)
}

/// xr_grip_from_fingers. Live; any thread.
#[no_mangle]
pub extern "C" fn cvr_fingergrip_grip_from_fingers() -> bool {
    GRIP_FROM_FINGERS.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_fingergrip_set_grip_from_fingers(on: bool) {
    GRIP_FROM_FINGERS.store(on, Ordering::Relaxed);
}

/// The bend last measured: thumb, index, middle, ring, little, 0 straight .. 1 bent. False until that hand has sent
/// finger data; then the last values. Any thread.
#[no_mangle]
pub unsafe extern "C" fn cvr_fingergrip_tracked_curl(hand: i32, curl: *mut f32) -> bool {
    if !(0..=1).contains(&hand) {
        return false;
    }
    let v = CURL[hand as usize].load(Ordering::Acquire);
    if v >> 63 == 0 {
        return false; // no data yet; afterwards the last values hold through pauses
    }
    for d in 0..5 {
        *curl.add(d) = ((v >> (d * 8)) & 255) as f32 / 255.0;
    }
    true
}

/// The spread: thumb-index .. ring-little, 0 touching .. 1 apart. False when none is fresher than 250 ms.
#[no_mangle]
pub unsafe extern "C" fn cvr_fingergrip_tracked_splay(hand: i32, splay: *mut f32) -> bool {
    if !(0..=1).contains(&hand) {
        return false;
    }
    let v = SPLAY[hand as usize].load(Ordering::Acquire);
    if v >> 63 == 0 {
        return false;
    }
    let age = now_ms().wrapping_sub((v >> 40) & STAMP_MASK) & STAMP_MASK;
    if age > 250 {
        return false;
    }
    for d in 0..4 {
        *splay.add(d) = ((v >> (d * 8)) & 255) as f32 / 255.0;
    }
    true
}
