//! StoryAttention (formerly src/Hooks/StoryAttention.cpp and include/Quest/StoryAttention.hpp): the targeting
//! system's look-at ray (TargetingUser_UpdateLookAtRay), right after its source's virtual call.
//!
//!   - In a scene that wants the player's attention (Pacifica's calls, workspots), the player's own ray comes from the
//!     rendered main view, not the native camera -- for the owner, entity and player the script published within the
//!     last 750 ms only.
//!   - A free hand reaching for a thing turns the player's own ray onto it from the eyes (include/Natives/Reach.hpp).
//!   - The manual clue: the physical X edge captured before the scanner's remaps, tied to one live clue, never
//!     replayed on a new target.
//!
//! The site's relay calls src/Hooks/StoryAttentionThunk.asm, which saves the volatile state and calls
//! `CvrStoryAttentionAfter` here. The script natives (src/Natives/StoryAttention.cpp) publish through the
//! `cvr_quest_*` exports.
use super::pose_transport::{cvr_pose_transport_read_main_aim, MainAimPose};
use super::roomscale_move::cvr_roomscale_player_identity;
use core::ffi::c_void;
use cvr_engine::code::{image_base, read, read_bytes};
use cvr_engine::trampoline::{allocate, patch_bytes, write_code};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// cvr::quest::Attention.
const NATIVE: u32 = 0;

/// cvr::quest::ObjectKey: a weak reference's object and its reference count block.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ObjectKey {
    pub address: usize,
    pub reference: usize,
}
impl ObjectKey {
    fn is_set(&self) -> bool {
        self.address != 0 && self.reference != 0
    }
}
fn fresh(stamp: u64, now: u64, lifetime: u64) -> bool {
    stamp != 0 && now >= stamp && now - stamp < lifetime
}
/// cvr::quest::AttentionContext.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AttentionContext {
    pub player: ObjectKey,
    pub entity: u64,
    pub stamp: u64,
    pub mode: u32,
}
impl AttentionContext {
    fn matches(&self, owner: ObjectKey, owner_entity: u64, current_player: usize, now: u64) -> bool {
        self.mode != NATIVE
            && self.player.is_set()
            && owner == self.player
            && owner_entity == self.entity
            && current_player == self.player.address
            && fresh(self.stamp, now, 750)
    }
}
/// cvr::quest::ManualClueInput: capture the physical X edge before the normal scanner/interaction remaps; keep the
/// press tied to one live clue; once claimed, the whole hold is swallowed even if the target or menu changes.
#[derive(Clone, Copy)]
struct ManualClueInput {
    armed: ObjectKey,
    pending: ObjectKey,
    stamp: u64,
    press_stamp: u64,
    seen: bool,
    down: bool,
    consumed: bool,
}
impl ManualClueInput {
    const fn new() -> Self {
        ManualClueInput {
            armed: ObjectKey { address: 0, reference: 0 },
            pending: ObjectKey { address: 0, reference: 0 },
            stamp: 0,
            press_stamp: 0,
            seen: false,
            down: false,
            consumed: false,
        }
    }
    fn suspend(&mut self) {
        self.armed = ObjectKey::default();
        self.pending = ObjectKey::default();
        self.stamp = 0;
        self.seen = false;
    }
    fn arm(&mut self, key: ObjectKey, now: u64) {
        if key != self.armed {
            self.pending = ObjectKey::default();
        }
        self.armed = key;
        self.stamp = if key.is_set() { now } else { 0 };
    }
    fn button(&mut self, pressed: bool, allowed: bool, now: u64) -> bool {
        let edge = self.seen && pressed && !self.down;
        self.seen = true;
        self.down = pressed;
        if !pressed {
            self.consumed = false;
        }
        if !allowed || !fresh(self.stamp, now, 250) {
            self.armed = ObjectKey::default();
            self.pending = ObjectKey::default();
            self.stamp = 0;
        }
        if edge && allowed && self.armed.is_set() {
            self.pending = self.armed;
            self.press_stamp = now;
            self.consumed = true;
        }
        pressed && self.consumed
    }
    fn take(&mut self, key: ObjectKey, now: u64) -> bool {
        let ready = key.is_set() && key == self.armed && key == self.pending && fresh(self.stamp, now, 250) && fresh(self.press_stamp, now, 250);
        self.pending = ObjectKey::default();
        ready
    }
}

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_StoryAttentionHookReady: i32;
    static CyberpunkVR_StoryAttentionCounters: [AtomicU64; 5];
    static CyberpunkVR_ReachTurns: AtomicU64;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_vrui_captures_input() -> bool;
    fn cvr_cpp_device_cam_active() -> bool;
    /// cvr::reach::AimPoint: the point the hand reaches for, while the script gives one (src/Natives/Reach.cpp).
    fn cvr_cpp_reach_aim_point(point: *mut f32) -> bool;
    /// src/Hooks/StoryAttentionThunk.asm.
    fn CvrStoryAttentionDetour();
}

