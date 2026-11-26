//! DEBUG DRAW'S SHAPES PER EYE (main's 0.2.4 src/Stereo/DebugDrawStereo.cpp): the engine's debug fragments
//! (externals/cp2077_debug_draw) drawn once per eye under engine stereo.
//!
//! THE NODE IS NOT STEREO-AWARE. Debug Draw adds CRenderNode_RenderDebugFragments to the main render graph (after
//! ApplyFXAA, at render resolution). Its work (sub_142EA16B4) sets one viewport over the whole target (sub_140333A44),
//! binds one camera (sub_140297904(ctx, camera copy, w, h, 0, 1, 3)) and draws its fragment lists (sub_14229D450).
//! Under engine stereo the target holds both eyes side by side, so a shape came out once across the pair, cut at the
//! seam, seen from the centre camera. A stereo-aware node (sub_1422A1050) binds each eye's camera instead: with
//! camera+921 bit 0 set, the eye cameras at camera+976 (left) and +984 (right) -- the node's camera copy clones both.
//!
//! SO EACH LIST IS DRAWN TWICE. While the node runs (thread-local, its own thread), the viewport and the camera it sets
//! are recorded on their way through; at each list draw the left eye gets the left half of that viewport and its own
//! camera, the right eye the right half and its camera, and the node's own viewport and camera are put back after. The
//! node itself runs once. Its 2D lists (text) bind no camera and are left alone. Off with CyberpunkVR_DebugDrawStereo 0.
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::*};

/// 1 = Debug Draw's shapes drawn per eye under engine stereo; 0 = once over both eyes, as the engine does.
#[no_mangle]
pub static mut CyberpunkVR_DebugDrawStereo: i32 = 1;
/// Lists drawn per eye since start (diagnostics).
#[no_mangle]
pub static CyberpunkVR_DebugDrawStereoLists: AtomicU32 = AtomicU32::new(0);

/// The trampolines MinHook writes: the node's work, the viewport, the camera bind, the list draw.
#[allow(non_upper_case_globals)]
pub static cvr_dds_originals: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];

pub const DEBUG_FRAGMENTS_WORK_RVA: usize = 0x2EA_16B4;
pub const SET_VIEWPORT_RVA: usize = 0x33_3A44;
pub const BIND_CAMERA_RVA: usize = 0x29_7904;
pub const DRAW_FRAGMENT_LIST_RVA: usize = 0x229_D450;
/// sub_14033E358(ctx, bit): 98 = engine stereo this frame.
const FEATURE_BIT_RVA: usize = 0x33_E358;
const STEREO_FEATURE: i32 = 98;
const CAMERA_STEREO_FLAGS: usize = 921;
const CAMERA_LEFT: usize = 976;
const CAMERA_RIGHT: usize = 984;

/// sub_1403DECAC's viewport: x, y, width, height (pixels), min and max depth.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Viewport {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    min_depth: f32,
    max_depth: f32,
}
const _: () = assert!(core::mem::size_of::<Viewport>() == 24);

type WorkFn = unsafe extern "C" fn(*mut c_void, *mut u8) -> i64;
type SetViewportFn = unsafe extern "C" fn(*mut Viewport) -> i64;
type BindCameraFn = unsafe extern "C" fn(*mut u8, *mut u8, u64, u64, *mut c_void, u64, u64) -> i64;
type DrawListFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u64, u64, u64) -> i64;
type FeatureBitFn = unsafe extern "C" fn(*mut c_void, i32) -> bool;

#[derive(Clone, Copy)]
struct Camera {
    ctx: *mut u8,
    camera: *mut u8,
    w: u64,
    h: u64,
    block: *mut c_void,
    a6: u64,
    slot: u64,
}
struct NodeState {
    ctx: *mut u8,
    viewport: Option<Viewport>,
    camera: Option<Camera>,
}
thread_local! {
    /// The debug node running on this thread under engine stereo; null otherwise.
    static NODE: Cell<*mut NodeState> = const { Cell::new(null_mut()) };
}

fn original<T: Copy>(i: usize) -> T {
    let f = cvr_dds_originals[i].load(Acquire);
    // SAFETY: a trampoline of type T, written before the detour is enabled.
    unsafe { core::mem::transmute_copy(&f) }
}

