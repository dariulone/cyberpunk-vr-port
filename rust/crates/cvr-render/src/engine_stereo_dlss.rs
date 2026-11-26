//! DLSS PER EYE UNDER THE ENGINE'S STEREO (formerly src/Stereo/EngineStereoDlss.cpp; that file keeps the exports,
//! the layout asserts and the C++ face). Under render feature 98 the engine's one DLSS evaluate over the side-by-side
//! frame becomes one evaluate per eye.
//!
//! What the engine hands Streamline for its viewport each frame -- the tags, the constants, the options -- is recorded
//! as it goes by. At its evaluate each eye gets a viewport of its own (one DLSS history per eye), the engine's tags
//! with their extents cut to the eye's half (the right eye reads copies of its halves at (0, 0) and writes a target of
//! its own, copied into its half: DLSS writes nothing at an output offset), the engine's options with half the output
//! width, the constants with the motion scale's x doubled, and the jitter in the eye's own pixels. Ray Reconstruction
//! (DLSS-D) goes through the same split with its own options, viewports and each eye's own camera. Anything that does
//! not fit runs the engine's evaluate unchanged.
use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cvr_engine::code;
use cvr_host::log;
use windows::core::{IUnknown, Interface, GUID, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::{EnterCriticalSection, InitializeCriticalSection, LeaveCriticalSection, CRITICAL_SECTION};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

use crate::streamline::*;

// ---- the C++ half's exports (src/Stereo/EngineStereoDlss.cpp)

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_EngineStereoDlssPerEye: AtomicU32;
    static mut CyberpunkVR_DebugEngineStereoDlss: [u64; 8];
    static CyberpunkVR_EngineStereoDlssMode: AtomicU32;
    static mut CyberpunkVR_DebugEngineStereoDlssResults: [i32; 8];
    static CyberpunkVR_EngineStereoDlssEyeJitter: AtomicU32;
    static mut CyberpunkVR_DebugEngineStereoDlssJitter: [f32; 6];
    static CyberpunkVR_EngineStereoDlssJitterTest: [f32; 3];
    static CyberpunkVR_EngineStereoDlssDropTags: AtomicU32;
    static CyberpunkVR_EngineStereoDlssFailTest: AtomicU32;
    static mut CyberpunkVR_DebugEngineStereoDlssFallback: [u64; 2];
    static CyberpunkVR_EngineStereoDlssDropTagsHigh: AtomicU32;
    static CyberpunkVR_EngineStereoRrCopyLeft: AtomicU32;
    static CyberpunkVR_EngineStereoRrPerEye: AtomicU32;
    static CyberpunkVR_EngineStereoRrEyeCamera: AtomicU32;
    static mut CyberpunkVR_DebugEngineStereoRr: [u64; 6];
}

#[allow(non_snake_case)]
extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

fn debug() -> &'static mut [u64; 8] {
    // SAFETY: an export of the C++ half, written under the module's lock (as the C++ file wrote it).
    unsafe { &mut *(&raw mut CyberpunkVR_DebugEngineStereoDlss) }
}
fn debug_rr() -> &'static mut [u64; 6] {
    // SAFETY: as above.
    unsafe { &mut *(&raw mut CyberpunkVR_DebugEngineStereoRr) }
}
fn now() -> u64 {
    // SAFETY: a plain Win32 call.
    unsafe { GetTickCount64() }
}

pub type SetTagFn = unsafe extern "C" fn(
    viewport: *const c_void,
    tags: *const c_void,
    count: u32,
    list: *mut c_void,
) -> u32;
pub type ConstantsFn = unsafe extern "C" fn(
    values: *const Constants,
    frame: *const c_void,
    viewport: *const ViewportHandle,
) -> SlResult;
pub type EvaluateFn = unsafe extern "C" fn(
    feature: Feature,
    frame: *const c_void,
    inputs: *const *const BaseStructure,
    count: u32,
    list: *mut c_void,
) -> SlResult;
type SetOptionsFn =
    unsafe extern "C" fn(viewport: *const ViewportHandle, options: *const DlssOptions) -> SlResult;
type SetRrOptionsFn =
    unsafe extern "C" fn(viewport: *const ViewportHandle, options: *const DlssdOptions) -> SlResult;
type GetFeatureFunctionFn =
    unsafe extern "C" fn(feature: Feature, name: *const u8, out: *mut *mut c_void) -> SlResult;
type AllocateResourcesFn = unsafe extern "C" fn(
    list: *mut c_void,
    feature: Feature,
    viewport: *const ViewportHandle,
) -> SlResult;
type FreeResourcesFn =
    unsafe extern "C" fn(feature: Feature, viewport: *const ViewportHandle) -> SlResult;

const EYE_VIEWPORT: [u32; 2] = [0x43565201, 0x43565202];
/// Ray Reconstruction's eyes have viewports of their own (both features on one viewport crashed the GPU on the switch).
const RR_EYE_VIEWPORT: [u32; 2] = [0x43565203, 0x43565204];
/// RR's guides on top of DLSS's.
const MAX_TAGS: usize = 40;
const READABLE: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATES(
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0 | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0,
);

fn own_viewport(v: u32) -> bool {
    v == EYE_VIEWPORT[0]
        || v == EYE_VIEWPORT[1]
        || v == RR_EYE_VIEWPORT[0]
        || v == RR_EYE_VIEWPORT[1]
}
fn fail_test(bit: u32) -> bool {
    // SAFETY: an export of the C++ half.
    let test = unsafe { CyberpunkVR_EngineStereoDlssFailTest.load(Ordering::Relaxed) };
    test & bit != 0
}

// ---- the engine's original DLSS context, idle (Render/IdleDlssContext.hpp)

/// A COM object a recording holds while its submission is in flight: the context's pending count while alive.
#[repr(C)]
struct Marker {
    vtable: *const MarkerVtbl,
    refs: AtomicU32,
    flight: Arc<AtomicU32>,
}
#[repr(C)]
struct MarkerVtbl {
    query_interface:
        unsafe extern "system" fn(*mut Marker, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut Marker) -> u32,
    release: unsafe extern "system" fn(*mut Marker) -> u32,
}
unsafe extern "system" fn marker_query(
    this: *mut Marker,
    id: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return windows::Win32::Foundation::E_POINTER;
    }
    // SAFETY: COM's contract.
    unsafe {
        *out = core::ptr::null_mut();
        if id.is_null() || *id != IUnknown::IID {
            return windows::Win32::Foundation::E_NOINTERFACE;
        }
        *out = this as *mut c_void;
        marker_add_ref(this);
    }
    windows::Win32::Foundation::S_OK
}
unsafe extern "system" fn marker_add_ref(this: *mut Marker) -> u32 {
    // SAFETY: a live marker.
    unsafe { (*this).refs.fetch_add(1, Ordering::AcqRel) + 1 }
}
unsafe extern "system" fn marker_release(this: *mut Marker) -> u32 {
    // SAFETY: a live marker; freed at its last reference.
    unsafe {
        let n = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if n == 0 {
            (*this).flight.fetch_sub(1, Ordering::AcqRel);
            drop(Box::from_raw(this));
        }
        n
    }
}
static MARKER_VTBL: MarkerVtbl = MarkerVtbl {
    query_interface: marker_query,
    add_ref: marker_add_ref,
    release: marker_release,
};

/// The engine's own viewport's DLSS feature: released once the split has run twice and nothing in flight holds it.
struct IdleDlssContext {
    flight: Arc<AtomicU32>,
    last_use: u64,
    split_successes: u32,
    resident: bool,
    unsafe_: bool,
}
impl IdleDlssContext {
    fn new() -> IdleDlssContext {
        IdleDlssContext {
            flight: Arc::new(AtomicU32::new(0)),
            last_use: 0,
            split_successes: 0,
            resident: false,
            unsafe_: false,
        }
    }
    fn used(&mut self, list: *mut c_void, now: u64) {
        self.resident = true;
        self.last_use = now;
        self.split_successes = 0;
        self.flight.fetch_add(1, Ordering::AcqRel);
        let marker = Box::into_raw(Box::new(Marker {
            vtable: &MARKER_VTBL,
            refs: AtomicU32::new(1),
            flight: self.flight.clone(),
        }));
        // SAFETY: the caller's recording list.
        let kept = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }
            .is_some_and(|l| cvr_gpu::command_resources::keep(l, &[marker as *mut c_void]));
        if !kept {
            self.unsafe_ = true;
        }
        // SAFETY: our reference.
        unsafe { marker_release(marker) };
    }
    fn split_succeeded(&mut self) {
        if self.split_successes < 2 {
            self.split_successes += 1;
        }
    }
    fn ready(&self, now: u64) -> bool {
        self.resident
            && !self.unsafe_
            && self.split_successes >= 2
            && now >= self.last_use
            && now - self.last_use >= 2000
            && self.flight.load(Ordering::Acquire) == 0
    }
    fn released(&mut self) {
        self.resident = false;
        self.split_successes = 0;
    }
    fn disable(&mut self) {
        self.unsafe_ = true;
    }
    fn pending(&self) -> u32 {
        self.flight.load(Ordering::Acquire)
    }
}

// ---- the recorded state

