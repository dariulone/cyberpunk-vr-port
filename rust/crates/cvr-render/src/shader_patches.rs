//! Shader twins and the shader census (formerly src/Render/HiddenAreaShaders.cpp; docs/hidden-area-mask-20261009.md).
//!
//! Patched shaders live beside the plugin, named by the FNV-1a of the original: `hidden_area\` holds signed containers
//! followed by 'HAM1', a count and the bit offsets of four placeholder constants (the visible ellipse), and every other
//! group's folder (`asym\`, `view_dir\`, `stereo_merge\`, ...) holds a signed container used as it is. A pipeline the
//! engine creates with such a shader is registered with a creator that rebuilds its description around other bytecode;
//! its twins hang off the pipeline itself (private data), so a reused address never inherits another pipeline's twins,
//! and Bind hands out the twin while its group (or the mask) is on.
//!
//! The census: with `cvr_shader_census.txt` in the log folder at startup, every shader a pipeline is created with is
//! kept, so the dispatches and draws of each render-graph node can be logged (CyberpunkVR_ShaderCensusLog) and their
//! shaders dumped once.
//!
//! The counters stay exported by the C++ half (the DLL's export table); it hands this module their addresses.
use cvr_host::fast::{FastMap, FastSet};
use core::ffi::c_void;

use std::path::Path;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use cvr_host::paths::{self, Folder};
use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D12::ID3D12PipelineState;

use crate::dxil_patch;

// ---- the exported counters (Render/HiddenAreaShaders.cpp)

/// CensusRecord: one row of CyberpunkVR_ShaderCensusLog.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CensusRecord {
    node: u32,
    side: i32,
    kind: u32,
    x: u32,
    y: u32,
    z: u32,
    hash: u64,
    count: u32,
    bytes: u32,
    pso: u64,
}
const _: () = assert!(core::mem::size_of::<CensusRecord>() == 48);
impl CensusRecord {
    pub const ZERO: CensusRecord = CensusRecord { node: 0, side: 0, kind: 0, x: 0, y: 0, z: 0, hash: 0, count: 0, bytes: 0, pso: 0 };
}

/// One fixed group's switch and counters.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GroupCounters {
    enabled: *const AtomicU32,
    loaded: *const AtomicU32,
    built: *const AtomicU32,
    failed: *const AtomicU32,
    binds: *const AtomicU64,
}
/// CvrShaderPatchCounters: the addresses of every exported switch and counter.
#[repr(C)]
pub struct Counters {
    hidden_area_enabled: *const AtomicU32,
    census: *const AtomicU32,
    census_recording: *const AtomicU32,
    census_pipelines: *const AtomicU64,
    census_log: *mut CensusRecord,
    census_log_len: u32,
    ham_patch: *const AtomicU32,
    ham_loaded: *const AtomicU32,
    ham_registered: *const AtomicU32,
    ham_built: *const AtomicU32,
    ham_failed: *const AtomicU32,
    ham_binds: *const AtomicU64,
    groups: [GroupCounters; GROUPS],
}

// In the plugin the table is PLUGIN (the Rust exports) and the two queries are called directly; a program without the
// plugin's exports (the unit tests) reads every switch off and its counters go nowhere, with no link-time reference to
// C++. A table attached through cvr_shader_patches_attach takes precedence.
#[cfg(not(feature = "rust-build"))]
static SWITCH_OFF: AtomicU32 = AtomicU32::new(0);
#[cfg(not(feature = "rust-build"))]
static SINK32: AtomicU32 = AtomicU32::new(0);
#[cfg(not(feature = "rust-build"))]
static SINK64: AtomicU64 = AtomicU64::new(0);
struct Detached(Counters);
// SAFETY: the pointers name the statics above.
unsafe impl Sync for Detached {}
#[cfg(not(feature = "rust-build"))]
static DETACHED: Detached = Detached(Counters {
    hidden_area_enabled: &SWITCH_OFF,
    census: &SWITCH_OFF,
    census_recording: &SINK32,
    census_pipelines: &SINK64,
    census_log: core::ptr::null_mut(),
    census_log_len: 0,
    ham_patch: &SWITCH_OFF,
    ham_loaded: &SINK32,
    ham_registered: &SINK32,
    ham_built: &SINK32,
    ham_failed: &SINK32,
    ham_binds: &SINK64,
    groups: [GroupCounters {
        enabled: &SWITCH_OFF,
        loaded: &SINK32,
        built: &SINK32,
        failed: &SINK32,
        binds: &SINK64,
    }; GROUPS],
});
static COUNTERS: AtomicPtr<Counters> = AtomicPtr::new(core::ptr::null_mut());
static ENGINE_STEREO: AtomicUsize = AtomicUsize::new(0);
static MENU_MODE: AtomicUsize = AtomicUsize::new(0);

