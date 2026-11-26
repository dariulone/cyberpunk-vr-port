//! The engine's own off-centre projection, per eye (formerly src/Stereo/AsymProjection.cpp).
//!
//! A headset lens is not centred on its panel: a Quest 3 eye sees -54/+40 deg horizontally and +44/-55 vertically.
//! The port used to render one frustum SYMMETRIC about the eye axis, sized to the widest half-angle, and let the
//! compositor throw the rest away -- a third of every rendered pixel never shown. REDengine 4 draws the off-centre
//! frustum itself: its camera matrix builder takes, besides the vertical fov, aspect and zoom, an NDC shift of the
//! projection centre and post-multiplies the projection by it -- the same multiply as the TAA jitter, but a separate
//! field the history camera keeps, so motion vectors and DLSS see an unjittered camera that is simply off-centre.
//!
//! WHERE THE SHIFT GOES IN. Every eye view is prepared by sub_1404240DC(ctx, spec, view), which builds the view's four
//! camera descriptors from its camera INPUT block at view+0x2C70 (fov, ?, aspect, zoom, shift x, shift y, ?, ?, near,
//! far). Writing that block on the way in reaches the descriptors, their copies, the jittered rebuild, the history, the
//! view-state matrices and the Streamline constants through the engine's own code, for both eyes.
//!
//! THE FIRST-PERSON PLANE HAS ITS OWN PROJECTION, AND ITS OWN SHIFT, rebuilt by sub_140422D2C from the plane's
//! override (fov [0], zoom [1], NDC shift [2]/[3]). VRCAM's override is attached DURING the producer, so the shift is
//! written where it is applied: the plane detour, told by the view detour which eye is being prepared.
//!
//! ONE SET OF SUN CASCADES HAS TO COVER BOTH EYES. The cascade fit (sub_1402A23AC) unprojects each slice through the
//! descriptor's own projection, shift included, and the port lends the second view's fit to MAIN. With mirror-image
//! lens windows MAIN's outer degrees lay outside it, so each view's fit is given the UNION of the two eyes' windows,
//! on a copy of its descriptor; the descriptor itself keeps the eye's own window.
//!
//! The exports, OpenXR's eye windows and the view tests stay in the C++ half, which attaches them here; the detours are
//! registered there (CVR_DETOUR) with these functions. The engine-stereo bookkeeping is `engine_stereo`.
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::Mutex;

use cvr_body::cmath::{atan, tan};
use cvr_camera::projection::{EngineProjection, EyeWindow, to_engine};
use cvr_engine::code;
use cvr_host::log;

use crate::engine_stereo::{self, EyeProjection};

pub const VIEW_PREPARE_RVA: usize = 0x4240DC;
pub const PLANE_OVERRIDE_RVA: usize = 0x422D2C;
pub const CASCADE_FIT_RVA: usize = 0x2A23AC;
const INPUT_FOV: usize = 0x2C70;
const FOV_SCALE: usize = 0x25D4;
const FOV_ADD: usize = 0x2B70;
const PLANE_OVERRIDES: usize = 0x2470;
const PLANES: usize = 4;
/// Render/ViewCameraLayout.hpp: the view's camera descriptors.
const DESC: usize = 0xE0;
const DESC_STRIDE: usize = 0x3E0;
/// Game 2.5's cascade-setup node hands the fit the view's +0x2390 as its fifth argument.
const CASCADE_FIT_VIEW_ARG: usize = 0x2390;
const DEG: f32 = 0.017_453_292_52;

/// cvr::projection::ViewWindow.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewWindow {
    pub shift_x: f32,
    pub shift_y: f32,
    pub half_x: f32,
    pub half_y: f32,
}

/// What the C++ half attaches: its exports and the questions only it can answer. Every callback that touches engine
/// memory guards itself.
#[repr(C)]
pub struct Runtime {
    pub mode: *const i32,
    pub inner_guard: *const f32,
    pub test: *const f32,
    pub debug_prepare: *mut u64,
    pub debug_view: *mut f32,
    pub debug_planes: *mut f32,
    pub debug_built: *mut f32,
    pub debug_fov_terms: *mut f32,
    pub debug_window: *mut f32,
    pub debug_descs: *mut f32,
    pub debug_plane_ptr: *mut u32,
    pub cascade_union: *const i32,
    pub debug_cascade_fits: *mut u64,
    pub debug_cascade_window: *mut f32,
    pub main_is_right_eye: *const i32,
    pub runtime_diagnostics: *const i32,
    pub vrcam_key: *const AtomicU64,
    /// OpenXRManager::GetAsymmetricEyeWindows into two windows.
    pub eye_windows: extern "C" fn(out: *mut EyeWindow, fit_target: bool) -> bool,
    pub is_main_view: extern "C" fn(view: *const u8) -> bool,
}
// SAFETY: exported variables and functions of the C++ half, alive for the process.
unsafe impl Sync for Runtime {}
unsafe impl Send for Runtime {}

