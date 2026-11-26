//! THE CAMERA SCOPE'S DLSS / RR STATE, ISOLATED FROM MAIN'S (main's 0.2.4: include/Stereo/ScopeDlssState.hpp,
//! ScopeRrOptions.hpp and src/Stereo/Dlss.cpp; docs/fixes/scope-rr-isolation.md).
//!
//! The native camera scope reused MAIN's DLSS/RR camera, viewport and size cache: a save loaded with a scope equipped
//! alternated tiny and full-size RR creation -- severe lag. The scope's constants and evaluation now run under the
//! engine's own recursive DLSS lock with a private Streamline viewport (0x43565301), a camera and cache of its own and
//! a consistent frame token; MAIN's state is restored after each call. The scope's completions and tags are kept out
//! of the headset's frame generation capture and the per-eye SR/RR split. RR's mode for the scope's 1:1 extent is DLAA
//! (MAIN's Ultra Performance asked NGX for a 114x114 input of a 341x341 scope).
use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};

use cvr_engine::code::read;
use cvr_host::log;
use windows_sys::Win32::System::Threading::{EnterCriticalSection, LeaveCriticalSection, CRITICAL_SECTION};

use crate::streamline::{BaseStructure, DlssdOptions, ViewportHandle, DLSSD_OPTIONS, VIEWPORT_HANDLE};

/// Separate from MAIN(0), VRCAM(1) and the SBS eye viewports (CVR01/02).
pub const VIEWPORT: u32 = 0x4356_5301;

// CP2.5 native DLSS state, verified in the camera writer and the tag/eval driver: camera/SL constants and numeric
// options, not GPU owners.
const CAMERA: usize = 0x20;
const CACHE: usize = 0x3C8;
const OPTIONS: usize = 0x3E8;
/// The native handle starts at 0x480; BaseStructure is 32 bytes. 0x498 is its size_t structVersion, NOT the viewport
/// value (the pre-2.5 offset was stale).
const END: usize = 0x40C;
const VIEWPORT_OFFSET: usize = 0x4A0;
const TOKEN: usize = 0x4A8;
const FG_AVAILABLE: usize = 0x43B;
const REFLEX_AVAILABLE: usize = 0x43C;

const DLSS_EVAL_RVA: usize = 0x228_F210;
const RENDER_RES_RVA: usize = 0x42_5054;
const RENDERER_GLOBAL_RVA: usize = 0x3BB_CAE8;
const OFF_VIEWSTATE: usize = 0x4658;
/// The scope RTT view's frame field.
const VIEW_FRAME: usize = 0x1A0;
/// sl::DLSSMode::eDLAA.
const DLSS_MODE_DLAA: u32 = 6;

/// ScopeDlssState.hpp's State: the scope's own camera, cache and options, the owner and view it belongs to.
pub struct State {
    camera_and_cache: [u8; OPTIONS - CAMERA],
    options: [u8; END - OPTIONS],
    owner: usize,
    view: usize,
    frame: u32,
    constants: bool,
    reset: bool,
}
impl State {
    const fn new() -> State {
        State {
            camera_and_cache: [0; OPTIONS - CAMERA],
            options: [0; END - OPTIONS],
            owner: 0,
            view: 0,
            frame: 0,
            constants: false,
            reset: true,
        }
    }
    fn select(&mut self, owner: usize, view: usize) {
        if self.owner != owner || self.view != view {
            *self = State::new();
            self.owner = owner;
            self.view = view;
        }
    }
    fn prepare_frame(&mut self, next: u32) {
        self.reset = !self.constants || (next != self.frame && next != self.frame.wrapping_add(1));
        self.frame = next;
    }
}

/// The state is the engine's DLSS lock's: every access is under that recursive critical section (NativeDlssGuard),
/// the nested calls on one thread one after another.
struct Shared(UnsafeCell<State>);
// SAFETY: as above.
unsafe impl Sync for Shared {}
static STATE: Shared = Shared(UnsafeCell::new(State::new()));
/// # Safety
/// Under NativeDlssGuard; no other reference held across the call.
unsafe fn state() -> &'static mut State {
    // SAFETY: as the caller vouches.
    unsafe { &mut *STATE.0.get() }
}