/// The plugin's table: the exported switches and counters shader_patch_exports.rs defines (the census log with them).
#[cfg(feature = "rust-build")]
static PLUGIN: Detached = {
    use crate::shader_patch_exports as e;
    macro_rules! group {
        ($on:ident, $loaded:ident, $built:ident, $failed:ident, $binds:ident) => {
            GroupCounters { enabled: &e::$on, loaded: &e::$loaded, built: &e::$built, failed: &e::$failed, binds: &e::$binds }
        };
    }
    Detached(Counters {
        hidden_area_enabled: &crate::used_moved_exports_rest::CyberpunkVR_HiddenAreaEnabled,
        census: &e::CyberpunkVR_ShaderCensus,
        census_recording: &e::CyberpunkVR_ShaderCensusRecording,
        census_pipelines: &e::CyberpunkVR_ShaderCensusPipelines,
        census_log: e::CyberpunkVR_ShaderCensusLog.0.get().cast(),
        census_log_len: e::CENSUS_LOG_LENGTH as u32,
        ham_patch: &e::CyberpunkVR_HiddenAreaPatch,
        ham_loaded: &e::CyberpunkVR_HiddenAreaPatchLoaded,
        ham_registered: &e::CyberpunkVR_HiddenAreaPatchRegistered,
        ham_built: &e::CyberpunkVR_HiddenAreaPatchBuilt,
        ham_failed: &e::CyberpunkVR_HiddenAreaPatchFailed,
        ham_binds: &e::CyberpunkVR_HiddenAreaPatchBinds,
        groups: [
            group!(CyberpunkVR_AsymFogPatch, CyberpunkVR_AsymFogPatchLoaded, CyberpunkVR_AsymFogPatchBuilt, CyberpunkVR_AsymFogPatchFailed, CyberpunkVR_AsymFogPatchBinds),
            group!(CyberpunkVR_AsymSsrPatch, CyberpunkVR_AsymSsrPatchLoaded, CyberpunkVR_AsymSsrPatchBuilt, CyberpunkVR_AsymSsrPatchFailed, CyberpunkVR_AsymSsrPatchBinds),
            group!(CyberpunkVR_AsymAoPatch, CyberpunkVR_AsymAoPatchLoaded, CyberpunkVR_AsymAoPatchBuilt, CyberpunkVR_AsymAoPatchFailed, CyberpunkVR_AsymAoPatchBinds),
            group!(CyberpunkVR_ParticleFacing, CyberpunkVR_ParticleFacingLoaded, CyberpunkVR_ParticleFacingBuilt, CyberpunkVR_ParticleFacingFailed, CyberpunkVR_ParticleFacingBinds),
            group!(CyberpunkVR_ExposureCentre, CyberpunkVR_ExposureCentreLoaded, CyberpunkVR_ExposureCentreBuilt, CyberpunkVR_ExposureCentreFailed, CyberpunkVR_ExposureCentreBinds),
            group!(CyberpunkVR_AsymShadowPatch, CyberpunkVR_AsymShadowPatchLoaded, CyberpunkVR_AsymShadowPatchBuilt, CyberpunkVR_AsymShadowPatchFailed, CyberpunkVR_AsymShadowPatchBinds),
            group!(CyberpunkVR_ViewDirPatch, CyberpunkVR_ViewDirPatchLoaded, CyberpunkVR_ViewDirPatchBuilt, CyberpunkVR_ViewDirPatchFailed, CyberpunkVR_ViewDirPatchBinds),
            group!(CyberpunkVR_ScreenFxPatch, CyberpunkVR_ScreenFxPatchLoaded, CyberpunkVR_ScreenFxPatchBuilt, CyberpunkVR_ScreenFxPatchFailed, CyberpunkVR_ScreenFxPatchBinds),
            group!(CyberpunkVR_StereoMergePatch, CyberpunkVR_StereoMergePatchLoaded, CyberpunkVR_StereoMergePatchBuilt, CyberpunkVR_StereoMergePatchFailed, CyberpunkVR_StereoMergePatchBinds),
            group!(CyberpunkVR_StereoEyePatch, CyberpunkVR_StereoEyePatchLoaded, CyberpunkVR_StereoEyePatchBuilt, CyberpunkVR_StereoEyePatchFailed, CyberpunkVR_StereoEyePatchBinds),
            group!(CyberpunkVR_StereoBloomPatch, CyberpunkVR_StereoBloomPatchLoaded, CyberpunkVR_StereoBloomPatchBuilt, CyberpunkVR_StereoBloomPatchFailed, CyberpunkVR_StereoBloomPatchBinds),
            group!(CyberpunkVR_StereoParticleLightingPatch, CyberpunkVR_StereoParticleLightingPatchLoaded, CyberpunkVR_StereoParticleLightingPatchBuilt, CyberpunkVR_StereoParticleLightingPatchFailed, CyberpunkVR_StereoParticleLightingPatchBinds),
            group!(CyberpunkVR_StereoLightCullPatch, CyberpunkVR_StereoLightCullPatchLoaded, CyberpunkVR_StereoLightCullPatchBuilt, CyberpunkVR_StereoLightCullPatchFailed, CyberpunkVR_StereoLightCullPatchBinds),
            group!(CyberpunkVR_StereoDistantLightPatch, CyberpunkVR_StereoDistantLightPatchLoaded, CyberpunkVR_StereoDistantLightPatchBuilt, CyberpunkVR_StereoDistantLightPatchFailed, CyberpunkVR_StereoDistantLightPatchBinds),
            group!(CyberpunkVR_StereoProbeCullPatch, CyberpunkVR_StereoProbeCullPatchLoaded, CyberpunkVR_StereoProbeCullPatchBuilt, CyberpunkVR_StereoProbeCullPatchFailed, CyberpunkVR_StereoProbeCullPatchBinds),
            group!(CyberpunkVR_StereoRtPatch, CyberpunkVR_StereoRtPatchLoaded, CyberpunkVR_StereoRtPatchBuilt, CyberpunkVR_StereoRtPatchFailed, CyberpunkVR_StereoRtPatchBinds),
            group!(CyberpunkVR_StereoLightMergePatch, CyberpunkVR_StereoLightMergePatchLoaded, CyberpunkVR_StereoLightMergePatchBuilt, CyberpunkVR_StereoLightMergePatchFailed, CyberpunkVR_StereoLightMergePatchBinds),
        ],
    })
};
#[cfg(feature = "rust-build")]
extern "C" {
    fn GetMenuMode() -> i32;
}

