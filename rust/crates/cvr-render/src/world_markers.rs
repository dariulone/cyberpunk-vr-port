//! THE WORLD MARKERS IN STEREO (formerly src/Hooks/WorldMarkers.cpp; that file keeps the exports, the hook's
//! registration and the forwarders, and src/Hooks/WorldMarkerTextThunk.asm the text call's thunk).
//!
//! The game's mappins, nameplates and script indicators are ink widgets on the flat HUD canvas, placed by the camera
//! the UI update projects through. In the headset that canvas is drawn into each eye, so a marker stands at the
//! canvas's infinity in both. The port owns the native projection: the update's world point and camera are kept for
//! each marker root (Remember), its paint carries the root's inverse depth to every primitive allocated under it
//! (Paint, Allocate), and the emitted vertices are lifted back into the world through the camera the canvas was laid
//! out with (Emit, the text tag), level from the marker's point; the replacement ink shaders project them per eye
//! with the eye packet appended to the canvas camera (RewriteConstants). Markers are shrunk to 0.7 about their anchor,
//! their margins kept unrounded, their quads and glyphs unsnapped, and a flattened effect takes its marker's
//! projection. Off-screen markers are not clamped to an eye's edge but drawn at their point, or not at all when an
//! eye cannot see it.
//!
//! Verified in world-markers-20260923.md (2.31) and on 2.5 with IDA headless; every offset is the C++ file's.
use cvr_host::fast::FastMap;
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use cvr_engine::code;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::function::BaseFunction;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::{object_class, Class, RttiSystem};
use red4ext_abi::stack::{CStack, StackFrame, StackType};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

use crate::world_marker_math::*;

// ---- the C++ half's exports and functions (src/Hooks/WorldMarkers.cpp keeps the exports)

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_WorldMarkerStereo: AtomicI32;
    static CyberpunkVR_WorldMarkerRoots: AtomicU64;
    static CyberpunkVR_WorldMarkerQuads: AtomicU64;
    static CyberpunkVR_WorldMarkerTexts: AtomicU64;
    static CyberpunkVR_WorldMarkerCameraUploads: [AtomicU64; 2];
    static CyberpunkVR_WorldMarkerTextUploads: [AtomicU64; 2];
    static CyberpunkVR_WorldMarkerMappinStages: [AtomicU64; 7];
    static CyberpunkVR_WorldMarkerProjectionStages: [AtomicU64; 5];
    static CyberpunkVR_AsymHudShift: AtomicI32;
    static CyberpunkVR_WorldMarkerProbe: AtomicI32;
    static CyberpunkVR_WorldMarkerOffCanvas: AtomicI32;
    static CyberpunkVR_WorldMarkerProbeSequence: [AtomicU32; 2];
    static mut CyberpunkVR_WorldMarkerProbeWords: [[u64; 4]; 2];
    static mut CyberpunkVR_WorldMarkerProbeFloats: [[f32; 64]; 2];
    static CyberpunkVR_WorldMarkerReproject: AtomicI32;
    static CyberpunkVR_WorldMarkerWorldVertices: AtomicU64;
    static CyberpunkVR_WorldMarkerWorldTexts: AtomicU64;
    static CyberpunkVR_WorldMarkerWorldCameras: [AtomicU64; 2];
    static CyberpunkVR_WorldMarkerTraceSequence: AtomicU32;
    static mut CyberpunkVR_WorldMarkerTraceWords: [[u64; 8]; 64];
    static mut CyberpunkVR_WorldMarkerTraceFloats: [[f32; 48]; 64];
    static mut CyberpunkVR_WorldMarkerTextLast: [u64; 8];
    static CyberpunkVR_WorldMarkerLevel: AtomicI32;
    static CyberpunkVR_WorldMarkerExactMargin: AtomicI32;
    static CyberpunkVR_WorldMarkerUnsnap: AtomicI32;
    static CyberpunkVR_WorldMarkerCompositeLoose: AtomicI32;
    static CyberpunkVR_WorldMarkerCompositeStats: [AtomicU64; 8];
    static mut CyberpunkVR_WorldMarkerCompositeMixed: [u64; 4];
    static CyberpunkVR_WorldMarkerExactMargins: AtomicU64;
    static CyberpunkVR_WorldMarkerUnlifted: [AtomicU64; 5];
    static mut CyberpunkVR_WorldMarkerUnliftedRoot: [u64; 5];
    static CyberpunkVR_WorldMarkerDetachedPaints: AtomicU64;
    static mut CyberpunkVR_WorldMarkerDetachedPaint: [u64; 4];
    static CyberpunkVR_WorldMarkerLayerRoot: u64;
    static CyberpunkVR_WorldMarkerLayerCount: AtomicU32;
    static mut CyberpunkVR_WorldMarkerLayerWords: [[u64; 4]; 64];
    static mut CyberpunkVR_WorldMarkerLayerFloats: [[f32; 8]; 64];
    static CyberpunkVR_WorldMarkerEdgeClamp: AtomicI32;

    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_StereoSubmit: i32;
    static CyberpunkVR_MainAdsZoomFactor: f32;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_HudRootWidth: f32;
    static CyberpunkVR_HudRootHeight: f32;

    fn CyberpunkVRPort_HalfIpd() -> f32;
    fn GetGameRenderFovDeg() -> f32;
    fn GetMenuMode() -> i32;
    /// The mid-function thunk (WorldMarkerTextThunk.asm): tags the geometry, then jumps on to the publication.
    fn CvrWorldMarkerTextPublishDetour();
    fn cvr_camera_located_frame_read(out: *mut LocatedCameraFrame) -> bool;
}

#[allow(non_snake_case)]
extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_DisableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

/// cvr::camera::LocatedCameraFrame (rust/crates/cvr-rig/src/vrik/camera_link.rs).
#[repr(C)]
#[derive(Default)]
struct LocatedCameraFrame {
    world_pos: [f32; 3],
    world_quat: [f32; 4],
    sequence: u32,
    frame_epoch: u32,
    body_centre_known: u32,
    body_base_world: [f32; 3],
}

/// The recording worker's current node view (render_tls.rs).
fn current_node_view() -> usize {
    crate::render_tls::current_node_view()
}

fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
macro_rules! diag {
    ($($e:tt)*) => {
        if diagnostics() {
            $($e)*
        }
    };
}
fn relaxed(v: &AtomicI32) -> i32 {
    v.load(Ordering::Relaxed)
}
fn bump(v: &AtomicU64) {
    v.fetch_add(1, Ordering::Relaxed);
}
fn now() -> u64 {
    // SAFETY: a plain Win32 call.
    unsafe { GetTickCount64() }
}
fn base() -> usize {
    code::image_base()
}

// ---- raw access

/// `*(T*)(p + offset)`.
///
/// # Safety
/// `p + offset` holds a T.
unsafe fn at<T: Copy>(p: usize, offset: usize) -> T {
    unsafe { core::ptr::read_unaligned((p + offset) as *const T) }
}
unsafe fn at_mut<T>(p: usize, offset: usize) -> *mut T {
    (p + offset) as *mut T
}
/// A WeakHandle field locked (`At<WeakHandle<T>>(p, offset).Lock()`).
///
/// # Safety
/// `p + offset` holds a WeakHandle.
unsafe fn lock_weak(p: usize, offset: usize) -> Handle {
    unsafe { (*((p + offset) as *const WeakHandle)).lock() }
}
fn weak_of(handle: &Handle) -> WeakHandle {
    WeakHandle::from_handle(handle)
}

// ---- ink::Widget (RED4ext.SDK inkWidget.hpp)

const WIDGET_MARGIN: usize = 0x98; // layout (0x88) + margin (0x10): left, top, right, bottom
const WIDGET_ANCHOR_POINT: usize = 0xA8; // layout + anchorPoint (0x20)
const WIDGET_SCALE: usize = 0xC0; // renderTransform (0xB8) + scale (0x08)
const WIDGET_LAYER_PROXY: usize = 0x148;
const WIDGET_PARENT: usize = 0x168;
const WIDGET_SIZE: usize = 0x198;
const WIDGET_PIVOT: usize = 0x1A8;
const LAYER_PROXY_LAYER: usize = 0x38;

fn vec2(p: usize, offset: usize) -> [f32; 2] {
    // SAFETY: a live widget's Vector2 field.
    unsafe { at::<[f32; 2]>(p, offset) }
}

/// The thread's state, as the C++ file's thread_locals.
#[derive(Clone, Copy)]
struct InkCamera {
    width: f32,
    #[allow(dead_code)] // kept as the C++ file kept it
    height: f32,
    parallax: f32,
    squeeze: f32,
    stamp: u64,
    world_ready: bool,
}
const INK_CAMERA: InkCamera = InkCamera {
    width: 0.0,
    height: 0.0,
    parallax: 0.0,
    squeeze: 1.0,
    stamp: 0,
    world_ready: false,
};
const NO_SOURCE: SourceProjection = SourceProjection {
    origin: [0.0; 3],
    right: [0.0; 3],
    up: [0.0; 3],
    forward: [0.0; 3],
    p00: 0.0,
    p11: 0.0,
    p20: 0.0,
    p21: 0.0,
    width: 0.0,
    height: 0.0,
};
const NO_ANCHOR: MarkerAnchor = MarkerAnchor {
    world: [0.0; 3],
    canvas: [0.0; 2],
    valid: false,
};

struct Tls {
    paint_depth: Cell<f32>,
    paint_owner: Cell<usize>,
    offscreen_pass: Cell<u32>,
    text_depth: Cell<f32>,
    mappin_view_context: Cell<usize>,
    paint_source: Cell<SourceProjection>,
    text_source: Cell<SourceProjection>,
    paint_anchor: Cell<MarkerAnchor>,
    text_anchor: Cell<MarkerAnchor>,
    unsnap_vertices: Cell<bool>,
    paint_nesting: Cell<u32>,
    text_exact: Cell<[f32; 2]>,
    text_exact_valid: Cell<bool>,
    text_layer_root: Cell<usize>,
    ink_cameras: Cell<[InkCamera; 2]>,
    ink_slot: Cell<i32>,
}
thread_local! {
    static TLS: Tls = const {
        Tls {
            paint_depth: Cell::new(0.0),
            paint_owner: Cell::new(0),
            offscreen_pass: Cell::new(0),
            text_depth: Cell::new(0.0),
            mappin_view_context: Cell::new(0),
            paint_source: Cell::new(NO_SOURCE),
            text_source: Cell::new(NO_SOURCE),
            paint_anchor: Cell::new(NO_ANCHOR),
            text_anchor: Cell::new(NO_ANCHOR),
            unsnap_vertices: Cell::new(false),
            paint_nesting: Cell::new(0),
            text_exact: Cell::new([0.0; 2]),
            text_exact_valid: Cell::new(false),
            text_layer_root: Cell::new(0),
            ink_cameras: Cell::new([INK_CAMERA; 2]),
            ink_slot: Cell::new(-1),
        }
    };
}
fn tls<R>(f: impl FnOnce(&Tls) -> R) -> R {
    TLS.with(f)
}

// ---- the switch

static HOOKS_READY: AtomicBool = AtomicBool::new(false);

fn enabled() -> bool {
    // SAFETY: exports of the C++ half.
    HOOKS_READY.load(Ordering::Acquire)
        && crate::world_marker_shader::vertex_shaders_ready()
        && relaxed(unsafe { &CyberpunkVR_WorldMarkerStereo }) != 0
        && unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_StereoSubmit) } != 0
}
fn reprojection_enabled() -> bool {
    // SAFETY: an export of the C++ half.
    enabled()
        && crate::engine_stereo::enabled()
        && relaxed(unsafe { &CyberpunkVR_WorldMarkerReproject }) != 0
}
fn menu_mode() -> bool {
    // SAFETY: a C function of the plugin.
    unsafe { GetMenuMode() != 0 }
}

// ---- the UI's camera

