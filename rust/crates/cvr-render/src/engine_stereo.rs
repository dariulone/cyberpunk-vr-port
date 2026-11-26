//! The engine's own two-eye frame as the headset's stereo (formerly src/Stereo/EngineStereo.cpp; the launch switch,
//! the ini it reads and the mono mode stay there). What the mode changes and where is in Stereo/EngineStereo.hpp; why,
//! and the measurements, in docs/engine-stereo-plan.md.
//!
//! WHERE THE EYES COME FROM (IDA, 2.5). The view producer, when render feature 98 is on, asks the platform for eye
//! separation, the first-person plane's separation and convergence; the camera build clones each plane's descriptor
//! into a left and a right camera at centre -/+ 0.5 * sep * right, and the matrix builder gives them the centre's
//! symmetric projection with the off-axis terms of a frustum converged at `conv`. The camera's own NDC shift is applied
//! after that, so the port's asymmetric window still reaches both eyes through the clones. A headset needs the
//! wearer's own values: sep = the IPD the rest of the port uses, convergence parallel, the weapon the same as the world.
//!
//! EACH EYE ITS OWN LENS WINDOW: each eye camera is handed its own aspect and shift right before it is built
//! (Detour_CameraBuild). THE FIRST-PERSON PLANE'S EYES are put back at their own places after the plane's override
//! rebuild moved them to its centre (KeepEyePlaces / RestoreEyePlaces). THE UI IS LAID OUT FOR ONE EYE: the per-eye ink
//! layers (the HUD, notifications) are sized to one eye's window, ticked with it, and their world points placed through
//! a view narrowed to it. THE LOADING SCREEN IS ONE PICTURE: the loading layer's own state says when it is up.
//!
//! The exports, the switch (Enabled), the wearer's IPD, the layers' RTTI class names, the intro's loading note and the
//! main menu's state are the C++ half's; it attaches them here.
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use cvr_engine::code;
use cvr_host::log;

pub const SEPARATION_RVA: usize = 0x2284990;
pub const WEAPON_SEPARATION_RVA: usize = 0x22849D0;
pub const CONVERGENCE_RVA: usize = 0x2284950;
/// .data float, read only by the view producer: the first-person plane's convergence.
const WEAPON_CONVERGENCE_RVA: usize = 0x3D31790;
/// The view's render feature bits; 98 is the stereo frame.
const VIEW_FEATURES: usize = 0x25B0;
const STEREO_FEATURE: usize = 98;
/// Far enough that the off-axis term is noise: both eyes look straight ahead, as a headset's do.
const PARALLEL: f32 = 1.0e6;

pub const CAMERA_BUILD_RVA: usize = 0x296EA0;
const ASPECT: usize = 0x28;
const SHIFT_X: usize = 0x30;
const SHIFT_Y: usize = 0x34;
/// 1 left, 2 right.
const EYE_KIND: usize = 0x398;
/// Bit 1: an eye camera, built off-axis; bit 0 on a centre: it has the clones below.
const EYE_FLAGS: usize = 0x399;
const HAS_EYES: u8 = 1;
/// P, row-major: P00 +0x190, P11 +0x1A4, P20 +0x1B0, P21 +0x1B4.
const PROJECTION: usize = 0x190;
/// The TAA jitter in pixels, and the size the builder turns it into NDC with.
const JITTER: usize = 0x370;
const JITTER_SIZE: usize = 0x378;
const LEFT_EYE: usize = 0x3D0;
const RIGHT_EYE: usize = 0x3D8;
/// int x, y, z in 1/131072 m; the same as float x, y, z, 1.
const POSITION: usize = 0x0;
const WORLD_POSITION: usize = 0x280;

pub const LAYER_RESIZE_RVA: usize = 0x132654C;
pub const LAYER_TARGET_RVA: usize = 0x399094;
pub const LAYER_TICK_RVA: usize = 0x60787C;
const UI_VIEW_VTABLE_RVA: usize = 0x3620AA8;
pub const VIEW_PROJECT_RVA: usize = 0x54AD18;
pub const VIEW_PROJECT_OTHER_RVA: usize = 0x2144370;
pub const VIEW_UNPROJECT_RVA: usize = 0x29F403C;
const LAYER_SIZE: usize = 0x30;
/// inkLayer::active.
const LAYER_ACTIVE: usize = 0xB0;
/// The presents a loading layer still counts as ticked for: the frames of a load it is skipped in.
const LOADING_TICK_FRAMES: i64 = 30;

/// The detours' trampolines, in the C++ half's array.
pub mod slot {
    pub const SEPARATION: usize = 0;
    pub const WEAPON_SEPARATION: usize = 1;
    pub const CONVERGENCE: usize = 2;
    pub const CAMERA_BUILD: usize = 3;
    pub const LAYER_RESIZE: usize = 4;
    pub const LAYER_TARGET: usize = 5;
    pub const LAYER_TICK: usize = 6;
    pub const VIEW_PROJECT: usize = 7;
    pub const VIEW_PROJECT_OTHER: usize = 8;
    pub const VIEW_UNPROJECT: usize = 9;
    pub const COUNT: usize = 10;
}

/// What a layer is to the UI: drawn per eye (the HUD, the game's notifications), the loading screen, or other.
pub mod layer_kind {
    pub const OTHER: i32 = 0;
    pub const PER_EYE: i32 = 1;
    pub const LOADING: i32 = 2;
}

/// cvr::engine_stereo::EyeProjection.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyeProjection {
    pub aspect: f32,
    pub shift_x: f32,
    pub shift_y: f32,
}

/// cvr::engine_stereo::EyePlaces: the first-person plane's eye cameras and where they were.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyePlaces {
    pub eye: [usize; 2],
    pub position: [[i32; 3]; 2],
    pub world: [[f32; 4]; 2],
    pub valid: bool,
}