fn count(counter: &AtomicU64) {
    // SAFETY: a plain exported int.
    if unsafe { CyberpunkVR_RuntimeDiagnostics } != 0 {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}
fn now_ms() -> u64 {
    // SAFETY: the tests' clock, or GetTickCount64.
    unsafe { super::gestures::now_ms() }
}
fn gameplay() -> bool {
    // SAFETY: C++ services.
    unsafe { cvr_cpp_menu_mode() == 0 && !cvr_cpp_vrui_captures_input() && super::xr_calls::session_running() && !cvr_cpp_device_cam_active() }
}

static ATTENTION: Mutex<AttentionContext> = Mutex::new(AttentionContext {
    player: ObjectKey { address: 0, reference: 0 },
    entity: 0,
    stamp: 0,
    mode: NATIVE,
});
struct Clue {
    input: ManualClueInput,
    player: ObjectKey,
}
static CLUE: Mutex<Clue> = Mutex::new(Clue {
    input: ManualClueInput::new(),
    player: ObjectKey { address: 0, reference: 0 },
});

/// cvr::quest::PublishAttention: the script thread's attention context.
#[no_mangle]
pub unsafe extern "C" fn cvr_quest_publish_attention(context: *const AttentionContext) {
    if let Some(c) = unsafe { context.as_ref() } {
        *ATTENTION.lock().unwrap_or_else(|p| p.into_inner()) = *c;
    }
}
/// cvr::quest::UpdateManualClue: the clue the player looks at this frame; true once for the press that claimed it.
#[no_mangle]
pub unsafe extern "C" fn cvr_quest_update_manual_clue(player: *const ObjectKey, clue: *const ObjectKey) -> bool {
    // SAFETY: the caller's keys.
    let (player, mut clue) = unsafe { (*player, *clue) };
    if !gameplay() || !player.is_set() || player.address != cvr_roomscale_player_identity() {
        clue = ObjectKey::default();
    }
    let mut c = CLUE.lock().unwrap_or_else(|p| p.into_inner());
    let now = now_ms();
    if player != c.player {
        c.input.arm(ObjectKey::default(), now);
        c.player = player;
    }
    c.input.arm(clue, now);
    let pressed = c.input.take(clue, now);
    if pressed {
        // SAFETY: the exported counters.
        count(unsafe { &CyberpunkVR_StoryAttentionCounters[4] });
    }
    pressed
}
/// cvr::quest::ConsumeManualClueButton: the pad's X, swallowed while it presses a clue.
#[no_mangle]
pub extern "C" fn cvr_quest_consume_manual_clue_button(pressed: bool, allowed: bool) -> bool {
    let mut c = CLUE.lock().unwrap_or_else(|p| p.into_inner());
    let allowed = allowed && gameplay();
    c.input.button(pressed, allowed, now_ms())
}
/// cvr::quest::SuspendManualClueInput.
#[no_mangle]
pub extern "C" fn cvr_quest_suspend_manual_clue_input() {
    let mut c = CLUE.lock().unwrap_or_else(|p| p.into_inner());
    c.input.suspend();
    c.player = ObjectKey::default();
}

/// include/Natives/Reach.hpp TurnToward: a transform (position X Y Z W, rotation X Y Z W; forward = +Y) turned onto a
/// point by the shortest arc, the position kept and the roll carried; false and untouched for a point within 5 cm or
/// past 50 m, or a rotation that is not one.
fn turn_toward(transform: &mut [f32; 8], point: &[f32; 3]) -> bool {
    use cvr_body::cmath::sqrt;
    if transform.iter().any(|v| !v.is_finite()) || point.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let n = transform[4] * transform[4] + transform[5] * transform[5] + transform[6] * transform[6] + transform[7] * transform[7];
    if n < 0.5 || n > 1.5 {
        return false;
    }
    let inv = 1.0 / sqrt(n);
    let (x, y, z, w) = (transform[4] * inv, transform[5] * inv, transform[6] * inv, transform[7] * inv);
    let d = [point[0] - transform[0], point[1] - transform[1], point[2] - transform[2]];
    let length = sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    if length < 0.05 || length > 50.0 {
        return false;
    }
    let to = [d[0] / length, d[1] / length, d[2] / length];
    let from = [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)];
    let dot = from[0] * to[0] + from[1] * to[1] + from[2] * to[2];
    let mut a = if dot < -0.9999 {
        [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y), 0.0]
    } else {
        [
            from[1] * to[2] - from[2] * to[1],
            from[2] * to[0] - from[0] * to[2],
            from[0] * to[1] - from[1] * to[0],
            1.0 + dot,
        ]
    };
    let an = sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2] + a[3] * a[3]);
    for v in a.iter_mut() {
        *v /= an;
    }
    transform[4] = a[3] * x + a[0] * w + a[1] * z - a[2] * y;
    transform[5] = a[3] * y - a[0] * z + a[1] * w + a[2] * x;
    transform[6] = a[3] * z + a[0] * y - a[1] * x + a[2] * w;
    transform[7] = a[3] * w - a[0] * x - a[1] * y - a[2] * z;
    true
}