/// 2.5 ink projection view -> gameCameraSystem; its WorldToScreen projects through the native camera at system+0x60
/// (sub_14054AD6C). Every read guarded.
fn read_ui_projection(view: usize, width: f32, height: f32) -> Option<SourceProjection> {
    if view == 0 || code::read::<usize>(view)? != base() + 0x3620AA8 {
        return None;
    }
    let system = code::read::<usize>(view + 8)?;
    if system == 0 || code::read::<usize>(system)? != base() + 0x30BB4A8 {
        return None;
    }
    let mut camera = [0u8; 0x1D0];
    if !code::read_bytes(system + 0x60, &mut camera) {
        return None;
    }
    let mut source = read_projection(&camera, width, height, false)?;
    // A one-eye layer's view places through one eye's window (Stereo/EngineStereo.hpp UiViewNarrowing).
    let narrow = crate::engine_stereo::ui_view_narrowing(view as *const c_void);
    source.p00 *= narrow;
    source.p20 *= narrow;
    Some(source)
}

struct LastSource {
    view: usize,
    size: [f32; 2],
    stamp: u64,
}
static LAST_SOURCE: Mutex<LastSource> = Mutex::new(LastSource {
    view: 0,
    size: [0.0; 2],
    stamp: 0,
});

fn capture_source(view: usize, size: *const f32) -> SourceProjection {
    if !reprojection_enabled() || size.is_null() {
        return NO_SOURCE;
    }
    // SAFETY: the update's canvas size, two floats.
    let size = unsafe { [*size, *size.add(1)] };
    let Some(source) = read_ui_projection(view, size[0], size[1]) else {
        return NO_SOURCE;
    };
    let mut last = LAST_SOURCE.lock().unwrap_or_else(|e| e.into_inner());
    *last = LastSource {
        view,
        size,
        stamp: now(),
    };
    source
}
fn current_ui_source() -> SourceProjection {
    let (view, size) = {
        let last = LAST_SOURCE.lock().unwrap_or_else(|e| e.into_inner());
        if now().wrapping_sub(last.stamp) > 100 || menu_mode() {
            return NO_SOURCE;
        }
        (last.view, last.size)
    };
    if !reprojection_enabled() {
        return NO_SOURCE;
    }
    read_ui_projection(view, size[0], size[1]).unwrap_or(NO_SOURCE)
}

/// The eyes' last cameras as the recording workers read them, for the update's "does each eye see this point".
struct EyeSources {
    sources: [SourceProjection; 2],
    stamps: [u64; 2],
}
static EYE_SOURCES: Mutex<EyeSources> = Mutex::new(EyeSources {
    sources: [NO_SOURCE; 2],
    stamps: [0; 2],
});

fn keep_eye_source(eye: i32, source: &SourceProjection) {
    let mut e = EYE_SOURCES.lock().unwrap_or_else(|e| e.into_inner());
    e.sources[eye as usize] = *source;
    e.stamps[eye as usize] = now();
}
/// True unless an eye with a fresh camera has the point outside its window (no fresh camera: no verdict).
fn both_eyes_see(point: &[f32; 3]) -> bool {
    let (eyes, stamps) = {
        let e = EYE_SOURCES.lock().unwrap_or_else(|e| e.into_inner());
        (e.sources, e.stamps)
    };
    let t = now();
    for eye in 0..2 {
        let s = &eyes[eye];
        if !s.valid() || t.wrapping_sub(stamps[eye]) > 250 {
            continue;
        }
        let mut v = [0.0f64; 3];
        for i in 0..3 {
            let d = point[i] as f64 - s.origin[i] as f64;
            v[0] += d * s.right[i] as f64;
            v[1] += d * s.up[i] as f64;
            v[2] += d * s.forward[i] as f64;
        }
        if !(v[2] > 0.05) {
            return false;
        }
        let x = v[0] / v[2] * s.p00 as f64 + s.p20 as f64;
        let y = v[1] / v[2] * s.p11 as f64 + s.p21 as f64;
        if !(x.abs() <= 1.0) || !(y.abs() <= 1.0) {
            return false;
        }
    }
    true
}
/// The worker's own view's eye camera (not the global MAIN pointer, which can already be a newer frame's).
fn read_eye_camera(eye: i32, width: f32, height: f32, view: usize) -> Option<WorldMarkerCamera> {
    if !(0..=1).contains(&eye) || view == 0 {
        return None;
    }
    let descriptor = view + 0xE0;
    if code::read::<u8>(descriptor + 0x399)? & 1 == 0 {
        return None;
    }
    let camera = code::read::<usize>(descriptor + if eye != 0 { 0x3D8 } else { 0x3D0 })?;
    if camera == 0 || code::read::<u8>(camera + 0x398)? != (eye + 1) as u8 {
        return None;
    }
    let mut block = [0u8; 0x380];
    if !code::read_bytes(camera, &mut block) {
        return None;
    }
    let target = read_projection(&block, width, height, true)?;
    keep_eye_source(eye, &target);
    Some(marker_camera(&target))
}
fn can_lift(source: &SourceProjection) -> bool {
    // Source ownership and the explicit offscreen-composition scope decide the encoding; the recording worker
    // supplies the matching eye packet later.
    reprojection_enabled() && tls(|t| t.offscreen_pass.get()) == 0 && source.valid()
}

// ---- the probes (CyberpunkVR_WorldMarkerProbe)

static TRACE: Mutex<()> = Mutex::new(());

/// # Safety
/// Under TRACE.
unsafe fn trace_slot(root: usize) -> usize {
    let words = unsafe { &*(&raw const CyberpunkVR_WorldMarkerTraceWords) };
    let mut oldest = 0;
    for i in 0..64 {
        let row = &words[i];
        if row[0] == root as u64 || row[0] == 0 {
            return i;
        }
        if row[4] < words[oldest][4] {
            oldest = i;
        }
    }
    oldest
}
#[allow(clippy::too_many_arguments)]
fn trace_mappin(
    controller: usize,
    root: usize,
    point: &[f32; 3],
    screen: *const f32,
    source: &SourceProjection,
    valid: bool,
    clamped: bool,
) {
    // SAFETY: exports of the C++ half, written under TRACE; the controller and root are live.
    unsafe {
        if relaxed(&CyberpunkVR_WorldMarkerProbe) == 0 {
            return;
        }
        let _lock = TRACE.lock().unwrap_or_else(|e| e.into_inner());
        CyberpunkVR_WorldMarkerTraceSequence.fetch_add(1, Ordering::AcqRel);
        let i = trace_slot(root);
        let w = &mut (*(&raw mut CyberpunkVR_WorldMarkerTraceWords))[i];
        let f = &mut (*(&raw mut CyberpunkVR_WorldMarkerTraceFloats))[i];
        if w[0] != root as u64 {
            *w = [0; 8];
            f[..48].fill(0.0);
        }
        w[0] = root as u64;
        w[1] = controller as u64;
        w[2] = at::<usize>(controller, 0x40) as u64;
        w[3] = at::<usize>(controller, 0x170) as u64;
        w[4] = now();
        w[7] = (w[7] & !15)
            | u64::from(valid)
            | if clamped { 2 } else { 0 }
            | if at::<u8>(controller, 0x190) != 0 {
                4
            } else {
                0
            }
            | if at::<u8>(controller, 0xE3) != 0 {
                8
            } else {
                0
            };
        f[..3].copy_from_slice(point);
        f[3] = source_depth(source, point);
        if !screen.is_null() {
            f[4] = *screen;
            f[5] = *screen.add(1);
        }
        f[8] = source.width;
        f[9] = source.height;
        let margin: [f32; 2] = at(root, WIDGET_MARGIN);
        f[10] = margin[0];
        f[11] = margin[1];
        let size = vec2(root, WIDGET_SIZE);
        f[12] = size[0];
        f[13] = size[1];
        let scale = vec2(root, WIDGET_SCALE);
        f[14] = scale[0];
        f[15] = scale[1];
        f[16..19].copy_from_slice(&source.origin);
        f[20..23].copy_from_slice(&source.forward);
        if source.valid() && f[3] > 0.0 {
            let (mut x, mut y) = (0.0f32, 0.0f32);
            for a in 0..3 {
                x += (point[a] - source.origin[a]) * source.right[a];
                y += (point[a] - source.origin[a]) * source.up[a];
            }
            f[6] = (x / f[3] * source.p00 + source.p20 + 1.0) * source.width * 0.5;
            f[7] = (1.0 - y / f[3] * source.p11 - source.p21) * source.height * 0.5;
        }
        CyberpunkVR_WorldMarkerTraceSequence.fetch_add(1, Ordering::Release);
    }
}

static PROBE_VIEW: Mutex<()> = Mutex::new(());

#[allow(clippy::too_many_arguments)]
fn probe_view(
    kind: usize,
    root: usize,
    view: usize,
    world: *const f32,
    size: *const f32,
    screen: *const f32,
    collected_world: Option<&[f32; 3]>,
) {
    // SAFETY: an export of the C++ half; the view is the update's live one, its slots as the C++ file calls them.
    unsafe {
        if relaxed(&CyberpunkVR_WorldMarkerProbe) == 0 || view == 0 || world.is_null() {
            return;
        }
        let mut data = [0.0f32; 64];
        let world = core::slice::from_raw_parts(world, 3);
        data[..3].copy_from_slice(world);
        let vt = *(view as *const *const usize);
        type PositionFn = unsafe extern "C" fn(usize, *mut f32);
        core::mem::transmute::<usize, PositionFn>(*vt.add(48 / 8))(view, data.as_mut_ptr().add(4));
        type VectorFn = unsafe extern "C" fn(usize, *mut f32) -> *const f32;
        for i in 0..3 {
            let mut temporary = [0.0f32; 4];
            let result = core::mem::transmute::<usize, VectorFn>(*vt.add((64 + i * 8) / 8))(
                view,
                temporary.as_mut_ptr(),
            );
            if result.is_null() {
                return;
            }
            data[8 + i * 4..8 + i * 4 + 3].copy_from_slice(core::slice::from_raw_parts(result, 3));
        }
        for i in 0..3 {
            data[3] += (world[i] - data[4 + i]) * data[8 + i];
        }
        type ProjectFn = unsafe extern "C" fn(usize, *mut f32, *const f32) -> *const f32;
        for sample in 0..4 {
            let mut point = [0.0f32; 4];
            let mut result = [0.0f32; 4];
            for i in 0..3 {
                point[i] = if sample == 0 {
                    world[i]
                } else {
                    data[4 + i]
                        + data[8 + i]
                        + if sample == 2 {
                            data[12 + i]
                        } else if sample == 3 {
                            data[16 + i]
                        } else {
                            0.0
                        }
                };
            }
            let projected = core::mem::transmute::<usize, ProjectFn>(*vt.add(104 / 8))(
                view,
                result.as_mut_ptr(),
                point.as_ptr(),
            );
            if projected.is_null() {
                return;
            }
            data[20 + sample * 4..22 + sample * 4]
                .copy_from_slice(core::slice::from_raw_parts(projected, 2));
        }
        if !size.is_null() {
            data[36..38].copy_from_slice(core::slice::from_raw_parts(size, 2));
        }
        if !screen.is_null() {
            data[38..40].copy_from_slice(core::slice::from_raw_parts(screen, 2));
        }
        let mut located = LocatedCameraFrame::default();
        if cvr_camera_located_frame_read(&mut located) {
            data[40..43].copy_from_slice(&located.world_pos);
            data[44..48].copy_from_slice(&located.world_quat);
            data[43] = located.sequence as f32;
        }
        if let Some(c) = collected_world {
            data[48..51].copy_from_slice(c);
        }
        let _lock = PROBE_VIEW.lock().unwrap_or_else(|e| e.into_inner());
        CyberpunkVR_WorldMarkerProbeSequence[kind].fetch_add(1, Ordering::AcqRel);
        let words = &mut (*(&raw mut CyberpunkVR_WorldMarkerProbeWords))[kind];
        words[0] = now();
        words[1] = root as u64;
        words[2] = view as u64;
        words[3] = vt as u64;
        (*(&raw mut CyberpunkVR_WorldMarkerProbeFloats))[kind] = data;
        CyberpunkVR_WorldMarkerProbeSequence[kind].fetch_add(1, Ordering::Release);
    }
}

// ---- RTTI