/// What the C++ half attaches.
#[repr(C)]
pub struct Runtime {
    pub eye_windows: *const i32,
    pub eye_jitter_width: *const i32,
    pub ui_one_eye: *const i32,
    pub loading_panel: *const i32,
    pub debug_frames: *mut u64,
    pub debug_values: *mut f32,
    pub debug_eye_builds: *mut u64,
    pub debug_eyes: *mut f32,
    pub debug_eye_jitter: *mut f32,
    pub debug_plane_eyes: *mut u64,
    pub debug_view_eyes: *mut f32,
    pub debug_ui_resizes: *mut u64,
    pub debug_ui_size: *mut i32,
    pub debug_ui_ticks: *mut u64,
    pub debug_loading_ticks: *mut u64,
    pub debug_loading_layer: *mut i32,
    pub launcher_width: *const i32,
    pub launcher_height: *const i32,
    /// The switch's exports: CyberpunkVR_EngineStereo, MonoMode, MonoView, Afw and IpdInWorldPos.
    pub engine_stereo: *mut i32,
    pub mono_mode: *mut i32,
    pub mono_view: *mut i32,
    pub afw: *mut i32,
    pub ipd_in_world_pos: *mut i32,
    /// cvr::afw::SetLaunch(turns != 0) and SetMode(turns); cvr::afw::Launch.
    pub afw_apply: extern "C" fn(turns: i32),
    pub afw_launch: extern "C" fn() -> bool,
    /// CyberpunkVR_SetVrcamEnabled (Stereo/SyncStereo.cpp).
    pub set_vrcam_enabled: extern "C" fn(on: u32),
    pub layer_kind: extern "C" fn(layer: *mut c_void) -> i32,
    pub armed: extern "C" fn() -> bool,
}
// SAFETY: exported variables and functions of the C++ half, alive for the process.
unsafe impl Sync for Runtime {}
unsafe impl Send for Runtime {}

/// The plugin's table is built in Rust (stereo_tables.rs).
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const crate::stereo_tables::ENGINE_STEREO).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());

fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached once with a pointer the C++ half keeps for the process.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}

/// The C++ half's exports and callbacks (Stereo/EngineStereo.cpp, at its static initialisation).
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}

unsafe fn get<T: Copy>(p: *const T) -> T {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(p) }
}
unsafe fn put<T: Copy>(p: *mut T, value: T) {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::write_volatile(p, value) }
}
unsafe fn bump(p: *mut u64) {
    // SAFETY: an export of the C++ half.
    unsafe { put(p, get(p).wrapping_add(1)) }
}

/// The trampolines MinHook writes (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_engine_stereo_originals: [AtomicPtr<c_void>; slot::COUNT] = [const { AtomicPtr::new(core::ptr::null_mut()) }; slot::COUNT];
fn original(_rt: &Runtime, index: usize) -> *mut c_void {
    cvr_engine_stereo_originals[index].load(Ordering::Acquire)
}

// ---- the launch's switch

/// What the launch decided, once: engine stereo on, the ini's own wish, a launch in the mono mode (AFW and OSR are of
/// its kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switch {
    pub enabled: bool,
    pub requested: bool,
    pub mono_launch: bool,
}
static SWITCH: OnceLock<Switch> = OnceLock::new();

/// `sscanf(line, "key=%d")` or `"key = %d"`: the key at the line's start, `=` with any whitespace around it, an
/// integer.
pub fn ini_int(line: &[u8], key: &[u8]) -> Option<i32> {
    let rest = line.strip_prefix(key)?;
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C);
    let rest = &rest[rest.iter().take_while(|b| space(b)).count()..];
    let rest = rest.strip_prefix(b"=")?;
    let rest = &rest[rest.iter().take_while(|b| space(b)).count()..];
    let (negative, digits) = match rest.first() {
        Some(b'-') => (true, &rest[1..]),
        Some(b'+') => (false, &rest[1..]),
        _ => (false, rest),
    };
    let count = digits.iter().take_while(|b| b.is_ascii_digit()).count();
    if count == 0 {
        return None;
    }
    let value = digits[..count].iter().fold(0i64, |v, &d| {
        (v * 10 + (d - b'0') as i64).min(i64::from(u32::MAX) + 1)
    });
    Some((if negative { -value } else { value }) as i32)
}