/// The thunk's call: rcx = rbx (the targeting user's state), rdx = the look-at ray [rbp-70h] (Vector4 + Quaternion).
#[no_mangle]
pub unsafe extern "C" fn CvrStoryAttentionAfter(user: usize, transform: *mut f32) {
    if !gameplay() {
        return;
    }
    let sample = *ATTENTION.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY (whole body): the engine's targeting state through SEH reads, its ray, the plugin's counters.
    unsafe {
        let mut point = [0.0f32; 3];
        let reaching = cvr_cpp_reach_aim_point(point.as_mut_ptr());
        if sample.mode == NATIVE && !reaching {
            return;
        }
        // The native locks the weak owner at user+10/+18 before QueueEvent; user+8 is its EntityID.
        let (Some(entity), Some(address), Some(reference)) = (read::<u64>(user + 8), read::<usize>(user + 0x10), read::<usize>(user + 0x18)) else {
            return;
        };
        let owner = ObjectKey { address, reference };
        let player = cvr_roomscale_player_identity();
        if !sample.matches(owner, entity, player, now_ms()) {
            if reaching && player != 0 && owner.address == player && read::<u64>(player + 0x48) == Some(entity) {
                let t = &mut *(transform as *mut [f32; 8]);
                if turn_toward(t, &point) {
                    count(&CyberpunkVR_ReachTurns);
                }
            }
            return;
        }
        count(&CyberpunkVR_StoryAttentionCounters[0]);
        let mut pose = MainAimPose::default();
        if !cvr_pose_transport_read_main_aim(&mut pose) {
            count(&CyberpunkVR_StoryAttentionCounters[3]);
            return;
        }
        core::ptr::copy_nonoverlapping(pose.position.as_ptr(), transform, 3);
        *transform.add(3) = 1.0;
        core::ptr::copy_nonoverlapping(pose.rotation.as_ptr(), transform.add(4), 4);
        count(&CyberpunkVR_StoryAttentionCounters[sample.mode as usize]);
    }
}

/// 2.5 TargetingUser_UpdateLookAtRay (2F2430), after the source's +228 virtual call: the whole call and both
/// displaced instructions are checked before only those 8 bytes are replaced.
const SITE_CHECK: usize = 0x2F2620;
const SITE: usize = 0x2F262D;
const EXPECTED: [u8; 21] = [
    0x4c, 0x8b, 0xc0, 0x48, 0x8d, 0x55, 0x90, 0x49, 0x8b, 0xca, 0x41, 0xff, 0xd1, 0x48, 0x8b, 0x45, 0x90, 0x48, 0x8d, 0x55, 0xf0,
];

/// Installed from src/Hooks/StoryAttention.cpp's registration (Boot, 89).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_story_attention_install() -> bool {
    let base = image_base();
    let mut got = [0u8; 21];
    if base == 0 || !read_bytes(base + SITE_CHECK, &mut got) || got != EXPECTED {
        return false;
    }
    let site = base + SITE;
    let Some(relay) = allocate(site, 14) else { return false };
    let distance = relay as i64 - (site as i64 + 5);
    if distance < i32::MIN as i64 || distance > i32::MAX as i64 {
        return false;
    }
    let mut jump = [0xFFu8, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    jump[6..].copy_from_slice(&(CvrStoryAttentionDetour as *const c_void as u64).to_le_bytes());
    let mut patch = [0xE8u8, 0, 0, 0, 0, 0x90, 0x90, 0x90];
    patch[1..5].copy_from_slice(&(distance as i32).to_le_bytes());
    // SAFETY: fresh arena memory, then the checked call site.
    let ok = unsafe {
        write_code(relay, &jump);
        patch_bytes(site, &patch)
    };
    // SAFETY: an exported int.
    unsafe { CyberpunkVR_StoryAttentionHookReady = ok as i32 };
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_clue_press_is_taken_once() {
        let key = ObjectKey { address: 1, reference: 2 };
        let mut c = ManualClueInput::new();
        c.arm(key, 100);
        assert!(!c.button(false, true, 100));
        assert!(c.button(true, true, 110));
        assert!(c.take(key, 120));
        assert!(!c.take(key, 121));
    }
}

