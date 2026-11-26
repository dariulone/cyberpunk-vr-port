//! The native preparation trace and the pair preparation (formerly src/Render/NativePreparation.cpp;
//! include/Render/NativePreparation.hpp declares it): on request, for up to four frames (eight with the camera trace),
//! every preparation stage, output, consumed packet span, graph view list, cull and materialize task, proxy gather (two
//! packets per proxy: its visible id and gather method, its vtable and update method), draw scope, eye copy, submitted
//! and reset list as a row of the exports' -- the packets copied while their native owners hold them; and the bounded
//! pair diagnostics -- mode 1 refusing an unowned RTT pair, mode 2 selecting both views under MAIN's output-owning
//! root, mode 3 (and a run of 30..1200 frames) additionally suppressing the standalone VRCAM request once per frame
//! after a verified warmup, every eye copy recorded and submitted once.
//!
//! The graph parent (the pair frame's published graph, the original image's certificate of the parent functions, a
//! context's producer, its revalidation), the thread's graph context and the graph job and scene update polls are here
//! too. The .cpp keeps the DrawScope class (its members mirrored here) and InvokeGraphContext's scene-child scope; the
//! exports are referenced by their symbols (Rust's in the plugin, the reference definitions in the single-pass tests).

use crate::packet_inputs::Packet;
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// The exports (the .cpp lends them at its attach: the plugin's are Rust's, the single-pass tests' their reference
/// definitions; referencing them by symbol would leave every other Rust test variant unlinked). Unattached, they
/// read 0 and the trace and the pairs stay idle.
#[repr(C)]
#[allow(non_snake_case)]
pub struct Exports {
    pub CyberpunkVR_PreparationRequest: *const AtomicU32,
    pub CyberpunkVR_PreparationState: *const AtomicU32,
    pub CyberpunkVR_PreparationSeq: *const AtomicU32,
    pub CyberpunkVR_PreparationCount: *const AtomicU32,
    pub CyberpunkVR_PreparationPacketCount: *const AtomicU32,
    pub CyberpunkVR_PreparationFailure: *const AtomicU32,
    pub CyberpunkVR_PreparationRecords: *mut u64,
    pub CyberpunkVR_PreparationPackets: *mut u64,
    pub CyberpunkVR_PreparationPairRequest: *const AtomicU32,
    pub CyberpunkVR_PreparationPairState: *const AtomicU32,
    pub CyberpunkVR_PreparationPairCompatible: *const AtomicU32,
    pub CyberpunkVR_PreparationPairData: *mut [u64; 8],
    pub CyberpunkVR_PreparationPairEligibility: *mut [u64; 8],
    pub CyberpunkVR_PreparationPairRoot: *mut [u64; 8],
    pub CyberpunkVR_PreparationCameraTrace: *const AtomicU32,
    pub CyberpunkVR_PreparationAllStages: *const AtomicU32,
    pub CyberpunkVR_PreparationGraphParents: *mut u64,
    pub CyberpunkVR_PreparationGraphParentCount: *mut u32,
    pub CyberpunkVR_PreparationPairVerified: *const AtomicU32,
    pub CyberpunkVR_PreparationRootSkipped: *const AtomicU32,
    pub CyberpunkVR_PreparationRootCompatible: *const AtomicU32,
    pub CyberpunkVR_PreparationPairMergeLimit: *const AtomicU32,
    pub CyberpunkVR_PreparationPairMerged: *const AtomicU32,
    pub CyberpunkVR_PreparationPairCompleted: *const AtomicU32,
    pub CyberpunkVR_PreparationPairGeneration: *const AtomicU64,
    pub CyberpunkVR_PreparationRunRequest: *const AtomicU32,
    pub CyberpunkVR_PreparationRunState: *const AtomicU32,
    pub CyberpunkVR_PreparationRunStop: *const AtomicU32,
    pub CyberpunkVR_PreparationRunFailure: *const AtomicU32,
    pub CyberpunkVR_PreparationRunFrames: *const AtomicU32,
    pub CyberpunkVR_PreparationRunDepth: *const [AtomicU32; 2],
    pub CyberpunkVR_PreparationRunEyeCopies: *const AtomicU32,
    pub CyberpunkVR_PreparationRunEyeSubmissions: *const AtomicU32,
}
// SAFETY: immutable; its pointers name process-lifetime exports.
unsafe impl Sync for Exports {}
unsafe impl Send for Exports {}
const _: () = assert!(core::mem::size_of::<Exports>() == 264);
static ZERO32: AtomicU32 = AtomicU32::new(0);
static ZERO64: AtomicU64 = AtomicU64::new(0);
static ZERO_PAIR: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static mut DUMMY_U64: u64 = 0;
static mut DUMMY_U32: u32 = 0;
static mut DUMMY_ARRAY: [u64; 8] = [0; 8];
static FALLBACK: Exports = Exports {
    CyberpunkVR_PreparationRequest: &ZERO32,
    CyberpunkVR_PreparationState: &ZERO32,
    CyberpunkVR_PreparationSeq: &ZERO32,
    CyberpunkVR_PreparationCount: &ZERO32,
    CyberpunkVR_PreparationPacketCount: &ZERO32,
    CyberpunkVR_PreparationFailure: &ZERO32,
    CyberpunkVR_PreparationRecords: core::ptr::addr_of_mut!(DUMMY_U64),
    CyberpunkVR_PreparationPackets: core::ptr::addr_of_mut!(DUMMY_U64),
    CyberpunkVR_PreparationPairRequest: &ZERO32,
    CyberpunkVR_PreparationPairState: &ZERO32,
    CyberpunkVR_PreparationPairCompatible: &ZERO32,
    CyberpunkVR_PreparationPairData: core::ptr::addr_of_mut!(DUMMY_ARRAY),
    CyberpunkVR_PreparationPairEligibility: core::ptr::addr_of_mut!(DUMMY_ARRAY),
    CyberpunkVR_PreparationPairRoot: core::ptr::addr_of_mut!(DUMMY_ARRAY),
    CyberpunkVR_PreparationCameraTrace: &ZERO32,
    CyberpunkVR_PreparationAllStages: &ZERO32,
    CyberpunkVR_PreparationGraphParents: core::ptr::addr_of_mut!(DUMMY_U64),
    CyberpunkVR_PreparationGraphParentCount: core::ptr::addr_of_mut!(DUMMY_U32),
    CyberpunkVR_PreparationPairVerified: &ZERO32,
    CyberpunkVR_PreparationRootSkipped: &ZERO32,
    CyberpunkVR_PreparationRootCompatible: &ZERO32,
    CyberpunkVR_PreparationPairMergeLimit: &ZERO32,
    CyberpunkVR_PreparationPairMerged: &ZERO32,
    CyberpunkVR_PreparationPairCompleted: &ZERO32,
    CyberpunkVR_PreparationPairGeneration: &ZERO64,
    CyberpunkVR_PreparationRunRequest: &ZERO32,
    CyberpunkVR_PreparationRunState: &ZERO32,
    CyberpunkVR_PreparationRunStop: &ZERO32,
    CyberpunkVR_PreparationRunFailure: &ZERO32,
    CyberpunkVR_PreparationRunFrames: &ZERO32,
    CyberpunkVR_PreparationRunDepth: &ZERO_PAIR,
    CyberpunkVR_PreparationRunEyeCopies: &ZERO32,
    CyberpunkVR_PreparationRunEyeSubmissions: &ZERO32,
};
static EXPORTS: std::sync::atomic::AtomicPtr<Exports> = std::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
/// The plugin's exports (formerly lent by NativePreparation.cpp's kExports): native_state_exports.rs'. The Rust test
/// variants attach the .cpp's.
#[cfg(all(feature = "rust-build", not(test)))]
fn plugin_exports() -> &'static Exports {
    use crate::native_state_exports as n;
    static PLUGIN: std::sync::OnceLock<Exports> = std::sync::OnceLock::new();
    PLUGIN.get_or_init(|| Exports {
    CyberpunkVR_PreparationRequest: core::ptr::addr_of!(n::CyberpunkVR_PreparationRequest).cast_mut().cast(),
    CyberpunkVR_PreparationState: core::ptr::addr_of!(n::CyberpunkVR_PreparationState).cast_mut().cast(),
    CyberpunkVR_PreparationSeq: core::ptr::addr_of!(n::CyberpunkVR_PreparationSeq).cast_mut().cast(),
    CyberpunkVR_PreparationCount: core::ptr::addr_of!(n::CyberpunkVR_PreparationCount).cast_mut().cast(),
    CyberpunkVR_PreparationPacketCount: core::ptr::addr_of!(n::CyberpunkVR_PreparationPacketCount).cast_mut().cast(),
    CyberpunkVR_PreparationFailure: core::ptr::addr_of!(n::CyberpunkVR_PreparationFailure).cast_mut().cast(),
    CyberpunkVR_PreparationRecords: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationRecords).cast(),
    CyberpunkVR_PreparationPackets: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationPackets).cast(),
    CyberpunkVR_PreparationPairRequest: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairRequest).cast_mut().cast(),
    CyberpunkVR_PreparationPairState: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairState).cast_mut().cast(),
    CyberpunkVR_PreparationPairCompatible: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairCompatible).cast_mut().cast(),
    CyberpunkVR_PreparationPairData: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationPairData).cast(),
    CyberpunkVR_PreparationPairEligibility: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationPairEligibility).cast(),
    CyberpunkVR_PreparationPairRoot: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationPairRoot).cast(),
    CyberpunkVR_PreparationCameraTrace: core::ptr::addr_of!(n::CyberpunkVR_PreparationCameraTrace).cast_mut().cast(),
    CyberpunkVR_PreparationAllStages: core::ptr::addr_of!(n::CyberpunkVR_PreparationAllStages).cast_mut().cast(),
    CyberpunkVR_PreparationGraphParents: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationGraphParents).cast(),
    CyberpunkVR_PreparationGraphParentCount: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationGraphParentCount).cast(),
    CyberpunkVR_PreparationPairVerified: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairVerified).cast_mut().cast(),
    CyberpunkVR_PreparationRootSkipped: core::ptr::addr_of!(n::CyberpunkVR_PreparationRootSkipped).cast_mut().cast(),
    CyberpunkVR_PreparationRootCompatible: core::ptr::addr_of!(n::CyberpunkVR_PreparationRootCompatible).cast_mut().cast(),
    CyberpunkVR_PreparationPairMergeLimit: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairMergeLimit).cast_mut().cast(),
    CyberpunkVR_PreparationPairMerged: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairMerged).cast_mut().cast(),
    CyberpunkVR_PreparationPairCompleted: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairCompleted).cast_mut().cast(),
    CyberpunkVR_PreparationPairGeneration: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairGeneration).cast_mut().cast(),
    CyberpunkVR_PreparationRunRequest: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunRequest).cast_mut().cast(),
    CyberpunkVR_PreparationRunState: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunState).cast_mut().cast(),
    CyberpunkVR_PreparationRunStop: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunStop).cast_mut().cast(),
    CyberpunkVR_PreparationRunFailure: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunFailure).cast_mut().cast(),
    CyberpunkVR_PreparationRunFrames: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunFrames).cast_mut().cast(),
    CyberpunkVR_PreparationRunDepth: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunDepth).cast_mut().cast(),
    CyberpunkVR_PreparationRunEyeCopies: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunEyeCopies).cast_mut().cast(),
    CyberpunkVR_PreparationRunEyeSubmissions: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunEyeSubmissions).cast_mut().cast(),
    })
}
fn e() -> &'static Exports {
    // SAFETY: attached once with the .cpp's static.
    let attached = unsafe { EXPORTS.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or_else(|| Some(plugin_exports()));
    attached.unwrap_or(&FALLBACK)
}
// SAFETY (all): the exports, live for the process; the arrays written under the trace's mutex.
macro_rules! x {
    ($name:ident) => {
        unsafe { &*e().$name }
    };
}
fn pair_data() -> &'static mut [u64; 8] {
    unsafe { &mut *e().CyberpunkVR_PreparationPairData }
}