/// RED4ext::ExecuteFunction(instance, f, out, args): the result (when the function has one) into `out`, the
/// arguments as given -- a null type taken from the function's parameter, as the SDK does (the engine's opcode reads
/// the type: null, it faulted) -- and a function with a result not called without a place for it.
///
/// # Safety
/// `instance` is a live instance of the function's class; `out` holds its result type or is null.
unsafe fn execute(
    instance: usize,
    f: &BaseFunction,
    out: *mut c_void,
    args: &mut [StackType],
) -> bool {
    let Some(rtti) = RttiSystem::get() else {
        return false;
    };
    let mut result = StackType {
        ty: core::ptr::null(),
        value: out,
    };
    // SAFETY: the function's own return property.
    let result_ptr = match unsafe { f.return_type.as_ref() } {
        Some(_) if out.is_null() => return false,
        Some(r) => {
            result.ty = r.ty;
            &mut result as *mut StackType
        }
        None => core::ptr::null_mut(),
    };
    // SAFETY: the function's own parameters.
    for (arg, param) in args.iter_mut().zip(f.params.as_slice()) {
        if arg.ty.is_null() {
            if let Some(param) = unsafe { param.as_ref() } {
                arg.ty = param.ty;
            }
        }
    }
    let Some(mut stack) = CStack::new(rtti, instance as *mut c_void, args, result_ptr) else {
        return false;
    };
    let Some(address) = f.execute_address() else {
        return false;
    };
    // SAFETY: the engine's Execute under the SEH adapter.
    unsafe {
        code::guarded_call3(
            address,
            f as *const BaseFunction as *mut c_void,
            &mut stack as *mut CStack as *mut c_void,
            core::ptr::null_mut(),
        )
    }
    .unwrap_or(false)
}
fn class_of(object: usize) -> Option<&'static Class> {
    // SAFETY: a live scriptable.
    unsafe { object_class(object as *const c_void) }
}
fn param_type_name(f: &BaseFunction, i: usize) -> Option<CName> {
    let p = *f.params.as_slice().get(i)?;
    // SAFETY: a registered function's parameters.
    let p = unsafe { p.as_ref() }?;
    // SAFETY: as above.
    Some(unsafe { p.ty.as_ref() }?.name())
}

const INK_HUD_LAYER: CName = CName::hashed("inkHUDLayer");
const VECTOR2: CName = CName::hashed("Vector2");

/// The root is drawn on the HUD layer: the first ancestor (itself included, 24 up) with a layer decides.
fn hud_root(root: &Handle) -> bool {
    let mut current = root.clone();
    for _ in 0..24 {
        if current.is_null() {
            return false;
        }
        let w = current.instance as usize;
        // SAFETY: a live widget's layer proxy (held by it) and its weak layer.
        let proxy: usize = unsafe { at(w, WIDGET_LAYER_PROXY) };
        let layer = if proxy != 0 {
            unsafe { lock_weak(proxy, LAYER_PROXY_LAYER) }
        } else {
            Handle::null()
        };
        if !layer.is_null() {
            return class_of(layer.instance as usize)
                .is_some_and(|c| c.class_name() == INK_HUD_LAYER);
        }
        // SAFETY: as above.
        current = unsafe { lock_weak(w, WIDGET_PARENT) };
    }
    false
}
/// The world container's game controller: a gameuiMappinBaseController that is no minimap's or world map's.
fn world_mappin_controller(controller: usize) -> bool {
    let mut class = class_of(controller);
    for _ in 0..64 {
        let Some(c) = class else { return false };
        let name = c.class_name();
        if name == CName::hashed("gameuiBaseMinimapMappinController")
            || name == CName::hashed("gameuiBaseWorldMapMappinController")
        {
            return false;
        }
        if name == CName::hashed("gameuiMappinBaseController") {
            return true;
        }
        class = c.parent();
    }
    false
}
fn set_widget_pivot(root: &Handle, mut pivot: [f32; 2]) {
    let w = root.instance as usize;
    let current = vec2(w, WIDGET_PIVOT);
    if current[0] == pivot[0] && current[1] == pivot[1] {
        return;
    }
    // Two script overloads share the short name: (pivot: Vector2) native, (x, y) scripted.
    let Some(setter) =
        class_of(w).and_then(|c| c.function(CName::hashed("SetRenderTransformPivot")))
    else {
        return;
    };
    let (x, y) = pivot.split_at_mut(1);
    let mut args: Vec<StackType> = Vec::new();
    if setter.params.len() == 1 && param_type_name(setter, 0) == Some(VECTOR2) {
        args.push(StackType {
            ty: core::ptr::null(),
            value: x.as_mut_ptr() as *mut c_void,
        });
    } else if setter.params.len() == 2 {
        args.push(StackType {
            ty: core::ptr::null(),
            value: x.as_mut_ptr() as *mut c_void,
        });
        args.push(StackType {
            ty: core::ptr::null(),
            value: y.as_mut_ptr() as *mut c_void,
        });
    } else {
        return;
    }
    // SAFETY: the widget's own function.
    unsafe { execute(w, setter, core::ptr::null_mut(), &mut args) };
}
static SCALE_WARNED: AtomicBool = AtomicBool::new(false);
fn set_widget_scale(root: &Handle, mut scale: [f32; 2]) -> bool {
    let w = root.instance as usize;
    let setter = class_of(w).and_then(|c| c.function(CName::hashed("SetScale")));
    let Some(setter) =
        setter.filter(|s| s.params.len() == 1 && param_type_name(s, 0) == Some(VECTOR2))
    else {
        if !SCALE_WARNED.swap(true, Ordering::Relaxed) {
            log!("[world-markers] SetScale(Vector2) unavailable; original marker size retained\n");
        }
        return false;
    };
    // The engine setter preserves the pivot and invalidates paint.
    let mut args = [StackType {
        ty: core::ptr::null(),
        value: scale.as_mut_ptr() as *mut c_void,
    }];
    // SAFETY: the widget's own function.
    unsafe { execute(w, setter, core::ptr::null_mut(), &mut args) }
}

// ---- the marker roots

struct RootDepth {
    root: WeakHandle,
    inverse_depth: f32,
    stamp: u64,
    scale: MarkerScale,
    inherit_depth: bool,
    source: SourceProjection,
    anchor: MarkerAnchor,
    saved_cache_pivot: [f32; 2],
    owns_cache_pivot: bool,
    hidden: bool,
}
impl RootDepth {
    fn new(root: WeakHandle) -> RootDepth {
        RootDepth {
            root,
            inverse_depth: 0.0,
            stamp: 0,
            scale: MarkerScale::default(),
            inherit_depth: false,
            source: NO_SOURCE,
            anchor: NO_ANCHOR,
            saved_cache_pivot: [0.0; 2],
            owns_cache_pivot: false,
            hidden: false,
        }
    }
    /// The entry is its widget's (a destroyed root's address is handed to new widgets).
    fn of(&self, widget: usize) -> bool {
        self.root.lock().instance as usize == widget
    }
}
struct Roots {
    map: FastMap<usize, RootDepth>,
    next_prune: u64,
}
// SAFETY: the weak references are only touched under the lock; their counts are the engine's atomics.
unsafe impl Send for Roots {}
unsafe impl Sync for Roots {}
static ROOTS: RwLock<Option<Roots>> = RwLock::new(None);

fn roots_write() -> std::sync::RwLockWriteGuard<'static, Option<Roots>> {
    let mut g = ROOTS.write().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(Roots {
            map: cvr_host::fast::fast_map(),
            next_prune: 0,
        });
    }
    g
}

#[allow(clippy::too_many_arguments)]
fn remember(
    root: &Handle,
    world: Option<&[f32; 3]>,
    valid: bool,
    scale_marker: bool,
    inherit_depth: bool,
    mut pin_to_anchor: bool,
    source: Option<&SourceProjection>,
    cache_pivot: Option<[f32; 2]>,
    cached: bool,
    hidden: bool,
) {
    if root.is_null() || !hud_root(root) {
        return;
    }
    let w = root.instance as usize;
    let depth = match (valid, world) {
        (true, Some(world)) => match source {
            Some(s) if s.valid() => source_depth(s, world),
            _ => {
                let mut frame = LocatedCameraFrame::default();
                // SAFETY: the camera link's own read.
                if unsafe { cvr_camera_located_frame_read(&mut frame) } {
                    forward_depth(world, &frame.world_pos, &frame.world_quat)
                } else {
                    0.0
                }
            }
        },
        _ => 0.0,
    };
    let t = now();
    let mut target = [0.0f32; 2];
    let mut anchor = vec2(w, WIDGET_ANCHOR_POINT);
    let native = vec2(w, WIDGET_SCALE);
    let mut resize = false;
    {
        let mut guard = roots_write();
        let roots = guard.as_mut().unwrap();
        let entry = roots
            .map
            .entry(w)
            .or_insert_with(|| RootDepth::new(WeakHandle::null()));
        if !entry.of(w) {
            *entry = RootDepth::new(weak_of(root));
        }
        entry.inverse_depth = if depth > 0.0 { 1.0 / depth } else { 0.0 };
        entry.stamp = t;
        entry.source = match source {
            Some(s) if depth > 0.0 => *s,
            _ => NO_SOURCE,
        };
        entry.anchor = NO_ANCHOR;
        if let (true, Some(s), Some(world)) = (depth > 0.0, source, world) {
            entry.anchor = anchor_of(s, world);
        }
        entry.inherit_depth = inherit_depth;
        entry.hidden = hidden;
        if cached {
            if let Some(pivot) = cache_pivot {
                if !entry.owns_cache_pivot {
                    entry.saved_cache_pivot = vec2(w, WIDGET_PIVOT);
                }
                entry.owns_cache_pivot = true;
                anchor = pivot;
                pin_to_anchor = true;
            } else if entry.owns_cache_pivot {
                anchor = entry.saved_cache_pivot;
                pin_to_anchor = true;
                entry.owns_cache_pivot = false;
            }
        } else if pin_to_anchor && !entry.owns_cache_pivot {
            // A live root's own pivot, for the hand-back when its controller caches it (release_cached_root).
            entry.saved_cache_pivot = vec2(w, WIDGET_PIVOT);
            entry.owns_cache_pivot = true;
        }
        // Under the engine's stereo the HUD's design scale is 1.92x the one-eye root's (HudDesignRatio).
        if scale_marker {
            // SAFETY: exports of the C++ half.
            let ratio = crate::engine_stereo::cvr_engine_stereo_hud_design_ratio(
                unsafe { CyberpunkVR_HudRootWidth },
                unsafe { CyberpunkVR_HudRootHeight },
            );
            if let Some(scaled) = entry.scale.target(native[0], native[1], 0.7 * ratio) {
                target = [scaled.0, scaled.1];
                resize = true;
            }
        }
        // Entries of destroyed roots go once a second, not only past 512.
        if t >= roots.next_prune || roots.map.len() > 512 {
            roots.next_prune = t + 1000;
            // Scale ownership is kept while a hidden marker is still alive.
            roots
                .map
                .retain(|&k, v| !(t.wrapping_sub(v.stamp) > 2000 && !v.of(k)));
        }
        let count = roots.map.len() as u64;
        // SAFETY: an export of the C++ half.
        diag!(unsafe { CyberpunkVR_WorldMarkerRoots.store(count, Ordering::Relaxed) });
    }
    // SHRINK ABOUT THE POINT ON THE TARGET: a mappin's root hangs its content from its anchor point; a cached
    // picture from its cache origin.
    if pin_to_anchor {
        set_widget_pivot(root, anchor);
    }
    // Only native world-mappin roots are resized, never map icons or chatters.
    if resize && set_widget_scale(root, target) {
        let mut guard = roots_write();
        if let Some(entry) = guard.as_mut().unwrap().map.get_mut(&w) {
            if entry.of(w) {
                entry
                    .scale
                    .commit(target[0], target[1], native[0], native[1]);
            }
        }
    }
}

