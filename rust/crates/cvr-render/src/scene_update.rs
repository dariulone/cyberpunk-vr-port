//! The scene-update probe (formerly src/Stereo/NativeSceneUpdate.cpp; include/Render/NativeSceneUpdate.hpp and
//! NativeSceneScopeLiveness.hpp declare it): on request, one native scene-update producer observed through the
//! ABI-preserving thunks -- its queued payloads admitted (the four known callers' kinds, others opaque), each
//! callback's entry and return, the generic runners' input inventory, the bounds writes made under a callback -- into a
//! receipt (scene_update_receipt.rs) published to the exports after each step; the thunks' scopes kept per thread and
//! their liveness proven by an unwind walk (a scope whose frame is gone is cancelled); receipts whose groups closed
//! retired at the frame boundary. Failed or unclosed native leases are quarantined through process exit: no engine job
//! group is released from a destructor after the engine shut down.
//!
//! The .cpp keeps the exports' definitions it had, the detours' registration (CVR_DETOUR_IF over the thunks) and their
//! state, lent through the attached runtime.

use crate::native::job_lease::JobLeaseApi;
use crate::scene_update_receipt::{failure, kind, CommandInput, Identity, MutationEntry, QueueBinding, Receipt, SceneInput};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const MAX_PRODUCERS: usize = 4;
const MAX_SCOPES: usize = 16;
const MAX_FRAMES: u32 = 128;

/// scene_update::ThunkFrame: the native input/output frame the thunks saved.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ThunkFrame {
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub flags: u64,
    pub xmm: [u8; 96],
    pub caller: u64,
    pub stack: u64,
    pub arg5: u64,
}
const _: () = assert!(core::mem::size_of::<ThunkFrame>() == 0xB8);

