//! Constant blocks rewritten on their way to the GPU for the shader twins (formerly ao_centre_write and
//! expo_centre_write in src/Stereo/Grading.cpp; the upload hooks and the node tests stay there).
//!
//! THE CONE AO'S PROJECTION CENTRE. The cone AO (node DrawConeAO) rebuilds view positions with its block's symmetric
//! tangents, and nothing else it binds knows the asymmetric projection's NDC shift, so its twins
//! (tools/asym_tests/asym_ao_shaders.py) read this view's centre from the block itself: row 12.z = x | y << 16 in signed
//! 1/16384 units (the pixel's NDC, y from the top, so shift.x and -shift.y), 12.w the tag. 12.zw is padding the
//! engine's shaders never read. Under ENGINE STEREO the engine runs the AO over the side-by-side frame as one picture
//! with the centre's tangents; the block's two-halves mode (row 14.y) is turned on with each eye's centre (12.z the
//! left, 13.w the right), row 0 given the eyes' tangents (rows 12/13 .xy, mirrored windows: the same) and the
//! separation in 14.w, for the twins' stereo recipe.
//!
//! THE AUTO-EXPOSURE'S VIEW CENTRE (node HistogramUpdate): the luminance image's 24-byte block gets row 1.zw = the
//! centre and its tag, so the twins (asym_expo_shaders.py) meter around the view's centre in both eyes. The block ends
//! at row 1.y: on the constant uploader (256-byte slots) the upload grows to 32 bytes; on the buffer uploader it is left
//! alone.
//!
//! The switches and counters are the C++ half's exports, attached here.
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use cvr_engine::code;

use crate::{asym_projection, engine_stereo, menu_lobby};

pub const AO_CENTRE_ROW: usize = 12;
pub const AO_CENTRE_TAG: u32 = 0x4173_4331;
pub const AO_STEREO_TAG: u32 = 0x4173_5331;
pub const AO_BLOCK_MAX: u32 = 1024;
pub const EXPO_CENTRE_TAG: u32 = 0x4173_4531;
pub const EXPO_BLOCK_MAX: u32 = 256;

/// The exports.
#[repr(C)]
pub struct Runtime {
    /// CyberpunkVR_AsymAoBlockSize, CyberpunkVR_AsymAoPatch, CyberpunkVR_AsymAoPatchLoaded.
    pub ao_block_size: *const u32,
    pub ao_patch: *const AtomicU32,
    pub ao_patch_loaded: *const AtomicU32,
    /// CyberpunkVR_DebugAsymAoPatched[3]: MAIN, second view, neither.
    pub ao_patched: *mut u64,
    /// CyberpunkVR_EngineStereoAoHalves; CyberpunkVR_DebugEngineStereoAo[2]: turned to two halves, left alone.
    pub engine_stereo_ao_halves: *const AtomicU32,
    pub engine_stereo_ao: *mut u64,
    /// CyberpunkVR_ExpoCentreBlockSize, CyberpunkVR_ExposureCentre, CyberpunkVR_ExposureCentreLoaded.
    pub expo_block_size: *const u32,
    pub exposure_centre: *const AtomicU32,
    pub exposure_centre_loaded: *const AtomicU32,
    /// CyberpunkVR_DebugExpoCentred[3].
    pub expo_centred: *mut u64,
    /// CyberpunkVR_HudFlatInEye, CyberpunkVR_HudFlatInEyeBlocks, CyberpunkVR_HudShadowOffset, CyberpunkVR_HudEngineCurvature[2]
    /// (the engine's HUD bend, kept for the HUD panel's composite), CyberpunkVR_RuntimeDiagnostics.
    pub hud_flat_in_eye: *const i32,
    pub hud_flat_blocks: *const core::sync::atomic::AtomicU64,
    pub hud_shadow_offset: *const f32,
    pub hud_engine_curvature: *mut f32,
    pub runtime_diagnostics: *const i32,
    /// The environment meter (CyberpunkVR_ExposureMemory and its tuning) and its counters/parameters.
    pub exposure_memory: *const AtomicU32,
    pub exposure_memory_mode: *const u32,
    pub exposure_memory_write: *const f32,
    pub exposure_memory_age: *const f32,
    pub exposure_memory_distance: *const f32,
    pub exposure_memory_surprise: *const f32,
    pub exposure_level_time: *const f32,
    pub exposure_hemisphere: *const f32,
    pub exposure_gaze_gain: *const f32,
    pub exposure_gaze_limit: *const f32,
    pub debug_expo_env: *mut u64,
    pub debug_expo_env_params: *mut f32,
}
// SAFETY: exported variables of the C++ half, alive for the process.
unsafe impl Sync for Runtime {}
unsafe impl Send for Runtime {}