/// The plugin's table is built in Rust (stereo_tables.rs).
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const crate::stereo_tables::ASYM).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());

fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached once with a pointer the C++ half keeps for the process.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}

/// The C++ half's exports and callbacks (Stereo/AsymProjection.cpp, at its static initialisation).
#[no_mangle]
pub extern "C" fn cvr_asym_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}

// ---- reads and writes of the exports and of engine memory

unsafe fn get<T: Copy>(p: *const T) -> T {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(p) }
}
unsafe fn put<T: Copy>(p: *mut T, value: T) {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::write_volatile(p, value) }
}
fn read_f32(address: usize) -> Option<f32> {
    code::read::<f32>(address)
}

/// Thrown out of a view pass: a read or write of the view faulted.
struct Fault;

fn r32(address: usize) -> Result<f32, Fault> {
    read_f32(address).ok_or(Fault)
}
fn w32(address: usize, value: f32) -> Result<(), Fault> {
    if code::write::<f32>(address, value) {
        Ok(())
    } else {
        Err(Fault)
    }
}

// ---- the state of the two sides

/// What we put in, so a later pass can tell the engine's own refresh from our value still standing, and put the
/// engine's back when the mode goes off.
#[derive(Clone, Copy, Default)]
struct Side {
    active: bool,
    wrote_fov: f32,
    wrote_x: f32,
    wrote_y: f32,
    wrote_aspect: f32,
    engine_fov: f32,
    engine_aspect: f32,
    half_x: f32,
    half_y: f32,
}

#[derive(Clone, Copy, Default)]
struct AspectNote {
    width: u32,
    height: u32,
    aspect: f32,
    lens: f32,
}

struct State {
    side: [Side; 2],
    aspect: AspectNote,
}

