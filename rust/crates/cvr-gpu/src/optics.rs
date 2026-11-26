//! The physical scope's glass (formerly src/Render/PhysicalOpticsShaders.cpp and LensMask.cpp;
//! include/Render/PhysicalOpticsShaders.hpp calls it).
//!
//! The glass_scope pixel shaders are replaced by the port's (the two 2.5 cache variants, by their native hash; 2.31:
//! 0x011D27FFE5AAD161, 0x39E6BD24D20C3295 -- the same techniques, recompiled with the engine's two-halves mode), and
//! each glass PSO owns its auxiliary twins as COM private data: the lens velocity redraw (the glass pass writes no
//! velocity, so the upscaler moved lens history with the unmagnified world behind it) and the upscaler history mask.
//! A glass draw shades every pixel: the lens magnifies one coarse VRS pixel into an M x M block, and the engine's
//! adaptive shading-rate image coarsens exactly while the camera turns.
//!
//! The history mask: one R8 mask per eye at the velocity target's size, kept in RENDER_TARGET for its whole life.
//! Right after each glass draw (and, on the camera path, each camera-surface draw) the port redraws the same geometry
//! into it with 1. When that eye's velocity is tagged for DLSS, the mask goes with it as
//! kBufferTypeBiasCurrentColorHint, lifecycle eOnlyValidNow: Streamline copies it on the tagging list right away, so the
//! port clears it on that same list after the tag and the next frame starts empty. A mask nothing drew into since the
//! last tag is not tagged.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::FastMap;
use core::ffi::c_void;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering,
};
use std::sync::{Mutex, MutexGuard, OnceLock};
use windows::core::{Interface, GUID};
use windows::Win32::Foundation::S_OK;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::SystemInformation::GetTickCount64;

fn log(text: &str) {
    cvr_host::log::write(text);
}
fn now() -> u64 {
    // SAFETY: no arguments.
    unsafe { GetTickCount64() }
}

// ---- the port's shaders