static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());

fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached once with a pointer the C++ half keeps for the process.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}

#[no_mangle]
pub extern "C" fn cvr_block_rewrites_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}

fn bump(counters: *mut u64, i: usize) {
    // SAFETY: an exported counter array of the C++ half.
    unsafe {
        let p = counters.add(i);
        core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
    }
}
fn flag(p: *const AtomicU32) -> bool {
    // SAFETY: an exported switch.
    unsafe { (*p).load(Ordering::Relaxed) != 0 }
}

/// A centre coordinate in the twins' signed 1/16384 units.
pub fn fixed(v: f32) -> u32 {
    let q = (v * 16384.0).round().clamp(-32768.0, 32767.0);
    (q as i16 as u16) as u32
}
/// x | y << 16.
pub fn packed(c: [f32; 2]) -> u32 {
    fixed(c[0]) | (fixed(c[1]) << 16)
}

fn put_u32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_ne_bytes());
}
fn get_f32(out: &[u8], at: usize) -> f32 {
    f32::from_ne_bytes(out[at..at + 4].try_into().unwrap_or([0; 4]))
}

/// The view's centre (shift x, -shift y), or the middle when the side has no window of its own.
fn centre(side: i32) -> [f32; 2] {
    match asym_projection::view_window_of(side) {
        Some(w) if w.shift_x.is_finite() && w.shift_y.is_finite() => [w.shift_x, -w.shift_y],
        _ => [0.0, 0.0],
    }
}

/// The engine-stereo part of the AO block: the two-halves mode with each eye's centre, when the eyes' tangents in rows
/// 12/13 are usable. `eyes` are the eye cameras' (shift x, shift y), or None for one shared window (both MAIN's).
pub fn ao_stereo_halves(
    out: &mut [u8],
    c: [f32; 2],
    eyes: Option<[[f32; 2]; 2]>,
    separation: f32,
) -> bool {
    let tl = [get_f32(out, 12 * 16), get_f32(out, 12 * 16 + 4)];
    let tr = [get_f32(out, 13 * 16), get_f32(out, 13 * 16 + 4)];
    let (mut cl, mut cr) = (c, c);
    if let Some(e) = eyes {
        cl = [e[0][0], -e[0][1]];
        cr = [e[1][0], -e[1][1]];
    }
    if (tl[0] + tl[1] + tr[0] + tr[1]).is_finite()
        && tl[0] > 0.0
        && tl[1] > 0.0
        && (tl[0] - tr[0]).abs() < 1e-3
        && (tl[1] - tr[1]).abs() < 1e-3
    {
        out[0..4].copy_from_slice(&tl[0].to_ne_bytes());
        out[4..8].copy_from_slice(&tl[1].to_ne_bytes());
        put_u32(out, 12 * 16 + 8, packed(cl));
        put_u32(out, 12 * 16 + 12, AO_STEREO_TAG);
        put_u32(out, 13 * 16 + 12, packed(cr));
        put_u32(out, 14 * 16 + 4, 1);
        out[14 * 16 + 12..14 * 16 + 16].copy_from_slice(&separation.to_ne_bytes());
        true
    } else {
        false
    }
}

/// ao_centre_write: true with the block to ship in `out`.
///
/// # Safety
/// `src` is the engine's block of `size` bytes; `out` has room for AO_BLOCK_MAX bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_ao_centre_write(
    size: u32,
    src: *const u8,
    out: *mut u8,
    side: i32,
) -> bool {
    let Some(rt) = runtime() else { return false };
    // SAFETY: the export.
    let want = unsafe { core::ptr::read_volatile(rt.ao_block_size) };
    if want == 0
        || size != want
        || size > AO_BLOCK_MAX
        || (size as usize) < (AO_CENTRE_ROW + 1) * 16
        || src.is_null()
        || out.is_null()
    {
        return false;
    }
    if !flag(rt.ao_patch) || !flag(rt.ao_patch_loaded) {
        return false;
    }
    let c = centre(side);
    // SAFETY: the caller's buffer.
    let out = unsafe { core::slice::from_raw_parts_mut(out, size as usize) };
    if !code::read_bytes(src as usize, out) {
        return false;
    }
    put_u32(out, AO_CENTRE_ROW * 16 + 8, packed(c));
    put_u32(out, AO_CENTRE_ROW * 16 + 12, AO_CENTRE_TAG);
    bump(
        rt.ao_patched,
        if side == 0 || side == 1 {
            side as usize
        } else {
            2
        },
    );
    if engine_stereo::enabled()
        && engine_stereo::rendering()
        && flag(rt.engine_stereo_ao_halves)
        && size >= 15 * 16
    {
        let eyes = match (engine_stereo::eye_window(0), engine_stereo::eye_window(1)) {
            (Some(a), Some(b)) if (a.shift_x + a.shift_y + b.shift_x + b.shift_y).is_finite() => {
                Some([[a.shift_x, a.shift_y], [b.shift_x, b.shift_y]])
            }
            _ => None,
        };
        let halves = ao_stereo_halves(out, c, eyes, engine_stereo::eye_separation());
        bump(rt.engine_stereo_ao, if halves { 0 } else { 1 });
    }
    true
}