/// What the .cpp lends: the exports and the detours' state.
#[repr(C)]
pub struct Runtime {
    pub request: *const AtomicU32,
    pub state: *const AtomicU32,
    pub compatible: *const AtomicU32,
    pub seq: *const AtomicU32,
    pub pending: *const AtomicU32,
    pub stats: *mut [u64; 16],
    pub jobs: *mut u64,
    pub scenes: *mut u64,
    pub commands: *mut u64,
    pub mutations: *mut u64,
    pub job_count: *mut u32,
    pub scene_count: *mut u32,
    pub command_count: *mut u32,
    pub mutation_count: *mut u32,
    /// All eight detours installed.
    pub hooks_ready: extern "C" fn() -> bool,
    /// The thunks' entry points by event.
    pub thunks: [Option<unsafe extern "C" fn()>; 8],
}
impl Runtime {
    // SAFETY (all): the .cpp's static, its pointers to live exports.
    fn request(&self) -> &AtomicU32 {
        unsafe { &*self.request }
    }
    fn state(&self) -> &AtomicU32 {
        unsafe { &*self.state }
    }
    fn compatible(&self) -> &AtomicU32 {
        unsafe { &*self.compatible }
    }
    fn seq(&self) -> &AtomicU32 {
        unsafe { &*self.seq }
    }
    fn pending(&self) -> &AtomicU32 {
        unsafe { &*self.pending }
    }
}
/// The plugin's table and the exports and originals it names, defined here (formerly the .cpp's); a test
/// program attaches its own.
#[cfg(feature = "rust-build")]
mod plugin {
    use super::Runtime;
    use crate::scene_update_receipt::{CommandInput, MutationEntry, SceneInput};
    use crate::used_moved_exports_rest as x;
    use core::ffi::c_void;
    unsafe extern "C" {
        fn CvrSceneUpdateQueueDetour();
        fn CvrSceneUpdateProducerDetour();
        fn CvrSceneUpdateGeneralDetour();
        fn CvrSceneUpdatePreDetour();
        fn CvrSceneUpdateMembershipDetour();
        fn CvrSceneUpdatePostDetour();
        fn CvrSceneUpdateGeneralRunnerDetour();
        fn CvrSceneUpdateLaneRunnerDetour();
        safe fn cvr_detours_installed_at(rva: usize) -> bool;
    }
    /// The thunks' originals (asm_thunks/native_scene_update_thunk.rs jumps through them; engine_detours.rs
    /// installs them), formerly src/Stereo/NativeSceneUpdate.cpp's.
    #[no_mangle]
    pub static mut CvrSceneUpdateQueueOriginal: *mut c_void = core::ptr::null_mut();
    #[no_mangle]
    pub static mut CvrSceneUpdateProducerOriginal: *mut c_void = core::ptr::null_mut();
    #[no_mangle]
    pub static mut CvrSceneUpdateGeneralOriginal: *mut c_void = core::ptr::null_mut();
    #[no_mangle]
    pub static mut CvrSceneUpdatePreOriginal: *mut c_void = core::ptr::null_mut();
    #[no_mangle]
    pub static mut CvrSceneUpdateMembershipOriginal: *mut c_void = core::ptr::null_mut();
    #[no_mangle]
    pub static mut CvrSceneUpdatePostOriginal: *mut c_void = core::ptr::null_mut();
    #[no_mangle]
    pub static mut CvrSceneUpdateGeneralRunnerOriginal: *mut c_void = core::ptr::null_mut();
    #[no_mangle]
    pub static mut CvrSceneUpdateLaneRunnerOriginal: *mut c_void = core::ptr::null_mut();
    /// The records' sizes the tools read.
    #[no_mangle]
    pub static CyberpunkVR_SceneUpdateJobBytes: u32 = 64;
    #[no_mangle]
    pub static CyberpunkVR_SceneUpdateSceneBytes: u32 = core::mem::size_of::<SceneInput>() as u32;
    #[no_mangle]
    pub static CyberpunkVR_SceneUpdateCommandBytes: u32 = core::mem::size_of::<CommandInput>() as u32;
    #[no_mangle]
    pub static CyberpunkVR_SceneUpdateMutationBytes: u32 = core::mem::size_of::<MutationEntry>() as u32;
    /// The eight detours are installed.
    extern "C" fn hooks_ready() -> bool {
        [0x272B40, 0x3D74F8, 0xD33AB4, 0xB67920, 0x3D7A54, 0xC296F4, 0x67A92C, 0x4EFF98].iter().all(|&rva| cvr_detours_installed_at(rva))
    }
    // SAFETY: process-lifetime exports and plain functions.
    unsafe impl Sync for Runtime {}
    pub(super) static RT: Runtime = Runtime {
        request: &raw const x::CyberpunkVR_SceneUpdateRequest,
        state: &raw const x::CyberpunkVR_SceneUpdateState,
        compatible: &raw const x::CyberpunkVR_SceneUpdateCompatible,
        seq: &raw const x::CyberpunkVR_SceneUpdateSeq,
        pending: &raw const x::CyberpunkVR_SceneUpdatePending,
        stats: &raw mut x::CyberpunkVR_SceneUpdateStats,
        jobs: &raw mut x::CyberpunkVR_SceneUpdateJobs,
        scenes: &raw mut x::CyberpunkVR_SceneUpdateScenes,
        commands: &raw mut x::CyberpunkVR_SceneUpdateCommands,
        mutations: &raw mut x::CyberpunkVR_SceneUpdateMutations,
        job_count: &raw mut x::CyberpunkVR_SceneUpdateJobCount,
        scene_count: &raw mut x::CyberpunkVR_SceneUpdateSceneCount,
        command_count: &raw mut x::CyberpunkVR_SceneUpdateCommandCount,
        mutation_count: &raw mut x::CyberpunkVR_SceneUpdateMutationCount,
        hooks_ready,
        thunks: [Some(CvrSceneUpdateQueueDetour), Some(CvrSceneUpdateProducerDetour), Some(CvrSceneUpdateGeneralDetour),
                 Some(CvrSceneUpdatePreDetour), Some(CvrSceneUpdateMembershipDetour), Some(CvrSceneUpdatePostDetour),
                 Some(CvrSceneUpdateGeneralRunnerDetour), Some(CvrSceneUpdateLaneRunnerDetour)],
    };
}
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: the .cpp's static.
    unsafe { RUNTIME.load(Acquire).as_ref() }
}