struct ShaderFile {
    native_hash: u64,
    file: &'static str,
    bytes: OnceLock<Vec<u8>>,
}
impl ShaderFile {
    const fn new(native_hash: u64, file: &'static str) -> Self {
        ShaderFile {
            native_hash,
            file,
            bytes: OnceLock::new(),
        }
    }
    fn get(&self) -> &[u8] {
        self.bytes.get_or_init(|| load(self.file))
    }
}
fn load(file: &str) -> Vec<u8> {
    let path = cvr_host::paths::dir(cvr_host::paths::Folder::Shaders).join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        log(&format!("[optics] Missing glass shader {file}\n"));
        return Vec::new();
    };
    if bytes.len() < 64 || bytes.len() > 262144 || &bytes[..4] != b"DXBC" {
        return Vec::new();
    }
    log(&format!(
        "[optics] Loaded glass shader {file} ({} bytes)\n",
        bytes.len()
    ));
    bytes
}
static GLASS_SHADERS: [ShaderFile; 2] = [
    ShaderFile::new(
        0x6495_67EB_5A03_9A01,
        "CyberpunkVR_ScopeGlass_649567EB5A039A01.dxil",
    ),
    ShaderFile::new(
        0x372E_EC30_C414_B67A,
        "CyberpunkVR_ScopeGlass_372EEC30C414B67A.dxil",
    ),
];
/// The auxiliary shaders as files, by `cvr_optics_shader`'s kind: the DXIL DXC made from src/Shaders/*.hlsl, read
/// with CVR_SHADER_HLSL only (`auxiliary`).
static AUXILIARY: [ShaderFile; 5] = [
    ShaderFile::new(0, "CyberpunkVR_LensVelocityPs.dxil"),
    ShaderFile::new(0, "CyberpunkVR_LensMaskPs.dxil"),
    ShaderFile::new(0, "CyberpunkVR_SightMaskPs.dxil"),
    ShaderFile::new(0, "CyberpunkVR_LensSourceResolveCs.dxil"),
    ShaderFile::new(0, "CyberpunkVR_CameraSourceResolveCs.dxil"),
];
/// An auxiliary shader by kind: the DXIL made from Rust (rust/shaders/passes/src/optics.rs, optics_resolve.rs),
/// embedded; the file it replaces when CVR_SHADER_HLSL names it.
fn auxiliary(kind: usize) -> &'static [u8] {
    let rust = match kind {
        0 => crate::dxil::LENS_VELOCITY_PS,
        1 => crate::dxil::LENS_MASK_PS,
        2 => crate::dxil::SIGHT_MASK_PS,
        3 => crate::dxil::LENS_SOURCE_RESOLVE_CS,
        4 => crate::dxil::CAMERA_SOURCE_RESOLVE_CS,
        _ => return &[],
    };
    if crate::dxil::hlsl_for(rust) {
        AUXILIARY[kind].get()
    } else {
        rust
    }
}
fn bytecode(bytes: &[u8]) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: bytes.as_ptr().cast(),
        BytecodeLength: bytes.len(),
    }
}
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(1469598103934665603u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    })
}
/// One of the port's auxiliary shaders (0 lens velocity, 1 lens mask, 2 sight mask, 3 lens source resolve, 4 camera
/// source resolve), loaded once.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_shader(kind: u32, out: *mut D3D12_SHADER_BYTECODE) -> bool {
    *out = D3D12_SHADER_BYTECODE::default();
    let bytes = auxiliary(kind as usize);
    if bytes.is_empty() {
        return false;
    }
    *out = bytecode(bytes);
    true
}
/// GlassShaderReplacement: the port's glass for one of the two native glass_scope pixel shaders (rejected by their
/// measured lengths before hashing unrelated shaders).
///
/// # Safety
/// `native` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_glass_replacement(
    native: *const D3D12_SHADER_BYTECODE,
    out: *mut D3D12_SHADER_BYTECODE,
) -> bool {
    *out = D3D12_SHADER_BYTECODE::default();
    let native = &*native;
    if native.pShaderBytecode.is_null() || !matches!(native.BytecodeLength, 32289 | 32581) {
        return false;
    }
    let hash = fnv(core::slice::from_raw_parts(
        native.pShaderBytecode.cast::<u8>(),
        native.BytecodeLength,
    ));
    let Some(shader) = GLASS_SHADERS.iter().find(|s| s.native_hash == hash) else {
        return false;
    };
    let bytes = shader.get();
    if bytes.is_empty() {
        return false;
    }
    *out = bytecode(bytes);
    true
}

// ---- the glass pipelines and their twins (the variant index: cvr-gpu's pipeline_variants.rs; the velocity variant
// and the mask)

const VARIANTS_KEY: GUID = GUID::from_values(
    0x445b6076,
    0x2f13,
    0x40e5,
    [0xa7, 0x3b, 0x11, 0xd6, 0x89, 0x8f, 0x42, 0x6a],
);
static INDEX: crate::pipeline_variants::VariantIndex = crate::pipeline_variants::VariantIndex::new(VARIANTS_KEY);

static GLASS_TICK: AtomicU64 = AtomicU64::new(0);
static GLASS_COUNT: AtomicU32 = AtomicU32::new(0);

