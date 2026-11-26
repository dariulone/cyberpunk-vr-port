//! Vive trackers (XR_HTCX_vive_tracker_interaction) for full-body tracking (formerly src/Fbt/Game/Trackers.cpp): with
//! xr_vive_trackers=1 in vrport.ini and the extension present, one pose action over the waist and both feet roles, its
//! spaces and a STAGE space; each XR cycle the trackers and the controllers' grips are located -- head-relative to the
//! head given (the filtered one the body is placed on), and in STAGE for the calibration, with the head taken into STAGE
//! by the base -> STAGE relation of the same instant. Once a second the rates and poses are logged. The calibration
//! (src/Fbt/Game/Calibration.cpp) is ticked from here. src/Fbt/Game/Trackers.cpp keeps the C++ names its callers use.
#![allow(clippy::missing_safety_doc, unsafe_op_in_unsafe_fn)]
use cvr_host::log;
use openxr_sys as xr;
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use xr::Handle;

const ROLES: usize = 3;
const ROLE_PATH: [&CStr; ROLES] = [
    c"/user/vive_tracker_htcx/role/waist",
    c"/user/vive_tracker_htcx/role/left_foot",
    c"/user/vive_tracker_htcx/role/right_foot",
];
const BINDING_PATH: [&CStr; ROLES] = [
    c"/user/vive_tracker_htcx/role/waist/input/grip/pose",
    c"/user/vive_tracker_htcx/role/left_foot/input/grip/pose",
    c"/user/vive_tracker_htcx/role/right_foot/input/grip/pose",
];
const ROLE_NAME: [&str; ROLES] = ["waist", "left foot", "right foot"];
const PROFILE: &CStr = c"/interaction_profiles/htc/vive_tracker_htcx";
const EXTENSION: &CStr = c"XR_HTCX_vive_tracker_interaction";