/// THE LIVE ROOT OF A CACHED MAPPIN IS THE GAME'S AGAIN: its scale and pivot handed back as the presentation
/// manager moves it into its offscreen cache canvas.
fn release_cached_root(root: &Handle) {
    if root.is_null() {
        return;
    }
    let w = root.instance as usize;
    {
        let g = ROOTS.read().unwrap_or_else(|e| e.into_inner());
        if !g.as_ref().is_some_and(|r| r.map.contains_key(&w)) {
            return;
        }
    }
    let mut scale = None;
    let mut pivot = None;
    {
        let mut guard = roots_write();
        let Some(entry) = guard.as_mut().unwrap().map.remove(&w) else {
            return;
        };
        if entry.of(w) {
            let current = vec2(w, WIDGET_SCALE);
            scale = entry.scale.restore(current[0], current[1]);
            if entry.owns_cache_pivot {
                pivot = Some(entry.saved_cache_pivot);
            }
        }
    }
    if let Some(p) = pivot {
        set_widget_pivot(root, p);
    }
    if let Some(s) = scale {
        set_widget_scale(root, [s.0, s.1]);
    }
}

// ---- the detours' trampolines

const MAPPIN_UPDATE: usize = 0;
const MAPPIN_POSITION: usize = 1;
const PROJECTION: usize = 2;
const PAINT: usize = 3;
const ALLOCATE: usize = 4;
const QUAD: usize = 5;
const TRIANGLES: usize = 6;
const TEXT: usize = 7;
const COMPOSITE: usize = 8;
const SNAP: usize = 9;
const SHOULD_CLAMP: usize = 10;
static ORIGINALS: [AtomicPtr<c_void>; 11] = [const { AtomicPtr::new(core::ptr::null_mut()) }; 11];
fn original<F: Copy>(i: usize) -> F {
    const { assert!(core::mem::size_of::<F>() == 8) };
    let p = ORIGINALS[i].load(Ordering::Acquire);
    // SAFETY: MinHook's trampoline of the function the detour stands in for, its signature F.
    unsafe { core::mem::transmute_copy::<*mut c_void, F>(&p) }
}

// ---- the mappins

/// A MARKER OFF THE SCREEN IS NOT HELD AT ITS EDGE (CyberpunkVR_WorldMarkerEdgeClamp 1 = the game's clamp).
fn edge_clamp_off(controller: usize) -> bool {
    // SAFETY: an export of the C++ half.
    controller != 0
        && relaxed(unsafe { &CyberpunkVR_WorldMarkerEdgeClamp }) == 0
        && enabled()
        && crate::engine_stereo::ui_one_eye()
        && world_mappin_controller(controller)
}
extern "C" fn should_clamp(controller: usize) -> bool {
    if edge_clamp_off(controller) {
        false
    } else {
        // SAFETY: the original.
        unsafe { original::<unsafe extern "C" fn(usize) -> bool>(SHOULD_CLAMP)(controller) }
    }
}

extern "C" fn mappin_update(controller: usize, context: usize) {
    let previous = tls(|t| t.mappin_view_context.replace(context));
    // SAFETY: the original.
    unsafe { original::<unsafe extern "C" fn(usize, usize)>(MAPPIN_UPDATE)(controller, context) };
    // Native clamp/visibility state is finalized AFTER SetScreenPosition.
    if reprojection_enabled() {
        collect_mappin(controller, (controller + 120) as *const f32);
    }
    tls(|t| t.mappin_view_context.set(previous));
}

/// What became of each kind of marker: a line per controller class and outcome, the first time (which markers the
/// port lifts into the world and where the others were left flat -- a partner's NightCoop marker, a mod's).
fn note_class(controller: usize, outcome: &str) {
    static SAID: Mutex<Option<std::collections::HashSet<(u64, u64)>>> = Mutex::new(None);
    let class = class_of(controller).map_or(CName(0), |c| c.class_name());
    let key = (class.0, CName::hashed(outcome).0);
    let fresh = SAID.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Default::default).insert(key);
    if fresh {
        log!("[world-markers] a {} marker: {outcome}\n", class.text());
    }
}
fn collect_mappin(controller: usize, position: *const f32) {
    if !enabled() {
        return;
    }
    // SAFETY: exports of the C++ half; the controller is the update's live MappinBaseController.
    unsafe {
        diag!(bump(&CyberpunkVR_WorldMarkerMappinStages[1]));
        let presentation = at::<u8>(controller, 0x190) != 0;
        let root = lock_weak(controller, if presentation { 0x170 } else { 0x40 });
        if root.is_null() {
            note_class(controller, "no root widget: left flat");
            return;
        }
        diag!(bump(&CyberpunkVR_WorldMarkerMappinStages[2]));
        if !world_mappin_controller(controller) {
            note_class(controller, "not a world mappin controller: left flat");
            return;
        }
        if presentation {
            release_cached_root(&lock_weak(controller, 0x40));
        }
        diag!(bump(&CyberpunkVR_WorldMarkerMappinStages[3]));
        if !hud_root(&root) {
            note_class(controller, "its widget is not on the HUD layer: left flat");
            return;
        }
        diag!(bump(&CyberpunkVR_WorldMarkerMappinStages[4]));
        // Visibility can be assigned later in this same native update; Paint itself gates hidden widgets.
        let vt = *(controller as *const *const usize);
        let clamped = core::mem::transmute::<usize, unsafe extern "C" fn(usize) -> bool>(
            *vt.add(0x158 / 8),
        )(controller);
        let mappin = lock_weak(controller, 0xA0);
        let mut world = [0.0f32; 4];
        let getter = if mappin.is_null() {
            None
        } else {
            class_of(mappin.instance as usize)
                .and_then(|c| c.function(CName::hashed("GetWorldPosition")))
        };
        // THE POINT IS KEPT EVEN WHEN THIS CONTROLLER DOES NOT PROJECT (+0xE3 off); a clamped edge icon is not at its
        // world point and keeps no depth.
        let valid = !clamped
            && getter.is_some_and(|g| {
                execute(
                    mappin.instance as usize,
                    g,
                    world.as_mut_ptr() as *mut c_void,
                    &mut [],
                )
            });
        let mut p = [world[0], world[1], world[2]];
        let mut source = NO_SOURCE;
        let context = tls(|t| t.mappin_view_context.get());
        type PointFn = unsafe extern "C" fn(usize, *mut f32) -> *const f32;
        if valid && reprojection_enabled() {
            // Outside the update the presentation manager re-applies the last update's screen point: the last
            // update's camera placed it.
            source = if context != 0 {
                capture_source(at::<usize>(context, 0), (context + 80) as *const f32)
            } else {
                current_ui_source()
            };
            let mut actual = [0.0f32; 4];
            let point = core::mem::transmute::<usize, PointFn>(*vt.add(312 / 8))(
                controller,
                actual.as_mut_ptr(),
            );
            if !point.is_null() && source.valid() {
                p.copy_from_slice(core::slice::from_raw_parts(point, 3));
            }
        }
        // UNCLAMPED THE ICON IS ON ITS POINT, on the screen or off it; ONLY WHERE THE PLAYER LOOKS
        // (CyberpunkVR_WorldMarkerOffCanvas 0) is a point one eye cannot see drawn at all.
        let unclamped = edge_clamp_off(controller);
        let mut at_point = valid;
        let mut behind = false;
        if valid && source.valid() {
            let a = anchor_of(&source, &p);
            let ahead = a.valid;
            let on_canvas = a.canvas[0] >= 0.0
                && a.canvas[0] <= source.width
                && a.canvas[1] >= 0.0
                && a.canvas[1] <= source.height;
            at_point = ahead && (unclamped || on_canvas);
            let in_sight = on_canvas && both_eyes_see(&p);
            behind = unclamped
                && (!ahead || (!in_sight && relaxed(&CyberpunkVR_WorldMarkerOffCanvas) == 0));
        }
        if valid && context != 0 && relaxed(&CyberpunkVR_WorldMarkerProbe) != 0 {
            let mut actual = [0.0f32; 4];
            let point = core::mem::transmute::<usize, PointFn>(*vt.add(312 / 8))(
                controller,
                actual.as_mut_ptr(),
            );
            if !point.is_null() {
                probe_view(
                    0,
                    root.instance as usize,
                    at::<usize>(context, 0),
                    point,
                    (context + 80) as *const f32,
                    position,
                    Some(&p),
                );
            }
        }
        // Stealth icons can be reparented into the NPC nameplate: they inherit its projection.
        let root_size = vec2(root.instance as usize, WIDGET_SIZE);
        let mut cache_pivot = None;
        if presentation && reprojection_enabled() {
            let (ok, x, y) = cached_marker_pivot(
                root_size[0],
                root_size[1],
                at::<f32>(controller, 0x168),
                at::<f32>(controller, 0x16C),
            );
            if ok {
                cache_pivot = Some([x, y]);
            }
        }
        remember(
            &root,
            Some(&p),
            at_point,
            true,
            at::<u8>(controller, 0xE3) == 0,
            !presentation,
            Some(&source),
            cache_pivot,
            presentation,
            behind,
        );
        trace_mappin(
            controller,
            root.instance as usize,
            &p,
            position,
            &source,
            at_point,
            clamped,
        );
        diag!(bump(&CyberpunkVR_WorldMarkerMappinStages[5]));
        if valid {
            diag!(bump(&CyberpunkVR_WorldMarkerMappinStages[6]));
        }
        note_class(
            controller,
            if !valid {
                if clamped {
                    "clamped to the edge: no depth"
                } else if getter.is_none() {
                    "its mappin has no GetWorldPosition: left flat"
                } else {
                    "its world point could not be read: left flat"
                }
            } else if at_point {
                "lifted to its world point"
            } else {
                "valid, not at its point this frame"
            },
        );
    }
}

/// THE MARKER'S MARGIN UNROUNDED: the exact margin back through the screen-position setter, only where it rounds to
/// what the game wrote.
fn exact_margin(controller: usize, screen: *const f32) {
    // SAFETY: exports of the C++ half; the controller is live, its widget held here.
    unsafe {
        if screen.is_null()
            || at::<u8>(controller, 0xE3) == 0
            || relaxed(&CyberpunkVR_WorldMarkerExactMargin) == 0
            || !world_mappin_controller(controller)
        {
            return;
        }
        let presentation = at::<u8>(controller, 0x190) != 0;
        let widget = lock_weak(controller, if presentation { 0x170 } else { 0x40 });
        if widget.is_null() {
            return;
        }
        let w = widget.instance as usize;
        let mut exact = [*screen, *screen.add(1)];
        if presentation {
            let mut adjust = [0.0f32; 2];
            type AdjustFn = unsafe extern "C" fn(usize, *mut f32, *const f32) -> *mut f32;
            core::mem::transmute::<usize, AdjustFn>(base() + 0x2232480)(
                controller,
                adjust.as_mut_ptr(),
                (w + WIDGET_SIZE) as *const f32,
            );
            exact[0] += at::<f32>(controller, 0x168) + adjust[0];
            exact[1] += at::<f32>(controller, 0x16C) + adjust[1];
        }
        let margin: [f32; 4] = at(w, WIDGET_MARGIN);
        if !exact[0].is_finite()
            || !exact[1].is_finite()
            || margin[2] != 0.0
            || margin[3] != 0.0
            || (margin[0] - exact[0]).abs() > 0.5001
            || (margin[1] - exact[1]).abs() > 0.5001
        {
            return;
        }
        if margin[0] == exact[0] && margin[1] == exact[1] {
            return;
        }
        let value = [exact[0], exact[1], 0.0, 0.0];
        core::mem::transmute::<usize, unsafe extern "C" fn(usize, *const f32)>(base() + 0x54B3A4)(
            w,
            value.as_ptr(),
        );
        diag!(bump(&CyberpunkVR_WorldMarkerExactMargins));
    }
}
extern "C" fn mappin_position(controller: usize, position: usize) -> usize {
    // SAFETY: an export of the C++ half; the original.
    unsafe {
        diag!(bump(&CyberpunkVR_WorldMarkerMappinStages[0]));
        let result = original::<unsafe extern "C" fn(usize, usize) -> usize>(MAPPIN_POSITION)(
            controller, position,
        );
        if reprojection_enabled() {
            exact_margin(controller, position as *const f32);
        }
        if !reprojection_enabled() || tls(|t| t.mappin_view_context.get()) == 0 {
            collect_mappin(controller, position as *const f32);
        }
        result
    }
}