#[derive(Clone, Copy)]
struct EyeCopy {
    resource: Option<*mut c_void>,
    tag: Resource,
    width: u64,
    height: u32,
    format: DXGI_FORMAT,
    /// Where it rests between evaluates.
    state: D3D12_RESOURCE_STATES,
    ty: BufferType,
    output: bool,
    logged: bool,
    /// A depth-stencil source copied whole first (it cannot be copied in part).
    staging: Option<*mut c_void>,
    /// The copy's and its staging's allocation (main's 8255f379b memory diagnostic).
    cache_bytes: u64,
    /// Tagged in this evaluate (the diagnostic's).
    used: bool,
    /// Every tag of it so far valid only until the evaluate: a Present-lifetime tag keeps it for Streamline's Present.
    release_after_evaluate: bool,
}
/// The eye copies' memory, per eye pairs (runtime diagnostics only; main's 8255f379b): [0..1] the copies' allocation,
/// [2..3] their resources (staging included), [4..5] bytes unused this evaluate and valid only until it, [6..7] bytes
/// allocated in all, [8..9] unused bytes kept for Streamline's Present lifetime. NGX's own and allocations still held by
/// recordings or unfinished GPU work are not in it.
#[no_mangle]
pub static CyberpunkVR_EngineStereoCopyMemory: [core::sync::atomic::AtomicU64; 10] = [const { core::sync::atomic::AtomicU64::new(0) }; 10];
fn report_copy_memory(s: &State) {
    for eye in 0..2 {
        let (mut bytes, mut count, mut until_evaluate, mut until_present) = (0u64, 0u64, 0u64, 0u64);
        for c in &s.copies[eye] {
            bytes += c.cache_bytes;
            count += u64::from(c.resource.is_some()) + u64::from(c.staging.is_some());
            if c.resource.is_some() && !c.used {
                if c.release_after_evaluate {
                    until_evaluate += c.cache_bytes;
                } else {
                    until_present += c.cache_bytes;
                }
            }
        }
        let m = &CyberpunkVR_EngineStereoCopyMemory;
        m[eye].store(bytes, Ordering::Relaxed);
        m[2 + eye].store(count, Ordering::Relaxed);
        m[4 + eye].store(until_evaluate, Ordering::Relaxed);
        m[8 + eye].store(until_present, Ordering::Relaxed);
    }
}
fn memory_diagnostic() -> bool {
    // SAFETY: an export, read whole.
    unsafe { core::ptr::read_volatile(&raw const crate::declared_moved_exports::CyberpunkVR_RuntimeDiagnostics) != 0 }
}
const NO_COPY: EyeCopy = EyeCopy {
    resource: None,
    tag: Resource {
        base: BaseStructure {
            next: core::ptr::null_mut(),
            struct_type: RESOURCE,
            struct_version: 1,
        },
        ty: 0,
        native: core::ptr::null_mut(),
        memory: core::ptr::null_mut(),
        view: core::ptr::null_mut(),
        state: u32::MAX,
        width: 0,
        height: 0,
        native_format: 0,
        mip_levels: 0,
        array_layers: 0,
        gpu_virtual_address: 0,
        flags: 0,
        usage: 0,
        internal_flags: 0,
        reserved: 0,
    },
    width: 0,
    height: 0,
    format: DXGI_FORMAT_UNKNOWN,
    state: D3D12_RESOURCE_STATE_COMMON,
    ty: 0,
    output: false,
    logged: false,
    staging: None,
    cache_bytes: 0,
    used: false,
    release_after_evaluate: true,
};

struct TagSet {
    tag: [ResourceTag; MAX_TAGS],
    resource: [Resource; MAX_TAGS],
    count: usize,
    viewport: u32,
    valid: bool,
}
struct State {
    original_context: IdleDlssContext,
    original_reclaimed: bool,
    tags: TagSet,
    constants: Option<(Constants, u32, u32)>,
    /// The engine's options (values, viewport), valid.
    engine_options: Option<(DlssOptions, u32)>,
    eye_options: [Option<DlssOptions>; 2],
    set_tag: Option<SetTagFn>,
    /// RR's options as bytes: the engine's copy keeps its own padding, and the eyes' are compared byte for byte.
    engine_rr_options: Option<([u8; 224], u32)>,
    eye_rr_options: [Option<[u8; 224]>; 2],
    output_retry_at: u64,
    output_missing_logged: bool,
    failed_frames: u32,
    copies: [[EyeCopy; MAX_TAGS]; 2],
}

/// The C++ file's recursive mutex: a critical section (Streamline may call back into the hooked lifecycle functions
/// while an evaluate holds it).
struct Locked {
    section: UnsafeCell<CRITICAL_SECTION>,
    ready: AtomicBool,
    init: Mutex<()>,
    state: UnsafeCell<Option<Box<State>>>,
}
// SAFETY: every access goes through the critical section.
unsafe impl Sync for Locked {}
static LOCKED: Locked = Locked {
    section: UnsafeCell::new(unsafe { core::mem::zeroed() }),
    ready: AtomicBool::new(false),
    init: Mutex::new(()),
    state: UnsafeCell::new(None),
};
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: entered by `lock`.
        unsafe { LeaveCriticalSection(LOCKED.section.get()) };
    }
}
/// The state under the lock (re-entrant on the same thread, as std::recursive_mutex).
fn lock() -> (Guard, &'static mut State) {
    if !LOCKED.ready.load(Ordering::Acquire) {
        let _g = LOCKED.init.lock().unwrap_or_else(|e| e.into_inner());
        if !LOCKED.ready.load(Ordering::Relaxed) {
            // SAFETY: initialised once, before any use.
            unsafe { InitializeCriticalSection(LOCKED.section.get()) };
            LOCKED.ready.store(true, Ordering::Release);
        }
    }
    // SAFETY: initialised above.
    unsafe { EnterCriticalSection(LOCKED.section.get()) };
    // SAFETY: under the critical section; a recursive entry hands out the same state, as the C++ globals were.
    let state = unsafe { &mut *LOCKED.state.get() }.get_or_insert_with(|| {
        Box::new(State {
            original_context: IdleDlssContext::new(),
            original_reclaimed: false,
            tags: TagSet {
                tag: [ResourceTag {
                    base: BaseStructure {
                        next: core::ptr::null_mut(),
                        struct_type: RESOURCE_TAG,
                        struct_version: 1,
                    },
                    resource: core::ptr::null_mut(),
                    ty: 0,
                    lifecycle: 0,
                    extent: Extent::default(),
                }; MAX_TAGS],
                resource: [NO_COPY.tag; MAX_TAGS],
                count: 0,
                viewport: 0,
                valid: false,
            },
            constants: None,
            engine_options: None,
            eye_options: [None; 2],
            set_tag: None,
            engine_rr_options: None,
            eye_rr_options: [None; 2],
            output_retry_at: 0,
            output_missing_logged: false,
            failed_frames: 0,
            copies: [[NO_COPY; MAX_TAGS]; 2],
        })
    });
    (Guard, state)
}

static LIFECYCLE_HOOKS_READY: AtomicBool = AtomicBool::new(false);
static OPTIONS_INTERCEPTED: AtomicBool = AtomicBool::new(false);
static OPTIONS_HOOKED: AtomicBool = AtomicBool::new(false);
static RR_OPTIONS_HOOKED: AtomicBool = AtomicBool::new(false);
static SET_OPTIONS: AtomicUsize = AtomicUsize::new(0);
static SET_RR_OPTIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATE_RESOURCES: AtomicUsize = AtomicUsize::new(0);
static FREE_RESOURCES: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static EVALUATING_EYE: Cell<i32> = const { Cell::new(-1) };
    /// EvaluatingAreas (main's 2e123a4a3): the eye's output, depth and motion areas and its motion scale.
    static EVALUATING_AREAS: Cell<crate::neural_render::EyeAreas> = Cell::new(crate::neural_render::EyeAreas {
        output: crate::neural_render::EyeArea { left: 0, top: 0, width: 0, height: 0 },
        depth: crate::neural_render::EyeArea { left: 0, top: 0, width: 0, height: 0 },
        motion: crate::neural_render::EyeArea { left: 0, top: 0, width: 0, height: 0 },
        motion_scale: [0.0; 2],
    });
}

fn fn_at<F: Copy>(slot: &AtomicUsize) -> Option<F> {
    const { assert!(core::mem::size_of::<F>() == 8) };
    let p = slot.load(Ordering::Acquire);
    // SAFETY: a function of the signature F (a MinHook trampoline or the export itself).
    (p != 0).then(|| unsafe { core::mem::transmute_copy::<usize, F>(&p) })
}

// ---- guarded reads of the engine's structures

/// `GetDesc` of a resource the engine names, under SEH (ID3D12Resource::GetDesc, vtable slot 10, its result through a
/// hidden pointer).
fn desc_of(resource: *mut c_void) -> Option<D3D12_RESOURCE_DESC> {
    if resource.is_null() {
        return None;
    }
    let vtable = code::read::<usize>(resource as usize)?;
    let get_desc = code::read::<usize>(vtable + 10 * 8)?;
    let mut desc = D3D12_RESOURCE_DESC::default();
    // SAFETY: the engine's resource's own method, inside the SEH adapter.
    unsafe {
        code::guarded_call4(
            get_desc,
            resource,
            &mut desc as *mut D3D12_RESOURCE_DESC as *mut c_void,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )?
    };
    Some(desc)
}
fn viewport_value(handle: *const c_void) -> u32 {
    if handle.is_null() {
        return EYE_VIEWPORT[0];
    }
    // Treated as ours (ignored) when it cannot be read.
    code::read::<u32>(handle as usize + 32).unwrap_or(EYE_VIEWPORT[0])
}
fn struct_type_at(p: usize) -> Option<StructType> {
    code::read::<StructType>(p + 8)
}
/// The index of the viewport among an evaluate's inputs and its value; None without one.
fn find_viewport(inputs: *const *const BaseStructure, count: u32) -> Option<(usize, u32)> {
    for i in 0..count as usize {
        let p = code::read::<usize>(inputs as usize + i * 8)?;
        if p != 0 && struct_type_at(p)? == VIEWPORT_HANDLE {
            return Some((i, code::read::<u32>(p + 32)?));
        }
    }
    None
}
pub(crate) fn frame_value(frame: *const c_void) -> u32 {
    // SAFETY: sl::FrameToken's `operator uint32_t`, its first virtual function.
    unsafe {
        let vtable = *(frame as *const *const usize);
        core::mem::transmute::<usize, unsafe extern "C" fn(*const c_void) -> u32>(*vtable)(frame)
    }
}