thread_local! {
    /// The scope's camera writer runs on this thread: its constants driver is the scope's.
    static SCOPE_CONSTANTS: Cell<bool> = const { Cell::new(false) };
    /// The native DLSS state of the scope's evaluation running on this thread (RR's options sizing reads it).
    static SCOPE_EVALUATION: Cell<usize> = const { Cell::new(0) };
}

fn exe_base() -> usize {
    cvr_engine::code::image_base()
}
extern "C" {
    /// framegen_inputs.rs: the renderer's frame clock.
    fn cvr_framegen_read_render_frame_index(index: *mut u32) -> bool;
}

/// The lock both native Evaluate and Configure take, derived from the tag/eval driver (its EnterCriticalSection import
/// validated): a separate mutex would miss native settings writes. Null when the code is not what it was written
/// against.
fn native_lock() -> *mut CRITICAL_SECTION {
    static LOCK: AtomicUsize = AtomicUsize::new(usize::MAX);
    let cached = LOCK.load(Acquire);
    if cached != usize::MAX {
        return cached as *mut CRITICAL_SECTION;
    }
    let found = (|| -> Option<usize> {
        let site = exe_base() + DLSS_EVAL_RVA + 0x21;
        let call = site + 13;
        // lea rcx, [rip+disp32] ; ... ; call [rip+disp32] -- guarded reads of the game's code
        let (b0, b1, b2): (u8, u8, u8) = (read(site)?, read(site + 1)?, read(site + 2)?);
        let (c0, c1): (u8, u8) = (read(call)?, read(call + 1)?);
        if b0 != 0x48 || b1 != 0x8D || b2 != 0x0D || c0 != 0xFF || c1 != 0x15 {
            return None;
        }
        let disp: i32 = read(call + 2)?;
        let slot = (call as isize + 6 + disp as isize) as usize;
        let imported: usize = read(slot)?;
        if imported == 0 || !is_enter_critical_section(imported) {
            return None;
        }
        let disp: i32 = read(site + 3)?;
        let lock = (site as isize + 7 + disp as isize) as usize;
        log!(
            "[scope-dlss] isolated viewport={:08X} native lock RVA={:X}\n",
            VIEWPORT,
            lock - exe_base()
        );
        Some(lock)
    })()
    .unwrap_or(0);
    LOCK.store(found, Release);
    found as *mut CRITICAL_SECTION
}
/// The game's import resolves kernel32's EnterCriticalSection (a forwarder to ntdll's RtlEnterCriticalSection).
fn is_enter_critical_section(address: usize) -> bool {
    use windows::core::{s, w};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    // SAFETY: module queries.
    unsafe {
        let mut known = [0usize; 2];
        if let Ok(k) = GetModuleHandleW(w!("kernel32.dll")) {
            known[0] = GetProcAddress(k, s!("EnterCriticalSection")).map_or(0, |f| f as usize);
        }
        if let Ok(n) = GetModuleHandleW(w!("ntdll.dll")) {
            known[1] = GetProcAddress(n, s!("RtlEnterCriticalSection")).map_or(0, |f| f as usize);
        }
        known.contains(&address)
    }
}

/// NativeDlssGuard: the engine's DLSS lock held for a detour's whole call.
pub struct Guard {
    lock: *mut CRITICAL_SECTION,
}
impl Guard {
    pub fn new() -> Guard {
        let lock = native_lock();
        if !lock.is_null() {
            // SAFETY: the engine's initialised critical section (recursive).
            unsafe { EnterCriticalSection(lock) };
        }
        Guard { lock }
    }
    pub fn held(&self) -> bool {
        !self.lock.is_null()
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.lock.is_null() {
            // SAFETY: entered in new.
            unsafe { LeaveCriticalSection(self.lock) };
        }
    }
}

/// ScopeObject: the scope's view object for a view spec whose context is the optics feed, else 0.
///
/// # Safety
/// `spec` is the engine's argument (read guarded).
pub unsafe fn scope_object(spec: usize, optics_feed: u64, view_obj: impl Fn(usize) -> Option<usize>) -> usize {
    if spec == 0 {
        return 0;
    }
    (|| -> Option<usize> {
        // guarded reads
        let ctx: usize = read(spec + 0x18)?;
        if ctx == 0 || read::<u64>(ctx + 0x28)? != optics_feed {
            return Some(0);
        }
        view_obj(spec)
    })()
    .unwrap_or(0)
}