fn counters() -> &'static Counters {
    let p = COUNTERS.load(Ordering::Acquire);
    if p.is_null() {
        #[cfg(feature = "rust-build")]
        return &PLUGIN.0;
        #[cfg(not(feature = "rust-build"))]
        &DETACHED.0
    } else {
        // SAFETY: the C++ half's table of its exports, alive for the process.
        unsafe { &*p }
    }
}
fn a32(p: *const AtomicU32) -> &'static AtomicU32 {
    // SAFETY: an exported std::atomic<uint32_t> (same layout) or a static here, alive for the process.
    unsafe { &*p }
}
fn a64(p: *const AtomicU64) -> &'static AtomicU64 {
    // SAFETY: an exported std::atomic<uint64_t> or a static here, alive for the process.
    unsafe { &*p }
}
/// cvr::engine_stereo::Enabled.
fn engine_stereo() -> bool {
    match ENGINE_STEREO.load(Ordering::Acquire) {
        #[cfg(feature = "rust-build")]
        0 => crate::engine_stereo::cvr_engine_stereo_enabled(),
        #[cfg(not(feature = "rust-build"))]
        0 => false,
        // SAFETY: the function the C++ half attached.
        f => unsafe { core::mem::transmute::<usize, extern "C" fn() -> bool>(f)() },
    }
}
/// GetMenuMode.
fn menu_mode() -> i32 {
    match MENU_MODE.load(Ordering::Acquire) {
        // SAFETY: the plugin's VrCore.cpp export.
        #[cfg(feature = "rust-build")]
        0 => unsafe { GetMenuMode() },
        #[cfg(not(feature = "rust-build"))]
        0 => 0,
        // SAFETY: the function the C++ half attached.
        f => unsafe { core::mem::transmute::<usize, extern "C" fn() -> i32>(f)() },
    }
}

/// The C++ half's counters and queries (Render/HiddenAreaShaders.cpp, at its static initialisation).
///
/// # Safety
/// `counters` is a CvrShaderPatchCounters table alive for the process, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_shader_patches_attach(
    counters: *const Counters,
    engine_stereo: Option<extern "C" fn() -> bool>,
    menu_mode: Option<extern "C" fn() -> i32>,
) {
    ENGINE_STEREO.store(engine_stereo.map_or(0, |f| f as usize), Ordering::Release);
    MENU_MODE.store(menu_mode.map_or(0, |f| f as usize), Ordering::Release);
    COUNTERS.store(counters as *mut Counters, Ordering::Release);
}

// ---- the groups

const GROUPS: usize = 17;
const STEREO_RT: usize = 15;
const LIGHT_MERGE: usize = 16;
/// Each fixed group's folder, and whether its twin is created with the original pipeline (eager) instead of at its
/// first bind. 16 is an alternate: a second twin of a shader another group already twins.
const FOLDERS: [(&str, bool); GROUPS] = [
    ("asym", false),
    ("asym_ssr", false),
    ("asym_ao", false),
    ("asym_particles", true),
    ("asym_expo", false),
    ("asym_shadow", false),
    ("view_dir", true),
    ("screen_fx", false),
    ("stereo_merge", false),
    ("stereo_eye", false),
    ("stereo_bloom", false),
    ("stereo_particle_lighting", false),
    ("stereo_light_cull", false),
    ("stereo_distant_light", false),
    ("stereo_probe_cull", false),
    ("stereo_rt", false),
    ("stereo_light_merge", false),
];
fn group_enabled(group: usize) -> bool {
    a32(counters().groups[group].enabled).load(Ordering::Relaxed) != 0
}
/// Groups 10..=14 only make sense under engine stereo.
fn stereo_only(group: usize) -> bool {
    (10..=14).contains(&group)
}

// ---- the library

struct Patch {
    container: Vec<u8>,
    offsets: [u32; 4],
    fixed: bool,
    group: usize,
    fallback: Vec<u8>,
    fallback_group: Option<usize>,
    alternate: Vec<u8>,
    alternate_group: Option<usize>,
}
impl Patch {
    fn new(container: Vec<u8>) -> Self {
        Patch {
            container,
            offsets: [0; 4],
            fixed: false,
            group: 0,
            fallback: Vec::new(),
            fallback_group: None,
            alternate: Vec::new(),
            alternate_group: None,
        }
    }
}

/// FNV-1a, 64 bit.
fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(1469598103934665603u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211))
}
fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
/// wcstoull(name, nullptr, 16): leading blanks, a sign, an optional 0x, then hex digits up to the first other
/// character; 0 without digits, the maximum on overflow, a minus negating.
fn hex_key(name: &str) -> u64 {
    let s = name.trim_start();
    let (negative, s) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let b = s.as_bytes();
    let s = if b.len() > 2 && b[0] == b'0' && (b[1] | 0x20) == b'x' && b[2].is_ascii_hexdigit() {
        &s[2..]
    } else {
        s
    };
    let (mut value, mut overflow, mut any) = (0u64, false, false);
    for d in s.chars().map_while(|c| c.to_digit(16)) {
        any = true;
        match value.checked_mul(16).and_then(|v| v.checked_add(d as u64)) {
            Some(v) => value = v,
            None => overflow = true,
        }
    }
    match (any, overflow) {
        (false, _) => 0,
        (_, true) => u64::MAX,
        _ if negative => value.wrapping_neg(),
        _ => value,
    }
}
/// The `*.dxil` files of a folder, in the order the file system lists them, with their bytes.
fn dxil_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("dxil")))
        .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).ok()?)))
        .collect()
}
/// A signed container whose hash matches.
fn signed(container: &[u8]) -> bool {
    dxil_patch::container_hash(container)[..] == container[4..20]
}