/// cvr::trackers::Pose: head-relative (OpenXR axes, metres) and in STAGE (floor centre).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Pose {
    pub valid: bool,
    pub pos: xr::Vector3f,
    pub ori: xr::Quaternionf,
    pub stage_valid: bool,
    pub stage: xr::Posef,
}
const _: () = assert!(core::mem::size_of::<Pose>() == 64);
const IDENTITY: xr::Posef = xr::Posef {
    orientation: xr::Quaternionf {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    },
    position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 },
};
impl Default for Pose {
    fn default() -> Pose {
        Pose {
            valid: false,
            pos: xr::Vector3f::default(),
            ori: xr::Quaternionf {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
            stage_valid: false,
            stage: IDENTITY,
        }
    }
}

#[repr(C)]
struct ViveTrackerPaths {
    ty: xr::StructureType,
    next: *mut c_void,
    persistent_path: xr::Path,
    role_path: xr::Path,
}
type EnumerateFn = unsafe extern "system" fn(xr::Instance, u32, *mut u32, *mut ViveTrackerPaths) -> xr::Result;
/// XR_TYPE_VIVE_TRACKER_PATHS_HTCX.
fn type_vive_tracker_paths() -> xr::StructureType {
    xr::StructureType::from_raw(1000103000)
}

#[cfg(not(test))]
extern "system" {
    fn xrGetInstanceProcAddr(instance: xr::Instance, name: *const c_char, function: *mut Option<xr::pfn::VoidFunction>) -> xr::Result;
    fn xrPathToString(instance: xr::Instance, path: xr::Path, capacity: u32, count: *mut u32, buffer: *mut c_char) -> xr::Result;
    fn xrStringToPath(instance: xr::Instance, string: *const c_char, path: *mut xr::Path) -> xr::Result;
    fn xrCreateAction(set: xr::ActionSet, info: *const xr::ActionCreateInfo, action: *mut xr::Action) -> xr::Result;
    fn xrSuggestInteractionProfileBindings(instance: xr::Instance, info: *const xr::InteractionProfileSuggestedBinding) -> xr::Result;
    fn xrCreateReferenceSpace(session: xr::Session, info: *const xr::ReferenceSpaceCreateInfo, space: *mut xr::Space) -> xr::Result;
    fn xrGetReferenceSpaceBoundsRect(session: xr::Session, ty: xr::ReferenceSpaceType, bounds: *mut xr::Extent2Df) -> xr::Result;
    fn xrCreateActionSpace(session: xr::Session, info: *const xr::ActionSpaceCreateInfo, space: *mut xr::Space) -> xr::Result;
    fn xrDestroySpace(space: xr::Space) -> xr::Result;
    fn xrGetActionStatePose(session: xr::Session, info: *const xr::ActionStateGetInfo, state: *mut xr::ActionStatePose) -> xr::Result;
    fn xrLocateSpace(space: xr::Space, base: xr::Space, time: xr::Time, location: *mut xr::SpaceLocation) -> xr::Result;
}
extern "C" {
    /// The calibration (src/Fbt/Game/Calibration.cpp): cancelled with the session, ticked while a step runs.
    fn cvr_cpp_fbt_cancel_calibration();
    fn cvr_cpp_fbt_calibration_step() -> i32;
    fn cvr_cpp_fbt_calibration_tick(view: u64, stage: u64, time: i64, trackers: *const Pose, hands: *const Pose);
}

struct State {
    enabled: bool,
    available: bool,
    instance: xr::Instance,
    session: xr::Session,
    enumerate: Option<EnumerateFn>,
    role_paths: [xr::Path; ROLES],
    pose_action: xr::Action,
    spaces: [xr::Space; ROLES],
    stage: xr::Space,
    hand_spaces: [xr::Space; 2],
    // Log state, touched only by Locate (the XR thread).
    last_enumerate: u64,
    last_report: u64,
    valid_samples: [i32; ROLES],
    samples: [i32; ROLES],
    dropouts: [i32; ROLES],
    was_valid: [bool; ROLES],
    last_count: u32,
}
static STATE: Mutex<State> = Mutex::new(State {
    enabled: false,
    available: false,
    instance: xr::Instance::NULL,
    session: xr::Session::NULL,
    enumerate: None,
    role_paths: [xr::Path::NULL; ROLES],
    pose_action: xr::Action::NULL,
    spaces: [xr::Space::NULL; ROLES],
    stage: xr::Space::NULL,
    hand_spaces: [xr::Space::NULL; 2],
    last_enumerate: 0,
    last_report: 0,
    valid_samples: [0; ROLES],
    samples: [0; ROLES],
    dropouts: [0; ROLES],
    was_valid: [false; ROLES],
    last_count: u32::MAX,
});
/// The published poses (Read, ReadHand, ReadHeadStage).
struct Published {
    poses: [Pose; ROLES],
    hands: [Pose; 2],
    head_stage: xr::Posef,
    head_stage_valid: bool,
}
static PUBLISHED: Mutex<Option<Published>> = Mutex::new(None);
static LOCATE_COUNT: AtomicU64 = AtomicU64::new(0);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}
fn published<R>(f: impl FnOnce(&mut Published) -> R) -> R {
    let mut guard = PUBLISHED.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(|| Published {
        poses: [Pose::default(); ROLES],
        hands: [Pose::default(); 2],
        head_stage: IDENTITY,
        head_stage_valid: false,
    }))
}
fn now_ms() -> u64 {
    // SAFETY: the system clock.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn failed(r: xr::Result) -> bool {
    r.into_raw() < 0
}