/// The file as `fgets` into 256 bytes reads it in text mode: lines through '\n' of at most 255 bytes, CR LF as LF,
/// ^Z the end.
pub fn text_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    let end = bytes.iter().position(|&b| b == 0x1A).unwrap_or(bytes.len());
    let mut lines = Vec::new();
    let mut line = Vec::new();
    let mut i = 0;
    while i < end {
        let b = bytes[i];
        i += 1;
        if b == b'\r' && i < end && bytes[i] == b'\n' {
            continue;
        }
        line.push(b);
        if b == b'\n' || line.len() == 255 {
            lines.push(core::mem::take(&mut line));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The ini's four switches: xr_engine_stereo (on unless it says 0), xr_mono_mode, xr_afw, xr_osr.
pub fn ini_switches(text: &[u8]) -> (i32, i32, i32, i32) {
    let (mut value, mut mono, mut afw, mut osr) = (1, 0, 0, 0);
    for line in text_lines(text) {
        if let Some(v) = ini_int(&line, b"xr_engine_stereo") {
            value = (v != 0) as i32;
        }
        if let Some(v) = ini_int(&line, b"xr_mono_mode") {
            mono = (v != 0) as i32;
        }
        if let Some(v) = ini_int(&line, b"xr_afw") {
            afw = (v != 0) as i32;
        }
        if let Some(v) = ini_int(&line, b"xr_osr") {
            osr = (v != 0) as i32;
        }
    }
    (value, mono, afw, osr)
}

/// ReadSwitch: on unless the ini says 0 (a fresh install's ini has no such key, and a 2.5 install belongs in engine
/// stereo); off for a launch in the mono mode or one whose eyes take turns (AFW, OSR).
fn read_switch(rt: &Runtime) -> Switch {
    let text = std::fs::read(cvr_host::paths::file(
        cvr_host::paths::Folder::Config,
        "vrport.ini",
    ))
    .unwrap_or_default();
    let (mut value, mono, afw, osr) = ini_switches(&text);
    let requested = value != 0;
    // AFW and OSR are launches of the mono mode's kind -- no engine stereo, no second view, MAIN through the union of
    // the lens windows -- with MAIN at one eye per frame instead of the head centre.
    let turns = if osr != 0 {
        2
    } else if afw != 0 {
        1
    } else {
        0
    };
    (rt.afw_apply)(turns);
    let mono_launch = mono != 0 || turns != 0;
    // SAFETY: the switch's exports.
    unsafe {
        put(rt.afw, turns);
        put(rt.mono_mode, mono);
        put(rt.mono_view, mono_launch as i32);
    }
    if turns != 0 && value != 0 {
        value = 0;
        log!(
            "[engine-stereo] OFF for this launch: {} (the eyes take turns, {} makes the other)\n",
            if turns == 2 { "xr_osr=1" } else { "xr_afw=1" },
            if turns == 2 {
                "the port's OSR"
            } else {
                "PureDark's frame warp"
            }
        );
    }
    // THE MONO MODE STARTS WITHOUT ENGINE STEREO: one picture for both eyes, the window one eye wide.
    if mono != 0 && value != 0 && turns == 0 {
        value = 0;
        log!("[engine-stereo] OFF for this launch: xr_mono_mode=1 (one picture for both eyes)\n");
    }
    // SAFETY: as above.
    unsafe { put(rt.engine_stereo, value) };
    if value != 0 {
        // MAIN is the head centre: the engine adds the eye offsets itself.
        // SAFETY: as above.
        unsafe { put(rt.ipd_in_world_pos, 0) };
        log!(
            "[engine-stereo] ON (xr_engine_stereo=1): engine side-by-side frame, VRCAM off, MAIN at the head centre\n"
        );
    }
    Switch {
        enabled: value != 0,
        requested,
        mono_launch,
    }
}

/// The switch, read at the first question once the C++ half is attached.
pub fn switch() -> Option<Switch> {
    if let Some(s) = SWITCH.get() {
        return Some(*s);
    }
    let rt = runtime()?;
    Some(*SWITCH.get_or_init(|| read_switch(rt)))
}

/// Enabled: the engine renders the headset's two eyes this launch.
pub fn enabled() -> bool {
    switch().is_some_and(|s| s.enabled)
}

pub fn requested() -> bool {
    switch().is_some_and(|s| s.requested)
}

pub fn mono_launch() -> bool {
    switch().is_some_and(|s| s.mono_launch)
}

static MONO_APPLIED: AtomicI32 = AtomicI32::new(-1);

/// ApplyMonoMode: the overlay's mono switch. Under engine stereo it takes effect at the next start; a launch that
/// started in mono stays mono (the second view's machinery is installed only for a launch that may render one).
pub fn apply_mono_mode(mono: bool) {
    let Some(rt) = runtime() else { return };
    // SAFETY: the switch's exports.
    unsafe { put(rt.mono_mode, mono as i32) };
    if enabled() {
        return;
    }
    let view = mono || mono_launch();
    if MONO_APPLIED.load(Ordering::Relaxed) == view as i32 {
        return;
    }
    MONO_APPLIED.store(view as i32, Ordering::Relaxed);
    let turns = (rt.afw_launch)();
    // MAIN at the head centre (the one picture is drawn from between the eyes), and the second view off; written on
    // the first apply as well, since the VRCAM request outlives a launch in the CET bridge. Under AFW MAIN is an eye.
    // SAFETY: as above.
    unsafe {
        put(rt.mono_view, view as i32);
        put(rt.ipd_in_world_pos, if view && !turns { 0 } else { 1 });
    }
    (rt.set_vrcam_enabled)(if view { 0 } else { 1 });
    log!(
        "[mono] {}\n",
        if turns {
            "AFW/OSR: MAIN draws the eyes in turn, VRCAM off"
        } else if view {
            "ON: one picture from the head centre for both eyes, VRCAM off"
        } else {
            "OFF: MAIN is one eye, VRCAM the other"
        }
    );
}

#[no_mangle]
pub extern "C" fn cvr_engine_stereo_enabled() -> bool {
    enabled()
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_requested() -> bool {
    requested()
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_mono_launch() -> bool {
    mono_launch()
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_apply_mono_mode(mono: bool) {
    apply_mono_mode(mono)
}

// ---- the eyes' values

static RENDERING: AtomicI32 = AtomicI32::new(-1);
/// The presents counted when MAIN was last prepared, plus one; 0 never.
static MAIN_PREPARED: AtomicU64 = AtomicU64::new(0);
static PRESENTS: AtomicU64 = AtomicU64::new(0);

struct EyeSlot {
    aspect: AtomicU32,
    shift_x: AtomicU32,
    shift_y: AtomicU32,
}
static EYES: [EyeSlot; 2] = [const {
    EyeSlot {
        aspect: AtomicU32::new(0),
        shift_x: AtomicU32::new(0),
        shift_y: AtomicU32::new(0),
    }
}; 2];
static EYES_VALID: AtomicBool = AtomicBool::new(false);
static JITTER_WIDTH: AtomicU32 = AtomicU32::new(0);
static JITTER_HEIGHT: AtomicU32 = AtomicU32::new(0);
/// The centre's (the view's) size, built right before its eyes.
static VIEW_JITTER_WIDTH: AtomicU32 = AtomicU32::new(0);
static VIEW_JITTER_HEIGHT: AtomicU32 = AtomicU32::new(0);

fn load_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}
fn store_f32(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

/// The separation the engine is handed: the IPD the rest of the port uses, or 64 mm when that is implausible.
pub fn separation_of(half_ipd: f32) -> f32 {
    let sep = 2.0 * half_ipd;
    if sep > 0.0 && sep < 1.0 { sep } else { 0.064 }
}

#[cfg(feature = "rust-build")]
extern "C" {
    /// GetDesiredHalfIpd's Rust half (cvr-rig's camera_math.rs, linked into the plugin).
    fn cvr_camera_desired_half_ipd() -> f32;
}
/// The probe and test builds link no rig: no desired IPD (separation_of falls back to 64 mm).
#[cfg(not(feature = "rust-build"))]
unsafe fn cvr_camera_desired_half_ipd() -> f32 {
    0.0
}
fn separation(rt: &Runtime) -> f32 {
    let _ = rt;
    // SAFETY: a plain getter (cvr-rig's camera_math.rs).
    separation_of(unsafe { cvr_camera_desired_half_ipd() })
}

/// Detour_Separation.
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_separation(_: *mut c_void) -> f32 {
    let Some(rt) = runtime() else { return 0.064 };
    let sep = separation(rt);
    // SAFETY: three floats.
    unsafe { put(rt.debug_values, sep) };
    sep
}

/// Detour_WeaponSeparation.
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_weapon_separation(_: *mut c_void) -> f32 {
    let Some(rt) = runtime() else { return 0.064 };
    let sep = separation(rt);
    // SAFETY: three floats.
    unsafe { put(rt.debug_values.add(1), sep) };
    sep
}

/// Detour_Convergence: parallel, and the first-person plane's cvar the producer reads right after this call.
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_convergence(_: *mut c_void) -> f32 {
    let base = code::image_base();
    if base != 0 {
        code::write::<f32>(base + WEAPON_CONVERGENCE_RVA, PARALLEL);
    }
    if let Some(rt) = runtime() {
        // SAFETY: three floats.
        unsafe { put(rt.debug_values.add(2), PARALLEL) };
    }
    PARALLEL
}

/// Detour_CameraBuild: each eye camera its own aspect and shift right before it is built, and the jitter size an eye
/// turns its jitter into NDC with.
///
/// # Safety
/// Called by the engine in place of the matrix builder, with a camera descriptor.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_camera_build(camera: *mut u8) -> i64 {
    let Some(rt) = runtime() else { return 0 };
    let mut eye: Option<usize> = None;
    // SAFETY: the engine's descriptor, as the C++ detour read and wrote it.
    unsafe {
        if !camera.is_null() {
            let flags = *camera.add(EYE_FLAGS);
            let size = camera.add(JITTER_SIZE) as *mut u32;
            if flags & 1 != 0 && *size > 1 && *size.add(1) > 1 {
                VIEW_JITTER_WIDTH.store(*size, Ordering::Relaxed);
                VIEW_JITTER_HEIGHT.store(*size.add(1), Ordering::Relaxed);
            }
            // Only the view's own width is halved: the plane and eye rebuilds keep the size they were last given.
            let view = VIEW_JITTER_WIDTH.load(Ordering::Relaxed);
            if flags & 2 != 0 && get(rt.eye_jitter_width) != 0 && view > 1 && *size == view {
                *size = view / 2;
            }
            let kind = *camera.add(EYE_KIND);
            if flags & 2 != 0
                && (kind == 1 || kind == 2)
                && get(rt.eye_windows) != 0
                && EYES_VALID.load(Ordering::Acquire)
            {
                let e = (kind - 1) as usize;
                let f = camera as *mut f32;
                *f.add(ASPECT / 4) = load_f32(&EYES[e].aspect);
                *f.add(SHIFT_X / 4) = load_f32(&EYES[e].shift_x);
                *f.add(SHIFT_Y / 4) = load_f32(&EYES[e].shift_y);
                eye = Some(e);
            }
        }
        let build: unsafe extern "C" fn(*mut u8) -> i64 =
            core::mem::transmute(original(rt, slot::CAMERA_BUILD));
        let r = build(camera);
        if let Some(e) = eye {
            bump(rt.debug_eye_builds.add(e));
            let f = camera as *const f32;
            let d = rt.debug_eyes.add(e * 7);
            let p = PROJECTION / 4;
            for (k, v) in [
                *f.add(ASPECT / 4),
                *f.add(SHIFT_X / 4),
                *f.add(SHIFT_Y / 4),
                *f.add(p),
                *f.add(p + 5),
                *f.add(p + 8),
                *f.add(p + 9),
            ]
            .iter()
            .enumerate()
            {
                put(d.add(k), *v);
            }
            // Builds without jitter (the history camera, rebuilds) carry a size of 0 or 1: only a real one is kept.
            let size = camera.add(JITTER_SIZE) as *const u32;
            let (w, h) = (*size, *size.add(1));
            if w > 1 && h > 1 {
                JITTER_WIDTH.store(w, Ordering::Relaxed);
                JITTER_HEIGHT.store(h, Ordering::Relaxed);
            }
            let j = rt.debug_eye_jitter.add(e * 4);
            put(j, *f.add(JITTER / 4));
            put(j.add(1), *f.add(JITTER / 4 + 1));
            put(j.add(2), w as f32);
            put(j.add(3), h as f32);
        }
        r
    }
}

/// PublishEyes: the eyes' own windows for their camera builds (Stereo/AsymProjection's view prepare).
pub fn publish_eyes(eyes: &[EyeProjection; 2], valid: bool) {
    if valid {
        for (slot, eye) in EYES.iter().zip(eyes) {
            store_f32(&slot.aspect, eye.aspect);
            store_f32(&slot.shift_x, eye.shift_x);
            store_f32(&slot.shift_y, eye.shift_y);
        }
    }
    EYES_VALID.store(valid, Ordering::Release);
}

/// EyeWindow: one eye's aspect and shift, while the eyes have their own windows.
pub fn eye_window(eye: i32) -> Option<EyeProjection> {
    let rt = runtime()?;
    // SAFETY: an export of the C++ half.
    if !(0..=1).contains(&eye)
        || unsafe { get(rt.eye_windows) } == 0
        || !EYES_VALID.load(Ordering::Acquire)
    {
        return None;
    }
    let slot = &EYES[eye as usize];
    let p = EyeProjection {
        aspect: load_f32(&slot.aspect),
        shift_x: load_f32(&slot.shift_x),
        shift_y: load_f32(&slot.shift_y),
    };
    (p.aspect > 0.05).then_some(p)
}

/// KeepEyePlaces: the first-person plane's eye cameras and their places, before the override rebuild moves them.
pub fn keep_eye_places(descriptor: usize) -> EyePlaces {
    let mut kept = EyePlaces::default();
    if !enabled()
        || descriptor == 0
        || code::read::<u8>(descriptor + EYE_FLAGS).is_none_or(|f| f & HAS_EYES == 0)
    {
        return kept;
    }
    for e in 0..2 {
        let Some(eye) = code::read::<usize>(descriptor + if e == 1 { RIGHT_EYE } else { LEFT_EYE })
            .filter(|&p| p != 0)
        else {
            return kept;
        };
        let (Some(position), Some(world)) = (
            code::read::<[i32; 3]>(eye + POSITION),
            code::read::<[f32; 4]>(eye + WORLD_POSITION),
        ) else {
            return EyePlaces::default();
        };
        kept.eye[e] = eye;
        kept.position[e] = position;
        kept.world[e] = world;
    }
    kept.valid = true;
    kept
}

/// RestoreEyePlaces: the eyes back at their own places, built again through the engine's entry (so the eye-window
/// detour gives each its window again).
pub fn restore_eye_places(kept: &EyePlaces) {
    let base = code::image_base();
    let Some(rt) = runtime() else { return };
    if !kept.valid || base == 0 {
        return;
    }
    for e in 0..2 {
        let eye = kept.eye[e];
        // Unmoved when the plane already matched its override and was not rebuilt.
        match code::read::<[i32; 3]>(eye + POSITION) {
            Some(now) if now == kept.position[e] => continue,
            Some(_) => {}
            None => return,
        }
        if !code::write::<[i32; 3]>(eye + POSITION, kept.position[e])
            || !code::write::<[f32; 4]>(eye + WORLD_POSITION, kept.world[e])
        {
            return;
        }
        // SAFETY: the engine's matrix builder on its own eye camera.
        unsafe {
            let build: unsafe extern "C" fn(*mut u8) -> i64 =
                core::mem::transmute(base + CAMERA_BUILD_RVA);
            build(eye as *mut u8);
            bump(rt.debug_plane_eyes.add(e));
        }
    }
}

fn note_camera_state(camera: usize) -> Option<[f32; 6]> {
    let jitter = code::read::<[f32; 2]>(camera + JITTER)?;
    let size = code::read::<[u32; 2]>(camera + JITTER_SIZE)?;
    let p = code::read::<[f32; 2]>(camera + PROJECTION + 8 * 4)?;
    Some([
        jitter[0],
        jitter[1],
        size[0] as f32,
        size[1] as f32,
        p[0],
        p[1],
    ])
}

/// NoteViewEyes: with diagnostics on, MAIN's world and first-person planes as the view producer left them.
pub fn note_view_eyes(world_plane: usize, first_person_plane: usize) {
    let Some(rt) = runtime() else { return };
    for (p, &centre) in [world_plane, first_person_plane].iter().enumerate() {
        if centre == 0 {
            continue;
        }
        let out = |slot: usize, values: [f32; 6]| {
            // SAFETY: the export, 2 x 3 x 6 floats.
            unsafe {
                for (k, v) in values.iter().enumerate() {
                    put(rt.debug_view_eyes.add((p * 3 + slot) * 6 + k), *v);
                }
            }
        };
        let Some(state) = note_camera_state(centre) else {
            return;
        };
        out(0, state);
        if code::read::<u8>(centre + EYE_FLAGS).is_none_or(|f| f & HAS_EYES == 0) {
            continue;
        }
        for e in 0..2 {
            if let Some(eye) =
                code::read::<usize>(centre + if e == 1 { RIGHT_EYE } else { LEFT_EYE })
                    .filter(|&p| p != 0)
            {
                if let Some(state) = note_camera_state(eye) {
                    out(1 + e, state);
                }
            }
        }
    }
}

/// NoteMainPrepared: whether the engine renders both eyes this frame (feature 98), counted and said when it changes.
pub fn note_main_prepared(view: usize) {
    let Some(rt) = runtime() else { return };
    if view == 0 || !enabled() {
        return;
    }
    let Some(bits) = code::read::<u64>(view + VIEW_FEATURES + (STEREO_FEATURE / 64) * 8) else {
        return;
    };
    let on = ((bits >> (STEREO_FEATURE % 64)) & 1) as i32;
    // SAFETY: two counters.
    unsafe { bump(rt.debug_frames.add(on as usize)) };
    MAIN_PREPARED.store(PRESENTS.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
    if RENDERING.swap(on, Ordering::Relaxed) != on {
        if on != 0 {
            log!("[engine-stereo] the engine renders both eyes (feature 98 on)\n");
        } else {
            log!(
                "[engine-stereo] the engine renders ONE eye (feature 98 off){} -- the headset shows it as one picture\n",
                if (rt.armed)() {
                    ""
                } else {
                    ": before the main menu"
                }
            );
        }
    }
}

pub fn rendering() -> bool {
    RENDERING.load(Ordering::Relaxed) == 1
}

pub fn note_present() {
    PRESENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn world_drawn() -> bool {
    let prepared = MAIN_PREPARED.load(Ordering::Relaxed);
    prepared != 0 && PRESENTS.load(Ordering::Relaxed) as i64 - prepared as i64 <= 1
}

pub fn eye_jitter_size() -> (u32, u32) {
    (
        JITTER_WIDTH.load(Ordering::Relaxed),
        JITTER_HEIGHT.load(Ordering::Relaxed),
    )
}

pub fn eye_size() -> (u32, u32) {
    (
        VIEW_JITTER_WIDTH.load(Ordering::Relaxed) / 2,
        VIEW_JITTER_HEIGHT.load(Ordering::Relaxed),
    )
}

/// HudDesignRatio: for a root two eyes wide, one eye's HUD design scale against the root's.
pub fn hud_design_ratio(enabled: bool, launcher: (i32, i32), width: f32, height: f32) -> f32 {
    let (lw, lh) = launcher;
    if !enabled || lw <= 0 || lh <= 0 || !(width > 0.0) || !(height > 0.0) {
        return 1.0;
    }
    // A root two eyes wide (the window); anything else is not the HUD this is about.
    let eye_width = height * lw as f32 / lh as f32;
    if (width - 2.0 * eye_width).abs() > 0.01 * width {
        return 1.0;
    }
    let own = (width / 3840.0).min(height / 2160.0);
    let eye = (eye_width / 3840.0).min(height / 2160.0);
    if own > 0.0 && eye > 0.0 {
        eye / own
    } else {
        1.0
    }
}

fn launcher(rt: &Runtime) -> (i32, i32) {
    // SAFETY: the C++ half's globals.
    unsafe { (get(rt.launcher_width), get(rt.launcher_height)) }
}
/// The launcher's render size (g_launcherWidth, g_launcherHeight); zeros before the C++ half attaches.
pub fn launcher_size() -> (i32, i32) {
    runtime().map_or((0, 0), launcher)
}

// ---- the UI laid out for one eye

static UI_ONE_EYE: AtomicBool = AtomicBool::new(false);

/// TwoEyesWide: a size two eyes of the launcher's render size wide.
pub fn two_eyes_wide(enabled: bool, launcher: (i32, i32), width: i32, height: i32) -> bool {
    let (lw, lh) = launcher;
    if !enabled || lw <= 0 || lh <= 0 || width <= 0 || height <= 0 {
        return false;
    }
    let eye_width = height as i64 * lw as i64 / lh as i64;
    (width as i64 - 2 * eye_width).abs() <= 2
}

fn wide(rt: &Runtime, width: i32, height: i32) -> bool {
    two_eyes_wide(enabled(), launcher(rt), width, height)
}

/// LayerWidth: one eye's of a window two eyes wide, for a layer drawn per eye while the port lays the UI out for one
/// eye.
fn layer_width(rt: &Runtime, layer: *mut c_void, width: i32, height: i32) -> i32 {
    // SAFETY: exports of the C++ half.
    unsafe {
        let per_eye =
            |rt: &Runtime| !layer.is_null() && (rt.layer_kind)(layer) == layer_kind::PER_EYE;
        if get(rt.ui_one_eye) != 0 && wide(rt, width, height) && per_eye(rt) {
            bump(rt.debug_ui_resizes);
            for (k, v) in [width, height, width / 2, height].iter().enumerate() {
                put(rt.debug_ui_size.add(k), *v);
            }
            if !UI_ONE_EYE.swap(true, Ordering::Relaxed) {
                log!(
                    "[engine-stereo] UI laid out for one eye: per-eye ink layers {}x{} instead of {}x{}\n",
                    width / 2,
                    height,
                    width,
                    height
                );
            }
            return width / 2;
        }
        bump(rt.debug_ui_resizes.add(1));
        if wide(rt, width, height) && per_eye(rt) {
            UI_ONE_EYE.store(false, Ordering::Relaxed);
        }
        width
    }
}

/// Detour_LayerResize (inkLayer::Resize).
///
/// # Safety
/// Called by the engine.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_layer_resize(
    layer: *mut c_void,
    width: i32,
    height: i32,
) -> i64 {
    let Some(rt) = runtime() else { return 0 };
    let width = layer_width(rt, layer, width, height);
    // SAFETY: the engine's own function, through MinHook's trampoline.
    unsafe {
        let f: unsafe extern "C" fn(*mut c_void, i32, i32) -> i64 =
            core::mem::transmute(original(rt, slot::LAYER_RESIZE));
        f(layer, width, height)
    }
}

/// Detour_LayerTarget: a layer drawn into a target of its own, sized with it.
///
/// # Safety
/// Called by the engine.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_layer_target(
    layer: *mut c_void,
    width: i32,
    height: i32,
    target: *mut c_void,
) -> i64 {
    let Some(rt) = runtime() else { return 0 };
    let width = layer_width(rt, layer, width, height);
    // SAFETY: the engine's own function, through MinHook's trampoline.
    unsafe {
        let f: unsafe extern "C" fn(*mut c_void, i32, i32, *mut c_void) -> i64 =
            core::mem::transmute(original(rt, slot::LAYER_TARGET));
        f(layer, width, height, target)
    }
}

/// The tick a layer hands its controllers: the time, the view they place world points with, the screen's size.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LayerTick {
    pub time: *mut c_void,
    pub view: *mut c_void,
    pub size: [f32; 2],
    pub flag: u8,
    pub pad: [u8; 7],
}

/// The view's own two fields, then the port's narrowing. The tick's jobs keep the pointer past the tick, so these
/// live on; one per camera system.
#[repr(C)]
#[derive(Clone, Copy)]
struct OneEyeView {
    vtable: usize,
    camera_system: usize,
    narrow: f32,
}
struct OneEyeViews(UnsafeCell<[OneEyeView; 4]>);
// SAFETY: written under VIEWS_LOCK; read as the engine reads the view it was handed.
unsafe impl Sync for OneEyeViews {}
static VIEWS: OneEyeViews = OneEyeViews(UnsafeCell::new(
    [OneEyeView {
        vtable: 0,
        camera_system: 0,
        narrow: 0.0,
    }; 4],
));
static VIEWS_LOCK: Mutex<usize> = Mutex::new(0);

fn as_one_eye_view(view: *const c_void) -> Option<*const OneEyeView> {
    let first = VIEWS.0.get() as usize;
    let at = view as usize;
    (at >= first && at < first + core::mem::size_of::<[OneEyeView; 4]>())
        .then_some(view as *const OneEyeView)
}

fn one_eye_view_of(view: *mut c_void, narrow: f32) -> *mut c_void {
    let base = code::image_base();
    if view.is_null() || base == 0 || as_one_eye_view(view).is_some() {
        return view;
    }
    let (Some(vtable), Some(camera_system)) = (
        code::read::<usize>(view as usize),
        code::read::<usize>(view as usize + 8),
    ) else {
        return view;
    };
    if vtable != base + UI_VIEW_VTABLE_RVA || camera_system == 0 {
        return view;
    }
    let mut next = VIEWS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: the views are written under the lock.
    unsafe {
        let views = &mut *VIEWS.0.get();
        let index = match views.iter().rposition(|v| v.camera_system == camera_system) {
            Some(i) => i,
            None => {
                let i = *next % views.len();
                *next += 1;
                i
            }
        };
        let own = &mut views[index];
        if own.camera_system != camera_system || own.narrow != narrow {
            *own = OneEyeView {
                vtable,
                camera_system,
                narrow,
            };
        }
        own as *mut OneEyeView as *mut c_void
    }
}

/// UiViewNarrowing: how much a view of ours narrows the camera's NDC across (1 for any other view).
pub fn ui_view_narrowing(view: *const c_void) -> f32 {
    // SAFETY: one of our four views.
    as_one_eye_view(view).map_or(1.0, |own| unsafe { (*own).narrow })
}

// ---- the loading screen

static LOADING_LAYER: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// The presents counted when the layer was last ticked, plus one; 0 never.
static LOADING_TICK: AtomicU64 = AtomicU64::new(0);

fn layer_active(layer: usize) -> bool {
    code::read::<u8>(layer + LAYER_ACTIVE).is_some_and(|b| b != 0)
}

fn note_layer_tick(rt: &Runtime, layer: *mut c_void) {
    if layer.is_null() || (rt.layer_kind)(layer) != layer_kind::LOADING {
        return;
    }
    LOADING_LAYER.store(layer, Ordering::Relaxed);
    LOADING_TICK.store(PRESENTS.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
    // SAFETY: an export.
    unsafe { bump(rt.debug_loading_ticks) };
    crate::intro_boot::note_loading_layer(layer_active(layer as usize));
}

/// Detour_LayerTick: a per-eye layer's controllers ticked with the layer's own screen and a view narrowed to it.
///
/// # Safety
/// Called by the engine.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_layer_tick(
    layer: *mut c_void,
    tick: *const LayerTick,
    jobs: *mut c_void,
) -> i64 {
    let Some(rt) = runtime() else { return 0 };
    note_layer_tick(rt, layer);
    // SAFETY: the engine's layer and tick; the engine's own function through MinHook's trampoline.
    unsafe {
        let f: unsafe extern "C" fn(*mut c_void, *const LayerTick, *mut c_void) -> i64 =
            core::mem::transmute(original(rt, slot::LAYER_TICK));
        if !layer.is_null() && !tick.is_null() && UI_ONE_EYE.load(Ordering::Relaxed) {
            let own = (layer as *const u8).add(LAYER_SIZE) as *const f32;
            let (own_w, own_h) = (*own, *own.add(1));
            let t = &*tick;
            if own_w > 0.0
                && own_h == t.size[1]
                && wide(rt, t.size[0] as i32, t.size[1] as i32)
                && !wide(rt, own_w as i32, own_h as i32)
            {
                let mut one = *t;
                one.size[0] = own_w;
                one.view = one_eye_view_of(t.view, t.size[0] / own_w);
                bump(rt.debug_ui_ticks);
                return f(layer, &one, jobs);
            }
        }
        f(layer, tick, jobs)
    }
}

/// Detour_ViewProject: NDC across through a view of ours, narrowed to the layer.
///
/// # Safety
/// Called by the engine.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_view_project(
    view: *mut c_void,
    ndc: *mut f32,
    world: *const c_void,
) -> *mut f32 {
    project(slot::VIEW_PROJECT, view, ndc, world)
}

/// Detour_ViewProjectOther.
///
/// # Safety
/// Called by the engine.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_view_project_other(
    view: *mut c_void,
    ndc: *mut f32,
    world: *const c_void,
) -> *mut f32 {
    project(slot::VIEW_PROJECT_OTHER, view, ndc, world)
}

fn project(index: usize, view: *mut c_void, ndc: *mut f32, world: *const c_void) -> *mut f32 {
    let Some(rt) = runtime() else { return ndc };
    // SAFETY: the engine's own function through MinHook's trampoline; its result is its NDC.
    unsafe {
        let f: unsafe extern "C" fn(*mut c_void, *mut f32, *const c_void) -> *mut f32 =
            core::mem::transmute(original(rt, index));
        let out = f(view, ndc, world);
        if let Some(own) = as_one_eye_view(view) {
            if !out.is_null() {
                *out *= (*own).narrow;
            }
        }
        out
    }
}

/// Detour_ViewUnproject: the layer's NDC back to the camera's before the engine unprojects it.
///
/// # Safety
/// Called by the engine.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_view_unproject(
    view: *mut c_void,
    world: *mut c_void,
    ndc: *const f32,
) -> *mut c_void {
    let Some(rt) = runtime() else { return world };
    // SAFETY: the engine's own function through MinHook's trampoline.
    unsafe {
        let f: unsafe extern "C" fn(*mut c_void, *mut c_void, *const f32) -> *mut c_void =
            core::mem::transmute(original(rt, slot::VIEW_UNPROJECT));
        if let Some(own) = as_one_eye_view(view) {
            if !ndc.is_null() {
                let camera = [*ndc / (*own).narrow, *ndc.add(1)];
                return f(view, world, camera.as_ptr());
            }
        }
        f(view, world, ndc)
    }
}