// ---- the script projections

struct ProjectionOwner {
    projection: WeakHandle,
    widget: WeakHandle,
    stamp: u64,
}
struct Projections(FastMap<usize, ProjectionOwner>);
// SAFETY: as Roots.
unsafe impl Send for Projections {}
static PROJECTIONS: Mutex<Option<Projections>> = Mutex::new(None);

extern "C" fn projection(projection: usize, view: usize, size: usize, world_scale: f32) {
    // SAFETY: exports of the C++ half; the original; the projection is the update's live inkScreenProjection.
    unsafe {
        diag!(bump(&CyberpunkVR_WorldMarkerProjectionStages[0]));
        original::<unsafe extern "C" fn(usize, usize, usize, f32)>(PROJECTION)(
            projection,
            view,
            size,
            world_scale,
        );
        if !enabled() {
            return;
        }
        diag!(bump(&CyberpunkVR_WorldMarkerProjectionStages[1]));
        let mut root = Handle::null();
        {
            let guard = PROJECTIONS.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(owner) = guard.as_ref().and_then(|p| p.0.get(&projection)) {
                if owner.projection.lock().instance as usize == projection
                    && now().wrapping_sub(owner.stamp) < 1000
                {
                    root = owner.widget.lock();
                }
            }
        }
        if root.is_null() {
            let owner = lock_weak(projection, 0xD0);
            let logic = RttiSystem::get()
                .and_then(|r| r.get_class(CName::hashed("inkIWidgetLogicController")));
            let Some(logic) = logic else { return };
            if owner.is_null()
                || !class_of(owner.instance as usize).is_some_and(|c| c.is_a(logic.class_name()))
            {
                return;
            }
            root = lock_weak(owner.instance as usize, 0x40);
        }
        diag!(bump(&CyberpunkVR_WorldMarkerProjectionStages[2]));
        if root.is_null() || !hud_root(&root) {
            return;
        }
        diag!(bump(&CyberpunkVR_WorldMarkerProjectionStages[3]));
        // The projection update's own world point (2.5 sub_142142700: projection, out, view, worldScale).
        let mut world = [0.0f32; 3];
        type WorldFn = unsafe extern "C" fn(usize, *mut f32, usize, f32) -> *mut f32;
        core::mem::transmute::<usize, WorldFn>(base() + 0x2142700)(
            projection,
            world.as_mut_ptr(),
            view,
            world_scale,
        );
        probe_view(
            1,
            root.instance as usize,
            view,
            world.as_ptr(),
            size as *const f32,
            (projection + 272) as *const f32,
            None,
        );
        let source = capture_source(view, size as *const f32);
        remember(
            &root,
            Some(&world),
            true,
            false,
            false,
            false,
            Some(&source),
            None,
            false,
            false,
        );
        diag!(bump(&CyberpunkVR_WorldMarkerProjectionStages[4]));
    }
}

// ---- the paint

extern "C" fn paint(widget: usize, context: usize) {
    let (previous, previous_owner, previous_source, previous_anchor) = tls(|t| {
        (
            t.paint_depth.get(),
            t.paint_owner.get(),
            t.paint_source.get(),
            t.paint_anchor.get(),
        )
    });
    let mut hidden = false;
    if enabled() {
        let guard = ROOTS.read().unwrap_or_else(|e| e.into_inner());
        let map = guard.as_ref().map(|r| &r.map);
        // AN ENTRY IS ONLY ITS OWN WIDGET'S (a destroyed root's address may be a newer widget's).
        let entry = map.and_then(|m| m.get(&widget)).filter(|e| e.of(widget));
        if let Some(e) = entry {
            // A marker whose point is behind the head stays as its update last left it.
            hidden = e.hidden;
            let own = if now().wrapping_sub(e.stamp) < 1000 {
                e.inverse_depth
            } else {
                0.0
            };
            // An inheriting root keeps the depth of the projected root it is painted inside (a nameplate); with none
            // around it, it is painted at its own point.
            tls(|t| {
                if !e.inherit_depth || t.paint_depth.get() <= 0.0 {
                    t.paint_depth.set(own);
                    let lifted = own > 0.0;
                    t.paint_owner.set(if lifted { widget } else { 0 });
                    t.paint_source
                        .set(if lifted { e.source } else { NO_SOURCE });
                    t.paint_anchor
                        .set(if lifted { e.anchor } else { NO_ANCHOR });
                }
            });
        }
        // Diagnostic: a marker's widget painted by a pass of its own, outside its root's Paint.
        let (nesting, depth) = tls(|t| (t.paint_nesting.get(), t.paint_depth.get()));
        if nesting == 0 && depth <= 0.0 && entry.is_none() && diagnostics() && widget != 0 {
            if let Some(map) = map {
                // SAFETY: a live widget's parent chain.
                let mut parent = unsafe { lock_weak(widget, WIDGET_PARENT) };
                for i in 0..24 {
                    if parent.is_null() {
                        break;
                    }
                    let p = parent.instance as usize;
                    if map.get(&p).is_some_and(|f| f.of(p)) {
                        // SAFETY: exports of the C++ half.
                        unsafe {
                            CyberpunkVR_WorldMarkerDetachedPaints.fetch_add(1, Ordering::Relaxed);
                            let d = &mut *(&raw mut CyberpunkVR_WorldMarkerDetachedPaint);
                            d[0] = p as u64;
                            d[1] = widget as u64;
                            d[2] = (i + 1) as u64;
                            d[3] = now();
                        }
                        break;
                    }
                    // SAFETY: as above.
                    parent = unsafe { lock_weak(p, WIDGET_PARENT) };
                }
            }
        }
    } else {
        tls(|t| {
            t.paint_depth.set(0.0);
            t.paint_owner.set(0);
            t.paint_source.set(NO_SOURCE);
            t.paint_anchor.set(NO_ANCHOR);
        });
    }
    if !hidden {
        tls(|t| t.paint_nesting.set(t.paint_nesting.get() + 1));
        // SAFETY: the original.
        unsafe { original::<unsafe extern "C" fn(usize, usize)>(PAINT)(widget, context) };
        tls(|t| t.paint_nesting.set(t.paint_nesting.get() - 1));
    }
    tls(|t| {
        t.paint_depth.set(previous);
        t.paint_owner.set(previous_owner);
        t.paint_source.set(previous_source);
        t.paint_anchor.set(previous_anchor);
    });
}

// ---- the primitives allocated under a marker's paint

/// A pool can reallocate while collecting a frame: metadata by pool/index, the address index rebuilt on growth;
/// never ownership inferred from rectangles.
struct PrimitivePool {
    data: usize,
    depths: Vec<MarkerOwner>,
    stamp: u64,
}
struct PrimitiveDepthEntry {
    owner: usize,
    marker: MarkerOwner,
}
struct Primitives {
    pools: FastMap<usize, PrimitivePool>,
    primitives: FastMap<usize, PrimitiveDepthEntry>,
    next_prune: u64,
}
static PRIMITIVES: Mutex<Option<Primitives>> = Mutex::new(None);
const RECORD: usize = 0x130;

fn forget_pool_records(
    primitives: &mut FastMap<usize, PrimitiveDepthEntry>,
    owner: usize,
    pool: &PrimitivePool,
) {
    for i in 0..pool.depths.len() {
        let key = pool.data + i * RECORD;
        if primitives.get(&key).is_some_and(|e| e.owner == owner) {
            primitives.remove(&key);
        }
    }
}
extern "C" fn allocate(batch: usize) -> usize {
    // SAFETY: the original; the batch is the engine's, its owner pool's data pointer at +0.
    let result = unsafe { original::<unsafe extern "C" fn(usize) -> usize>(ALLOCATE)(batch) };
    if result == 0 || !HOOKS_READY.load(Ordering::Acquire) {
        return result;
    }
    // SAFETY: as above.
    let owner: usize = unsafe { at(batch, 0x28) };
    let data: usize = unsafe { at(owner, 0) };
    if result < data || (result - data) % RECORD != 0 || (result - data) / RECORD > 131072 {
        return result;
    }
    let index = (result - data) / RECORD;
    let marker = tls(|t| MarkerOwner {
        root: t.paint_owner.get(),
        depth: t.paint_depth.get(),
        source: t.paint_source.get(),
        anchor: t.paint_anchor.get(),
    });
    let mut guard = PRIMITIVES.lock().unwrap_or_else(|e| e.into_inner());
    let all = guard.get_or_insert_with(|| Primitives {
        pools: cvr_host::fast::fast_map(),
        primitives: cvr_host::fast::fast_map(),
        next_prune: 0,
    });
    let pool = all.pools.entry(owner).or_insert_with(|| PrimitivePool {
        data: 0,
        depths: Vec::new(),
        stamp: 0,
    });
    if index == 0 || pool.data != data {
        forget_pool_records(&mut all.primitives, owner, pool);
        if index == 0 {
            pool.depths.clear();
        } else {
            for (i, d) in pool.depths.iter().enumerate() {
                if d.depth > 0.0 {
                    all.primitives
                        .insert(data + i * RECORD, PrimitiveDepthEntry { owner, marker: *d });
                }
            }
        }
        pool.data = data;
    }
    pool.stamp = now();
    if pool.depths.len() <= index {
        pool.depths.resize(index + 1, MarkerOwner::default());
    }
    pool.depths[index] = marker;
    if marker.depth > 0.0 {
        all.primitives
            .insert(result, PrimitiveDepthEntry { owner, marker });
    } else {
        all.primitives.remove(&result);
    }
    let t = pool.stamp;
    if t >= all.next_prune {
        all.next_prune = t + 10000;
        let stale: Vec<usize> = all
            .pools
            .iter()
            .filter(|(_, p)| t.wrapping_sub(p.stamp) > 60000)
            .map(|(&k, _)| k)
            .collect();
        for k in stale {
            if let Some(p) = all.pools.remove(&k) {
                forget_pool_records(&mut all.primitives, k, &p);
            }
        }
    }
    result
}
fn primitive_marker(record: usize) -> MarkerOwner {
    if !enabled() {
        return MarkerOwner::default();
    }
    let mut guard = PRIMITIVES.lock().unwrap_or_else(|e| e.into_inner());
    let Some(all) = guard.as_mut() else {
        return MarkerOwner::default();
    };
    let Some(entry) = all.primitives.get(&record) else {
        return MarkerOwner::default();
    };
    let (owner, marker) = (entry.owner, entry.marker);
    if let Some(pool) = all.pools.get_mut(&owner) {
        pool.stamp = now();
    }
    marker
}