/// NativeDlssState: the renderer's native DLSS state, its viewport handle's ABI checked; 0 otherwise.
pub fn native_state() -> usize {
    static WARNED: AtomicU32 = AtomicU32::new(0);
    (|| -> Option<usize> {
        // guarded reads of the renderer's globals
        let renderer: usize = read(exe_base() + RENDERER_GLOBAL_RVA)?;
        if renderer == 0 {
            return None;
        }
        let state: usize = read(renderer + OFF_VIEWSTATE)?;
        if state == 0 {
            return None;
        }
        let handle: BaseStructure = read(state + 0x480)?;
        if handle.struct_type != VIEWPORT_HANDLE || handle.struct_version != 1 {
            if WARNED.swap(1, Relaxed) == 0 {
                log!("[scope-dlss] native viewport ABI mismatch; isolation disabled\n");
            }
            return None;
        }
        Some(state)
    })()
    .unwrap_or(0)
}

/// ScopeDlssState.hpp's Swap: the scope's camera, cache, viewport and token in the native state for one call, MAIN's
/// restored when it ends.
struct Swap {
    native: *mut u8,
    saved: [u8; END - CAMERA],
    viewport: u32,
    token: u64,
    fg_available: u8,
    reflex_available: u8,
}
impl Swap {
    /// # Safety
    /// `p` is the live native DLSS state (validated by native_state, or the driver's own argument); under the guard.
    unsafe fn new(p: *mut u8, s: &mut State) -> Swap {
        // SAFETY (whole body): as the caller vouches.
        unsafe {
            let mut swap = Swap {
                native: p,
                saved: [0; END - CAMERA],
                viewport: p.add(VIEWPORT_OFFSET).cast::<u32>().read_unaligned(),
                token: p.add(TOKEN).cast::<u64>().read_unaligned(),
                fg_available: *p.add(FG_AVAILABLE),
                reflex_available: *p.add(REFLEX_AVAILABLE),
            };
            core::ptr::copy_nonoverlapping(p.add(CAMERA), swap.saved.as_mut_ptr(), END - CAMERA);
            if s.constants {
                core::ptr::copy_nonoverlapping(s.camera_and_cache.as_ptr(), p.add(CAMERA), OPTIONS - CAMERA);
            }
            // Never let MAIN's dimensions suppress the scope's initial configuration.
            let options = core::slice::from_raw_parts(p.add(OPTIONS), END - OPTIONS);
            if !s.constants || s.options[..] != options[..] {
                core::ptr::write_bytes(p.add(CACHE), 0, OPTIONS - CACHE);
            }
            s.options.copy_from_slice(core::slice::from_raw_parts(p.add(OPTIONS), END - OPTIONS));
            p.add(VIEWPORT_OFFSET).cast::<u32>().write_unaligned(VIEWPORT);
            // RTT's native per-view token can stay zero: its camera is always uploaded.
            p.add(TOKEN).cast::<u64>().write_unaligned(u64::MAX);
            // The native constants driver configures FG and Reflex when available: the scope is an upscale-only
            // viewport, never a second presentation.
            *p.add(FG_AVAILABLE) = 0;
            *p.add(REFLEX_AVAILABLE) = 0;
            swap
        }
    }
}
impl Drop for Swap {
    fn drop(&mut self) {
        // SAFETY: the state the swap was made on, still under the guard.
        unsafe {
            let s = state();
            let p = self.native;
            core::ptr::copy_nonoverlapping(p.add(CAMERA), s.camera_and_cache.as_mut_ptr(), OPTIONS - CAMERA);
            core::ptr::copy_nonoverlapping(self.saved.as_ptr(), p.add(CAMERA), END - CAMERA);
            p.add(VIEWPORT_OFFSET).cast::<u32>().write_unaligned(self.viewport);
            p.add(TOKEN).cast::<u64>().write_unaligned(self.token);
            *p.add(FG_AVAILABLE) = self.fg_available;
            *p.add(REFLEX_AVAILABLE) = self.reflex_available;
        }
    }
}

/// ScopeFrameToken: the scope RTT's view frame can stay zero; the same renderer clock for its constants AND evaluate,
/// the view's field restored before the call returns.
struct FrameToken {
    field: *mut u32,
    saved: u32,
}
impl FrameToken {
    /// # Safety
    /// `obj` is the scope's live view object.
    unsafe fn new(obj: usize, frame: u32) -> FrameToken {
        let field = (obj + VIEW_FRAME) as *mut u32;
        // SAFETY: as the caller vouches.
        let saved = unsafe { field.read_unaligned() };
        unsafe { field.write_unaligned(frame) };
        FrameToken { field, saved }
    }
}
impl Drop for FrameToken {
    fn drop(&mut self) {
        // SAFETY: the field written in new.
        unsafe { self.field.write_unaligned(self.saved) };
    }
}

