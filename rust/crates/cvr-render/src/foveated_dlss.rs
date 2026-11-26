//! FOVEATED DLSS'S NGX HOOKS (formerly src/Render/FoveatedDlss.cpp): the game's DLSS super-resolution evaluate, while
//! foveation is on, becomes two private evaluates -- the centre at the game's quality, the periphery at a lower input
//! size through DLAA -- composited into the game's output (cvr_gpu::foveated_gpu); the settings and the centre region
//! are crate::foveated's. Everything else (off, a menu, inputs it cannot take, a failure) is the game's own evaluate.
//! The status, the GPU timings sampled one call in thirty, and a JSON report every 500 ms for the tools
//! (CyberpunkVR_FoveatedReport, written as jsoncpp wrote it).
use cvr_host::fast::FastMap;
use core::cell::{Cell, UnsafeCell};
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use cvr_gpu::foveated_gpu::{Gpu, Region as GpuRegion, Source};
use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D12::*;

use crate::foveated::{self, Region, Settings};
use crate::framegen_inputs::NodePose;

extern "C" {
    static mut CyberpunkVR_FoveatedReport: [u8; 32768];
    static CyberpunkVR_FoveatedReportSeq: AtomicU32;
    fn GetMenuMode() -> i32;
    /// src/Framegen/Inputs.cpp: the node pose's fields (cvr::camera::CurrentNodePoseIdentity).
    fn cvr_cpp_framegen_node_pose(out: *mut NodePose);
}
#[allow(non_snake_case)]
extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
}
const MH_OK: i32 = 0;

/// What the C++ half provides.
#[repr(C)]
pub struct Runtime {
    /// cvr::sharpen::gameUpscaler.ObserveGameQuality (an inline variable of Render/SharpenPolicy.hpp).
    observe_game_quality: extern "C" fn(view: u32, quality: u32),
}
unsafe extern "C" {
    /// cvr::sharpen::gameUpscaler.ObserveGameQuality (src/Render/FoveatedDlss.cpp).
    safe fn cvr_cpp_observe_game_quality(view: u32, quality: u32);
}
/// The table, built here (formerly src/Render/FoveatedDlss.cpp's).
static RT: Runtime = Runtime { observe_game_quality: cvr_cpp_observe_game_quality };
static RUNTIME: core::sync::atomic::AtomicPtr<Runtime> =
    core::sync::atomic::AtomicPtr::new((&raw const RT).cast_mut());
#[no_mangle]
pub extern "C" fn cvr_foveated_dlss_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}

fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}

// ---- NGX's parameter object (FoveatedNgx.hpp) ---------------------------------------------------------------------

type Result = u32;
fn success(r: Result) -> bool {
    r & 0xFFF0_0000 != 0xBAD0_0000
}
/// NVSDK_NGX_Parameter's virtual table as MSVC lays it out: each overload set in reverse order of declaration.
#[repr(C)]
struct ParamsVtbl {
    set_ptr: unsafe extern "system" fn(*mut Parameters, *const c_char, *mut c_void),
    set_d3d12: unsafe extern "system" fn(*mut Parameters, *const c_char, *mut c_void),
    set_d3d11: unsafe extern "system" fn(*mut Parameters, *const c_char, *mut c_void),
    set_i32: unsafe extern "system" fn(*mut Parameters, *const c_char, i32),
    set_u32: unsafe extern "system" fn(*mut Parameters, *const c_char, u32),
    set_f64: unsafe extern "system" fn(*mut Parameters, *const c_char, f64),
    set_f32: unsafe extern "system" fn(*mut Parameters, *const c_char, f32),
    set_u64: unsafe extern "system" fn(*mut Parameters, *const c_char, u64),
    get_ptr:
        unsafe extern "system" fn(*const Parameters, *const c_char, *mut *mut c_void) -> Result,
    get_d3d12:
        unsafe extern "system" fn(*const Parameters, *const c_char, *mut *mut c_void) -> Result,
    get_d3d11:
        unsafe extern "system" fn(*const Parameters, *const c_char, *mut *mut c_void) -> Result,
    get_i32: unsafe extern "system" fn(*const Parameters, *const c_char, *mut i32) -> Result,
    get_u32: unsafe extern "system" fn(*const Parameters, *const c_char, *mut u32) -> Result,
    get_f64: unsafe extern "system" fn(*const Parameters, *const c_char, *mut f64) -> Result,
    get_f32: unsafe extern "system" fn(*const Parameters, *const c_char, *mut f32) -> Result,
    get_u64: unsafe extern "system" fn(*const Parameters, *const c_char, *mut u64) -> Result,
    reset: unsafe extern "system" fn(*mut Parameters),
}
#[repr(C)]
pub struct Parameters {
    vtbl: *const ParamsVtbl,
}
fn vt(p: *const Parameters) -> &'static ParamsVtbl {
    // SAFETY: the game's live parameter object.
    unsafe { &*(*p).vtbl }
}
/// ngx::Read<ID3D12Resource*>.
unsafe fn read_resource(p: *const Parameters, name: &CStr) -> *mut c_void {
    let mut v = core::ptr::null_mut();
    // SAFETY: the game's object.
    if !p.is_null() && success(unsafe { (vt(p).get_d3d12)(p, name.as_ptr(), &mut v) }) {
        v
    } else {
        core::ptr::null_mut()
    }
}
/// ngx::Read<float>.
unsafe fn read_f32(p: *const Parameters, name: &CStr, fallback: f32) -> f32 {
    let mut v = 0.0f32;
    // SAFETY: the game's object.
    if !p.is_null() && success(unsafe { (vt(p).get_f32)(p, name.as_ptr(), &mut v) }) {
        v
    } else {
        fallback
    }
}
/// ngx::Integer: unsigned, else signed, else the fallback.
unsafe fn integer(p: *const Parameters, name: &CStr, fallback: u32) -> u32 {
    if p.is_null() {
        return fallback;
    }
    let mut v = 0u32;
    // SAFETY: the game's object.
    if success(unsafe { (vt(p).get_u32)(p, name.as_ptr(), &mut v) }) {
        return v;
    }
    let mut s = 0i32;
    // SAFETY: as above.
    if success(unsafe { (vt(p).get_i32)(p, name.as_ptr(), &mut s) }) {
        s as u32
    } else {
        fallback
    }
}