fn copy_options(s: &mut State, options: *const DlssOptions, v: u32) {
    s.engine_options = code::read::<DlssOptions>(options as usize)
        .filter(|o| o.base.struct_version >= 1)
        .map(|o| (o, v));
}
fn copy_tags(s: &mut State, v: u32, list: *const ResourceTag, count: u32) {
    let t = &mut s.tags;
    // A new viewport starts a new set; the same one adds to it, a type tagged again replaces its entry.
    if !t.valid || t.viewport != v {
        t.count = 0;
        t.viewport = v;
    }
    for i in 0..count as usize {
        let Some(src) =
            code::read::<ResourceTag>(list as usize + i * core::mem::size_of::<ResourceTag>())
        else {
            t.valid = false;
            return;
        };
        if src.base.struct_type != RESOURCE_TAG
            || src.base.struct_version != 1
            || src.resource.is_null()
        {
            continue;
        }
        let Some(resource) = code::read::<Resource>(src.resource as usize) else {
            t.valid = false;
            return;
        };
        if resource.base.struct_type != RESOURCE || resource.base.struct_version != 1 {
            continue;
        }
        let mut slot = 0;
        while slot < t.count && t.tag[slot].ty != src.ty {
            slot += 1;
        }
        if slot == MAX_TAGS {
            continue;
        }
        t.tag[slot] = src;
        t.resource[slot] = resource;
        t.resource[slot].base.next = core::ptr::null_mut();
        t.tag[slot].resource = &mut t.resource[slot];
        t.tag[slot].base.next = core::ptr::null_mut();
        if slot == t.count {
            t.count += 1;
        }
    }
    t.valid = t.count != 0;
}
fn copy_constants(s: &mut State, values: *const Constants, frame: u32, v: u32) {
    let Some(mut c) = code::read::<Constants>(values as usize) else {
        s.constants = None;
        return;
    };
    if c.base.struct_type != CONSTANTS || !(1..=2).contains(&c.base.struct_version) {
        return;
    }
    c.base.next = core::ptr::null_mut();
    s.constants = Some((c, frame, v));
}
fn copy_rr_options(s: &mut State, options: *const DlssdOptions, v: u32) {
    let mut bytes = [0u8; 224];
    if !code::read_bytes(options as usize, &mut bytes) {
        s.engine_rr_options = None;
        return;
    }
    let o = bytes.as_mut_ptr() as *mut DlssdOptions;
    // SAFETY: 224 bytes holding the engine's DLSSDOptions.
    unsafe {
        if (*o).base.struct_type != DLSSD_OPTIONS || (*o).base.struct_version < 1 {
            s.engine_rr_options = None;
            return;
        }
        core::ptr::write_unaligned(
            core::ptr::addr_of_mut!((*o).base.next),
            core::ptr::null_mut(),
        );
    }
    s.engine_rr_options = Some((bytes, v));
}

// ---- the hooked Streamline functions

unsafe extern "C" fn tracked_allocate(
    list: *mut c_void,
    feature: Feature,
    viewport: *const ViewportHandle,
) -> SlResult {
    let (_g, s) = lock();
    if feature == FEATURE_DLSS && viewport_value(viewport as *const c_void) == 0 {
        s.original_context.used(list, now());
    }
    let f: AllocateResourcesFn = fn_at(&ALLOCATE_RESOURCES).expect("hooked");
    // SAFETY: Streamline's own function.
    unsafe { f(list, feature, viewport) }
}
unsafe extern "C" fn tracked_free(feature: Feature, viewport: *const ViewportHandle) -> SlResult {
    let (_g, s) = lock();
    let f: FreeResourcesFn = fn_at(&FREE_RESOURCES).expect("hooked");
    // SAFETY: Streamline's own function.
    let result = unsafe { f(feature, viewport) };
    if result == RESULT_OK
        && feature == FEATURE_DLSS
        && viewport_value(viewport as *const c_void) == 0
    {
        s.original_context.released();
    }
    result
}
unsafe extern "C" fn hooked_set_options(
    viewport: *const ViewportHandle,
    options: *const DlssOptions,
) -> SlResult {
    let (_g, s) = lock();
    let v = viewport_value(viewport as *const c_void);
    if !own_viewport(v) && v != crate::scope_dlss::VIEWPORT {
        copy_options(s, options, v);
        debug()[7] += 1;
    }
    let f: SetOptionsFn = fn_at(&SET_OPTIONS).expect("hooked");
    // SAFETY: the trampoline.
    unsafe { f(viewport, options) }
}
unsafe extern "C" fn hooked_set_rr_options(
    viewport: *const ViewportHandle,
    options: *const DlssdOptions,
) -> SlResult {
    let (_g, s) = lock();
    let v = viewport_value(viewport as *const c_void);
    if !own_viewport(v) && v != crate::scope_dlss::VIEWPORT {
        copy_rr_options(s, options, v);
        debug_rr()[3] += 1;
    }
    let f: SetRrOptionsFn = fn_at(&SET_RR_OPTIONS).expect("hooked");
    // SAFETY: the trampoline.
    unsafe { f(viewport, options) }
}

// ---- THE ENGINE'S DLSS AVAILABILITY ABOVE 8192 PIXELS (main's 83ad8989d). The engine asks slDLSSGetOptimalSettings for
// the side-by-side output (both eyes, e.g. 8304x3476 for 4152 per eye) and turns DLSS off when the call fails (2.5: the
// caller of the thunk returns false on a result other than eOk); above 8192 it does, so no tags, options or evaluate
// reach the eyes and they get no upscaled picture. The eyes never evaluate that wide -- each gets half the width -- so
// for a side-by-side frame too wide for DLSS the question is asked for one eye and the answer given back for two: the
// widths doubled, the heights as they are. At 8192 or below nothing changes.
type GetOptimalSettingsFn = unsafe extern "C" fn(*const DlssOptions, *mut DlssOptimalSettings) -> SlResult;
static GET_OPTIMAL_SETTINGS: AtomicUsize = AtomicUsize::new(0);
const DLSS_MAX_OUTPUT_WIDTH: u32 = 8192;
/// 1: above 8192 pixels the engine's question is asked per eye (the default); 0: passed through as asked. Live.
#[no_mangle]
pub static CyberpunkVR_EngineStereoDlssWideQuery: AtomicU32 = AtomicU32::new(1);
/// [0] queries asked per eye, [1] of them answered eOk, [2] passed through, [3] the per-eye answer failed.
#[no_mangle]
pub static CyberpunkVR_DebugEngineStereoDlssWide: [core::sync::atomic::AtomicU64; 4] = [const { core::sync::atomic::AtomicU64::new(0) }; 4];

fn wide_query(options: *const DlssOptions) -> bool {
    let Some(width) = code::read::<u32>(options as usize + core::mem::offset_of!(DlssOptions, output_width)) else { return false };
    CyberpunkVR_EngineStereoDlssWideQuery.load(Ordering::Relaxed) != 0
        && crate::engine_stereo::enabled()
        && width > DLSS_MAX_OUTPUT_WIDTH
        && width % 2 == 0
        && width / 2 <= DLSS_MAX_OUTPUT_WIDTH
}
unsafe extern "C" fn hooked_get_optimal_settings(options: *const DlssOptions, settings: *mut DlssOptimalSettings) -> SlResult {
    let f: GetOptimalSettingsFn = fn_at(&GET_OPTIMAL_SETTINGS).expect("hooked");
    let debug = &CyberpunkVR_DebugEngineStereoDlssWide;
    if !wide_query(options) {
        debug[2].fetch_add(1, Ordering::Relaxed);
        // SAFETY: the trampoline, with the engine's own arguments.
        return unsafe { f(options, settings) };
    }
    // SAFETY: the engine's options, read just now; the chain (next) stays the engine's.
    let asked = unsafe { *options };
    let mut eye = asked;
    eye.output_width = asked.output_width / 2;
    debug[0].fetch_add(1, Ordering::Relaxed);
    // SAFETY: the trampoline, with our copy for one eye and the engine's settings.
    let result = unsafe { f(&eye, settings) };
    static LOGGED: AtomicU32 = AtomicU32::new(0);
    if result != RESULT_OK {
        debug[3].fetch_add(1, Ordering::Relaxed);
        if LOGGED.fetch_add(1, Ordering::Relaxed) < 4 {
            log!(
                "[engine-stereo-dlss] optimal settings for {}x{} asked per eye ({}x{}): failed {}\n",
                asked.output_width,
                asked.output_height,
                eye.output_width,
                eye.output_height,
                result
            );
        }
        return result;
    }
    debug[1].fetch_add(1, Ordering::Relaxed);
    // SAFETY: the engine's settings, filled by DLSS.
    let s = unsafe { &mut *settings };
    let eye_render = s.optimal_render_width;
    s.optimal_render_width *= 2;
    s.render_width_min *= 2;
    s.render_width_max *= 2;
    if LOGGED.fetch_add(1, Ordering::Relaxed) < 4 {
        log!(
            "[engine-stereo-dlss] optimal settings for {}x{} (wider than DLSS takes) asked per eye: mode {} eye output {}x{} -> render {}x{}, given back as {}x{} (min {} max {})\n",
            asked.output_width,
            asked.output_height,
            asked.mode,
            eye.output_width,
            eye.output_height,
            eye_render,
            s.optimal_render_height,
            s.optimal_render_width,
            s.optimal_render_height,
            s.render_width_min,
            s.render_width_max
        );
    }
    result
}

fn interposer() -> Option<windows::Win32::Foundation::HMODULE> {
    // SAFETY: a plain Win32 call.
    unsafe { GetModuleHandleW(PCWSTR(windows::core::w!("sl.interposer.dll").as_ptr())) }.ok()
}
fn proc(module: windows::Win32::Foundation::HMODULE, name: &'static [u8]) -> usize {
    // SAFETY: a NUL-terminated name.
    unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) }.map_or(0, |f| f as usize)
}
fn feature_function(
    module: windows::Win32::Foundation::HMODULE,
    feature: Feature,
    name: &'static [u8],
) -> usize {
    let get = proc(module, b"slGetFeatureFunction\0");
    if get == 0 {
        return 0;
    }
    let mut target: *mut c_void = core::ptr::null_mut();
    // SAFETY: Streamline's slGetFeatureFunction.
    let r = unsafe {
        core::mem::transmute::<usize, GetFeatureFunctionFn>(get)(
            feature,
            name.as_ptr(),
            &mut target,
        )
    };
    if r != RESULT_OK {
        return 0;
    }
    target as usize
}