/// Whether the pipeline a thread has bound is a glass: the draw hook asks three times on every draw (full-rate
/// shading, lens velocity, the lens mask) and the answer only changes with the pipeline or the index; the index's
/// probe is a cache miss on most draws, this a compare.
fn is_glass(pso: usize) -> bool {
    thread_local!(static LAST: Cell<(usize, u64, bool)> = const { Cell::new((0, u64::MAX, false)) });
    let generation = INDEX.generation();
    LAST.with(|last| {
        let (p, g, glass) = last.get();
        if p == pso && g == generation {
            return glass;
        }
        let glass = pso != 0 && INDEX.find(pso).is_some();
        last.set((pso, generation, glass));
        glass
    })
}
#[no_mangle]
pub extern "C" fn cvr_optics_glass_pipeline(pso: usize) -> bool {
    is_glass(pso)
}
/// GlassPipelineCreated: the glass and its twins (velocity, mask; both creation references taken, either may be
/// null). Even a glass with no twins needs an entry for full-rate shading.
///
/// # Safety
/// `pso` is a live pipeline; `velocity` and `mask` are owned references or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_glass_created(
    pso: *mut c_void,
    velocity: *mut c_void,
    mask: *mut c_void,
) {
    let hr = INDEX.store(pso, velocity, mask);
    if hr == S_OK {
        let n = GLASS_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 || n % 16 == 0 {
            log(&format!("[optics] Glass pipelines registered: {n}\n"));
        }
    } else if hr.is_err() {
        log(&format!(
            "[optics] Glass variants attachment failed hr={:08X}\n",
            hr.0 as u32
        ));
    }
}
#[no_mangle]
pub extern "C" fn cvr_optics_glass_recent() -> bool {
    now().wrapping_sub(GLASS_TICK.load(Ordering::Relaxed)) < 500
}
/// GlassMaskPipeline: the mask twin of a glass pipeline (borrowed), null for any other pipeline.
#[no_mangle]
pub extern "C" fn cvr_optics_glass_mask_pipeline(pso: usize) -> *mut c_void {
    if !is_glass(pso) {
        return core::ptr::null_mut();
    }
    INDEX
        .find(pso)
        .and_then(|t| t.mask.as_ref())
        .map_or(core::ptr::null_mut(), |m| m.as_raw())
}

// ---- the engine's shading rate per list (only lists that set one)

#[derive(Clone, Copy, PartialEq, Eq)]
struct Rate {
    value: i32,
    combiners: [i32; 2],
}
// D3D12_SHADING_RATE_1X1 with both combiners D3D12_SHADING_RATE_COMBINER_PASSTHROUGH.
const DEFAULT_RATE: Rate = Rate {
    value: 0,
    combiners: [0, 0],
};
static RATES: Mutex<Option<FastMap<usize, Rate>>> = Mutex::new(None);
fn rates() -> MutexGuard<'static, Option<FastMap<usize, Rate>>> {
    RATES.lock().unwrap_or_else(|e| e.into_inner())
}
/// # Safety
/// `combiners` holds two combiners or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_shading_rate_set(
    list: usize,
    value: i32,
    combiners: *const i32,
) {
    let rate = Rate {
        value,
        combiners: if combiners.is_null() {
            [0, 0]
        } else {
            *(combiners as *const [i32; 2])
        },
    };
    let mut rates = rates();
    let rates = rates.get_or_insert_with(cvr_host::fast::fast_map);
    if rates.len() < 4096 || rates.contains_key(&list) {
        rates.insert(list, rate);
    }
}
/// FullRateDraw: for a glass draw, the engine's rate to hand back after it (false: it is already 1x1 passthrough,
/// or not a glass). Base 1x1 with both combiners passthrough ignores per-primitive rates and the rate image.
///
/// # Safety
/// `value` and `combiners` are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_full_rate(
    list: usize,
    pso: usize,
    value: *mut i32,
    combiners: *mut i32,
) -> bool {
    if list == 0 || pso == 0 || !is_glass(pso) {
        return false;
    }
    let engine = rates()
        .as_ref()
        .and_then(|r| r.get(&list).copied())
        .unwrap_or(DEFAULT_RATE);
    if engine == DEFAULT_RATE {
        return false;
    }
    *value = engine.value;
    *(combiners as *mut [i32; 2]) = engine.combiners;
    true
}

// ---- the engine's velocity target, as Streamline is told about it (both eyes render into the same G-buffer
// velocity texture, tagged before every DLSS evaluation, so the previous tag names the coming glass draw's target)