pub fn ui_one_eye() -> bool {
    enabled() && UI_ONE_EYE.load(Ordering::Relaxed)
}

/// LoadingShown: the loading layer is up and was ticked within the last frames presented.
pub fn loading_shown() -> bool {
    let Some(rt) = runtime() else { return false };
    // The tick stands for the present after it: 0 presents since when the frame it was part of is the last
    // presented, -1 when the game has ticked the frame it builds before the last one was presented.
    let tick = LOADING_TICK.load(Ordering::Relaxed);
    let since = PRESENTS.load(Ordering::Relaxed) as i64 - tick as i64;
    // SAFETY: exports of the C++ half.
    unsafe {
        put(
            rt.debug_loading_layer,
            if tick != 0 {
                since.clamp(i32::MIN as i64 + 1, i32::MAX as i64) as i32
            } else {
                i32::MIN
            },
        );
        if !enabled() || get(rt.loading_panel) == 0 || tick == 0 || since > LOADING_TICK_FRAMES {
            return false;
        }
        let layer = LOADING_LAYER.load(Ordering::Relaxed) as usize;
        let active = layer != 0 && layer_active(layer);
        put(
            rt.debug_loading_layer.add(1),
            if layer != 0 { active as i32 } else { -1 },
        );
        active
    }
}