/// expo_centre_write: true with the block to ship in `out` and its size in `ship`.
///
/// # Safety
/// `src` is the engine's block of `size` bytes; `out` has room for EXPO_BLOCK_MAX bytes; `ship` is a u32.
#[no_mangle]
pub unsafe extern "C" fn cvr_expo_centre_write(
    size: u32,
    src: *const u8,
    cb_path: bool,
    out: *mut u8,
    ship: *mut u32,
    side: i32,
) -> bool {
    let Some(rt) = runtime() else { return false };
    // SAFETY: the export.
    let want = unsafe { core::ptr::read_volatile(rt.expo_block_size) };
    if src.is_null()
        || out.is_null()
        || ship.is_null()
        || size != want
        || size < 24
        || size > EXPO_BLOCK_MAX
    {
        return false;
    }
    if size < 32 && !cb_path {
        return false;
    }
    if !flag(rt.exposure_centre) || !flag(rt.exposure_centre_loaded) {
        return false;
    }
    let c = centre(side);
    let shipped = size.max(32);
    // SAFETY: the caller's buffer and size slot.
    unsafe { *ship = shipped };
    let out = unsafe { core::slice::from_raw_parts_mut(out, shipped as usize) };
    out.fill(0);
    if !code::read_bytes(src as usize, &mut out[..size as usize]) {
        return false;
    }
    put_u32(out, 24, packed(c));
    put_u32(out, 28, EXPO_CENTRE_TAG);
    bump(
        rt.expo_centred,
        if side == 0 || side == 1 {
            side as usize
        } else {
            2
        },
    );
    true
}

// ---- the auto-exposure's environment meter

const EXPO_ENV_TAG: u32 = 0x4173_4532;
const EXPO_ACC_TAG: u32 = 0x4173_4533;
const EXPO_ADAPT_BYTES: u32 = 56;
const EXPO_ADAPT_ENV_BYTES: u32 = 112;
const EXPO_ENV_BYTES: u32 = 96;
/// Render/ViewCameraLayout.hpp: the scene camera descriptor.
const VIEW_CAMERA: usize = 0xE0;

/// The view's camera: position (1/131072 fixed point), and the camera's right, forward and up (its +X, +Y, +Z) from
/// the orientation quaternion at +16.
pub fn camera_basis(
    fixed: [i32; 3],
    q: [f32; 4],
) -> Option<([f32; 3], [f32; 3], [f32; 3], [f32; 3])> {
    let mut norm = 0.0f32;
    for v in q {
        if !v.is_finite() {
            return None;
        }
        norm += v * v;
    }
    if !(0.98..=1.02).contains(&norm) {
        return None;
    }
    let position = fixed.map(|f| (f as f64 / 131072.0) as f32);
    let [x, y, z, w] = q;
    let forward = [
        2.0 * (x * y - w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + w * x),
    ];
    let right = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + w * z),
        2.0 * (x * z - w * y),
    ];
    let up = [
        2.0 * (x * z + w * y),
        2.0 * (y * z - w * x),
        1.0 - 2.0 * (x * x + y * y),
    ];
    Some((position, forward, right, up))
}

fn view_camera(view: usize) -> Option<([f32; 3], [f32; 3], [f32; 3], [f32; 3])> {
    if view == 0 {
        return None;
    }
    let fixed = code::read::<[i32; 3]>(view + VIEW_CAMERA)?;
    let q = code::read::<[f32; 4]>(view + VIEW_CAMERA + 16)?;
    camera_basis(fixed, q)
}

unsafe fn bind_cells(debug: *mut u64, snapshot: bool) -> bool {
    // SAFETY: the debug counters of the caller.
    unsafe { crate::ssr_merge::cvr_ssr_merge_bind_exposure_cells(0, debug, snapshot) }
}