/// One number from vrport.ini ("key=value" or "key = value"); the fallback when it is missing.
fn read_ini(key: &str, fallback: f32) -> f32 {
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Config, "vrport.ini");
    let Ok(text) = std::fs::read(path) else { return fallback };
    for line in text.split(|&b| b == b'\n') {
        let Some(rest) = line.strip_prefix(key.as_bytes()) else { continue };
        let rest = rest.trim_ascii_start();
        let Some(rest) = rest.strip_prefix(b"=") else { continue };
        let rest = rest.trim_ascii_start();
        // scanf's %f: the longest prefix that reads as a number.
        let end = rest
            .iter()
            .position(|c| !(c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.' | b'e' | b'E')))
            .unwrap_or(rest.len());
        let mut n = end;
        while n > 0 {
            if let Some(v) = std::str::from_utf8(&rest[..n]).ok().and_then(|s| s.parse::<f32>().ok()) {
                return v;
            }
            n -= 1;
        }
    }
    fallback
}

fn conj(q: xr::Quaternionf) -> xr::Quaternionf {
    xr::Quaternionf {
        x: -q.x,
        y: -q.y,
        z: -q.z,
        w: q.w,
    }
}
fn mul(a: xr::Quaternionf, b: xr::Quaternionf) -> xr::Quaternionf {
    xr::Quaternionf {
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    }
}
fn rotate(q: xr::Quaternionf, v: xr::Vector3f) -> xr::Vector3f {
    let r = mul(
        mul(
            q,
            xr::Quaternionf {
                x: v.x,
                y: v.y,
                z: v.z,
                w: 0.0,
            },
        ),
        conj(q),
    );
    xr::Vector3f { x: r.x, y: r.y, z: r.z }
}

/// Every tracker the runtime knows and its role, logged when the count changes.
#[cfg(not(test))]
unsafe fn enumerate(s: &mut State) {
    let Some(f) = s.enumerate else { return };
    let mut count = 0u32;
    if failed(f(s.instance, 0, &mut count, core::ptr::null_mut())) || count == s.last_count {
        return;
    }
    s.last_count = count;
    let mut paths: Vec<ViveTrackerPaths> = (0..count)
        .map(|_| ViveTrackerPaths {
            ty: type_vive_tracker_paths(),
            next: core::ptr::null_mut(),
            persistent_path: xr::Path::NULL,
            role_path: xr::Path::NULL,
        })
        .collect();
    if count != 0 && failed(f(s.instance, count, &mut count, paths.as_mut_ptr())) {
        return;
    }
    log!("[trackers] {count} tracker(s) known to the runtime\n");
    let text = |path: xr::Path| -> Option<String> {
        let mut buffer = [0 as c_char; 256];
        let mut n = 0u32;
        (!failed(xrPathToString(s.instance, path, buffer.len() as u32, &mut n, buffer.as_mut_ptr())))
            .then(|| CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned())
    };
    for p in &paths {
        let persistent = if p.persistent_path == xr::Path::NULL { None } else { text(p.persistent_path) }.unwrap_or_else(|| "?".into());
        let role = if p.role_path == xr::Path::NULL {
            "(no role)".to_string()
        } else {
            text(p.role_path).unwrap_or_else(|| "?".into())
        };
        log!("[trackers]   {persistent} role={role}\n");
    }
}

/// ChooseExtensions: with xr_vive_trackers=1, the extension's name when the runtime has it (null otherwise).
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_choose_extension(available: *const xr::ExtensionProperties, count: u32) -> *const c_char {
    let mut s = state();
    s.enabled = read_ini("xr_vive_trackers", 0.0) == 1.0;
    if !s.enabled {
        return core::ptr::null();
    }
    let list = if available.is_null() {
        &[][..]
    } else {
        core::slice::from_raw_parts(available, count as usize)
    };
    if list.iter().any(|p| CStr::from_ptr(p.extension_name.as_ptr()) == EXTENSION) {
        s.available = true;
    }
    log!(
        "[trackers] xr_vive_trackers=1: {} {}\n",
        EXTENSION.to_string_lossy(),
        if s.available { "available" } else { "NOT available (no trackers)" }
    );
    if s.available {
        EXTENSION.as_ptr()
    } else {
        core::ptr::null()
    }
}
/// OnInstanceCreated: the tracker enumeration's entry point.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_on_instance_created(instance: u64) {
    let mut s = state();
    if !s.enabled || !s.available {
        return;
    }
    s.instance = xr::Instance::from_raw(instance);
    let mut f: Option<xr::pfn::VoidFunction> = None;
    xrGetInstanceProcAddr(s.instance, c"xrEnumerateViveTrackerPathsHTCX".as_ptr(), &mut f);
    s.enumerate = f.map(|f| core::mem::transmute::<xr::pfn::VoidFunction, EnumerateFn>(f));
    if s.enumerate.is_none() {
        log!("[trackers] xrEnumerateViveTrackerPathsHTCX missing\n");
        s.available = false;
        return;
    }
    enumerate(&mut s);
}
fn copy_name(dst: &mut [c_char], src: &str) {
    for (d, b) in dst.iter_mut().zip(src.bytes().chain(core::iter::once(0))) {
        *d = b as c_char;
    }
}
/// CreateActions: the pose action over the three roles, suggested for the tracker profile.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_create_actions(instance: u64, action_set: u64) {
    let mut s = state();
    if !s.enabled || !s.available || action_set == 0 {
        return;
    }
    let instance = xr::Instance::from_raw(instance);
    for r in 0..ROLES {
        if failed(xrStringToPath(instance, ROLE_PATH[r].as_ptr(), &mut s.role_paths[r])) {
            log!("[trackers] bad role path {}\n", ROLE_PATH[r].to_string_lossy());
            return;
        }
    }
    let mut info: xr::ActionCreateInfo = core::mem::zeroed();
    info.ty = xr::StructureType::ACTION_CREATE_INFO;
    info.action_type = xr::ActionType::POSE_INPUT;
    copy_name(&mut info.action_name, "vive_tracker_pose");
    copy_name(&mut info.localized_action_name, "Vive Tracker Pose");
    info.count_subaction_paths = ROLES as u32;
    info.subaction_paths = s.role_paths.as_ptr();
    let r = xrCreateAction(xr::ActionSet::from_raw(action_set), &info, &mut s.pose_action);
    if failed(r) {
        log!("[trackers] pose action not created ({})\n", r.into_raw());
        s.pose_action = xr::Action::NULL;
        return;
    }
    let mut profile = xr::Path::NULL;
    if failed(xrStringToPath(instance, PROFILE.as_ptr(), &mut profile)) {
        return;
    }
    let mut bindings = Vec::with_capacity(ROLES);
    for path in BINDING_PATH {
        let mut p = xr::Path::NULL;
        if !failed(xrStringToPath(instance, path.as_ptr(), &mut p)) {
            bindings.push(xr::ActionSuggestedBinding {
                action: s.pose_action,
                binding: p,
            });
        }
    }
    let sb = xr::InteractionProfileSuggestedBinding {
        ty: xr::StructureType::INTERACTION_PROFILE_SUGGESTED_BINDING,
        next: core::ptr::null(),
        interaction_profile: profile,
        count_suggested_bindings: bindings.len() as u32,
        suggested_bindings: bindings.as_ptr(),
    };
    let result = xrSuggestInteractionProfileBindings(instance, &sb);
    log!(
        "[trackers] suggest bindings {} -> {} (count={})\n",
        PROFILE.to_string_lossy(),
        result.into_raw(),
        bindings.len()
    );
}
/// OnSessionCreated: the STAGE space and one space per role; the controllers' grip spaces are OpenXRManager's.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_on_session_created(session: u64, hand_spaces: *const u64) {
    let mut s = state();
    if s.pose_action == xr::Action::NULL {
        return;
    }
    let session = xr::Session::from_raw(session);
    s.session = session;
    for h in 0..2 {
        s.hand_spaces[h] = if hand_spaces.is_null() {
            xr::Space::NULL
        } else {
            xr::Space::from_raw(*hand_spaces.add(h))
        };
    }
    let stage_info = xr::ReferenceSpaceCreateInfo {
        ty: xr::StructureType::REFERENCE_SPACE_CREATE_INFO,
        next: core::ptr::null(),
        reference_space_type: xr::ReferenceSpaceType::STAGE,
        pose_in_reference_space: IDENTITY,
    };
    let res = xrCreateReferenceSpace(session, &stage_info, &mut s.stage);
    if failed(res) {
        log!("[trackers] STAGE space not created ({})\n", res.into_raw());
        s.stage = xr::Space::NULL;
    } else {
        let mut bounds = xr::Extent2Df::default();
        let b = xrGetReferenceSpaceBoundsRect(session, xr::ReferenceSpaceType::STAGE, &mut bounds);
        log!(
            "[trackers] STAGE space created; play area {:.2} x {:.2} m ({})\n",
            bounds.width,
            bounds.height,
            b.into_raw()
        );
    }
    for r in 0..ROLES {
        let ci = xr::ActionSpaceCreateInfo {
            ty: xr::StructureType::ACTION_SPACE_CREATE_INFO,
            next: core::ptr::null(),
            action: s.pose_action,
            subaction_path: s.role_paths[r],
            pose_in_action_space: IDENTITY,
        };
        let res = xrCreateActionSpace(session, &ci, &mut s.spaces[r]);
        if failed(res) {
            log!("[trackers] {} space not created ({})\n", ROLE_NAME[r], res.into_raw());
            s.spaces[r] = xr::Space::NULL;
        }
    }
}
/// OnSessionDestroying: the spaces destroyed, a calibration in progress cancelled, the poses cleared.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_on_session_destroying() {
    {
        let mut s = state();
        for space in s.spaces.iter_mut() {
            if *space != xr::Space::NULL {
                xrDestroySpace(*space);
                *space = xr::Space::NULL;
            }
        }
        if s.stage != xr::Space::NULL {
            xrDestroySpace(s.stage);
            s.stage = xr::Space::NULL;
        }
        s.session = xr::Session::NULL;
        s.hand_spaces = [xr::Space::NULL; 2];
    }
    cvr_cpp_fbt_cancel_calibration();
    published(|p| {
        p.poses = [Pose::default(); ROLES];
        p.hands = [Pose::default(); 2];
    });
}