// The story module's natives (formerly src/Natives/StoryAttention.cpp), registered by src/Natives/Registration.cpp.

/// cvr::quest::Attention::Pacifica / Workspot.
const PACIFICA: u32 = 1;
const WORKSPOT: u32 = 2;

/// SelectAttention (include/Quest/StoryAttention.hpp; Ajson44's final V39 rules): walking authored scenes keep native
/// attention (the broader Tier2 rule blocked Judy's apartment doorway); passenger choices keep their native ray.
#[allow(clippy::too_many_arguments)]
pub fn select_attention(started: i32, done: i32, locomotion: i32, tier: i32, workspot: bool, mounted: bool, vehicle: i32, choices: bool) -> u32 {
    if started > 0 && done <= 0 {
        return PACIFICA;
    }
    if !mounted && tier >= 2 && locomotion != 9 {
        return NATIVE;
    }
    if mounted && choices && (vehicle == 3 || vehicle == 4 || vehicle == 7) {
        return NATIVE;
    }
    if locomotion == 9 && (tier >= 2 || workspot) {
        WORKSPOT
    } else {
        NATIVE
    }
}

fn handle_key(handle: &red4ext_abi::handle::Handle) -> ObjectKey {
    ObjectKey {
        address: handle.instance as usize,
        reference: handle.ref_count as usize,
    }
}

/// VRStoryAttentionUpdate(player: handle:entEntity, started, done, locomotion, tier, workspot, vehicle, choices,
/// paused: Int32) -> Int32 (the attention selected).
#[no_mangle]
pub unsafe extern "C" fn VRStoryAttentionUpdate(_this: *mut core::ffi::c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut i32, _unused: i64) {
    let mut player = red4ext_abi::handle::Handle::null();
    let (mut started, mut done, mut locomotion, mut tier, mut workspot, mut vehicle, mut choices, mut paused) =
        (0i32, 0i32, -1i32, 0i32, 0i32, 0i32, 0i32, 1i32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut player);
        for value in [
            &mut started,
            &mut done,
            &mut locomotion,
            &mut tier,
            &mut workspot,
            &mut vehicle,
            &mut choices,
            &mut paused,
        ] {
            frame.get_parameter(value);
        }
        frame.finish_params();
    }
    let mode = update_attention(&player, started, done, locomotion, tier, workspot, vehicle, choices, paused);
    if !out.is_null() {
        *out = mode;
    }
}
/// The attention selected for `player`'s story context (VRStoryAttentionUpdate's work; a null player or `paused`
/// selects none), published.
///
/// # Safety
/// `player` is null or a live entity's handle.
#[allow(clippy::too_many_arguments)]
pub unsafe fn update_attention(
    player: &red4ext_abi::handle::Handle,
    started: i32,
    done: i32,
    locomotion: i32,
    tier: i32,
    workspot: i32,
    vehicle: i32,
    choices: i32,
    paused: i32,
) -> i32 {
    let mut context = AttentionContext::default();
    if !player.is_null() && paused == 0 {
        context.player = handle_key(player);
        // entEntity::entityID (+0x48): its hash.
        context.entity = core::ptr::read_unaligned((player.instance as *const u8).add(0x48) as *const u64);
        context.stamp = now_ms();
        context.mode = select_attention(started, done, locomotion, tier, workspot != 0, super::env::g_isInVehicle, vehicle, choices != 0);
    }
    cvr_quest_publish_attention(&context);
    context.mode as i32
}
/// VRManualClueUpdate(player, clue: handle:entEntity) -> Int32 (1 while the press is claimed for this clue).
#[no_mangle]
pub unsafe extern "C" fn VRManualClueUpdate(_this: *mut core::ffi::c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut i32, _unused: i64) {
    let (mut player, mut clue) = (red4ext_abi::handle::Handle::null(), red4ext_abi::handle::Handle::null());
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut player);
        frame.get_parameter(&mut clue);
        frame.finish_params();
    }
    let pressed = update_manual_clue(&player, &clue);
    if !out.is_null() {
        *out = pressed;
    }
}
/// The clue press (VRManualClueUpdate's work): 1 while the press is claimed for `clue`.
///
/// # Safety
/// The handles are null or live.
pub unsafe fn update_manual_clue(player: &red4ext_abi::handle::Handle, clue: &red4ext_abi::handle::Handle) -> i32 {
    cvr_quest_update_manual_clue(&handle_key(player), &handle_key(clue)) as i32
}