#[derive(Clone, Copy)]
enum Value {
    U(u32),
    F(f32),
    R(*mut c_void),
}
/// ngx::Override: every touched parameter restored (in reverse) when it drops, even when a private create or evaluate
/// fails.
struct Override {
    p: *mut Parameters,
    entries: Vec<(&'static CStr, Value)>,
}
impl Override {
    fn new(p: *mut Parameters) -> Self {
        Override {
            p,
            entries: Vec::with_capacity(32),
        }
    }
    fn remember(&mut self, name: &'static CStr, now: impl FnOnce() -> Value) {
        if !self.entries.iter().any(|(n, _)| *n == name) {
            self.entries.push((name, now()));
        }
    }
    unsafe fn set_u32(&mut self, name: &'static CStr, value: u32) {
        let p = self.p;
        // SAFETY: the game's object.
        unsafe {
            self.remember(name, || Value::U(integer(p, name, 0)));
            (vt(p).set_u32)(p, name.as_ptr(), value);
        }
    }
    unsafe fn set_f32(&mut self, name: &'static CStr, value: f32) {
        let p = self.p;
        // SAFETY: the game's object.
        unsafe {
            self.remember(name, || Value::F(read_f32(p, name, 0.0)));
            (vt(p).set_f32)(p, name.as_ptr(), value);
        }
    }
    unsafe fn set_resource(&mut self, name: &'static CStr, value: *mut c_void) {
        let p = self.p;
        // SAFETY: the game's object.
        unsafe {
            self.remember(name, || Value::R(read_resource(p, name)));
            (vt(p).set_d3d12)(p, name.as_ptr(), value);
        }
    }
}
impl Drop for Override {
    fn drop(&mut self) {
        let p = self.p;
        for (name, value) in self.entries.iter().rev() {
            // SAFETY: the game's object, alive across the call that touched it.
            unsafe {
                match *value {
                    Value::U(v) => (vt(p).set_u32)(p, name.as_ptr(), v),
                    Value::F(v) => (vt(p).set_f32)(p, name.as_ptr(), v),
                    Value::R(v) => (vt(p).set_d3d12)(p, name.as_ptr(), v),
                }
            }
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct FeatureConfig {
    output_w: u32,
    output_h: u32,
    flags: u32,
    quality: u32,
    presets: [u32; 6],
}
impl FeatureConfig {
    fn matches(&self, o: &FeatureConfig) -> bool {
        self.output_w == o.output_w
            && self.output_h == o.output_h
            && self.flags == o.flags
            && self.quality == o.quality
    }
}
/// ngx::CreationRegistry: NGX dispatch can substitute a handle or parameter object between Create and Evaluate --
/// exact identities first, then only unambiguous configurations.
#[derive(Default)]
struct Registry {
    entries: Vec<(usize, usize, u32, FeatureConfig)>,
}
impl Registry {
    fn remember(&mut self, handle: usize, parameters: usize, view: u32, config: FeatureConfig) {
        if self.entries.len() >= 128 {
            self.entries.remove(0);
        }
        self.entries.push((handle, parameters, view, config));
    }
    fn find(
        &self,
        handle: usize,
        parameters: usize,
        view: u32,
        config: &mut FeatureConfig,
    ) -> bool {
        let mut best: Option<&(usize, usize, u32, FeatureConfig)> = None;
        let mut best_rank = 0u32;
        let mut ambiguous = false;
        for e in self.entries.iter().rev() {
            if !e.3.matches(config) {
                continue;
            }
            let rank = if e.0 == handle {
                3
            } else if e.1 == parameters {
                2
            } else if view != 0 && e.2 == view {
                1
            } else {
                0
            };
            match best {
                Some(b) if rank <= best_rank => {
                    if rank == 0 && best_rank == 0 && e.3.presets != b.3.presets {
                        ambiguous = true;
                    }
                }
                _ => {
                    best = Some(e);
                    best_rank = rank;
                    ambiguous = false;
                }
            }
        }
        match best {
            Some(b) if !ambiguous => {
                *config = b.3;
                true
            }
            _ => false,
        }
    }
}

type CreateFn = unsafe extern "C" fn(*mut c_void, u32, *mut Parameters, *mut *mut c_void) -> Result;
type EvaluateFn =
    unsafe extern "C" fn(*mut c_void, *const c_void, *const Parameters, *mut c_void) -> Result;
type ReleaseFn = unsafe extern "C" fn(*mut c_void) -> Result;

const PRESET_KEYS: [&CStr; 6] = [
    c"DLSS.Hint.Render.Preset.DLAA",
    c"DLSS.Hint.Render.Preset.Quality",
    c"DLSS.Hint.Render.Preset.Balanced",
    c"DLSS.Hint.Render.Preset.Performance",
    c"DLSS.Hint.Render.Preset.UltraPerformance",
    c"DLSS.Hint.Render.Preset.UltraQuality",
];

// ---- the objects a command list keeps alive (Owned) ----------------------------------------------------------------

/// GPU timestamps around a call: read back when the last reference goes (the list has run).
struct Timed {
    device: ID3D12Device,
    queries: Option<ID3D12QueryHeap>,
    readback: Option<ID3D12Resource>,
    frequency: AtomicU64,
    count: AtomicU32,
    native: bool,
}
impl Drop for Timed {
    fn drop(&mut self) {
        let f = self.frequency.load(SeqCst);
        let count = self.count.load(SeqCst);
        // SAFETY: our own device and readback buffer.
        unsafe {
            if f == 0 || count == 0 || self.device.GetDeviceRemovedReason().is_err() {
                return;
            }
            let Some(readback) = self.readback.as_ref() else {
                return;
            };
            let mut data = core::ptr::null_mut();
            let range = D3D12_RANGE {
                Begin: 0,
                End: count as usize * 8,
            };
            if readback.Map(0, Some(&range), Some(&mut data)).is_err() || data.is_null() {
                return;
            }
            let ticks = core::slice::from_raw_parts(data as *const u64, count as usize);
            let mut valid = true;
            for i in 1..count as usize {
                valid &= ticks[i] >= ticks[i - 1] && ticks[i] - ticks[i - 1] < f;
            }
            if valid {
                let ms =
                    |a: usize, b: usize| ((ticks[b] - ticks[a]) as f64 * 1000.0 / f as f64) as f32;
                let mut s = stats();
                if self.native {
                    s.native_ms = ms(0, 1);
                } else {
                    s.peripheral_ms = ms(0, 1);
                    s.foveated_ms = ms(1, 2);
                    s.total_ms = ms(0, 3);
                }
            }
            let none = D3D12_RANGE::default();
            readback.Unmap(0, Some(&none));
        }
    }
}

/// One view's foveation: its GPU passes, its two private NGX features and what they were made for.
struct State {
    gpu: Gpu,
    center: Prepared,
    periphery: Prepared,
    handles: [*mut c_void; 2],
    release: Option<ReleaseFn>,
    original: usize,
    render_w: u32,
    render_h: u32,
    output_w: u32,
    output_h: u32,
    flags: u32,
    quality: u32,
    presets: [u32; 6],
    settings: Settings,
    evaluations: u64,
    resets: u64,
    first: bool,
}
#[derive(Default)]
struct Prepared {
    region: GpuRegion,
    color: Option<ID3D12Resource>,
    depth: Option<ID3D12Resource>,
    motion: Option<ID3D12Resource>,
    output: Option<ID3D12Resource>,
}
impl Drop for State {
    fn drop(&mut self) {
        // Destruction can occur under the command-resource registry's lock: the NGX releases wait for the next Tick
        // (Present) so they do not re-enter D3D from that lock.
        let mut retired = RETIRED.lock().unwrap_or_else(|p| p.into_inner());
        for &handle in &self.handles {
            if let (false, Some(release)) = (handle.is_null(), self.release) {
                retired.push((release as usize, handle as usize));
            }
        }
    }
}

enum Payload {
    Timed(Timed),
    /// Touched only under CORE's lock (and by the last release).
    State(UnsafeCell<State>),
}
/// An IUnknown of ours (Owned), so the command-resource registry can keep it until the GPU is done.
#[repr(C)]
struct Owned {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    payload: Payload,
}
unsafe extern "system" fn owned_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: COM's contract.
    unsafe {
        if out.is_null() {
            return windows::Win32::Foundation::E_POINTER;
        }
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IUnknown::IID {
            return windows::Win32::Foundation::E_NOINTERFACE;
        }
        owned_add_ref(this);
        *out = this;
        HRESULT(0)
    }
}
unsafe extern "system" fn owned_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: one of ours.
    unsafe { (*(this as *const Owned)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn owned_release(this: *mut c_void) -> u32 {
    // SAFETY: one of ours; freed with the last reference.
    unsafe {
        let n = (*(this as *const Owned)).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Owned));
        }
        n
    }
}
static OWNED_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: owned_query,
    AddRef: owned_add_ref,
    Release: owned_release,
};

/// A reference to one of ours.
struct Ref(core::ptr::NonNull<Owned>);
// SAFETY: the payloads are free-threaded D3D12 objects, atomics, or touched under CORE's lock.
unsafe impl Send for Ref {}
impl Ref {
    fn new(payload: Payload) -> Ref {
        let b = Box::new(Owned {
            vtbl: &OWNED_VTBL,
            refs: AtomicU32::new(1),
            payload,
        });
        Ref(core::ptr::NonNull::from(Box::leak(b)))
    }
    fn raw(&self) -> *mut c_void {
        self.0.as_ptr() as *mut c_void
    }
    fn timed(&self) -> &Timed {
        // SAFETY: alive while referenced.
        match unsafe { &self.0.as_ref().payload } {
            Payload::Timed(t) => t,
            Payload::State(_) => unreachable!(),
        }
    }
    /// # Safety
    /// Under CORE's lock, no other reference to the state alive.
    #[allow(clippy::mut_from_ref)]
    unsafe fn state(&self) -> &mut State {
        // SAFETY: the caller's guarantee.
        match unsafe { &self.0.as_ref().payload } {
            Payload::State(s) => unsafe { &mut *s.get() },
            Payload::Timed(_) => unreachable!(),
        }
    }
}
impl Clone for Ref {
    fn clone(&self) -> Ref {
        // SAFETY: one of ours.
        unsafe { owned_add_ref(self.raw()) };
        Ref(self.0)
    }
}
impl Drop for Ref {
    fn drop(&mut self) {
        // SAFETY: our reference.
        unsafe { owned_release(self.raw()) };
    }
}
fn keep(list: &ID3D12GraphicsCommandList, r: &Ref) -> bool {
    cvr_gpu::command_resources::keep(list, &[r.raw()])
}

// ---- the state ------------------------------------------------------------------------------------------------------

/// cvr::foveated::Status.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Status {
    loaded: bool,
    ready: bool,
    active: bool,
    pub(crate) evaluations: u64,
    processed: u64,
    pub(crate) native_ms: f32,
    foveated_ms: f32,
    peripheral_ms: f32,
    pub(crate) total_ms: f32,
    views: u32,
    pub(crate) text: [u8; 192],
}
const _: () =
    assert!(core::mem::size_of::<Status>() == 240 && core::mem::offset_of!(Status, text) == 44);