static STATE: Mutex<State> = Mutex::new(State {
    side: [Side {
        active: false,
        wrote_fov: 0.0,
        wrote_x: 0.0,
        wrote_y: 0.0,
        wrote_aspect: 0.0,
        engine_fov: 0.0,
        engine_aspect: 0.0,
        half_x: 0.0,
        half_y: 0.0,
    }; 2],
    aspect: AspectNote {
        width: 0,
        height: 0,
        aspect: 0.0,
        lens: 0.0,
    },
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The eye this thread is preparing, for the plane detour: side (-1 none), the shift to give the planes, and whether
/// to give it (else: take back a shift of ours still standing).
#[derive(Clone, Copy)]
struct Preparing {
    side: i32,
    want: bool,
    x: f32,
    y: f32,
    last_x: f32,
    last_y: f32,
}
const NOT_PREPARING: Preparing = Preparing {
    side: -1,
    want: false,
    x: 0.0,
    y: 0.0,
    last_x: 0.0,
    last_y: 0.0,
};
thread_local! {
    static PREPARING: Cell<Preparing> = const { Cell::new(NOT_PREPARING) };
}
fn set_preparing(p: Preparing) {
    PREPARING.with(|c| c.set(p));
}
fn preparing() -> Preparing {
    PREPARING.with(|c| c.get())
}
fn stop_preparing() {
    PREPARING.with(|c| {
        let mut p = c.get();
        p.side = -1;
        c.set(p);
    });
}

fn windows(rt: &Runtime, fit_target: bool) -> Option<[EyeWindow; 2]> {
    let mut w = [EyeWindow::default(); 2];
    (rt.eye_windows)(w.as_mut_ptr(), fit_target).then_some(w)
}

fn main_eye(rt: &Runtime) -> usize {
    // SAFETY: an export of the C++ half.
    if unsafe { get(rt.main_is_right_eye) } != 0 {
        1
    } else {
        0
    }
}

/// Which side a view is: 1 for VRCAM (its key), 0 for MAIN, -1 for any other.
fn side_of(rt: &Runtime, view: usize) -> Result<i32, Fault> {
    let key = code::read::<u64>(view + 0x28).ok_or(Fault)?;
    // SAFETY: the C++ half's atomic.
    let vrcam = unsafe { (*rt.vrcam_key).load(Ordering::Relaxed) };
    Ok(if key != 0 && key == vrcam {
        1
    } else if (rt.is_main_view)(view as *const u8) {
        0
    } else {
        -1
    })
}

fn diagnostics(rt: &Runtime) -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { get(rt.runtime_diagnostics) != 0 }
}

// ---- the window each eye is given

/// The render target keeps the game resolution's aspect, the lens window has the headset's: the window is grown to
/// the target's aspect so the pixels stay square, and the resolution that would make the growth zero is said here,
/// once per change.
fn note_aspect(rt: &Runtime, view: usize, w: &EyeWindow, content: &EyeWindow) {
    let mut size = [
        code::read::<u32>(view + 0x34).unwrap_or(0),
        code::read::<u32>(view + 0x38).unwrap_or(0),
    ];
    if engine_stereo::enabled() {
        size[0] /= 2; // both eyes side by side in one target
    }
    let aspect = (w.r - w.l) / (w.u - w.d);
    let lens_aspect = (content.r - content.l) / (content.u - content.d);
    if size[0] == 0
        || size[1] == 0
        || size[0] > 16384
        || size[1] > 16384
        || !(aspect > 0.05)
        || !(lens_aspect > 0.05)
    {
        return;
    }
    let mut st = state();
    let last = st.aspect;
    if size[0] == last.width
        && size[1] == last.height
        && (aspect - last.aspect).abs() < 1e-4
        && (lens_aspect - last.lens).abs() < 1e-4
    {
        return;
    }
    st.aspect = AspectNote {
        width: size[0],
        height: size[1],
        aspect,
        lens: lens_aspect,
    };
    drop(st);
    let render = size[0] as f32 / size[1] as f32;
    let grown = (aspect / lens_aspect - 1.0).abs() * 100.0;
    // SAFETY: an export of the C++ half.
    let guard = unsafe { get(rt.inner_guard) };
    log!(
        "[asym] MAIN window tan l={:.4} r={:.4} d={:.4} u={:.4} ({:.1} x {:.1} deg), aspect {:.4}; render {}x{}, aspect {:.4} \
         -> {}; content aspect {:.4} (inner guard {:.2}), window grown {:.2}% {}\n",
        w.l,
        w.r,
        w.d,
        w.u,
        (atan(w.r) - atan(w.l)) * 57.295_779_5,
        (atan(w.u) - atan(w.d)) * 57.295_779_5,
        aspect,
        size[0],
        size[1],
        render,
        if (render / aspect - 1.0).abs() < 0.002 {
            "square pixels"
        } else {
            "NON-SQUARE pixels"
        },
        lens_aspect,
        guard,
        grown,
        if aspect < lens_aspect {
            "vertically"
        } else {
            "horizontally"
        }
    );
    if grown >= 0.3 {
        log!(
            "[asym] a target matching the content exactly: {}x{} or {}x{}\n",
            size[0],
            (size[0] as f32 / lens_aspect).round() as u32,
            (size[1] as f32 * lens_aspect).round() as u32,
            size[1]
        );
    }
}

/// ENGINE STEREO: MAIN is the head centre the engine clones into the two eyes. MAIN gets the horizontal union of the
/// eyes' windows -- what culling and the sun cascades see from the centre -- at the eyes' one vertical span, centred
/// between their centres; each eye's own aspect and shift go to the eye cameras' build.
pub fn engine_stereo_centre(
    w: &[EyeWindow; 2],
    publish: impl FnOnce(&[EyeProjection; 2], bool),
) -> EyeWindow {
    let (a, b) = (&w[0], &w[1]);
    let span = a.u - a.d;
    let c = 0.25 * (a.u + a.d + b.u + b.d);
    let mut eyes = [EyeProjection::default(); 2];
    let mut valid = span > 0.05 && ((b.u - b.d) - span).abs() < 1e-3 * span;
    for (e, eye) in eyes.iter_mut().enumerate() {
        let p = to_engine(&w[e]);
        *eye = EyeProjection {
            aspect: p.aspect,
            shift_x: p.shift_x,
            shift_y: p.shift_y,
        };
        valid = valid
            && p.aspect > 0.05
            && p.aspect < 20.0
            && p.shift_x.is_finite()
            && p.shift_y.is_finite()
            && p.shift_x.abs() < 1.0
            && p.shift_y.abs() < 1.0;
    }
    publish(&eyes, valid);
    EyeWindow {
        l: a.l.min(b.l),
        r: a.r.max(b.r),
        d: c - 0.5 * span,
        u: c + 0.5 * span,
    }
}

/// The input block values one eye is given: (fov, shift x, shift y, aspect, want).
pub fn solve(p: &EngineProjection, fov_scale: f32, fov_add: f32) -> (f32, f32, f32, f32, bool) {
    // The descriptor gets scale * input + add; solve for the input that lands on the window's own vertical fov.
    let fov = (p.vfov_deg - fov_add) / fov_scale;
    let (x, y, aspect) = (p.shift_x, p.shift_y, p.aspect);
    let want = fov > 1.0
        && fov < 175.0
        && aspect > 0.05
        && aspect < 20.0
        && x.is_finite()
        && y.is_finite()
        && x.abs() < 1.0
        && y.abs() < 1.0;
    (fov, x, y, aspect, want)
}

fn restore(s: &mut Side, input: usize, values: &[f32; 10]) -> Result<(), Fault> {
    if values[0] == s.wrote_fov {
        w32(input, s.engine_fov)?;
    }
    if values[4] == s.wrote_x {
        w32(input + 16, 0.0)?;
    }
    if values[5] == s.wrote_y {
        w32(input + 20, 0.0)?;
    }
    if values[2] == s.wrote_aspect {
        w32(input + 8, s.engine_aspect)?;
    }
    s.active = false;
    Ok(())
}

fn apply(rt: &Runtime, view: usize, side: usize) -> Result<(), Fault> {
    let input = view + INPUT_FOV;
    let mut values = [0.0f32; 10];
    for (i, v) in values.iter_mut().enumerate() {
        *v = r32(input + i * 4)?;
    }
    // The side's state is locked only around its own updates: the callbacks below are the C++ half's.
    let known = {
        let mut st = state();
        let s = &mut st.side[side];
        // The engine's own values: refreshed whenever the field no longer holds what we wrote.
        if !s.active || values[0] != s.wrote_fov {
            s.engine_fov = values[0];
        }
        if !s.active || values[2] != s.wrote_aspect {
            s.engine_aspect = values[2];
        }
        *s
    };
    let mut plane = [0usize; PLANES];
    for (i, p) in plane.iter_mut().enumerate() {
        *p = code::read::<usize>(view + PLANE_OVERRIDES + i * 0x10).ok_or(Fault)?;
    }
    if !known.active {
        let shown = [
            values[0], values[1], values[2], values[3], values[4], values[5], values[8], values[9],
        ];
        let mut planes = [[0.0f32; 8]; PLANES];
        for (i, &p) in plane.iter().enumerate() {
            if p != 0 {
                for (k, v) in planes[i].iter_mut().enumerate() {
                    *v = r32(p + k * 4)?;
                }
            }
        }
        // SAFETY: the C++ half's debug exports, sized as their declarations.
        unsafe {
            for (k, v) in shown.iter().enumerate() {
                put(rt.debug_view.add(side * 8 + k), *v);
            }
            for (i, row) in planes.iter().enumerate() {
                for (k, v) in row.iter().enumerate() {
                    put(rt.debug_planes.add(side * 32 + i * 8 + k), *v);
                }
            }
        }
    }
    let fov_scale = r32(view + FOV_SCALE)?;
    let fov_add = r32(view + FOV_ADD)?;
    // SAFETY: as above.
    unsafe {
        put(rt.debug_fov_terms.add(side * 2), fov_scale);
        put(rt.debug_fov_terms.add(side * 2 + 1), fov_add);
    }

    let (mut fov, mut x, mut y, mut aspect, mut want) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, false);
    // SAFETY: an export of the C++ half.
    let mode = unsafe { get(rt.mode) };
    if mode == 1 {
        let main = main_eye(rt);
        let eye = if side == 0 { main } else { 1 - main };
        if let Some(w) = windows(rt, true).filter(|_| fov_scale.abs() > 0.05) {
            let own = if side == 0 && engine_stereo::enabled() {
                engine_stereo_centre(&w, engine_stereo::publish_eyes)
            } else {
                w[eye]
            };
            let p = to_engine(&own);
            // SAFETY: as above.
            unsafe {
                for (k, v) in [own.l, own.r, own.d, own.u].iter().enumerate() {
                    put(rt.debug_window.add(side * 4 + k), *v);
                }
            }
            (fov, x, y, aspect, want) = solve(&p, fov_scale, fov_add);
            if want && side == 0 {
                if let Some(content) = windows(rt, false) {
                    note_aspect(rt, view, &w[eye], &content[eye]);
                }
            }
        }
    } else if mode == 2 {
        // SAFETY: the test export, eight floats.
        let t = unsafe {
            [
                get(rt.test.add(side * 4)),
                get(rt.test.add(side * 4 + 1)),
                get(rt.test.add(side * 4 + 2)),
            ]
        };
        fov = known.engine_fov;
        x = if t[0].is_finite() { t[0] } else { 0.0 };
        y = if t[1].is_finite() { t[1] } else { 0.0 };
        aspect = if t[2].is_finite() && t[2] > 0.05 && t[2] < 20.0 {
            t[2]
        } else {
            known.engine_aspect
        };
        want = true;
    }

    // The planes get the shift from the plane detour. Their own fov is left to the engine: it follows the camera's,
    // which the CameraFov hook already sets to the window -- scaling it here as well applied the ratio twice.
    let mut st = state();
    let s = &mut st.side[side];
    set_preparing(Preparing {
        side: side as i32,
        want,
        x,
        y,
        last_x: s.wrote_x,
        last_y: s.wrote_y,
    });
    if want {
        w32(input, fov)?;
        w32(input + 8, aspect)?;
        w32(input + 16, x)?;
        w32(input + 20, y)?;
        s.wrote_fov = fov;
        s.wrote_x = x;
        s.wrote_y = y;
        s.wrote_aspect = aspect;
        s.half_y = tan((fov * fov_scale + fov_add) * 0.5 * DEG);
        s.half_x = s.half_y * aspect;
        s.active = true;
    } else if s.active {
        restore(s, input, &values)?;
    }
    Ok(())
}
fn read_built(rt: &Runtime, view: usize, side: usize) -> Result<(), Fault> {
    // SAFETY: the C++ half's debug exports, sized as their declarations.
    unsafe {
        for i in 0..PLANES {
            let d = view + DESC + i * DESC_STRIDE;
            let o = rt.debug_descs.add((side * 4 + i) * 8);
            for (k, off) in [0x20, 0x28, 0x2C, 0x30, 0x34, 0x370, 0x374, 0x1B0]
                .iter()
                .enumerate()
            {
                put(o.add(k), r32(d + off)?);
            }
            let p = code::read::<usize>(view + PLANE_OVERRIDES + i * 0x10).ok_or(Fault)?;
            put(rt.debug_plane_ptr.add(side * 4 + i), (p != 0) as u32);
        }
        let o = rt.debug_built.add(side * 16);
        let (d0, d2) = (view + DESC, view + DESC + 2 * DESC_STRIDE);
        let built = [
            (d0, 0x20),
            (d0, 0x28),
            (d0, 0x2C),
            (d0, 0x30),
            (d0, 0x34),
            (d0, 0x190),
            (d0, 0x1A4),
            (d0, 0x1B0),
            (d0, 0x1B4),
            (d2, 0x20),
            (d2, 0x30),
            (d2, 0x34),
            (d2, 0x190),
            (d2, 0x1A4),
            (d2, 0x1B0),
            (d2, 0x1B4),
        ];
        for (k, (d, off)) in built.iter().enumerate() {
            put(o.add(k), r32(d + off)?);
        }
        if side == 0 {
            engine_stereo::note_view_eyes(d0, d2);
        }
    }
    Ok(())
}