pub const RECORD_CAPACITY: usize = 8192;
pub const PACKET_CAPACITY: usize = 524_288;
/// preparation::Kind.
pub mod kind {
    pub const STAGE_BEGIN: u32 = 1;
    pub const STAGE_END: u32 = 5;
    pub const CONSUME: u32 = 6;
    pub const GRAPH_BEFORE: u32 = 7;
    pub const GRAPH_AFTER: u32 = 8;
    pub const CULL_BEGIN: u32 = 9;
    pub const CULL_END: u32 = 10;
    pub const ROOT_SUPPRESSED: u32 = 11;
    pub const EYE_COPY_RECORDED: u32 = 12;
    pub const DRAW_BEGIN: u32 = 13;
    pub const DRAW_END: u32 = 14;
    pub const SUBMIT_LIST: u32 = 15;
    pub const RESET_LIST: u32 = 16;
    pub const ROOT_COMPLETED: u32 = 17;
    pub const MATERIALIZE_BEGIN: u32 = 18;
    pub const MATERIALIZE_END: u32 = 19;
    pub const PROXY_GATHER_BEGIN: u32 = 20;
    pub const PROXY_GATHER_END: u32 = 21;
}

/// preparation::Record.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub sequence: u64,
    pub qpc: u64,
    pub scope: u64,
    pub context: u64,
    pub view: u64,
    pub storage: u64,
    pub input: u64,
    pub output: u64,
    pub frame: u32,
    pub kind: u32,
    pub side: i32,
    pub thread: u32,
    pub stage: u32,
    pub mode: u32,
    pub flags0: u32,
    pub flags1: u32,
    pub offset: u32,
    pub count: u32,
    pub status: u32,
    pub reserved: u32,
}
impl Default for Record {
    fn default() -> Self {
        Record { sequence: 0, qpc: 0, scope: 0, context: 0, view: 0, storage: 0, input: 0, output: 0, frame: 0, kind: 0, side: -1, thread: 0, stage: 0, mode: 0, flags0: 0, flags1: 0, offset: 0, count: 0, status: 0, reserved: 0 }
    }
}
/// preparation::Ticket.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ticket {
    pub epoch: u64,
    pub scope: u64,
    pub previous_epoch: u64,
    pub previous: Record,
}
/// preparation::MaterializeTicket.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MaterializeTicket {
    pub epoch: u64,
    pub row: Record,
}
/// preparation::CopySource.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopySource {
    pub view: u64,
    pub node: u32,
    pub side: i32,
}
const NO_COPY: CopySource = CopySource { view: 0, node: 0, side: -1 };
/// preparation::DrawTag.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawTag {
    pub scope: u64,
    pub stage: u32,
    pub frame: u32,
    pub side: i32,
}
const NO_TAG: DrawTag = DrawTag { scope: 0, stage: 0, frame: 0, side: -1 };
/// preparation::GraphParentRecord.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphParentRecord {
    pub scope: u64,
    pub context: u64,
    pub manager: u64,
    pub fingerprint: u64,
    pub object: u64,
    pub work: u64,
    pub name: u64,
    pub handle: u32,
    pub ordinal: u32,
    pub view_index: u32,
    pub slot: u32,
    pub status: u32,
    pub stage: u32,
}
/// DrawScope's members (the .cpp's class).
#[repr(C)]
pub struct DrawScope {
    pub previous: *mut DrawScope,
    pub source: Record,
    pub capture_epoch: u64,
    pub lists: [u64; 64],
    pub draws: [u32; 64],
    pub count: u32,
    pub valid: bool,
}
const _: () = assert!(
    core::mem::size_of::<Record>() == 112
        && core::mem::size_of::<Ticket>() == 136
        && core::mem::size_of::<MaterializeTicket>() == 120
        && core::mem::size_of::<CopySource>() == 16
        && core::mem::size_of::<DrawTag>() == 24
        && core::mem::size_of::<GraphParentRecord>() == 80
        && core::mem::offset_of!(DrawScope, source) == 8
        && core::mem::offset_of!(DrawScope, capture_epoch) == 120
        && core::mem::offset_of!(DrawScope, count) == 896
        && core::mem::size_of::<DrawScope>() == 904
);

struct State {
    frame: u32,
    limit: u32,
    epoch: u64,
    next_scope: u64,
    next_submission: u64,
    last_skip_frame: u32,
    run_budget: u32,
    run_deadline: u64,
    run_views: [u64; 2],
    /// false recorded, true submitted; a reset retires the generation.
    run_copies: BTreeMap<u64, bool>,
    records: Vec<Record>,
    data: Vec<Packet>,
    graph_parents: Vec<GraphParentRecord>,
}
static STATE: Mutex<State> = Mutex::new(State {
    frame: 0,
    limit: 0,
    epoch: 0,
    next_scope: 0,
    next_submission: 0,
    last_skip_frame: 0,
    run_budget: 0,
    run_deadline: 0,
    run_views: [0; 2],
    run_copies: BTreeMap::new(),
    records: Vec::new(),
    data: Vec::new(),
    graph_parents: Vec::new(),
});
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}
thread_local! {
    static CURRENT: Cell<(Record, u64)> = const { Cell::new((Record { sequence: 0, qpc: 0, scope: 0, context: 0, view: 0, storage: 0, input: 0, output: 0, frame: 0, kind: 0, side: -1, thread: 0, stage: 0, mode: 0, flags0: 0, flags1: 0, offset: 0, count: 0, status: 0, reserved: 0 }, 0)) };
    static DRAW_SCOPE: Cell<*mut DrawScope> = const { Cell::new(core::ptr::null_mut()) };
    static COPY_SOURCE: Cell<CopySource> = const { Cell::new(NO_COPY) };
}