pub fn eye_separation() -> f32 {
    runtime().map_or(0.064, separation)
}

// ---- the C++ names

#[no_mangle]
pub extern "C" fn cvr_engine_stereo_rendering() -> bool {
    rendering()
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_note_present() {
    note_present()
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_world_drawn() -> bool {
    world_drawn()
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_loading_shown() -> bool {
    loading_shown()
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_ui_one_eye() -> bool {
    ui_one_eye()
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_ui_view_narrowing(view: *const c_void) -> f32 {
    ui_view_narrowing(view)
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_note_main_prepared(view: *const u8) {
    note_main_prepared(view as usize)
}
/// # Safety
/// `eyes` points at two EyeProjections.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_publish_eyes(eyes: *const EyeProjection, valid: bool) {
    if eyes.is_null() {
        return;
    }
    // SAFETY: two EyeProjections.
    publish_eyes(unsafe { &*(eyes as *const [EyeProjection; 2]) }, valid)
}
/// # Safety
/// `out` is null or an EyeProjection.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_eye_window(eye: i32, out: *mut EyeProjection) -> bool {
    if out.is_null() || !(0..=1).contains(&eye) {
        return false;
    }
    let Some(rt) = runtime() else { return false };
    // SAFETY: an export of the C++ half; the caller's struct, written whenever the eyes are valid (as the C++ did).
    unsafe {
        if get(rt.eye_windows) == 0 || !EYES_VALID.load(Ordering::Acquire) {
            return false;
        }
        let slot = &EYES[eye as usize];
        *out = EyeProjection {
            aspect: load_f32(&slot.aspect),
            shift_x: load_f32(&slot.shift_x),
            shift_y: load_f32(&slot.shift_y),
        };
        (*out).aspect > 0.05
    }
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_eye_separation() -> f32 {
    eye_separation()
}
/// # Safety
/// `width` and `height` are u32s.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_eye_jitter_size(
    width: *mut u32,
    height: *mut u32,
) -> bool {
    let (w, h) = eye_jitter_size();
    // SAFETY: the caller's.
    unsafe {
        *width = w;
        *height = h;
    }
    w > 1 && h > 1
}
/// # Safety
/// `width` and `height` are u32s.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_eye_size(width: *mut u32, height: *mut u32) -> bool {
    let (w, h) = eye_size();
    // SAFETY: the caller's.
    unsafe {
        *width = w;
        *height = h;
    }
    w > 1 && h > 1
}
/// # Safety
/// `out` is an EyePlaces.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_keep_eye_places(
    descriptor: *mut u8,
    out: *mut EyePlaces,
) {
    // SAFETY: the caller's.
    unsafe { *out = keep_eye_places(descriptor as usize) };
}
/// # Safety
/// `places` is an EyePlaces.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_restore_eye_places(places: *const EyePlaces) {
    // SAFETY: the caller's.
    restore_eye_places(unsafe { &*places })
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_note_view_eyes(world: *const u8, first_person: *const u8) {
    note_view_eyes(world as usize, first_person as usize)
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_hud_design_ratio(width: f32, height: f32) -> f32 {
    match runtime() {
        Some(rt) => hud_design_ratio(enabled(), launcher(rt), width, height),
        None => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_reads_the_ini_as_sscanf_did() {
        assert_eq!(
            ini_int(b"xr_engine_stereo=0\n", b"xr_engine_stereo"),
            Some(0)
        );
        assert_eq!(
            ini_int(b"xr_engine_stereo = 1\n", b"xr_engine_stereo"),
            Some(1)
        );
        assert_eq!(
            ini_int(b"xr_engine_stereo\t=\t-3", b"xr_engine_stereo"),
            Some(-3)
        );
        assert_eq!(
            ini_int(b" xr_engine_stereo=0", b"xr_engine_stereo"),
            None,
            "the key at the line's start only"
        );
        assert_eq!(ini_int(b"xr_engine_stereo=", b"xr_engine_stereo"), None);
        assert_eq!(ini_int(b"xr_engine_stereo_x=0", b"xr_engine_stereo"), None);
        assert_eq!(ini_int(b"xr_afw=12abc", b"xr_afw"), Some(12));
        // A missing ini, or one without the key: on.
        assert_eq!(ini_switches(b""), (1, 0, 0, 0));
        assert_eq!(
            ini_switches(b"xr_engine_stereo=0\r\nxr_mono_mode = 1\r\nxr_osr=5\r\n"),
            (0, 1, 0, 1)
        );
        // The last line for a key wins; ^Z ends the text.
        assert_eq!(
            ini_switches(b"xr_afw=1\nxr_afw=0\n\x1Axr_afw=1\n"),
            (1, 0, 0, 0)
        );
        // fgets' 255-byte pieces: a key past them starts a piece of its own.
        let mut long = vec![b'#'; 255];
        long.extend_from_slice(b"xr_mono_mode=1\n");
        assert_eq!(ini_switches(&long), (1, 1, 0, 0));
        assert_eq!(text_lines(b"a\r\nb\rc\n").len(), 2);
    }
    #[test]
    fn the_separation_is_the_wearers() {
        assert_eq!(separation_of(0.032), 0.064);
        assert!((separation_of(0.0315) - 0.063).abs() < 1e-7);
        assert_eq!(separation_of(0.0), 0.064);
        assert_eq!(separation_of(0.6), 0.064);
        assert_eq!(separation_of(f32::NAN), 0.064);
    }

    #[test]
    fn two_eyes_wide_is_twice_the_launchers_eye() {
        let launcher = (2064, 2232);
        assert!(two_eyes_wide(true, launcher, 4128, 2232));
        assert!(two_eyes_wide(true, launcher, 4130, 2232));
        assert!(!two_eyes_wide(true, launcher, 4131, 2232));
        assert!(!two_eyes_wide(true, launcher, 2064, 2232));
        assert!(!two_eyes_wide(false, launcher, 4128, 2232));
        assert!(!two_eyes_wide(true, (0, 2232), 4128, 2232));
        // Another resolution of the same aspect is still two eyes wide.
        assert!(two_eyes_wide(true, launcher, 2 * 1032, 1116));
    }

    #[test]
    fn the_hud_design_ratio() {
        let launcher = (2064, 2232);
        // Two eyes wide: bound by the height at the window, by the width at one eye.
        let r = hud_design_ratio(true, launcher, 4128.0, 2232.0);
        let own = (4128.0f32 / 3840.0).min(2232.0 / 2160.0);
        let eye = (2064.0f32 / 3840.0).min(2232.0 / 2160.0);
        assert!((r - eye / own).abs() < 1e-6);
        assert!(r < 1.0);
        assert_eq!(hud_design_ratio(true, launcher, 3840.0, 2160.0), 1.0);
        assert_eq!(hud_design_ratio(false, launcher, 4128.0, 2232.0), 1.0);
    }

    #[test]
    fn eyes_are_published_whole() {
        publish_eyes(
            &[
                EyeProjection {
                    aspect: 0.9,
                    shift_x: 0.2,
                    shift_y: -0.1,
                },
                EyeProjection {
                    aspect: 0.9,
                    shift_x: -0.2,
                    shift_y: -0.1,
                },
            ],
            true,
        );
        assert!(EYES_VALID.load(Ordering::Acquire));
        assert_eq!(load_f32(&EYES[1].shift_x), -0.2);
        // Invalid keeps the last values but says so.
        publish_eyes(&[EyeProjection::default(); 2], false);
        assert!(!EYES_VALID.load(Ordering::Acquire));
        assert_eq!(load_f32(&EYES[0].shift_x), 0.2);
    }

    #[test]
    fn views_of_ours_are_recognised_by_address() {
        assert!(as_one_eye_view(core::ptr::null()).is_none());
        let first = VIEWS.0.get() as *const c_void;
        assert!(as_one_eye_view(first).is_some());
        assert_eq!(ui_view_narrowing(core::ptr::null()), 1.0);
        let local = 0u64;
        assert!(as_one_eye_view(&local as *const u64 as *const c_void).is_none());
    }
}
