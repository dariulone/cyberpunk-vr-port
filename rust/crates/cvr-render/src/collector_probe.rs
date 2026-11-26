//! The CPU collector probe (formerly src/Stereo/CpuCollectorProbe.cpp): request 1 clones the second view's collector
//! at its lifecycle call and checks the clone against the source; requests 2..5 at the VRCAM query initializer --
//! an owned collector cloned from the source query's inputs and queried through the engine's own initializer (2), its
//! fine culling ranges captured against the native calls (3), the scene pool captured and evaluated (4), and a peer
//! eye predicted at MAIN's pose with its own collector, query, fine ranges and scene (5) --; the job groups held until
//! both native completions; the comparisons published once everything finished (the bounds trace closed first).
//!
//! The .cpp keeps the two detours' frames (their return addresses) and lends the stereo state through the runtime;
//! the exports are early_depth_exports' (the changed bounds, a C++ record, stay the .cpp's).

use crate::collector_compare::{compare_query, different_ids, QueryComparison};
use crate::collector_input::{collector_input, inputs_unchanged, vrcam_view, CAMERA_BYTES, VIEW_FLAGS_BYTES};
use crate::collector_peer::{candidate_type, id_overlap, main_projection, peer_view};
use crate::cpu_collector::{Api, Collector, Initialize, Input};
use crate::early_depth_exports as x;
use crate::fine_visibility::{fine_candidate_count, NativeCullCandidate, NativeFineCull};
use crate::native::job_lease::{JobLeaseApi, NativeJobLease};
use crate::scene_visibility::{run_fine, NativeCoarseCull, SceneVisibility};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::{Mutex, MutexGuard};