pub fn active() -> bool {
    x!(CyberpunkVR_PreparationState).load(Relaxed) == 1
}
pub fn run_active() -> bool {
    x!(CyberpunkVR_PreparationRunState).load(Acquire) == 1
}
/// The pair's state (2: both views verified under MAIN's root), one of its data words (3 the VRCAM view, 4 MAIN's) and
/// the camera trace switch, for the peer camera's prediction.
pub fn pair_state() -> u32 {
    x!(CyberpunkVR_PreparationPairState).load(SeqCst)
}
pub fn pair_view(index: usize) -> u64 {
    pair_data()[index]
}
pub fn camera_trace() -> u32 {
    x!(CyberpunkVR_PreparationCameraTrace).load(SeqCst)
}
pub fn pairing() -> bool {
    active() || run_active()
}
fn read(address: u64, out: &mut [u8]) -> bool {
    crate::geometry_packets::read(address, out)
}
fn read_into<T: Copy>(address: u64, field: &mut T) -> bool {
    // SAFETY: the field's bytes (what arrived stays, as the .cpp's reads left it).
    read(address, unsafe { core::slice::from_raw_parts_mut((field as *mut T).cast::<u8>(), core::mem::size_of::<T>()) })
}
fn read_u64(address: u64) -> Option<u64> {
    let mut v = 0u64;
    read_into(address, &mut v).then_some(v)
}
fn now() -> u64 {
    let mut value = 0i64;
    // SAFETY: one output.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut value) };
    value as u64
}
fn thread() -> u32 {
    // SAFETY: a plain call.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}
fn ticks() -> u64 {
    // SAFETY: a plain call.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn publish(s: &State) {
    // SAFETY: the exports, under the trace's mutex.
    unsafe {
        *e().CyberpunkVR_PreparationRecords = s.records.as_ptr() as u64;
        *e().CyberpunkVR_PreparationPackets = s.data.as_ptr() as u64;
        *e().CyberpunkVR_PreparationGraphParents = s.graph_parents.as_ptr() as u64;
        *e().CyberpunkVR_PreparationGraphParentCount = s.graph_parents.len() as u32;
    }
    x!(CyberpunkVR_PreparationCount).store(s.records.len() as u32, SeqCst);
    x!(CyberpunkVR_PreparationPacketCount).store(s.data.len() as u32, SeqCst);
}
/// View: the context's view (+0x18) and its storage (+0x2C40), status 1 when not readable.
fn view(row: &mut Record) {
    let ok = row.context != 0 && row.context <= u64::MAX - 0x20 && read_into(row.context + 0x18, &mut row.view) && row.view != 0 && row.view <= u64::MAX - 0x2C48 && read_into(row.view + 0x2C40, &mut row.storage);
    if !ok {
        row.status = 1;
    }
}
/// Arguments: the stage call's 32 argument bytes -- the context, the stage (+0x10), a reserved word (+0x14), the mode's
/// three bytes (+0x18) -- and the context's view.
fn arguments(row: &mut Record, source: u64) -> bool {
    let mut bytes = [0u8; 32];
    if !read(source, &mut bytes) {
        row.status = 1;
        return false;
    }
    row.context = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
    row.stage = u32::from_le_bytes(bytes[0x10..0x14].try_into().unwrap());
    row.reserved = u32::from_le_bytes(bytes[0x14..0x18].try_into().unwrap());
    row.mode = u32::from(bytes[0x18]) | u32::from(bytes[0x19]) << 8 | u32::from(bytes[0x1A]) << 16;
    view(row);
    row.status == 0
}
/// Copy: `count` packets at `begin` into the packet store (at most 32768, within its capacity).
fn copy(s: &mut State, row: &mut Record, begin: u64, count: u32) {
    row.offset = s.data.len() as u32;
    if count == 0 {
        return;
    }
    if count > 32768 || count as usize > PACKET_CAPACITY - s.data.len() {
        row.status = 2;
        x!(CyberpunkVR_PreparationFailure).store(2, SeqCst);
        return;
    }
    let offset = s.data.len();
    s.data.resize(offset + count as usize, Packet::default());
    // SAFETY: the packets just made room for.
    let bytes = unsafe { core::slice::from_raw_parts_mut(s.data.as_mut_ptr().add(offset).cast::<u8>(), count as usize * 16) };
    if !read(begin, bytes) {
        s.data.truncate(offset);
        row.status = 1;
        return;
    }
    row.count = count;
}
fn array(s: &mut State, row: &mut Record) {
    let mut header = [0u8; 16];
    if !read(row.output, &mut header) {
        row.status = 1;
        return;
    }
    let (data, capacity, count) = (u64::from_le_bytes(header[0..8].try_into().unwrap()), u32::from_le_bytes(header[8..12].try_into().unwrap()), u32::from_le_bytes(header[12..16].try_into().unwrap()));
    if count > capacity {
        row.status = 1;
        return;
    }
    copy(s, row, data, count);
}
fn append(s: &mut State, mut row: Record, k: u32, with_array: bool) {
    if s.records.len() >= RECORD_CAPACITY {
        x!(CyberpunkVR_PreparationFailure).store(3, SeqCst);
        return;
    }
    row.kind = k;
    row.sequence = s.records.len() as u64 + 1;
    row.frame = s.frame;
    row.thread = thread();
    row.qpc = now();
    if with_array {
        array(s, &mut row);
    }
    s.records.push(row);
}
/// Change: a trace step under the mutex while the trace is active, published after.
fn change(f: impl FnOnce(&mut State)) {
    if !active() {
        return;
    }
    let mut s = lock();
    if !active() {
        return;
    }
    x!(CyberpunkVR_PreparationSeq).fetch_add(1, SeqCst);
    f(&mut s);
    publish(&s);
    x!(CyberpunkVR_PreparationSeq).fetch_add(1, SeqCst);
}
/// PairChange: a pair step under the mutex while pairing, published when the trace is active too.
fn pair_change(f: impl FnOnce(&mut State, bool)) {
    if !pairing() {
        return;
    }
    let mut s = lock();
    if !pairing() {
        return;
    }
    let trace = active();
    if trace {
        x!(CyberpunkVR_PreparationSeq).fetch_add(1, SeqCst);
    }
    f(&mut s, trace);
    if trace {
        publish(&s);
        x!(CyberpunkVR_PreparationSeq).fetch_add(1, SeqCst);
    }
}
fn pair_budget(s: &State) -> u32 {
    if run_active() {
        s.run_budget
    } else {
        x!(CyberpunkVR_PreparationPairMergeLimit).load(SeqCst)
    }
}
fn pair_frame(s: &State) -> u32 {
    if run_active() {
        x!(CyberpunkVR_PreparationRunFrames).load(SeqCst)
    } else {
        s.frame
    }
}
/// RunBoundary: a running pair run counted, ended on its deadline (45 s), a failure, or its budget (or a stop) met with
/// every merge completed (every eye copy submitted); else a run request of 30..1200 frames started after a verified
/// pair warmup.
fn run_boundary(s: &mut State) {
    if run_active() {
        let (skipped, completed) = (x!(CyberpunkVR_PreparationRootSkipped).load(SeqCst), x!(CyberpunkVR_PreparationPairCompleted).load(SeqCst));
        if ticks() >= s.run_deadline {
            x!(CyberpunkVR_PreparationRunFailure).store(2, SeqCst);
        }
        if x!(CyberpunkVR_PreparationRunFailure).load(SeqCst) != 0 || ((completed >= s.run_budget || x!(CyberpunkVR_PreparationRunStop).load(SeqCst) != 0) && completed == skipped) {
            x!(CyberpunkVR_PreparationPairRequest).store(0, SeqCst);
            if s.run_copies.values().any(|&submitted| !submitted) {
                x!(CyberpunkVR_PreparationRunFailure).store(8, SeqCst);
            }
            let failed = x!(CyberpunkVR_PreparationRunFailure).load(SeqCst) != 0;
            x!(CyberpunkVR_PreparationRunState).store(if failed { 3 } else { 2 }, Release);
            s.run_copies.clear();
            crate::eager_depth::cvr_eager_depth_end_run();
            return;
        }
        x!(CyberpunkVR_PreparationRunFrames).fetch_add(1, SeqCst);
        return;
    }
    let request = x!(CyberpunkVR_PreparationRunRequest).swap(0, SeqCst);
    if request == 0 {
        return;
    }
    x!(CyberpunkVR_PreparationRunFailure).store(0, SeqCst);
    if !(30..=1200).contains(&request)
        || active()
        || x!(CyberpunkVR_PreparationRequest).load(SeqCst) != 0
        || x!(CyberpunkVR_PreparationPairRequest).load(SeqCst) != 0
        || x!(CyberpunkVR_PreparationPairVerified).load(SeqCst) != 1
        || x!(CyberpunkVR_PreparationRootCompatible).load(SeqCst) != 1
        || x!(CyberpunkVR_PreparationPairCompatible).load(SeqCst) != 1
    {
        x!(CyberpunkVR_PreparationRunFailure).store(1, SeqCst);
        x!(CyberpunkVR_PreparationRunState).store(3, SeqCst);
        return;
    }
    s.run_budget = request;
    s.run_deadline = ticks() + 45000;
    crate::eager_depth::cvr_eager_depth_begin_run();
    s.run_views = [pair_data()[4], pair_data()[3]];
    s.run_copies.clear();
    x!(CyberpunkVR_PreparationRunFrames).store(1, SeqCst);
    x!(CyberpunkVR_PreparationRunStop).store(0, SeqCst);
    x!(CyberpunkVR_PreparationRunDepth)[0].store(0, SeqCst);
    x!(CyberpunkVR_PreparationRunDepth)[1].store(0, SeqCst);
    x!(CyberpunkVR_PreparationRunEyeCopies).store(0, SeqCst);
    x!(CyberpunkVR_PreparationRunEyeSubmissions).store(0, SeqCst);
    x!(CyberpunkVR_PreparationRootSkipped).store(0, SeqCst);
    x!(CyberpunkVR_PreparationPairMerged).store(0, SeqCst);
    x!(CyberpunkVR_PreparationPairCompleted).store(0, SeqCst);
    s.last_skip_frame = 0;
    x!(CyberpunkVR_PreparationRunState).store(1, SeqCst);
    x!(CyberpunkVR_PreparationPairRequest).store(3, SeqCst);
}
pub fn frame_boundary() {
    if !active() && x!(CyberpunkVR_PreparationRequest).load(Relaxed) == 0 && !run_active() && x!(CyberpunkVR_PreparationRunRequest).load(SeqCst) == 0 {
        return;
    }
    let mut s = lock();
    run_boundary(&mut s);
    if active() {
        s.frame += 1;
        if s.frame > s.limit {
            x!(CyberpunkVR_PreparationState).store(2, SeqCst);
        }
        return;
    }
    let request = x!(CyberpunkVR_PreparationRequest).swap(0, SeqCst);
    if request == 0 {
        return;
    }
    x!(CyberpunkVR_PreparationSeq).fetch_add(1, SeqCst);
    s.records.clear();
    s.data.clear();
    s.graph_parents.clear();
    s.epoch += 1;
    s.frame = 1;
    s.limit = request.min(if x!(CyberpunkVR_PreparationCameraTrace).load(SeqCst) != 0 { 8 } else { 4 });
    s.next_scope = 0;
    s.next_submission = 0;
    if x!(CyberpunkVR_PreparationPairRequest).load(SeqCst) == 3 && !run_active() {
        x!(CyberpunkVR_PreparationRootSkipped).store(0, SeqCst);
        x!(CyberpunkVR_PreparationPairMerged).store(0, SeqCst);
        x!(CyberpunkVR_PreparationPairCompleted).store(0, SeqCst);
        s.last_skip_frame = 0;
    }
    x!(CyberpunkVR_PreparationFailure).store(0, SeqCst);
    publish(&s);
    x!(CyberpunkVR_PreparationSeq).fetch_add(1, SeqCst);
    x!(CyberpunkVR_PreparationState).store(1, SeqCst);
}
pub fn begin_stage(context: u64, buckets: u64, output: u64, flags0: u32, flags1: u32, side: i32) -> Ticket {
    let mut ticket = Ticket::default();
    change(|s| {
        let mut row = Record { context: 0, input: buckets, output, flags0, flags1, side, ..Record::default() };
        s.next_scope += 1;
        row.scope = s.next_scope;
        let (current, current_epoch) = CURRENT.with(Cell::get);
        ticket = Ticket { epoch: s.epoch, scope: row.scope, previous_epoch: current_epoch, previous: current };
        if !arguments(&mut row, context) {
            if side >= 0 {
                x!(CyberpunkVR_PreparationFailure).store(1, SeqCst);
            }
            row.scope = 0;
            CURRENT.with(|c| c.set((Record::default(), 0)));
            append(s, row, kind::STAGE_BEGIN, false);
            return;
        }
        CURRENT.with(|c| c.set((row, s.epoch)));
        append(s, row, kind::STAGE_BEGIN, false);
    });
    ticket
}
pub fn end_stage(ticket: &Ticket) {
    if ticket.scope == 0 {
        return;
    }
    change(|s| {
        let (current, current_epoch) = CURRENT.with(Cell::get);
        if ticket.epoch == s.epoch && current_epoch == s.epoch && current.scope == ticket.scope {
            append(s, current, kind::STAGE_END, true);
        }
    });
    CURRENT.with(|c| c.set((ticket.previous, ticket.previous_epoch)));
}
pub fn output(k: u32, output: u64) {
    change(|s| {
        let (current, current_epoch) = CURRENT.with(Cell::get);
        if current_epoch != s.epoch || current.scope == 0 {
            return;
        }
        append(s, Record { output, ..current }, k, true);
    });
}
pub fn consume(node: u32, side: i32, context: u64, span: u64) {
    if node != 0x33DDA4 || !(0..=1).contains(&side) {
        return;
    }
    if run_active() {
        let mut row = Record { side, ..Record::default() };
        if arguments(&mut row, context) && row.stage == 3 {
            let views = lock().run_views;
            if row.view == views[side as usize] {
                x!(CyberpunkVR_PreparationRunDepth)[side as usize].fetch_add(1, SeqCst);
            } else {
                x!(CyberpunkVR_PreparationRunFailure).store(5, SeqCst);
            }
        }
    }
    change(|s| {
        let mut row = Record { side, ..Record::default() };
        arguments(&mut row, context);
        let mut range = [0u8; 16];
        let valid = read(span, &mut range);
        let (begin, end) = (u64::from_le_bytes(range[0..8].try_into().unwrap()), u64::from_le_bytes(range[8..16].try_into().unwrap()));
        if !valid || end < begin || (end - begin) % 16 != 0 || (end - begin) / 16 > 32768 {
            row.status = 1;
        } else {
            copy(s, &mut row, begin, ((end - begin) / 16) as u32);
        }
        append(s, row, kind::CONSUME, false);
    });
}
pub fn graph(manager: u64, after: bool) {
    change(|s| {
        let k = if after { kind::GRAPH_AFTER } else { kind::GRAPH_BEFORE };
        let mut row = Record { context: manager, ..Record::default() };
        let mut views = 0u64;
        let mut count = 0u32;
        if row.context > u64::MAX - 0x58 || !read_into(row.context + 0x48, &mut views) || !read_into(row.context + 0x54, &mut count) || count > 64 {
            row.status = 1;
            append(s, row, k, false);
            return;
        }
        if count == 0 {
            append(s, row, k, false);
            return;
        }
        row.flags0 = count;
        for i in 0..count {
            row.flags1 = i;
            row.status = 0;
            row.view = 0;
            row.storage = 0;
            row.input = 0;
            let ok = read_into(views.wrapping_add(u64::from(i) * 8), &mut row.view) && row.view <= u64::MAX - 0x2C48 && read_into(row.view + 0x2C40, &mut row.storage) && read_into(row.view + 0x28, &mut row.input);
            if !ok {
                row.status = 1;
            }
            append(s, row, k, false);
        }
    });
}
pub fn cull(job: u64, output: u64, side: i32, after: bool, definition: u64) {
    change(|s| {
        let mut row = Record { context: job, output, side, ..Record::default() };
        view(&mut row);
        row.input = definition;
        if definition != 0 {
            read_into(row.input.wrapping_add(0x18), &mut row.flags0);
        }
        let mut byte = row.flags1 as u8;
        if read_into(row.context.wrapping_add(0x30), &mut byte) {
            row.flags1 = u32::from(byte);
        }
        read_into(row.context.wrapping_add(0x34), &mut row.mode);
        append(s, row, if after { kind::CULL_END } else { kind::CULL_BEGIN }, false);
    });
}
pub fn begin_materialize(task: u64, side: i32) -> MaterializeTicket {
    let mut ticket = MaterializeTicket::default();
    change(|s| {
        let mut row = Record { side, context: task, ..Record::default() };
        view(&mut row);
        read_into(row.context.wrapping_add(72), &mut row.input);
        read_into(row.context.wrapping_add(80), &mut row.output);
        read_into(row.context.wrapping_add(352), &mut row.flags0);
        let mut byte = row.flags1 as u8;
        if read_into(row.context.wrapping_add(0x30), &mut byte) {
            row.flags1 = u32::from(byte);
        }
        read_into(row.context.wrapping_add(0x34), &mut row.mode);
        s.next_scope += 1;
        row.scope = s.next_scope;
        ticket = MaterializeTicket { epoch: s.epoch, row };
        append(s, row, kind::MATERIALIZE_BEGIN, false);
    });
    ticket
}
pub fn end_materialize(ticket: &MaterializeTicket, k: u32) {
    if ticket.epoch != 0 {
        change(|s| {
            if ticket.epoch == s.epoch {
                append(s, ticket.row, k, false);
            }
        });
    }
}
/// BeginProxyGather: the view's proxies (a span of visible ids, read twice): for each its gather method (the vtable at
/// id - 16, slot +0xF8) and update method (+0xF0) as two packets.
pub fn begin_proxy_gather(context: u64, range: u64, side: i32) -> MaterializeTicket {
    let mut ticket = MaterializeTicket::default();
    if !(0..=1).contains(&side) {
        return ticket;
    }
    change(|s| {
        let mut row = Record { side, context, ..Record::default() };
        view(&mut row);
        s.next_scope += 1;
        row.scope = s.next_scope;
        row.input = range;
        row.offset = s.data.len() as u32;
        row.reserved = 2;
        let (mut span, mut after) = ([0u8; 16], [0u8; 16]);
        let ok = read_into(row.context.wrapping_add(0x48), &mut row.output) && row.output == row.storage && read(row.input, &mut span);
        let (begin, end) = (u64::from_le_bytes(span[0..8].try_into().unwrap()), u64::from_le_bytes(span[8..16].try_into().unwrap()));
        if !ok || end < begin || (end - begin) % 8 != 0 {
            row.status = 1;
        }
        let count = end.wrapping_sub(begin) / 8;
        if row.status == 0 && (count > 16384 || count * 2 > (PACKET_CAPACITY - s.data.len()) as u64) {
            row.status = 2;
        }
        if row.status == 0 {
            let mut ids = vec![0u8; count as usize * 8];
            let mut check = vec![0u8; ids.len()];
            let mut methods: Vec<Packet> = Vec::with_capacity(count as usize * 2);
            if count != 0 && !read(begin, &mut ids) {
                row.status = 1;
            }
            for id in ids.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())) {
                let mut functions = [0u8; 16];
                let table = if row.status == 0 && id >= 16 { read_u64(id - 16) } else { None };
                let Some(table) = table.filter(|&t| t != 0 && t <= u64::MAX - 0x100 && read(t + 0xF0, &mut functions)) else {
                    row.status = 1;
                    break;
                };
                let (update, gather) = (u64::from_le_bytes(functions[0..8].try_into().unwrap()), u64::from_le_bytes(functions[8..16].try_into().unwrap()));
                if gather == 0 {
                    row.status = 1;
                    break;
                }
                methods.push(Packet { first: id, second: gather });
                methods.push(Packet { first: table, second: update });
            }
            if !read(row.input, &mut after) || after != span || (count != 0 && (!read(begin, &mut check) || check != ids)) {
                row.status = 1;
            }
            if row.status == 0 {
                row.flags0 = count as u32;
                row.count = methods.len() as u32;
                s.data.extend(methods);
            }
        }
        ticket = MaterializeTicket { epoch: s.epoch, row };
        append(s, row, kind::PROXY_GATHER_BEGIN, false);
    });
    ticket
}
pub type PrepareGraph = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void;
/// PreparePair: the bounded pair diagnostics around the graph preparation (`original`). Mode 2/3: MAIN's
/// output-owning root (its composition, provider and builder mode; the views' sizes equal) selects both registered
/// views by a two-name request of its own -- the original request vector is never rewritten -- and the result is
/// verified (both views, their storage); mode 1 records and refuses an unowned RTT pair (state 4).
///
/// # Safety
/// `original` is the engine's preparation; the pointers are its call's.
#[allow(clippy::too_many_arguments)]
pub unsafe fn prepare_pair(original: Option<PrepareGraph>, manager: *mut c_void, arguments: *mut c_void, requested: *mut c_void, eye_name: u64, main_view: u64, gameplay: bool, vrcam_view: u64) -> *mut c_void {
    let Some(original) = original else { return core::ptr::null_mut() };
    let call = |requests: *mut c_void| unsafe { original(manager, arguments, requests) };
    if !gameplay || !pairing() || x!(CyberpunkVR_PreparationPairRequest).load(Acquire) == 0 || eye_name == 0 || main_view == 0 {
        return call(requested);
    }
    let mut native = [0u8; 16];
    let readable = read(requested as u64, &mut native);
    let (data, capacity, count) = (u64::from_le_bytes(native[0..8].try_into().unwrap()), u32::from_le_bytes(native[8..12].try_into().unwrap()), u32::from_le_bytes(native[12..16].try_into().unwrap()));
    let mut first = 0u64;
    if count != 0 {
        read_into(data, &mut first);
    }
    let mut name = 0u64;
    let main_read = main_view <= u64::MAX - 0x30 && read_into(main_view + 0x28, &mut name);
    // SAFETY: the exports.
    let eligibility = unsafe { &mut *e().CyberpunkVR_PreparationPairEligibility };
    eligibility[0] += 1;
    eligibility[1] = u64::from(count);
    eligibility[2] = u64::from(capacity);
    eligibility[3] = first;
    eligibility[4] = eye_name;
    eligibility[5] = name;
    eligibility[6] = main_view;
    eligibility[7] = readable as u64;
    let pair_request = x!(CyberpunkVR_PreparationPairRequest).load(SeqCst);
    if pair_request == 2 || pair_request == 3 {
        if pair_request == 3 && x!(CyberpunkVR_PreparationRootSkipped).load(SeqCst) != x!(CyberpunkVR_PreparationPairMerged).load(SeqCst) + 1 {
            return call(requested);
        }
        if !readable || count != 1 || capacity < 1 || first != 0 || !main_read || name != 0 {
            return call(requested);
        }
        let (mut source, mut composition, mut provider, mut vrcam_name, mut mode, mut main_size, mut eye_size) = (0u64, [0u64; 2], 0u64, 0u64, 0u32, [0u32; 2], [0u32; 2]);
        let root_valid = read_into((arguments as u64).wrapping_add(16), &mut source)
            && source != 0
            && read_into(source.wrapping_add(0x168), &mut composition)
            && composition[0] != 0
            && composition[1] != 0
            && read_into(source.wrapping_add(0xF98), &mut provider)
            && provider != 0
            && read_into(source.wrapping_add(0xFA0), &mut mode)
            && mode <= 1
            && vrcam_view != 0
            && read_into(vrcam_view.wrapping_add(0x28), &mut vrcam_name)
            && vrcam_name == eye_name
            && read_into(main_view.wrapping_add(0x54), &mut main_size)
            && read_into(vrcam_view.wrapping_add(0x54), &mut eye_size)
            && main_size[0] != 0
            && main_size[1] != 0
            && main_size[0] <= 16384
            && main_size[1] <= 16384
            && main_size == eye_size;
        if x!(CyberpunkVR_PreparationPairRequest).swap(0, SeqCst) == 0 {
            return call(requested);
        }
        // SAFETY: the exports.
        let root = unsafe { &mut *e().CyberpunkVR_PreparationPairRoot };
        *root = [source, composition[0], composition[1], provider, u64::from(mode), u64::from(main_size[0]), u64::from(main_size[1]), root_valid as u64];
        *pair_data() = [0; 8];
        let views = lock().run_views;
        if !root_valid || (run_active() && (main_view != views[0] || vrcam_view != views[1])) {
            x!(CyberpunkVR_PreparationPairState).store(5, SeqCst);
            if run_active() {
                x!(CyberpunkVR_PreparationRunFailure).store(3, SeqCst);
            }
            return call(requested);
        }
        // Diagnostic only: MAIN's output-owning root selects both registered views.
        x!(CyberpunkVR_PreparationPairState).store(1, SeqCst);
        let names = [0u64, eye_name];
        let mut pair = [0u8; 16];
        pair[0..8].copy_from_slice(&(names.as_ptr() as u64).to_le_bytes());
        pair[8..12].copy_from_slice(&2u32.to_le_bytes());
        pair[12..16].copy_from_slice(&2u32.to_le_bytes());
        let result = call(pair.as_mut_ptr().cast());
        let (mut active_views, mut view_count) = (0u64, 0u32);
        let mut valid = read_into((manager as u64).wrapping_add(0x48), &mut active_views) && read_into((manager as u64).wrapping_add(0x54), &mut view_count) && view_count == 2;
        let data = pair_data();
        data[0] = eye_name;
        data[1] = 0;
        data[2] = u64::from(view_count);
        let mut i = 0u32;
        while valid && i < view_count {
            let (mut v, mut key, mut storage) = (0u64, 0u64, 0u64);
            valid = read_into(active_views.wrapping_add(u64::from(i) * 8), &mut v) && read_into(v.wrapping_add(0x28), &mut key) && read_into(v.wrapping_add(0x2C40), &mut storage) && storage != 0;
            if key == eye_name {
                data[3] = v;
                data[5] = storage;
            } else if key == 0 {
                data[4] = v;
                data[6] = storage;
            } else {
                valid = false;
            }
            i += 1;
        }
        data[7] = (valid && data[3] == vrcam_view && data[4] == main_view && data[5] != 0 && data[6] != 0 && data[5] != data[6]) as u64;
        if data[7] != 0 {
            x!(CyberpunkVR_PreparationPairVerified).store(1, SeqCst);
            x!(CyberpunkVR_PreparationPairGeneration).fetch_add(1, SeqCst);
        }
        if data[7] != 0 && pair_request == 3 {
            x!(CyberpunkVR_PreparationPairMerged).fetch_add(1, SeqCst);
        }
        if data[7] == 0 && run_active() {
            x!(CyberpunkVR_PreparationRunFailure).store(3, SeqCst);
        }
        x!(CyberpunkVR_PreparationPairState).store(if data[7] != 0 { 2 } else { 3 }, Release);
        return result;
    }
    // The is_main_view contract identifies MAIN by the zero CName.
    if !readable || count != 1 || capacity < 1 || first != eye_name || !main_read || name != 0 {
        return call(requested);
    }
    if x!(CyberpunkVR_PreparationPairRequest).swap(0, SeqCst) == 0 {
        return call(requested);
    }
    let data = pair_data();
    *data = [0; 8];
    data[0] = eye_name;
    data[1] = name;
    data[4] = main_view;
    // 2026-10-01 PID19072: a two-name request reached DrawComposition +20A3FA with texture handle 0; view selection is
    // not sufficient (the outer RTT request does not supply MAIN's composition output resource).
    x!(CyberpunkVR_PreparationPairState).store(4, Release);
    call(requested)
}
/// SuppressRtt: in mode 3, after a verified warmup, the standalone VRCAM request (the full gameplay builder with one
/// name, the eye's) suppressed once per frame within the budget, every earlier merge completed.
pub fn suppress_rtt(frame_data: u64, eye_name: u64, gameplay: bool) -> bool {
    if !gameplay
        || !pairing()
        || x!(CyberpunkVR_PreparationPairRequest).load(SeqCst) != 3
        || x!(CyberpunkVR_PreparationPairVerified).load(SeqCst) == 0
        || (run_active() && (x!(CyberpunkVR_PreparationRunFailure).load(SeqCst) != 0 || x!(CyberpunkVR_PreparationRunStop).load(SeqCst) != 0))
        || eye_name == 0
    {
        return false;
    }
    let (mut mode, mut requests, mut name, mut capacity, mut count) = (0u32, 0u64, 0u64, 0u32, 0u32);
    if !read_into(frame_data.wrapping_add(0xFA0), &mut mode)
        || mode != 0
        || !read_into(frame_data.wrapping_add(0x140), &mut requests)
        || !read_into(frame_data.wrapping_add(0x148), &mut capacity)
        || !read_into(frame_data.wrapping_add(0x14C), &mut count)
        || count != 1
        || capacity < 1
        || !read_into(requests, &mut name)
        || name != eye_name
    {
        return false;
    }
    let mut suppressed = false;
    pair_change(|s, trace| {
        let skipped = x!(CyberpunkVR_PreparationRootSkipped).load(SeqCst);
        let budget = pair_budget(s);
        let bound = if run_active() { 1200 } else if x!(CyberpunkVR_PreparationCameraTrace).load(SeqCst) != 0 { 7 } else { 3 };
        if budget == 0 || budget > bound || skipped >= budget || skipped != x!(CyberpunkVR_PreparationPairMerged).load(SeqCst) || skipped != x!(CyberpunkVR_PreparationPairCompleted).load(SeqCst) || s.last_skip_frame == pair_frame(s) {
            return;
        }
        x!(CyberpunkVR_PreparationRootSkipped).fetch_add(1, SeqCst);
        s.last_skip_frame = pair_frame(s);
        suppressed = true;
        if trace {
            append(s, Record { side: 1, input: frame_data, flags0: mode, ..Record::default() }, kind::ROOT_SUPPRESSED, false);
        }
    });
    suppressed
}
/// RootCompleted: MAIN's paired root returned with its composition unchanged; the merge counted complete and the next
/// one asked for within the budget.
pub fn root_completed(frame_data: u64, generation_before: u64) {
    pair_change(|s, trace| {
        if x!(CyberpunkVR_PreparationPairGeneration).load(SeqCst) == generation_before {
            return;
        }
        // SAFETY: the exports.
        let root = unsafe { *e().CyberpunkVR_PreparationPairRoot };
        if x!(CyberpunkVR_PreparationPairState).load(SeqCst) != 2 || frame_data != root[0] {
            return;
        }
        let mut composition = [0u64; 2];
        let read_ok = read_into(frame_data.wrapping_add(0x168), &mut composition);
        let mut row = Record { context: frame_data, ..Record::default() };
        row.flags0 = (read_ok && composition[0] == root[1] && composition[1] != 0 && composition[1] == root[2]) as u32;
        row.input = composition[0];
        row.output = composition[1];
        if trace {
            append(s, row, kind::ROOT_COMPLETED, false);
        }
        if row.flags0 == 0 {
            x!(CyberpunkVR_PreparationPairVerified).store(0, SeqCst);
            x!(CyberpunkVR_PreparationPairState).store(6, SeqCst);
            x!(CyberpunkVR_PreparationPairRequest).store(0, SeqCst);
            if run_active() {
                x!(CyberpunkVR_PreparationRunFailure).store(3, SeqCst);
            }
            return;
        }
        let merged = x!(CyberpunkVR_PreparationPairMerged).load(SeqCst);
        if merged == x!(CyberpunkVR_PreparationPairCompleted).load(SeqCst) + 1 && merged == x!(CyberpunkVR_PreparationRootSkipped).load(SeqCst) {
            x!(CyberpunkVR_PreparationPairCompleted).store(merged, SeqCst);
            if merged < pair_budget(s) && x!(CyberpunkVR_PreparationRunStop).load(SeqCst) == 0 {
                x!(CyberpunkVR_PreparationPairRequest).store(3, SeqCst);
            }
        }
    });
}
pub fn eye_copy_recorded(list: u64, source: u64, destination: u64) {
    let copy_source = COPY_SOURCE.with(Cell::get);
    if run_active() {
        pair_change(|s, _| {
            if copy_source.side != 1 || copy_source.view != s.run_views[1] || list == 0 || s.run_copies.len() >= 64 || s.run_copies.contains_key(&list) {
                x!(CyberpunkVR_PreparationRunFailure).store(6, SeqCst);
                return;
            }
            s.run_copies.insert(list, false);
            x!(CyberpunkVR_PreparationRunEyeCopies).fetch_add(1, SeqCst);
        });
    }
    change(|s| {
        append(s, Record { side: copy_source.side, view: copy_source.view, flags0: copy_source.node, context: list, input: source, output: destination, ..Record::default() }, kind::EYE_COPY_RECORDED, false);
    });
}
pub fn set_copy_source(value: CopySource) -> CopySource {
    COPY_SOURCE.with(|c| c.replace(value))
}
/// DrawScope's constructor: the scope pushed; a consumer stage 3 call's (any stage with AllStages) source row captured,
/// with its graph parent when AllStages.
///
/// # Safety
/// `scope` is the .cpp's live DrawScope.
pub unsafe fn draw_scope_enter(scope: *mut DrawScope, node: u32, side: i32, args: u64) {
    unsafe { (*scope).previous = DRAW_SCOPE.with(|d| d.replace(scope)) };
    if node != 0x33DDA4 || !(0..=1).contains(&side) {
        return;
    }
    change(|s| {
        // SAFETY: as documented.
        let d = unsafe { &mut *scope };
        d.source.side = side;
        if arguments(&mut d.source, args) && (d.source.stage == 3 || x!(CyberpunkVR_PreparationAllStages).load(SeqCst) != 0) {
            d.capture_epoch = s.epoch;
            s.next_scope += 1;
            d.source.scope = s.next_scope;
            d.source.frame = s.frame;
            d.valid = true;
            if x!(CyberpunkVR_PreparationAllStages).load(SeqCst) != 0 && s.graph_parents.len() < RECORD_CAPACITY {
                let mut parent = GraphParentRecord { scope: d.source.scope, context: d.source.context, stage: d.source.stage, ..GraphParentRecord::default() };
                describe_graph_parent(d.source.context, side, d.source.frame, d.capture_epoch, &mut parent);
                s.graph_parents.push(parent);
            }
        }
    });
}
/// DrawScope's destructor: each list's draw count as a row, the scope popped.
///
/// # Safety
/// `scope` is the .cpp's live DrawScope.
pub unsafe fn draw_scope_leave(scope: *mut DrawScope) {
    // SAFETY: as documented.
    let d = unsafe { &*scope };
    if d.valid {
        change(|s| {
            if d.capture_epoch != s.epoch {
                return;
            }
            for i in 0..d.count as usize {
                append(s, Record { output: d.lists[i], flags0: d.draws[i], ..d.source }, kind::DRAW_END, false);
            }
        });
    }
    DRAW_SCOPE.with(|c| c.set(d.previous));
}
pub fn current_draw_tag() -> DrawTag {
    let scope = DRAW_SCOPE.with(Cell::get);
    // SAFETY: the thread's live scope or null.
    let Some(d) = (unsafe { scope.as_ref() }) else { return NO_TAG };
    if !d.valid || !active() {
        return NO_TAG;
    }
    let s = lock();
    if d.capture_epoch == s.epoch && active() {
        DrawTag { scope: d.source.scope, stage: d.source.stage, frame: d.source.frame, side: d.source.side }
    } else {
        NO_TAG
    }
}
pub fn draw_recorded(list: u64) {
    let scope = DRAW_SCOPE.with(Cell::get);
    if !active() || scope.is_null() || list == 0 {
        return;
    }
    // SAFETY: the thread's live scope.
    let d = unsafe { &mut *scope };
    if !d.valid {
        return;
    }
    if let Some(i) = d.lists[..d.count as usize].iter().position(|&l| l == list) {
        d.draws[i] += 1;
        return;
    }
    change(|s| {
        if d.capture_epoch != s.epoch {
            return;
        }
        if d.count as usize == d.lists.len() {
            x!(CyberpunkVR_PreparationFailure).store(5, SeqCst);
            return;
        }
        d.lists[d.count as usize] = list;
        d.draws[d.count as usize] = 1;
        d.count += 1;
        append(s, Record { output: list, ..d.source }, kind::DRAW_BEGIN, false);
    });
}
pub fn submit_lists(queue: u64, lists: &[u64], serialized: bool) {
    if queue == 0 || lists.is_empty() {
        return;
    }
    if run_active() {
        pair_change(|s, _| {
            for list in lists {
                match s.run_copies.get_mut(list) {
                    None => {}
                    Some(submitted) if *submitted => x!(CyberpunkVR_PreparationRunFailure).store(7, SeqCst),
                    Some(submitted) => {
                        *submitted = true;
                        x!(CyberpunkVR_PreparationRunEyeSubmissions).fetch_add(1, SeqCst);
                    }
                }
            }
        });
    }
    change(|s| {
        if lists.len() > 256 {
            x!(CyberpunkVR_PreparationFailure).store(5, SeqCst);
            return;
        }
        s.next_submission += 1;
        let batch = s.next_submission;
        for (i, &list) in lists.iter().enumerate() {
            append(s, Record { scope: batch, context: list, input: queue, flags0: i as u32, flags1: lists.len() as u32, reserved: serialized as u32, ..Record::default() }, kind::SUBMIT_LIST, false);
        }
    });
}
pub fn reset_list(list: u64) {
    if run_active() {
        pair_change(|s, _| {
            if let Some(submitted) = s.run_copies.remove(&list) {
                if !submitted {
                    x!(CyberpunkVR_PreparationRunFailure).store(7, SeqCst);
                }
            }
        });
    }
    change(|s| append(s, Record { context: list, ..Record::default() }, kind::RESET_LIST, false));
}
pub fn pair_prepare_compatible() -> bool {
    if x!(CyberpunkVR_PreparationPairRequest).load(Relaxed) == 0 {
        return false;
    }
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    let compatible = *COMPATIBLE.get_or_init(|| {
        // SAFETY: the executable's own module.
        let base = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 };
        crate::binding_code::cvr_binding_code(0, base)
    });
    x!(CyberpunkVR_PreparationPairCompatible).store(if compatible { 1 } else { 2 }, Release);
    compatible
}
/// The trace's frame and epoch while active (the .cpp's graph parent reads them under the trace's mutex).
pub fn frame_epoch() -> Option<(u32, u64)> {
    let s = lock();
    active().then_some((s.frame, s.epoch))
}
/// CurrentGraphParent's scope: the thread's capturing draw scope's source (context, scope, stage) under the expected
/// frame, and the epoch.
pub fn graph_scope(expected_frame: u32) -> Option<(Option<(u64, u64, u32)>, u64)> {
    let s = lock();
    if !active() || s.frame != expected_frame {
        return None;
    }
    let scope = DRAW_SCOPE.with(Cell::get);
    // SAFETY: the thread's live scope or null.
    let source = unsafe { scope.as_ref() }.filter(|d| d.valid && d.capture_epoch == s.epoch).map(|d| (d.source.context, d.source.scope, d.source.stage));
    Some((source, s.epoch))
}