/// AN EFFECT IS ITS MARKER'S, EVEN WITH A FEW RECORDS OF NO MARKER IN IT; two markers in one effect keep it flat.
fn composite_owner(source: usize) -> MarkerOwner {
    let mut common = MarkerOwner::default();
    if source == 0 || !enabled() {
        return common;
    }
    // SAFETY: the composition's source, its batches and their records as the engine laid them out (2.5 0xC0
    // batches); exports of the C++ half.
    unsafe {
        let count: u32 = at(source, 0xC);
        let indices: *const u32 = at(source, 0);
        let batch_pool: usize = at(source, 0x10);
        if indices.is_null() || batch_pool == 0 || count > 4096 {
            return common;
        }
        let batches: usize = at(batch_pool, 0);
        let batch_count: u32 = at(batch_pool, 0x10);
        if batches == 0 || batch_count > 131072 {
            return common;
        }
        let (mut owned, mut ownerless) = (0u32, 0u32);
        {
            let guard = PRIMITIVES.lock().unwrap_or_else(|e| e.into_inner());
            let unknown = || {
                diag!(bump(&CyberpunkVR_WorldMarkerCompositeStats[5]));
                MarkerOwner::default()
            };
            let empty = cvr_host::fast::fast_map();
            let pools = guard.as_ref().map_or(&empty, |a| &a.pools);
            for i in 0..count as usize {
                let index = *indices.add(i);
                if index >= batch_count {
                    return unknown();
                }
                let batch = batches + index as usize * 0xC0; // 2.31: 0xB0
                let records: *const u32 = at(batch, 0x18);
                let size: u32 = at(batch, 0x30);
                if size == 0 {
                    continue;
                }
                if records.is_null() || size > 131072 {
                    return unknown();
                }
                let owner: usize = at(batch, 0x28);
                let Some(pool) = pools
                    .get(&owner)
                    .filter(|p| p.data == at::<usize>(owner, 0))
                else {
                    return unknown();
                };
                for j in 0..size as usize {
                    let r = *records.add(j) as usize;
                    if r >= pool.depths.len() {
                        return unknown();
                    }
                    let record = pool.depths[r];
                    if record.root == 0 || !(record.depth > 0.0) {
                        ownerless += 1;
                        continue;
                    }
                    if !merge_effect_owner(&mut common, record) {
                        diag!(bump(&CyberpunkVR_WorldMarkerCompositeStats[3]));
                        return MarkerOwner::default();
                    }
                    owned += 1;
                }
            }
        }
        if owned == 0 {
            diag!(bump(&CyberpunkVR_WorldMarkerCompositeStats[4]));
            return MarkerOwner::default();
        }
        if ownerless == 0 {
            diag!(bump(&CyberpunkVR_WorldMarkerCompositeStats[0]));
            return common;
        }
        diag!(bump(&CyberpunkVR_WorldMarkerCompositeStats[2]));
        let mixed = &mut *(&raw mut CyberpunkVR_WorldMarkerCompositeMixed);
        mixed[0] = common.root as u64;
        mixed[1] = owned as u64;
        mixed[2] = ownerless as u64;
        mixed[3] = now();
        if relaxed(&CyberpunkVR_WorldMarkerCompositeLoose) == 0 || ownerless > owned {
            return MarkerOwner::default();
        }
        diag!(bump(&CyberpunkVR_WorldMarkerCompositeStats[1]));
        common
    }
}
#[allow(clippy::too_many_arguments)]
extern "C" fn composite(
    context: usize,
    destination: usize,
    source: usize,
    writer: usize,
    texture: usize,
    bounds: usize,
    mode: u8,
) -> usize {
    let owner = composite_owner(source);
    let saved = tls(|t| {
        let saved = (
            t.paint_owner.get(),
            t.paint_depth.get(),
            t.paint_source.get(),
            t.paint_anchor.get(),
        );
        t.paint_owner.set(owner.root);
        t.paint_depth.set(owner.depth);
        t.paint_source.set(owner.source);
        t.paint_anchor.set(owner.anchor);
        t.offscreen_pass.set(t.offscreen_pass.get() + 1);
        saved
    });
    // SAFETY: the original.
    let result = unsafe {
        original::<unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, u8) -> usize>(
            COMPOSITE,
        )(context, destination, source, writer, texture, bounds, mode)
    };
    tls(|t| {
        t.offscreen_pass.set(t.offscreen_pass.get() - 1);
        t.paint_owner.set(saved.0);
        t.paint_depth.set(saved.1);
        t.paint_source.set(saved.2);
        t.paint_anchor.set(saved.3);
    });
    result
}

// ---- the vertices and glyphs

#[repr(C)]
struct VertexStream {
    geometry: usize,
    vertices: *mut u8,
    count: u32,
}
const _: () = assert!(core::mem::size_of::<VertexStream>() == 0x18);
const VERTEX: usize = 24;

/// The layers of one marker (CyberpunkVR_WorldMarkerLayerRoot), as emitted: kind 1 quad, 2 triangles, 3 text.
fn note_layer(
    record: usize,
    kind: u64,
    vertices: *const u8,
    count: u32,
    lift: bool,
    bounds: Option<&[f32; 4]>,
) {
    // SAFETY: exports of the C++ half; the vertices the emitter just wrote.
    unsafe {
        let n = (CyberpunkVR_WorldMarkerLayerCount.fetch_add(1, Ordering::Relaxed) % 64) as usize;
        let w = &mut (*(&raw mut CyberpunkVR_WorldMarkerLayerWords))[n];
        let f = &mut (*(&raw mut CyberpunkVR_WorldMarkerLayerFloats))[n];
        let offscreen = tls(|t| t.offscreen_pass.get()) != 0;
        w[0] = record as u64;
        w[1] = kind
            | if lift { 256 } else { 0 }
            | if offscreen { 512 } else { 0 }
            | ((count as u64) << 16);
        w[2] = now();
        w[3] = 0;
        if !vertices.is_null() && count != 0 {
            w[3] = core::ptr::read_unaligned(vertices.add(12) as *const u64);
        }
        if let Some(b) = bounds {
            f[..4].copy_from_slice(b);
        } else if !vertices.is_null() && count != 0 {
            let v = vertices as *const f32;
            f[0] = *v;
            f[2] = *v;
            f[1] = *v.add(1);
            f[3] = *v.add(1);
            for i in 1..count as usize {
                let v = vertices.add(i * VERTEX) as *const f32;
                f[0] = cvr_body::cmath::min(f[0], *v);
                f[1] = cvr_body::cmath::min(f[1], *v.add(1));
                f[2] = cvr_body::cmath::max(f[2], *v);
                f[3] = cvr_body::cmath::max(f[3], *v.add(1));
            }
        }
        f[4..8].fill(0.0);
        if !vertices.is_null() && count != 0 {
            let uv = core::ptr::read_unaligned(vertices.add(20) as *const u32);
            f[4] = (uv & 0xFFFF) as f32;
            f[5] = (uv >> 16) as f32;
            let uv = core::ptr::read_unaligned(
                vertices.add((count as usize - 1) * VERTEX + 20) as *const u32
            );
            f[6] = (uv & 0xFFFF) as f32;
            f[7] = (uv >> 16) as f32;
        }
    }
}
/// Level about the marker's point when it has one, the source camera's own axes otherwise.
fn lift_marker(marker: &MarkerOwner, x: f32, y: f32) -> Option<[f32; 3]> {
    // SAFETY: an export of the C++ half.
    if relaxed(unsafe { &CyberpunkVR_WorldMarkerLevel }) != 0 && marker.anchor.valid {
        return canvas_to_world_level(&marker.source, &marker.anchor, marker.depth, x, y);
    }
    canvas_to_world(&marker.source, marker.depth, x, y)
}
type EmitFn = unsafe extern "C" fn(usize, usize, *mut VertexStream);

fn emit(which: usize, context: usize, record: usize, stream: *mut VertexStream) {
    // SAFETY: the emitter's own stream; exports of the C++ half.
    unsafe {
        let stream = &mut *stream;
        let before = stream.count;
        let marker = primitive_marker(record);
        let encoded = encode_depth(marker.depth);
        // Pixel-snapped geometry, once lifted into the world, steps against it while the head moves (quad_snap).
        let unsnap = encoded != 0.0
            && can_lift(&marker.source)
            && relaxed(&CyberpunkVR_WorldMarkerUnsnap) != 0;
        let previous_unsnap = tls(|t| t.unsnap_vertices.replace(unsnap));
        original::<EmitFn>(which)(context, record, stream);
        tls(|t| t.unsnap_vertices.set(previous_unsnap));
        if encoded == 0.0
            || stream.vertices.is_null()
            || stream.count < before
            || stream.count - before > 65536
        {
            return;
        }
        let offscreen = tls(|t| t.offscreen_pass.get()) != 0;
        if relaxed(&CyberpunkVR_WorldMarkerProbe) != 0 && !offscreen && before < stream.count {
            let _lock = TRACE.lock().unwrap_or_else(|e| e.into_inner());
            CyberpunkVR_WorldMarkerTraceSequence.fetch_add(1, Ordering::AcqRel);
            let slot = trace_slot(marker.root);
            let w = &mut (*(&raw mut CyberpunkVR_WorldMarkerTraceWords))[slot];
            let f = &mut (*(&raw mut CyberpunkVR_WorldMarkerTraceFloats))[slot];
            if w[0] == marker.root as u64 {
                w[5] = now();
                w[6] = record as u64;
                w[7] = (w[7] & 15) | if can_lift(&marker.source) { 16 } else { 0 };
                let first = stream.vertices.add(before as usize * VERTEX) as *const f32;
                f[24] = *first;
                f[26] = *first;
                f[25] = *first.add(1);
                f[27] = *first.add(1);
                for n in before as usize + 1..stream.count as usize {
                    let v = stream.vertices.add(n * VERTEX) as *const f32;
                    f[24] = cvr_body::cmath::min(f[24], *v);
                    f[25] = cvr_body::cmath::min(f[25], *v.add(1));
                    f[26] = cvr_body::cmath::max(f[26], *v);
                    f[27] = cvr_body::cmath::max(f[27], *v.add(1));
                }
                f[28..31].copy_from_slice(&marker.source.origin);
                f[32..35].copy_from_slice(&marker.source.forward);
                if let Some(p) = lift_marker(&marker, (f[24] + f[26]) * 0.5, (f[25] + f[27]) * 0.5)
                {
                    f[36..39].copy_from_slice(&p);
                }
                f[39] = marker.depth;
                if let Some(p) = lift_marker(&marker, (f[24] + f[26]) * 0.5, f[25]) {
                    f[40..43].copy_from_slice(&p);
                }
            }
            if marker.root != 0
                && marker.root as u64
                    == core::ptr::read_volatile(&raw const CyberpunkVR_WorldMarkerLayerRoot)
            {
                note_layer(
                    record,
                    if which == TRIANGLES { 2 } else { 1 },
                    stream.vertices.add(before as usize * VERTEX),
                    stream.count - before,
                    can_lift(&marker.source),
                    None,
                );
            }
            CyberpunkVR_WorldMarkerTraceSequence.fetch_add(1, Ordering::Release);
        }
        for i in before as usize..stream.count as usize {
            let vertex = stream.vertices.add(i * VERTEX);
            let position = vertex as *mut f32;
            let alpha = vertex.add(18) as *mut u16;
            let z = position.add(2);
            let reason = if core::ptr::read_unaligned(alpha) & 0x8000 != 0 {
                0
            } else if *z != 0.0 && !(decode_depth(*z) > 0.0) {
                1
            } else if tls(|t| t.offscreen_pass.get()) != 0 {
                2
            } else if !can_lift(&marker.source) {
                3
            } else {
                4
            };
            if reason == 4 {
                if let Some(world) = lift_marker(&marker, *position, *position.add(1)) {
                    *position = world[0];
                    *position.add(1) = world[1];
                    *position.add(2) = world[2];
                    core::ptr::write_unaligned(alpha, core::ptr::read_unaligned(alpha) | 0x8000);
                    diag!(bump(&CyberpunkVR_WorldMarkerWorldVertices));
                    continue;
                }
            }
            diag!({
                bump(&CyberpunkVR_WorldMarkerUnlifted[reason]);
                (*(&raw mut CyberpunkVR_WorldMarkerUnliftedRoot))[reason] = marker.root as u64;
            });
            if *z == 0.0 {
                *z = encoded;
            }
        }
        diag!(bump(&CyberpunkVR_WorldMarkerQuads));
    }
}
extern "C" fn quad(context: usize, record: usize, stream: *mut VertexStream) {
    emit(QUAD, context, record, stream);
}
extern "C" fn triangles(context: usize, record: usize, stream: *mut VertexStream) {
    emit(TRIANGLES, context, record, stream);
}
/// 2.5: (record, writer, batch+50).
extern "C" fn text(record: usize, context: usize, batch_data: usize) {
    let marker = primitive_marker(record);
    let saved = tls(|t| {
        let saved = (
            t.text_depth.replace(marker.depth),
            t.text_source.replace(marker.source),
            t.text_anchor.replace(marker.anchor),
            t.text_exact.get(),
            t.text_exact_valid.get(),
            t.text_layer_root.replace(marker.root),
        );
        // The record's own translation, before the builder rounds it to the pixel (text_translation_exact).
        let valid = record != 0 && marker.depth > 0.0;
        t.text_exact_valid.set(valid);
        if valid {
            // SAFETY: the text record's translation.
            t.text_exact
                .set(unsafe { [at::<f32>(record, 0x10), at::<f32>(record, 0x14)] });
        }
        saved
    });
    // SAFETY: the original.
    unsafe {
        original::<unsafe extern "C" fn(usize, usize, usize)>(TEXT)(record, context, batch_data)
    };
    tls(|t| {
        t.text_exact.set(saved.3);
        t.text_exact_valid.set(saved.4);
        t.text_layer_root.set(saved.5);
        t.text_depth.set(saved.0);
        t.text_source.set(saved.1);
        t.text_anchor.set(saved.2);
    });
}
/// The quad emitter's vertex transform (sub_14028A6D0) rounds each vertex to the canvas pixel when the batch snaps:
/// not for a marker lifted into the world.
extern "C" fn quad_snap(
    context: usize,
    begin: *mut f32,
    end: *mut f32,
    matrix: *mut f32,
    snap: i8,
) {
    let snap = if tls(|t| t.unsnap_vertices.get()) {
        0
    } else {
        snap
    };
    // SAFETY: the original.
    unsafe {
        original::<unsafe extern "C" fn(usize, *mut f32, *mut f32, *mut f32, i8)>(SNAP)(
            context, begin, end, matrix, snap,
        )
    };
}