/// expo_acc_extend: the luminance image's block after the centre write (32 bytes) grown to 96 with its samples'
/// frame -- MAIN's lens window and camera basis and the accumulation tag on MAIN with the cells bound, zeros elsewhere.
///
/// # Safety
/// `out` has room for 96 bytes and holds `*ship` of them; `ship` is a u32.
#[no_mangle]
pub unsafe extern "C" fn cvr_expo_acc_extend(out: *mut u8, ship: *mut u32, side: i32, view: usize) {
    let Some(rt) = runtime() else { return };
    // SAFETY: the caller's buffer and size.
    let (out, shipped) = unsafe {
        (
            core::slice::from_raw_parts_mut(out, EXPO_ENV_BYTES as usize),
            &mut *ship,
        )
    };
    if *shipped > 32 {
        return;
    }
    out[*shipped as usize..].fill(0);
    *shipped = EXPO_ENV_BYTES;
    if side != 0 || !flag(rt.exposure_memory) {
        return;
    }
    let (Some(w), Some((_, forward, right, up))) =
        (asym_projection::view_window_of(0), view_camera(view))
    else {
        bump(rt.debug_expo_env, 5);
        return;
    };
    // SAFETY: the export counters.
    if !unsafe { bind_cells(rt.debug_expo_env, false) } {
        return;
    }
    // shift = -centre / half span, in tangents
    let (cx, cy) = (-w.shift_x * w.half_x, -w.shift_y * w.half_y);
    let window = [cx - w.half_x, cx + w.half_x, cy - w.half_y, cy + w.half_y];
    for (k, v) in window.iter().enumerate() {
        out[32 + 4 * k..36 + 4 * k].copy_from_slice(&v.to_ne_bytes());
    }
    for (row, axis) in [right, up, forward].iter().enumerate() {
        for (k, v) in axis.iter().enumerate() {
            let at = 48 + 16 * row + 4 * k;
            out[at..at + 4].copy_from_slice(&v.to_ne_bytes());
        }
    }
    put_u32(out, 60, EXPO_ACC_TAG);
    bump(rt.debug_expo_env, 7);
}

struct EnvClock {
    last: i64,
    position: [f32; 3],
    have: bool,
    /// The last block was metered for a vehicle's cabin.
    cabin: bool,
}
static ENV_CLOCK: std::sync::Mutex<EnvClock> = std::sync::Mutex::new(EnvClock {
    last: 0,
    position: [0.0; 3],
    have: false,
    cabin: false,
});

#[cfg(feature = "rust-build")]
extern "C" {
    /// cvr-rig wheel_grab: the mounted vehicle is a motorcycle (no cabin).
    fn cvr_anim_wheel_mounted_bike() -> bool;
}

/// CyberpunkVR_ExposureVehicleLowPercentile.
#[cfg(feature = "rust-build")]
fn cabin_low_percentile() -> f32 {
    // SAFETY: an export.
    unsafe { core::ptr::read_volatile(&raw const crate::used_moved_exports::CyberpunkVR_ExposureVehicleLowPercentile) }
}
/// The other builds (the unit tests) have no exports.
#[cfg(not(feature = "rust-build"))]
fn cabin_low_percentile() -> f32 {
    0.6
}
#[cfg(not(feature = "rust-build"))]
fn metering_cabin() -> bool {
    false
}
/// The view is metered for a vehicle's cabin (CyberpunkVR_ExposureVehicleMeter): mounted in anything but a motorcycle
/// at 1, in every vehicle at 2.
#[cfg(feature = "rust-build")]
fn metering_cabin() -> bool {
    // SAFETY: an export.
    let meter = unsafe { core::ptr::read_volatile(&raw const crate::used_moved_exports::CyberpunkVR_ExposureVehicleMeter) };
    if meter == 0 || !crate::vrcore_exports::g_isInVehicle.load(Ordering::Relaxed) {
        return false;
    }
    // SAFETY: a plain query of the wheel module's state.
    meter >= 2 || !unsafe { cvr_anim_wheel_mounted_bike() }
}