static HOOKS_ENABLED: AtomicBool = AtomicBool::new(false);
static IMAGE_BASE: AtomicU64 = AtomicU64::new(0);
static GUARDS: OnceLock<bool> = OnceLock::new();

/// ReadNative.
unsafe extern "C" fn read_native(address: u64, out: *mut c_void, bytes: usize) -> bool {
    if bytes == 0 {
        return address != 0 && cvr_engine::stack::read(address, &mut []);
    }
    // SAFETY: the receipt's buffer of `bytes`.
    address != 0 && address <= u64::MAX - bytes as u64 && cvr_engine::stack::read(address, unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) })
}
fn read_words<const N: usize>(address: u64) -> Option<[u8; N]> {
    let mut bytes = [0u8; N];
    // SAFETY: N bytes.
    unsafe { read_native(address, bytes.as_mut_ptr().cast(), N) }.then_some(bytes)
}
// The engine's job group calls: a reference (+0x1C), its release (0x2744CC nulls the pointer), its pending counter (+0x18).
unsafe extern "C" fn retain(group: *mut c_void) {
    // SAFETY: a live engine job group.
    unsafe { AtomicI32::from_ptr(group.cast::<u8>().add(0x1C).cast()) }.fetch_add(1, SeqCst);
}
unsafe extern "C" fn release(group: *mut *mut c_void) {
    // SAFETY: the engine's release at its RVA, for a group this probe retained.
    let function: extern "C" fn(*mut *mut c_void) = unsafe { core::mem::transmute((IMAGE_BASE.load(Relaxed) + 0x2744CC) as usize) };
    function(group);
}
unsafe extern "C" fn complete(group: *mut c_void) -> bool {
    // SAFETY: a group this probe holds.
    unsafe { AtomicI32::from_ptr(group.cast::<u8>().add(0x18).cast()) }.compare_exchange(0, 0, SeqCst, SeqCst).is_ok()
}
const JOBS_API: JobLeaseApi = JobLeaseApi { retain: Some(retain), release: Some(release), complete: Some(complete) };
fn rva(value: u64) -> u64 {
    let base = IMAGE_BASE.load(Relaxed);
    if value >= base && value - base <= u64::from(u32::MAX) {
        value - base
    } else {
        0
    }
}

// ---- the thunks' scopes and their liveness

/// scope_liveness::Anchor: a scope's cookie, its thunk's entry stack and function, the stack's limits then.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Anchor {
    pub cookie: u64,
    pub stack: u64,
    pub function: u64,
    pub low: u64,
    pub high: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Life {
    #[default]
    Unknown,
    Live,
    Absent,
}
pub fn capture_anchor(cookie: u64, stack: u64, function: u64) -> Anchor {
    let (low, high) = cvr_engine::stack::limits();
    Anchor { cookie, stack, function, low, high }
}
/// scope_liveness::Inspect: an unwind walk of this thread proves each scope's thunk frame -- its function, its entry
/// stack (RSP = stack + 8 once unwound) and its cookie at stack - 16; a frame passed without it is Absent. Other
/// stacks, broken unwind data and an exhausted budget leave it Unknown; a fault leaves them all Unknown. A new Before
/// at exactly an old entry stack (`exclude`) is a different instance.
pub fn inspect(anchors: &[Anchor], exclude: u64, max_frames: u32) -> [Life; MAX_SCOPES] {
    let mut result = [Life::Unknown; MAX_SCOPES];
    if anchors.len() > MAX_SCOPES || max_frames > MAX_FRAMES {
        return result;
    }
    let (low, high) = cvr_engine::stack::limits();
    let mut eligible = [false; MAX_SCOPES];
    for (i, a) in anchors.iter().enumerate() {
        eligible[i] = a.cookie != 0 && a.function != 0 && a.low == low && a.high == high && a.stack >= low + 16 && a.stack <= high.wrapping_sub(8) && a.stack & 7 == 0;
        if eligible[i] && exclude != 0 && a.stack == exclude {
            result[i] = Life::Absent;
        }
    }
    let walked = cvr_engine::stack::unwind_frames(max_frames, |c, begin| {
        for (i, a) in anchors.iter().enumerate() {
            if !eligible[i] || result[i] != Life::Unknown {
                continue;
            }
            if begin == a.function && c.Rsp == a.stack + 8 {
                let Some(cookie) = read_words::<8>(a.stack - 16) else { continue };
                result[i] = if u64::from_le_bytes(cookie) == a.cookie { Life::Live } else { Life::Absent };
            } else if c.Rsp >= a.stack + 8 {
                result[i] = Life::Absent;
            }
        }
        false
    });
    if walked.is_err() {
        // A failed walk cannot authorize a write attribution or cancellation.
        return [Life::Unknown; MAX_SCOPES];
    }
    result
}