fn library() -> &'static FastMap<u64, Patch> {
    static LIBRARY: OnceLock<FastMap<u64, Patch>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let c = counters();
        let root = paths::dir(Folder::Shaders);
        let mut l: FastMap<u64, Patch> = cvr_host::fast::fast_map();
        for (name, mut bytes) in dxil_files(&root.join("hidden_area")) {
            if bytes.len() < 32 || &bytes[..4] != b"DXBC" {
                continue;
            }
            let size = word(&bytes, 24) as usize;
            if size + 8 + 16 != bytes.len() || &bytes[size..size + 4] != b"HAM1" || word(&bytes, size + 4) != 4 {
                continue;
            }
            let offsets = core::array::from_fn(|i| word(&bytes, size + 8 + i * 4));
            bytes.truncate(size);
            if !signed(&bytes) {
                continue;
            }
            l.entry(hex_key(&name)).or_insert(Patch { offsets, ..Patch::new(bytes) });
        }
        a32(c.ham_loaded).store(l.len() as u32, Ordering::Relaxed);
        // Fixed patches: a signed container, nothing appended. The alternate group goes last: it attaches to the fixed
        // twin already loaded for its shader.
        for (group, (folder, _)) in FOLDERS.iter().enumerate() {
            let alternate = group == LIGHT_MERGE;
            let mut count = 0u32;
            for (name, bytes) in dxil_files(&root.join(folder)) {
                if bytes.len() < 32 || &bytes[..4] != b"DXBC" || word(&bytes, 24) as usize != bytes.len() || !signed(&bytes) {
                    continue;
                }
                let key = hex_key(&name);
                if let Some(p) = l.get_mut(&key) {
                    if alternate && p.fixed && p.alternate.is_empty() {
                        p.alternate = bytes;
                        p.alternate_group = Some(group);
                        count += 1;
                    } else if !alternate && !p.fixed && p.fallback.is_empty() {
                        p.fallback = bytes;
                        p.fallback_group = Some(group);
                        count += 1;
                    }
                    continue;
                }
                l.insert(
                    key,
                    Patch {
                        fixed: true,
                        group,
                        ..Patch::new(bytes)
                    },
                );
                count += 1;
            }
            a32(c.groups[group].loaded).store(count, Ordering::Relaxed);
        }
        l
    })
}
fn patch_for(code: &[u8]) -> Option<&'static Patch> {
    if code.is_empty() {
        return None;
    }
    let library = library();
    if library.is_empty() {
        return None;
    }
    library.get(&fnv(code))
}

// ---- the ellipse each view publishes

/// Placeholder values for one view: 1 + scale/8, so every value stays in [1, 2). Compared bit for bit.
type Params = [u32; 4];
/// A view's ellipse is published only after it held for this many frames, so dragging the slider does not build a
/// twin per intermediate value.
const STABLE_FRAMES: u32 = 30;
struct PublishState {
    pending: [Params; 2],
    published: [Params; 2],
    stable: [u32; 2],
}
static PUBLISH: Mutex<PublishState> = Mutex::new(PublishState {
    pending: [[0; 4]; 2],
    published: [[0; 4]; 2],
    stable: [0; 2],
});
static PUBLISHED_GENERATION: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static GENERATIONS: AtomicU64 = AtomicU64::new(0);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Publish: view `side`'s visible ellipse (a*x*x + b*y*y <= 1 in NDC, y down; x<0, x>=0, y<0, y>=0).
pub fn publish(side: i32, scales: &[f32; 4]) {
    if !(0..=1).contains(&side) {
        return;
    }
    let side = side as usize;
    let params: Params = core::array::from_fn(|i| {
        let s = if scales[i] >= 0.0 && scales[i] < 7.99 { scales[i] } else { 7.99 }; // a larger ellipse skips less
        (1.0f32 + s / 8.0).to_bits()
    });
    let mut p = lock(&PUBLISH);
    if params != p.pending[side] {
        p.pending[side] = params;
        p.stable[side] = 0;
        return;
    }
    p.stable[side] = p.stable[side].wrapping_add(1);
    if p.stable[side] < STABLE_FRAMES || (PUBLISHED_GENERATION[side].load(Ordering::Relaxed) != 0 && p.published[side] == params) {
        return;
    }
    p.published[side] = params;
    PUBLISHED_GENERATION[side].store(GENERATIONS.fetch_add(1, Ordering::Relaxed) + 1, Ordering::Release);
}

// ---- the creator a registration carries

/// The engine's pipeline description rebuilt around other bytecode (C++: a std::function the header boxes); the
/// answer is a new pipeline (one reference) or null.
pub struct Creator {
    ctx: *mut c_void,
    call: unsafe extern "C" fn(*mut c_void, *const u8, usize) -> *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
}
// SAFETY: the C++ callable is used under the entry's mutex only, and freed once.
unsafe impl Send for Creator {}
impl Creator {
    fn create(&self, code: &[u8]) -> *mut c_void {
        // SAFETY: the C++ callable with bytes that outlive the call.
        unsafe { (self.call)(self.ctx, code.as_ptr(), code.len()) }
    }
}
impl Drop for Creator {
    fn drop(&mut self) {
        // SAFETY: the callable is freed once, here.
        unsafe { (self.free)(self.ctx) }
    }
}

// ---- a pipeline's twins: a COM object owned by the pipeline (private data)