const fn status_text(s: &str) -> [u8; 192] {
    let b = s.as_bytes();
    let mut t = [0u8; 192];
    let mut i = 0;
    while i < b.len() {
        t[i] = b[i];
        i += 1;
    }
    t
}
static STATS: Mutex<Status> = Mutex::new(Status {
    loaded: false,
    ready: false,
    active: false,
    evaluations: 0,
    processed: 0,
    native_ms: 0.0,
    foveated_ms: 0.0,
    peripheral_ms: 0.0,
    total_ms: 0.0,
    views: 0,
    text: status_text("Off"),
});
fn stats() -> MutexGuard<'static, Status> {
    STATS.lock().unwrap_or_else(|p| p.into_inner())
}
impl Status {
    fn set_text(&mut self, message: &str) {
        // strncpy_s with _TRUNCATE.
        let b = &message.as_bytes()[..message.len().min(191)];
        self.text = [0; 192];
        self.text[..b.len()].copy_from_slice(b);
    }
    fn text(&self) -> &str {
        let n = self.text.iter().position(|&c| c == 0).unwrap_or(192);
        core::str::from_utf8(&self.text[..n]).unwrap_or("")
    }
}
fn message(text: &str) {
    stats().set_text(text);
}

struct Core {
    game_features: Registry,
    views: [Option<Ref>; 2],
    native_reset: [bool; 2],
    view_calls: [u64; 2],
}
static CORE: Mutex<Core> = Mutex::new(Core {
    game_features: Registry {
        entries: Vec::new(),
    },
    views: [None, None],
    native_reset: [false; 2],
    view_calls: [0; 2],
});
fn core() -> MutexGuard<'static, Core> {
    CORE.lock().unwrap_or_else(|p| p.into_inner())
}
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// The two evaluate exports' routes: [evaluate trampoline, create, release, target].
static ROUTES: [[AtomicUsize; 4]; 2] = [
    [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ],
    [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ],
];
static ORIGINAL_CREATE: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);
static ACTIVE: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU64 = AtomicU64::new(0);
static NEXT_INSTALL: AtomicU64 = AtomicU64::new(0);
static LAST_CALL: AtomicU64 = AtomicU64::new(0);
static TIMINGS: Mutex<Option<FastMap<usize, Vec<Ref>>>> = Mutex::new(None);
static RETIRED: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
thread_local! {
    /// The port's own NGX calls pass straight through.
    static INTERNAL: Cell<bool> = const { Cell::new(false) };
}
struct Recursion(bool);
impl Recursion {
    fn new() -> Self {
        Recursion(INTERNAL.with(|i| i.replace(true)))
    }
}
impl Drop for Recursion {
    fn drop(&mut self) {
        INTERNAL.with(|i| i.set(self.0));
    }
}

fn node_pose() -> NodePose {
    let mut p = NodePose::default();
    // SAFETY: the C++ adapter writes the struct.
    unsafe { cvr_cpp_framegen_node_pose(&mut p) };
    p
}

// ---- the timings --------------------------------------------------------------------------------------------------