#[derive(Clone, Copy)]
struct Scope {
    cookie: u64,
    stack: u64,
    event: u32,
    ordinal: u32,
    receipt: *mut Receipt,
    anchor: Anchor,
}
const NO_SCOPE: Scope = Scope { cookie: 0, stack: 0, event: 0, ordinal: 0, receipt: core::ptr::null_mut(), anchor: Anchor { cookie: 0, stack: 0, function: 0, low: 0, high: 0 } };
struct Scopes {
    scopes: [Scope; MAX_SCOPES],
    depth: usize,
}
thread_local! {
    static SCOPES: RefCell<Scopes> = const { RefCell::new(Scopes { scopes: [NO_SCOPE; MAX_SCOPES], depth: 0 }) };
}

// ---- the store (the .cpp's mutex and what it guarded)

#[derive(Clone, Copy)]
struct PayloadKey {
    receipt: *mut Receipt,
    ordinal: u32,
    kind: u32,
}
struct Store {
    next_receipt: u64,
    next_scope: u64,
    latest: u64,
    pending: Vec<Box<Receipt>>,
    payloads: BTreeMap<u64, PayloadKey>,
    jobs: Vec<[u64; 8]>,
    scenes: Vec<SceneInput>,
    commands: Vec<CommandInput>,
    mutations: Vec<MutationEntry>,
}
// SAFETY: the receipts' pointers are only used under the store's mutex.
unsafe impl Send for Store {}
static STORE: Mutex<Store> = Mutex::new(Store { next_receipt: 0, next_scope: 0, latest: 0, pending: Vec::new(), payloads: BTreeMap::new(), jobs: Vec::new(), scenes: Vec::new(), commands: Vec::new(), mutations: Vec::new() });
fn lock() -> MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Publish: the latest receipt's queues, scenes, commands and mutations into the exports (their own copies), its
/// summary in the stats, the state 2 while open, 3 closed, 4 rejected.
fn publish(rt: &Runtime, store: &mut Store, receipt: &Receipt, retired: bool) {
    let id = *receipt.identity();
    if id.nonce != store.latest {
        return;
    }
    let (mut entered, mut returned, mut opaque) = (0u64, 0u64, 0u64);
    let mut jobs = Vec::with_capacity(receipt.queues().len());
    for row in receipt.queues() {
        entered += row.entered as u64;
        returned += row.returned as u64;
        let is_opaque = row.binding.kind == kind::OPAQUE;
        opaque += is_opaque as u64;
        let flags = 1 | if row.entered { 2 } else { 0 } | if row.returned { 4 } else { 0 } | if row.inventory { 8 } else { 0 } | if is_opaque { 16 } else { 0 } | if row.cancelled { 32 } else { 0 };
        let b = row.binding;
        jobs.push([id.nonce, u64::from(b.ordinal), u64::from(b.kind), b.payload, b.prerequisite, b.completion, flags, b.caller]);
    }
    let closed = receipt.groups_closed();
    let stats: [u64; 16] = [
        id.nonce,
        id.manager,
        u64::from(id.thread),
        id.caller,
        (receipt.producer_closed() as u64) | ((receipt.producer_cancelled() as u64) << 1),
        receipt.queues().len() as u64,
        entered,
        returned,
        opaque,
        receipt.scenes().len() as u64,
        receipt.commands().len() as u64,
        receipt.mutations().len() as u64,
        u64::from(receipt.rejection()),
        closed as u64,
        0,
        retired as u64,
    ];
    rt.seq().fetch_add(1, AcqRel);
    store.jobs = jobs;
    store.scenes = receipt.scenes().to_vec();
    store.commands = receipt.commands().to_vec();
    store.mutations = receipt.mutations().to_vec();
    // SAFETY: the .cpp's exports, written under the store's mutex.
    unsafe {
        *rt.stats = stats;
        *rt.jobs = store.jobs.as_ptr() as u64;
        *rt.job_count = store.jobs.len() as u32;
        *rt.scenes = store.scenes.as_ptr() as u64;
        *rt.scene_count = store.scenes.len() as u32;
        *rt.commands = store.commands.as_ptr() as u64;
        *rt.command_count = store.commands.len() as u32;
        *rt.mutations = store.mutations.as_ptr() as u64;
        *rt.mutation_count = store.mutations.len() as u32;
    }
    rt.pending().store(store.pending.len() as u32, SeqCst);
    rt.state().store(if receipt.rejection() != failure::NONE { 4 } else if closed { 3 } else { 2 }, SeqCst);
    rt.seq().fetch_add(1, Release);
}
// SAFETY (receipt pointers): every scope and payload key names a receipt boxed in the store's pending list, which is
// retired only once its groups closed; all of them are used under the store's mutex.
fn receipt_mut<'a>(receipt: *mut Receipt) -> &'a mut Receipt {
    unsafe { &mut *receipt }
}