const MAX_TWINS_PER_PIPELINE: usize = 16;
const ENTRY_KEY: GUID = GUID::from_values(0x9b9c51a9, 0x78bd, 0x4dc7, [0x9f, 0xf5, 0x58, 0x54, 0x62, 0x32, 0x31, 0x7c]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

struct Inner {
    /// Twins built per ellipse (null: creation failed).
    built: Vec<(Params, *mut c_void)>,
    create: Option<Creator>,
}
#[repr(C)]
struct Entry {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    patch: &'static Patch,
    inner: Mutex<Inner>,
    generation: [AtomicU64; 2],
    twin: [AtomicPtr<c_void>; 2],
    fixed_done: AtomicU32,
    fixed_twin: AtomicPtr<c_void>,
    alternate_done: AtomicU32,
    alternate_twin: AtomicPtr<c_void>,
}
// SAFETY: the raw twins are owned references to free-threaded pipelines; the rest is atomics and a mutex.
unsafe impl Sync for Entry {}
unsafe impl Send for Entry {}

/// Releases one owned pipeline reference.
fn release(pipeline: *mut c_void) {
    if !pipeline.is_null() {
        // SAFETY: a reference this entry owns, released once.
        drop(unsafe { ID3D12PipelineState::from_raw(pipeline) });
    }
}
impl Drop for Entry {
    fn drop(&mut self) {
        let inner = self.inner.get_mut().unwrap_or_else(|p| p.into_inner());
        for &(_, twin) in &inner.built {
            release(twin);
        }
        release(*self.fixed_twin.get_mut());
        release(*self.alternate_twin.get_mut());
    }
}
unsafe extern "system" fn entry_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return windows::Win32::Foundation::E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Entry.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return windows::Win32::Foundation::E_NOINTERFACE;
        }
        *out = this;
        entry_add_ref(this);
    }
    HRESULT(0)
}
unsafe extern "system" fn entry_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live Entry.
    unsafe { (*(this as *const Entry)).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}
unsafe extern "system" fn entry_release(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live Entry; the last release frees it (it came from Box::into_raw).
    unsafe {
        let n = (*(this as *const Entry)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Entry));
        }
        n
    }
}
static ENTRY_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: entry_query,
    AddRef: entry_add_ref,
    Release: entry_release,
};

/// One counted reference to an Entry.
struct EntryRef(*mut Entry);
impl EntryRef {
    fn new(patch: &'static Patch, create: Creator) -> Self {
        EntryRef(Box::into_raw(Box::new(Entry {
            vtbl: &ENTRY_VTBL,
            refs: AtomicU32::new(1),
            patch,
            inner: Mutex::new(Inner {
                built: Vec::new(),
                create: Some(create),
            }),
            generation: [AtomicU64::new(0), AtomicU64::new(0)],
            twin: [AtomicPtr::new(core::ptr::null_mut()), AtomicPtr::new(core::ptr::null_mut())],
            fixed_done: AtomicU32::new(0),
            fixed_twin: AtomicPtr::new(core::ptr::null_mut()),
            alternate_done: AtomicU32::new(0),
            alternate_twin: AtomicPtr::new(core::ptr::null_mut()),
        })))
    }
}
impl core::ops::Deref for EntryRef {
    type Target = Entry;
    fn deref(&self) -> &Entry {
        // SAFETY: a counted reference keeps the entry alive.
        unsafe { &*self.0 }
    }
}
impl Drop for EntryRef {
    fn drop(&mut self) {
        // SAFETY: the reference this wrapper counts.
        unsafe { entry_release(self.0 as *mut c_void) };
    }
}

/// The entry a pipeline owns, if any. Only private data identifies the object's lifetime: an address can be reused by
/// an unrelated pipeline, including one created outside the device hooks. GetPrivateData adds a reference.
fn find(pso: &ID3D12PipelineState) -> Option<EntryRef> {
    let mut size = core::mem::size_of::<*mut Entry>() as u32;
    let mut entry: *mut Entry = core::ptr::null_mut();
    // SAFETY: the live pipeline and an out-pointer of the announced size.
    let ok = unsafe { pso.GetPrivateData(&ENTRY_KEY, &mut size, Some(&mut entry as *mut *mut Entry as *mut c_void)) }.is_ok();
    (ok && size as usize == core::mem::size_of::<*mut Entry>() && !entry.is_null()).then(|| EntryRef(entry))
}

static REGISTRATION: Mutex<()> = Mutex::new(());
static REGISTERED: AtomicU32 = AtomicU32::new(0);

/// A twin of the hidden-area patch for one ellipse, built once per ellipse (at most 16 per pipeline).
fn build(e: &Entry, inner: &mut Inner, params: Params) -> *mut c_void {
    if let Some(&(_, twin)) = inner.built.iter().find(|(p, _)| *p == params) {
        return twin;
    }
    if inner.built.len() >= MAX_TWINS_PER_PIPELINE {
        return core::ptr::null_mut();
    }
    let mut bytes = e.patch.container.clone();
    let ok = (0..4).all(|i| dxil_patch::write_float(&mut bytes, e.patch.offsets[i] as usize, f32::from_bits(params[i])));
    let twin = match (&inner.create, ok && dxil_patch::sign(&mut bytes)) {
        (Some(create), true) => create.create(&bytes),
        _ => core::ptr::null_mut(),
    };
    inner.built.push((params, twin));
    let c = counters();
    a32(if twin.is_null() { c.ham_failed } else { c.ham_built }).fetch_add(1, Ordering::Relaxed);
    twin
}
/// The one twin of a fixed patch (or a hidden-area patch's fallback), created once.
fn fixed_twin(e: &Entry, container: &[u8], group: usize) -> *mut c_void {
    let twin = e.fixed_twin.load(Ordering::Acquire);
    if !twin.is_null() || e.fixed_done.load(Ordering::Acquire) != 0 {
        return twin;
    }
    let inner = lock(&e.inner);
    if e.fixed_done.load(Ordering::Relaxed) != 0 {
        return e.fixed_twin.load(Ordering::Acquire);
    }
    let twin = inner.create.as_ref().map_or(core::ptr::null_mut(), |c| c.create(container));
    let g = &counters().groups[group];
    a32(if twin.is_null() { g.failed } else { g.built }).fetch_add(1, Ordering::Relaxed);
    e.fixed_twin.store(twin, Ordering::Release);
    e.fixed_done.store(1, Ordering::Release);
    twin
}
/// The alternate twin (group 16), created once: with the original pipeline while its switch is on (register), else at
/// its first bind (creating the 78 light-pass twins at their first binds made one 50 ms frame).
fn create_alternate(e: &Entry) -> *mut c_void {
    let twin = e.alternate_twin.load(Ordering::Acquire);
    if !twin.is_null() || e.alternate_done.load(Ordering::Acquire) != 0 {
        return twin;
    }
    let Some(group) = e.patch.alternate_group else { return core::ptr::null_mut() };
    let inner = lock(&e.inner);
    if e.alternate_done.load(Ordering::Relaxed) == 0 {
        if let Some(create) = inner.create.as_ref() {
            let twin = create.create(&e.patch.alternate);
            let g = &counters().groups[group];
            a32(if twin.is_null() { g.failed } else { g.built }).fetch_add(1, Ordering::Relaxed);
            e.alternate_twin.store(twin, Ordering::Release);
        }
    }
    e.alternate_done.store(1, Ordering::Release);
    e.alternate_twin.load(Ordering::Acquire)
}
/// One twin for every view: the shader reads the view's own shift.
fn bind_fixed(e: &Entry, container: &[u8], group: usize, pso: *mut c_void) -> *mut c_void {
    if !group_enabled(group) || (stereo_only(group) && !engine_stereo()) {
        return pso;
    }
    let twin = fixed_twin(e, container, group);
    if twin.is_null() {
        return pso;
    }
    a64(counters().groups[group].binds).fetch_add(1, Ordering::Relaxed);
    twin
}
fn bind_alternate(e: &Entry) -> Option<*mut c_void> {
    let group = e.patch.alternate_group?;
    if !group_enabled(group) || !engine_stereo() {
        return None;
    }
    let twin = create_alternate(e);
    if twin.is_null() {
        return None;
    }
    a64(counters().groups[group].binds).fetch_add(1, Ordering::Relaxed);
    Some(twin)
}
fn fallback(e: Option<&Entry>, pso: *mut c_void) -> *mut c_void {
    match (e, e.and_then(|e| e.patch.fallback_group)) {
        (Some(e), Some(group)) if !e.patch.fallback.is_empty() => bind_fixed(e, &e.patch.fallback, group, pso),
        _ => pso,
    }
}