// ---- the graph parent (formerly NativePreparation.cpp's PublishGraphParentSource, PrepareGraphParentCodeGuards,
// DescribeGraphParent, CurrentGraphParent, RevalidateGraphParent and the thread's graph context)

/// The pair frame's published render graph, its fingerprint and the frame and capture epoch it belongs to.
struct ParentSource {
    source: Option<std::sync::Arc<crate::native::graph_snapshot::Snapshot>>,
    fingerprint: u64,
    base: u64,
    frame: u32,
    epoch: u64,
}
static PARENT: Mutex<ParentSource> = Mutex::new(ParentSource { source: None, fingerprint: 0, base: 0, frame: 0, epoch: 0 });
/// The original image's certificate of the four graph-parent functions, captured once before the detours.
static PARENT_CODE: Mutex<Option<crate::native::graph_parent::CodeReceipt>> = Mutex::new(None);
thread_local! {
    /// The graph context the native graph work runs under on this thread (GraphContextScope).
    static GRAPH_CONTEXT: Cell<u64> = const { Cell::new(0) };
}
fn parent_lock() -> MutexGuard<'static, ParentSource> {
    PARENT.lock().unwrap_or_else(|e| e.into_inner())
}
fn parent_reset() {
    let mut p = parent_lock();
    p.source = None;
    p.fingerprint = 0;
    p.base = 0;
    p.epoch = 0;
    p.frame = 0;
}
/// PublishGraphParentSource: the snapshot the pair frame's graph was read into, for the parents of this frame.
pub fn publish_graph_parent_source(nodes: &[crate::native::graph_snapshot::Node], edges: &[crate::native::graph_snapshot::Edge], failure: u32, base: u64) {
    let (frame, epoch) = frame_epoch().unwrap_or((0, 0));
    let mut p = parent_lock();
    p.source = None;
    p.fingerprint = 0;
    p.base = base;
    p.frame = frame;
    p.epoch = epoch;
    if frame == 0 || failure != 0 || nodes.is_empty() {
        return;
    }
    let snapshot = crate::native::graph_snapshot::Snapshot { nodes: nodes.to_vec(), edges: edges.to_vec(), failure };
    p.source = Some(std::sync::Arc::new(snapshot));
    p.fingerprint = crate::paired_frontier::fingerprint(nodes, edges, failure, base);
}
/// PrepareGraphParentCodeGuards.
pub fn prepare_graph_parent_code_guards(base: u64) {
    const FUNCTIONS: [crate::native::graph_parent::Code; 4] = [
        crate::native::graph_parent::Code { rva: 0x40B10C, bytes: 0x82, hash: 0x177E_F1EE_2846_E667 },
        crate::native::graph_parent::Code { rva: 0x333374, bytes: 0x98, hash: 0x6B7C_1293_E981_4833 },
        crate::native::graph_parent::Code { rva: 0x40AEF8, bytes: 0xD8, hash: 0x4232_C2AC_8E04_3AAD },
        crate::native::graph_parent::Code { rva: 0xB3FCFC, bytes: 0xAF, hash: 0x9125_9D62_4E26_FF57 },
    ];
    let mut code = PARENT_CODE.lock().unwrap_or_else(|e| e.into_inner());
    let receipt = code.get_or_insert_with(Default::default);
    if receipt.attempted() {
        return;
    }
    let mut reader = |at: u64, out: &mut [u8]| read(at, out);
    receipt.capture(base, Some(&mut reader), &FUNCTIONS);
}
/// DescribeGraphParent: the producer of `context` in the published graph of the expected frame, or the stage it
/// failed at in `row.status`.
pub fn describe_graph_parent(context: u64, side: i32, expected_frame: u32, expected_epoch: u64, row: &mut GraphParentRecord) -> bool {
    row.status = 1;
    if !(0..=1).contains(&side) {
        return false;
    }
    let (source, fingerprint, base, frame, epoch) = {
        let p = parent_lock();
        (p.source.clone(), p.fingerprint, p.base, p.frame, p.epoch)
    };
    let Some(source) = source else { return false };
    if fingerprint == 0 || base == 0 || expected_frame == 0 || expected_frame != frame || expected_epoch == 0 || expected_epoch != epoch {
        return false;
    }
    prepare_graph_parent_code_guards(base);
    {
        let code = PARENT_CODE.lock().unwrap_or_else(|e| e.into_inner());
        row.status = 2;
        if !code.as_ref().is_some_and(|c| c.verified_for(base)) {
            return false;
        }
    }
    row.status = 3;
    let mut reader = |at: u64, out: &mut [u8]| read(at, out);
    let native = crate::native::graph_parent::read_context(Some(&mut reader), context);
    let view = pair_data()[if side != 0 { 3 } else { 4 }];
    let Some(native) = native.filter(|n| n.view_index == side as u32 && n.view == view) else { return false };
    row.manager = native.manager;
    row.status = 4;
    let Some(receipt) = crate::native::graph_parent::resolve(&source, &native) else { return false };
    row.fingerprint = fingerprint;
    row.object = receipt.object;
    row.work = receipt.work;
    row.name = receipt.name;
    row.handle = receipt.handle;
    row.ordinal = receipt.ordinal;
    row.view_index = receipt.view_index;
    row.slot = receipt.slot;
    row.status = 0;
    true
}
/// CurrentGraphParent: the parent of the graph work this thread runs now (its capturing scope's context, else the
/// thread's graph context).
pub fn current_graph_parent(side: i32, expected_frame: u32, row: &mut GraphParentRecord) -> bool {
    *row = GraphParentRecord::default();
    let mut context = GRAPH_CONTEXT.with(Cell::get);
    let (mut scope, mut stage) = (0u64, 0u32);
    let Some((captured, epoch)) = graph_scope(expected_frame) else { return false };
    if let Some((c, sc, st)) = captured {
        (context, scope, stage) = (c, sc, st);
    }
    row.context = context;
    row.scope = scope;
    row.stage = stage;
    describe_graph_parent(context, side, expected_frame, epoch, row)
}
/// RevalidateGraphParent: the same parent, read again now, in the same frame and capture epoch.
pub fn revalidate_graph_parent(expected: &GraphParentRecord) -> bool {
    if expected.status != 0 || expected.view_index > 1 || expected.context == 0 || expected.manager == 0 || expected.fingerprint == 0 {
        return false;
    }
    let Some((frame, epoch)) = frame_epoch() else { return false };
    let mut fresh = GraphParentRecord::default();
    if !current_graph_parent(expected.view_index as i32, frame, &mut fresh) || fresh != *expected {
        return false;
    }
    frame_epoch().is_some_and(|(f, e)| f == frame && e == epoch)
}
/// PublishGraphParentSource as its callers run it: a failure leaves no source.
pub fn publish_graph_parent(nodes: &[crate::native::graph_snapshot::Node], edges: &[crate::native::graph_snapshot::Edge], failure: u32, base: u64) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| publish_graph_parent_source(nodes, edges, failure, base))).is_err() {
        parent_reset();
    }
}
/// # Safety
/// The arrays hold their counts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_publish_parent(nodes: *const crate::native::graph_snapshot::Node, node_count: usize, edges: *const crate::native::graph_snapshot::Edge, edge_count: usize, failure: u32, base: u64) {
    // SAFETY: as documented.
    let nodes = if nodes.is_null() || node_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(nodes, node_count) } };
    // SAFETY: as documented.
    let edges = if edges.is_null() || edge_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(edges, edge_count) } };
    publish_graph_parent(nodes, edges, failure, base);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_parent_code_guards(base: u64) {
    prepare_graph_parent_code_guards(base);
}
/// # Safety
/// `row` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_describe_parent(context: u64, side: i32, frame: u32, epoch: u64, row: *mut GraphParentRecord) -> bool {
    describe_graph_parent(context, side, frame, epoch, unsafe { &mut *row })
}
/// # Safety
/// `row` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_current_parent(side: i32, frame: u32, row: *mut GraphParentRecord) -> bool {
    current_graph_parent(side, frame, unsafe { &mut *row })
}
/// # Safety
/// `expected` is readable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_revalidate_parent(expected: *const GraphParentRecord) -> bool {
    revalidate_graph_parent(unsafe { &*expected })
}
/// GraphContextScope: the thread's graph context set to `context`, the previous one returned.
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_graph_context_swap(context: u64) -> u64 {
    GRAPH_CONTEXT.with(|c| c.replace(context))
}