/// The scope's camera writer (Detour_SlConstants): under the guard, the scope's state selected and swapped in, its
/// frame token set and the constants driver told, around `inner`. `inner` alone otherwise.
///
/// # Safety
/// The engine's arguments.
pub unsafe fn sl_constants(spec: usize, obj: usize, inner: impl FnOnce() -> i64) -> i64 {
    let guard = Guard::new();
    let native = native_state();
    let mut frame = 0u32;
    // SAFETY: the renderer clock's guarded read.
    if !guard.held() || obj == 0 || native == 0 || !unsafe { cvr_framegen_read_render_frame_index(&mut frame) } {
        return inner();
    }
    let _ = spec;
    // SAFETY (whole body): under the guard; the native state validated.
    unsafe {
        {
            let s = state();
            s.select(native, obj);
            s.prepare_frame(frame);
        }
        let _swap = Swap::new(native as *mut u8, state());
        let _token = FrameToken::new(obj, frame);
        let previous = SCOPE_CONSTANTS.with(|c| c.replace(true));
        let result = inner();
        SCOPE_CONSTANTS.with(|c| c.set(previous));
        result
    }
}

/// Whether the constants driver now runs for the scope (inside its camera writer).
pub fn constants_active() -> bool {
    SCOPE_CONSTANTS.with(|c| c.get())
}
/// The constants driver's guard, and the scope's reset asked of it (native -> Constants.reset).
///
/// # Safety
/// `a1` is the driver's live state; under the guard.
pub unsafe fn before_constants(a1: usize) {
    // SAFETY: as the caller vouches.
    unsafe {
        if constants_active() && state().reset && a1 != 0 {
            *((a1 + 0x1F0) as *mut u8) = 0;
        }
    }
}
/// After the constants driver: whether it uploaded the scope's constants for this frame.
///
/// # Safety
/// As before_constants.
pub unsafe fn after_constants(a1: usize, frame: u32) {
    if constants_active() && a1 != 0 {
        // SAFETY: as the caller vouches.
        unsafe {
            let token = ((a1 + TOKEN) as *const u64).read_unaligned();
            state().constants = token == frame as u64;
        }
    }
}

/// The tag/eval driver (Detour_DlssEval) for the scope: true when it was evaluated here (the scope's state swapped in
/// around the original); false for the caller's own path.
///
/// # Safety
/// The engine's arguments; `original` the driver's trampoline call; under the caller's guard.
pub unsafe fn evaluate(guard: &Guard, a1: usize, obj: usize, original: impl FnOnce()) -> bool {
    static REPORTS: AtomicU32 = AtomicU32::new(0);
    static WARNED: AtomicU32 = AtomicU32::new(0);
    // SAFETY (whole body): under the guard.
    unsafe {
        let s = state();
        if guard.held() && obj != 0 && a1 != 0 && s.constants && s.view == obj && s.owner == a1 {
            let frame = s.frame;
            let swap = Swap::new(a1 as *mut u8, s);
            let token = FrameToken::new(obj, frame);
            let previous = SCOPE_EVALUATION.with(|c| c.replace(a1));
            original();
            SCOPE_EVALUATION.with(|c| c.set(previous));
            // reported while the scope's state is still in place (main logs inside the swap's scope)
            if REPORTS.fetch_add(1, Relaxed) < 3 {
                let p = a1 as *const u8;
                let handle = p.add(0x480).cast::<ViewportHandle>().read_unaligned();
                let at = |o: usize| p.add(o).cast::<u32>().read_unaligned();
                log!(
                    "[scope-dlss] evaluate actual_viewport={:08X} version={} frame={} input={}x{} output={}x{}\n",
                    handle.value,
                    handle.base.struct_version,
                    frame,
                    at(0x3D0),
                    at(0x3D4),
                    at(0x3E0),
                    at(0x3E4)
                );
            }
            drop(token);
            drop(swap);
            return true;
        }
        if obj != 0 && WARNED.swap(1, Relaxed) == 0 {
            log!("[scope-dlss] no matching scope constants; using native path\n");
        }
        false
    }
}