/// bounds_trace::Kind's capture marks.
mod trace_kind {
    pub const VRCAM_BEGIN: u32 = 3;
    pub const VRCAM_END: u32 = 4;
    pub const MAIN_BEGIN: u32 = 5;
    pub const MAIN_END: u32 = 6;
}
pub type QueryInitialize = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> usize;
pub type MainCullTest = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) -> i64;
/// The peer's changed bounds (the .cpp's ChangedBounds).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ChangedBounds {
    pub id: u64,
    pub kind: u32,
    pub prepare: u32,
    pub before: [f32; 6],
    pub after: [f32; 6],
}
/// What the .cpp lends: the stereo state, the detours' and the culling hooks' originals and the changed bounds.
#[repr(C)]
pub struct Runtime {
    pub exe_base: extern "C" fn() -> u64,
    /// g_menuModeValue != 0.
    pub menu_mode: extern "C" fn() -> bool,
    /// g_main_view_ctx.
    pub main_view: extern "C" fn() -> u64,
    /// g_vrcam_ctx_key.
    pub vrcam_key: extern "C" fn() -> u64,
    /// CyberpunkVR_CullReuseMode != 0.
    pub cull_reuse: extern "C" fn() -> bool,
    pub second_view_wanted: extern "C" fn() -> bool,
    /// projection::Active.
    pub projection_active: extern "C" fn() -> bool,
    pub fine_materialize: *const Option<NativeFineCull>,
    pub main_cull_test: *const Option<MainCullTest>,
    pub peer_changes: *mut ChangedBounds,
}
// SAFETY: immutable; its pointers name process-lifetime C++ globals.
unsafe impl Sync for Runtime {}
const _: () = assert!(core::mem::size_of::<ChangedBounds>() == 64 && core::mem::size_of::<Runtime>() == 80);
/// The detours' trampolines: the collector's lifecycle (its initialize) and the query tasks' (registered from
/// engine_detours.rs).
#[no_mangle]
pub static cvr_ccp_originals: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
fn collector_original() -> Option<Initialize> {
    let raw = cvr_ccp_originals[0].load(Acquire);
    // SAFETY: the lifecycle detour's trampoline, written once at its installation.
    (raw != 0).then(|| unsafe { core::mem::transmute::<usize, Initialize>(raw) })
}
/// The plugin's table and the exports and originals it names, defined here (formerly the .cpp's); a test
/// program attaches its own.
#[cfg(feature = "rust-build")]
mod plugin {
    use super::{ChangedBounds, MainCullTest, NativeFineCull, Runtime};
    use core::sync::atomic::Ordering::{Acquire, SeqCst};
    extern "C" fn exe_base() -> u64 {
        // SAFETY: the game module base, set once at the stereo install.
        unsafe { core::ptr::read_volatile(&raw const crate::stereo_shared::g_exe_base) as u64 }
    }
    extern "C" fn vrcam_key() -> u64 {
        crate::moved_globals::g_vrcam_ctx_key.load(SeqCst)
    }
    unsafe extern "C" {
        /// volatile int g_menuModeValue (src/Core/VrCore.cpp, C linkage).
        static g_menuModeValue: i32;
    }
    /// The peer view's changed bounds (formerly src/Stereo/CpuCollectorProbe.cpp's).
    #[no_mangle]
    pub static mut CyberpunkVR_CpuCollectorPeerChanges: [ChangedBounds; 64] = unsafe { core::mem::zeroed() };
    extern "C" fn menu_mode() -> bool {
        // SAFETY: the plugin's menu flag.
        unsafe { core::ptr::read_volatile(&raw const g_menuModeValue) != 0 }
    }
    extern "C" fn main_view() -> u64 {
        crate::moved_globals::g_main_view_ctx.load(Acquire) as u64
    }
    extern "C" fn cull_reuse() -> bool {
        // SAFETY: the plugin's export.
        unsafe { core::ptr::read_volatile(&raw const crate::declared_moved_exports::CyberpunkVR_CullReuseMode) != 0 }
    }
    /// engine_stereo::SecondViewWanted.
    extern "C" fn second_view_wanted() -> bool {
        !crate::engine_stereo::cvr_engine_stereo_enabled() && !crate::engine_stereo::cvr_engine_stereo_mono_launch()
    }
    extern "C" fn projection_active() -> bool {
        crate::eye_windows::projection_active()
    }
    pub(super) static RT: Runtime = Runtime {
        exe_base,
        menu_mode,
        main_view,
        vrcam_key,
        cull_reuse,
        second_view_wanted,
        projection_active,
        // cull_visibility.rs's originals: [3] FineMaterialize, [2] MainCullTest
        fine_materialize: (&raw const crate::cull_visibility::cvr_cull_stage_originals[3]).cast::<Option<NativeFineCull>>(),
        main_cull_test: (&raw const crate::cull_visibility::cvr_cull_stage_originals[2]).cast::<Option<MainCullTest>>(),
        peer_changes: (&raw mut CyberpunkVR_CpuCollectorPeerChanges).cast(),
    };
}
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn rt() -> Option<&'static Runtime> {
    // SAFETY: attached once with a static runtime.
    unsafe { RUNTIME.load(Acquire).as_ref() }
}
fn base() -> u64 {
    rt().map_or(0, |r| (r.exe_base)())
}
fn fine_materialize() -> Option<NativeFineCull> {
    // SAFETY: the .cpp's hook original, set before its detour is enabled.
    rt().and_then(|r| unsafe { r.fine_materialize.as_ref() }.copied().flatten())
}
fn main_cull_test() -> Option<MainCullTest> {
    // SAFETY: as fine_materialize.
    rt().and_then(|r| unsafe { r.main_cull_test.as_ref() }.copied().flatten())
}
fn engine<T: Copy>(rva: u64) -> T {
    // SAFETY: a function of the executable at its guarded RVA.
    unsafe { core::mem::transmute_copy(&(base() + rva)) }
}
fn read(address: u64, out: &mut [u8]) -> bool {
    address != 0 && crate::geometry_packets::read(address, out)
}
fn read_u64(address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn read_pair(address: u64) -> Option<[u64; 2]> {
    let mut b = [0u8; 16];
    read(address, &mut b).then(|| [u64::from_le_bytes(b[..8].try_into().unwrap()), u64::from_le_bytes(b[8..].try_into().unwrap())])
}
macro_rules! export {
    ($name:ident) => {
        // SAFETY: the export, written under the probe's mutex or before any reader.
        unsafe { &mut *core::ptr::addr_of_mut!(x::$name) }
    };
}

// The engine's job group calls: a reference (+0x1C), its release (0x2744CC nulls the pointer), its pending counter (+0x18).
unsafe extern "C" fn retain_job(group: *mut c_void) {
    // SAFETY: a live engine job group.
    unsafe { AtomicI32::from_ptr(group.cast::<u8>().add(0x1C).cast()) }.fetch_add(1, SeqCst);
}
unsafe extern "C" fn release_job(group: *mut *mut c_void) {
    // SAFETY: the engine's release, for a group this probe retained.
    unsafe { engine::<unsafe extern "C" fn(*mut *mut c_void)>(0x2744CC)(group) };
}
unsafe extern "C" fn job_complete(group: *mut c_void) -> bool {
    // SAFETY: a group this probe holds.
    unsafe { AtomicI32::from_ptr(group.cast::<u8>().add(0x18).cast()) }.compare_exchange(0, 0, SeqCst, SeqCst).is_ok()
}
const JOBS: JobLeaseApi = JobLeaseApi { retain: Some(retain_job), release: Some(release_job), complete: Some(job_complete) };

#[repr(C, align(16))]
struct Aligned<const N: usize>([u8; N]);
struct FineRange {
    candidates: Vec<NativeCullCandidate>,
    reference: Vec<u64>,
    partial: i8,
    source_begin: u64,
}
/// QueryWork. Fields in the reverse of the .cpp's members: Rust drops them in this order, C++ destroyed them in reverse.
struct QueryWork {
    early_ids: [Vec<u64>; 2],
    bounds_trace: bool,
    main_view: u64,
    epoch: u64,
    /// The peer's stand-in view. 2.5's query initializer also tests feature bit 98 (stereo) through context+0x18, i.e.
    /// view+0x25B0+8 (sub_14033E358): the zeroed block reaches past it, so that test reads 0.
    query_view: Aligned<0x25C0>,
    peer: Option<Box<QueryWork>>,
    scene: SceneVisibility,
    cull_jobs_retained: bool,
    scene_valid: bool,
    scene_processing: bool,
    scene_started: bool,
    capture_scene: bool,
    fine_ranges: Vec<FineRange>,
    fine_bytes: u32,
    fine_failures: u32,
    capture_fine: bool,
    source_query: [u64; 2],
    context: Aligned<0x50>,
    cull_jobs: NativeJobLease,
    source_jobs: NativeJobLease,
    jobs: NativeJobLease,
    collector: Collector,
}
// SAFETY: the probe's owner moves between the engine's threads only under its mutex.
unsafe impl Send for QueryWork {}
impl QueryWork {
    fn new(api: Api) -> Box<Self> {
        Box::new(QueryWork {
            early_ids: [Vec::new(), Vec::new()],
            bounds_trace: false,
            main_view: 0,
            epoch: 0,
            query_view: Aligned([0; 0x25C0]),
            peer: None,
            scene: SceneVisibility::default(),
            cull_jobs_retained: false,
            scene_valid: false,
            scene_processing: false,
            scene_started: false,
            capture_scene: false,
            fine_ranges: Vec::new(),
            fine_bytes: 0,
            fine_failures: 0,
            capture_fine: false,
            source_query: [0; 2],
            context: Aligned([0; 0x50]),
            cull_jobs: NativeJobLease::new(JOBS),
            source_jobs: NativeJobLease::new(JOBS),
            jobs: NativeJobLease::new(JOBS),
            collector: Collector::new(api),
        })
    }
    fn context_ptr(&mut self) -> *mut c_void {
        self.context.0.as_mut_ptr().cast()
    }
}
impl Drop for QueryWork {
    fn drop(&mut self) {
        self.collector.reset();
        if self.source_query[0] != 0 {
            // SAFETY: the engine's release of the source query this work referenced.
            unsafe { engine::<unsafe extern "C" fn(*mut u64)>(0x4ED568)(self.source_query.as_mut_ptr()) };
        }
        let owner = u64::from_le_bytes(self.context.0[..8].try_into().unwrap());
        if owner != 0 {
            // SAFETY: the engine's release of the copied context's owner.
            unsafe { engine::<unsafe extern "C" fn(*mut c_void)>(0x32DE14)(owner as *mut c_void) };
        }
    }
}
fn finished(work: &QueryWork) -> bool {
    work.jobs.complete() && (work.source_query[0] == 0 || work.source_jobs.complete()) && !work.scene_processing && (!work.cull_jobs_retained || work.cull_jobs.complete())
}

/// If a native completion cannot be proven, the bounded diagnostic owner stays alive through process exit rather than
/// being destroyed under pending tasks.
struct Pending(Option<Box<QueryWork>>);
static PENDING: Mutex<Pending> = Mutex::new(Pending(None));
fn lock() -> MutexGuard<'static, Pending> {
    PENDING.lock().unwrap_or_else(|p| p.into_inner())
}
static QUERY_PENDING: AtomicBool = AtomicBool::new(false);
static FINE_PENDING: AtomicBool = AtomicBool::new(false);
static SCENE_PENDING: AtomicBool = AtomicBool::new(false);
static PEER_PENDING: AtomicBool = AtomicBool::new(false);
static PRESENT_EPOCH: AtomicU64 = AtomicU64::new(0);