/// The three trampolines MinHook writes: view prepare, plane override, cascade fit (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_asym_originals: [AtomicPtr<c_void>; 3] = [const { AtomicPtr::new(core::ptr::null_mut()) }; 3];
fn original(_rt: &Runtime, index: usize) -> *mut c_void {
    cvr_asym_originals[index].load(Ordering::Acquire)
}

/// Detour_ViewPrepare: the eye's window into the view's input block on the way in.
///
/// # Safety
/// Called by the engine in place of sub_1404240DC.
#[no_mangle]
pub unsafe extern "C" fn cvr_asym_view_prepare(
    ctx: *mut c_void,
    spec: *mut c_void,
    view: *mut u8,
) -> i64 {
    let Some(rt) = runtime() else { return 0 };
    let mut side = -1;
    stop_preparing();
    if !view.is_null() {
        let at = view as usize;
        if let Ok(s) = side_of(rt, at) {
            side = s;
            if diagnostics(rt) {
                // SAFETY: three counters.
                unsafe {
                    let counter = rt
                        .debug_prepare
                        .add(if side < 0 { 2 } else { side as usize });
                    put(counter, get(counter) + 1);
                }
            }
            if side >= 0 && apply(rt, at, side as usize).is_err() {
                side = -1;
            }
        }
    }
    // SAFETY: the engine's own function, through MinHook's trampoline.
    let r = unsafe {
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u8) -> i64 =
            core::mem::transmute(original(rt, 0));
        f(ctx, spec, view)
    };
    stop_preparing();
    if side == 0 {
        engine_stereo::note_main_prepared(view as usize);
    }
    if side >= 0 && diagnostics(rt) {
        let _ = read_built(rt, view as usize, side as usize);
    }
    r
}