static RR_INSTALL: Mutex<()> = Mutex::new(());
/// slDLSSDSetOptions is reachable once the RR plugin is loaded: tried at RR evaluates until it is hooked.
fn install_rr_options_hook() {
    if RR_OPTIONS_HOOKED.load(Ordering::Acquire) {
        return;
    }
    let _g = RR_INSTALL.lock().unwrap_or_else(|e| e.into_inner());
    if RR_OPTIONS_HOOKED.load(Ordering::Relaxed) {
        return;
    }
    let Some(module) = interposer() else { return };
    let target = feature_function(module, FEATURE_DLSS_RR, b"slDLSSDSetOptions\0");
    if target == 0 {
        return;
    }
    let mut original: *mut c_void = core::ptr::null_mut();
    // SAFETY: MinHook on Streamline's export.
    let ok = unsafe {
        MH_CreateHook(
            target as *mut c_void,
            hooked_set_rr_options as *const () as *mut c_void,
            &mut original,
        ) == 0
            && MH_EnableHook(target as *mut c_void) == 0
    };
    if !ok {
        // SAFETY: as above.
        unsafe { MH_RemoveHook(target as *mut c_void) };
        SET_RR_OPTIONS.store(0, Ordering::Release);
        log!("[engine-stereo-dlss] slDLSSDSetOptions could not be hooked; Ray Reconstruction stays over both eyes\n");
    } else {
        SET_RR_OPTIONS.store(original as usize, Ordering::Release);
        log!("[engine-stereo-dlss] slDLSSDSetOptions hooked\n");
    }
    RR_OPTIONS_HOOKED.store(true, Ordering::Release);
}

static INSTALL: Mutex<(bool, bool)> = Mutex::new((false, false));
/// The options and lifecycle hooks (slDLSSSetOptions, slAllocateResources, slFreeResources).
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_dlss_install_options_hook() {
    if OPTIONS_HOOKED.load(Ordering::Acquire) {
        return;
    }
    let mut hooked = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
    if OPTIONS_HOOKED.load(Ordering::Relaxed) {
        return;
    }
    let module = interposer();
    if let Some(module) = module {
        let hook = |slot: &AtomicUsize, name: &'static [u8], detour: usize| -> Option<bool> {
            if slot.load(Ordering::Acquire) != 0 {
                return None;
            }
            let target = proc(module, name);
            let mut original: *mut c_void = core::ptr::null_mut();
            // SAFETY: MinHook on Streamline's export.
            if target != 0
                && unsafe {
                    MH_CreateHook(target as *mut c_void, detour as *mut c_void, &mut original)
                } == 0
            {
                slot.store(original as usize, Ordering::Release);
                // SAFETY: as above.
                return Some(unsafe { MH_EnableHook(target as *mut c_void) } == 0);
            }
            None
        };
        if let Some(ok) = hook(
            &ALLOCATE_RESOURCES,
            b"slAllocateResources\0",
            tracked_allocate as *const () as usize,
        ) {
            hooked.0 = ok;
        }
        if let Some(ok) = hook(
            &FREE_RESOURCES,
            b"slFreeResources\0",
            tracked_free as *const () as usize,
        ) {
            hooked.1 = ok;
        }
        LIFECYCLE_HOOKS_READY.store(hooked.0 && hooked.1, Ordering::Release);
    }
    let target = module.map_or(0, |m| {
        feature_function(m, FEATURE_DLSS, b"slDLSSSetOptions\0")
    });
    if target == 0 {
        return;
    }
    let mut original: *mut c_void = core::ptr::null_mut();
    // SAFETY: MinHook on Streamline's export.
    let ok = unsafe {
        MH_CreateHook(
            target as *mut c_void,
            hooked_set_options as *const () as *mut c_void,
            &mut original,
        ) == 0
            && MH_EnableHook(target as *mut c_void) == 0
    };
    if !ok {
        // Not hookable: still callable for the eyes, and the options derived.
        // SAFETY: as above.
        unsafe { MH_RemoveHook(target as *mut c_void) };
        SET_OPTIONS.store(target, Ordering::Release);
        log!(
            "[engine-stereo-dlss] slDLSSSetOptions could not be hooked; eye options are derived\n"
        );
    } else {
        SET_OPTIONS.store(original as usize, Ordering::Release);
        OPTIONS_INTERCEPTED.store(true, Ordering::Release);
        log!("[engine-stereo-dlss] slDLSSSetOptions hooked\n");
    }
    let target = module.map_or(0, |m| feature_function(m, FEATURE_DLSS, b"slDLSSGetOptimalSettings\0"));
    if target != 0 {
        let mut original: *mut c_void = core::ptr::null_mut();
        // SAFETY: MinHook on Streamline's export.
        let ok = unsafe {
            MH_CreateHook(target as *mut c_void, hooked_get_optimal_settings as *const () as *mut c_void, &mut original) == 0
                && MH_EnableHook(target as *mut c_void) == 0
        };
        if ok {
            GET_OPTIMAL_SETTINGS.store(original as usize, Ordering::Release);
            log!("[engine-stereo-dlss] slDLSSGetOptimalSettings hooked (side-by-side wider than {DLSS_MAX_OUTPUT_WIDTH} asked per eye)\n");
        } else {
            // SAFETY: as above.
            unsafe { MH_RemoveHook(target as *mut c_void) };
            log!("[engine-stereo-dlss] slDLSSGetOptimalSettings could not be hooked; DLSS stays off above {DLSS_MAX_OUTPUT_WIDTH} pixels\n");
        }
    }
    OPTIONS_HOOKED.store(true, Ordering::Release);
}

// ---- the right eye's own resources

fn typed(f: DXGI_FORMAT) -> DXGI_FORMAT {
    match f {
        DXGI_FORMAT_R16G16B16A16_TYPELESS => DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R32G32B32A32_TYPELESS => DXGI_FORMAT_R32G32B32A32_FLOAT,
        DXGI_FORMAT_R10G10B10A2_TYPELESS => DXGI_FORMAT_R10G10B10A2_UNORM,
        DXGI_FORMAT_R8G8B8A8_TYPELESS => DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_B8G8R8A8_TYPELESS => DXGI_FORMAT_B8G8R8A8_UNORM,
        DXGI_FORMAT_R16G16_TYPELESS => DXGI_FORMAT_R16G16_FLOAT,
        DXGI_FORMAT_R32G32_TYPELESS => DXGI_FORMAT_R32G32_FLOAT,
        DXGI_FORMAT_R32_TYPELESS => DXGI_FORMAT_R32_FLOAT,
        DXGI_FORMAT_R16_TYPELESS => DXGI_FORMAT_R16_FLOAT,
        DXGI_FORMAT_R8_TYPELESS => DXGI_FORMAT_R8_UNORM,
        DXGI_FORMAT_R8G8_TYPELESS => DXGI_FORMAT_R8G8_UNORM,
        f => f,
    }
}
/// The engine's (or our) resource as an interface value that is never released (no reference of its own).
fn resource_of(p: *mut c_void) -> Option<core::mem::ManuallyDrop<ID3D12Resource>> {
    // SAFETY: a live resource; the wrapper takes no reference and drops none.
    (!p.is_null()).then(|| core::mem::ManuallyDrop::new(unsafe { ID3D12Resource::from_raw(p) }))
}
/// The evaluate's recording list, the same way.
fn list_of(p: *mut c_void) -> Option<core::mem::ManuallyDrop<ID3D12GraphicsCommandList>> {
    // SAFETY: as resource_of.
    (!p.is_null()).then(|| core::mem::ManuallyDrop::new(unsafe { ID3D12GraphicsCommandList::from_raw(p) }))
}
fn keep_copy(list: *mut c_void, copy: &EyeCopy) -> bool {
    let Some(l) = list_of(list) else { return false };
    let l: &ID3D12GraphicsCommandList = &l;
    let resource = copy.resource.unwrap_or(core::ptr::null_mut());
    if cvr_gpu::command_resources::has(l, resource)
        && copy
            .staging
            .is_none_or(|s| cvr_gpu::command_resources::has(l, s))
    {
        return true;
    }
    match copy.staging {
        Some(s) => cvr_gpu::command_resources::keep(l, &[resource, s]),
        None => cvr_gpu::command_resources::keep(l, &[resource]),
    }
}
fn release(p: Option<*mut c_void>) {
    if let Some(p) = p {
        // SAFETY: our own reference, taken at creation.
        drop(unsafe { ID3D12Resource::from_raw(p) });
    }
}