fn need() -> xr::SpaceLocationFlags {
    xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID
}
fn location() -> xr::SpaceLocation {
    xr::SpaceLocation {
        ty: xr::StructureType::SPACE_LOCATION,
        next: core::ptr::null_mut(),
        location_flags: xr::SpaceLocationFlags::EMPTY,
        pose: IDENTITY,
    }
}
#[cfg(not(test))]
unsafe fn locate(space: xr::Space, base: xr::Space, time: xr::Time) -> Option<xr::SpaceLocation> {
    let mut loc = location();
    (!failed(xrLocateSpace(space, base, time, &mut loc))).then_some(loc)
}
fn relative(head: &xr::Posef, head_inv: xr::Quaternionf, pose: &xr::Posef) -> (xr::Vector3f, xr::Quaternionf) {
    let d = xr::Vector3f {
        x: pose.position.x - head.position.x,
        y: pose.position.y - head.position.y,
        z: pose.position.z - head.position.z,
    };
    (rotate(head_inv, d), mul(head_inv, pose.orientation))
}

/// Locate (each XR cycle, the XR thread): the trackers and the grips located, published, the calibration ticked.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_locate(base: u64, view: u64, time: i64, head: *const xr::Posef) {
    let Some(head) = head.as_ref() else { return };
    let mut s = state();
    if s.session == xr::Session::NULL {
        return;
    }
    let (base, view, time) = (xr::Space::from_raw(base), xr::Space::from_raw(view), xr::Time::from_nanos(time));
    let now = now_ms();
    if now.wrapping_sub(s.last_enumerate) >= 2000 {
        s.last_enumerate = now;
        enumerate(&mut s);
    }
    let head_inv = conj(head.orientation);
    let mut poses = [Pose::default(); ROLES];
    let mut active = [false; ROLES];
    let mut flags = [0u64; ROLES];
    for r in 0..ROLES {
        if s.spaces[r] == xr::Space::NULL {
            continue;
        }
        let get = xr::ActionStateGetInfo {
            ty: xr::StructureType::ACTION_STATE_GET_INFO,
            next: core::ptr::null(),
            action: s.pose_action,
            subaction_path: s.role_paths[r],
        };
        let mut state_pose = xr::ActionStatePose {
            ty: xr::StructureType::ACTION_STATE_POSE,
            next: core::ptr::null_mut(),
            is_active: xr::FALSE,
        };
        active[r] = !failed(xrGetActionStatePose(s.session, &get, &mut state_pose)) && state_pose.is_active.into();
        if active[r] {
            if let Some(loc) = locate(s.spaces[r], base, time) {
                flags[r] = loc.location_flags.into_raw();
                if loc.location_flags.contains(need()) {
                    poses[r].valid = true;
                    (poses[r].pos, poses[r].ori) = relative(head, head_inv, &loc.pose);
                }
            }
            if s.stage != xr::Space::NULL {
                if let Some(loc) = locate(s.spaces[r], s.stage, time).filter(|l| l.location_flags.contains(need())) {
                    poses[r].stage_valid = true;
                    poses[r].stage = loc.pose;
                }
            }
        }
        s.samples[r] += 1;
        if poses[r].valid {
            s.valid_samples[r] += 1;
        } else if s.was_valid[r] {
            s.dropouts[r] += 1;
        }
        s.was_valid[r] = poses[r].valid;
    }
    // The controllers (grip poses), the same way: head-relative for the helpers, STAGE for the calibration.
    let mut hands = [Pose::default(); 2];
    for h in 0..2 {
        if s.hand_spaces[h] == xr::Space::NULL {
            continue;
        }
        if let Some(loc) = locate(s.hand_spaces[h], base, time).filter(|l| l.location_flags.contains(need())) {
            hands[h].valid = true;
            (hands[h].pos, hands[h].ori) = relative(head, head_inv, &loc.pose);
        }
        if s.stage != xr::Space::NULL {
            if let Some(loc) = locate(s.hand_spaces[h], s.stage, time).filter(|l| l.location_flags.contains(need())) {
                hands[h].stage_valid = true;
                hands[h].stage = loc.pose;
            }
        }
    }
    let head_loc = if s.stage != xr::Space::NULL && view != xr::Space::NULL {
        locate(view, s.stage, time)
    } else {
        None
    };
    let head_ok = head_loc.is_some_and(|l| l.location_flags.contains(need()));
    // The head the body is measured from is the head it is placed on: the given (filtered) head taken into STAGE.
    let mut head_stage = head_loc.map_or(IDENTITY, |l| l.pose);
    if head_ok {
        if let Some(in_base) = locate(view, base, time).filter(|l| l.location_flags.contains(need())) {
            let hl = head_loc.unwrap_or(location()).pose;
            let q = mul(hl.orientation, conj(in_base.pose.orientation));
            let rb = rotate(q, in_base.pose.position);
            let t = xr::Vector3f {
                x: hl.position.x - rb.x,
                y: hl.position.y - rb.y,
                z: hl.position.z - rb.z,
            };
            let hp = rotate(q, head.position);
            head_stage.position = xr::Vector3f {
                x: t.x + hp.x,
                y: t.y + hp.y,
                z: t.z + hp.z,
            };
            head_stage.orientation = mul(q, head.orientation);
        }
    }
    published(|p| {
        p.poses = poses;
        p.head_stage_valid = head_ok;
        if head_ok {
            p.head_stage = head_stage;
        }
        LOCATE_COUNT.fetch_add(1, Ordering::Relaxed);
        p.hands = hands;
    });
    let stage = s.stage;
    if cvr_cpp_fbt_calibration_step() != 0 {
        drop(s);
        cvr_cpp_fbt_calibration_tick(view.into_raw(), stage.into_raw(), time.as_nanos(), poses.as_ptr(), hands.as_ptr());
        s = state();
    }
    if now.wrapping_sub(s.last_report) < 1000 {
        return;
    }
    let seconds = if s.last_report != 0 {
        now.wrapping_sub(s.last_report) as f32 / 1000.0
    } else {
        1.0
    };
    s.last_report = now;
    if s.stage != xr::Space::NULL {
        // The head in STAGE and LOCAL in STAGE: the change of base between the port's space and the floor centre.
        let hs = if view != xr::Space::NULL { locate(view, s.stage, time) } else { None };
        let hs_ok = hs.is_some_and(|l| l.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID));
        let ls = locate(base, s.stage, time);
        let ls_ok = ls.is_some_and(|l| l.location_flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID));
        let hp = hs.unwrap_or(location()).pose.position;
        let lp = ls.unwrap_or(location()).pose;
        let up = rotate(lp.orientation, xr::Vector3f { x: 0.0, y: 1.0, z: 0.0 });
        let fwd = rotate(lp.orientation, xr::Vector3f { x: 0.0, y: 0.0, z: -1.0 });
        let tilt = up.y.clamp(-1.0, 1.0).acos() * 57.29578;
        let yaw = (-fwd.x).atan2(-fwd.z) * 57.29578;
        log!(
            "[trackers] head stage=({:+.3} {:+.3} {:+.3}){} | LOCAL in STAGE pos=({:+.3} {:+.3} {:+.3}) yaw={:+.1} tilt={:.2} deg{}\n",
            hp.x,
            hp.y,
            hp.z,
            if hs_ok { "" } else { " (invalid)" },
            lp.position.x,
            lp.position.y,
            lp.position.z,
            yaw,
            tilt,
            if ls_ok { "" } else { " (invalid)" }
        );
    }
    for r in 0..ROLES {
        let p = &poses[r];
        log!(
            "[trackers] {:<10} active={} flags=0x{:02X} valid={:.0}/s of {:.0}/s drops={} rel=({:+.3} {:+.3} {:+.3}) ori=({:+.3} {:+.3} {:+.3} {:+.3}) stage=({:+.3} {:+.3} {:+.3})\n",
            ROLE_NAME[r],
            active[r] as i32,
            flags[r] as u32,
            s.valid_samples[r] as f32 / seconds,
            s.samples[r] as f32 / seconds,
            s.dropouts[r],
            p.pos.x,
            p.pos.y,
            p.pos.z,
            p.ori.x,
            p.ori.y,
            p.ori.z,
            p.ori.w,
            p.stage.position.x,
            p.stage.position.y,
            p.stage.position.z
        );
        s.samples[r] = 0;
        s.valid_samples[r] = 0;
        s.dropouts[r] = 0;
    }
}