// The heap only keeps the target's view alive.
#[allow(dead_code)]
struct Velocity {
    resource: Option<ID3D12Resource>,
    heap: Option<ID3D12DescriptorHeap>,
    rtv: usize,
    ready: bool,
}
// SAFETY: free-threaded D3D12 objects, under the mutex.
unsafe impl Send for Velocity {}
static VELOCITY: Mutex<Velocity> = Mutex::new(Velocity {
    resource: None,
    heap: None,
    rtv: 0,
    ready: false,
});
fn velocity() -> MutexGuard<'static, Velocity> {
    VELOCITY.lock().unwrap_or_else(|e| e.into_inner())
}
static VELOCITY_FORMAT_LOGGED: AtomicBool = AtomicBool::new(false);
/// MotionVectorsTagged: Streamline's MotionVectors tag (sl::BufferType 1) names the velocity target.
///
/// # Safety
/// `resource` is null or live.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_motion_vectors_tagged(resource: *mut c_void, verbose: bool) {
    let Some(r) = ID3D12Resource::from_raw_borrowed(&resource) else {
        return;
    };
    let mut v = velocity();
    if v.resource.as_ref().is_some_and(|x| x.as_raw() == resource) {
        return;
    }
    *v = Velocity {
        resource: None,
        heap: None,
        rtv: 0,
        ready: false,
    };
    let desc = r.GetDesc();
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.Format != DXGI_FORMAT_R16G16_FLOAT
        || desc.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET.0 == 0
    {
        if !VELOCITY_FORMAT_LOGGED.swap(true, Ordering::Relaxed) {
            log(&format!(
                "[optics] Velocity tag format {} flags {:X} unsupported; lens velocity off\n",
                desc.Format.0 as u32, desc.Flags.0 as u32
            ));
        }
        return;
    }
    let mut device: Option<ID3D12Device> = None;
    if r.GetDevice(&mut device).is_err() {
        return;
    }
    let Some(device) = device else { return };
    let heap_desc = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
        NumDescriptors: 1,
        Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
        NodeMask: 0,
    };
    let Ok(heap) = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&heap_desc) else {
        return;
    };
    let view = D3D12_RENDER_TARGET_VIEW_DESC {
        Format: DXGI_FORMAT_R16G16_FLOAT,
        ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let rtv = heap.GetCPUDescriptorHandleForHeapStart();
    device.CreateRenderTargetView(r, Some(&view), rtv);
    *v = Velocity {
        resource: Some(r.clone()),
        heap: Some(heap),
        rtv: rtv.ptr,
        ready: true,
    };
    // Re-tagged every time the game recreates its motion vectors: a line for the debug log only.
    if verbose {
        log(&format!(
            "[optics] Lens velocity target {:016X} {}x{}\n",
            resource as usize, desc.Width, desc.Height
        ));
    }
}
/// MotionVectors: the tagged velocity target (an owned reference), or null.
#[no_mangle]
pub extern "C" fn cvr_optics_motion_vectors() -> *mut c_void {
    let v = velocity();
    match (&v.resource, v.ready) {
        (Some(r), true) => r.clone().into_raw(),
        _ => core::ptr::null_mut(),
    }
}

// ---- the render targets as last bound on a list by its recording thread (one list is recorded by one thread at a
// time; a few slots cover interleaved recording)