/// The eye's copy for a tag of this type: the source's format at the eye's size, made or remade as needed.
fn own_copy(
    s: &mut State,
    eye: usize,
    ty: BufferType,
    source: *mut c_void,
    width: u64,
    height: u32,
    output: bool,
) -> Option<usize> {
    let mut d = desc_of(source)?;
    let name = if eye != 0 { "right" } else { "left" };
    let copies = &mut s.copies[eye];
    let slot = copies
        .iter()
        .position(|c| (c.resource.is_some() || c.logged) && c.ty == ty)
        .or_else(|| {
            copies
                .iter()
                .position(|c| c.resource.is_none() && !c.logged)
        })?;
    let c = &mut copies[slot];
    c.ty = ty;
    let depth = d.Flags.contains(D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);
    if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || (depth && (output || d.MipLevels != 1 || d.DepthOrArraySize != 1))
    {
        if !c.logged {
            log!(
                "[engine-stereo-dlss] {} eye tag {}: format {} flags 0x{:X} not copied; offset kept\n",
                name,
                ty,
                d.Format.0,
                d.Flags.0 as u32
            );
        }
        c.logged = true;
        return None;
    }
    let format = typed(d.Format);
    if c.resource.is_some()
        && c.width == width
        && c.height == height
        && c.format == format
        && c.output == output
        && c.staging.is_some() == depth
    {
        return Some(slot);
    }
    release(c.resource.take());
    release(c.staging.take());
    c.cache_bytes = 0;
    c.release_after_evaluate = true;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: the engine's live resource.
    unsafe { resource_of(source)?.GetDevice(&mut device) }.ok()?;
    let device = device?;
    let source_format = d.Format;
    let mut staging: Option<ID3D12Resource> = None;
    let mut staging_bytes = 0u64;
    // SAFETY: a plain query of the device.
    let allocation = |desc: &D3D12_RESOURCE_DESC| match unsafe { device.GetResourceAllocationInfo(0, &[*desc]) }.SizeInBytes {
        u64::MAX => 0,
        n => n,
    };
    if depth {
        let mut whole = d;
        whole.Flags = D3D12_RESOURCE_FLAG_NONE;
        whole.Alignment = 0;
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        // SAFETY: the device's own call.
        let hr = unsafe {
            device.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &whole,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut staging,
            )
        };
        log!(
            "[engine-stereo-dlss] {} eye tag {}: whole-depth staging {}x{} format {}: 0x{:08X}\n",
            name,
            ty,
            whole.Width,
            whole.Height,
            whole.Format.0,
            hr.as_ref().err().map_or(0, |e: &windows::core::Error| e.code().0) as u32
        );
        if hr.is_err() || staging.is_none() {
            return None;
        }
        staging_bytes = allocation(&whole);
        // SAFETY: our resource.
        let _ = unsafe {
            staging.as_ref().unwrap().SetName(windows::core::w!(
                "CyberpunkVR right eye DLSS depth staging"
            ))
        };
    }
    d.Width = width;
    d.Height = height;
    d.DepthOrArraySize = 1;
    d.MipLevels = 1;
    d.Alignment = 0;
    d.Format = format;
    d.SampleDesc = DXGI_SAMPLE_DESC {
        Count: 1,
        Quality: 0,
    };
    d.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
    d.Flags = if output {
        D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
    } else {
        D3D12_RESOURCE_FLAG_NONE
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let state = if output {
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS
    } else {
        READABLE
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: the device's own call.
    let hr = unsafe {
        device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, state, None, &mut resource)
    };
    log!(
        "[engine-stereo-dlss] {} eye {} tag {}: {}x{} format {} (source format {}): 0x{:08X}\n",
        name,
        if output { "output" } else { "input" },
        ty,
        width,
        height,
        format.0,
        source_format.0,
        hr.as_ref().err().map_or(0, |e: &windows::core::Error| e.code().0) as u32
    );
    let (Ok(()), Some(resource)) = (hr, resource) else {
        return None;
    };
    // SAFETY: our resource.
    let _ = unsafe {
        resource.SetName(if output {
            windows::core::w!("CyberpunkVR right eye DLSS output")
        } else {
            windows::core::w!("CyberpunkVR right eye DLSS input")
        })
    };
    let resource_bytes = allocation(&d);
    let raw = resource.into_raw();
    c.resource = Some(raw);
    c.staging = staging.map(|s| s.into_raw());
    c.cache_bytes = staging_bytes + resource_bytes;
    CyberpunkVR_EngineStereoCopyMemory[6 + eye].fetch_add(c.cache_bytes, Ordering::Relaxed);
    c.tag = Resource::tex2d(raw, state.0 as u32);
    c.width = width;
    c.height = height;
    c.format = format;
    c.state = state;
    c.output = output;
    Some(slot)
}

fn transition(
    r: *mut c_void,
    from: D3D12_RESOURCE_STATES,
    to: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe {
                    core::mem::transmute::<
                        *mut c_void,
                        core::mem::ManuallyDrop<Option<ID3D12Resource>>,
                    >(r)
                },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: from,
                StateAfter: to,
            }),
        },
    }
}
fn location(r: *mut c_void) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        // SAFETY: as transition.
        pResource: unsafe {
            core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(r)
        },
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    }
}

/// Copies between regions of engine resources and eye copies, one way, everything returned to its state.
#[derive(Clone, Copy)]
struct CopyJob {
    engine: *mut c_void,
    engine_state: D3D12_RESOURCE_STATES,
    own: EyeCopy,
    half: Extent,
}
fn run_copies(list: *mut c_void, jobs: &[CopyJob], into_engine: bool) {
    let (Some(l), false) = (list_of(list), jobs.is_empty()) else {
        return;
    };
    let l: &ID3D12GraphicsCommandList = &l;
    let engine_to = if into_engine {
        D3D12_RESOURCE_STATE_COPY_DEST
    } else {
        D3D12_RESOURCE_STATE_COPY_SOURCE
    };
    let own_to = if into_engine {
        D3D12_RESOURCE_STATE_COPY_SOURCE
    } else {
        D3D12_RESOURCE_STATE_COPY_DEST
    };
    let own = |j: &CopyJob| j.own.resource.unwrap_or(core::ptr::null_mut());
    let mut b: Vec<D3D12_RESOURCE_BARRIER> = Vec::with_capacity(3 * MAX_TAGS);
    for j in jobs {
        if j.engine_state != engine_to {
            b.push(transition(j.engine, j.engine_state, engine_to));
        }
        b.push(transition(own(j), j.own.state, own_to));
    }
    // SAFETY: the evaluate's recording list and the resources above.
    unsafe { l.ResourceBarrier(&b) };
    // depth: the whole resource into its staging copy first, which the half is then taken from
    b.clear();
    for j in jobs {
        let Some(staging) = j.own.staging.filter(|_| !into_engine) else {
            continue;
        };
        let (whole, from) = (location(staging), location(j.engine));
        // SAFETY: as above.
        unsafe { l.CopyTextureRegion(&whole, 0, 0, 0, &from, None) };
        b.push(transition(
            staging,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        ));
    }
    if !b.is_empty() {
        // SAFETY: as above.
        unsafe { l.ResourceBarrier(&b) };
    }
    for j in jobs {
        let engine = location(match j.own.staging {
            Some(s) if !into_engine => s,
            _ => j.engine,
        });
        let target = location(own(j));
        let h = j.half;
        // SAFETY: as above.
        unsafe {
            if into_engine {
                let bx = D3D12_BOX {
                    left: 0,
                    top: 0,
                    front: 0,
                    right: h.width,
                    bottom: h.height,
                    back: 1,
                };
                l.CopyTextureRegion(&engine, h.left, h.top, 0, &target, Some(&bx));
            } else {
                let bx = D3D12_BOX {
                    left: h.left,
                    top: h.top,
                    front: 0,
                    right: h.left + h.width,
                    bottom: h.top + h.height,
                    back: 1,
                };
                l.CopyTextureRegion(&target, 0, 0, 0, &engine, Some(&bx));
            }
        }
    }
    b.clear();
    for j in jobs {
        if j.engine_state != engine_to {
            b.push(transition(j.engine, engine_to, j.engine_state));
        }
        b.push(transition(own(j), own_to, j.own.state));
        if let Some(staging) = j.own.staging.filter(|_| !into_engine) {
            b.push(transition(
                staging,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_COPY_DEST,
            ));
        }
    }
    // SAFETY: as above.
    unsafe { l.ResourceBarrier(&b) };
}

// ---- the split

/// The eye's camera: `e` (world) from the centre, in cameraViewToWorld's origin and -e in worldToCameraView's, whichever
/// way round the engine wrote them; false when neither is.
fn move_view_matrices(o: &mut DlssdOptions, position: &Float3, e: &[f32; 3]) -> bool {
    let p = [position.x, position.y, position.z];
    let close = |a: f32, b: f32| (a - b).abs() < 0.01 + 1e-5 * b.abs();
    let w = &mut o.world_to_camera_view;
    let v = &mut o.camera_view_to_world;
    if close(v[3].x, p[0]) && close(v[3].y, p[1]) && close(v[3].z, p[2]) {
        let d = [
            e[0] * w[0].x + e[1] * w[1].x + e[2] * w[2].x,
            e[0] * w[0].y + e[1] * w[1].y + e[2] * w[2].y,
            e[0] * w[0].z + e[1] * w[1].z + e[2] * w[2].z,
        ];
        v[3].x += e[0];
        v[3].y += e[1];
        v[3].z += e[2];
        w[3].x -= d[0];
        w[3].y -= d[1];
        w[3].z -= d[2];
        return true;
    }
    if close(v[0].w, p[0]) && close(v[1].w, p[1]) && close(v[2].w, p[2]) {
        for row in w.iter_mut().take(3) {
            row.w -= row.x * e[0] + row.y * e[1] + row.z * e[2];
        }
        v[0].w += e[0];
        v[1].w += e[1];
        v[2].w += e[2];
        return true;
    }
    false
}

static RR_LOGGED: AtomicBool = AtomicBool::new(false);
fn log_rr_once(c: &Constants, o: &DlssdOptions) {
    if RR_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    let row = |name: &str, m: &Float4x4| {
        let f = |v: f32| v as f64;
        log!(
            "[engine-stereo-dlss]   {} ({:.4} {:.4} {:.4} {:.4}) ({:.4} {:.4} {:.4} {:.4}) ({:.4} {:.4} {:.4} {:.4}) ({:.4} {:.4} {:.4} {:.4})\n",
            name,
            f(m[0].x),
            f(m[0].y),
            f(m[0].z),
            f(m[0].w),
            f(m[1].x),
            f(m[1].y),
            f(m[1].z),
            f(m[1].w),
            f(m[2].x),
            f(m[2].y),
            f(m[2].z),
            f(m[2].w),
            f(m[3].x),
            f(m[3].y),
            f(m[3].z),
            f(m[3].w)
        );
    };
    log!(
        "[engine-stereo-dlss] per-eye RR: options v{} mode {} output {}x{} normalRoughness {}; camera pos ({:.3} {:.3} {:.3}) right ({:.3} {:.3} {:.3}) fov {:.4} aspect {:.4}; eye separation {:.4}\n",
        o.base.struct_version as u32,
        o.mode,
        o.output_width,
        o.output_height,
        o.normal_roughness_mode,
        c.camera_pos.x as f64,
        c.camera_pos.y as f64,
        c.camera_pos.z as f64,
        c.camera_right.x as f64,
        c.camera_right.y as f64,
        c.camera_right.z as f64,
        c.camera_fov as f64,
        c.camera_aspect_ratio as f64,
        crate::engine_stereo::eye_separation() as f64
    );
    row("viewToClip", &c.camera_view_to_clip);
    row("worldToView", &o.world_to_camera_view);
    row("viewToWorld", &o.camera_view_to_world);
}