/// ValidateScopes: the thread's scopes whose frames are gone cancelled (a producer, a callback, else rejected), those
/// of unknown life rejected but kept; false when any life is unknown.
fn validate_scopes(rt: &Runtime, store: &mut Store, scopes: &mut Scopes, exclude: u64) -> bool {
    if scopes.depth == 0 {
        return true;
    }
    let anchors: Vec<Anchor> = scopes.scopes[..scopes.depth].iter().map(|s| s.anchor).collect();
    let life = inspect(&anchors, exclude, MAX_FRAMES);
    let (mut known, mut keep) = (true, 0usize);
    for i in 0..scopes.depth {
        let scope = scopes.scopes[i];
        let receipt = receipt_mut(scope.receipt);
        if life[i] == Life::Absent {
            match scope.event {
                1 => receipt.cancel_producer(),
                2..=5 => receipt.cancel_callback(scope.ordinal),
                _ => receipt.reject(failure::SCOPE),
            }
            publish(rt, store, receipt, false);
        } else {
            if life[i] != Life::Live {
                known = false;
                receipt.reject(failure::SCOPE);
                publish(rt, store, receipt, false);
            }
            scopes.scopes[keep] = scope;
            keep += 1;
        }
    }
    scopes.depth = keep;
    known
}
fn push(rt: &Runtime, store: &mut Store, scopes: &mut Scopes, event: u32, frame: &ThunkFrame, receipt: *mut Receipt, ordinal: u32) -> u64 {
    if scopes.depth >= MAX_SCOPES {
        receipt_mut(receipt).reject(failure::LIMIT);
        return 0;
    }
    store.next_scope += 1;
    let cookie = store.next_scope;
    let function = rt.thunks.get(event as usize).copied().flatten().map_or(0, |f| f as usize as u64);
    scopes.scopes[scopes.depth] = Scope { cookie, stack: frame.stack, event, ordinal, receipt, anchor: capture_anchor(cookie, frame.stack, function) };
    scopes.depth += 1;
    cookie
}
fn active(scopes: &Scopes, producer: bool) -> Option<usize> {
    (0..scopes.depth).rev().find(|&i| if producer { scopes.scopes[i].event == 1 } else { (2..=5).contains(&scopes.scopes[i].event) })
}
/// SourceQueue: the four known queueing callers and their callbacks (general, pre, membership, post).
fn source_queue(caller: u64) -> Option<(u32, u64)> {
    const CALLERS: [u64; 4] = [0xCA653C, 0x3D7878, 0x3D7956, 0x3D6864];
    const CALLBACKS: [u64; 4] = [0xD33AB4, 0xB67920, 0x3D7A54, 0xC296F4];
    CALLERS.iter().position(|&c| c == caller).map(|i| (i as u32, IMAGE_BASE.load(Relaxed) + CALLBACKS[i]))
}
fn before_queue(rt: &Runtime, store: &mut Store, scopes: &mut Scopes, frame: &ThunkFrame) -> u64 {
    let (producer, callback) = (active(scopes, true), active(scopes, false));
    let Some(any) = producer.or(callback) else { return 0 };
    let Some(job) = read_words::<32>(frame.rdx) else {
        receipt_mut(scopes.scopes[any].receipt).reject(failure::PAYLOAD);
        return 0;
    };
    let word = |i: usize| u64::from_le_bytes(job[i * 8..i * 8 + 8].try_into().unwrap());
    let found = source_queue(rva(frame.caller));
    let known = found.is_some() && producer.is_some();
    let scope = if known { producer.unwrap() } else { callback.unwrap_or_else(|| producer.unwrap()) };
    let pointer = scopes.scopes[scope].receipt;
    let receipt = receipt_mut(pointer);
    let k = if known { found.unwrap().0 } else { kind::OPAQUE };
    if known && word(0) != found.unwrap().1 {
        receipt.reject(failure::BINDING);
        return 0;
    }
    if known && store.payloads.contains_key(&word(1)) {
        receipt.reject(failure::BINDING);
        return 0;
    }
    let binding = QueueBinding { kind: k, ordinal: 0, caller: rva(frame.caller), callback: word(0), payload: word(1), prerequisite: frame.r9, completion: frame.arg5 };
    if let Some(ordinal) = receipt.register(binding, Some(read_native)) {
        if known {
            store.payloads.insert(word(1), PayloadKey { receipt: pointer, ordinal, kind: k });
        }
    }
    publish(rt, store, receipt, false);
    0
}
fn before_producer(rt: &Runtime, store: &mut Store, scopes: &mut Scopes, frame: &ThunkFrame) -> u64 {
    if rt.request().load(Relaxed) != 1 {
        return 0;
    }
    let caller = rva(frame.caller);
    if !matches!(caller, 0x3D6E20 | 0x3D706F | 0xB2414E) {
        return 0;
    }
    if !(rt.hooks_ready)() {
        rt.compatible().store(2, SeqCst);
        rt.state().store(4, SeqCst);
        rt.request().store(0, SeqCst);
        return 0;
    }
    if store.pending.len() >= MAX_PRODUCERS {
        rt.state().store(4, SeqCst);
        rt.request().store(0, SeqCst);
        return 0;
    }
    if rt.request().swap(0, SeqCst) == 0 {
        return 0;
    }
    store.next_receipt += 1;
    let nonce = store.next_receipt;
    // SAFETY: a plain call.
    let thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    let mut receipt = Box::new(Receipt::new(Identity { nonce, manager: frame.rcx, caller, thread, reserved: 0 }, JOBS_API));
    let pointer: *mut Receipt = &mut *receipt;
    store.pending.push(receipt);
    store.latest = nonce;
    let cookie = push(rt, store, scopes, 1, frame, pointer, u32::MAX);
    publish(rt, store, receipt_mut(pointer), false);
    cookie
}
fn before_callback(rt: &Runtime, store: &mut Store, scopes: &mut Scopes, event: u32, frame: &ThunkFrame) -> u64 {
    let Some(key) = store.payloads.remove(&frame.rcx) else { return 0 };
    let receipt = receipt_mut(key.receipt);
    let k = event - 2;
    let group = (frame.rdx <= u64::MAX - 0x28)
        .then(|| read_words::<8>(frame.rdx + 0x20))
        .flatten()
        .map(u64::from_le_bytes)
        .and_then(|parent| read_words::<8>(parent))
        .map(u64::from_le_bytes)
        .filter(|&group| group <= u64::MAX - 0x20)
        .filter(|&group| read_words::<4>(group + 0x18).is_some_and(|p| i32::from_le_bytes(p) > 0));
    let entered = group.is_some_and(|group| key.kind == k && receipt.enter(key.ordinal, k, frame.rcx, group, Some(read_native)));
    if !entered {
        receipt.reject(failure::BINDING);
        publish(rt, store, receipt, false);
        return 0;
    }
    let cookie = push(rt, store, scopes, event, frame, key.receipt, key.ordinal);
    publish(rt, store, receipt, false);
    cookie
}
fn before_runner(rt: &Runtime, store: &mut Store, scopes: &mut Scopes, event: u32, frame: &ThunkFrame) -> u64 {
    let Some(at) = active(scopes, false) else { return 0 };
    let scope = scopes.scopes[at];
    let receipt = receipt_mut(scope.receipt);
    let caller = rva(frame.caller);
    let lane = frame.r8 as u32;
    let expected = if event == 6 { caller == 0xD33AF1 } else { (caller == 0xB67963 && lane == 1) || (caller == 0xC29734 && lane == 0) };
    if !expected {
        receipt.reject(failure::BINDING);
        return 0;
    }
    receipt.inventory(scope.ordinal, frame.rcx, frame.rdx, lane, Some(read_native));
    let cookie = push(rt, store, scopes, event, frame, scope.receipt, scope.ordinal);
    publish(rt, store, receipt, false);
    cookie
}