/// Detour_PlaneOverride: the plane's shift for the eye being prepared, and the engine-stereo eyes kept in place.
///
/// # Safety
/// Called by the engine in place of sub_140422D2C.
#[no_mangle]
pub unsafe extern "C" fn cvr_asym_plane_override(over: *mut f32, descriptor: *mut u8) -> i64 {
    let Some(rt) = runtime() else { return 0 };
    let p = preparing();
    if !over.is_null() && p.side >= 0 {
        let at = over as usize;
        if p.want {
            let _ = w32(at + 8, p.x).and_then(|_| w32(at + 12, p.y));
        } else if r32(at + 8).ok() == Some(p.last_x) && r32(at + 12).ok() == Some(p.last_y) {
            let _ = w32(at + 8, 0.0).and_then(|_| w32(at + 12, 0.0));
        }
    }
    // Engine stereo: the rebuild moves the plane's eyes to its centre; they go back to their places.
    let places = engine_stereo::keep_eye_places(descriptor as usize);
    // SAFETY: the engine's own function, through MinHook's trampoline.
    let r = unsafe {
        let f: unsafe extern "C" fn(*mut f32, *mut u8) -> i64 =
            core::mem::transmute(original(rt, 1));
        f(over, descriptor)
    };
    engine_stereo::restore_eye_places(&places);
    r
}