fn same(a: &DlssOptions, b: &DlssOptions) -> bool {
    a.mode == b.mode
        && a.output_width == b.output_width
        && a.output_height == b.output_height
        && a.pre_exposure == b.pre_exposure
        && a.exposure_scale == b.exposure_scale
        && a.color_buffers_hdr == b.color_buffers_hdr
        && a.dlaa_preset == b.dlaa_preset
        && a.quality_preset == b.quality_preset
        && a.balanced_preset == b.balanced_preset
        && a.performance_preset == b.performance_preset
        && a.ultra_performance_preset == b.ultra_performance_preset
        && a.ultra_quality_preset == b.ultra_quality_preset
        && a.use_auto_exposure == b.use_auto_exposure
        && a.alpha_upscaling_enabled == b.alpha_upscaling_enabled
        && a.base.struct_version == b.base.struct_version
}
/// The extent a tag covers: its own, or the whole resource.
fn area(tag: &ResourceTag) -> Option<Extent> {
    if tag.extent.width != 0 && tag.extent.height != 0 {
        return Some(tag.extent);
    }
    // SAFETY: the tag's resource is the set's own copy.
    let native = unsafe { tag.resource.as_ref() }.map_or(core::ptr::null_mut(), |r| r.native);
    let d = desc_of(native)?;
    let e = Extent {
        top: 0,
        left: 0,
        width: d.Width as u32,
        height: d.Height,
    };
    (e.width != 0 && e.height != 0).then_some(e)
}
fn half(e: &Extent, eye: usize) -> Extent {
    let w = e.width / 2;
    Extent {
        top: e.top,
        left: e.left + if eye != 0 { w } else { 0 },
        width: w,
        height: e.height,
    }
}
fn native_of(tag: &ResourceTag) -> *mut c_void {
    // SAFETY: as area.
    unsafe { tag.resource.as_ref() }.map_or(core::ptr::null_mut(), |r| r.native)
}
/// Whether the right eye has its own output target, made and kept for this command list.
fn right_output_ready(s: &mut State, out: &Extent, list: *mut c_void) -> bool {
    for i in 0..s.tags.count {
        let tag = s.tags.tag[i];
        if tag.ty != BUFFER_SCALING_OUTPUT_COLOR || tag.resource.is_null() {
            continue;
        }
        let Some(a) = area(&tag).filter(|a| a.width == out.width && a.height == out.height) else {
            continue;
        };
        let native = native_of(&tag);
        for j in 0..s.tags.count {
            if j != i && !s.tags.tag[j].resource.is_null() && native_of(&s.tags.tag[j]) == native {
                return false;
            }
        }
        let h = half(&a, 1);
        let own = if !native.is_null() && !fail_test(1) {
            own_copy(s, 1, tag.ty, native, h.width as u64, h.height, true)
        } else {
            None
        };
        return own.is_some_and(|slot| keep_copy(list, &s.copies[1][slot]));
    }
    false
}
/// Options when the engine set its own before the hook went in: the mode from the scale, the rest the defaults of an
/// HDR colour pipeline that tags its exposure.
fn derived(input: &Extent, out: &Extent, exposure_tagged: bool) -> DlssOptions {
    let mut o = DlssOptions::default();
    let scale = input.width as f64 / out.width as f64;
    o.mode = if scale > 0.95 {
        DLSS_MODE_DLAA
    } else if scale > 0.62 {
        DLSS_MODE_MAX_QUALITY
    } else if scale > 0.54 {
        DLSS_MODE_BALANCED
    } else if scale > 0.42 {
        DLSS_MODE_MAX_PERFORMANCE
    } else {
        DLSS_MODE_ULTRA_PERFORMANCE
    };
    o.color_buffers_hdr = TRUE;
    o.use_auto_exposure = if exposure_tagged { FALSE } else { TRUE };
    o
}
static LOGGED_ONCE: AtomicBool = AtomicBool::new(false);
fn log_once(
    t: &TagSet,
    c: &Constants,
    o: &DlssOptions,
    recorded: bool,
    input: &Extent,
    out: &Extent,
) {
    if LOGGED_ONCE.swap(true, Ordering::Relaxed) {
        return;
    }
    log!(
        "[engine-stereo-dlss] per-eye DLSS: viewport {} -> {}/{}, input {}x{} at ({},{}), output {}x{} at ({},{})\n",
        t.viewport,
        EYE_VIEWPORT[0],
        EYE_VIEWPORT[1],
        input.width,
        input.height,
        input.left,
        input.top,
        out.width,
        out.height,
        out.left,
        out.top
    );
    for i in 0..t.count {
        let tag = &t.tag[i];
        let a = area(tag).unwrap_or_default();
        // SAFETY: the set's own resource copy.
        let state = unsafe { tag.resource.as_ref() }.map_or(0, |r| r.state);
        log!(
            "[engine-stereo-dlss]   tag type {} lifecycle {} extent {}x{} at ({},{}) resource {}x{} state 0x{:X}\n",
            tag.ty,
            tag.lifecycle,
            tag.extent.width,
            tag.extent.height,
            tag.extent.left,
            tag.extent.top,
            a.width,
            a.height,
            state
        );
    }
    log!(
        "[engine-stereo-dlss]   constants jitter ({:.4},{:.4}) mvecScale ({:.6},{:.6}) depthInverted {} mvJittered {} cameraMotion {} mv3D {} reset {}\n",
        c.jitter_offset.x as f64,
        c.jitter_offset.y as f64,
        c.mvec_scale.x as f64,
        c.mvec_scale.y as f64,
        c.depth_inverted as i8,
        c.motion_vectors_jittered as i8,
        c.camera_motion_included as i8,
        c.motion_vectors_3d as i8,
        c.reset as i8
    );
    log!(
        "[engine-stereo-dlss]   options ({}) v{} mode {} output {}x{} preExposure {:.3} exposureScale {:.3} hdr {} autoExposure {} presets {}/{}/{}/{}/{}/{}\n",
        if recorded { "the engine's" } else { "derived" },
        o.base.struct_version as u64,
        o.mode,
        o.output_width,
        o.output_height,
        o.pre_exposure as f64,
        o.exposure_scale as f64,
        o.color_buffers_hdr as i8,
        o.use_auto_exposure as i8,
        o.dlaa_preset,
        o.quality_preset,
        o.balanced_preset,
        o.performance_preset,
        o.ultra_performance_preset,
        o.ultra_quality_preset
    );
}

/// RecordTags: the engine's tags for its viewport, as slSetTag passes them.
///
/// # Safety
/// Streamline's arguments as the slSetTag hook received them.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_dlss_record_tags(
    viewport: *const c_void,
    list: *const c_void,
    count: u32,
    original: Option<SetTagFn>,
) {
    if !crate::engine_stereo::enabled() || viewport.is_null() || list.is_null() || count == 0 {
        return;
    }
    let v = viewport_value(viewport);
    if own_viewport(v) || v == crate::scope_dlss::VIEWPORT {
        return;
    }
    let (_g, s) = lock();
    s.set_tag = original;
    copy_tags(s, v, list as *const ResourceTag, count);
}
/// RecordConstants: the engine's constants for its viewport and frame.
///
/// # Safety
/// Streamline's arguments as slSetConstants received them.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_dlss_record_constants(
    values: *const Constants,
    frame: *const c_void,
    viewport: *const ViewportHandle,
) {
    if !crate::engine_stereo::enabled() {
        return;
    }
    let v = viewport_value(viewport as *const c_void);
    if own_viewport(v) || v == crate::scope_dlss::VIEWPORT {
        return;
    }
    let (_g, s) = lock();
    copy_constants(s, values, frame_value(frame), v);
}