/// PrepareCodeGuards: the probe's code contract (stereo_code.rs table 3), once.
pub fn prepare_code_guards(rt: &Runtime) {
    GUARDS.get_or_init(|| {
        // SAFETY: the executable's own module.
        let base = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 };
        IMAGE_BASE.store(base, Relaxed);
        let valid = crate::stereo_code::cvr_stereo_code(3, base);
        rt.compatible().store(if valid { 1 } else { 2 }, SeqCst);
        valid
    });
}
/// Wanted: the probe asked for (CVR_STEREO_SCENE_UPDATE=1) with its code guards valid.
pub fn wanted(rt: &Runtime) -> bool {
    let asked = std::env::var_os("CVR_STEREO_SCENE_UPDATE").is_some_and(|v| v == "1");
    if !asked {
        return false;
    }
    let enabled = GUARDS.get().copied().unwrap_or(false);
    HOOKS_ENABLED.store(enabled, SeqCst);
    rt.compatible().store(if enabled { 1 } else { 2 }, SeqCst);
    enabled
}
pub fn before(rt: &Runtime, event: u32, frame: &ThunkFrame) -> u64 {
    if !HOOKS_ENABLED.load(SeqCst) {
        return 0;
    }
    SCOPES.with(|s| {
        let mut scopes = s.borrow_mut();
        if scopes.depth == 0 && event == 1 && rt.request().load(Relaxed) != 1 {
            return 0;
        }
        let mut store = lock();
        if !validate_scopes(rt, &mut store, &mut scopes, frame.stack) {
            return 0;
        }
        match event {
            0 => before_queue(rt, &mut store, &mut scopes, frame),
            1 => before_producer(rt, &mut store, &mut scopes, frame),
            2..=5 => before_callback(rt, &mut store, &mut scopes, event, frame),
            6 | 7 => before_runner(rt, &mut store, &mut scopes, event, frame),
            _ => 0,
        }
    })
}
pub fn after(rt: &Runtime, event: u32, cookie: u64, frame: &ThunkFrame) {
    if cookie == 0 {
        return;
    }
    SCOPES.with(|s| {
        let mut scopes = s.borrow_mut();
        let mut store = lock();
        if !validate_scopes(rt, &mut store, &mut scopes, 0) {
            return;
        }
        let top = scopes.depth.checked_sub(1).map(|i| scopes.scopes[i]);
        if !top.is_some_and(|t| t.cookie == cookie && t.event == event && t.stack == frame.stack) {
            if let Some(t) = top {
                receipt_mut(t.receipt).reject(failure::SCOPE);
            }
            scopes.depth = 0;
            rt.state().store(4, SeqCst);
            return;
        }
        scopes.depth -= 1;
        let scope = scopes.scopes[scopes.depth];
        let receipt = receipt_mut(scope.receipt);
        if event == 1 {
            receipt.close_producer();
        } else if (2..=5).contains(&event) {
            receipt.returned(scope.ordinal);
        }
        publish(rt, &mut store, receipt, false);
    });
}
/// Poll: receipts whose groups closed published retired and dropped (their payload keys with them), the others again.
pub fn poll(rt: &Runtime) {
    if !HOOKS_ENABLED.load(SeqCst) {
        return;
    }
    let mut store = lock();
    let mut i = 0;
    while i < store.pending.len() {
        let pointer: *mut Receipt = &mut *store.pending[i];
        let receipt = receipt_mut(pointer);
        if receipt.groups_closed() {
            publish(rt, &mut store, receipt, true);
            store.payloads.retain(|_, key| key.receipt != pointer);
            store.pending.remove(i);
        } else {
            publish(rt, &mut store, receipt, false);
            i += 1;
        }
    }
    rt.pending().store(store.pending.len() as u32, SeqCst);
}
/// ObserveBoundsWrite: a bounds write under a live callback attributed to its queue.
pub fn observe_bounds_write(rt: &Runtime, pool: u64, indices: u64, caller: u64) {
    if !HOOKS_ENABLED.load(SeqCst) {
        return;
    }
    SCOPES.with(|s| {
        let mut scopes = s.borrow_mut();
        if scopes.depth == 0 {
            return;
        }
        let mut store = lock();
        if !validate_scopes(rt, &mut store, &mut scopes, 0) {
            return;
        }
        if let Some(at) = active(&scopes, false) {
            let scope = scopes.scopes[at];
            let receipt = receipt_mut(scope.receipt);
            // SAFETY: a plain call.
            let thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
            receipt.mutation(scope.ordinal, pool, indices, rva(caller), thread);
            publish(rt, &mut store, receipt, false);
        }
    });
}