/// One call in thirty gets timestamps; timing must not become a per-frame allocator load.
unsafe fn timer(list: &ID3D12GraphicsCommandList, native: bool) -> Option<Ref> {
    if CALLS.load(SeqCst) % 30 != 0 || (native && ACTIVE.load(SeqCst) == 0) {
        return None;
    }
    // SAFETY: the recording list and the objects made here.
    unsafe {
        let mut device: Option<ID3D12Device> = None;
        list.GetDevice(&mut device).ok()?;
        let device = device?;
        let query = D3D12_QUERY_HEAP_DESC {
            Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
            Count: 4,
            NodeMask: 0,
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 32,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let mut queries: Option<ID3D12QueryHeap> = None;
        let mut readback: Option<ID3D12Resource> = None;
        let made = device.CreateQueryHeap(&query, &mut queries).is_ok()
            && device
                .CreateCommittedResource(
                    &heap,
                    D3D12_HEAP_FLAG_NONE,
                    &desc,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    None,
                    &mut readback,
                )
                .is_ok();
        let t = Ref::new(Payload::Timed(Timed {
            device,
            queries,
            readback,
            frequency: AtomicU64::new(0),
            count: AtomicU32::new(0),
            native,
        }));
        if !made || !keep(list, &t) {
            return None;
        }
        list.EndQuery(t.timed().queries.as_ref()?, D3D12_QUERY_TYPE_TIMESTAMP, 0);
        Some(t)
    }
}
unsafe fn stamp(list: &ID3D12GraphicsCommandList, t: &Option<Ref>, index: u32) {
    if let Some(q) = t.as_ref().and_then(|t| t.timed().queries.as_ref()) {
        // SAFETY: the recording list.
        unsafe { list.EndQuery(q, D3D12_QUERY_TYPE_TIMESTAMP, index) };
    }
}
unsafe fn finish_timer(list: &ID3D12GraphicsCommandList, t: Option<Ref>, count: u32) {
    let Some(t) = t else { return };
    let timed = t.timed();
    timed.count.store(count, SeqCst);
    if let (Some(q), Some(r)) = (timed.queries.as_ref(), timed.readback.as_ref()) {
        // SAFETY: the recording list.
        unsafe { list.ResolveQueryData(q, D3D12_QUERY_TYPE_TIMESTAMP, 0, count, r, 0) };
    }
    let mut timings = TIMINGS.lock().unwrap_or_else(|p| p.into_inner());
    timings
        .get_or_insert_with(cvr_host::fast::fast_map)
        .entry(list.as_raw() as usize)
        .or_default()
        .push(t);
}

// ---- the evaluate ---------------------------------------------------------------------------------------------------

fn texture_bounds(resource: *mut c_void, x: u32, y: u32, w: u32, h: u32) -> bool {
    // SAFETY: the game's resource or null.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return false;
    };
    if w == 0 || h == 0 {
        return false;
    }
    // SAFETY: as above.
    let d = unsafe { r.GetDesc() };
    d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && d.DepthOrArraySize == 1
        && d.MipLevels == 1
        && d.SampleDesc.Count == 1
        && u64::from(x) <= d.Width
        && u64::from(w) <= d.Width - u64::from(x)
        && y <= d.Height
        && h <= d.Height - y
}
unsafe fn read_source(
    p: *const Parameters,
    s: &mut Source,
    flags: &mut u32,
    quality: &mut u32,
) -> bool {
    // SAFETY: the game's parameter object.
    unsafe {
        s.color = read_resource(p, c"Color");
        s.depth = read_resource(p, c"Depth");
        s.motion = read_resource(p, c"MotionVectors");
        s.output = read_resource(p, c"Output");
        s.render_w = integer(
            p,
            c"DLSS.Render.Subrect.Dimensions.Width",
            integer(p, c"Width", 0),
        );
        s.render_h = integer(
            p,
            c"DLSS.Render.Subrect.Dimensions.Height",
            integer(p, c"Height", 0),
        );
        s.output_w = integer(p, c"OutWidth", 0);
        s.output_h = integer(p, c"OutHeight", 0);
        s.color_x = integer(p, c"DLSS.Input.Color.Subrect.Base.X", 0);
        s.color_y = integer(p, c"DLSS.Input.Color.Subrect.Base.Y", 0);
        s.depth_x = integer(p, c"DLSS.Input.Depth.Subrect.Base.X", 0);
        s.depth_y = integer(p, c"DLSS.Input.Depth.Subrect.Base.Y", 0);
        s.motion_x = integer(p, c"DLSS.Input.MV.Subrect.Base.X", 0);
        s.motion_y = integer(p, c"DLSS.Input.MV.Subrect.Base.Y", 0);
        *flags = integer(p, c"DLSS.Feature.Create.Flags", 0);
        *quality = integer(p, c"PerfQualityValue", 0);
        s.low_motion = *flags & 2 != 0;
        s.inverted = *flags & 8 != 0;
        s.motion_scale_x = read_f32(p, c"MV.Scale.X", 1.0);
        s.motion_scale_y = read_f32(p, c"MV.Scale.Y", 1.0);
        s.render_w >= 64
            && s.render_h >= 64
            && s.output_w <= 16384
            && s.output_h <= 16384
            && s.output_w >= s.render_w
            && s.output_h >= s.render_h
            && s.motion_scale_x.is_finite()
            && s.motion_scale_y.is_finite()
            && integer(p, c"DLSS.Output.Subrect.Base.X", 0) == 0
            && integer(p, c"DLSS.Output.Subrect.Base.Y", 0) == 0
            && texture_bounds(s.color, s.color_x, s.color_y, s.render_w, s.render_h)
            && texture_bounds(s.depth, s.depth_x, s.depth_y, s.render_w, s.render_h)
            && texture_bounds(
                s.motion,
                s.motion_x,
                s.motion_y,
                if s.low_motion { s.render_w } else { s.output_w },
                if s.low_motion { s.render_h } else { s.output_h },
            )
            && texture_bounds(s.output, 0, 0, s.output_w, s.output_h)
            && ID3D12Resource::from_raw_borrowed(&s.output).is_some_and(|o| {
                o.GetDesc()
                    .Flags
                    .contains(D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)
            })
    }
}
fn dimension(v: f32) -> u32 {
    // std::max(32u, unsigned(std::lround(v)))
    ((v.round() as i32) as u32).max(32)
}
fn gpu_region(r: Region) -> GpuRegion {
    GpuRegion {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
        input_w: r.input_w,
        input_h: r.input_h,
        source_x: r.source_x,
        source_y: r.source_y,
    }
}
fn owned_resource(p: *mut c_void) -> Option<ID3D12Resource> {
    // SAFETY: one reference handed over (or null).
    (!p.is_null()).then(|| unsafe { ID3D12Resource::from_raw(p) })
}
unsafe fn create_state(
    list: &ID3D12GraphicsCommandList,
    s: &Source,
    settings: Settings,
    original: usize,
    config: &FeatureConfig,
    release: Option<ReleaseFn>,
) -> Option<Ref> {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: the recording list.
    unsafe { list.GetDevice(&mut device) }.ok()?;
    let device = device?;
    let center = gpu_region(foveated::center_region(
        settings, s.render_w, s.render_h, s.output_w, s.output_h,
    ));
    let scales = [1.0f32, 1.0, 0.75, 0.5];
    let mut peripheral = GpuRegion::default();
    peripheral.width = dimension(s.render_w as f32 * scales[settings.mode as usize]);
    peripheral.input_w = peripheral.width;
    peripheral.height = dimension(s.render_h as f32 * scales[settings.mode as usize]);
    peripheral.input_h = peripheral.height;
    let gpu = Gpu::default();
    let ok = gpu.initialize(&device, center, peripheral);
    let prepared = |central: bool| {
        let (region, r) = gpu.prepared(central);
        Prepared {
            region,
            color: owned_resource(r[0]),
            depth: owned_resource(r[1]),
            motion: owned_resource(r[2]),
            output: owned_resource(r[3]),
        }
    };
    let (c, p) = (prepared(true), prepared(false));
    if !ok {
        return None;
    }
    log(&format!(
        "[foveated-native] center {}x{} -> {}x{} at {},{} quality={} peripheral={}x{}\n",
        center.input_w,
        center.input_h,
        center.width,
        center.height,
        center.x,
        center.y,
        config.quality,
        peripheral.width,
        peripheral.height
    ));
    Some(Ref::new(Payload::State(UnsafeCell::new(State {
        gpu,
        center: c,
        periphery: p,
        handles: [core::ptr::null_mut(); 2],
        release,
        original,
        render_w: s.render_w,
        render_h: s.render_h,
        output_w: s.output_w,
        output_h: s.output_h,
        flags: config.flags,
        quality: config.quality,
        presets: config.presets,
        settings,
        evaluations: 0,
        resets: 0,
        first: true,
    }))))
}
fn raw(r: &Option<ID3D12Resource>) -> *mut c_void {
    r.as_ref().map_or(core::ptr::null_mut(), Interface::as_raw)
}
#[allow(clippy::too_many_arguments)]
unsafe fn evaluate_region(
    list: &ID3D12GraphicsCommandList,
    p: *mut Parameters,
    source: &Source,
    state: &mut State,
    route: usize,
    central: bool,
    reset: bool,
) -> Result {
    if !state.gpu.prepare(list, source, central) {
        return 0xBAD0_0005;
    }
    let pass = if central {
        &state.center
    } else {
        &state.periphery
    };
    let r = pass.region;
    let (color, depth, motion, output) = (
        raw(&pass.color),
        raw(&pass.depth),
        raw(&pass.motion),
        raw(&pass.output),
    );
    // SAFETY: the game's parameter object and the NGX entry points the route resolved.
    unsafe {
        let mut change = Override::new(p);
        change.set_resource(c"Color", color);
        change.set_resource(c"Depth", depth);
        change.set_resource(c"MotionVectors", motion);
        change.set_resource(c"Output", output);
        change.set_u32(c"Width", r.input_w);
        change.set_u32(c"Height", r.input_h);
        change.set_u32(c"OutWidth", r.width);
        change.set_u32(c"OutHeight", r.height);
        change.set_u32(c"DLSS.Render.Subrect.Dimensions.Width", r.input_w);
        change.set_u32(c"DLSS.Render.Subrect.Dimensions.Height", r.input_h);
        for key in [
            c"DLSS.Input.Color.Subrect.Base.X",
            c"DLSS.Input.Color.Subrect.Base.Y",
            c"DLSS.Input.Depth.Subrect.Base.X",
            c"DLSS.Input.Depth.Subrect.Base.Y",
            c"DLSS.Input.MV.Subrect.Base.X",
            c"DLSS.Input.MV.Subrect.Base.Y",
            c"DLSS.Output.Subrect.Base.X",
            c"DLSS.Output.Subrect.Base.Y",
            c"DLSS.Enable.Output.Subrects",
        ] {
            change.set_u32(key, 0);
        }
        change.set_u32(c"PerfQualityValue", if central { state.quality } else { 5 });
        change.set_u32(c"DLSS.Feature.Create.Flags", state.flags | 2);
        for (i, key) in PRESET_KEYS.iter().enumerate() {
            change.set_u32(key, if central { state.presets[i] } else { 5 });
        }
        change.set_f32(c"MV.Scale.X", 1.0);
        change.set_f32(c"MV.Scale.Y", 1.0);
        if !central {
            change.set_f32(
                c"Jitter.Offset.X",
                read_f32(p, c"Jitter.Offset.X", 0.0) * r.input_w as f32 / source.render_w as f32,
            );
            change.set_f32(
                c"Jitter.Offset.Y",
                read_f32(p, c"Jitter.Offset.Y", 0.0) * r.input_h as f32 / source.render_h as f32,
            );
            change.set_u32(c"DLSS.Hint.Render.Preset.DLAA", 5);
        }
        change.set_u32(c"Reset", reset as u32);
        let route = &ROUTES[route];
        let slot = &mut state.handles[if central { 0 } else { 1 }];
        if slot.is_null() {
            let create: CreateFn = core::mem::transmute(route[1].load(SeqCst));
            let result = create(list.as_raw(), 1, p, slot);
            log(&format!(
                "[foveated-native] create {} result=0x{:08X} handle={:016X}\n",
                if central { "center" } else { "periphery" },
                result,
                *slot as usize
            ));
            if !success(result) || slot.is_null() {
                return 0xBAD0_0005;
            }
        }
        let evaluate: EvaluateFn = core::mem::transmute(route[0].load(SeqCst));
        evaluate(list.as_raw(), *slot, p, core::ptr::null_mut())
    }
}