// ---- the calls the device hooks make

pub fn has_patch(code: &[u8]) -> bool {
    patch_for(code).is_some()
}
/// LibraryTwin: the stereo_rt twin of a state object's DXIL library while that group is on.
pub fn library_twin(code: &[u8]) -> Option<&'static [u8]> {
    let p = patch_for(code)?;
    (p.fixed && p.group == STEREO_RT && group_enabled(STEREO_RT)).then_some(&p.container[..])
}
pub fn library_twin_created(ok: bool, libraries: u32) {
    let g = &counters().groups[STEREO_RT];
    if ok {
        a64(g.binds).fetch_add(1, Ordering::Relaxed);
    }
    a32(if ok { g.built } else { g.failed }).fetch_add(libraries, Ordering::Relaxed);
}
/// BoundCode: under engine stereo, the bytecode Bind puts in place of `code` while the groups stay as they are (its
/// alternate, fixed twin or fallback), or None for `code` itself.
pub fn bound_code(code: &[u8]) -> Option<&'static [u8]> {
    let p = patch_for(code)?;
    let on = |group: Option<usize>| group.is_some_and(|g| group_enabled(g) && (!stereo_only(g) || engine_stereo()));
    if p.fixed {
        if p.alternate_group.is_some_and(group_enabled) && engine_stereo() {
            return Some(&p.alternate);
        }
        return on(Some(p.group)).then_some(&p.container[..]);
    }
    // A mask-parameterised twin is never bound under engine stereo: its fallback is.
    if engine_stereo() && !p.fallback.is_empty() && on(p.fallback_group) {
        return Some(&p.fallback);
    }
    None
}
/// FixedVertexTwin: the fixed twin of a vertex shader whose group is on.
pub fn fixed_vertex_twin(code: &[u8]) -> Option<&'static [u8]> {
    let p = patch_for(code)?;
    (p.fixed && group_enabled(p.group)).then_some(&p.container[..])
}
/// Register: `original` was created with `code`; `create` rebuilds its description around a twin's bytecode.
pub fn register(original: &ID3D12PipelineState, code: &[u8], create: Creator) {
    let Some(patch) = patch_for(code) else { return };
    let entry = EntryRef::new(patch, create);
    {
        let _guard = lock(&REGISTRATION);
        // A driver may return an existing immutable pipeline again: keep its current twins alive instead of replacing
        // their owner while recorded commands can still use them.
        if find(original).is_some() {
            return;
        }
        let raw = entry.0 as *mut c_void;
        // SAFETY: the entry is a live COM object (IUnknown vtable first); the pipeline takes its own reference.
        let unknown = unsafe { IUnknown::from_raw_borrowed(&raw) };
        // SAFETY: the live pipeline.
        if unsafe { original.SetPrivateDataInterface(&ENTRY_KEY, unknown) }.is_err() {
            return;
        }
        a32(counters().ham_registered).store(REGISTERED.fetch_add(1, Ordering::Relaxed) + 1, Ordering::Relaxed);
    }
    if patch.alternate_group.is_some_and(group_enabled) {
        create_alternate(&entry);
    }
    if patch.fixed && FOLDERS[patch.group].1 && group_enabled(patch.group) {
        fixed_twin(&entry, &patch.container, patch.group);
        lock(&entry.inner).create = None; // done for good: drop the owned description
    }
}
/// Bind: the pipeline to bind for `pso` in view `side`.
pub fn bind(pso: &ID3D12PipelineState, side: i32) -> *mut c_void {
    let raw = pso.as_raw();
    if REGISTERED.load(Ordering::Relaxed) == 0 {
        return raw;
    }
    let owner = find(pso);
    let e = owner.as_deref();
    if let Some(e) = e.filter(|e| e.patch.fixed) {
        if let Some(alternate) = bind_alternate(e) {
            return alternate;
        }
        return bind_fixed(e, &e.patch.container, e.patch.group, raw);
    }
    let c = counters();
    if a32(c.ham_patch).load(Ordering::Relaxed) == 0 {
        return fallback(e, raw);
    }
    let Some(entry) = e else { return raw };
    if !(0..=1).contains(&side) || a32(c.hidden_area_enabled).load(Ordering::Relaxed) == 0 || menu_mode() != 0 || engine_stereo() {
        return fallback(e, raw);
    }
    let side = side as usize;
    let generation = PUBLISHED_GENERATION[side].load(Ordering::Acquire);
    if generation == 0 {
        return fallback(e, raw);
    }
    let twin = if entry.generation[side].load(Ordering::Acquire) == generation {
        entry.twin[side].load(Ordering::Acquire)
    } else {
        let (params, current) = {
            let p = lock(&PUBLISH);
            (p.published[side], PUBLISHED_GENERATION[side].load(Ordering::Relaxed))
        };
        let mut inner = lock(&entry.inner);
        let twin = build(entry, &mut inner, params);
        entry.twin[side].store(twin, Ordering::Release);
        entry.generation[side].store(current, Ordering::Release);
        twin
    };
    if twin.is_null() {
        return fallback(e, raw);
    }
    a64(c.ham_binds).fetch_add(1, Ordering::Relaxed);
    twin
}