/// optics::BoundTargets (88 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BoundTargets {
    pub count: u32,
    pub contiguous: i32,
    pub handles: [usize; 8],
    pub depth: usize,
    pub has_depth: bool,
    pub valid: bool,
}
const _: () = assert!(core::mem::size_of::<BoundTargets>() == 88);
#[derive(Clone, Copy, Default)]
struct Bound {
    list: usize,
    targets: BoundTargets,
}
thread_local! {
    static BOUND: RefCell<([Bound; 4], usize)> = RefCell::new(([Bound::default(); 4], 0));
}
/// Targets: every engine OMSetRenderTargets, so the redraw can restore the glass targets.
///
/// # Safety
/// `handles` holds `count` handles (one when contiguous) or is null; `depth` is null or valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_targets(
    list: usize,
    count: u32,
    handles: *const usize,
    contiguous: i32,
    depth: *const usize,
) {
    BOUND.with(|b| {
        let (slots, next) = &mut *b.borrow_mut();
        let index = match slots.iter().position(|s| s.list == list) {
            Some(i) => i,
            None => {
                let i = *next % slots.len();
                *next += 1;
                slots[i].list = list;
                i
            }
        };
        let entry = &mut slots[index];
        entry.targets = BoundTargets::default();
        let count = if handles.is_null() { 0 } else { count };
        if count > 8 {
            // Unsupported: never redraw over it.
            entry.list = 0;
            return;
        }
        let t = &mut entry.targets;
        t.count = count;
        t.contiguous = contiguous;
        let copied = if contiguous != 0 { count.min(1) } else { count };
        for i in 0..copied as usize {
            t.handles[i] = *handles.add(i);
        }
        if !depth.is_null() && *depth != 0 {
            t.depth = *depth;
            t.has_depth = true;
        }
        t.valid = true;
    });
}
fn bound_of(list: usize) -> Option<BoundTargets> {
    BOUND.with(|b| {
        b.borrow()
            .0
            .iter()
            .find(|s| s.list == list && s.targets.valid)
            .map(|s| s.targets)
    })
}
/// BoundTargetsOf: the render targets the engine last bound on this list (this thread).
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_bound_targets(list: usize, out: *mut BoundTargets) -> bool {
    match bound_of(list) {
        Some(t) => {
            *out = t;
            true
        }
        None => false,
    }
}
/// ListReset: Reset and ClearState restore D3D12's default rate (1x1, both combiners passthrough) and unbind.
#[no_mangle]
pub extern "C" fn cvr_optics_list_reset(list: usize) {
    BOUND.with(|b| {
        for slot in b.borrow_mut().0.iter_mut().filter(|s| s.list == list) {
            *slot = Bound::default();
        }
    });
    if let Some(rate) = rates().as_mut().and_then(|r| r.get_mut(&list)) {
        *rate = DEFAULT_RATE;
    }
}
/// optics::LensVelocityPass with its target as an owned reference.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LensVelocityPass {
    pub pipeline: usize,
    pub target: usize,
    pub rtv: usize,
    pub targets: BoundTargets,
}
/// PrepareLensVelocity: the glass's velocity twin, the targets bound for the glass and the velocity target.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_prepare_lens_velocity(
    list: usize,
    pso: usize,
    out: *mut LensVelocityPass,
) -> bool {
    *out = LensVelocityPass::default();
    if !is_glass(pso) {
        return false;
    }
    let Some(glass) = INDEX.find(pso) else {
        return false;
    };
    GLASS_TICK.store(now(), Ordering::Relaxed);
    let Some(pipeline) = glass.variant.as_ref() else {
        return false;
    };
    let Some(targets) = bound_of(list) else {
        return false;
    };
    let v = velocity();
    let (true, Some(target)) = (v.ready, v.resource.as_ref()) else {
        return false;
    };
    *out = LensVelocityPass {
        pipeline: pipeline.as_raw() as usize,
        target: target.clone().into_raw() as usize,
        rtv: v.rtv,
        targets,
    };
    true
}

// ---- the upscaler history mask

// The C++ exports (src/Render/LensMask.cpp attaches them): CyberpunkVR_OpticsLensClean -- 1 the upscaler takes the
// current frame inside the scope image (and the lens source resolve hands the glass its stabilised image always), 0
// off -- and per eye the mask redraws recorded and the masks tagged for DLSS.
static LENS_CLEAN: AtomicPtr<AtomicI32> = AtomicPtr::new(core::ptr::null_mut());
static MASK_DRAWS: AtomicPtr<AtomicU64> = AtomicPtr::new(core::ptr::null_mut());
static MASK_TAGS: AtomicPtr<AtomicU64> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// The pointers are the C++ exports (a flag and two arrays of two counters), live for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_mask_attach(
    clean: *mut AtomicI32,
    draws: *mut AtomicU64,
    tags: *mut AtomicU64,
) {
    LENS_CLEAN.store(clean, Ordering::Release);
    MASK_DRAWS.store(draws, Ordering::Release);
    MASK_TAGS.store(tags, Ordering::Release);
}
/// CyberpunkVR_OpticsLensClean (1 until attached).
pub fn lens_clean() -> i32 {
    // SAFETY: the attached export.
    unsafe { LENS_CLEAN.load(Ordering::Acquire).as_ref() }.map_or(1, |c| c.load(Ordering::Relaxed))
}
fn bump(counters: &AtomicPtr<AtomicU64>, side: usize) {
    let base = counters.load(Ordering::Acquire);
    if !base.is_null() {
        // SAFETY: the attached two-counter export; side is 0 or 1.
        unsafe { (*base.add(side)).fetch_add(1, Ordering::Relaxed) };
    }
}