unsafe fn process(
    index: usize,
    list: *mut c_void,
    handle: *const c_void,
    params: *const Parameters,
    callback: *mut c_void,
) -> Result {
    let route = &ROUTES[index];
    let evaluate = route[0].load(SeqCst);
    if evaluate == 0 {
        return 0xBAD0_0007;
    }
    // SAFETY: the trampoline MinHook made.
    let evaluate: EvaluateFn = unsafe { core::mem::transmute(evaluate) };
    // SAFETY: the game's call, passed on as it came.
    let original = || unsafe { evaluate(list, handle, params, callback) };
    if INTERNAL.with(Cell::get) || params.is_null() || handle.is_null() {
        return original();
    }
    // SAFETY: the game's recording list.
    let Some(l) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else {
        return original();
    };
    let _recursion = Recursion::new();
    let pose = node_pose();
    // SAFETY: as above.
    if !pose.valid
        || pose.view < 1
        || pose.view > 2
        || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
    {
        return original();
    }
    // SAFETY: null or the C++ half's static.
    if let Some(rt) = unsafe { RUNTIME.load(Acquire).as_ref() } {
        // SAFETY: the game's parameter object.
        (rt.observe_game_quality)(pose.view, unsafe {
            integer(params, c"PerfQualityValue", u32::MAX)
        });
    }
    // Engine stereo: both eyes are MAIN's view, each evaluated on its own viewport (engine_stereo_dlss) with its half
    // as the whole input and output -- told apart by the eye being evaluated, each with its own state.
    let stereo_eye = if crate::engine_stereo::enabled() {
        crate::engine_stereo_dlss::cvr_engine_stereo_dlss_evaluating_eye()
    } else {
        -1
    };
    let mut g = core();
    let eye = if stereo_eye >= 0 {
        stereo_eye as usize
    } else {
        pose.view as usize - 1
    };
    let settings = foveated::cvr_foveated_get_applied_settings();
    CALLS.fetch_add(1, SeqCst);
    LAST_CALL.store(tick(), SeqCst);
    let p = params as *mut Parameters;
    let native = |g: &mut Core| {
        // SAFETY: the game's objects.
        unsafe {
            let mut change = Override::new(p);
            if g.native_reset[eye] {
                change.set_u32(c"Reset", 1);
                g.native_reset[eye] = false;
            }
            let timer = timer(l, true);
            let result = original();
            stamp(l, &timer, 1);
            finish_timer(l, timer, 2);
            result
        }
    };
    // One evaluate over both eyes side by side (per-eye DLSS off) has no single centre region to keep sharp.
    // SAFETY: the C++ half's function.
    if !foveated::enabled(settings)
        || unsafe { GetMenuMode() } != 0
        || (crate::engine_stereo::enabled() && stereo_eye < 0)
    {
        g.views[eye] = None;
        return native(&mut g);
    }
    let mut source = Source {
        color: core::ptr::null_mut(),
        depth: core::ptr::null_mut(),
        motion: core::ptr::null_mut(),
        output: core::ptr::null_mut(),
        render_w: 0,
        render_h: 0,
        output_w: 0,
        output_h: 0,
        color_x: 0,
        color_y: 0,
        depth_x: 0,
        depth_y: 0,
        motion_x: 0,
        motion_y: 0,
        motion_scale_x: 0.0,
        motion_scale_y: 0.0,
        low_motion: false,
        inverted: false,
    };
    let (mut flags, mut quality) = (0u32, 0u32);
    // SAFETY: the game's parameter object.
    if !unsafe { read_source(params, &mut source, &mut flags, &mut quality) } {
        message("Unsupported DLSS inputs; native rendering");
        return native(&mut g);
    }
    let mut config = FeatureConfig {
        output_w: source.output_w,
        output_h: source.output_h,
        flags,
        quality,
        presets: [0; 6],
    };
    if !g
        .game_features
        .find(handle as usize, params as usize, pose.view, &mut config)
    {
        g.views[eye] = None;
        message("Waiting for game DLSS configuration; native rendering");
        return native(&mut g);
    }
    let stale = match &g.views[eye] {
        None => true,
        Some(r) => {
            // SAFETY: under CORE's lock.
            let s = unsafe { r.state() };
            s.original != handle as usize
                || s.render_w != source.render_w
                || s.render_h != source.render_h
                || s.output_w != source.output_w
                || s.output_h != source.output_h
                || s.flags != flags
                || s.quality != quality
                || s.presets != config.presets
                || !s.settings.same_reconstruction(&settings)
        }
    };
    if stale {
        let release = match route[2].load(SeqCst) {
            0 => None,
            // SAFETY: NGX's export.
            f => Some(unsafe { core::mem::transmute::<usize, ReleaseFn>(f) }),
        };
        // SAFETY: the recording list.
        g.views[eye] =
            unsafe { create_state(l, &source, settings, handle as usize, &config, release) };
    }
    let Some(state_ref) = g.views[eye].clone().filter(|s| keep(l, s)) else {
        message("Foveated resource initialization failed");
        return native(&mut g);
    };
    let _private_commands = cvr_host::probe::Internal::new();
    // SAFETY: under CORE's lock; the state's only user.
    let state = unsafe { state_ref.state() };
    // SAFETY: the game's parameter object.
    let reset = state.first || unsafe { integer(params, c"Reset", 0) } != 0;
    // SAFETY: the recording list, the game's parameters and NGX's entry points.
    unsafe {
        let timer = timer(l, false);
        let mut result = evaluate_region(l, p, &source, state, index, false, reset);
        stamp(l, &timer, 1);
        if success(result) {
            result = evaluate_region(l, p, &source, state, index, true, reset);
        }
        stamp(l, &timer, 2);
        if success(result) && state.gpu.composite(l, &source, settings.show_boundary) {
            stamp(l, &timer, 3);
            finish_timer(l, timer, 4);
            state.first = false;
            state.evaluations += 1;
            state.resets += reset as u64;
            g.view_calls[eye] += 1;
            ACTIVE.fetch_add(1, SeqCst);
            g.native_reset[eye] = true;
            message("Active");
            return result;
        }
        FAILED.fetch_add(1, SeqCst);
        log(&format!(
            "[foveated-native] evaluation failed result=0x{result:08X}; native fallback\n"
        ));
        drop(state_ref);
        g.views[eye] = None;
        g.native_reset[eye] = true;
        message("Foveated evaluation failed; native rendering");
        native(&mut g)
    }
}
unsafe extern "C" fn hook0(
    list: *mut c_void,
    handle: *const c_void,
    p: *const Parameters,
    callback: *mut c_void,
) -> Result {
    // SAFETY: the game's call.
    unsafe { process(0, list, handle, p, callback) }
}
unsafe extern "C" fn hook1(
    list: *mut c_void,
    handle: *const c_void,
    p: *const Parameters,
    callback: *mut c_void,
) -> Result {
    // SAFETY: the game's call.
    unsafe { process(1, list, handle, p, callback) }
}
unsafe extern "C" fn create_hook(
    list: *mut c_void,
    feature: u32,
    p: *mut Parameters,
    handle: *mut *mut c_void,
) -> Result {
    // SAFETY: the trampoline MinHook made before this hook was enabled.
    let original: CreateFn = unsafe { core::mem::transmute(ORIGINAL_CREATE.load(SeqCst)) };
    if INTERNAL.with(Cell::get) || feature != 1 || p.is_null() {
        // SAFETY: the game's call.
        return unsafe { original(list, feature, p, handle) };
    }
    let _recursion = Recursion::new();
    // SAFETY: the game's parameter object and call.
    unsafe {
        let mut config = FeatureConfig {
            output_w: integer(p, c"OutWidth", 0),
            output_h: integer(p, c"OutHeight", 0),
            flags: integer(p, c"DLSS.Feature.Create.Flags", 0),
            quality: integer(p, c"PerfQualityValue", 0),
            presets: [0; 6],
        };
        for (i, key) in PRESET_KEYS.iter().enumerate() {
            config.presets[i] = integer(p, key, 0);
        }
        let pose = node_pose();
        let result = original(list, feature, p, handle);
        if success(result) && !handle.is_null() && !(*handle).is_null() {
            let view = if pose.valid { pose.view } else { 0 };
            core()
                .game_features
                .remember(*handle as usize, p as usize, view, config);
            let pr = config.presets;
            log(&format!(
                "[foveated-native] game feature={:016X} params={:016X} view={} output={}x{} quality={} flags=0x{:X} presets={}/{}/{}/{}/{}/{}\n",
                *handle as usize,
                p as usize,
                view,
                config.output_w,
                config.output_h,
                config.quality,
                config.flags,
                pr[0],
                pr[1],
                pr[2],
                pr[3],
                pr[4],
                pr[5]
            ));
        }
        result
    }
}

