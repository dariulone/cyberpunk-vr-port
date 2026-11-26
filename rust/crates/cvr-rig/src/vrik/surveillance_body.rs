//! SurveillanceBody (formerly src/Hooks/SurveillanceBody.cpp and include/Camera/SurveillanceFollow.hpp): a taken-over
//! surveillance camera or sniper nest turns with the head. Inside DeviceCameraControlComponent's native update its
//! six-argument input reader fills the Euler deltas; after it, the head's yaw and pitch deltas are added to the mouse's
//! (the native limits, pole guard and animation feature then apply as always), and the sniper's barrel is aligned with
//! the rendered base + HMD on acquisition. The device update later adds CameraSystem's automatic rotation to the same
//! buffer: the captured input is put back and the automatic part dropped. The view keeps a manual base plus the HMD,
//! so asynchronous animation updates cannot feed the HMD back twice.
//!
//! The two sites relay to src/Hooks/SurveillanceInputThunk.asm, which calls `CvrSurveillanceInputAfter` and
//! `CvrSurveillanceFinishInput` here. The component's owner, its type and entity are read through the engine's RTTI
//! (`cvr_cpp_surveillance_control`).
use super::camera_math::cvr_camera_plausible_unit_quaternion;
use super::camera_link::{HeadPose, Vec2};
use core::ffi::c_void;
use cvr_body::cmath::{atan2, fmod, hypot, remainder, sqrt};
use cvr_engine::code::{image_base, read, read_bytes};
use cvr_engine::trampoline::{allocate, patch_span, write_code};
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

const PI: f32 = core::f32::consts::PI;
const RADIANS: f32 = PI / 180.0;