struct Side {
    mask: Option<ID3D12Resource>,
    heap: Option<ID3D12DescriptorHeap>,
    rtv: usize,
    width: u64,
    height: u32,
}
struct Retired {
    _mask: Option<ID3D12Resource>,
    _heap: Option<ID3D12DescriptorHeap>,
    tick: u64,
}
struct Masks {
    sides: [Side; 2],
    retired: Vec<Retired>,
}
// SAFETY: free-threaded D3D12 objects, under the mutex.
unsafe impl Send for Masks {}
const NO_SIDE: Side = Side {
    mask: None,
    heap: None,
    rtv: 0,
    width: 0,
    height: 0,
};
static MASKS: Mutex<Masks> = Mutex::new(Masks {
    sides: [NO_SIDE, NO_SIDE],
    retired: Vec::new(),
});
static DIRTY: [AtomicBool; 2] = [const { AtomicBool::new(false) }; 2];
static TAGGED: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
// Streamline rejected the mask tag once: never sent again.
static REFUSED: AtomicBool = AtomicBool::new(false);
fn masks() -> MutexGuard<'static, Masks> {
    MASKS.lock().unwrap_or_else(|e| e.into_inner())
}

#[no_mangle]
pub extern "C" fn cvr_optics_mask_enabled() -> bool {
    lens_clean() != 0 && !REFUSED.load(Ordering::Relaxed)
}
/// The eye's mask reached the upscaler within the last half second.
#[no_mangle]
pub extern "C" fn cvr_optics_mask_active(side: i32) -> bool {
    cvr_optics_mask_enabled()
        && (0..=1).contains(&side)
        && now().wrapping_sub(TAGGED[side as usize].load(Ordering::Relaxed)) < 500
}
/// The eye's mask (an owned reference) and its render target view, once its velocity was tagged.
///
/// # Safety
/// `resource` and `rtv` are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_mask_target(
    side: i32,
    resource: *mut *mut c_void,
    rtv: *mut usize,
) -> bool {
    if !cvr_optics_mask_enabled() || !(0..=1).contains(&side) {
        return false;
    }
    let m = masks();
    let s = &m.sides[side as usize];
    let Some(mask) = s.mask.as_ref() else {
        return false;
    };
    *resource = mask.clone().into_raw();
    *rtv = s.rtv;
    true
}
/// A mask redraw was recorded for the eye.
#[no_mangle]
pub extern "C" fn cvr_optics_mask_drawn(side: i32) {
    if !(0..=1).contains(&side) {
        return;
    }
    DIRTY[side as usize].store(true, Ordering::Release);
    bump(&MASK_DRAWS, side as usize);
}