/// The adaptation block's histogram percentiles (row 0.w low, row 1.x high; measured 0.02 / 0.97): in a cabin the low
/// one raised to `want` (below the high one by 0.05), so only the brighter part of the view -- the street through the
/// glass -- sets the exposure, and the dash and a black roof filling a headset's tall view no longer push it up.
pub fn raise_low_percentile(block: &mut [u8], want: f32) {
    if block.len() < 20 {
        return;
    }
    let low = f32::from_ne_bytes([block[12], block[13], block[14], block[15]]);
    let high = f32::from_ne_bytes([block[16], block[17], block[18], block[19]]);
    if !(want.is_finite() && high.is_finite() && low.is_finite()) || want <= low {
        return;
    }
    let raised = want.min(high - 0.05);
    if raised > low {
        block[12..16].copy_from_slice(&raised.to_ne_bytes());
    }
}

fn qpc() -> (i64, i64) {
    let (mut now, mut frequency) = (0i64, 0i64);
    // SAFETY: out integers.
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut now);
        let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency);
    }
    (now, frequency)
}

/// expo_env_write: the adaptation's 56-byte block grown to 112 with the environment meter's rates (the cells' write
/// rate, the confidence fade with time and the player's movement, the surprise fade, the level offset's rate), the
/// view's forward, its own part (gain, limit) and the mode. A pause or a jump starts the cells afresh. True with the
/// block to ship in `out` and its size in `ship`.
///
/// # Safety
/// `src` is the engine's block; `out` has room for 112 bytes; `ship` is a u32.
#[no_mangle]
pub unsafe extern "C" fn cvr_expo_env_write(
    size: u32,
    src: *const u8,
    cb_path: bool,
    out: *mut u8,
    ship: *mut u32,
    side: i32,
    view: usize,
) -> bool {
    let Some(rt) = runtime() else { return false };
    if src.is_null() || size != EXPO_ADAPT_BYTES {
        return false;
    }
    if !flag(rt.exposure_centre) || !flag(rt.exposure_centre_loaded) {
        return false;
    }
    if !cb_path {
        bump(rt.debug_expo_env, 2);
        return false;
    }
    // SAFETY: the caller's buffer and size slot.
    let out = unsafe { core::slice::from_raw_parts_mut(out, EXPO_ADAPT_ENV_BYTES as usize) };
    out.fill(0);
    if !code::read_bytes(src as usize, &mut out[..size as usize]) {
        return false;
    }
    // SAFETY: as above.
    unsafe { *ship = EXPO_ADAPT_ENV_BYTES };
    let cabin = metering_cabin();
    if cabin {
        raise_low_percentile(out, cabin_low_percentile());
    }
    if side != 0 || !flag(rt.exposure_memory) {
        bump(rt.debug_expo_env, 1);
        return true;
    }
    let Some((position, forward, _, _)) = view_camera(view) else {
        bump(rt.debug_expo_env, 5);
        return true;
    };
    // SAFETY: the export counters.
    if !unsafe { bind_cells(rt.debug_expo_env, true) } {
        return true;
    }
    let (now, frequency) = qpc();
    let mut clock = ENV_CLOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dt = if clock.have {
        (now - clock.last) as f64 / frequency as f64
    } else {
        0.0
    };
    let mut moved = 0.0f32;
    for i in 0..3 {
        moved += (position[i] - clock.position[i]) * (position[i] - clock.position[i]);
    }
    moved = moved.sqrt();
    let left_cabin = clock.cabin && !cabin;
    clock.cabin = cabin;
    // A pause (loading, a menu), a jump (teleport, fast travel) or stepping out of a cabin starts the cells afresh.
    let reset = !clock.have || !(0.0..=2.0).contains(&dt) || !(moved <= 30.0) || left_cabin;
    clock.last = now;
    clock.position = position;
    clock.have = true;
    drop(clock);
    // SAFETY: the tuning exports.
    let tuning = |p: *const f32| unsafe { core::ptr::read_volatile(p) };
    let positive = |v: f32, fallback: f32| {
        if v.is_finite() && v > 0.0 {
            v
        } else {
            fallback
        }
    };
    let (mut rate, mut fade, mut surprise, mut level_rate) = (1.0f32, 0.0f32, 0.0f32, 1.0f32);
    if reset {
        moved = 0.0;
        bump(rt.debug_expo_env, 6);
    } else {
        rate = (1.0 - (-dt / positive(tuning(rt.exposure_memory_write), 0.25) as f64).exp()) as f32;
        fade = (-dt / positive(tuning(rt.exposure_memory_age), 600.0) as f64
            - (moved / positive(tuning(rt.exposure_memory_distance), 4.0)) as f64)
            .exp() as f32;
        surprise = (dt / positive(tuning(rt.exposure_memory_surprise), 0.5) as f64) as f32;
        level_rate =
            (1.0 - (-dt / positive(tuning(rt.exposure_level_time), 8.0) as f64).exp()) as f32;
    }
    let mut gain = tuning(rt.exposure_gaze_gain);
    let mut limit = tuning(rt.exposure_gaze_limit);
    if !gain.is_finite() || gain < 0.0 {
        gain = 0.0;
    }
    if !limit.is_finite() || limit < 0.0 {
        limit = 0.0;
    }
    let mut lean = tuning(rt.exposure_hemisphere);
    if !lean.is_finite() {
        lean = 0.0;
    }
    // SAFETY: an export.
    let memory_mode = unsafe { core::ptr::read_volatile(rt.exposure_memory_mode) };
    // in a cabin the environment gives way to the game's own meter (mode 1); a diagnostic mode stays
    let base_mode = if cabin && memory_mode & 255 == 0 { 1 } else { memory_mode & 255 };
    let mode = base_mode | (((lean.clamp(0.0, 1.0) * 255.0).round() as i32 as u32) << 8);
    put_u32(out, 56, EXPO_ENV_TAG);
    out[60..64].copy_from_slice(&surprise.to_ne_bytes());
    for (k, v) in [forward[0], forward[1], forward[2], rate]
        .iter()
        .enumerate()
    {
        out[64 + 4 * k..68 + 4 * k].copy_from_slice(&v.to_ne_bytes());
    }
    for (k, v) in [fade, gain, limit * 0.693_147_18].iter().enumerate() {
        out[80 + 4 * k..84 + 4 * k].copy_from_slice(&v.to_ne_bytes());
    }
    put_u32(out, 92, mode);
    out[96..100].copy_from_slice(&level_rate.to_ne_bytes());
    let debug = [
        forward[0], forward[1], forward[2], rate, fade, moved, dt as f32, level_rate,
    ];
    // SAFETY: the export, eight floats.
    unsafe { core::ptr::copy_nonoverlapping(debug.as_ptr(), rt.debug_expo_env_params, 8) };
    bump(rt.debug_expo_env, 0);
    true
}
// ---- the HUD composite