/// Evaluate: true when the evaluate was split per eye (its result in `result`); false to run the engine's evaluate.
///
/// # Safety
/// Streamline's arguments as slEvaluateFeature received them, with the original evaluate and slSetConstants.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_dlss_evaluate(
    feature: Feature,
    frame: *const c_void,
    inputs: *const *const BaseStructure,
    count: u32,
    list: *mut c_void,
    evaluate: Option<EvaluateFn>,
    set_constants: Option<ConstantsFn>,
    result: *mut SlResult,
) -> bool {
    // SAFETY: exports of the C++ half.
    let rr = feature == FEATURE_DLSS_RR;
    let per_eye = unsafe {
        if rr {
            &CyberpunkVR_EngineStereoRrPerEye
        } else {
            &CyberpunkVR_EngineStereoDlssPerEye
        }
    };
    // A frame the engine rendered mono is one picture: one evaluate over it, as the engine's.
    let (Some(evaluate), Some(set_constants)) = (evaluate, set_constants) else {
        if rr {
            debug_rr()[1] += 1
        } else {
            debug()[1] += 1
        }
        return false;
    };
    if (feature != FEATURE_DLSS && !rr)
        || !crate::engine_stereo::enabled()
        || !crate::engine_stereo::rendering()
        || per_eye.load(Ordering::Relaxed) == 0
        || inputs.is_null()
        || count == 0
        || count > 15
        || list.is_null()
        || result.is_null()
    {
        if rr {
            debug_rr()[1] += 1
        } else {
            debug()[1] += 1
        }
        return false;
    }
    cvr_engine_stereo_dlss_install_options_hook();
    if rr {
        install_rr_options_hook();
    }
    let found = find_viewport(inputs, count);
    let (_g, s) = lock();
    let set_options: Option<SetOptionsFn> = fn_at(&SET_OPTIONS);
    let (Some((at, viewport)), Some(set_tag), Some(set_options)) = (
        found.filter(|&(_, v)| !own_viewport(v) && v != crate::scope_dlss::VIEWPORT),
        s.set_tag,
        set_options,
    ) else {
        debug()[1] += 1;
        return false;
    };
    let set_rr_options: Option<SetRrOptionsFn> = fn_at(&SET_RR_OPTIONS);
    if rr {
        let engine_rr_ok = s.engine_rr_options.as_ref().is_some_and(|(bytes, v)| {
            // SAFETY: the engine's options, as copied.
            *v == viewport
                && unsafe { (*(bytes.as_ptr() as *const DlssdOptions)).mode } != DLSS_MODE_OFF
        });
        if set_rr_options.is_none() || !engine_rr_ok {
            debug_rr()[1] += 1;
            return false;
        }
    }
    if !s.tags.valid || s.tags.viewport != viewport {
        debug()[2] += 1;
        return false;
    }
    let frame_number = frame_value(frame);
    let Some((constants, _, _)) = s
        .constants
        .filter(|&(_, f, v)| v == viewport && f == frame_number)
    else {
        debug()[3] += 1;
        return false;
    };
    let (mut input, mut out) = (Extent::default(), Extent::default());
    let (mut have_in, mut have_out, mut exposure_tagged) = (false, false, false);
    for i in 0..s.tags.count {
        let tag = &s.tags.tag[i];
        if tag.ty == BUFFER_SCALING_INPUT_COLOR {
            match area(tag) {
                Some(a) => {
                    input = a;
                    have_in = true;
                }
                None => have_in = false,
            }
        } else if tag.ty == BUFFER_SCALING_OUTPUT_COLOR {
            match area(tag) {
                Some(a) => {
                    out = a;
                    have_out = true;
                }
                None => have_out = false,
            }
        } else if tag.ty == BUFFER_EXPOSURE {
            exposure_tagged = true;
        }
    }
    if !have_in
        || !have_out
        || input.width < 2
        || out.width < 2
        || input.width % 2 != 0
        || out.width % 2 != 0
    {
        debug()[4] += 1;
        return false;
    }
    // SAFETY: exports of the C++ half.
    let mode = unsafe { CyberpunkVR_EngineStereoDlssMode.load(Ordering::Relaxed) };
    // THE RIGHT EYE'S OUTPUT TARGET BEFORE ANY EYE IS EVALUATED: without one the frame goes to the engine's one
    // evaluate over both eyes, and the target is tried again 10 s later.
    if mode & (2 | 4 | 16) == 0 {
        let t = now();
        if t < s.output_retry_at {
            debug()[1] += 1;
            return false;
        }
        if !right_output_ready(s, &out, list) {
            s.output_retry_at = t + 10000;
            // SAFETY: an export of the C++ half.
            unsafe { (*(&raw mut CyberpunkVR_DebugEngineStereoDlssFallback))[0] += 1 };
            if !s.output_missing_logged {
                log!("[engine-stereo-dlss] no output target of the right eye's own: both eyes go through the engine's one evaluate, another try every 10 s\n");
            }
            s.output_missing_logged = true;
            debug()[1] += 1;
            return false;
        }
    }
    let recorded = s.engine_options.is_some_and(|(_, v)| v == viewport);
    let base = match s.engine_options {
        Some((o, v)) if v == viewport => o,
        _ => derived(&input, &out, exposure_tagged),
    };
    let engine_rr = s.engine_rr_options.map(|(bytes, _)| bytes);
    if rr {
        // SAFETY: the engine's options, as copied.
        log_rr_once(&constants, unsafe {
            &*(engine_rr.as_ref().unwrap().as_ptr() as *const DlssdOptions)
        });
    } else {
        log_once(&s.tags, &constants, &base, recorded, &input, &out);
    }
    // Only the observed engine viewport (0). A live recording or unfinished GPU submission owns a marker, so elapsed
    // frames alone can never allow release.
    if !rr
        && viewport == 0
        && recorded
        && OPTIONS_INTERCEPTED.load(Ordering::Acquire)
        && LIFECYCLE_HOOKS_READY.load(Ordering::Acquire)
        && s.original_context.ready(now())
    {
        let original = ViewportHandle::new(0);
        let free: FreeResourcesFn = fn_at(&FREE_RESOURCES).expect("hooked");
        // SAFETY: Streamline's own function.
        let released = unsafe { free(FEATURE_DLSS, &original) };
        log!(
            "[engine-stereo-dlss-reclaim] viewport=0 result={} pending={} tick_ms={}\n",
            released,
            s.original_context.pending(),
            now()
        );
        if released == RESULT_OK {
            s.original_context.released();
            s.original_reclaimed = true;
        } else {
            s.original_context.disable();
        }
    }

    static LOGGED: AtomicU32 = AtomicU32::new(0);
    let log_now = LOGGED.fetch_add(1, Ordering::Relaxed) < 3;
    if log_now {
        let mut line = format!(
            "[engine-stereo-dlss] evaluate frame {} viewport {} inputs {} (mode {}):",
            frame_number, viewport, count, mode
        );
        for i in 0..count as usize {
            let p = code::read::<usize>(inputs as usize + i * 8).unwrap_or(0);
            let data1 = if p != 0 {
                struct_type_at(p).map_or(0, |t| t.data1)
            } else {
                0
            };
            line.push_str(&format!(" {:08X}", data1));
        }
        line.push('\n');
        log!("{}", line);
    }
    let (mut jitter_x, mut jitter_y) = (1.0f32, 1.0f32);
    // SAFETY: exports of the C++ half.
    unsafe {
        let (jw, jh) = crate::engine_stereo::eye_jitter_size();
        if CyberpunkVR_EngineStereoDlssEyeJitter.load(Ordering::Relaxed) != 0 && jw > 1 && jh > 1 {
            jitter_x = (input.width / 2) as f32 / jw as f32;
            jitter_y = input.height as f32 / jh as f32;
        }
        let test = &CyberpunkVR_EngineStereoDlssJitterTest;
        if test[0] != 0.0 {
            jitter_x = test[0];
        }
        if test[1] != 0.0 {
            jitter_y = test[1];
        }
    }
    // SAFETY: as above.
    let motion_x = unsafe {
        let t = CyberpunkVR_EngineStereoDlssJitterTest[2];
        if t != 0.0 {
            t
        } else {
            2.0
        }
    };
    // SAFETY: as above.
    unsafe {
        let j = &mut *(&raw mut CyberpunkVR_DebugEngineStereoDlssJitter);
        *j = [
            constants.jitter_offset.x,
            constants.jitter_offset.y,
            constants.mvec_scale.x,
            constants.mvec_scale.y,
            jitter_x,
            jitter_y,
        ];
    }
    let mut worst = RESULT_OK;
    let memory = memory_diagnostic();
    if memory {
        for c in s.copies.iter_mut().flatten() {
            c.used = false;
        }
    }
    for step in 0..2usize {
        let eye = if mode & 1 != 0 { 1 - step } else { step };
        if eye == 1 && mode & 4 != 0 {
            continue;
        }
        let handle = ViewportHandle::new(if rr {
            RR_EYE_VIEWPORT[eye]
        } else {
            EYE_VIEWPORT[eye]
        });
        // SAFETY: an export of the C++ half.
        let res = unsafe {
            &mut (&mut *(&raw mut CyberpunkVR_DebugEngineStereoDlssResults))[eye * 4..eye * 4 + 4]
        };
        let mut values = constants;
        values.mvec_scale.x *= motion_x;
        values.jitter_offset.x *= jitter_x;
        values.jitter_offset.y *= jitter_y;
        if rr {
            // The eye's camera: the engine places the eyes -+sep/2 along the camera's right from the centre it reports.
            let mut bytes = engine_rr.unwrap();
            let options = bytes.as_mut_ptr() as *mut DlssdOptions;
            // SAFETY: the 224 bytes of the engine's options, edited in place (their padding kept).
            unsafe {
                (*options).output_width = out.width / 2;
                (*options).output_height = out.height;
                if CyberpunkVR_EngineStereoRrEyeCamera.load(Ordering::Relaxed) != 0 {
                    let sep = if eye != 0 { 0.5f32 } else { -0.5 }
                        * crate::engine_stereo::eye_separation();
                    let e = [
                        sep * values.camera_right.x,
                        sep * values.camera_right.y,
                        sep * values.camera_right.z,
                    ];
                    if move_view_matrices(&mut *options, &values.camera_pos, &e) {
                        debug_rr()[4] += 1;
                    } else {
                        debug_rr()[5] += 1;
                    }
                    values.camera_pos.x += e[0];
                    values.camera_pos.y += e[1];
                    values.camera_pos.z += e[2];
                }
            }
            if s.eye_rr_options[eye] != Some(bytes) {
                // SAFETY: Streamline's slDLSSDSetOptions through its trampoline.
                let r = unsafe { set_rr_options.unwrap()(&handle, options) };
                s.eye_rr_options[eye] = (r == RESULT_OK).then_some(bytes);
                res[0] = r;
                debug_rr()[2] += 1;
                if r != RESULT_OK {
                    log!(
                        "[engine-stereo-dlss] eye {} RR options refused ({})\n",
                        eye,
                        r
                    );
                }
            }
        } else {
            let mut options = base;
            options.output_width = out.width / 2;
            options.output_height = out.height;
            if !s.eye_options[eye].is_some_and(|o| same(&o, &options)) {
                // SAFETY: Streamline's slDLSSSetOptions.
                let r = unsafe { set_options(&handle, &options) };
                s.eye_options[eye] = (r == RESULT_OK).then_some(options);
                res[0] = r;
                debug()[5] += 1;
                if r != RESULT_OK {
                    log!("[engine-stereo-dlss] eye {} options refused ({})\n", eye, r);
                }
            }
        }
        // SAFETY: Streamline's slSetConstants.
        res[2] = unsafe { set_constants(&values, frame, &handle) };
        let mut eye_tags = s.tags.tag;
        // Only the scaling output is the evaluate's to write (DLSS-G's HUD-less colour is output-sized but an input).
        let mut before: Vec<CopyJob> = Vec::with_capacity(MAX_TAGS);
        let mut after: Vec<CopyJob> = Vec::with_capacity(MAX_TAGS);
        // The eye copies' tags must stay where the tag list points while Streamline reads it.
        let mut own_tags: [Option<usize>; MAX_TAGS] = [None; MAX_TAGS];
        // SAFETY: an export of the C++ half.
        let copy_left =
            rr && unsafe { CyberpunkVR_EngineStereoRrCopyLeft.load(Ordering::Relaxed) } != 0;
        for i in 0..s.tags.count {
            let tag = s.tags.tag[i];
            let Some(a) = area(&tag) else { continue };
            // The scaling output is told by its type first (under DLAA it is the input's size).
            let is_output = tag.ty == BUFFER_SCALING_OUTPUT_COLOR
                && a.width == out.width
                && a.height == out.height;
            let is_input = !is_output && a.width == input.width && a.height == input.height;
            if is_input {
                eye_tags[i].extent = half(&a, eye);
            } else if is_output {
                eye_tags[i].extent = half(&a, if mode & 2 != 0 { 0 } else { eye });
            }
            // The left eye reads in place (DLSS), or copies of its inputs (RR); its output is written in place.
            if (eye != 1 && !(copy_left && is_input))
                || (!is_input && !is_output)
                || (is_output && mode & 18 != 0)
                || (is_input && mode & 32 != 0)
            {
                continue;
            }
            let native = native_of(&tag);
            let h = eye_tags[i].extent;
            // One resource under two tags: copied once, offset kept.
            let shared = (0..s.tags.count).any(|j| {
                j != i && !s.tags.tag[j].resource.is_null() && native_of(&s.tags.tag[j]) == native
            });
            let own = if !native.is_null() && !shared && !(is_output && fail_test(1)) {
                own_copy(s, eye, tag.ty, native, h.width as u64, h.height, is_output)
            } else {
                None
            };
            let Some(slot) = own else { continue };
            if !keep_copy(list, &s.copies[eye][slot]) {
                // Do not record references whose lifetime we cannot protect.
                static WARNED: AtomicBool = AtomicBool::new(false);
                if !WARNED.load(Ordering::Relaxed) {
                    log!("[engine-stereo-dlss-memory] lifetime registration failed; right-eye copy skipped\n");
                }
                WARNED.store(true, Ordering::Relaxed);
                continue;
            }
            // SAFETY: the set's own resource copy.
            let state = unsafe { (*tag.resource).state };
            let job = CopyJob {
                engine: native,
                engine_state: D3D12_RESOURCE_STATES(state as i32),
                own: s.copies[eye][slot],
                half: h,
            };
            if is_output {
                after.push(job);
            } else {
                before.push(job);
            }
            own_tags[i] = Some(slot);
            if memory {
                s.copies[eye][slot].used = true;
            }
            eye_tags[i].extent = Extent {
                top: 0,
                left: 0,
                width: h.width,
                height: h.height,
            };
        }
        for i in 0..s.tags.count {
            if let Some(slot) = own_tags[i] {
                eye_tags[i].resource = &mut s.copies[eye][slot].tag;
            }
        }
        // The scene GPU profile sees the per-eye DLSS stages as pseudo-nodes 0xD15A00xx.
        let profile_holder = list_of(list);
        let profile_list = profile_holder.as_deref();
        if !before.is_empty() {
            if let Some(l) = profile_list {
                cvr_gpu::scene_profile::mark(l, 0xD15A0010 + eye as u32, 0, 0, 0);
            }
            run_copies(list, &before, false);
        }
        if let Some(l) = profile_list {
            cvr_gpu::scene_profile::mark(l, 0xD15A0020 + eye as u32, 0, 0, 0);
        }
        // SAFETY: exports of the C++ half.
        let drop_tags = unsafe {
            CyberpunkVR_EngineStereoDlssDropTags.load(Ordering::Relaxed) as u64
                | (CyberpunkVR_EngineStereoDlssDropTagsHigh.load(Ordering::Relaxed) as u64) << 32
        };
        for i in 0..s.tags.count {
            if s.tags.tag[i].ty < 64 && (drop_tags >> s.tags.tag[i].ty) & 1 != 0 {
                eye_tags[i].resource = core::ptr::null_mut();
            }
        }
        // SAFETY: Streamline's slSetTag (the original).
        res[1] = unsafe {
            set_tag(
                &handle as *const ViewportHandle as *const c_void,
                eye_tags.as_ptr() as *const c_void,
                s.tags.count as u32,
                list,
            )
        } as i32;
        // A failed call may have taken some tags, and an older viewport may still own a Present-lifetime tag: neither
        // is revoked by a later evaluate-lifetime tag (sl::eOnlyValidNow 0, eValidUntilEvaluate 2).
        for i in 0..s.tags.count {
            if let (Some(slot), false) = (own_tags[i], eye_tags[i].resource.is_null()) {
                let lifecycle = eye_tags[i].lifecycle;
                let c = &mut s.copies[eye][slot];
                c.release_after_evaluate = c.release_after_evaluate && res[1] == RESULT_OK && (lifecycle == 0 || lifecycle == 2);
            }
        }
        let mut eye_inputs: Vec<*const BaseStructure> = Vec::with_capacity(16 + MAX_TAGS);
        for i in 0..count as usize {
            // SAFETY: the evaluate's inputs, read above.
            let p = unsafe { *inputs.add(i) };
            eye_inputs.push(if i == at {
                &handle as *const ViewportHandle as *const BaseStructure
            } else {
                p
            });
        }
        if mode & 8 != 0 {
            for t in eye_tags.iter().take(s.tags.count) {
                eye_inputs.push(t as *const ResourceTag as *const BaseStructure);
            }
        }
        // where this eye is in the resources its evaluate hands NGX (neural_render.rs works on the eye's areas)
        let mut areas = crate::neural_render::EyeAreas::default();
        for (tag, eye_tag) in s.tags.tag.iter().zip(eye_tags.iter()).take(s.tags.count) {
            let slot = match tag.ty {
                BUFFER_SCALING_OUTPUT_COLOR => &mut areas.output,
                BUFFER_DEPTH => &mut areas.depth,
                BUFFER_MOTION_VECTORS => &mut areas.motion,
                _ => continue,
            };
            if let (false, Some(a)) = (eye_tag.resource.is_null(), area(eye_tag)) {
                *slot = crate::neural_render::EyeArea { left: a.left, top: a.top, width: a.width, height: a.height };
            }
        }
        areas.motion_scale = [values.mvec_scale.x, values.mvec_scale.y];
        EVALUATING_AREAS.with(|e| e.set(areas));
        EVALUATING_EYE.with(|e| e.set(eye as i32));
        // SAFETY: Streamline's evaluate (the original).
        let mut r = unsafe {
            evaluate(
                feature,
                frame,
                eye_inputs.as_ptr(),
                eye_inputs.len() as u32,
                list,
            )
        };
        EVALUATING_EYE.with(|e| e.set(-1));
        if eye == 1 && fail_test(2) {
            r = RESULT_ERROR_INVALID_PARAMETER;
        }
        res[3] = r;
        if r == RESULT_OK && !after.is_empty() {
            if let Some(l) = profile_list {
                cvr_gpu::scene_profile::mark(l, 0xD15A0030 + eye as u32, 0, 0, 0);
            }
            run_copies(list, &after, true);
        }
        if let Some(l) = profile_list {
            cvr_gpu::scene_profile::end_range(l);
        }
        if log_now {
            let hi = half(&input, eye);
            let ho = half(&out, if mode & 2 != 0 { 0 } else { eye });
            let hw = half(&out, eye);
            log!(
                "[engine-stereo-dlss]   eye {}: tags {} constants {} evaluate {}; in ({},{} {}x{}) out ({},{} {}x{})\n",
                eye,
                res[1],
                res[2],
                res[3],
                hi.left,
                hi.top,
                hi.width,
                hi.height,
                ho.left,
                ho.top,
                hw.width,
                hw.height
            );
        }
        if r != RESULT_OK {
            worst = r;
            let d = debug();
            if d[6] < 4 {
                log!("[engine-stereo-dlss] eye {} evaluate failed ({})\n", eye, r);
            }
            d[6] += 1;
        }
    }
    if rr {
        debug_rr()[0] += 1;
    } else {
        debug()[0] += 1;
    }
    // AN EYE WHOSE EVALUATE FAILS IS AN EYE LEFT UNDRAWN: three frames in a row and DLSS (or RR) goes back to the
    // engine's one evaluate over both eyes for this session.
    if worst != RESULT_OK {
        s.failed_frames += 1;
        if s.failed_frames >= 3 {
            per_eye.store(0, Ordering::Relaxed);
            // SAFETY: an export of the C++ half.
            unsafe { (*(&raw mut CyberpunkVR_DebugEngineStereoDlssFallback))[1] = 1 };
            log!(
                "[engine-stereo-dlss] the eyes' evaluates keep failing ({}): {} goes back to the engine's one evaluate over both eyes for this session\n",
                worst,
                if rr { "Ray Reconstruction" } else { "DLSS" }
            );
        }
    } else {
        s.failed_frames = 0;
    }
    if memory {
        report_copy_memory(s);
    }
    if !rr && worst == RESULT_OK && viewport == 0 {
        s.original_context.split_succeeded();
    }
    // SAFETY: as the caller vouches.
    unsafe { *result = worst };
    true
}