fn api() -> Api {
    Api {
        create: Some(engine(0x928D28)),
        destroy: Some(engine(0x11F3160)),
        initialize: collector_original(),
    }
}
fn set_u64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn qword(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
fn dword(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
/// CheckClone: the clone's storage names the owned inputs and its own metadata; counts and bits as the source's.
fn check_clone(source: u64, clone: &Collector) -> bool {
    let Some(input) = clone.inputs() else { return false };
    let (mut a, mut b) = ([0u8; 0x30], [0u8; 0x30]);
    if !read(source, &mut a) || !read(clone.storage() as u64, &mut b) {
        return false;
    }
    let (source_count, clone_count) = (dword(&a, 0x20), dword(&b, 0x20));
    let (mut source_bits, mut clone_bits) = ([0u8; 4], [0u8; 4]);
    if !read(qword(&a, 0x28).wrapping_add(0x10), &mut source_bits) || !read((clone.metadata() as u64).wrapping_add(0x10), &mut clone_bits) {
        return false;
    }
    let (source_bits, clone_bits) = (u32::from_le_bytes(source_bits), u32::from_le_bytes(clone_bits));
    let result = export!(CyberpunkVR_CpuCollectorResult);
    result[..4].copy_from_slice(&[source_count, clone_count, source_bits, clone_bits]);
    let inputs = input as *const Input as u64;
    qword(&b, 0) == qword(&input.0, 8)
        && qword(&b, 8) == qword(&input.0, 0)
        && qword(&b, 0x10) == inputs + 0x400
        && qword(&b, 0x18) == inputs + 0x20
        && qword(&b, 0x28) == clone.metadata() as u64
        && clone.storage() as u64 != source
        && qword(&a, 0x28) != qword(&b, 0x28)
        && source_count == clone_count
        && source_bits == clone_bits
        && source_bits > 0
}
/// The collector lifecycle detour's work after the original (request 1, the second view, from its one caller).
pub fn collector_returned(storage: u64, data: u64, mode: u8, camera: u64, scene: u64, flags: u64, caller: u64) {
    let Some(r) = rt() else { return };
    if x::CyberpunkVR_CpuCollectorRequest.load(Relaxed) != 1 || (r.menu_mode)() || crate::render_tls::view_side() != 1 || caller != base() + 0xC0F291 {
        return;
    }
    if x::CyberpunkVR_CpuCollectorRequest.swap(0, SeqCst) == 0 {
        return;
    }
    x::CyberpunkVR_CpuCollectorState.store(1, Release);
    *export!(CyberpunkVR_CpuCollectorResult) = [0; 8];
    let mut input = Box::new(Input([0; crate::cpu_collector::INPUT_BYTES]));
    set_u64(&mut input.0, 0, data);
    set_u64(&mut input.0, 8, scene);
    input.0[16] = mode;
    if !read(camera, &mut input.0[0x20..0x20 + CAMERA_BYTES]) || !read(flags, &mut input.0[0x400..0x400 + VIEW_FLAGS_BYTES]) {
        x::CyberpunkVR_CpuCollectorState.store(3, SeqCst);
        return;
    }
    let valid = {
        let mut owned = Collector::new(api());
        owned.create(&input) && check_clone(storage, &owned)
    };
    let result = export!(CyberpunkVR_CpuCollectorResult);
    result[4] = valid.into();
    result[5] = 1;
    let (mut live_camera, mut live_flags) = ([0u8; CAMERA_BYTES], [0u8; VIEW_FLAGS_BYTES]);
    let unchanged = read(camera, &mut live_camera) && read(flags, &mut live_flags) && live_camera[..] == input.0[0x20..0x20 + CAMERA_BYTES] && live_flags[..] == input.0[0x400..0x400 + VIEW_FLAGS_BYTES];
    result[6] = unchanged.into();
    x::CyberpunkVR_CpuCollectorState.store(if valid && unchanged { 2 } else { 3 }, Release);
}
/// Compatible (the lifecycle detour's admission): the second view wanted and the collector's code as built against.
pub fn wanted(which: u32) -> bool {
    let Some(r) = rt() else { return false };
    if !(r.second_view_wanted)() {
        return false;
    }
    // SAFETY: the executable's own module.
    let module = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) } as u64;
    let code = crate::collector_code::cvr_ccp_code;
    if which == 0 {
        let compatible = code(0, module);
        x::CyberpunkVR_CpuCollectorCompatible.store(if compatible { 1 } else { 2 }, SeqCst);
        return compatible;
    }
    if !code(1, module) {
        x::CyberpunkVR_CpuCollectorQueryCompatible.store(2, SeqCst);
        return false;
    }
    // The fine culling bodies beyond their detours' prologues, then the scene's, then the peer camera rebuild.
    let fine = code(2, module);
    x::CyberpunkVR_CpuCollectorFineCompatible.store(if fine { 1 } else { 2 }, SeqCst);
    let scene = code(3, module) && fine;
    x::CyberpunkVR_CpuCollectorSceneCompatible.store(if scene { 1 } else { 2 }, SeqCst);
    x::CyberpunkVR_CpuCollectorPeerCompatible.store(if scene && code(4, module) { 1 } else { 2 }, SeqCst);
    x::CyberpunkVR_CpuCollectorQueryCompatible.store(1, SeqCst);
    true
}
fn retain_source_query(work: &mut QueryWork, storage: u64, source_job: u64) -> bool {
    let Some(groups) = read_pair(source_job.wrapping_add(0x10)) else { return false };
    // SAFETY: the source job's live groups.
    if !unsafe { work.source_jobs.capture(groups[0] as *mut c_void, groups[1] as *mut c_void) } {
        return false;
    }
    match read_pair(storage.wrapping_add(0xDF2F0)).filter(|q| q[0] != 0 && q[1] != 0) {
        Some(query) => {
            work.source_query = query;
            // SAFETY: the source query's reference count.
            unsafe { AtomicI32::from_ptr(query[1] as *mut i32) }.fetch_add(1, SeqCst);
            true
        }
        None => {
            work.source_query = [0; 2];
            false
        }
    }
}
fn query_original(storage: *mut c_void, context: *mut c_void, job: *mut c_void) -> usize {
    match cvr_ccp_originals[1].load(Acquire) {
        0 => 0,
        // SAFETY: the query detour's trampoline with the engine's own arguments.
        raw => unsafe { core::mem::transmute::<usize, QueryInitialize>(raw)(storage, context, job) },
    }
}
fn start_query(work: &mut QueryWork, source_job: u64) -> bool {
    let mut job = Aligned([0u8; 0x40]);
    let job_ptr: *mut c_void = job.0.as_mut_ptr().cast();
    // SAFETY: the engine's job handle copy, the initializer and the handle's release over our 0x40 bytes.
    unsafe { engine::<unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void>(0x2746D8)(job_ptr, source_job as *mut c_void) };
    let storage = work.collector.storage();
    query_original(storage, work.context_ptr(), job_ptr);
    let (begin, end) = (qword(&job.0, 0x10), qword(&job.0, 0x18));
    // SAFETY: the job's groups.
    let retained = unsafe { work.jobs.capture(begin as *mut c_void, end as *mut c_void) };
    unsafe { engine::<unsafe extern "C" fn(*mut c_void)>(0x272DB8)(job_ptr) };
    retained
}
fn create_peer(work: &mut QueryWork, api: Api, input: &Input, context: u64) -> bool {
    let Some(r) = rt() else { return false };
    let main = (r.main_view)();
    let (mut size, mut pose) = ([0u32; 2], [0u8; 32]);
    if !peer_view(&read, main, context, &mut size, &mut pose) {
        return false;
    }
    let mut peer = QueryWork::new(api);
    let mut predicted = Box::new(Input(input.0));
    // MAIN's own window under the asymmetric projection (its input block, as the view producer left it).
    let own = if (r.projection_active)() { main_projection(&read, main) } else { None };
    let projection = own.map(|p| crate::camera_input::PeerProjection { aspect: p[0], shift_x: p[1], shift_y: p[2] });
    let rebuild: crate::camera_input::RebuildCameraInput = engine(0x296EA0);
    let camera: &mut [u8; CAMERA_BYTES] = (&mut predicted.0[0x20..0x20 + CAMERA_BYTES]).try_into().unwrap();
    // SAFETY: the source camera's 904 bytes, the pose's 32 and the engine's rebuild.
    let prepared = unsafe { crate::camera_input::prepare_camera_at_pose(input.0[0x20..].as_ptr(), pose.as_ptr(), &Default::default(), size[0], size[1], Some(rebuild), camera, projection.as_ref()) };
    if !prepared || !peer.collector.create(&predicted) {
        return false;
    }
    // SAFETY: the engine's context copy into the peer's 0x50 bytes.
    unsafe { engine::<unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void>(0x333374)(peer.context_ptr(), context as *const c_void) };
    peer.query_view.0[0x34..0x38].copy_from_slice(&size[0].to_le_bytes());
    peer.query_view.0[0x38..0x3C].copy_from_slice(&size[1].to_le_bytes());
    // The verified query initializer reads only width/height through context+18; it independently removes jitter
    // before constructing its culling frustum.
    let (view, storage) = (peer.query_view.0.as_ptr() as u64, peer.collector.storage() as u64);
    set_u64(&mut peer.context.0, 0x18, view);
    set_u64(&mut peer.context.0, 0x48, storage);
    peer.capture_fine = true;
    peer.capture_scene = true;
    work.main_view = main;
    work.peer = Some(peer);
    true
}
fn clear_results() {
    x::CyberpunkVR_CpuCollectorState.store(1, SeqCst);
    *export!(CyberpunkVR_CpuCollectorResult) = [0; 8];
    *export!(CyberpunkVR_CpuCollectorQueryResult) = [0; 8];
    *export!(CyberpunkVR_CpuCollectorFineResult) = [0; 12];
    *export!(CyberpunkVR_CpuCollectorSceneResult) = [0; 12];
    *export!(CyberpunkVR_CpuCollectorSceneControl) = [0; 8];
    *export!(CyberpunkVR_CpuCollectorPeerResult) = [0; 16];
    *export!(CyberpunkVR_CpuCollectorPeerQuery) = [0; 8];
    *export!(CyberpunkVR_CpuCollectorPeerFine) = [0; 12];
    *export!(CyberpunkVR_CpuCollectorPeerScene) = [0; 12];
    *export!(CyberpunkVR_CpuCollectorPeerControl) = [0; 8];
    *export!(CyberpunkVR_CpuCollectorPeerPlanes) = [0.0; 64];
    *export!(CyberpunkVR_CpuCollectorPeerInputDiff) = [0; 10];
    *export!(CyberpunkVR_CpuCollectorPeerChangesCount) = 0;
    if let Some(changes) = rt().map(|r| r.peer_changes).filter(|p| !p.is_null()) {
        // SAFETY: the .cpp's 64 records.
        unsafe { core::slice::from_raw_parts_mut(changes, 64) }.fill(ChangedBounds::default());
    }
}
/// The query initializer detour (from its one caller): the peer's MAIN source query retained once its own query ran;
/// a request 2..5 for the VRCAM view starts the owned query (and the peer's).
pub fn query(storage: u64, context: u64, source_job: u64, caller: u64) -> usize {
    let Some(r) = rt() else { return 0 };
    let expected = caller == base() + 0x5ABB85;
    let mut capture_main = false;
    if expected && PEER_PENDING.load(Acquire) {
        let s = lock();
        if let Some(pending) = s.0.as_ref() {
            if let Some(peer) = pending.peer.as_ref().filter(|p| p.source_query[0] == 0) {
                if read_u64(context.wrapping_add(0x18)).is_some_and(|view| view == pending.main_view) {
                    capture_main = true;
                    export!(CyberpunkVR_CpuCollectorPeerResult)[5] = peer.jobs.complete().into();
                }
            }
        }
    }
    let result = query_original(storage as *mut c_void, context as *mut c_void, source_job as *mut c_void);
    if capture_main {
        let mut s = lock();
        if let Some(pending) = s.0.as_mut().filter(|p| p.epoch == PRESENT_EPOCH.load(SeqCst)) {
            if let Some(peer) = pending.peer.as_mut().filter(|p| p.source_query[0] == 0) {
                let retained = retain_source_query(peer, storage, source_job);
                let stats = export!(CyberpunkVR_CpuCollectorPeerResult);
                stats[4] = retained.into();
                if !retained {
                    stats[13] = 2;
                }
            }
        }
        return result;
    }
    let request = x::CyberpunkVR_CpuCollectorRequest.load(Relaxed);
    if !(2..=5).contains(&request) || (r.menu_mode)() || !expected {
        return result;
    }
    if !vrcam_view(&read, context, (r.vrcam_key)()) {
        return result;
    }
    let mut s = lock();
    if s.0.is_some() || x::CyberpunkVR_CpuCollectorRequest.swap(0, SeqCst) == 0 {
        return result;
    }
    clear_results();
    let fail = || {
        x::CyberpunkVR_CpuCollectorState.store(3, SeqCst);
        result
    };
    if request >= 3 && (x::CyberpunkVR_CpuCollectorFineCompatible.load(SeqCst) != 1 || (r.cull_reuse)() || fine_materialize().is_none()) {
        return fail();
    }
    if request >= 4 && (x::CyberpunkVR_CpuCollectorSceneCompatible.load(SeqCst) != 1 || main_cull_test().is_none()) {
        return fail();
    }
    if request == 5 && x::CyberpunkVR_CpuCollectorPeerCompatible.load(SeqCst) != 1 {
        return fail();
    }
    let api = api();
    let mut work = QueryWork::new(api);
    work.capture_fine = request >= 3;
    work.capture_scene = request >= 4;
    work.epoch = PRESENT_EPOCH.load(SeqCst);
    let mut input = Box::new(Input([0; crate::cpu_collector::INPUT_BYTES]));
    let mut source = [0u64; 4];
    if !retain_source_query(&mut work, storage, source_job) {
        return fail();
    }
    {
        let (head, flags) = input.0.split_at_mut(0x400);
        let mut mode = 0u8;
        let camera: &mut [u8; CAMERA_BYTES] = (&mut head[0x20..0x20 + CAMERA_BYTES]).try_into().unwrap();
        let flags: &mut [u8; VIEW_FLAGS_BYTES] = (&mut flags[..VIEW_FLAGS_BYTES]).try_into().unwrap();
        if !collector_input(&read, storage, &mut source, &mut mode, camera, flags) {
            return fail();
        }
        head[16] = mode;
    }
    set_u64(&mut input.0, 8, source[0]);
    set_u64(&mut input.0, 0, source[1]);
    if !work.collector.create(&input) || !check_clone(storage, &work.collector) {
        return fail();
    }
    export!(CyberpunkVR_CpuCollectorResult)[4] = 1;
    // SAFETY: the engine's context copy into the work's 0x50 bytes.
    unsafe { engine::<unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void>(0x333374)(work.context_ptr(), context as *const c_void) };
    let target = work.collector.storage() as u64;
    set_u64(&mut work.context.0, 0x48, target);
    if request == 5 {
        if !create_peer(&mut work, api, &input, context) {
            export!(CyberpunkVR_CpuCollectorPeerResult)[13] = 1;
            return fail();
        }
        export!(CyberpunkVR_CpuCollectorPeerResult)[0] = 1;
    }
    // Once jobs can refer to the collector, even an allocation/validation failure must not destroy it until both
    // native completion groups finish.
    let pending = s.0.insert(work);
    QUERY_PENDING.store(true, SeqCst);
    if pending.peer.is_some() {
        pending.bounds_trace = crate::bounds_trace::cvr_bt_begin(source[0].wrapping_add(0x7E0F0));
    }
    let retained = start_query(pending, source_job);
    if let Some(peer) = pending.peer.as_mut() {
        export!(CyberpunkVR_CpuCollectorPeerResult)[1] = start_query(peer, source_job).into();
        PEER_PENDING.store(true, SeqCst);
    }
    let camera: &[u8; CAMERA_BYTES] = (&input.0[0x20..0x20 + CAMERA_BYTES]).try_into().unwrap();
    let flags: &[u8; VIEW_FLAGS_BYTES] = (&input.0[0x400..0x400 + VIEW_FLAGS_BYTES]).try_into().unwrap();
    export!(CyberpunkVR_CpuCollectorResult)[6] = inputs_unchanged(&read, source[3], source[2], camera, flags).into();
    export!(CyberpunkVR_CpuCollectorQueryResult)[0] = retained.into();
    FINE_PENDING.store(pending.capture_fine && retained, SeqCst);
    SCENE_PENDING.store(pending.capture_scene && retained, SeqCst);
    x::CyberpunkVR_CpuCollectorState.store(if retained { 4 } else { 3 }, Release);
    result
}
/// FindQuery: the owned work (or its peer) whose source query is `query`.
fn find_query(pending: &mut Option<Box<QueryWork>>, query: u64) -> Option<*mut QueryWork> {
    let work = pending.as_mut()?;
    if query != 0 && work.source_query[0] == query {
        return Some(&mut **work);
    }
    let peer = work.peer.as_mut().filter(|p| query != 0 && p.source_query[0] == query)?;
    Some(&mut **peer)
}
/// CaptureOwnedCpuFine: a fine culling call on a captured query -- its candidates copied, the native call made exactly
/// once, only its appended id suffix copied.
///
/// # Safety
/// The engine's fine-materialize arguments.
pub unsafe fn capture_fine(tester: *mut c_void, range: *mut i64, partial: i8, output: *mut c_void, result: &mut i8) -> bool {
    if !FINE_PENDING.load(Relaxed) {
        return false;
    }
    let mut s = lock();
    let Some(work) = find_query(&mut s.0, tester as u64) else { return false };
    // SAFETY: the owner, kept alive under the mutex.
    let work = unsafe { &mut *work };
    if !work.capture_fine {
        return false;
    }
    let bounds = read_pair(range as u64);
    let (counted, count) = bounds.map_or((false, 0), |b| fine_candidate_count(b[0] as usize, b[1] as usize));
    let ids = |address: u64| {
        let mut b = [0u8; 16];
        read(address, &mut b).then(|| (u64::from_le_bytes(b[..8].try_into().unwrap()), dword(&b, 8), dword(&b, 12)))
    };
    let before = ids(output as u64);
    let mut valid = counted
        && before.is_some_and(|(_, capacity, used)| used <= capacity)
        && work.fine_ranges.len() < 4096
        && (count as u64) * 48 <= (4u64 << 20).wrapping_sub(u64::from(work.fine_bytes));
    let mut captured = FineRange { candidates: Vec::new(), reference: Vec::new(), partial: 0, source_begin: 0 };
    if valid {
        let begin = bounds.unwrap_or_default()[0];
        captured.candidates = vec![NativeCullCandidate::default(); count];
        captured.partial = partial;
        captured.source_begin = begin;
        // SAFETY: the candidates' bytes.
        let bytes = unsafe { core::slice::from_raw_parts_mut(captured.candidates.as_mut_ptr().cast::<u8>(), count * 40) };
        valid = count == 0 || read(begin, bytes);
    }
    // The actual native call and its output, exactly once, even when the diagnostic capture fails.
    if let Some(fine) = fine_materialize() {
        *result = unsafe { fine(tester, range, partial, output) };
    }
    if valid {
        let after = ids(output as u64);
        let before_count = before.map_or(0, |b| b.2);
        valid = after.is_some_and(|(_, capacity, used)| used >= before_count && used <= capacity && (used - before_count) as usize <= count);
        if valid {
            let (data, _, used) = after.unwrap_or_default();
            captured.reference = vec![0u64; (used - before_count) as usize];
            // SAFETY: the reference's bytes.
            let bytes = unsafe { core::slice::from_raw_parts_mut(captured.reference.as_mut_ptr().cast::<u8>(), captured.reference.len() * 8) };
            valid = bytes.is_empty() || read(data.wrapping_add(u64::from(before_count) * 8), bytes);
            if valid {
                work.fine_bytes += (count * 40 + captured.reference.len() * 8) as u32;
                work.fine_ranges.push(captured);
            }
        }
    }
    if !valid {
        work.fine_failures += 1;
    }
    true
}
/// CaptureOwnedCpuScene: the scene pool captured under the main cull test of a captured query (the bounds trace
/// marking the capture); with the peer, both eyes' early ids evaluated; the native call made without the probe's mutex
/// (submission may run a fine-cull worker inline) -- `scene_processing` pins the owner meanwhile.
///
/// # Safety
/// The engine's main cull test arguments.
pub unsafe fn capture_scene(manager: *mut c_void, context: *mut c_void, output: *mut c_void, query: *mut c_void, job: *mut c_void, result: &mut i64) -> bool {
    if !SCENE_PENDING.load(Relaxed) {
        return false;
    }
    let work: *mut QueryWork;
    {
        let mut s = lock();
        let top: *const QueryWork = s.0.as_deref().map_or(core::ptr::null(), |p| p as *const QueryWork);
        let trace = s.0.as_ref().is_some_and(|p| p.bounds_trace);
        let Some(found) = find_query(&mut s.0, query as u64) else { return false };
        // SAFETY: the owner, kept alive under the mutex.
        let w = unsafe { &mut *found };
        if !w.capture_scene || w.scene_started {
            return false;
        }
        w.scene_started = true;
        w.scene_processing = true;
        let mut counter = [0u8; 4];
        read((manager as u64).wrapping_add(0x28), &mut counter);
        let counter = u32::from_le_bytes(counter);
        let is_top = core::ptr::eq(found, top);
        if trace {
            crate::bounds_trace::cvr_bt_mark(if is_top { trace_kind::VRCAM_BEGIN } else { trace_kind::MAIN_BEGIN }, is_top.into(), counter);
        }
        w.scene_valid = w.scene.capture((manager as u64).wrapping_add(0x7E0F0), &read);
        if trace {
            crate::bounds_trace::cvr_bt_mark(if is_top { trace_kind::VRCAM_END } else { trace_kind::MAIN_END }, is_top.into(), counter);
        }
        if is_top {
            if let Some(peer) = w.peer.as_deref() {
                let stats = export!(CyberpunkVR_CpuCollectorPeerResult);
                stats[2] = (w.jobs.complete() && peer.jobs.complete()).into();
                let mut valid = w.scene_valid && stats[2] != 0 && peer.source_query[0] == 0;
                let storages = [w.collector.storage() as u64, peer.collector.storage() as u64];
                let mut visible = 0u32;
                let mut eye = 0;
                while valid && eye < 2 {
                    let target = read_u64(storages[eye].wrapping_add(0xDF2F0)).filter(|&t| t != 0);
                    let coarse: NativeCoarseCull = engine(0x29CEE4);
                    // SAFETY: the engine's tests over our owned buckets.
                    valid = target.is_some_and(|t| unsafe { w.scene.evaluate(Some(coarse), fine_materialize(), t as *mut c_void, &mut w.early_ids[eye], &mut visible) });
                    stats[6 + eye] = w.early_ids[eye].len() as u32;
                    eye += 1;
                }
                stats[3] = valid.into();
                if !valid {
                    stats[13] = 3;
                }
            }
        }
        work = found;
    }
    if let Some(test) = main_cull_test() {
        *result = unsafe { test(manager, context, output, query, job) };
    }
    let _s = lock();
    // SAFETY: pinned by scene_processing.
    let w = unsafe { &mut *work };
    w.cull_jobs_retained = read_pair((job as u64).wrapping_add(0x10)).is_some_and(|g| unsafe { w.cull_jobs.capture(g[0] as *mut c_void, g[1] as *mut c_void) });
    w.scene_processing = false;
    true
}
/// CompareEye: the clone's query against the source's, the captured fine ranges re-run on the clone, the scene
/// evaluated on both queries against the ranges inside the pool.
fn compare_eye(work: &QueryWork, query_stats: &mut [u32; 8], fine_stats: &mut [u32; 12], scene_stats: &mut [u32; 12], control: &mut [u32; 8], planes: Option<&mut [f32; 64]>) -> bool {
    let base = base();
    let mut query = QueryComparison::default();
    compare_query(&read, work.collector.storage() as u64, work.source_query[0], base, &mut query);
    let (clone_query, source_type, clone_type, valid, differences) = (query.clone_query, query.source_type, query.clone_type, query.valid, query.differences);
    if let Some(planes) = planes {
        for (i, chunk) in query.planes.iter().flat_map(|p| p.chunks_exact(4)).enumerate() {
            planes[i] = f32::from_le_bytes(chunk.try_into().unwrap());
        }
    }
    let rva = |value: u64| if value >= base && value - base <= u64::from(u32::MAX) { (value - base) as u32 } else { 0 };
    query_stats[0] = work.jobs.complete().into();
    query_stats[1] = valid.into();
    query_stats[2] = differences;
    query_stats[3] = (clone_query != work.source_query[0]).into();
    query_stats[4] = rva(source_type);
    query_stats[5] = rva(clone_type);
    query_stats[6] = (work.jobs.complete() && work.source_jobs.complete()).into();
    let fine = fine_materialize();
    let mut fine_valid = true;
    if work.capture_fine {
        let stats = fine_stats;
        stats[0] = work.fine_ranges.len() as u32;
        stats[5] = work.fine_failures;
        stats[7] = work.fine_bytes;
        let (mut source_occlusion, mut clone_occlusion, mut source_active, mut clone_active) = ([0u8; 8], [0u8; 8], [0u8; 1], [0u8; 1]);
        fine_valid = valid
            && read(work.source_query[0].wrapping_add(0x240), &mut source_occlusion)
            && read(clone_query.wrapping_add(0x240), &mut clone_occlusion)
            && read(work.source_query[0].wrapping_add(0x462), &mut source_active)
            && read(clone_query.wrapping_add(0x462), &mut clone_active);
        let (source_occlusion, clone_occlusion) = (u64::from_le_bytes(source_occlusion), u64::from_le_bytes(clone_occlusion));
        stats[8] = source_active[0].into();
        stats[9] = clone_active[0].into();
        stats[10] = (source_occlusion != 0).into();
        stats[11] = (clone_occlusion != 0).into();
        if fine_valid {
            for row in &work.fine_ranges {
                stats[1] += row.candidates.len() as u32;
                stats[2] += row.reference.len() as u32;
                // SAFETY: the engine's fine test on the clone's query.
                let Some(ids) = (unsafe { run_fine(fine, clone_query as *mut c_void, &row.candidates, row.partial) }) else {
                    stats[6] += 1;
                    continue;
                };
                stats[3] += ids.len() as u32;
                stats[4] += u32::from(ids != row.reference);
            }
        }
        fine_valid = fine_valid && stats[0] != 0 && stats[1] != 0 && stats[4] == 0 && stats[5] == 0 && stats[6] == 0 && source_active == clone_active && (source_occlusion != 0) == (clone_occlusion != 0);
    }
    let mut scene_valid = true;
    if work.capture_scene {
        let stats = scene_stats;
        stats[0] = work.scene_started.into();
        stats[1] = work.scene.failure;
        stats[2] = work.scene.declared;
        stats[3] = work.scene.buckets.len() as u32;
        stats[4] = work.scene.candidate_count;
        stats[9] = (work.cull_jobs_retained && work.cull_jobs.complete()).into();
        stats[10] = work.scene.bytes_owned;
        scene_valid = valid && work.scene_valid && stats[0] != 0 && stats[9] != 0;
        if scene_valid {
            let coarse: NativeCoarseCull = engine(0x29CEE4);
            let (mut ids, mut reference, mut source_ids) = (Vec::new(), Vec::new(), Vec::new());
            // SAFETY: the engine's tests over our owned buckets, on the clone's and the source's queries.
            scene_valid = unsafe { work.scene.evaluate(Some(coarse), fine, clone_query as *mut c_void, &mut ids, &mut stats[5]) };
            scene_valid = scene_valid && unsafe { work.scene.evaluate(Some(coarse), fine, work.source_query[0] as *mut c_void, &mut source_ids, &mut control[3]) };
            control[4] = source_ids.len() as u32;
            for row in &work.fine_ranges {
                let in_pool = work.scene.buckets.iter().any(|b| b.source_begin == row.source_begin && b.candidates.len() == row.candidates.len());
                if in_pool {
                    control[0] += 1;
                    reference.extend_from_slice(&row.reference);
                } else {
                    control[1] += 1;
                    control[2] += row.reference.len() as u32;
                }
            }
            stats[6] = ids.len() as u32;
            stats[7] = reference.len() as u32;
            ids.sort_unstable();
            reference.sort_unstable();
            source_ids.sort_unstable();
            stats[8] = different_ids(&ids, &reference);
            stats[11] = (stats[5] == control[0]).into();
            control[5] = (source_ids != reference).into();
            control[6] = (source_ids != ids).into();
            control[7] = (control[0] + control[1] == work.fine_ranges.len() as u32).into();
            scene_valid = scene_valid && control[5] == 0 && control[6] == 0 && control[7] != 0 && control[3] == control[0];
        }
        scene_valid = scene_valid && stats[3] != 0 && stats[8] == 0 && stats[11] != 0;
    }
    valid && differences == 0 && fine_valid && scene_valid && query_stats[0] != 0 && query_stats[3] != 0 && query_stats[6] != 0
}
/// The ids of the captured fine ranges inside the eye's pool, concatenated.
fn pool_reference(work: &QueryWork) -> Vec<u64> {
    let mut reference = Vec::new();
    for row in &work.fine_ranges {
        if work.scene.buckets.iter().any(|b| b.source_begin == row.source_begin && b.candidates.len() == row.candidates.len()) {
            reference.extend_from_slice(&row.reference);
        }
    }
    reference
}
/// PollOwnedCpuQuery (the present boundary): once both eyes finished, the comparisons published and the owner freed.
pub fn poll() {
    PRESENT_EPOCH.fetch_add(1, Release);
    if !QUERY_PENDING.load(Acquire) {
        return;
    }
    let mut s = lock();
    let Some(pending) = s.0.as_deref() else { return };
    if !finished(pending) || pending.peer.as_deref().is_some_and(|p| !finished(p)) {
        return;
    }
    if pending.bounds_trace && !crate::bounds_trace::cvr_bt_end() {
        return;
    }
    FINE_PENDING.store(false, SeqCst);
    SCENE_PENDING.store(false, SeqCst);
    PEER_PENDING.store(false, SeqCst);
    let mut valid = compare_eye(pending, export!(CyberpunkVR_CpuCollectorQueryResult), export!(CyberpunkVR_CpuCollectorFineResult), export!(CyberpunkVR_CpuCollectorSceneResult), export!(CyberpunkVR_CpuCollectorSceneControl), None);
    if let Some(peer) = pending.peer.as_deref() {
        let stats = export!(CyberpunkVR_CpuCollectorPeerResult);
        *export!(CyberpunkVR_CpuCollectorPeerInputDiff) = pending.scene.candidate_differences(&peer.scene);
        let changes = rt().map_or(core::ptr::null_mut(), |r| r.peer_changes);
        let count = export!(CyberpunkVR_CpuCollectorPeerChangesCount);
        let base = base();
        for (first, second) in pending.scene.buckets.iter().zip(&peer.scene.buckets) {
            for (a, b) in first.candidates.iter().zip(&second.candidates) {
                if a.id != b.id || a.bounds.map(f32::to_bits) == b.bounds.map(f32::to_bits) || *count == 64 || changes.is_null() {
                    continue;
                }
                // SAFETY: the .cpp's 64 records.
                let row = unsafe { &mut *changes.add(*count as usize) };
                *count += 1;
                (row.id, row.before, row.after) = (a.id, a.bounds, b.bounds);
                if let Some([kind, prepare]) = candidate_type(&read, a.id, base) {
                    (row.kind, row.prepare) = (kind, prepare);
                }
            }
        }
        stats[10] = pending.scene.same_candidates(&peer.scene).into();
        stats[11] = compare_eye(peer, export!(CyberpunkVR_CpuCollectorPeerQuery), export!(CyberpunkVR_CpuCollectorPeerFine), export!(CyberpunkVR_CpuCollectorPeerScene), export!(CyberpunkVR_CpuCollectorPeerControl), Some(export!(CyberpunkVR_CpuCollectorPeerPlanes))).into();
        for (eye, work) in [pending, peer].into_iter().enumerate() {
            stats[8 + eye] = different_ids(&pending.early_ids[eye], &pool_reference(work));
        }
        let (first, second) = id_overlap(&pending.early_ids[0], &pending.early_ids[1]);
        (stats[14], stats[15]) = (first, second);
        valid = valid
            && stats[..8].iter().all(|&v| v != 0)
            && stats[8] == 0
            && stats[9] == 0
            && stats[10] != 0
            && stats[11] != 0
            && stats[13] == 0;
    }
    let peer = pending.peer.is_some();
    s.0 = None;
    QUERY_PENDING.store(false, SeqCst);
    export!(CyberpunkVR_CpuCollectorResult)[5] = 1;
    export!(CyberpunkVR_CpuCollectorQueryResult)[7] = 1;
    if peer {
        export!(CyberpunkVR_CpuCollectorPeerResult)[12] = 1;
        export!(CyberpunkVR_CpuCollectorPeerQuery)[7] = 1;
    }
    let unchanged = export!(CyberpunkVR_CpuCollectorResult)[6] != 0;
    x::CyberpunkVR_CpuCollectorState.store(if valid && unchanged { 2 } else { 3 }, Release);
}