// ---- RR's options sizing (the native wrapper of slDLSSDSetOptions, 0x22902D0)

/// The trampoline MinHook writes.
#[allow(non_upper_case_globals)]
pub static cvr_scope_rr_options_original: AtomicUsize = AtomicUsize::new(0);
type RrOptionsFn = unsafe extern "C" fn(*const ViewportHandle, *const DlssdOptions) -> i32;

/// MatchRrMode: RR's mode defines its expected input size. MAIN's Ultra Performance cannot describe a scope whose
/// tagged input and output are BOTH 341x341 (it asks NGX for 114x114 instead): the native dimensions and matrices kept,
/// the SDK's 1:1 mode used only for this exact, independently identified viewport.
pub fn match_rr_mode(viewport: u32, options: &mut DlssdOptions, input: (u32, u32), output: (u32, u32)) -> bool {
    if viewport != VIEWPORT
        || options.base.struct_type != DLSSD_OPTIONS
        || options.base.struct_version != 3
        || options.mode == 0
        || input.0 < 16
        || input.1 < 16
        || input.0 > 8192
        || input.1 > 8192
        || input != output
        || options.output_width != output.0
        || options.output_height != output.1
    {
        return false;
    }
    options.mode = DLSS_MODE_DLAA;
    true
}

/// ScopeRrOptions: during the scope's evaluation, its RR options at the 1:1 mode; MAIN's unchanged.
///
/// # Safety
/// Streamline's arguments as the native wrapper received them.
#[no_mangle]
pub unsafe extern "C" fn cvr_scope_rr_options(viewport: *const ViewportHandle, options: *const DlssdOptions) -> i32 {
    static REPORTS: AtomicU32 = AtomicU32::new(0);
    // SAFETY (whole body): the trampoline and the engine's live arguments.
    unsafe {
        let original: RrOptionsFn = core::mem::transmute(cvr_scope_rr_options_original.load(Acquire));
        let p = SCOPE_EVALUATION.with(|c| c.get());
        if p == 0 || viewport.is_null() || options.is_null() {
            return original(viewport, options);
        }
        let value = (*viewport).value;
        let mut matched = core::ptr::read(options);
        let at = |o: usize| ((p + o) as *const u32).read_unaligned();
        if !match_rr_mode(value, &mut matched, (at(0x3D0), at(0x3D4)), (at(0x3E0), at(0x3E4))) {
            return original(viewport, options);
        }
        let result = original(viewport, &matched);
        if REPORTS.fetch_add(1, Relaxed) < 3 {
            log!(
                "[scope-dlss] RR 1:1 mode={}->{} extent={}x{} result={}; MAIN options unchanged\n",
                (*options).mode,
                matched.mode,
                matched.output_width,
                matched.output_height,
                result
            );
        }
        result
    }
}
/// RrOptionsSiteMatches: the CP2.5 native wrapper of slDLSSDSetOptions (its feature-1001 import verified).
pub extern "C" fn rr_options_site_matches() -> bool {
    const EXPECTED: [u8; 10] = [0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83, 0xEC, 0x20];
    let mut bytes = [0u8; 10];
    // a guarded read of the game's code
    cvr_engine::code::read_bytes(exe_base() + 0x229_02D0, &mut bytes) && bytes == EXPECTED
}
/// The RR options detour is live.
pub fn rr_options_hooked() -> bool {
    cvr_scope_rr_options_original.load(Acquire) != 0
}

// ---- the scope's full resolution (ScopeFullResolutionReady / RenderScopeFullResolution)