fn wrap(angle: f32) -> f32 {
    remainder(angle, 2.0 * PI)
}
fn cpp_clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}
#[derive(Clone, Copy, Default)]
struct Angles {
    yaw: f32,
    pitch: f32,
}
/// SurveillanceManualYaw: the mouse's yaw within the authored limits.
fn manual_yaw(current: f32, delta: f32, minimum: f32, maximum: f32) -> f32 {
    if !(current + delta + minimum + maximum).is_finite() {
        return 0.0;
    }
    if minimum * minimum + maximum * maximum < 1e-7 {
        return delta; // native unlimited yaw
    }
    let positive = |v: f32| {
        let v = fmod(v, 360.0);
        if v < 0.0 {
            v + 360.0
        } else {
            v
        }
    };
    let span = positive(maximum - minimum);
    let from = positive(current - minimum);
    if span < 1e-5 {
        return 0.0;
    }
    if from > span + 0.001 {
        return delta; // leave authored out-of-range startup to the engine
    }
    cpp_clamp(from + delta, 0.0, span) - from
}
/// SurveillanceHeadAngles: the head's yaw and pitch in game axes (OpenXR (x,y,z,w) -> game (x,-z,y,w), forward +Y).
fn head_angles(mut x: f32, mut y: f32, mut z: f32, mut w: f32, prior_yaw: f32) -> Option<Angles> {
    let n = x * x + y * y + z * z + w * w;
    if !n.is_finite() || n < 0.5 || n > 1.5 {
        return None;
    }
    let inv = 1.0 / sqrt(n);
    x *= inv;
    y *= inv;
    z *= inv;
    w *= inv;
    let fx = 2.0 * (-x * z - y * w);
    let fy = 1.0 - 2.0 * (x * x + y * y);
    let fz = 2.0 * (-z * y + x * w);
    let horizontal = hypot(fx, fy);
    let yaw = if horizontal > 0.02 { atan2(-fx, fy) } else { prior_yaw };
    let pitch = atan2(fz, horizontal);
    (yaw + pitch).is_finite().then_some(Angles { yaw, pitch })
}
/// SurveillanceFollow: the motor consumes head deltas; the view keeps a manual base plus the HMD.
#[derive(Clone, Copy)]
struct Follow {
    source: usize,
    entity: u64,
    origin: u64,
    stamp: u64,
    last: Angles,
    local_yaw: f32,
    valid: bool,
    head_ready: bool,
    alignment_pending: bool,
}
impl Follow {
    const fn new() -> Follow {
        Follow {
            source: 0,
            entity: 0,
            origin: 0,
            stamp: 0,
            last: Angles { yaw: 0.0, pitch: 0.0 },
            local_yaw: 0.0,
            valid: false,
            head_ready: false,
            alignment_pending: false,
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        source: usize,
        entity: u64,
        origin: u64,
        now: u64,
        head: Angles,
        native_yaw: f32,
        owner_yaw: f32,
        manual_yaw: f32,
        enabled: bool,
        align_view: bool,
        native_pitch: f32,
        manual_pitch: f32,
    ) -> Angles {
        if source == 0 || entity == 0 || origin == 0 || !(head.yaw + head.pitch + native_yaw + owner_yaw + manual_yaw + native_pitch + manual_pitch).is_finite() {
            *self = Follow::new();
            return Angles::default();
        }
        if !self.valid || self.source != source || self.entity != entity || self.origin != origin || now < self.stamp || now - self.stamp > 500 {
            self.source = source;
            self.entity = entity;
            self.origin = origin;
            self.local_yaw = wrap(native_yaw - owner_yaw);
            self.valid = true;
            self.head_ready = false;
            self.alignment_pending = align_view;
        }
        if enabled {
            self.local_yaw = wrap(self.local_yaw + manual_yaw);
        }
        let mut delta = Angles::default();
        if enabled && self.alignment_pending {
            // The sniper fires along its physical barrel: the rendered base + HMD on acquisition.
            delta = Angles { yaw: wrap(owner_yaw + self.local_yaw + head.yaw - native_yaw - manual_yaw), pitch: head.pitch - native_pitch - manual_pitch };
            self.alignment_pending = false;
        } else if enabled && self.head_ready && now >= self.stamp && now - self.stamp <= 250 {
            delta = Angles { yaw: wrap(head.yaw - self.last.yaw), pitch: head.pitch - self.last.pitch };
            // A tracking discontinuity must not slam the motor through a turn.
            if delta.yaw.abs() > PI * 0.5 || delta.pitch.abs() > PI * 0.5 {
                delta = Angles::default();
            }
        }
        self.last = head;
        self.stamp = now;
        self.head_ready = enabled;
        delta
    }
    fn view(&self, source: usize, entity: u64, origin: u64, now: u64, owner_yaw: f32) -> Option<f32> {
        if !self.valid || self.source != source || self.entity != entity || self.origin != origin || now < self.stamp || now - self.stamp > 500 || !owner_yaw.is_finite() {
            return None;
        }
        Some(wrap(owner_yaw + self.local_yaw))
    }
}
/// SurveillanceInputSample: the mouse + HMD input, kept across the automatic modifiers for the same update.
#[derive(Clone, Copy)]
struct InputSample {
    component: usize,
    input: *mut f32,
    values: [f32; 3],
}
impl InputSample {
    unsafe fn capture(&mut self, component: usize, input: *mut f32, enabled: bool) {
        self.component = if enabled { component } else { 0 };
        self.input = if enabled { input } else { core::ptr::null_mut() };
        if !self.input.is_null() {
            // SAFETY: the update's Euler buffer.
            unsafe { core::ptr::copy_nonoverlapping(self.input, self.values.as_mut_ptr(), 3) };
        }
    }
    unsafe fn finish(&mut self, component: usize, automatic: *mut f32, input: *mut f32) -> bool {
        let matched = self.component != 0 && self.component == component && self.input == input && !automatic.is_null();
        if matched {
            // SAFETY: the same update's buffers.
            unsafe {
                core::ptr::copy_nonoverlapping(self.values.as_ptr(), input, 3);
                core::ptr::write_bytes(automatic, 0, 3);
            }
        }
        self.component = 0;
        self.input = core::ptr::null_mut();
        matched
    }
}

/// What the engine's RTTI says about a device camera control component (src/Hooks/SurveillanceBody.cpp).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SurveillanceControl {
    pub owner: usize,
    pub entity: u64,
    pub enabled: bool,
    /// 0 another owner type, 1 SurveillanceCamera, 2 SniperNest.
    pub kind: u8,
}

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_SurveillanceBodyFollow: i32;
    static CyberpunkVR_DeviceCamOrient: i32;
    static CyberpunkVR_SurveillanceBodyUpdates: AtomicU64;
    static CyberpunkVR_SurveillanceBodyInjected: AtomicU64;
    static CyberpunkVR_SurveillanceBodyDebugSeq: AtomicU32;
    static mut CyberpunkVR_SurveillanceBodyDebug: [f32; 9];
    static CyberpunkVR_SurveillanceAutoFiltered: AtomicU64;
    static mut CyberpunkVR_SurveillanceAutoDebug: [f32; 6];
    static mut CvrSurveillanceInputOriginal: usize;
    static g_lensComp: AtomicUsize;
    static g_takeoverEntity: AtomicUsize;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_vrui_captures_input() -> bool;
    fn cvr_cpp_remote_cam_on() -> bool;
    fn cvr_cpp_takeover_entity_id() -> u64;
    fn cvr_xr_acquire_camera_pose_frame(head: *mut HeadPose, consumed: *mut Vec2, sequence: *mut u64) -> bool;
    fn cvr_cpp_surveillance_control(component: usize, out: *mut SurveillanceControl) -> bool;
    /// src/Hooks/SurveillanceInputThunk.asm.
    fn CvrSurveillanceInputDetour();
    fn CvrSurveillanceFinishDetour();
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
fn now_ms() -> u64 {
    // SAFETY: the tests' clock, or GetTickCount64.
    unsafe { super::gestures::now_ms() }
}