fn stereo_frame(ctx: *mut u8) -> bool {
    let base = cvr_engine::code::image_base();
    if base == 0 || ctx.is_null() {
        return false;
    }
    // SAFETY: the engine's own feature test (through its detour when it has one), as the node calls it.
    unsafe { core::mem::transmute::<usize, FeatureBitFn>(base + FEATURE_BIT_RVA)(ctx.cast(), STEREO_FEATURE) }
}
/// The eye's camera (1 left, 2 right) the camera copy carries, or null.
///
/// # Safety
/// `camera` is the node's live camera copy or null.
unsafe fn eye_camera(camera: *mut u8, eye: i32) -> *mut u8 {
    // SAFETY: as the caller vouches.
    unsafe {
        if camera.is_null() || *camera.add(CAMERA_STEREO_FLAGS) & 1 == 0 {
            return null_mut();
        }
        *(camera.add(if eye == 1 { CAMERA_LEFT } else { CAMERA_RIGHT }) as *const *mut u8)
    }
}

/// # Safety
/// The engine's arguments.
pub unsafe extern "C" fn work(node: *mut c_void, ctx: *mut u8) -> i64 {
    let f: WorkFn = original(0);
    // SAFETY: the exported switch.
    if unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_DebugDrawStereo) } == 0 || !stereo_frame(ctx) {
        // SAFETY: the trampoline with the engine's arguments.
        return unsafe { f(node, ctx) };
    }
    let mut state = NodeState { ctx, viewport: None, camera: None };
    let outer = NODE.with(|n| n.replace(&mut state));
    // SAFETY: as above; the state outlives the call.
    let r = unsafe { f(node, ctx) };
    NODE.with(|n| n.set(outer));
    r
}

/// # Safety
/// The engine's argument.
pub unsafe extern "C" fn set_viewport(viewport: *mut Viewport) -> i64 {
    let s = NODE.with(|n| n.get());
    if !s.is_null() && !viewport.is_null() {
        // SAFETY: this thread's live node state and the engine's viewport.
        unsafe { (*s).viewport = Some(*viewport) };
    }
    let f: SetViewportFn = original(1);
    // SAFETY: the trampoline.
    unsafe { f(viewport) }
}

/// # Safety
/// The engine's arguments.
pub unsafe extern "C" fn bind_camera(ctx: *mut u8, camera: *mut u8, w: u64, h: u64, block: *mut c_void, a6: u64, slot: u64) -> i64 {
    let s = NODE.with(|n| n.get());
    // SAFETY: this thread's live node state.
    if let Some(s) = unsafe { s.as_mut() } {
        if ctx == s.ctx {
            s.camera = Some(Camera { ctx, camera, w, h, block, a6, slot });
        }
    }
    let f: BindCameraFn = original(2);
    // SAFETY: the trampoline.
    unsafe { f(ctx, camera, w, h, block, a6, slot) }
}

/// # Safety
/// The engine's arguments.
pub unsafe extern "C" fn draw_list(drawer: *mut c_void, draw_ctx: *mut c_void, list: u64, a4: u64, a5: u64) -> i64 {
    static SAID: AtomicBool = AtomicBool::new(false);
    let f: DrawListFn = original(3);
    let s = NODE.with(|n| n.get());
    // SAFETY: this thread's live node state.
    let state = unsafe { s.as_ref() };
    let (Some(viewport), Some(camera)) = (state.and_then(|s| s.viewport), state.and_then(|s| s.camera)) else {
        // SAFETY: the trampoline.
        return unsafe { f(drawer, draw_ctx, list, a4, a5) };
    };
    // SAFETY: the node's camera copy.
    let (left, right) = unsafe { (eye_camera(camera.camera, 1), eye_camera(camera.camera, 2)) };
    if left.is_null() || right.is_null() || viewport.w < 2 {
        // SAFETY: the trampoline.
        return unsafe { f(drawer, draw_ctx, list, a4, a5) };
    }
    if !SAID.swap(true, Relaxed) {
        cvr_host::log!(
            "[debugdraw-stereo] debug fragments drawn per eye: viewport {} x {} split in two, eye cameras {:016X} / {:016X}\n",
            viewport.w,
            viewport.h,
            left as usize,
            right as usize
        );
    }
    let set: SetViewportFn = original(1);
    let bind: BindCameraFn = original(2);
    let half = viewport.w / 2;
    let mut r = 0;
    // SAFETY (whole loop): the trampolines with the node's own recorded arguments.
    unsafe {
        for eye in 1..=2 {
            let mut v = viewport;
            if eye == 2 {
                v.x += half;
            }
            v.w = half;
            set(&mut v);
            bind(camera.ctx, if eye == 1 { left } else { right }, camera.w, camera.h, camera.block, camera.a6, camera.slot);
            r = f(drawer, draw_ctx, list, a4, a5);
        }
        // the node's own state again, for whatever it does next
        let mut own = viewport;
        set(&mut own);
        bind(camera.ctx, camera.camera, camera.w, camera.h, camera.block, camera.a6, camera.slot);
    }
    CyberpunkVR_DebugDrawStereoLists.fetch_add(1, Relaxed);
    r
}