// sl::ResourceTag and sl::Resource as Streamline lays them out (sl_struct.h BaseStructure: next, structType,
// structVersion).
const SL_RESOURCE_TAG: GUID = GUID::from_values(
    0x4c6a5aad,
    0xb445,
    0x496c,
    [0x87, 0xff, 0x1a, 0xf3, 0x84, 0x5b, 0xe6, 0x53],
);
const SL_RESOURCE: GUID = GUID::from_values(
    0x3a9d70cf,
    0x2418,
    0x4b72,
    [0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61],
);
const SL_MOTION_VECTORS: u32 = 1;
const SL_BIAS_CURRENT_COLOR_HINT: u32 = 29;
#[repr(C)]
#[derive(Clone, Copy)]
struct SlExtent {
    top: u32,
    left: u32,
    width: u32,
    height: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct SlResource {
    next: *mut c_void,
    struct_type: GUID,
    struct_version: usize,
    kind: u8,
    native: *mut c_void,
    memory: *mut c_void,
    view: *mut c_void,
    state: u32,
    width: u32,
    height: u32,
    native_format: u32,
    mip_levels: u32,
    array_layers: u32,
    gpu_virtual_address: u64,
    flags: u32,
    usage: u32,
    internal_flags: u16,
    reserved: u16,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct SlResourceTag {
    next: *mut c_void,
    struct_type: GUID,
    struct_version: usize,
    resource: *mut SlResource,
    kind: u32,
    lifecycle: i32,
    extent: SlExtent,
}
const _: () = assert!(
    core::mem::size_of::<SlResource>() == 112 && core::mem::size_of::<SlResourceTag>() == 64
);

/// A MotionVectors tag's 2D resource and extent, each structure copied under SEH (the caller's tags).
fn read_velocity_tag(tag: usize) -> Option<(*mut c_void, SlExtent)> {
    let tag = cvr_engine::code::read::<SlResourceTag>(tag)?;
    if tag.struct_type != SL_RESOURCE_TAG
        || tag.struct_version != 1
        || tag.kind != SL_MOTION_VECTORS
        || tag.resource.is_null()
    {
        return None;
    }
    let r = cvr_engine::code::read::<SlResource>(tag.resource as usize)?;
    if r.struct_type != SL_RESOURCE || r.struct_version != 1 || r.kind != 0 || r.native.is_null() {
        return None;
    }
    Some((r.native, tag.extent))
}
/// Creates (or resizes) the eye's mask; Some(true) when it is new and still holds garbage.
unsafe fn ensure(m: &mut Masks, side: usize, velocity: &ID3D12Resource) -> Option<bool> {
    let desc = velocity.GetDesc();
    let s = &m.sides[side];
    if s.mask.is_some() && s.width == desc.Width && s.height == desc.Height {
        return Some(false);
    }
    let mut device: Option<ID3D12Device> = None;
    velocity.GetDevice(&mut device).ok()?;
    let device = device?;
    let now = now();
    m.retired.retain(|r| now.wrapping_sub(r.tick) <= 3000);
    let s = &mut m.sides[side];
    if s.mask.is_some() {
        let (mask, heap) = (s.mask.take(), s.heap.take());
        m.retired.push(Retired {
            _mask: mask,
            _heap: heap,
            tick: now,
        });
    }
    let s = &mut m.sides[side];
    s.mask = None;
    s.heap = None;
    s.width = 0;
    s.height = 0;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let mask_desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: desc.Width,
        Height: desc.Height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Format: DXGI_FORMAT_R8_UNORM,
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        ..Default::default()
    };
    let clear = D3D12_CLEAR_VALUE {
        Format: DXGI_FORMAT_R8_UNORM,
        ..Default::default()
    };
    let rtvheap = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
        NumDescriptors: 1,
        Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
        NodeMask: 0,
    };
    let _internal = cvr_host::probe::Internal::new();
    let mut mask: Option<ID3D12Resource> = None;
    let created = device.CreateCommittedResource(
        &heap,
        D3D12_HEAP_FLAG_NONE,
        &mask_desc,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
        Some(&clear),
        &mut mask,
    );
    let heap = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&rtvheap);
    let (Ok(()), Some(mask), Ok(heap)) = (created, mask, heap) else {
        return None;
    };
    let _ = mask.SetName(windows::core::w!("CVR scope history mask"));
    let rtv = heap.GetCPUDescriptorHandleForHeapStart();
    let view = D3D12_RENDER_TARGET_VIEW_DESC {
        Format: DXGI_FORMAT_R8_UNORM,
        ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    device.CreateRenderTargetView(&mask, Some(&view), rtv);
    let s = &mut m.sides[side];
    *s = Side {
        mask: Some(mask),
        heap: Some(heap),
        rtv: rtv.ptr,
        width: desc.Width,
        height: desc.Height,
    };
    DIRTY[side].store(false, Ordering::Relaxed);
    log(&format!(
        "[optics] Scope history mask {}x{}\n",
        desc.Width, desc.Height
    ));
    Some(true)
}
/// The mask cleared through the driver's own entry (`clear`: the port's and the stage replay's observers never see
/// it) or the list's.
unsafe fn clear(list: &ID3D12GraphicsCommandList, rtv: usize, clear: usize) {
    const ZERO: [f32; 4] = [0.0; 4];
    let _internal = cvr_host::probe::Internal::new();
    let handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv };
    if clear != 0 {
        type ClearFn = unsafe extern "system" fn(
            *mut c_void,
            D3D12_CPU_DESCRIPTOR_HANDLE,
            *const f32,
            u32,
            *const c_void,
        );
        let f: ClearFn = core::mem::transmute(clear);
        f(list.as_raw(), handle, ZERO.as_ptr(), 0, core::ptr::null());
    } else {
        list.ClearRenderTargetView(handle, &ZERO, None);
    }
}
type SetTagFn = unsafe extern "C" fn(*const c_void, *const c_void, u32, *mut c_void) -> u32;
/// SetTag: slSetTag through the original, the eye's mask added to a velocity tag and cleared after (`clear_original`:
/// the list's unhooked ClearRenderTargetView, or 0).
///
/// # Safety
/// The arguments are slSetTag's; `original` is the original slSetTag or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_mask_set_tag(
    viewport: *const c_void,
    tags: *const c_void,
    count: u32,
    command_list: *mut c_void,
    original: Option<SetTagFn>,
    side: i32,
    clear_original: usize,
) -> u32 {
    let Some(original) = original else { return 1 };
    if !cvr_optics_mask_enabled()
        || !(0..=1).contains(&side)
        || tags.is_null()
        || count == 0
        || count >= 32
        || command_list.is_null()
    {
        return original(viewport, tags, count, command_list);
    }
    let input = tags as *const SlResourceTag;
    let Some((velocity, extent)) =
        (0..count as usize).find_map(|i| read_velocity_tag(input.add(i) as usize))
    else {
        return original(viewport, tags, count, command_list);
    };
    let Some(velocity_resource) = ID3D12Resource::from_raw_borrowed(&velocity) else {
        return original(viewport, tags, count, command_list);
    };
    let Some(list) = ID3D12GraphicsCommandList::from_raw_borrowed(&command_list) else {
        return original(viewport, tags, count, command_list);
    };
    let side = side as usize;
    let mut m = masks();
    let Some(created) = ensure(&mut m, side, velocity_resource) else {
        return original(viewport, tags, count, command_list);
    };
    let (mask, rtv) = (
        m.sides[side].mask.clone().expect("the eye's mask"),
        m.sides[side].rtv,
    );
    if created || !DIRTY[side].swap(false, Ordering::AcqRel) {
        let result = original(viewport, tags, count, command_list);
        if created {
            clear(list, rtv, clear_original);
        }
        return result;
    }
    let mut extended: Vec<SlResourceTag> = (0..count as usize).map(|i| *input.add(i)).collect();
    let mut resource = SlResource {
        next: core::ptr::null_mut(),
        struct_type: SL_RESOURCE,
        struct_version: 1,
        kind: 0,
        native: mask.as_raw(),
        memory: core::ptr::null_mut(),
        view: core::ptr::null_mut(),
        state: D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32,
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
    };
    extended.push(SlResourceTag {
        next: core::ptr::null_mut(),
        struct_type: SL_RESOURCE_TAG,
        struct_version: 1,
        resource: &mut resource,
        kind: SL_BIAS_CURRENT_COLOR_HINT,
        lifecycle: 0,
        extent,
    });
    let result = original(viewport, extended.as_ptr().cast(), count + 1, command_list);
    if result != 0 {
        // An older Streamline that does not know the hint: tag as the game did, mask off for good.
        REFUSED.store(true, Ordering::Relaxed);
        log(&format!(
            "[optics] Streamline refused the scope history mask (result {result}); mask off\n"
        ));
        return original(viewport, tags, count, command_list);
    }
    // Streamline copied the volatile tag on this list; the next frame starts empty.
    clear(list, rtv, clear_original);
    TAGGED[side].store(now(), Ordering::Relaxed);
    bump(&MASK_TAGS, side);
    result
}

// ---- for the source and camera resolves

/// One of the port's auxiliary shaders (as cvr_optics_shader), empty when missing.
pub fn shader(kind: usize) -> &'static [u8] {
    auxiliary(kind)
}
pub fn glass_recent() -> bool {
    cvr_optics_glass_recent()
}
pub fn mask_active(side: usize) -> bool {
    cvr_optics_mask_active(side as i32)
}
/// The tagged velocity target, owned.
pub fn motion_vectors() -> Option<ID3D12Resource> {
    let v = velocity();
    match (&v.resource, v.ready) {
        (Some(r), true) => Some(r.clone()),
        _ => None,
    }
}