/// The union window in the descriptor's terms (vertical fov, aspect, shift x, shift y), or None to leave the fit
/// alone. `camera` is the descriptor's fov, aspect, shift x and shift y.
pub fn cascade_union(own: &EyeWindow, w: &[EyeWindow; 2], camera: [f32; 4]) -> Option<[f32; 4]> {
    let [fov, aspect, sx, sy] = camera;
    if !(fov > 1.0 && fov < 179.0) || !(aspect > 0.05 && aspect < 20.0) {
        return None;
    }
    // The descriptor must still hold the window this eye was given, or the union would be built for some other
    // frustum.
    let given = to_engine(own);
    if (sx - given.shift_x).abs() > 1e-3
        || (sy - given.shift_y).abs() > 1e-3
        || (aspect - given.aspect).abs() > 1e-3 * given.aspect
    {
        return None;
    }
    // Scaled to the descriptor's own vertical span, so a zoom or a fov the engine changed on top stays applied.
    let k = tan(fov * 0.5 * DEG) / (0.5 * (own.u - own.d));
    let u = EyeWindow {
        l: k * w[0].l.min(w[1].l),
        r: k * w[0].r.max(w[1].r),
        d: k * w[0].d.min(w[1].d),
        u: k * w[0].u.max(w[1].u),
    };
    let p = to_engine(&u);
    if !(p.vfov_deg > 1.0 && p.vfov_deg < 179.0)
        || !(p.aspect > 0.05 && p.aspect < 20.0)
        || !p.shift_x.is_finite()
        || !p.shift_y.is_finite()
    {
        return None;
    }
    Some([p.vfov_deg, p.aspect, p.shift_x, p.shift_y])
}

fn cascade_window(rt: &Runtime, camera: usize, side: usize) -> Result<Option<[f32; 4]>, Fault> {
    // SAFETY: exports of the C++ half.
    let (union, mode) = unsafe { (get(rt.cascade_union), get(rt.mode)) };
    if union == 0 || mode != 1 || !state().side[side].active {
        return Ok(None);
    }
    let Some(w) = windows(rt, true) else {
        return Ok(None);
    };
    let main = main_eye(rt);
    let own = w[if side == 0 { main } else { 1 - main }];
    let d = [
        r32(camera + 0x20)?,
        r32(camera + 0x28)?,
        r32(camera + 0x30)?,
        r32(camera + 0x34)?,
    ];
    Ok(cascade_union(&own, &w, d))
}

/// Detour_CascadeFit: the fit of each eye's sun cascades given the union of both eyes' windows, on a copy of the
/// descriptor (the fit copies it before use and reads nothing else of it but the cascade count).
///
/// # Safety
/// Called by the engine in place of sub_1402A23AC.
#[no_mangle]
pub unsafe extern "C" fn cvr_asym_cascade_fit(
    records: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    camera: *mut u8,
    a5: *mut u8,
) -> i64 {
    let Some(rt) = runtime() else { return 0 };
    #[repr(C, align(16))]
    struct Copy([u8; DESC_STRIDE]);
    let mut copy = Copy([0; DESC_STRIDE]);
    let mut patched = false;
    let mut side = -1i32;
    if !camera.is_null() && !a5.is_null() {
        let view = (a5 as usize).wrapping_sub(CASCADE_FIT_VIEW_ARG);
        let attempt = (|| -> Result<(), Fault> {
            side = side_of(rt, view)?;
            if side < 0 {
                return Ok(());
            }
            if let Some(u) = cascade_window(rt, camera as usize, side as usize)? {
                if !code::read_bytes(camera as usize, &mut copy.0) {
                    return Err(Fault);
                }
                for (off, v) in [(0x20usize, u[0]), (0x28, u[1]), (0x30, u[2]), (0x34, u[3])] {
                    copy.0[off..off + 4].copy_from_slice(&v.to_ne_bytes());
                }
                // SAFETY: the debug export, eight floats.
                unsafe {
                    for (k, v) in u.iter().enumerate() {
                        put(rt.debug_cascade_window.add(side as usize * 4 + k), *v);
                    }
                }
                patched = true;
            }
            Ok(())
        })();
        if attempt.is_err() {
            patched = false;
        }
    }
    let use_camera = if patched { copy.0.as_mut_ptr() } else { camera };
    if diagnostics(rt) {
        // SAFETY: three counters.
        unsafe {
            let counter = rt
                .debug_cascade_fits
                .add(if patched { side as usize } else { 2 });
            put(counter, get(counter) + 1);
        }
    }
    // SAFETY: the engine's own function, through MinHook's trampoline.
    unsafe {
        let f: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
            *mut u8,
            *mut u8,
        ) -> i64 = core::mem::transmute(original(rt, 2));
        f(records, a2, a3, use_camera, a5)
    }
}