/// The installed CP2.5 scaler: the upscaler tests followed by the native unscaled size writes, verified beyond the
/// detoured entry (which MinHook has already patched).
fn resolution_site_matches() -> bool {
    static MATCHES: AtomicU32 = AtomicU32::new(2);
    let known = MATCHES.load(Relaxed);
    if known != 2 {
        return known == 1;
    }
    const EXPECTED: [u8; 98] = [
        0x49, 0x8b, 0x86, 0xb8, 0x25, 0x00, 0x00, 0xa8, 0x40, 0x75, 0x57, 0x84, 0xc0, 0x78, 0x53, 0x48, 0x0f, 0xba,
        0xe0, 0x09, 0x72, 0x4c, 0x48, 0x0f, 0xba, 0xe0, 0x08, 0x72, 0x45, 0x48, 0x0f, 0xba, 0xe0, 0x0d, 0x72, 0x3e,
        0x48, 0x0f, 0xba, 0xe0, 0x0c, 0x72, 0x37, 0x48, 0x0f, 0xba, 0xe0, 0x0a, 0x72, 0x30, 0x48, 0x0f, 0xba, 0xe0,
        0x0e, 0x72, 0x29, 0x48, 0x0f, 0xba, 0xe0, 0x1b, 0x72, 0x22, 0x44, 0x8b, 0x5b, 0x24, 0x45, 0x8b, 0xc3, 0x44,
        0x8b, 0x4b, 0x20, 0x41, 0x8b, 0xf3, 0x44, 0x89, 0x5b, 0x1c, 0x41, 0x8b, 0xfb, 0x44, 0x89, 0x5b, 0x18, 0x4c,
        0x89, 0x63, 0x30, 0xe9, 0xdc, 0x00, 0x00, 0x00,
    ];
    let mut bytes = [0u8; 98];
    // a guarded read of the game's code
    let ok = cvr_engine::code::read_bytes(exe_base() + RENDER_RES_RVA + 0x569, &mut bytes) && bytes == EXPECTED;
    if !ok {
        log!("[scope-resolution] native scaler signature mismatch; full resolution disabled\n");
    }
    MATCHES.store(ok as u32, Relaxed);
    ok
}

/// ScopeFullResolutionReady: the scope's RR view sized natively (xr_scope_rr_full_resolution) -- only with the scope's
/// isolation itself in place.
///
/// # Safety
/// The scaler's arguments.
pub unsafe fn full_resolution_ready(dimensions: usize, rect: usize, ctx: usize, optics_feed: u64) -> bool {
    if !crate::scope_resolution::FULL_RESOLUTION.load(Relaxed) {
        return false;
    }
    if dimensions == 0 || rect == 0 || ctx == 0 || dimensions != ctx + 0x34 {
        return false;
    }
    let eligible = (|| -> Option<bool> {
        // guarded reads
        let bounds: [i32; 4] = read(rect)?;
        let key: u64 = read(ctx + 0x28)?;
        let flags: u64 = read(ctx + 0x25B8)?;
        Some(crate::scope_resolution::full_resolution_eligible(
            true,
            key == optics_feed,
            flags,
            bounds[2] as i64 - bounds[0] as i64,
            bounds[3] as i64 - bounds[1] as i64,
        ))
    })()
    .unwrap_or(false);
    eligible && rr_options_hooked() && resolution_site_matches() && !native_lock().is_null() && native_state() != 0
}
/// RenderScopeFullResolution: the resolution-scale flags masked only around the native calculation, restored BEFORE
/// returning to any builder or pass.
///
/// # Safety
/// As full_resolution_ready, which said yes.
pub unsafe fn render_full_resolution(dimensions: usize, ctx: usize, original: impl FnOnce() -> i64) -> i64 {
    static REPORTS: AtomicU32 = AtomicU32::new(0);
    // SAFETY: the view's live flags word for the call.
    let result = unsafe {
        let _flags = crate::scope_resolution::NativeResolutionFlags::new((ctx + 0x25B8) as *mut u64);
        original()
    };
    if REPORTS.fetch_add(1, Relaxed) < 3 {
        let at = |o: usize| -> u32 { read(dimensions + o).unwrap_or(0) };
        log!(
            "[scope-resolution] RR full-size render={}x{} target={}x{}; graph flags restored\n",
            at(0),
            at(4),
            at(0x20),
            at(0x24)
        );
    }
    result
}