// ---- the census

struct Shader {
    hash: u64,
    bytes: Option<Arc<Vec<u8>>>,
}
#[derive(Default)]
struct Census {
    blobs: FastMap<u64, Arc<Vec<u8>>>,
    pipelines: FastMap<usize, (u64, Arc<Vec<u8>>)>,
    vertex_shaders: FastMap<usize, (u64, Arc<Vec<u8>>)>,
    written: FastSet<u64>,
}
static CENSUS: Mutex<Option<Census>> = Mutex::new(None);
static CENSUS_LOG: Mutex<()> = Mutex::new(());

/// The census switch file in the log folder, looked for once.
fn recording() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let folder = paths::dir(Folder::Log);
        let found = !folder.as_os_str().is_empty() && folder.join("cvr_shader_census.txt").exists();
        a32(counters().census_recording).store(found as u32, Ordering::Relaxed);
        found
    })
}

/// PipelineCreated: the shaders of a pipeline the engine created (kept only while the census records).
pub fn pipeline_created(pso: *mut c_void, cs: &[u8], ps: &[u8], vs: &[u8]) {
    if pso.is_null() || !recording() {
        return;
    }
    let keep = |code: &[u8], vertex: bool| {
        if code.is_empty() {
            return;
        }
        let hash = fnv(code);
        let mut guard = lock(&CENSUS);
        let census = guard.get_or_insert_with(Census::default);
        let blob = census.blobs.entry(hash).or_insert_with(|| Arc::new(code.to_vec())).clone();
        let into = if vertex { &mut census.vertex_shaders } else { &mut census.pipelines };
        into.insert(pso as usize, (hash, blob));
    };
    keep(if cs.is_empty() { ps } else { cs }, false);
    keep(vs, true);
    a64(counters().census_pipelines).fetch_add(1, Ordering::Relaxed);
}

/// The kinds of Command (HiddenAreaShaders.hpp Kind).
const KIND_DRAW: u32 = 3;
const KIND_DRAW_INDEXED: u32 = 4;

/// Command: every dispatch, indirect execution and draw, logged by the census.
#[allow(clippy::too_many_arguments)]
pub fn command(node: u32, side: i32, pso: *mut c_void, kind: u32, x: u32, y: u32, z: u32) {
    let c = counters();
    let mode = a32(c.census).load(Ordering::Relaxed);
    if mode == 0 || !recording() {
        return;
    }
    let draw = kind == KIND_DRAW || kind == KIND_DRAW_INDEXED;
    // Mode 1: full-screen draws and dispatches -- material draws are not screen-space passes.
    // Mode 2: instanced draws only, by vertex shader (particles and the like).
    // Mode 3: every draw by vertex shader, one row per node and shader (counts dropped so it fits).
    // Mode 4: every draw by pixel shader, one row per node and shader (material shaders).
    if mode == 1 && (kind == KIND_DRAW_INDEXED || (kind == KIND_DRAW && !(x <= 4 && y == 1))) {
        return;
    }
    if mode == 2 && !(draw && y > 1) {
        return;
    }
    if mode >= 3 && !draw {
        return;
    }
    let (side, x, y, z) = if mode >= 3 { (-9, 0, 0, 0) } else { (side, x, y, z) };
    let shader = {
        let mut guard = lock(&CENSUS);
        let census = guard.get_or_insert_with(Census::default);
        let from = if mode == 2 || mode == 3 { &census.vertex_shaders } else { &census.pipelines };
        let shader = from.get(&(pso as usize)).map_or(Shader { hash: 0, bytes: None }, |(h, b)| Shader {
            hash: *h,
            bytes: Some(b.clone()),
        });
        if let Some(bytes) = &shader.bytes {
            if census.written.insert(shader.hash) {
                let folder = paths::dir(Folder::Log).join("cvr_shaders");
                let _ = std::fs::create_dir(&folder);
                let _ = std::fs::write(folder.join(format!("{:016x}.dxil", shader.hash)), bytes.as_slice());
            }
        }
        shader
    };
    let key = if mode >= 3 { 0 } else { pso as u64 };
    let bytes = shader.bytes.as_ref().map_or(0, |b| b.len() as u32);
    if c.census_log.is_null() {
        return;
    }
    let _guard = lock(&CENSUS_LOG);
    // SAFETY: the exported log array and its length; every writer holds CENSUS_LOG.
    let log = unsafe { core::slice::from_raw_parts_mut(c.census_log, c.census_log_len as usize) };
    for r in log.iter_mut() {
        if r.count != 0 && r.node == node && r.side == side && r.kind == kind && r.hash == shader.hash && r.x == x && r.y == y && r.z == z && r.pso == key {
            r.count += 1;
            return;
        }
        if r.count == 0 {
            *r = CensusRecord {
                node,
                side,
                kind,
                x,
                y,
                z,
                hash: shader.hash,
                count: 1,
                bytes,
                pso: key,
            };
            return;
        }
    }
}