/// Called by the text thunk before the native geometry publication, while its writer still owns it.
///
/// # Safety
/// `geometry` is the text record's live geometry or null.
#[no_mangle]
pub unsafe extern "C" fn CvrWorldMarkerTextTag(geometry: usize) {
    // SAFETY: exports of the C++ half; the geometry's matrix (0x150) and depth slot (0x188) as the C++ file wrote them.
    unsafe {
        let probe = relaxed(&CyberpunkVR_WorldMarkerProbe) != 0;
        if geometry != 0 && probe {
            let last = &mut *(&raw mut CyberpunkVR_WorldMarkerTextLast);
            last[0] = geometry as u64;
            last[1] = now();
            last[2] = at::<usize>(geometry, 0x240) as u64;
            last[3] = current_node_view() as u64;
            last[4] = (tls(|t| t.ink_slot.get()) + 1) as i64 as u64;
        }
        let (text_source, text_anchor, text_depth, exact, exact_valid, layer_root) = tls(|t| {
            (
                t.text_source.get(),
                t.text_anchor.get(),
                t.text_depth.get(),
                t.text_exact.get(),
                t.text_exact_valid.get(),
                t.text_layer_root.get(),
            )
        });
        if geometry != 0 && can_lift(&text_source) {
            // Text colour is consumed by the native font renderer before the VS: its opacity stays.
            let level = relaxed(&CyberpunkVR_WorldMarkerLevel) != 0;
            let matrix = &mut *at_mut::<[f32; 16]>(geometry, 0x150);
            let layer = core::ptr::read_volatile(&raw const CyberpunkVR_WorldMarkerLayerRoot);
            if layer != 0 && probe && text_anchor.valid && layer_root as u64 == layer {
                let b = [
                    matrix[12],
                    matrix[13],
                    if exact_valid { exact[0] } else { 0.0 },
                    if exact_valid { exact[1] } else { 0.0 },
                ];
                note_layer(geometry, 3, core::ptr::null(), 0, true, Some(&b));
            }
            if exact_valid && relaxed(&CyberpunkVR_WorldMarkerUnsnap) != 0 {
                text_translation_exact(matrix, exact[0], exact[1]);
            }
            if text_to_world(
                matrix,
                &text_source,
                text_depth,
                if level { Some(&text_anchor) } else { None },
            ) {
                diag!(bump(&CyberpunkVR_WorldMarkerWorldTexts));
                return;
            }
        }
        let encoded = encode_depth(text_depth);
        if encoded != 0.0
            && geometry != 0
            && at::<f32>(geometry, 0x188) == 0.0
            && at::<f32>(geometry, 0x18C) == 1.0
        {
            *at_mut::<f32>(geometry, 0x188) = encoded;
            diag!(bump(&CyberpunkVR_WorldMarkerTexts));
        }
    }
}

// ---- the canvas camera per eye

/// The engine's stereo eye's window in the canvas's terms: the eyes share the centre's vertical span; with the eye
/// windows off they render the centre's window.
fn engine_eye_window(eye: i32) -> Option<crate::asym_projection::ViewWindow> {
    let centre = crate::asym_projection::view_window_of(0)?;
    Some(match crate::engine_stereo::eye_window(eye) {
        Some(own) => crate::asym_projection::ViewWindow {
            shift_x: own.shift_x,
            shift_y: own.shift_y,
            half_x: own.aspect * centre.half_y,
            half_y: centre.half_y,
        },
        None => centre,
    })
}
/// THE FLAT HUD UNDER THE ASYMMETRIC PROJECTION: the view's own shift and the canvas squeezed to the window's aspect.
/// `eye`: the engine's stereo eye (0 left, 1 right) the canvas is drawn into, -1 outside that mode.
/// As the C++ InkShift, `window` and `squeeze` are written once the window is known, even when the squeeze is then
/// refused.
fn ink_shift(
    side: i32,
    eye: i32,
    width: f32,
    height: f32,
    window: &mut crate::asym_projection::ViewWindow,
    squeeze: &mut f32,
) -> bool {
    let (lw, lh) = crate::engine_stereo::launcher_size();
    // SAFETY: an export of the C++ half.
    if relaxed(unsafe { &CyberpunkVR_AsymHudShift }) == 0
        || menu_mode()
        || lh <= 0
        || !(height > 0.0)
    {
        return false;
    }
    let across = if eye >= 0 && !crate::engine_stereo::ui_one_eye() {
        2.0f32
    } else {
        1.0
    };
    let aspect = across * lw as f32 / lh as f32;
    if (width / height - aspect).abs() > 0.01 * aspect {
        return false;
    }
    let found = if eye >= 0 {
        engine_eye_window(eye)
    } else {
        crate::asym_projection::view_window_of(side)
    };
    let Some(found) = found else { return false };
    *window = found;
    *squeeze = (width / height) / (window.half_x / window.half_y);
    squeeze.is_finite() && *squeeze > 0.5 * across && *squeeze < 2.0 * across
}
fn ads_zoom() -> f32 {
    // SAFETY: an export of the C++ half.
    let zoom = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_MainAdsZoomFactor) };
    if zoom.is_finite() && zoom > 0.05 {
        zoom
    } else {
        1.0
    }
}
/// The second eye's marker parallax, in canvas NDC per inverse metre.
fn eye_scale(width: f32, height: f32) -> f32 {
    let z = ads_zoom();
    // SAFETY: exports and functions of the C++ half.
    let main_right =
        unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_MainIsRightEye) } != 0;
    let half_ipd = unsafe { CyberpunkVRPort_HalfIpd() };
    let mut window = crate::asym_projection::ViewWindow::default();
    let mut squeeze = 1.0f32;
    if ink_shift(1, -1, width, height, &mut window, &mut squeeze) {
        // The second view's own window: ipd / distance radians, over its horizontal half span.
        let ipd = 2.0 * half_ipd;
        if !ipd.is_finite() || ipd <= 0.0 || ipd > 0.5 {
            return 0.0;
        }
        let scale = ipd * z / window.half_x / squeeze;
        return if main_right { scale } else { -scale };
    }
    // SAFETY: as above.
    stereo_scale(
        2.0 * half_ipd,
        unsafe { GetGameRenderFovDeg() },
        width / height,
        z,
        main_right,
    )
}
/// The engine's stereo: each eye half the IPD off the head centre the markers were placed from.
fn engine_eye_parallax(eye: i32, window: &crate::asym_projection::ViewWindow) -> f32 {
    let z = ads_zoom();
    // SAFETY: a function of the C++ half.
    let half = unsafe { CyberpunkVRPort_HalfIpd() };
    if !half.is_finite() || half <= 0.0 || half > 0.25 {
        return 0.0;
    }
    let scale = half * z / window.half_x;
    if eye == 0 {
        scale
    } else {
        -scale
    }
}
/// The width an eye's canvas camera's layout fills: one eye's of a camera two eyes wide while the UI is laid out for
/// one eye.
fn content_width(width: f32, height: f32) -> f32 {
    let (lw, lh) = crate::engine_stereo::launcher_size();
    if !crate::engine_stereo::ui_one_eye() || lh <= 0 || !(height > 0.0) {
        return width;
    }
    let aspect = 2.0 * lw as f32 / lh as f32;
    if (width / height - aspect).abs() <= 0.01 * aspect {
        0.5 * width
    } else {
        width
    }
}

/// RewriteConstants: the canvas camera moved per eye (and the eye packet appended under the engine's stereo), or a
/// direct text upload's depth tag decoded; `scratch` holds the block shipped, `shipped` its size.
///
/// # Safety
/// `source` holds `size` bytes, `scratch` WORLD_CAMERA_BYTES; `shipped` is null or writable. `side` and `view` are
/// the recording thread's t_view_side and t_current_node_view.
#[no_mangle]
pub unsafe extern "C" fn cvr_world_markers_rewrite_constants(
    size: u32,
    source: *const u8,
    scratch: *mut u8,
    shipped: *mut u32,
    side: i32,
    view: usize,
) -> bool {
    // SAFETY: as the caller vouches; exports of the C++ half.
    unsafe {
        if !shipped.is_null() {
            *shipped = size;
        }
        if source.is_null() || scratch.is_null() {
            return false;
        }
        let engine_stereo = crate::engine_stereo::enabled();
        let floats = core::slice::from_raw_parts(source as *const f32, size as usize / 4);
        if is_ink_camera(floats, size as usize) {
            let bytes = core::slice::from_raw_parts(source, size as usize);
            let p = floats;
            // The engine's stereo draws the UI once per eye; its centre (eye 0) draws none of the headset's.
            let eye = if engine_stereo {
                ink_camera_eye(bytes) - 1
            } else {
                -1
            };
            let slot = if engine_stereo { eye } else { side };
            if !(0..=1).contains(&slot) {
                return false;
            }
            tls(|t| {
                t.ink_slot.set(slot);
                let mut cameras = t.ink_cameras.get();
                cameras[slot as usize] = InkCamera {
                    width: p[188],
                    height: p[189],
                    parallax: 0.0,
                    squeeze: 1.0,
                    stamp: now(),
                    world_ready: false,
                };
                t.ink_cameras.set(cameras);
            });
            diag!(bump(&CyberpunkVR_WorldMarkerCameraUploads[slot as usize]));
            if tls(|t| t.offscreen_pass.get()) != 0 {
                return false;
            }
            let world_camera = if engine_stereo && enabled() {
                read_eye_camera(eye, p[188], p[189], view)
            } else {
                None
            };
            let world_ready = world_camera.is_some();
            let world_camera = world_camera.unwrap_or_default();
            let content = if engine_stereo {
                content_width(p[188], p[189])
            } else {
                p[188]
            };
            let mut window = crate::asym_projection::ViewWindow::default();
            let mut squeeze = 1.0f32;
            let shift = ink_shift(side, eye, content, p[189], &mut window, &mut squeeze);
            // Both eyes of the engine's stereo are off the centre the markers were placed from; of the port's own two
            // views only the second is.
            let parallax = enabled() && if engine_stereo { shift } else { side == 1 };
            tls(|t| {
                let mut cameras = t.ink_cameras.get();
                cameras[slot as usize].world_ready = world_ready;
                t.ink_cameras.set(cameras);
            });
            if !parallax && !shift && !world_ready {
                return false;
            }
            core::ptr::copy_nonoverlapping(source, scratch, size as usize);
            if engine_stereo {
                // The native prefix stays; the tail is cleared on a missed camera rather than left stale.
                core::ptr::copy_nonoverlapping(
                    &world_camera as *const WorldMarkerCamera as *const u8,
                    scratch.add(INK_CAMERA_BYTES as usize),
                    core::mem::size_of::<WorldMarkerCamera>(),
                );
                if !shipped.is_null() {
                    *shipped = WORLD_CAMERA_BYTES;
                }
                if world_ready {
                    diag!(bump(&CyberpunkVR_WorldMarkerWorldCameras[slot as usize]));
                }
            }
            let out = core::slice::from_raw_parts_mut(scratch as *mut f32, size as usize / 4);
            if parallax {
                out[2] = if engine_stereo {
                    engine_eye_parallax(eye, &window)
                } else {
                    eye_scale(p[188], p[189]) * squeeze
                };
            }
            if shift {
                shift_ink_camera(
                    out,
                    p,
                    size as usize,
                    squeeze,
                    window.shift_x,
                    window.shift_y,
                    content,
                );
            }
            let parallax_value = if parallax { out[2] } else { 0.0 };
            tls(|t| {
                let mut cameras = t.ink_cameras.get();
                cameras[slot as usize].parallax = parallax_value;
                cameras[slot as usize].squeeze = squeeze;
                t.ink_cameras.set(cameras);
            });
            return true;
        }
        // Direct uploads can decode here; batched native composition bypasses these uploaders.
        if size != 112 {
            return false;
        }
        // Engine stereo's VS decodes both direct and batched text.
        if engine_stereo {
            return false;
        }
        if text_matrix_depth(floats, size as usize) == 0.0 {
            return false;
        }
        core::ptr::copy_nonoverlapping(source, scratch, size as usize);
        let out = core::slice::from_raw_parts_mut(scratch as *mut f32, 28);
        let mut shift = 0.0f32;
        let slot = side;
        if (0..2).contains(&slot) {
            diag!(bump(&CyberpunkVR_WorldMarkerTextUploads[slot as usize]));
            let camera = tls(|t| t.ink_cameras.get())[slot as usize];
            // The camera's parallax is in view NDC after its squeeze; the text matrix moves in canvas pixels before it.
            if enabled()
                && tls(|t| t.offscreen_pass.get()) == 0
                && camera.width > 0.0
                && camera.squeeze > 0.0
                && now().wrapping_sub(camera.stamp) < 1000
            {
                shift = camera.parallax / camera.squeeze * camera.width * 0.5;
            }
        }
        reproject_text_matrix(out, shift);
        true
    }
}