// ---- the C ABI

/// # Safety
/// `runtime` is the .cpp's static.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_su_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_su_prepare() {
    if let Some(rt) = runtime() {
        prepare_code_guards(rt);
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_su_wanted() -> bool {
    runtime().is_some_and(wanted)
}
/// # Safety
/// `frame` is null or the thunk's saved frame.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_su_before(event: u32, frame: *const ThunkFrame) -> u64 {
    match (runtime(), unsafe { frame.as_ref() }) {
        (Some(rt), Some(frame)) => before(rt, event, frame),
        _ => 0,
    }
}
/// # Safety
/// `frame` is null or the thunk's saved frame.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_su_after(event: u32, cookie: u64, frame: *const ThunkFrame) {
    if let (Some(rt), Some(frame)) = (runtime(), unsafe { frame.as_ref() }) {
        after(rt, event, cookie, frame);
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_su_poll() {
    if let Some(rt) = runtime() {
        poll(rt);
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_su_bounds_write(pool: u64, indices: u64, caller: u64) {
    if let Some(rt) = runtime() {
        observe_bounds_write(rt, pool, indices, caller);
    }
}

#[cfg(test)]
mod tests {
    //! The scopes' liveness on this thread's own stack.
    use super::*;

    #[test]
    fn liveness_of_frames_gone_and_unknown() {
        let (low, high) = cvr_engine::stack::limits();
        let gone = Anchor { cookie: 7, stack: (low + 0x100) & !7, function: 0x1234, low, high };
        let elsewhere = Anchor { low: low + 8, ..gone };
        let excluded = Anchor { cookie: 9, stack: (low + 0x200) & !7, ..gone };
        let life = inspect(&[gone, elsewhere, excluded], excluded.stack, MAX_FRAMES);
        assert_eq!(life[0], Life::Absent, "a frame below the walk's start is passed by it");
        assert_eq!(life[1], Life::Unknown, "another stack's anchor");
        assert_eq!(life[2], Life::Absent, "a new instance at an old entry stack");
        assert_eq!(inspect(&[gone; 17], 0, MAX_FRAMES), [Life::Unknown; MAX_SCOPES], "too many scopes");
        assert_eq!(rva(0), 0);
    }
}