/// hud_cb_block_plausible: the HUD composite's constants (fxCompositionShaderParams) by fingerprint -- register 16.zw
/// exactly the HUD surface's size, and the rest in range for what it claims to be (curvature small, glow weights and
/// saturation in [0, 1], aberration tiny) and actually set (a halo, a glow and a curvature present; the curvature may
/// be the port's own zero). A block of zeros passes every range test, and the upload ring is full of them.
pub fn hud_block_plausible(r: &[f32; 68], w: f32, h: f32, engine_curvature_y: f32) -> bool {
    if r[66] != w || r[67] != h {
        return false;
    }
    let (cxk, cyk) = (r[12], r[13]);
    if !(cxk.abs() < 0.5 && cyk.abs() < 0.5) {
        return false;
    }
    let (w1, w2, w4) = (r[34], r[35], r[36]);
    if !((0.0..=1.0).contains(&w1) && (0.0..=1.0).contains(&w2) && (0.0..=1.0).contains(&w4)) {
        return false;
    }
    let (sat, ab) = (r[26], r[27]);
    if !((0.0..0.01).contains(&ab) && (0.0..=4.0).contains(&sat)) {
        return false;
    }
    let (bg, bl) = (r[20], r[21]);
    if !((0.0..=4.0).contains(&bg) && (0.0..=8.0).contains(&bl)) {
        return false;
    }
    bg > 1e-4
        && (w1 > 1e-4 || w2 > 1e-4 || w4 > 1e-4)
        && (cxk.abs() > 1e-6
            || cyk.abs() > 1e-6
            || (engine_curvature_y != 0.0 && cxk == 0.0 && cyk == 0.0))
}

/// The C++ name, for Hud.cpp's ring scan and Grading.cpp's flatten.
///
/// # Safety
/// `p` is any address; it is read guarded.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_block_plausible(p: *const u8, w: f32, h: f32) -> bool {
    let Some(r) = code::read::<[f32; 68]>(p as usize) else {
        return false;
    };
    let curvature_y = runtime().map_or(0.0, |rt| {
        // SAFETY: the export's second float.
        unsafe { core::ptr::read_volatile(rt.hud_engine_curvature.add(1)) }
    });
    hud_block_plausible(&r, w, h, curvature_y)
}