/// EvaluateOriginal: the engine's evaluate, with the original context's use noted (and its options reasserted when
/// it was reclaimed).
///
/// # Safety
/// As cvr_engine_stereo_dlss_evaluate.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_dlss_evaluate_original(
    feature: Feature,
    frame: *const c_void,
    inputs: *const *const BaseStructure,
    count: u32,
    list: *mut c_void,
    evaluate: EvaluateFn,
) -> SlResult {
    let (_g, s) = lock();
    if feature == FEATURE_DLSS {
        match find_viewport(inputs, count) {
            None => s.original_context.disable(),
            Some((_, 0)) => {
                s.original_context.used(list, now());
                if s.original_reclaimed {
                    // Streamline recreates the feature lazily on evaluate: the engine's options reasserted.
                    if let (Some((o, 0)), Some(set_options)) =
                        (s.engine_options, fn_at::<SetOptionsFn>(&SET_OPTIONS))
                    {
                        let original = ViewportHandle::new(0);
                        // SAFETY: Streamline's slDLSSSetOptions.
                        unsafe { set_options(&original, &o) };
                    }
                    log!("[engine-stereo-dlss-reclaim] viewport=0 resumed; lazy recreation tick_ms={}\n", now());
                    s.original_reclaimed = false;
                }
            }
            Some(_) => {}
        }
    }
    // SAFETY: Streamline's evaluate.
    unsafe { evaluate(feature, frame, inputs, count, list) }
}

/// EvaluatingAreas: where the eye whose evaluate runs on this thread is in the resources its evaluate hands NGX -- its
/// output, depth and motion areas, in pixels of each resource (the left eye's halves of the engine's, the right eye's
/// copies at (0, 0)) -- and the motion scale its DLSS was given. None outside an eye's evaluate.
pub fn evaluating_areas() -> Option<crate::neural_render::EyeAreas> {
    (EVALUATING_EYE.with(|e| e.get()) >= 0).then(|| EVALUATING_AREAS.with(|e| e.get()))
}

/// EvaluatingEye: the eye whose evaluate runs on this thread, -1 outside one.
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_dlss_evaluating_eye() -> i32 {
    EVALUATING_EYE.with(|e| e.get())
}

/// The mirrors' sizes, for the C++ half's layout asserts.
#[no_mangle]
pub extern "C" fn cvr_streamline_sizes(out: *mut usize, count: usize) -> usize {
    if !out.is_null() {
        for (i, s) in SIZES.iter().take(count).enumerate() {
            // SAFETY: the caller's array of `count`.
            unsafe { *out.add(i) = *s };
        }
    }
    SIZES.len()
}