// ---- the graph job and scene update polls (formerly the .cpp's SetGraphJobPoll, SetSceneUpdatePoll, PollGraphJobs)

static GRAPH_JOB_POLL: core::sync::atomic::AtomicPtr<c_void> = core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
static SCENE_UPDATE_POLL: core::sync::atomic::AtomicPtr<c_void> = core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
/// SetGraphJobPoll: the paired frame's owned graph job poll.
pub fn set_graph_job_poll(callback: extern "C" fn()) {
    GRAPH_JOB_POLL.store(callback as *const () as *mut c_void, Release);
}
/// PollGraphJobs: the graph job poll, then the scene update poll, each when registered.
pub fn poll_graph_jobs() {
    for slot in [&GRAPH_JOB_POLL, &SCENE_UPDATE_POLL] {
        let callback = slot.load(Acquire);
        if !callback.is_null() {
            // SAFETY: a registered callback (Rust's or C++'s, which may throw back to its C++ caller).
            let callback: extern "C-unwind" fn() = unsafe { core::mem::transmute(callback) };
            callback();
        }
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_set_graph_job_poll(callback: *mut c_void) {
    GRAPH_JOB_POLL.store(callback, Release);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_set_scene_update_poll(callback: *mut c_void) {
    SCENE_UPDATE_POLL.store(callback, Release);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_poll_graph_jobs() {
    poll_graph_jobs();
}

// ---- the C ABI

/// # Safety
/// `exports` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_attach(exports: *const Exports) {
    EXPORTS.store(exports.cast_mut(), Release);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_frame_boundary() {
    frame_boundary();
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_begin_stage(context: u64, buckets: u64, output: u64, flags0: u32, flags1: u32, side: i32, out: *mut Ticket) {
    unsafe { *out = begin_stage(context, buckets, output, flags0, flags1, side) };
}
/// # Safety
/// `ticket` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_end_stage(ticket: *const Ticket) {
    end_stage(unsafe { &*ticket });
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_output(k: u32, out: u64) {
    output(k, out);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_consume(node: u32, side: i32, context: u64, span: u64) {
    consume(node, side, context, span);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_graph(manager: u64, after: bool) {
    graph(manager, after);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_cull(job: u64, out: u64, side: i32, after: bool, definition: u64) {
    cull(job, out, side, after, definition);
}
/// 0 materialize, 1 proxy gather.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_begin_task(which: u32, context: u64, range: u64, side: i32, out: *mut MaterializeTicket) {
    unsafe { *out = if which == 0 { begin_materialize(context, side) } else { begin_proxy_gather(context, range, side) } };
}
/// # Safety
/// `ticket` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_end_task(which: u32, ticket: *const MaterializeTicket) {
    end_materialize(unsafe { &*ticket }, if which == 0 { kind::MATERIALIZE_END } else { kind::PROXY_GATHER_END });
}
/// # Safety
/// As prepare_pair.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_prep_prepare_pair(original: Option<PrepareGraph>, manager: *mut c_void, arguments: *mut c_void, requested: *mut c_void, eye_name: u64, main_view: u64, gameplay: bool, vrcam_view: u64) -> *mut c_void {
    unsafe { prepare_pair(original, manager, arguments, requested, eye_name, main_view, gameplay, vrcam_view) }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_suppress_rtt(frame_data: u64, eye_name: u64, gameplay: bool) -> bool {
    suppress_rtt(frame_data, eye_name, gameplay)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_root_completed(frame_data: u64, generation_before: u64) {
    root_completed(frame_data, generation_before);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_eye_copy(list: u64, source: u64, destination: u64) {
    eye_copy_recorded(list, source, destination);
}
/// # Safety
/// `value` and `previous` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_copy_source(value: *const CopySource, previous: *mut CopySource) {
    unsafe { *previous = set_copy_source(*value) };
}
/// 0 enter, 1 leave.
///
/// # Safety
/// `scope` is the .cpp's live DrawScope.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_draw_scope(which: u32, scope: *mut DrawScope, node: u32, side: i32, arguments: u64) {
    unsafe {
        if which == 0 {
            draw_scope_enter(scope, node, side, arguments)
        } else {
            draw_scope_leave(scope)
        }
    }
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_draw_tag(out: *mut DrawTag) {
    unsafe { *out = current_draw_tag() };
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_draw_recorded(list: u64) {
    draw_recorded(list);
}
/// # Safety
/// `lists` holds `count` list pointers.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_submit_lists(queue: u64, count: u32, lists: *const u64, serialized: bool) {
    if lists.is_null() || count == 0 {
        return;
    }
    submit_lists(queue, unsafe { core::slice::from_raw_parts(lists, count as usize) }, serialized);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_reset_list(list: u64) {
    reset_list(list);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_prep_pair_compatible() -> bool {
    pair_prepare_compatible()
}
/// # Safety
/// The outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_frame_epoch(frame: *mut u32, epoch: *mut u64) -> bool {
    let value = frame_epoch();
    let (f, e) = value.unwrap_or((0, 0));
    unsafe { (*frame, *epoch) = (f, e) };
    value.is_some()
}
/// CurrentGraphParent's scope into `context` (kept when no capturing scope), `scope`, `stage`, `epoch`.
///
/// # Safety
/// The outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_prep_graph_scope(expected_frame: u32, context: *mut u64, scope: *mut u64, stage: *mut u32, epoch: *mut u64) -> bool {
    let Some((source, e)) = graph_scope(expected_frame) else { return false };
    unsafe {
        *epoch = e;
        if let Some((c, sc, st)) = source {
            (*context, *scope, *stage) = (c, sc, st);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    //! The trace over fabricated memory: a request's frames, a stage and its end with the output's packet array, a
    //! consumed span, a draw scope's lists, a submission, the record capacity and the pair diagnostics' refusals.
    use super::*;

    #[test]
    fn trace_stages_draws_and_pairs() {
        use crate::native_state_exports as n;
        static EXPORTS_OF_CRATE: OnceLock<Exports> = OnceLock::new();
        let exports = EXPORTS_OF_CRATE.get_or_init(|| Exports {
            CyberpunkVR_PreparationRequest: core::ptr::addr_of!(n::CyberpunkVR_PreparationRequest).cast_mut().cast(),
            CyberpunkVR_PreparationState: core::ptr::addr_of!(n::CyberpunkVR_PreparationState).cast_mut().cast(),
            CyberpunkVR_PreparationSeq: core::ptr::addr_of!(n::CyberpunkVR_PreparationSeq).cast_mut().cast(),
            CyberpunkVR_PreparationCount: core::ptr::addr_of!(n::CyberpunkVR_PreparationCount).cast_mut().cast(),
            CyberpunkVR_PreparationPacketCount: core::ptr::addr_of!(n::CyberpunkVR_PreparationPacketCount).cast_mut().cast(),
            CyberpunkVR_PreparationFailure: core::ptr::addr_of!(n::CyberpunkVR_PreparationFailure).cast_mut().cast(),
            CyberpunkVR_PreparationRecords: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationRecords).cast(),
            CyberpunkVR_PreparationPackets: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationPackets).cast(),
            CyberpunkVR_PreparationPairRequest: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairRequest).cast_mut().cast(),
            CyberpunkVR_PreparationPairState: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairState).cast_mut().cast(),
            CyberpunkVR_PreparationPairCompatible: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairCompatible).cast_mut().cast(),
            CyberpunkVR_PreparationPairData: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationPairData).cast(),
            CyberpunkVR_PreparationPairEligibility: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationPairEligibility).cast(),
            CyberpunkVR_PreparationPairRoot: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationPairRoot).cast(),
            CyberpunkVR_PreparationCameraTrace: core::ptr::addr_of!(n::CyberpunkVR_PreparationCameraTrace).cast_mut().cast(),
            CyberpunkVR_PreparationAllStages: core::ptr::addr_of!(n::CyberpunkVR_PreparationAllStages).cast_mut().cast(),
            CyberpunkVR_PreparationGraphParents: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationGraphParents).cast(),
            CyberpunkVR_PreparationGraphParentCount: core::ptr::addr_of_mut!(n::CyberpunkVR_PreparationGraphParentCount).cast(),
            CyberpunkVR_PreparationPairVerified: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairVerified).cast_mut().cast(),
            CyberpunkVR_PreparationRootSkipped: core::ptr::addr_of!(n::CyberpunkVR_PreparationRootSkipped).cast_mut().cast(),
            CyberpunkVR_PreparationRootCompatible: core::ptr::addr_of!(n::CyberpunkVR_PreparationRootCompatible).cast_mut().cast(),
            CyberpunkVR_PreparationPairMergeLimit: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairMergeLimit).cast_mut().cast(),
            CyberpunkVR_PreparationPairMerged: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairMerged).cast_mut().cast(),
            CyberpunkVR_PreparationPairCompleted: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairCompleted).cast_mut().cast(),
            CyberpunkVR_PreparationPairGeneration: core::ptr::addr_of!(n::CyberpunkVR_PreparationPairGeneration).cast_mut().cast(),
            CyberpunkVR_PreparationRunRequest: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunRequest).cast_mut().cast(),
            CyberpunkVR_PreparationRunState: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunState).cast_mut().cast(),
            CyberpunkVR_PreparationRunStop: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunStop).cast_mut().cast(),
            CyberpunkVR_PreparationRunFailure: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunFailure).cast_mut().cast(),
            CyberpunkVR_PreparationRunFrames: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunFrames).cast_mut().cast(),
            CyberpunkVR_PreparationRunDepth: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunDepth).cast_mut().cast(),
            CyberpunkVR_PreparationRunEyeCopies: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunEyeCopies).cast_mut().cast(),
            CyberpunkVR_PreparationRunEyeSubmissions: core::ptr::addr_of!(n::CyberpunkVR_PreparationRunEyeSubmissions).cast_mut().cast(),
        });
        unsafe { cvr_prep_attach(exports) };
        assert!(!active());
        x!(CyberpunkVR_PreparationRequest).store(2, SeqCst);
        frame_boundary();
        assert!(active() && frame_epoch().map(|f| f.0) == Some(1));
        // A context with its view and storage; the stage arguments naming it, stage 3.
        let mut view = vec![0u8; 0x2C48];
        view[0x2C40..0x2C48].copy_from_slice(&0x5151u64.to_le_bytes());
        let mut context = [0u8; 0x20];
        context[0x18..0x20].copy_from_slice(&(view.as_ptr() as u64).to_le_bytes());
        let mut args = [0u8; 32];
        args[0..8].copy_from_slice(&(context.as_ptr() as u64).to_le_bytes());
        args[0x10..0x14].copy_from_slice(&3u32.to_le_bytes());
        args[0x18] = 7;
        // The stage's output: an array header naming two packets.
        let packets = [Packet { first: 1, second: 2 }, Packet { first: 3, second: 4 }];
        let mut header = [0u8; 16];
        header[0..8].copy_from_slice(&(packets.as_ptr() as u64).to_le_bytes());
        header[8..12].copy_from_slice(&4u32.to_le_bytes());
        header[12..16].copy_from_slice(&2u32.to_le_bytes());
        let ticket = begin_stage(args.as_ptr() as u64, 0x99, header.as_ptr() as u64, 1, 2, 0);
        assert!(ticket.scope == 1 && ticket.previous.scope == 0);
        end_stage(&ticket);
        let rows = { lock().records.clone() };
        assert_eq!(rows.len(), 2);
        assert!(rows[0].kind == kind::STAGE_BEGIN && rows[0].storage == 0x5151 && rows[0].stage == 3 && rows[0].mode == 7);
        assert!(rows[1].kind == kind::STAGE_END && rows[1].count == 2 && rows[1].offset == 0, "the output's packets");
        let span = [packets.as_ptr() as u64, packets.as_ptr() as u64 + 32];
        consume(0x33DDA4, 1, args.as_ptr() as u64, span.as_ptr() as u64);
        consume(0x1234, 1, args.as_ptr() as u64, span.as_ptr() as u64);
        let s = lock();
        assert!(s.records.len() == 3 && s.records[2].count == 2 && s.records[2].offset == 2 && s.data[3].second == 4, "another node is not traced");
        drop(s);
        let mut scope = DrawScope { previous: core::ptr::null_mut(), source: Record::default(), capture_epoch: 0, lists: [0; 64], draws: [0; 64], count: 0, valid: false };
        unsafe { draw_scope_enter(&mut scope, 0x33DDA4, 0, args.as_ptr() as u64) };
        assert!(scope.valid && current_draw_tag().stage == 3);
        draw_recorded(0x4000);
        draw_recorded(0x4000);
        draw_recorded(0x5000);
        assert_eq!((scope.count, scope.draws[0]), (2, 2));
        unsafe { draw_scope_leave(&mut scope) };
        assert_eq!(current_draw_tag(), NO_TAG, "the scope popped");
        submit_lists(0x77, &[0x4000, 0x5000], true);
        let kinds: Vec<u32> = lock().records.iter().map(|r| r.kind).collect();
        assert_eq!(kinds[3..], [kind::DRAW_BEGIN, kind::DRAW_BEGIN, kind::DRAW_END, kind::DRAW_END, kind::SUBMIT_LIST, kind::SUBMIT_LIST]);
        assert!(!suppress_rtt(0, 0x31, true), "no pair asked for");
        unsafe extern "C" fn plain(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> *mut c_void {
            0x42 as *mut c_void
        }
        assert_eq!(unsafe { prepare_pair(Some(plain), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), 0x31, 0x1000, true, 0) } as u64, 0x42, "no pair request: the original");
        frame_boundary();
        frame_boundary();
        assert!(!active() && x!(CyberpunkVR_PreparationState).load(SeqCst) == 2, "two frames");
        for _ in 0..3 {
            reset_list(1);
        }
        assert_eq!(lock().records.len(), 9, "nothing recorded once the trace ended");
    }
}