/// cvr::projection::ViewWindowOf: the window a side was given, for the passes that map between the eyes.
/// cvr::projection::Active: each eye's own runtime frustum (mode 1).
pub fn active() -> bool {
    // SAFETY: an export of the C++ half.
    runtime().is_some_and(|rt| unsafe { get(rt.mode) } == 1)
}

pub fn view_window_of(side: i32) -> Option<ViewWindow> {
    if !(0..=1).contains(&side) {
        return None;
    }
    let s = state().side[side as usize];
    if !s.active {
        return None;
    }
    let w = ViewWindow {
        shift_x: s.wrote_x,
        shift_y: s.wrote_y,
        half_x: s.half_x,
        half_y: s.half_y,
    };
    (w.shift_x.is_finite()
        && w.shift_y.is_finite()
        && w.half_x > 0.01
        && w.half_y > 0.01
        && w.half_x.is_finite()
        && w.half_y.is_finite())
    .then_some(w)
}

/// ViewWindowOf for the C++ half. `out` is written whenever the side is active, like the C++ function did.
///
/// # Safety
/// `out` is null or a ViewWindow.
#[no_mangle]
pub unsafe extern "C" fn cvr_asym_view_window(side: i32, out: *mut ViewWindow) -> bool {
    if out.is_null() || !(0..=1).contains(&side) {
        return false;
    }
    let s = state().side[side as usize];
    if !s.active {
        return false;
    }
    let w = ViewWindow {
        shift_x: s.wrote_x,
        shift_y: s.wrote_y,
        half_x: s.half_x,
        half_y: s.half_y,
    };
    // SAFETY: the caller's struct.
    unsafe { *out = w };
    view_window_of(side).is_some()
}