// ---- the install

/// The text record's geometry publication (2.5 Text, sub_1422F63D0): `movups [rbx+10],xmm7`, then the call that
/// publishes rbx. The call's own target is redirected: the thunk tags the geometry and tail-jumps to the publication.
fn install_text_call() -> bool {
    let site = base() + 0x22F66C3;
    let expected = [0x0fu8, 0x11, 0x7b, 0x10, 0xe8, 0x58, 0x74, 0x98, 0xfe];
    // SAFETY: the game's code, read in place.
    if unsafe { core::slice::from_raw_parts(site as *const u8, expected.len()) } != expected {
        return false;
    }
    let call = site + 4;
    // SAFETY: the call's rel32.
    let relative: i32 = unsafe { core::ptr::read_unaligned((call + 1) as *const i32) };
    // SAFETY: the thunk's jump target, read by the thunk only after the patch below.
    unsafe {
        *(&raw mut CvrWorldMarkerTextPublishTarget) = (call as i64 + 5 + relative as i64) as usize
    };
    let Some(relay) = cvr_engine::trampoline::allocate(call, 14) else {
        return false;
    };
    let mut jump = [0u8; 14];
    jump[..6].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
    jump[6..].copy_from_slice(
        &(CvrWorldMarkerTextPublishDetour as *const () as usize as u64).to_le_bytes(),
    );
    // SAFETY: the arena's executable scratch, ours.
    unsafe { cvr_engine::trampoline::write_code(relay, &jump) };
    // SAFETY: the call instruction found above; the relay stands in for its callee with the same signature.
    unsafe { cvr_engine::trampoline::patch_call(call, relay) }
}

struct HookSite {
    rva: usize,
    bytes: &'static [u8],
    detour: usize,
}

/// The detours (signatures checked first; a function another module already detoured is chained in front of), then
/// the text call. Install of the hook stage Boot, order 83.
#[no_mangle]
pub extern "C" fn cvr_world_markers_install() -> bool {
    let hooks = [
        HookSite {
            rva: 0x548FC4,
            bytes: b"\x48\x8b\xc4\x48\x89\x58\x08\x55\x56\x57\x41\x54\x41\x55\x41\x56",
            detour: mappin_update as *const () as usize,
        },
        HookSite {
            rva: 0x54A0F8,
            bytes: b"\x48\x89\x5c\x24\x10\x48\x89\x7c\x24\x20\x55\x48\x8b\xec",
            detour: mappin_position as *const () as usize,
        },
        HookSite {
            rva: 0x2A4B18,
            bytes: b"\x48\x8b\xc4\x48\x89\x58\x10\x48\x89\x70\x18\x57\x48\x83\xec",
            detour: projection as *const () as usize,
        },
        HookSite {
            rva: 0x27F26C,
            bytes: b"\x48\x8b\xc4\x48\x89\x58\x08\x48\x89\x70\x10\x48\x89\x78\x18",
            detour: paint as *const () as usize,
        },
        HookSite {
            rva: 0x28894C,
            bytes: b"\x48\x8b\xc4\x48\x89\x58\x10\x48\x89\x68\x18\x48\x89\x70\x20",
            detour: allocate as *const () as usize,
        },
        HookSite {
            rva: 0x28A064,
            bytes: b"\x48\x8b\xc4\x48\x89\x58\x08\x4c\x89\x40\x18\x48\x89\x50\x10",
            detour: quad as *const () as usize,
        },
        HookSite {
            rva: 0x28A774,
            bytes: b"\x48\x8b\xc4\x48\x89\x58\x18\x48\x89\x48\x08\x55\x56\x57",
            detour: triangles as *const () as usize,
        },
        HookSite {
            rva: 0x22F63D0,
            bytes: b"\x48\x8b\xc4\x53\x55\x56\x48\x81\xec\xd0\x00\x00\x00",
            detour: text as *const () as usize,
        },
        HookSite {
            rva: 0x1F4490,
            bytes: b"\x48\x8b\xc4\x48\x89\x58\x08\x48\x89\x70\x10\x48\x89\x78\x20",
            detour: composite as *const () as usize,
        },
        HookSite {
            rva: 0x28A6D0,
            bytes: b"\x48\x89\x5c\x24\x08\x48\x89\x74\x24\x10\x57\x48\x83\xec\x30",
            detour: quad_snap as *const () as usize,
        },
        HookSite {
            rva: 0x54A0B4,
            bytes: b"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\xf9",
            detour: should_clamp as *const () as usize,
        },
    ];
    // A function another module already detoured (RedHotTools hooks the ink paint) starts with its 5-byte jmp and
    // keeps the rest of the prologue; MinHook relocates that jmp into the trampoline.
    for h in &hooks {
        // SAFETY: the game's code, read in place.
        let site =
            unsafe { core::slice::from_raw_parts((base() + h.rva) as *const u8, h.bytes.len()) };
        if site == h.bytes {
            continue;
        }
        if site[0] == 0xe9 && site[5..] == h.bytes[5..] {
            log!(
                "[world-markers] {:X} already detoured by another module; chaining in front\n",
                h.rva
            );
            continue;
        }
        log!(
            "[world-markers] signature mismatch at {:X}; native placement retained\n",
            h.rva
        );
        return false;
    }
    let mut made = 0;
    for (i, h) in hooks.iter().enumerate() {
        // SAFETY: MinHook on the game's functions, the trampoline into this module's slot.
        if unsafe {
            MH_CreateHook(
                (base() + h.rva) as *mut c_void,
                h.detour as *mut c_void,
                ORIGINALS[i].as_ptr(),
            )
        } != 0
        {
            break;
        }
        made += 1;
    }
    let mut ok = made == hooks.len();
    if ok {
        for h in &hooks {
            // SAFETY: as above.
            if unsafe { MH_EnableHook((base() + h.rva) as *mut c_void) } != 0 {
                ok = false;
                break;
            }
        }
    }
    if ok {
        ok = install_text_call();
    }
    if !ok {
        for h in &hooks[..made] {
            // SAFETY: as above.
            unsafe {
                MH_DisableHook((base() + h.rva) as *mut c_void);
                MH_RemoveHook((base() + h.rva) as *mut c_void);
            }
        }
        return false;
    }
    HOOKS_READY.store(true, Ordering::Release);
    log!("[world-markers] native projection ownership hooks installed\n");
    true
}

// ---- the script's markers

/// `world.w`: 1 lifted to its point, 0 left flat where it is, -1 not drawn at all (a point no eye sees: no marker
/// held at the screen's edge, as the port's mappins have none).
fn register_widget(widget: &Handle, world: &[f32; 4]) {
    let point = [world[0], world[1], world[2]];
    let source = current_ui_source();
    remember(
        widget,
        Some(&point),
        world[3] > 0.5,
        false,
        false,
        false,
        Some(&source),
        None,
        false,
        world[3] < -0.5,
    );
}
fn register_projection(widget: &Handle, projection: &Handle) {
    if widget.is_null() || !hud_root(widget) {
        return;
    }
    let mut guard = PROJECTIONS.lock().unwrap_or_else(|e| e.into_inner());
    let owners = &mut guard.get_or_insert_with(|| Projections(cvr_host::fast::fast_map())).0;
    let w = widget.instance as usize;
    let p = projection.instance as usize;
    owners.retain(|&k, o| {
        let live = o.widget.lock();
        !(live.is_null() || (live.instance as usize == w && k != p))
    });
    if !projection.is_null() {
        owners.insert(
            p,
            ProjectionOwner {
                projection: weak_of(projection),
                widget: weak_of(widget),
                stamp: now(),
            },
        );
    } else {
        drop(guard);
        remember(
            widget, None, false, false, false, false, None, None, false, false,
        );
    }
}

/// VRWorldMarkerAnchor(widget, world): script-owned world indicators use the same paint/vertex path as mappins.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRWorldMarkerAnchor(
    _this: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    let mut widget = Handle::null();
    let mut world = [0.0f32; 4];
    // SAFETY: the VM's frame: a handle and a Vector4.
    let Some(f) = (unsafe { frame.as_mut() }) else {
        return;
    };
    unsafe {
        f.get_parameter(&mut widget);
        f.get_parameter(&mut world);
    }
    f.finish_params();
    register_widget(&widget, &world);
}

/// VRWorldMarkerScreenPoint(world) -> Vector4: a world point on the UI's screen as the UI's own update places things
/// (x, y in -1..1 of the canvas with y down, z the depth, w 1); w 0 without a fresh UI camera or behind it.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRWorldMarkerScreenPoint(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut [f32; 4],
    _a4: i64,
) {
    let mut world = [0.0f32; 4];
    // SAFETY: the VM's frame: a Vector4.
    let Some(f) = (unsafe { frame.as_mut() }) else {
        return;
    };
    unsafe { f.get_parameter(&mut world) };
    f.finish_params();
    // SAFETY: the VM's result slot.
    let Some(out) = (unsafe { out.as_mut() }) else {
        return;
    };
    *out = [0.0; 4];
    let source = current_ui_source();
    let point = [world[0], world[1], world[2]];
    if !source.valid() {
        return;
    }
    let anchor = anchor_of(&source, &point);
    if !anchor.valid {
        return;
    }
    *out = [
        2.0 * anchor.canvas[0] / source.width - 1.0,
        2.0 * anchor.canvas[1] / source.height - 1.0,
        source_depth(&source, &point),
        1.0,
    ];
}

/// VRWorldMarkerProjection(widget, projection): the projection that places this marker root.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRWorldMarkerProjection(
    _this: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    let mut widget = Handle::null();
    let mut projection = Handle::null();
    // SAFETY: the VM's frame: two handles.
    let Some(f) = (unsafe { frame.as_mut() }) else {
        return;
    };
    unsafe {
        f.get_parameter(&mut widget);
        f.get_parameter(&mut projection);
    }
    f.finish_params();
    register_projection(&widget, &projection);
}

/// The native publication the text thunk jumps on to (formerly Hooks/WorldMarkers.cpp's; written at install).
#[no_mangle]
#[allow(non_upper_case_globals)]
pub static mut CvrWorldMarkerTextPublishTarget: usize = 0;