/// A failed MH_CreateHook has already scanned the address space near its target for a trampoline block (VirtualQuery,
/// about 10 ms in this process) -- on the game's Present thread. Retried every two seconds it cost one 45 fps frame slot
/// each time. A failure is deterministic for a loaded module: a few attempts, then nothing until a different
/// nvngx_dlss.dll is loaded (main's 0.2.4).
const INSTALL_ATTEMPTS: u32 = 3;
static INSTALL_MODULE: AtomicUsize = AtomicUsize::new(0);
static INSTALL_ATTEMPT: AtomicU32 = AtomicU32::new(0);
const MH_ERROR_MEMORY_ALLOC: i32 = 9;

/// MinHook's hook, or -- with no free memory near nvngx_dlss.dll for its relay -- the far hook
/// (cvr_engine::far_hook); the failure logged with the attempt.
///
/// # Safety
/// `target` is an NGX export; `detour` has its signature; `original` a static slot.
unsafe fn hook_ngx(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void, what: &str) -> bool {
    // SAFETY: as the caller vouches.
    unsafe {
        let mut created = MH_CreateHook(target, detour, original);
        if created == MH_OK {
            created = MH_EnableHook(target);
        }
        if created == MH_OK {
            return true;
        }
        if created == MH_ERROR_MEMORY_ALLOC {
            let distant = cvr_engine::far_hook::far_inline_hook(target, detour, original);
            log(&format!(
                "[foveated-native] {what}: no free memory near nvngx_dlss.dll for MinHook; far hook {}\n",
                distant.text()
            ));
            if distant == cvr_engine::far_hook::FarHookResult::Ok {
                return true;
            }
        }
        let attempt = INSTALL_ATTEMPT.load(SeqCst);
        log(&format!(
            "[foveated-native] {what} failed: {} (attempt {attempt}/{INSTALL_ATTEMPTS}){}\n",
            cvr_engine::hooks::status_name(created),
            if attempt >= INSTALL_ATTEMPTS { "; not retrying for this DLSS module" } else { "" }
        ));
        false
    }
}

