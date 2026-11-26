//! Shoot to Thrill's range (formerly CyberpunkVRPort_StoryAttention's vrport_wilson_range.reds, main's 7b4db33fe,
//! #270): both ready checks of mq011_00_gun_range (392 and 1366) wait for UpperBody.Aim, which a physically aimed VR
//! pistol never raises. While the objective "00b_ready" is active, every 0.1 s the held handgun or revolver aimed down
//! the range -- its muzzle within the scene's own 15-degree cone round #mq011_target_002, with the primary hand
//! tracked -- feeds the normal aim decision: the player's AimToggleListener answers GetADS true, and the aiming
//! decisions re-evaluate their enter condition on each change. The journal's state is polled at that rate instead of
//! a script callback on AimingStateDecisions.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_hooks::{Call, HookFn, Kind};
use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::sync::Mutex;

const READY: &str = "quests/minor_quest/mq011_wilson/01_competition/00b_ready";
const POLL: f64 = 0.10;

pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 3] = [
    ("AimingStateDecisions", "OnAttach", Kind::After, on_attach),
    ("AimingStateDecisions", "OnDetach", Kind::Before, on_detach),
    ("AimToggleListener", "GetADS", Kind::After, get_ads),
];

/// RED4ext's NodeRef hash: FNV-1a 64 over the path, '#' skipped and ';' runs dropped up to the next '/'.
const fn node_ref(path: &[u8]) -> u64 {
    const SEED: u64 = 0xCBF2_9CE4_8422_2325;
    let mut h = SEED;
    let mut i = 0;
    while i < path.len() {
        let c = path[i];
        if c == b'#' {
            i += 1;
            continue;
        }
        if c == b';' {
            i += 1;
            while i < path.len() && path[i] != b'/' {
                i += 1;
            }
            if i >= path.len() {
                break;
            }
        }
        h = (h ^ path[i] as u64).wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    if h == SEED {
        0
    } else {
        h
    }
}
/// VRWilsonRangeTarget: the range's target node.
pub(super) const TARGET: u64 = node_ref(b"#mq011_target_002");

struct State {
    decisions: WeakHandle,
    ready: WeakHandle,
    next: f64,
}
// SAFETY: the game thread's alone (the state machine's attach and detach, the mechanics' frame).
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
/// The listener answering GetADS true (0 for none): read by the hook without a lock.
static AIMING_LISTENER: AtomicUsize = AtomicUsize::new(0);

fn journal() -> Handle {
    super::game_system("GetJournalManager")
}
fn engine_string(text: &str) -> red4ext_abi::cstring::CString {
    let c = std::ffi::CString::new(text).unwrap_or_default();
    red4ext_abi::cstring::CString::new(&c).unwrap_or_else(|| red4ext_abi::cstring::CString::empty())
}
fn on_attach(call: &mut Call) {
    let decisions = call.this();
    let (path, class) = (engine_string(READY), engine_string("gameJournalQuestObjective"));
    let ready = rt::obj(&journal(), "GetEntryByString", &[A::P((&path as *const red4ext_abi::cstring::CString).cast()), A::P((&class as *const red4ext_abi::cstring::CString).cast())]);
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if ready.is_null() {
        *g = None;
        return;
    }
    *g = Some(State { decisions: WeakHandle::from_handle(&decisions), ready: WeakHandle::from_handle(&ready), next: 0.0 });
}
fn on_detach(call: &mut Call) {
    set_aim(&call.this(), false);
    *STATE.lock().unwrap_or_else(|p| p.into_inner()) = None;
}
fn get_ads(call: &mut Call) {
    let l = AIMING_LISTENER.load(Relaxed);
    if l != 0 && call.this().instance as usize == l {
        call.set_result_bool(true);
    }
}
/// VRSetRangeAim: the decisions' listener's flag, the enter condition re-evaluated on a change.
fn set_aim(decisions: &Handle, aim: bool) {
    if decisions.is_null() {
        return;
    }
    let listener = rt::field(decisions, "m_controlsListener").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if listener.is_null() {
        return;
    }
    let now = AIMING_LISTENER.load(Relaxed) == listener.instance as usize;
    if now == aim {
        return;
    }
    AIMING_LISTENER.store(if aim { listener.instance as usize } else { 0 }, Relaxed);
    rt::call(decisions, "UpdateEnterConditionEnabled", &[]);
}
fn has_tag(ts: &Handle, pl: &Handle, tag: &str, id: &crate::script_call::Owned) -> bool {
    rt::call(ts, "HasTag", &[A::H(pl), A::Name(CName::hashed(tag).0), A::P(id.as_ptr().cast())]).is_some_and(|o| Val::of(&o).bool())
}
/// VRWilsonAimedDownrange: the held handgun's muzzle within 15 degrees of the range target.
fn aimed_downrange(pl: &Handle) -> bool {
    let hands = super::aim_threat::input_frame();
    if hands.len() < 3 || hands[2][3] <= 0.0 {
        return false;
    }
    let ts = super::game_system("GetTransactionSystem");
    let gun = rt::obj(&ts, "GetItemInSlot", &[A::H(pl), A::Name(super::left_handed::held_slot(pl))]);
    if gun.is_null() || !super::menu_rt::is_a(&gun, "WeaponObject") {
        return false;
    }
    let Some(id) = rt::call(&gun, "GetItemID", &[]) else { return false };
    if !(has_tag(&ts, pl, "Handgun", &id) || has_tag(&ts, pl, "Revolver", &id)) {
        return false;
    }
    let Some(entity) = rt::call(pl, "GetEntityID", &[]) else { return false };
    let node = TARGET;
    let Some(ref_id) = rt::global_call("ResolveNodeRefWithEntityID", &[A::P((&node as *const u64).cast()), A::P(entity.as_ptr().cast())]) else { return false };
    let g = rt::game();
    let Some((found, outs)) = rt::static_outs("GameInstance", "GetNodeTransform", &[A::P((&g as *const crate::script_call::GameInstance).cast()), A::P(ref_id.as_ptr().cast())]) else {
        return false;
    };
    if !found.is_some_and(|f| Val::of(&f).bool()) {
        return false;
    }
    let Some(t) = outs.into_iter().next() else { return false };
    // SAFETY: a Transform out value (its position first).
    let target = unsafe { *(t.as_ptr() as *const [f32; 4]) };
    let Some((p, f)) = muzzle(&gun) else { return false };
    let d = [target[0] - p[0], target[1] - p[1], target[2] - p[2]];
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if n <= 0.0 {
        return false;
    }
    (f[0] * d[0] + f[1] * d[1] + f[2] * d[2]) / n >= 15.0f32.to_radians().cos()
}
/// The muzzle slot's world position and forward.
fn muzzle(gun: &Handle) -> Option<([f32; 3], [f32; 3])> {
    #[repr(C)]
    struct WorldTransform([u32; 8]);
    // SAFETY: the gun, held.
    let f = unsafe { crate::script_call::find_function(gun.instance, "GetMuzzleSlotWorldTransform", 0) }?;
    let mut xf = WorldTransform([0; 8]);
    // SAFETY: the transform is a plain value written into our storage.
    if !unsafe { crate::script_call::invoke_into(gun.instance, f, &[], (&mut xf as *mut WorldTransform).cast::<c_void>()) } {
        return None;
    }
    let p = super::weapon::muzzle_words_position(&xf.0);
    let [x, y, z, w] = [4, 5, 6, 7].map(|i| f32::from_bits(xf.0[i]));
    Some((p, [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)]))
}
/// The poll: while the objective is active, the aim fed every 0.1 s; otherwise let go.
pub(super) fn tick() {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let Some(s) = g.as_mut() else { return };
    let decisions = s.decisions.lock();
    if decisions.is_null() {
        AIMING_LISTENER.store(0, Relaxed);
        *g = None;
        return;
    }
    let t = super::belt::now();
    if t < s.next {
        return;
    }
    s.next = t + POLL;
    let ready = s.ready.lock();
    let active = !ready.is_null()
        && rt::num(&journal(), "GetEntryState", &[A::H(&ready)]).is_some_and(|v| v == rt::enum_value("gameJournalEntryState", "Active"));
    drop(g);
    let aim = active && aimed_downrange(&super::belt::player());
    set_aim(&decisions, aim);
}
pub(super) fn shutdown() {
    AIMING_LISTENER.store(0, Relaxed);
    *STATE.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

#[no_mangle]
/// VRWilsonRangeTarget() -> NodeRef.
///
/// # Safety
/// Called by the script VM.
pub unsafe extern "C" fn VRWilsonRangeTarget(_context: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut c_void, _a4: i64) {
    // SAFETY: the VM's frame (no arguments) and its NodeRef result.
    unsafe {
        crate::native_args::args(frame).finish();
        if !out.is_null() {
            *(out as *mut u64) = TARGET;
        }
    }
}