/// cvr::projection::WindowParams from its inputs: the inner guard -- the whole window (1) when both eyes share one
/// window or the headset gets the mono picture, else the guard export clamped to 0..1 (0 when not finite) -- and the
/// square-pixel target -- the launcher's render aspect (0 when unknown) when the windows are fitted to it with square
/// pixels on, else 0.
pub fn window_params_from(shared_or_mono: bool, inner_guard: f32, square_target: bool, launcher: (i32, i32)) -> (f32, f32) {
    let guard = if shared_or_mono {
        1.0
    } else if inner_guard.is_finite() {
        inner_guard.max(0.0).min(1.0)
    } else {
        0.0
    };
    let (w, h) = launcher;
    let target = if square_target && w > 0 && h > 0 { w as f32 / h as f32 } else { 0.0 };
    (guard, target)
}
#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_EngineStereoEyeWindows: i32;
    static CyberpunkVR_MonoView: i32;
    static CyberpunkVR_AsymInnerGuard: f32;
    static CyberpunkVR_AsymSquarePixels: i32;
}
/// cvr::projection::WindowParams: the windows' guard and square-pixel target, from the engine-stereo switch, the
/// mono view, the eye-window and projection exports and the launcher's size.
#[cfg(not(test))]
pub fn window_params(fit_target: bool) -> (f32, f32) {
    // SAFETY: the exports, read as they change.
    let (eye_windows, mono_view, guard, square) = unsafe {
        (
            core::ptr::read_volatile(&raw const CyberpunkVR_EngineStereoEyeWindows),
            core::ptr::read_volatile(&raw const CyberpunkVR_MonoView),
            core::ptr::read_volatile(&raw const CyberpunkVR_AsymInnerGuard),
            core::ptr::read_volatile(&raw const CyberpunkVR_AsymSquarePixels),
        )
    };
    let shared_window = crate::engine_stereo::enabled() && eye_windows == 0;
    window_params_from(shared_window || mono_view != 0, guard, fit_target && square != 0, crate::engine_stereo::launcher_size())
}
/// The C++ half's cvr::projection::WindowParams.
///
/// # Safety
/// `guard` and `target` are writable.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_projection_window_params(fit_target: bool, guard: *mut f32, target: *mut f32) {
    let (g, t) = window_params(fit_target);
    // SAFETY: the caller's outputs.
    unsafe {
        *guard = g;
        *target = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_parameters() {
        // One shared window (or the mono picture): the whole window, whatever the export says.
        assert_eq!(window_params_from(true, 0.3, false, (2048, 1024)), (1.0, 0.0));
        // The guard clamped, a non-finite one none; the target the render aspect only when fitted with square pixels.
        assert_eq!(window_params_from(false, 0.3, true, (2048, 1024)), (0.3, 2.0));
        assert_eq!(window_params_from(false, 1.7, true, (0, 1024)), (1.0, 0.0));
        assert_eq!(window_params_from(false, -0.5, false, (2048, 1024)), (0.0, 0.0));
        assert_eq!(window_params_from(false, f32::NAN, true, (1864, 2016)), (0.0, 1864.0 / 2016.0));
    }

    // Quest 3's eyes: mirror-image windows.
    const LEFT: EyeWindow = EyeWindow {
        l: -1.3764,
        r: 0.8391,
        d: -1.4281,
        u: 0.9657,
    };
    const RIGHT: EyeWindow = EyeWindow {
        l: -0.8391,
        r: 1.3764,
        d: -1.4281,
        u: 0.9657,
    };

    #[test]
    fn the_input_block_lands_on_the_window() {
        let p = to_engine(&LEFT);
        let (fov, x, y, aspect, want) = solve(&p, 1.0, 0.0);
        assert!(want);
        assert!((fov - p.vfov_deg).abs() < 1e-6);
        assert_eq!((x, y, aspect), (p.shift_x, p.shift_y, p.aspect));
        assert!(
            x > 0.0,
            "a window reaching further left shifts the centre right"
        );
        // The descriptor's own terms are solved for: scale * input + add = the window's fov.
        let (fov2, ..) = solve(&p, 0.5, 10.0);
        assert!((fov2 * 0.5 + 10.0 - p.vfov_deg).abs() < 1e-4);
        // Implausible results are not wanted.
        assert!(
            !solve(
                &EngineProjection {
                    vfov_deg: 200.0,
                    aspect: 1.0,
                    shift_x: 0.0,
                    shift_y: 0.0
                },
                1.0,
                0.0
            )
            .4
        );
        assert!(
            !solve(
                &EngineProjection {
                    vfov_deg: 90.0,
                    aspect: 1.0,
                    shift_x: 1.5,
                    shift_y: 0.0
                },
                1.0,
                0.0
            )
            .4
        );
    }

    #[test]
    fn engine_stereo_centre_is_the_union_at_one_span() {
        let mut published = None;
        let c = engine_stereo_centre(&[LEFT, RIGHT], |eyes, valid| {
            published = Some((*eyes, valid))
        });
        assert_eq!((c.l, c.r), (LEFT.l, RIGHT.r));
        assert!((c.u - c.d - (LEFT.u - LEFT.d)).abs() < 1e-6);
        let (eyes, valid) = published.unwrap();
        assert!(valid);
        assert!(
            (eyes[0].shift_x + eyes[1].shift_x).abs() < 1e-6,
            "mirror-image eyes shift oppositely"
        );
        // Different vertical spans are not one engine-stereo pair.
        let mut tall = RIGHT;
        tall.u += 0.1;
        engine_stereo_centre(&[LEFT, tall], |_, valid| assert!(!valid));
    }

    #[test]
    fn the_cascade_fit_sees_both_eyes() {
        let p = to_engine(&LEFT);
        let u = cascade_union(
            &LEFT,
            &[LEFT, RIGHT],
            [p.vfov_deg, p.aspect, p.shift_x, p.shift_y],
        )
        .unwrap();
        let union = to_engine(&EyeWindow {
            l: LEFT.l,
            r: RIGHT.r,
            d: LEFT.d,
            u: LEFT.u,
        });
        assert!((u[0] - union.vfov_deg).abs() < 1e-3);
        assert!((u[1] - union.aspect).abs() < 1e-4);
        assert!(u[2].abs() < 1e-5, "the union of mirror images is centred");
        // A descriptor that no longer holds this eye's window is left alone.
        assert!(
            cascade_union(
                &LEFT,
                &[LEFT, RIGHT],
                [p.vfov_deg, p.aspect, 0.0, p.shift_y]
            )
            .is_none()
        );
        // A zoomed descriptor keeps its zoom: the union is scaled to its span.
        let zoomed = cascade_union(
            &LEFT,
            &[LEFT, RIGHT],
            [p.vfov_deg * 0.5, p.aspect, p.shift_x, p.shift_y],
        )
        .unwrap();
        assert!(zoomed[0] < u[0]);
        assert!((zoomed[1] - u[1]).abs() < 1e-4);
    }
}