fn install() {
    use windows::core::{s, w};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    // SAFETY: module queries and MinHook on NGX's exports.
    unsafe {
        let Ok(module) = GetModuleHandleW(w!("nvngx_dlss.dll")) else {
            return;
        };
        if GetModuleHandleW(w!("CheekyFoveatedDLSSRuntime.dll")).is_ok() {
            message("Another foveated backend is loaded; restart required");
            return;
        }
        let create =
            GetProcAddress(module, s!("NVSDK_NGX_D3D12_CreateFeature")).map_or(0, |f| f as usize);
        let release =
            GetProcAddress(module, s!("NVSDK_NGX_D3D12_ReleaseFeature")).map_or(0, |f| f as usize);
        if create == 0 || release == 0 {
            return;
        }
        if INSTALL_MODULE.swap(module.0 as usize, SeqCst) != module.0 as usize {
            INSTALL_ATTEMPT.store(0, SeqCst);
        }
        if INSTALL_ATTEMPT.load(SeqCst) >= INSTALL_ATTEMPTS {
            return;
        }
        INSTALL_ATTEMPT.fetch_add(1, SeqCst);
        if ORIGINAL_CREATE.load(SeqCst) == 0
            && !hook_ngx(
                create as *mut c_void,
                create_hook as *mut c_void,
                ORIGINAL_CREATE.as_ptr() as *mut *mut c_void,
                "CreateFeature hook",
            )
        {
            return;
        }
        let create = ORIGINAL_CREATE.load(SeqCst);
        let names = [
            c"NVSDK_NGX_D3D12_EvaluateFeature",
            c"NVSDK_NGX_D3D12_EvaluateFeature_C",
        ];
        let hooks = [hook0 as *const () as usize, hook1 as *const () as usize];
        for i in 0..2 {
            let target =
                GetProcAddress(module, windows::core::PCSTR(names[i].as_ptr() as *const u8))
                    .map_or(0, |f| f as usize);
            if target == 0 || (i == 1 && target == ROUTES[0][3].load(SeqCst)) {
                continue;
            }
            if ROUTES[i][0].load(SeqCst) != 0 {
                continue;
            }
            ROUTES[i][1].store(create, SeqCst);
            ROUTES[i][2].store(release, SeqCst);
            ROUTES[i][3].store(target, SeqCst);
            if hook_ngx(
                target as *mut c_void,
                hooks[i] as *mut c_void,
                ROUTES[i][0].as_ptr() as *mut *mut c_void,
                &names[i].to_string_lossy(),
            ) {
                INSTALLED.store(true, SeqCst);
                log(&format!(
                    "[foveated-native] hooked {}\n",
                    names[i].to_string_lossy()
                ));
            }
        }
    }
}

// ---- the report (as jsoncpp's StreamWriterBuilder with indentation "" wrote it) ---------------------------------