/// Running: a session with the trackers' spaces.
#[no_mangle]
pub extern "C" fn cvr_trackers_running() -> bool {
    state().session != xr::Session::NULL
}
/// ReadHand: a controller's grip (0 left, 1 right); its validity.
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_read_hand(hand: i32, out: *mut Pose) -> bool {
    let Some(out) = out.as_mut() else { return false };
    if !(0..=1).contains(&hand) {
        return false;
    }
    *out = published(|p| p.hands[hand as usize]);
    out.valid
}
/// LocateCount: the cycles located so far.
#[no_mangle]
pub extern "C" fn cvr_trackers_locate_count() -> u64 {
    LOCATE_COUNT.load(Ordering::Relaxed)
}
/// ReadHeadStage: the head in STAGE (the filtered head); its validity.
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_read_head_stage(out: *mut xr::Posef) -> bool {
    let Some(out) = out.as_mut() else { return false };
    published(|p| {
        *out = p.head_stage;
        p.head_stage_valid
    })
}
/// Read: one role's pose; its validity.
#[no_mangle]
pub unsafe extern "C" fn cvr_trackers_read(role: i32, out: *mut Pose) -> bool {
    let Some(out) = out.as_mut() else { return false };
    if !(0..ROLES as i32).contains(&role) {
        return false;
    }
    *out = published(|p| p.poses[role as usize]);
    out.valid
}