struct State {
    follow: Follow,
    source_lens: usize,
    source_owner: usize,
    source_control: usize,
    source_entity: u64,
}
static STATE: Mutex<State> = Mutex::new(State { follow: Follow::new(), source_lens: 0, source_owner: 0, source_control: 0, source_entity: 0 });
static SNIPER_ENTITY: AtomicU64 = AtomicU64::new(0);
static SNIPER_STAMP: AtomicU64 = AtomicU64::new(0);
thread_local! {
    static PENDING: Cell<InputSample> = const { Cell::new(InputSample { component: 0, input: core::ptr::null_mut(), values: [0.0; 3] }) };
}

/// A quaternion's yaw (and pitch) in game axes, forward +Y.
fn quat_yaw(at: usize, pitch: Option<&mut f32>) -> Option<f32> {
    let mut q = [0.0f32; 4];
    for (i, v) in q.iter_mut().enumerate() {
        *v = read::<f32>(at + i * 4)?;
    }
    // SAFETY: four floats.
    if !unsafe { cvr_camera_plausible_unit_quaternion(q.as_ptr()) } {
        return None;
    }
    let x = 2.0 * (q[0] * q[1] - q[2] * q[3]);
    let y = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    if x * x + y * y < 1e-6 {
        return None;
    }
    if let Some(p) = pitch {
        *p = atan2(2.0 * (q[1] * q[2] + q[0] * q[3]), hypot(x, y));
    }
    Some(atan2(-x, y))
}
fn clear() {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.follow = Follow::new();
    s.source_lens = 0;
    s.source_owner = 0;
    s.source_control = 0;
    s.source_entity = 0;
    SNIPER_ENTITY.store(0, Ordering::Release);
}