// ---- the C ABI

/// # Safety
/// `runtime` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime.cast_mut(), Release);
}
/// The address the hook returns to (`_ReturnAddress()`): frame 0 is the hook (this inlined), frame 1 its caller.
#[inline(always)]
fn return_address() -> u64 {
    let mut frames = [core::ptr::null_mut::<c_void>(); 1];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None) };
    if n == 0 { 0 } else { frames[0] as u64 }
}
/// The collector lifecycle detour (formerly CpuCollectorProbe.cpp's Hook): the native initialize, then its return.
///
/// # Safety
/// The engine's call with its arguments.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_collector_hook(
    storage: *mut c_void,
    data: *mut c_void,
    mode: u8,
    camera: *const c_void,
    scene: *mut c_void,
    metadata: *mut c_void,
    flags: *const c_void,
) -> usize {
    let caller = return_address();
    let Some(original) = collector_original() else { return 0 };
    // SAFETY: the trampoline with the engine's own arguments.
    let result = unsafe { original(storage, data, mode, camera, scene, metadata, flags) };
    cvr_ccp_collector_returned(storage as u64, data as u64, mode, camera as u64, scene as u64, flags as u64, caller);
    result
}
/// The query task detour (formerly CpuCollectorProbe.cpp's QueryHook).
///
/// # Safety
/// The engine's call with its arguments.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_query_hook(storage: *mut c_void, context: *mut c_void, source_job: *mut c_void) -> usize {
    cvr_ccp_query(storage as u64, context as u64, source_job as u64, return_address())
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ccp_wanted(which: u32) -> bool {
    wanted(which)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ccp_collector_returned(storage: u64, data: u64, mode: u8, camera: u64, scene: u64, flags: u64, caller: u64) {
    collector_returned(storage, data, mode, camera, scene, flags, caller);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ccp_query(storage: u64, context: u64, source_job: u64, caller: u64) -> usize {
    query(storage, context, source_job, caller)
}
/// # Safety
/// The engine's fine-materialize arguments; `result` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_capture_fine(tester: *mut c_void, range: *mut i64, partial: i8, output: *mut c_void, result: *mut i8) -> bool {
    unsafe { capture_fine(tester, range, partial, output, &mut *result) }
}
/// # Safety
/// The engine's main cull test arguments; `result` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_capture_scene(manager: *mut c_void, context: *mut c_void, output: *mut c_void, query: *mut c_void, job: *mut c_void, result: *mut i64) -> bool {
    unsafe { capture_scene(manager, context, output, query, job, &mut *result) }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ccp_poll() {
    poll();
}

#[cfg(test)]
mod tests {
    //! The paths reachable without the engine: the gates of both detours, the fine and scene captures idle without a
    //! pending query, the poll's idle boundary, and a query refused before any work (its results cleared).
    use super::*;

    extern "C" fn base_of() -> u64 {
        0x1_4000_0000
    }
    extern "C" fn no() -> bool {
        false
    }
    extern "C" fn yes() -> bool {
        true
    }
    extern "C" fn zero() -> u64 {
        0
    }
    static NONE_FINE: Option<NativeFineCull> = None;
    static NONE_CULL: Option<MainCullTest> = None;
    static mut CHANGES: [ChangedBounds; 64] = [ChangedBounds { id: 9, kind: 0, prepare: 0, before: [0.0; 6], after: [0.0; 6] }; 64];
    static RT: Runtime = Runtime {
        exe_base: base_of,
        menu_mode: no,
        main_view: zero,
        vrcam_key: zero,
        cull_reuse: no,
        second_view_wanted: yes,
        projection_active: no,
        fine_materialize: &NONE_FINE,
        main_cull_test: &NONE_CULL,
        peer_changes: core::ptr::addr_of_mut!(CHANGES).cast(),
    };

    #[test]
    fn gates_idle_paths_and_refusals() {
        unsafe { cvr_ccp_attach(&RT) };
        // The render thread's state: the second view.
        // SAFETY: this thread's block.
        unsafe { (*crate::render_tls::cvr_render_tls()).view_side = 1 };
        // The lifecycle detour: only request 1, from its one caller.
        x::CyberpunkVR_CpuCollectorRequest.store(1, SeqCst);
        collector_returned(0, 0, 0, 0, 0, 0, 0x1234);
        assert_eq!(x::CyberpunkVR_CpuCollectorRequest.load(SeqCst), 1, "another caller leaves the request");
        collector_returned(0, 0, 0, 0, 0, 0, base_of() + 0xC0F291);
        assert_eq!((x::CyberpunkVR_CpuCollectorRequest.load(SeqCst), x::CyberpunkVR_CpuCollectorState.load(SeqCst)), (0, 3), "taken; no camera to read");
        // The query detour: another caller or request only calls the original (none here: 0).
        x::CyberpunkVR_CpuCollectorRequest.store(2, SeqCst);
        assert_eq!(query(0, 0, 0, 0x1234), 0);
        assert_eq!(x::CyberpunkVR_CpuCollectorRequest.load(SeqCst), 2, "not its caller");
        // From its caller but not the VRCAM view (no context to read): left alone.
        assert_eq!(query(0, 0, 0, base_of() + 0x5ABB85), 0);
        assert_eq!(x::CyberpunkVR_CpuCollectorRequest.load(SeqCst), 2);
        // Idle captures and poll.
        let mut r8 = 5i8;
        assert!(!unsafe { capture_fine(core::ptr::null_mut(), core::ptr::null_mut(), 0, core::ptr::null_mut(), &mut r8) } && r8 == 5);
        let mut r64 = 5i64;
        assert!(!unsafe { capture_scene(core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), &mut r64) } && r64 == 5);
        let epoch = PRESENT_EPOCH.load(SeqCst);
        poll();
        assert_eq!(PRESENT_EPOCH.load(SeqCst), epoch + 1, "every present counts");
        // The results' clearing: every table, the changed bounds included.
        export!(CyberpunkVR_CpuCollectorPeerResult)[3] = 7;
        clear_results();
        assert_eq!(export!(CyberpunkVR_CpuCollectorPeerResult)[3], 0);
        assert_eq!(unsafe { (*core::ptr::addr_of!(CHANGES))[63].id }, 0);
        // A pending fine capture finds no query: the native call is not this probe's.
        FINE_PENDING.store(true, SeqCst);
        assert!(!unsafe { capture_fine(0x40 as *mut c_void, core::ptr::null_mut(), 0, core::ptr::null_mut(), &mut r8) });
        FINE_PENDING.store(false, SeqCst);
    }
}