// ---- the C ABI (include/Render/HiddenAreaShaders.hpp)

/// # Safety
/// `p` is null or `n` readable bytes alive for the call.
unsafe fn code<'a>(p: *const c_void, n: usize) -> &'a [u8] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        // SAFETY: the caller's bytecode.
        unsafe { core::slice::from_raw_parts(p as *const u8, n) }
    }
}
/// # Safety
/// `ptr`/`len` are writable.
unsafe fn answer(found: Option<&'static [u8]>, ptr: *mut *const c_void, len: *mut usize) -> bool {
    let (p, n) = found.map_or((core::ptr::null(), 0), |b| (b.as_ptr() as *const c_void, b.len()));
    // SAFETY: the caller's out-pointers.
    unsafe {
        if let Some(ptr) = ptr.as_mut() {
            *ptr = p;
        }
        if let Some(len) = len.as_mut() {
            *len = n;
        }
    }
    found.is_some()
}

/// # Safety
/// `code` is null or `size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_shader_patches_has_patch(code_ptr: *const c_void, size: usize) -> bool {
    // SAFETY: the caller's bytecode.
    has_patch(unsafe { code(code_ptr, size) })
}
/// # Safety
/// As cvr_shader_patches_has_patch; `out`/`out_size` writable. The answer points into the library (process lifetime).
#[no_mangle]
pub unsafe extern "C" fn cvr_shader_patches_library_twin(code_ptr: *const c_void, size: usize, out: *mut *const c_void, out_size: *mut usize) -> bool {
    // SAFETY: the caller's buffers.
    unsafe { answer(library_twin(code(code_ptr, size)), out, out_size) }
}
#[no_mangle]
pub extern "C" fn cvr_shader_patches_library_twin_created(ok: bool, libraries: u32) {
    library_twin_created(ok, libraries)
}
/// # Safety
/// As cvr_shader_patches_library_twin.
#[no_mangle]
pub unsafe extern "C" fn cvr_shader_patches_bound_code(code_ptr: *const c_void, size: usize, out: *mut *const c_void, out_size: *mut usize) -> bool {
    // SAFETY: the caller's buffers.
    unsafe { answer(bound_code(code(code_ptr, size)), out, out_size) }
}
/// # Safety
/// As cvr_shader_patches_library_twin.
#[no_mangle]
pub unsafe extern "C" fn cvr_shader_patches_fixed_vertex_twin(code_ptr: *const c_void, size: usize, out: *mut *const c_void, out_size: *mut usize) -> bool {
    // SAFETY: the caller's buffers.
    unsafe { answer(fixed_vertex_twin(code(code_ptr, size)), out, out_size) }
}
/// Takes ownership of `ctx` (freed through `free`, also when nothing is registered).
///
/// # Safety
/// `original` is an ID3D12PipelineState or null; `code` as cvr_shader_patches_has_patch; `call`/`free` valid for `ctx`.
#[no_mangle]
pub unsafe extern "C" fn cvr_shader_patches_register(
    original: *mut c_void,
    code_ptr: *const c_void,
    size: usize,
    ctx: *mut c_void,
    call: unsafe extern "C" fn(*mut c_void, *const u8, usize) -> *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
) {
    let create = Creator { ctx, call, free };
    // SAFETY: the caller's pipeline and bytecode.
    unsafe {
        if let Some(original) = ID3D12PipelineState::from_raw_borrowed(&original) {
            register(original, code(code_ptr, size), create);
        }
    }
}
/// # Safety
/// `scales` is four floats or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_shader_patches_publish(side: i32, scales: *const [f32; 4]) {
    // SAFETY: the caller's floats.
    if let Some(scales) = unsafe { scales.as_ref() } {
        publish(side, scales);
    }
}
/// # Safety
/// `pso` is an ID3D12PipelineState or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_shader_patches_bind(pso: *mut c_void, side: i32) -> *mut c_void {
    // SAFETY: the caller's pipeline.
    match unsafe { ID3D12PipelineState::from_raw_borrowed(&pso) } {
        Some(p) => bind(p, side),
        None => pso,
    }
}
/// # Safety
/// Each bytecode is null or its size in readable bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_shader_patches_pipeline_created(
    pso: *mut c_void,
    cs: *const c_void,
    cs_size: usize,
    ps: *const c_void,
    ps_size: usize,
    vs: *const c_void,
    vs_size: usize,
) {
    // SAFETY: the caller's bytecode.
    unsafe { pipeline_created(pso, code(cs, cs_size), code(ps, ps_size), code(vs, vs_size)) }
}
#[no_mangle]
pub extern "C" fn cvr_shader_patches_command(node: u32, side: i32, pso: *mut c_void, kind: u32, x: u32, y: u32, z: u32) {
    command(node, side, pso, kind, x, y, z)
}

#[cfg(test)]
mod tests {
    use super::hex_key;
    #[test]
    fn keys_parse_like_wcstoull() {
        assert_eq!(hex_key("110db751682e69a1.dxil"), 0x110db751682e69a1);
        assert_eq!(hex_key("ABCDEF.dxil"), 0xabcdef);
        assert_eq!(hex_key("0x1F.dxil"), 0x1f);
        assert_eq!(hex_key("zz.dxil"), 0);
        assert_eq!(hex_key("11111111111111111.dxil"), u64::MAX);
        assert_eq!(hex_key("-1.dxil"), u64::MAX);
    }
}