/// After the input reader: rcx = the live component (rdi), rdx = its Euler deltas (roll, pitch, yaw).
#[no_mangle]
pub unsafe extern "C" fn CvrSurveillanceInputAfter(component: usize, delta: *mut f32) {
    // SAFETY (whole body): the engine's component and buffers through SEH reads, the plugin's state and services.
    unsafe {
        PENDING.with(|p| {
            let mut s = p.get();
            s.capture(0, core::ptr::null_mut(), false);
            p.set(s)
        });
        if CyberpunkVR_SurveillanceBodyFollow == 0 || CyberpunkVR_DeviceCamOrient == 0 || !cvr_cpp_device_cam_active() {
            clear();
            return;
        }
        let mut control = SurveillanceControl::default();
        if !cvr_cpp_surveillance_control(component, &mut control) {
            return;
        }
        let owner = control.owner;
        let lens = g_lensComp.load(Ordering::Acquire);
        if owner == 0 || owner != g_takeoverEntity.load(Ordering::Acquire) || lens == 0 || !control.enabled {
            return;
        }
        let sniper = control.kind == 2;
        if control.kind == 0 {
            return;
        }
        if read::<usize>(component + 0xE0) != Some(lens) {
            return;
        }
        // This callback exists only on the native active-controller update path.
        if delta.is_null() || !(*delta + *delta.add(1) + *delta.add(2)).is_finite() {
            return;
        }
        let Some(root) = read::<usize>(owner + 0xB0).filter(|&r| r != 0) else { return };
        let Some(owner_yaw) = quat_yaw(root + 0xF0, None) else { return };
        let mut native_pitch = 0.0f32;
        let Some(native_yaw) = quat_yaw(lens + 0xF0, if sniper { Some(&mut native_pitch) } else { None }) else { return };
        let Some(feature) = read::<usize>(component + 0xC0).filter(|&f| f != 0) else { return };
        let (Some(current_yaw), Some(minimum), Some(maximum)) = (read::<f32>(feature + 0x40), read::<f32>(component + 0x110), read::<f32>(component + 0x114)) else { return };
        let manual = manual_yaw(current_yaw, *delta.add(2), minimum, maximum);
        let mut head = HeadPose::default();
        let mut consumed = Vec2::default();
        let mut sequence = 0u64;
        if !cvr_xr_acquire_camera_pose_frame(&mut head, &mut consumed, &mut sequence) || !head.valid {
            return;
        }
        let enabled = cvr_cpp_menu_mode() == 0 && !cvr_cpp_vrui_captures_input();
        let entity = control.entity;
        let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        let Some(angles) = head_angles(head.ori[0], head.ori[1], head.ori[2], head.ori[3], s.follow.last.yaw) else { return };
        let added = s.follow.step(
            component,
            entity,
            head.origin_serial,
            now_ms(),
            angles,
            native_yaw,
            owner_yaw,
            manual * RADIANS,
            enabled,
            sniper,
            native_pitch,
            *delta.add(1) * RADIANS,
        );
        s.source_lens = lens;
        s.source_owner = owner;
        s.source_control = component;
        s.source_entity = entity;
        SNIPER_STAMP.store(now_ms(), Ordering::Relaxed);
        SNIPER_ENTITY.store(if sniper { entity } else { 0 }, Ordering::Release);
        if diagnostics() {
            CyberpunkVR_SurveillanceBodyDebugSeq.fetch_add(1, Ordering::AcqRel);
            CyberpunkVR_SurveillanceBodyUpdates.fetch_add(1, Ordering::SeqCst);
            let view_yaw = s.follow.view(component, entity, head.origin_serial, now_ms(), owner_yaw).unwrap_or(0.0);
            CyberpunkVR_SurveillanceBodyDebug = [
                *delta.add(2),
                *delta.add(1),
                added.yaw / RADIANS,
                added.pitch / RADIANS,
                native_yaw / RADIANS,
                owner_yaw / RADIANS,
                angles.yaw / RADIANS,
                angles.pitch / RADIANS,
                view_yaw / RADIANS,
            ];
            if added.yaw != 0.0 || added.pitch != 0.0 {
                CyberpunkVR_SurveillanceBodyInjected.fetch_add(1, Ordering::SeqCst);
            }
            CyberpunkVR_SurveillanceBodyDebugSeq.fetch_add(1, Ordering::Release);
        }
        // The original update continues with the combined input.
        *delta.add(1) += added.pitch / RADIANS;
        *delta.add(2) += added.yaw / RADIANS;
        PENDING.with(|p| {
            let mut sample = p.get();
            sample.capture(component, delta, enabled);
            p.set(sample)
        });
    }
}
/// Before the update reads its Euler buffer back: the captured input restored, the automatic rotation dropped.
#[no_mangle]
pub unsafe extern "C" fn CvrSurveillanceFinishInput(component: usize, automatic: *mut f32, delta: *mut f32) {
    // SAFETY: the update's buffers, the plugin's exports.
    unsafe {
        let mut before = [0.0f32; 6];
        if diagnostics() {
            core::ptr::copy_nonoverlapping(delta, before.as_mut_ptr(), 3);
            core::ptr::copy_nonoverlapping(automatic, before.as_mut_ptr().add(3), 3);
        }
        if CyberpunkVR_SurveillanceBodyFollow != 0 {
            let matched = PENDING.with(|p| {
                let mut s = p.get();
                let m = s.finish(component, automatic, delta);
                p.set(s);
                m
            });
            if matched && diagnostics() {
                CyberpunkVR_SurveillanceAutoDebug = before;
                CyberpunkVR_SurveillanceAutoFiltered.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}
/// cvr::camera::SurveillanceViewYaw: the view's yaw for the lens and owner the live callback last served.
#[no_mangle]
pub unsafe extern "C" fn cvr_surveillance_view_yaw(lens: usize, owner: usize, origin: u64, owner_yaw: f32, yaw: *mut f32) -> bool {
    // SAFETY: an exported int.
    if unsafe { CyberpunkVR_SurveillanceBodyFollow } == 0 || yaw.is_null() {
        return false;
    }
    let s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if lens != s.source_lens || owner != s.source_owner {
        return false;
    }
    match s.follow.view(s.source_control, s.source_entity, origin, now_ms(), owner_yaw) {
        Some(v) => {
            // SAFETY: the caller's float.
            unsafe { *yaw = v };
            true
        }
        None => false,
    }
}
/// cvr::camera::SniperHeadFollowActive.
#[no_mangle]
pub unsafe extern "C" fn cvr_sniper_head_follow_active() -> bool {
    let entity = SNIPER_ENTITY.load(Ordering::Acquire);
    let stamp = SNIPER_STAMP.load(Ordering::Relaxed);
    let now = now_ms();
    // SAFETY: C++ services.
    unsafe {
        entity != 0 && entity == cvr_cpp_takeover_entity_id() && cvr_cpp_remote_cam_on() && now >= stamp && now - stamp < 300 && cvr_cpp_device_cam_active()
    }
}

/// Installed from src/Hooks/SurveillanceBody.cpp's registration (Boot, 88): the input reader's call inside the
/// component's update (rdi = the live component) and the read-back of the Euler buffer.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_surveillance_body_install() -> bool {
    let base = image_base();
    if base == 0 {
        return false;
    }
    let site = base + 0x2B34F4E;
    let finish = base + 0x2B351B2;
    let reader = base + 0x648844;
    let mut register = [0u8; 3];
    let mut before = [0u8; 6];
    let mut call = [0u8; 5];
    let mut finish_bytes = [0u8; 6];
    if !read_bytes(base + 0x2B34ED1, &mut register)
        || register != [0x48, 0x8b, 0xf9]
        || !read_bytes(site - 6, &mut before)
        || before != [0x4d, 0x8b, 0xc6, 0x49, 0x8b, 0xca]
        || !read_bytes(site, &mut call)
        || call[0] != 0xE8
        || (base as i64 + 0x2B34F53 + i32::from_le_bytes([call[1], call[2], call[3], call[4]]) as i64) as usize != reader
        || !read_bytes(finish, &mut finish_bytes)
        || finish_bytes != [0xf3, 0x0f, 0x10, 0x44, 0x24, 0x44]
    {
        return false;
    }
    let mut patches: Vec<(usize, Vec<u8>)> = Vec::new();
    for (at, size, thunk) in [(site, 5usize, CvrSurveillanceInputDetour as *const c_void as u64), (finish, 6, CvrSurveillanceFinishDetour as *const c_void as u64)] {
        let Some(relay) = allocate(at, 14) else { return false };
        let distance = relay as i64 - (at as i64 + 5);
        if distance < i32::MIN as i64 || distance > i32::MAX as i64 {
            return false;
        }
        let mut jump = [0xFFu8, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        jump[6..].copy_from_slice(&thunk.to_le_bytes());
        // SAFETY: fresh arena memory.
        unsafe { write_code(relay, &jump) };
        let mut bytes = vec![0xE8u8, 0, 0, 0, 0, 0x90];
        bytes[1..5].copy_from_slice(&(distance as i32).to_le_bytes());
        bytes.truncate(size);
        patches.push((at, bytes));
    }
    // SAFETY: the thunk calls the reader through this slot.
    unsafe { CvrSurveillanceInputOriginal = reader };
    let list: Vec<(usize, &[u8])> = patches.iter().map(|(a, b)| (*a, b.as_slice())).collect();
    // SAFETY: both sites were checked above; they share one code page, patched under one protection change.
    unsafe { patch_span(site, finish + 6 - site, &list) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_yaw_respects_limits() {
        assert_eq!(manual_yaw(10.0, 5.0, 0.0, 0.0), 5.0);
        assert_eq!(manual_yaw(350.0, 20.0, -20.0, 20.0), 20.0);
    }
}