/// FlattenHudComposite: the engine's HUD warp off under the asymmetric projection, and under engine stereo the halo
/// taps and the shadow offset taken out of each eye's half (the composite runs each eye about its own centre); the main
/// menu's block, while the lobby shows its world, composed with the world instead. True with the block to ship in
/// `out` (the engine's curvature kept for the HUD panel).
///
/// # Safety
/// `src` is the engine's block of `size` bytes; `out` has room for 512 bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_flatten_hud_composite(
    size: u32,
    src: *const u8,
    out: *mut u8,
) -> bool {
    let Some(rt) = runtime() else { return false };
    let engine_stereo = size == 432 && engine_stereo::enabled();
    if src.is_null()
        || out.is_null()
        || !(engine_stereo || (size == 512 && asym_projection::active()))
    {
        return false;
    }
    let Some(head) = code::read::<[f32; 68]>(src as usize) else {
        return false;
    };
    let (w, h) = (head[66], head[67]);
    if !((64.0..=16384.0).contains(&w) && (64.0..=16384.0).contains(&h)) {
        return false;
    }
    // SAFETY: the caller's 512-byte buffer.
    let out_bytes = unsafe { core::slice::from_raw_parts_mut(out, size as usize) };
    // The main menu shown in its world. Its block has no glow, so the HUD's fingerprint below does not take it.
    if engine_stereo && menu_lobby::active() {
        if !code::read_bytes(src as usize, out_bytes) {
            return false;
        }
        // SAFETY: a 432-byte block in the caller's buffer.
        if unsafe { menu_lobby::compose_world(out as *mut f32) } {
            return true;
        }
    }
    // SAFETY: exports of the C++ half.
    unsafe {
        if core::ptr::read_volatile(rt.hud_flat_in_eye) == 0 {
            return false;
        }
        let curvature_y = core::ptr::read_volatile(rt.hud_engine_curvature.add(1));
        if !hud_block_plausible(&head, w, h, curvature_y) {
            return false;
        }
        if !code::read_bytes(src as usize, out_bytes) {
            return false;
        }
        let o = out as *mut f32;
        core::ptr::write_volatile(rt.hud_engine_curvature, *o.add(12));
        core::ptr::write_volatile(rt.hud_engine_curvature.add(1), *o.add(13));
        *o.add(12) = 0.0;
        *o.add(13) = 0.0;
        if engine_stereo {
            for i in 0..4 {
                *o.add(7 * 4 + i) = 0.0;
            }
            *o.add(8 * 4) = 0.0;
            *o.add(8 * 4 + 1) = 0.0;
            let shadow = core::ptr::read_volatile(rt.hud_shadow_offset);
            *o.add(14) *= shadow;
            *o.add(15) *= shadow;
        }
        if core::ptr::read_volatile(rt.runtime_diagnostics) != 0 {
            (*rt.hud_flat_blocks).fetch_add(1, Ordering::Relaxed);
        }
    }
    true
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cabin_meters_the_brighter_part_of_the_view() {
        let block = |low: f32, high: f32| {
            let mut b = [0u8; 56];
            b[12..16].copy_from_slice(&low.to_ne_bytes());
            b[16..20].copy_from_slice(&high.to_ne_bytes());
            b
        };
        let low_of = |b: &[u8]| f32::from_ne_bytes([b[12], b[13], b[14], b[15]]);
        let mut b = block(0.02, 0.97);
        raise_low_percentile(&mut b, 0.6);
        assert_eq!(low_of(&b), 0.6);
        // never up to the high percentile
        let mut b = block(0.02, 0.62);
        raise_low_percentile(&mut b, 0.6);
        assert!((low_of(&b) - 0.57).abs() < 1e-6);
        // a lower wish, or a broken block, leaves it
        let mut b = block(0.3, 0.97);
        raise_low_percentile(&mut b, 0.2);
        assert_eq!(low_of(&b), 0.3);
        let mut b = block(0.02, f32::NAN);
        raise_low_percentile(&mut b, 0.6);
        assert_eq!(low_of(&b), 0.02);
    }

    #[test]
    fn the_centre_is_in_signed_units_of_one_16384th() {
        assert_eq!(fixed(0.0), 0);
        assert_eq!(fixed(0.5), 8192);
        assert_eq!(fixed(-0.5), (-8192i16) as u16 as u32);
        assert_eq!(fixed(3.0), 32767, "clamped");
        assert_eq!(fixed(-3.0), 0x8000);
        assert_eq!(
            packed([0.25, -0.25]),
            4096 | ((-4096i16 as u16 as u32) << 16)
        );
        // Half away from zero, as roundf.
        assert_eq!(fixed(0.5 / 16384.0), 1);
    }

    #[test]
    fn the_two_halves_go_where_the_twins_read_them() {
        let mut block = vec![0u8; 15 * 16];
        let t = [1.2f32, 1.1f32];
        for row in [12usize, 13] {
            block[row * 16..row * 16 + 4].copy_from_slice(&t[0].to_ne_bytes());
            block[row * 16 + 4..row * 16 + 8].copy_from_slice(&t[1].to_ne_bytes());
        }
        let eyes = Some([[0.2f32, -0.1], [-0.2, -0.1]]);
        assert!(ao_stereo_halves(&mut block, [0.0, 0.0], eyes, 0.064));
        assert_eq!(get_f32(&block, 0), 1.2);
        assert_eq!(get_f32(&block, 4), 1.1);
        assert_eq!(
            u32::from_ne_bytes(block[12 * 16 + 8..12 * 16 + 12].try_into().unwrap()),
            packed([0.2, 0.1])
        );
        assert_eq!(
            u32::from_ne_bytes(block[12 * 16 + 12..12 * 16 + 16].try_into().unwrap()),
            AO_STEREO_TAG
        );
        assert_eq!(
            u32::from_ne_bytes(block[13 * 16 + 12..13 * 16 + 16].try_into().unwrap()),
            packed([-0.2, 0.1])
        );
        assert_eq!(
            u32::from_ne_bytes(block[14 * 16 + 4..14 * 16 + 8].try_into().unwrap()),
            1
        );
        assert_eq!(get_f32(&block, 14 * 16 + 12), 0.064);
        // Tangents that disagree between the eyes: left alone.
        let mut odd = vec![0u8; 15 * 16];
        odd[12 * 16..12 * 16 + 4].copy_from_slice(&1.0f32.to_ne_bytes());
        odd[12 * 16 + 4..12 * 16 + 8].copy_from_slice(&1.0f32.to_ne_bytes());
        odd[13 * 16..13 * 16 + 4].copy_from_slice(&2.0f32.to_ne_bytes());
        odd[13 * 16 + 4..13 * 16 + 8].copy_from_slice(&1.0f32.to_ne_bytes());
        let before = odd.clone();
        assert!(!ao_stereo_halves(&mut odd, [0.0, 0.0], None, 0.064));
        assert_eq!(odd, before);
    }

    fn hud_block() -> [f32; 68] {
        let mut r = [0.0f32; 68];
        r[66] = 4128.0;
        r[67] = 2232.0;
        r[12] = 0.05; // the engine's bend
        r[13] = 0.02;
        r[34] = 0.5; // a glow
        r[26] = 1.0; // saturation
        r[27] = 0.001; // aberration
        r[20] = 1.0; // a halo
        r
    }

    #[test]
    fn the_hud_composite_by_fingerprint() {
        let r = hud_block();
        assert!(hud_block_plausible(&r, 4128.0, 2232.0, 0.0));
        assert!(
            !hud_block_plausible(&r, 4128.0, 2160.0, 0.0),
            "another surface"
        );
        let zeros = {
            let mut z = [0.0f32; 68];
            z[66] = 4128.0;
            z[67] = 2232.0;
            z
        };
        assert!(
            !hud_block_plausible(&zeros, 4128.0, 2232.0, 0.0),
            "a block of zeros is not set"
        );
        // The port's own zero curvature counts once the engine's was kept.
        let mut flat = r;
        flat[12] = 0.0;
        flat[13] = 0.0;
        assert!(!hud_block_plausible(&flat, 4128.0, 2232.0, 0.0));
        assert!(hud_block_plausible(&flat, 4128.0, 2232.0, 0.02));
        let mut nan = r;
        nan[13] = f32::NAN;
        assert!(!hud_block_plausible(&nan, 4128.0, 2232.0, 0.0));
    }

    #[test]
    fn the_camera_basis_is_the_quaternion_s_axes() {
        let (p, f, r, u) = camera_basis([131072, -262144, 0], [0.0, 0.0, 0.0, 1.0]).unwrap();
        assert_eq!(p, [1.0, -2.0, 0.0]);
        assert_eq!(
            (f, r, u),
            ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
        );
        // A quarter turn about +Z: forward becomes -X.
        let h = core::f32::consts::FRAC_1_SQRT_2;
        let (_, f, _, _) = camera_basis([0; 3], [0.0, 0.0, h, h]).unwrap();
        assert!((f[0] + 1.0).abs() < 1e-6 && f[1].abs() < 1e-6);
        // Not a rotation: refused.
        assert!(camera_basis([0; 3], [0.0, 0.0, 0.0, 0.5]).is_none());
        assert!(camera_basis([0; 3], [f32::NAN, 0.0, 0.0, 1.0]).is_none());
    }
}