/// Streamline's evaluate for the scope's viewport (main's Inputs.cpp): a camera-scope completion is not a completed
/// headset eye -- true when one of the inputs is the scope's viewport (logged a few times per feature).
///
/// # Safety
/// Streamline's input list.
pub unsafe fn is_scope_evaluation(feature: u32, frame: u32, inputs: *const *const BaseStructure, count: u32, result: i32) -> bool {
    static REPORTS: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
    if inputs.is_null() {
        return false;
    }
    for i in 0..count as usize {
        // SAFETY: Streamline's list of `count` entries.
        let input = unsafe { *inputs.add(i) };
        if input.is_null() {
            continue;
        }
        // SAFETY: a live base structure.
        let base = unsafe { &*input };
        if base.struct_type == VIEWPORT_HANDLE && unsafe { (*input.cast::<ViewportHandle>()).value } == VIEWPORT {
            if REPORTS[(feature == 1001) as usize].fetch_add(1, Relaxed) < 4 {
                log!(
                    "[scope-dlss] Streamline received viewport={:08X} version={} feature={} frame={} result={}\n",
                    VIEWPORT,
                    base.struct_version,
                    feature,
                    frame,
                    result
                );
            }
            return true;
        }
    }
    false
}
/// The tags of the scope's viewport (RecordTags): not the headset's.
///
/// # Safety
/// A live ViewportHandle or null.
pub unsafe fn is_scope_viewport(viewport: *const c_void) -> bool {
    // SAFETY: as the caller vouches.
    !viewport.is_null() && unsafe { (*viewport.cast::<ViewportHandle>()).value } == VIEWPORT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(mode: u32, w: u32, h: u32) -> DlssdOptions {
        // SAFETY: plain numbers.
        let mut o: DlssdOptions = unsafe { core::mem::zeroed() };
        o.base.struct_type = DLSSD_OPTIONS;
        o.base.struct_version = 3;
        o.mode = mode;
        o.output_width = w;
        o.output_height = h;
        o
    }

    #[test]
    fn rr_takes_dlaa_only_for_the_scopes_one_to_one_extent() {
        let mut o = options(4, 341, 341);
        assert!(match_rr_mode(VIEWPORT, &mut o, (341, 341), (341, 341)));
        assert_eq!(o.mode, DLSS_MODE_DLAA);
        // MAIN, VRCAM, a true upscale, off, a stale output size: unchanged
        for (viewport, input, output, mode, size) in [
            (0u32, (341, 341), (341, 341), 4u32, (341, 341)),
            (1, (341, 341), (341, 341), 4, (341, 341)),
            (VIEWPORT, (114, 114), (341, 341), 4, (341, 341)),
            (VIEWPORT, (341, 341), (341, 341), 0, (341, 341)),
            (VIEWPORT, (341, 341), (341, 341), 4, (640, 480)),
            (VIEWPORT, (8, 8), (8, 8), 4, (8, 8)),
        ] {
            let mut o = options(mode, size.0, size.1);
            assert!(!match_rr_mode(viewport, &mut o, input, output));
            assert_eq!(o.mode, mode);
        }
    }

    #[test]
    fn the_state_resets_on_an_owner_change_or_a_frame_gap() {
        let mut s = State::new();
        s.select(1, 2);
        s.prepare_frame(10);
        assert!(s.reset);
        s.constants = true;
        s.prepare_frame(11);
        assert!(!s.reset);
        s.prepare_frame(11);
        assert!(!s.reset);
        s.prepare_frame(14);
        assert!(s.reset);
        s.select(1, 3);
        assert!(!s.constants && s.view == 3);
    }

    #[test]
    fn the_swap_restores_main() {
        let mut native = vec![0u8; 0x500];
        for (i, b) in native.iter_mut().enumerate() {
            *b = i as u8;
        }
        let before = native.clone();
        let mut s = State::new();
        // SAFETY: a buffer of the state's size; the test's own state.
        unsafe {
            let p = native.as_mut_ptr();
            {
                let swap = Swap::new(p, &mut s);
                assert_eq!(p.add(VIEWPORT_OFFSET).cast::<u32>().read_unaligned(), VIEWPORT);
                assert_eq!(*p.add(FG_AVAILABLE), 0);
                // the scope's camera written by the evaluation
                *p.add(CAMERA) = 0xAB;
                // Drop writes into the global state: the test's swap is taken apart by hand instead
                let s2 = &mut s;
                core::ptr::copy_nonoverlapping(p.add(CAMERA), s2.camera_and_cache.as_mut_ptr(), OPTIONS - CAMERA);
                core::ptr::copy_nonoverlapping(swap.saved.as_ptr(), p.add(CAMERA), END - CAMERA);
                p.add(VIEWPORT_OFFSET).cast::<u32>().write_unaligned(swap.viewport);
                p.add(TOKEN).cast::<u64>().write_unaligned(swap.token);
                *p.add(FG_AVAILABLE) = swap.fg_available;
                *p.add(REFLEX_AVAILABLE) = swap.reflex_available;
                core::mem::forget(swap);
            }
        }
        assert_eq!(native, before);
        assert_eq!(s.camera_and_cache[0], 0xAB);
    }
}