enum Json {
    Null,
    Bool(bool),
    U(u64),
    F(f64),
    S(String),
    A(Vec<Json>),
    O(BTreeMap<&'static str, Json>),
}
fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
/// jsoncpp's valueToString(double, false, 17, significantDigits).
fn real(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "null"
        } else if v < 0.0 {
            "-1e+9999"
        } else {
            "1e+9999"
        }
        .into();
    }
    let mut s = cvr_crt::format(c"%.17g", &[cvr_crt::Arg::F(v)])
        .to_string_lossy()
        .into_owned();
    if !s.contains('.') && !s.contains('e') {
        s.push_str(".0");
    }
    s
}
impl Json {
    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::U(v) => out.push_str(&v.to_string()),
            Json::F(v) => out.push_str(&real(*v)),
            Json::S(s) => quote(s, out),
            Json::A(a) if a.is_empty() => out.push_str("[]"),
            Json::A(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Json::O(o) if o.is_empty() => out.push_str("{}"),
            Json::O(o) => {
                out.push('{');
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    quote(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn publish(g: &Core) {
    let settings = foveated::cvr_foveated_get_applied_settings();
    let requested = foveated::cvr_foveated_get_settings();
    let enabled = foveated::enabled(settings);
    let mut root = BTreeMap::new();
    root.insert("backend", Json::S("native".into()));
    root.insert("enabled", Json::Bool(enabled));
    let same = settings.same_reconstruction(&requested)
        && settings.show_boundary == requested.show_boundary;
    root.insert("pending_settings", Json::Bool(!same));
    let current = {
        let mut s = stats();
        let installed = INSTALLED.load(SeqCst);
        s.loaded = installed;
        s.ready = installed;
        s.evaluations = CALLS.load(SeqCst);
        s.processed = ACTIVE.load(SeqCst);
        s.views = g.views[0].is_some() as u32 + g.views[1].is_some() as u32;
        let since = tick().wrapping_sub(LAST_CALL.load(SeqCst));
        s.active = enabled && since < 2000 && s.text() == "Active";
        if !enabled {
            s.set_text("Off");
        } else if tick().wrapping_sub(LAST_CALL.load(SeqCst)) > 2000 {
            s.set_text("Waiting for DLSS SR");
        }
        *s
    };
    let mut api = BTreeMap::new();
    api.insert("state", Json::S(current.text().into()));
    api.insert("evaluations", Json::U(CALLS.load(SeqCst)));
    api.insert("active", Json::U(ACTIVE.load(SeqCst)));
    api.insert("failures", Json::U(FAILED.load(SeqCst)));
    api.insert("native_ms", Json::F(f64::from(current.native_ms)));
    api.insert("foveated_ms", Json::F(f64::from(current.foveated_ms)));
    api.insert("peripheral_ms", Json::F(f64::from(current.peripheral_ms)));
    api.insert(
        "after_pipeline_ms",
        Json::F(f64::from(if enabled {
            current.total_ms
        } else {
            current.native_ms
        })),
    );
    root.insert("apis", Json::A(vec![Json::Null, Json::O(api)]));
    let mut details = Vec::new();
    for (i, view) in g.views.iter().enumerate() {
        let Some(r) = view else { continue };
        // SAFETY: under CORE's lock (Tick holds it).
        let state = unsafe { r.state() };
        let c = state.center.region;
        let mut v = BTreeMap::new();
        v.insert("id", Json::U(i as u64));
        let eye = if crate::engine_stereo::enabled() {
            if i == 1 {
                "right"
            } else {
                "left"
            }
        } else if i == 1 {
            "VRCAM"
        } else {
            "MAIN"
        };
        v.insert("eye", Json::S(eye.into()));
        v.insert("evaluations", Json::U(g.view_calls[i]));
        v.insert("resets", Json::U(state.resets));
        v.insert("input_width", Json::U(u64::from(state.render_w)));
        v.insert("input_height", Json::U(u64::from(state.render_h)));
        v.insert("output_width", Json::U(u64::from(state.output_w)));
        v.insert("output_height", Json::U(u64::from(state.output_h)));
        v.insert("center_input_width", Json::U(u64::from(c.input_w)));
        v.insert("center_input_height", Json::U(u64::from(c.input_h)));
        v.insert("center_quality", Json::U(u64::from(state.quality)));
        v.insert(
            "offset_x_percent",
            Json::F(f64::from(state.settings.offset_x_percent)),
        );
        v.insert(
            "offset_y_percent",
            Json::F(f64::from(state.settings.offset_y_percent)),
        );
        v.insert(
            "center_presets",
            Json::A(
                state
                    .presets
                    .iter()
                    .map(|&p| Json::U(u64::from(p)))
                    .collect(),
            ),
        );
        v.insert("crop_width", Json::U(u64::from(c.width)));
        v.insert("crop_height", Json::U(u64::from(c.height)));
        v.insert("crop_x", Json::U(u64::from(c.x)));
        v.insert("crop_y", Json::U(u64::from(c.y)));
        details.push(Json::O(v));
    }
    root.insert("view_details", Json::A(details));
    let mut text = String::new();
    Json::O(root).write(&mut text);
    // SAFETY: the C++ half's export, written by this thread only.
    unsafe {
        CyberpunkVR_FoveatedReportSeq.fetch_add(1, AcqRel);
        let report = &mut *(&raw mut CyberpunkVR_FoveatedReport);
        let n = text.len().min(report.len() - 1);
        report[..n].copy_from_slice(&text.as_bytes()[..n]);
        report[n] = 0;
        CyberpunkVR_FoveatedReportSeq.fetch_add(1, Release);
    }
}

// ---- the C API (include/Render/FoveatedDlss.hpp, forwarded by FoveatedDlss.cpp) --------------------------------

/// GetStatus.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_dlss_status(out: *mut Status) {
    let s = *stats();
    // SAFETY: the caller's slot.
    if let Some(out) = unsafe { out.as_mut() } {
        *out = s;
    }
}

/// Tick: once per game Present, outside the image-submission lock -- the settings, the hooks' install (every 2 s until
/// they are in), the private NGX features released since, and the report every 500 ms.
#[no_mangle]
pub extern "C" fn cvr_foveated_dlss_tick() {
    foveated::cvr_foveated_update_settings();
    if !INSTALLED.load(SeqCst) && tick() >= NEXT_INSTALL.load(SeqCst) {
        NEXT_INSTALL.store(tick() + 2000, SeqCst);
        install();
    }
    let g = core();
    let retired = core::mem::take(&mut *RETIRED.lock().unwrap_or_else(|p| p.into_inner()));
    for (release, handle) in retired {
        use windows_sys::Win32::System::LibraryLoader::{
            GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        };
        let mut module = core::ptr::null_mut();
        // SAFETY: an address query; the release only while its module is still loaded.
        unsafe {
            if GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                release as *const u16,
                &mut module,
            ) != 0
            {
                core::mem::transmute::<usize, ReleaseFn>(release)(handle as *mut c_void);
            }
        }
    }
    static NEXT_REPORT: AtomicU64 = AtomicU64::new(0);
    if tick() >= NEXT_REPORT.load(SeqCst) {
        NEXT_REPORT.store(tick() + 500, SeqCst);
        publish(&g);
    }
}

/// BeforeSubmit: the timings recorded on these lists learn the queue's timestamp frequency.
///
/// # Safety
/// `queue` is an ID3D12CommandQueue; `lists` holds `count` command lists (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_dlss_before_submit(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    if lists.is_null() {
        return;
    }
    let mut timings = TIMINGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(map) = timings.as_mut() else { return };
    let mut frequency = 0u64;
    // SAFETY: the caller's array and queue.
    unsafe {
        for &list in core::slice::from_raw_parts(lists, count as usize) {
            if let Some(found) = map.remove(&(list as usize)) {
                if frequency == 0 {
                    if let Some(q) = ID3D12CommandQueue::from_raw_borrowed(&queue) {
                        frequency = q.GetTimestampFrequency().unwrap_or(0);
                    }
                }
                for t in &found {
                    t.timed().frequency.store(frequency, SeqCst);
                }
            }
        }
    }
}

/// ResetList: a list reset before it was submitted takes its timings with it.
#[no_mangle]
pub extern "C" fn cvr_foveated_dlss_reset_list(list: *mut c_void) {
    let mut timings = TIMINGS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(map) = timings.as_mut() {
        map.remove(&(list as usize));
    }
}

#[no_mangle]
pub extern "C" fn cvr_foveated_dlss_shutdown() {
    let mut g = core();
    g.views = [None, None];
}
