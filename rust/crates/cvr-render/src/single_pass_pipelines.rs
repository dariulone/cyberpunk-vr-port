//! THE PIPELINES SINGLE PASS DRAWS WITH (formerly src/Render/SinglePassPipelines.cpp; include/Render/SinglePassPipelines.hpp
//! has the contract).
//!
//! MATERIAL ROOT: the engine's root signature of RenderElements -- 36 descriptor tables, parameter 0 the vertex stage's
//! CBVs b0..b3, parameter 18 the pixel stage's. Its TWIN appends parameters 36 (vertex) and 37 (pixel): the same four
//! CBVs in register space 10, where the twin shaders read the left eye's b0..b3.
//!
//! MATERIAL PIPELINE: a graphics pipeline created with a material root (VS and an optional PS, no other stage, no
//! stream output, no view instancing). Its description is owned at creation (one copy of each shader for all its
//! pipelines) and kept on the PSO as a COM object, so it lives exactly as long as the PSO. Its view-instanced TWIN is
//! built in the background at first use by one of three workers: VS and PS rewritten by the twin compiler (cached on
//! disk by the shader's hash and the recipe, a pixel shader per vertex shader once its twin turned out to depend on it),
//! the twin root, two views -- view 0 on viewport 0, view 1 on viewport 1, both on render target slice 0. When the
//! hidden-area twins replace a stage, the twin is built from that replacement (the second variant).
//!
//! src/Render/SinglePassPipelines.cpp keeps the exports and the C++ names as forwarders.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::{FastMap, FastSet};
use crate::pipeline_stream::{self as ps, OwnedGraphics, OwnedStream, PipelineStreamOffsets};
use crate::shader_patches;
use crate::view_twin_compiler::{self, Compiler};
use crate::view_twin_ir::{TWIN_RECIPE, TWIN_SPACE};
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, LazyLock, Mutex, Once, OnceLock, Weak};
use std::time::Instant;
use windows::core::{w, IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadDescription, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};

#[allow(non_upper_case_globals)]
extern "C" {
    /// [0] material roots, [1] material pipelines registered, [2] twins queued, [3] twins built, [4] twins failed,
    /// [5] shaders rewritten, [6] shaders that need no rewrite, [7] shaders failed, [8] shaders from the disk cache,
    /// [9] owned description bytes, [10] build milliseconds summed, [11] bound replacement unknown.
    static CyberpunkVR_SinglePassPipelineCounts: [AtomicU64; 12];
    static mut CyberpunkVR_SinglePassError: [c_char; 256];
    /// 0: no material pipeline is registered (and nothing else of single pass runs); read at each root.
    static CyberpunkVR_SinglePassPipelines: AtomicU32;
}
fn count(i: usize) -> &'static AtomicU64 {
    // SAFETY: an export of the C++ half.
    unsafe { &CyberpunkVR_SinglePassPipelineCounts[i] }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
/// The first two dozen errors logged; the last one kept in CyberpunkVR_SinglePassError.
fn error(what: &str) {
    static LOGGED: AtomicI32 = AtomicI32::new(0);
    if LOGGED.fetch_add(1, Relaxed) < 24 {
        log(&format!("[single-pass] {what}\n"));
    }
    // SAFETY: the C++ half's buffer; strncpy_s with _TRUNCATE: at most 255 bytes and a terminator.
    unsafe {
        let buffer = &raw mut CyberpunkVR_SinglePassError;
        let bytes = what.as_bytes();
        let n = bytes.len().min(255);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer as *mut u8, n);
        *(buffer as *mut u8).add(n) = 0;
    }
}
fn fnv(data: &[u8]) -> u64 {
    let mut h = 1469598103934665603u64;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}
unsafe fn bytes_of(code: &D3D12_SHADER_BYTECODE) -> &[u8] {
    if code.pShaderBytecode.is_null() || code.BytecodeLength == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(code.pShaderBytecode as *const u8, code.BytecodeLength)
    }
}
fn code_of(bytes: &[u8]) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: bytes.as_ptr() as *const c_void,
        BytecodeLength: bytes.len(),
    }
}
/// HiddenAreaShaders' BoundCode: what Bind puts in place of `code` under engine stereo, else `code` itself.
unsafe fn bound_code(code: &D3D12_SHADER_BYTECODE) -> D3D12_SHADER_BYTECODE {
    match shader_patches::bound_code(bytes_of(code)) {
        Some(replacement) => code_of(replacement),
        None => *code,
    }
}

// ---- roots

static ROOTS: LazyLock<Mutex<FastMap<usize, ID3D12RootSignature>>> =
    LazyLock::new(|| Mutex::new(cvr_host::fast::fast_map()));
static ROOT_COUNT: AtomicU32 = AtomicU32::new(0);

unsafe fn table_is_camera_block(
    p: &D3D12_ROOT_PARAMETER1,
    visibility: D3D12_SHADER_VISIBILITY,
) -> bool {
    if p.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE
        || p.ShaderVisibility != visibility
        || p.Anonymous.DescriptorTable.NumDescriptorRanges != 1
    {
        return false;
    }
    let r = &*p.Anonymous.DescriptorTable.pDescriptorRanges;
    r.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_CBV
        && r.BaseShaderRegister == 0
        && r.NumDescriptors == 4
        && r.RegisterSpace == 0
}
fn wanted() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { CyberpunkVR_SinglePassPipelines.load(Relaxed) != 0 }
}

/// RootCreated: a material root gets its twin (the device's root just created from `blob`).
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_root_created(
    device: *mut c_void,
    root: *mut c_void,
    blob: *const c_void,
    bytes: usize,
) {
    let Some(device) = ID3D12Device::from_raw_borrowed(&device) else {
        return;
    };
    if root.is_null() || blob.is_null() || bytes == 0 || !wanted() {
        return;
    }
    let mut raw: *mut c_void = core::ptr::null_mut();
    if D3D12CreateVersionedRootSignatureDeserializer(
        blob,
        bytes,
        &ID3D12VersionedRootSignatureDeserializer::IID,
        &mut raw,
    )
    .is_err()
        || raw.is_null()
    {
        return;
    }
    let des = ID3D12VersionedRootSignatureDeserializer::from_raw(raw);
    let Ok(v) = des.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) else {
        return;
    };
    if v.is_null() {
        return;
    }
    let d = &(*v).Anonymous.Desc_1_1;
    if d.NumParameters != 36 || d.pParameters.is_null() {
        return;
    }
    let params_in = core::slice::from_raw_parts(d.pParameters, d.NumParameters as usize);
    if !table_is_camera_block(&params_in[0], D3D12_SHADER_VISIBILITY_VERTEX)
        || !table_is_camera_block(&params_in[18], D3D12_SHADER_VISIBILITY_PIXEL)
    {
        return;
    }
    if params_in
        .iter()
        .any(|p| p.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE)
    {
        return;
    }
    // The twin: the same 36 tables, then b0..b3 in space 10 for the vertex (36) and the pixel stage (37).
    let mut params: Vec<D3D12_ROOT_PARAMETER1> = params_in.to_vec();
    let mut ranges = [
        *params_in[0].Anonymous.DescriptorTable.pDescriptorRanges,
        *params_in[18].Anonymous.DescriptorTable.pDescriptorRanges,
    ];
    for r in ranges.iter_mut() {
        r.RegisterSpace = TWIN_SPACE;
    }
    for (k, range) in ranges.iter().enumerate() {
        params.push(D3D12_ROOT_PARAMETER1 {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER1_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: range,
                },
            },
            ShaderVisibility: if k != 0 {
                D3D12_SHADER_VISIBILITY_PIXEL
            } else {
                D3D12_SHADER_VISIBILITY_VERTEX
            },
        });
    }
    let twin_desc = D3D12_VERSIONED_ROOT_SIGNATURE_DESC {
        Version: D3D_ROOT_SIGNATURE_VERSION_1_1,
        Anonymous: D3D12_VERSIONED_ROOT_SIGNATURE_DESC_0 {
            Desc_1_1: D3D12_ROOT_SIGNATURE_DESC1 {
                NumParameters: params.len() as u32,
                pParameters: params.as_ptr(),
                NumStaticSamplers: d.NumStaticSamplers,
                pStaticSamplers: d.pStaticSamplers,
                Flags: d.Flags,
            },
        },
    };
    let (mut serialized, mut failure): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    if D3D12SerializeVersionedRootSignature(&twin_desc, &mut serialized, Some(&mut failure))
        .is_err()
    {
        error("twin root: serialization failed");
        return;
    }
    let Some(serialized) = serialized else {
        error("twin root: serialization failed");
        return;
    };
    let blob = core::slice::from_raw_parts(
        serialized.GetBufferPointer() as *const u8,
        serialized.GetBufferSize(),
    );
    let Ok(twin) = device.CreateRootSignature::<ID3D12RootSignature>(0, blob) else {
        error("twin root: creation failed");
        return;
    };
    let mut roots = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let twin_raw = twin.as_raw() as usize;
    roots.insert(root as usize, twin);
    count(0).store(roots.len() as u64, Relaxed);
    ROOT_COUNT.store(roots.len() as u32, Release);
    crate::single_pass::cvr_single_pass_activate();
    log(&format!(
        "[single-pass] material root {:016X}: twin {:016X} (38 tables)\n",
        root as usize, twin_raw
    ));
}

// The last roots each thread asked for (root, twin, the root count when looked up): the draw hook asks once or twice
// per draw, from every render worker, and the map's lock was 1.2 ms a frame (ETW stacks). Twins are only ever added,
// so a twin found stays right; "no twin" holds while the count is the same.
thread_local! {
    static TWIN_CACHE: core::cell::Cell<[(usize, usize, u32); 4]> = const { core::cell::Cell::new([(0, 0, 0); 4]) };
    static TWIN_NEXT: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// TwinRoot: the twin of a material root, else null.
#[no_mangle]
pub extern "C" fn cvr_single_pass_twin_root(root: *mut c_void) -> *mut c_void {
    let n = ROOT_COUNT.load(Acquire);
    if root.is_null() || n == 0 {
        return core::ptr::null_mut();
    }
    let key = root as usize;
    let cache = TWIN_CACHE.get();
    if let Some(&(_, twin, _)) = cache.iter().find(|e| e.0 == key && (e.1 != 0 || e.2 == n)) {
        return twin as *mut c_void;
    }
    let twin = {
        let roots = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
        roots.get(&key).map_or(0, |t| t.as_raw() as usize)
    };
    let mut cache = cache;
    let slot = match cache.iter().position(|e| e.0 == key) {
        Some(i) => i,
        None => {
            let i = TWIN_NEXT.get();
            TWIN_NEXT.set((i + 1) % 4);
            i
        }
    };
    cache[slot] = (key, twin, n);
    TWIN_CACHE.set(cache);
    twin as *mut c_void
}

// ---- shader twins (memory and disk)

struct ShaderTwin {
    ok: bool,
    unchanged: bool,
    bytes: Vec<u8>,
}
#[derive(Default)]
struct Shaders {
    shaders: FastMap<u64, Arc<ShaderTwin>>,
    /// Pixel shaders whose twin depends on the vertex shader.
    linked: FastMap<u64, bool>,
    /// Shaders a worker is building now (the others wait for it).
    building: FastSet<u64>,
}
static SHADERS: LazyLock<Mutex<Shaders>> = LazyLock::new(|| Mutex::new(Shaders::default()));
static BUILT: Condvar = Condvar::new();

/// The plugin's folder with its trailing separator, as the C++ half's Dir(Root).
fn root_dir() -> String {
    let mut s = cvr_host::paths::dir(cvr_host::paths::Folder::Root)
        .to_string_lossy()
        .into_owned();
    if !s.is_empty() && !s.ends_with('\\') {
        s.push('\\');
    }
    s
}
fn cache_dir() -> &'static str {
    static DIR: OnceLock<String> = OnceLock::new();
    DIR.get_or_init(|| {
        let d = format!("{}cache\\view_twins\\", root_dir());
        let _ = std::fs::create_dir_all(&d);
        d
    })
}
/// A twin's files: <hash>.r<recipe>.<ext>, or per vertex shader <hash>_<vertex hash>.r<recipe>.<ext>.
fn base(hash: u64, vertex: u64) -> String {
    if vertex != 0 {
        format!("{}{hash:016x}_{vertex:016x}.r{TWIN_RECIPE}.", cache_dir())
    } else {
        format!("{}{hash:016x}.r{TWIN_RECIPE}.", cache_dir())
    }
}
fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}
/// A twin from the disk cache: "same" (needs no rewrite), "fail", or the signed "dxil".
fn load(base: &str) -> Option<Arc<ShaderTwin>> {
    let twin = if exists(&format!("{base}same")) {
        ShaderTwin {
            ok: true,
            unchanged: true,
            bytes: Vec::new(),
        }
    } else if exists(&format!("{base}fail")) {
        ShaderTwin {
            ok: false,
            unchanged: false,
            bytes: Vec::new(),
        }
    } else {
        match std::fs::read(format!("{base}dxil")) {
            Ok(bytes) if bytes.len() > 32 => ShaderTwin {
                ok: true,
                unchanged: false,
                bytes,
            },
            _ => return None,
        }
    };
    count(8).fetch_add(1, Relaxed);
    Some(Arc::new(twin))
}
/// A shader's twin from memory, the disk cache or the compiler. A pixel shader whose twin depends on its vertex shader
/// (inputs completed) has a marker <hash>.link and its twins per vertex shader.
unsafe fn build_shader(
    compiler: &Compiler,
    code: &D3D12_SHADER_BYTECODE,
    vertex: Option<&D3D12_SHADER_BYTECODE>,
) -> Arc<ShaderTwin> {
    let hash = fnv(bytes_of(code));
    let vertex_hash = vertex.map_or(0, |v| fnv(bytes_of(v)));
    let pair_key = hash
        ^ vertex_hash
            .wrapping_mul(0x9E3779B97F4A7C15)
            .wrapping_add(0x632BE59BD9B4E019);
    let mut linked;
    {
        let mut s = SHADERS.lock().unwrap_or_else(|e| e.into_inner());
        while s.building.contains(&hash) {
            s = BUILT.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        linked = *s
            .linked
            .entry(hash)
            .or_insert_with(|| exists(&format!("{}link", base(hash, 0))));
        let key = if linked && vertex.is_some() {
            pair_key
        } else {
            hash
        };
        if let Some(t) = s.shaders.get(&key) {
            return t.clone();
        }
        s.building.insert(hash);
    }
    struct Done(u64);
    impl Drop for Done {
        fn drop(&mut self) {
            SHADERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .building
                .remove(&self.0);
            BUILT.notify_all();
        }
    }
    let _done = Done(hash);
    let mut twin = if linked && vertex.is_some() {
        load(&base(hash, vertex_hash))
    } else {
        load(&base(hash, 0))
    };
    if twin.is_none() {
        let result = compiler.build(bytes_of(code), vertex.map_or(&[][..], |v| bytes_of(v)));
        if result.linked && vertex.is_some() && !linked {
            // From now on per vertex shader.
            linked = true;
            let _ = std::fs::File::create(format!("{}link", base(hash, 0)));
            SHADERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .linked
                .insert(hash, true);
        }
        let at = base(
            hash,
            if linked && vertex.is_some() {
                vertex_hash
            } else {
                0
            },
        );
        let built = if result.ok && result.unchanged {
            count(6).fetch_add(1, Relaxed);
            let _ = std::fs::File::create(format!("{at}same"));
            ShaderTwin {
                ok: true,
                unchanged: true,
                bytes: Vec::new(),
            }
        } else if result.ok {
            count(5).fetch_add(1, Relaxed);
            let _ = std::fs::write(format!("{at}dxil"), &result.container);
            ShaderTwin {
                ok: true,
                unchanged: false,
                bytes: result.container,
            }
        } else {
            count(7).fetch_add(1, Relaxed);
            let _ = std::fs::write(format!("{at}fail"), &result.error);
            let shown = &result.error[..result.error.len().min(140)];
            error(&format!(
                "shader {hash:016x}: {}",
                String::from_utf8_lossy(shown)
            ));
            ShaderTwin {
                ok: false,
                unchanged: false,
                bytes: Vec::new(),
            }
        };
        twin = Some(Arc::new(built));
    }
    let key = if linked && vertex.is_some() {
        pair_key
    } else {
        hash
    };
    let mut s = SHADERS.lock().unwrap_or_else(|e| e.into_inner());
    s.shaders
        .entry(key)
        .or_insert_with(|| {
            twin.unwrap_or_else(|| {
                Arc::new(ShaderTwin {
                    ok: false,
                    unchanged: false,
                    bytes: Vec::new(),
                })
            })
        })
        .clone()
}

// ---- pipelines

/// The device's original CreatePipelineState (the stream form).
pub(crate) type CreateStreamFn = unsafe extern "system" fn(
    *mut c_void,
    *const D3D12_PIPELINE_STATE_STREAM_DESC,
    *const GUID,
    *mut *mut c_void,
) -> HRESULT;
const PIPELINE_KEY: GUID = GUID::from_values(
    0x5a1e9b37,
    0x2c4d,
    0x4f61,
    [0x9e, 0x0a, 0x31, 0x7c, 0x55, 0x8b, 0x02, 0xd4],
);

struct Pipeline {
    device: usize,
    create: CreateStreamFn,
    root: usize,
    /// The owned description: a stream (graphics descriptions are converted at registration).
    stream: OwnedStream,
    /// Keeps the pointed-at parts of a converted graphics description.
    graphics: Option<OwnedGraphics>,
    converted: Vec<u8>,
    vs: D3D12_SHADER_BYTECODE,
    ps: D3D12_SHADER_BYTECODE,
    /// One copy of each shader for all its pipelines.
    _vs_blob: Option<Arc<Vec<u8>>>,
    _ps_blob: Option<Arc<Vec<u8>>>,
    /// Vertex buffer slots the input layout reads.
    slots: u32,
    /// [0] the engine's shaders, [1] their hidden-area replacements: 0 idle, 1 queued, 2 ready, 3 failed.
    state: [AtomicI32; 2],
    /// Whether a hidden-area twin replaces a stage (asked once, not per draw).
    replaced: AtomicI32,
    twin: [OnceLock<ID3D12PipelineState>; 2],
}
impl Pipeline {
    fn desc(&self) -> D3D12_PIPELINE_STATE_STREAM_DESC {
        let bytes = if self.converted.is_empty() {
            &self.stream.stream
        } else {
            &self.converted
        };
        D3D12_PIPELINE_STATE_STREAM_DESC {
            SizeInBytes: bytes.len(),
            pPipelineStateSubobjectStream: bytes.as_ptr() as *mut c_void,
        }
    }
}
/// The record as a COM object on the PSO (SetPrivateDataInterface): released with it.
#[repr(C)]
struct PipelineCom {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    p: Pipeline,
}
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn pipeline_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return windows::Win32::Foundation::E_POINTER;
    }
    *out = core::ptr::null_mut();
    if iid.is_null() || *iid != IID_IUNKNOWN {
        return windows::Win32::Foundation::E_NOINTERFACE;
    }
    *out = this;
    pipeline_add_ref(this);
    HRESULT(0)
}
unsafe extern "system" fn pipeline_add_ref(this: *mut c_void) -> u32 {
    (*(this as *const PipelineCom)).refs.fetch_add(1, Relaxed) + 1
}
unsafe extern "system" fn pipeline_release(this: *mut c_void) -> u32 {
    let n = (*(this as *const PipelineCom)).refs.fetch_sub(1, AcqRel) - 1;
    if n == 0 {
        drop(Box::from_raw(this as *mut PipelineCom));
    }
    n
}
static PIPELINE_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: pipeline_query,
    AddRef: pipeline_add_ref,
    Release: pipeline_release,
};
// SAFETY: the vtable holds plain function pointers; the record's mutable parts are atomics and once-cells.
unsafe impl Sync for PipelineCom {}
unsafe impl Send for PipelineCom {}

/// A reference to a record (AddRef on clone, Release on drop).
struct Held(*mut PipelineCom);
// SAFETY: the record is free-threaded (see PipelineCom).
unsafe impl Send for Held {}
impl Held {
    unsafe fn new(p: *mut PipelineCom) -> Held {
        pipeline_add_ref(p as *mut c_void);
        Held(p)
    }
    fn get(&self) -> &Pipeline {
        // SAFETY: a held reference keeps the record alive.
        unsafe { &(*self.0).p }
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: the reference this object holds.
        unsafe { pipeline_release(self.0 as *mut c_void) };
    }
}

// ---- the workers

struct Job {
    pipeline: Held,
    variant: usize,
}
static JOBS: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());
static JOB_SIGNAL: Condvar = Condvar::new();
static WORKERS: Once = Once::new();
/// 0 untried, 1 ready, 2 unavailable.
static COMPILER_STATE: AtomicI32 = AtomicI32::new(0);

unsafe fn build_pipeline(compiler: &Compiler, p: &Pipeline, variant: usize) {
    let t0 = Instant::now();
    let vs = if variant != 0 {
        bound_code(&p.vs)
    } else {
        p.vs
    };
    let ps_code = if p.ps.BytecodeLength != 0 {
        if variant != 0 {
            bound_code(&p.ps)
        } else {
            p.ps
        }
    } else {
        D3D12_SHADER_BYTECODE::default()
    };
    let vs_twin = build_shader(compiler, &vs, None);
    let ps_twin =
        (ps_code.BytecodeLength != 0).then(|| build_shader(compiler, &ps_code, Some(&vs)));
    let twin_root = cvr_single_pass_twin_root(p.root as *mut c_void);
    let mut result = 3;
    if vs_twin.ok && ps_twin.as_ref().is_none_or(|t| t.ok) && !twin_root.is_null() {
        let v = if vs_twin.unchanged {
            vs
        } else {
            code_of(&vs_twin.bytes)
        };
        let f = match &ps_twin {
            None => D3D12_SHADER_BYTECODE::default(),
            Some(t) if t.unchanged => ps_code,
            Some(t) => code_of(&t.bytes),
        };
        // View 0 -> viewport 0, view 1 -> viewport 1.
        let layout = [
            D3D12_VIEW_INSTANCE_LOCATION {
                ViewportArrayIndex: 0,
                RenderTargetArrayIndex: 0,
            },
            D3D12_VIEW_INSTANCE_LOCATION {
                ViewportArrayIndex: 1,
                RenderTargetArrayIndex: 0,
            },
        ];
        let desc = p.desc();
        if let Some(bytes) =
            ps::build_view_instanced(&desc, &v, layout.as_ptr(), false, &f, twin_root)
        {
            let stream = D3D12_PIPELINE_STATE_STREAM_DESC {
                SizeInBytes: bytes.len(),
                pPipelineStateSubobjectStream: bytes.as_ptr() as *mut c_void,
            };
            let mut created: *mut c_void = core::ptr::null_mut();
            let hr = (p.create)(
                p.device as *mut c_void,
                &stream,
                &ID3D12PipelineState::IID,
                &mut created,
            );
            if hr.is_ok() && !created.is_null() {
                let _ = p.twin[variant].set(ID3D12PipelineState::from_raw(created));
                result = 2;
            } else {
                error(&format!(
                    "view-instanced pipeline refused: 0x{:08X}",
                    hr.0 as u32
                ));
                probe_refusal(p, &vs, &ps_code, &v, &f, twin_root, &layout);
            }
        } else {
            error("pipeline stream could not take the views");
        }
    }
    count(if result == 2 { 3 } else { 4 }).fetch_add(1, Relaxed);
    count(10).fetch_add((t0.elapsed().as_secs_f64() * 1000.0 + 0.5) as u64, Relaxed);
    p.state[variant].store(result, Release);
}

/// Which part of a refused twin the driver objects to: the same pipeline with one change at a time (the first few
/// failures only).
unsafe fn probe_refusal(
    p: &Pipeline,
    vs: &D3D12_SHADER_BYTECODE,
    ps_code: &D3D12_SHADER_BYTECODE,
    v: &D3D12_SHADER_BYTECODE,
    f: &D3D12_SHADER_BYTECODE,
    twin_root: *mut c_void,
    layout: &[D3D12_VIEW_INSTANCE_LOCATION; 2],
) {
    static PROBES: AtomicI32 = AtomicI32::new(0);
    if PROBES.fetch_add(1, Relaxed) >= 6 {
        return;
    }
    let create = |stream: &[u8]| -> HRESULT {
        let sd = D3D12_PIPELINE_STATE_STREAM_DESC {
            SizeInBytes: stream.len(),
            pPipelineStateSubobjectStream: stream.as_ptr() as *mut c_void,
        };
        let mut t: *mut c_void = core::ptr::null_mut();
        let r = (p.create)(
            p.device as *mut c_void,
            &sd,
            &ID3D12PipelineState::IID,
            &mut t,
        );
        if !t.is_null() {
            drop(IUnknown::from_raw(t));
        }
        r
    };
    let abort = HRESULT(0x80004004u32 as i32);
    let desc = p.desc();
    let try_stream = |sv: &D3D12_SHADER_BYTECODE,
                      sp: &D3D12_SHADER_BYTECODE,
                      r: *mut c_void,
                      views: bool|
     -> HRESULT {
        let Some(vi) = ps::build_view_instanced(&desc, sv, layout.as_ptr(), false, sp, r) else {
            return abort;
        };
        if views {
            create(&vi)
        } else {
            // No view instancing part: the trailing views part dropped.
            create(
                &vi[..vi.len()
                    - core::mem::size_of::<*const c_void>()
                    - core::mem::size_of::<D3D12_VIEW_INSTANCING_DESC>()],
            )
        }
    };
    let original = try_stream(vs, ps_code, p.root as *mut c_void, false);
    let original_root2 = try_stream(vs, ps_code, twin_root, false);
    let twins_no_views = try_stream(v, f, twin_root, false);
    let original_views = try_stream(vs, ps_code, twin_root, true);
    let twin_vs_only = try_stream(v, ps_code, twin_root, true);
    let twin_ps_only = try_stream(vs, f, twin_root, true);
    log(&format!(
        "[single-pass]   probe: original {:08X}, twin root {:08X}, twin shaders no views {:08X}, original shaders + views {:08X}, twin VS only {:08X}, twin PS only {:08X}\n",
        original.0 as u32, original_root2.0 as u32, twins_no_views.0 as u32, original_views.0 as u32, twin_vs_only.0 as u32, twin_ps_only.0 as u32
    ));
    let mut described = String::from_utf8_lossy(&ps::describe(&desc)).into_owned();
    described.insert_str(0, "[single-pass]   desc:");
    described.push('\n');
    log(&described);
    // The views on one viewport for both (is viewport 1 the problem?).
    let same = [D3D12_VIEW_INSTANCE_LOCATION::default(); 2];
    let one_viewport =
        match ps::build_view_instanced(&desc, vs, same.as_ptr(), false, ps_code, twin_root) {
            Some(b1) => create(&b1),
            None => abort,
        };
    log(&format!(
        "[single-pass]   views on one viewport {:08X}\n",
        one_viewport.0 as u32
    ));
    // One part of the engine's description changed at a time, original shaders, two views.
    // Each variant logs its result; the probe itself only reports.
    let variant = |what: &str, edit: &dyn Fn(i32, *mut u8), drop_ps: bool| {
        let mut inputs: Vec<D3D12_INPUT_ELEMENT_DESC> = Vec::new();
        let edited = ps::edit(&desc, |t, value| {
            if t == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT.0 && inputs.is_empty() {
                // An own copy before edits.
                let mut d: D3D12_INPUT_LAYOUT_DESC =
                    core::ptr::read_unaligned(value as *const D3D12_INPUT_LAYOUT_DESC);
                if !d.pInputElementDescs.is_null() {
                    inputs =
                        core::slice::from_raw_parts(d.pInputElementDescs, d.NumElements as usize)
                            .to_vec();
                }
                d.pInputElementDescs = inputs.as_ptr();
                core::ptr::write_unaligned(value as *mut D3D12_INPUT_LAYOUT_DESC, d);
            }
            edit(t, value);
        });
        let Some(edited) = edited else { return };
        let ed = D3D12_PIPELINE_STATE_STREAM_DESC {
            SizeInBytes: edited.len(),
            pPipelineStateSubobjectStream: edited.as_ptr() as *mut c_void,
        };
        let none = D3D12_SHADER_BYTECODE::default();
        let Some(b) = ps::build_view_instanced(
            &ed,
            vs,
            layout.as_ptr(),
            false,
            if drop_ps { &none } else { ps_code },
            twin_root,
        ) else {
            return;
        };
        let r = create(&b);
        log(&format!(
            "[single-pass]   views, {what}: {:08X}\n",
            r.0 as u32
        ));
    };
    const PS: i32 = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS.0;
    const RT: i32 = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS.0;
    const IL: i32 = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT.0;
    const DS: i32 = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL.0;
    const DSF: i32 = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT.0;
    let put = |value: *mut u8, bytes: &[u8]| {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), value, bytes.len())
    };
    let zero_ps =
        |value: *mut u8| put(value, &[0u8; core::mem::size_of::<D3D12_SHADER_BYTECODE>()]);
    let zero_rt =
        |value: *mut u8| put(value, &[0u8; core::mem::size_of::<D3D12_RT_FORMAT_ARRAY>()]);
    variant(
        "no pixel shader, no targets",
        &|t, v| {
            if t == PS {
                zero_ps(v);
            }
            if t == RT {
                zero_rt(v);
            }
        },
        true,
    );
    variant(
        "every input per vertex",
        &|t, v| {
            if t != IL {
                return;
            }
            let d: D3D12_INPUT_LAYOUT_DESC =
                core::ptr::read_unaligned(v as *const D3D12_INPUT_LAYOUT_DESC);
            for i in 0..d.NumElements as usize {
                let e = d.pInputElementDescs.add(i) as *mut D3D12_INPUT_ELEMENT_DESC;
                (*e).InputSlotClass = D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA;
                (*e).InstanceDataStepRate = 0;
            }
        },
        false,
    );
    variant(
        "no stencil",
        &|t, v| {
            if t != DS {
                return;
            }
            let mut d: D3D12_DEPTH_STENCIL_DESC =
                core::ptr::read_unaligned(v as *const D3D12_DEPTH_STENCIL_DESC);
            d.StencilEnable = false.into();
            core::ptr::write_unaligned(v as *mut D3D12_DEPTH_STENCIL_DESC, d);
        },
        false,
    );
    variant(
        "no input layout",
        &|t, v| {
            if t == IL {
                put(v, &[0u8; core::mem::size_of::<D3D12_INPUT_LAYOUT_DESC>()]);
            }
        },
        false,
    );
    variant(
        "pixel shader, no targets",
        &|t, v| {
            if t == RT {
                zero_rt(v);
            }
        },
        false,
    );
    variant(
        "no pixel shader, targets kept",
        &|t, v| {
            if t == PS {
                zero_ps(v);
            }
        },
        true,
    );
    variant(
        "targets RGBA16F",
        &|t, v| {
            if t != RT {
                return;
            }
            let mut a: D3D12_RT_FORMAT_ARRAY =
                core::ptr::read_unaligned(v as *const D3D12_RT_FORMAT_ARRAY);
            for i in 0..(a.NumRenderTargets as usize).min(8) {
                a.RTFormats[i] = DXGI_FORMAT_R16G16B16A16_FLOAT;
            }
            core::ptr::write_unaligned(v as *mut D3D12_RT_FORMAT_ARRAY, a);
        },
        false,
    );
    variant(
        "no depth stencil",
        &|t, v| {
            if t == DS {
                put(v, &[0u8; core::mem::size_of::<D3D12_DEPTH_STENCIL_DESC>()]);
            }
            if t == DSF {
                put(v, &DXGI_FORMAT_UNKNOWN.0.to_ne_bytes());
            }
        },
        false,
    );
    variant(
        "one target",
        &|t, v| {
            if t != RT {
                return;
            }
            let mut a: D3D12_RT_FORMAT_ARRAY =
                core::ptr::read_unaligned(v as *const D3D12_RT_FORMAT_ARRAY);
            a.NumRenderTargets = a.NumRenderTargets.min(1);
            core::ptr::write_unaligned(v as *mut D3D12_RT_FORMAT_ARRAY, a);
        },
        false,
    );
    // The pixel shader's signatures, to compare with a vertex shader's outputs.
    static DUMPED: AtomicI32 = AtomicI32::new(0);
    if DUMPED.fetch_add(1, Relaxed) < 2 {
        let hv = fnv(bytes_of(vs));
        let hp = if ps_code.BytecodeLength != 0 {
            fnv(bytes_of(ps_code))
        } else {
            0
        };
        log(&format!(
            "[single-pass]   shaders: vs {hv:016x} ps {hp:016x}\n"
        ));
        let save = |h: u64, c: &D3D12_SHADER_BYTECODE| {
            let _ = std::fs::write(format!("{}refused_{h:016x}.dxil", cache_dir()), bytes_of(c));
        };
        save(hv, vs);
        if hp != 0 {
            save(hp, ps_code);
        }
    }
}

fn worker() {
    // SAFETY: this thread's own handle.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        let _ = SetThreadDescription(GetCurrentThread(), w!("cvr single-pass twins"));
    }
    let mut compiler = Compiler::default();
    let folder: Vec<u16> = format!("{}dxc\\", root_dir()).encode_utf16().collect();
    let ready = compiler.load(&folder);
    COMPILER_STATE.store(if ready { 1 } else { 2 }, Release);
    if !ready {
        let (mut data, mut len): (*const u8, usize) = (core::ptr::null(), 0);
        // SAFETY: the compiler just loaded; its error lives with it.
        let reason = unsafe {
            view_twin_compiler::cvr_view_twin_compiler_error(&compiler, &mut data, &mut len);
            if data.is_null() {
                String::new()
            } else {
                String::from_utf8_lossy(core::slice::from_raw_parts(data, len)).into_owned()
            }
        };
        error(&format!("compiler: {reason}"));
    } else {
        log(&format!(
            "[single-pass] DXC loaded; twins cached in {}\n",
            cache_dir()
        ));
    }
    loop {
        let job = {
            let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(job) = jobs.pop_front() {
                    break job;
                }
                jobs = JOB_SIGNAL.wait(jobs).unwrap_or_else(|e| e.into_inner());
            }
        };
        if !ready {
            job.pipeline.get().state[job.variant].store(3, Release);
            count(4).fetch_add(1, Relaxed);
            continue;
        }
        // SAFETY: the held record.
        unsafe { build_pipeline(&compiler, job.pipeline.get(), job.variant) };
    }
}
unsafe fn queue(p: *mut PipelineCom, variant: usize) {
    // Three builders, below normal priority.
    WORKERS.call_once(|| {
        for _ in 0..3 {
            std::thread::spawn(worker);
        }
    });
    count(2).fetch_add(1, Relaxed);
    JOBS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back(Job {
            pipeline: Held::new(p),
            variant,
        });
    JOB_SIGNAL.notify_one();
}

/// Available: the compiler not known missing and a material root seen.
#[no_mangle]
pub extern "C" fn cvr_single_pass_available() -> bool {
    COMPILER_STATE.load(Acquire) != 2 && ROOT_COUNT.load(Acquire) != 0
}

// ---- registration

/// The bytecode the owned descriptions point at, one copy per shader (a DXBC container is keyed by its own 16-byte
/// hash and its size; ~4,800 pipelines share a few hundred shaders).
static BLOBS: LazyLock<Mutex<FastMap<Vec<u8>, Weak<Vec<u8>>>>> =
    LazyLock::new(|| Mutex::new(cvr_host::fast::fast_map()));
unsafe fn shared(code: &D3D12_SHADER_BYTECODE) -> Option<Arc<Vec<u8>>> {
    let b = bytes_of(code);
    if b.is_empty() {
        return None;
    }
    let mut key: Vec<u8> = if b.len() >= 20 && &b[..4] == b"DXBC" {
        b[4..20].to_vec()
    } else {
        fnv(b).to_string().into_bytes()
    };
    key.extend_from_slice(b.len().to_string().as_bytes());
    let mut blobs = BLOBS.lock().unwrap_or_else(|e| e.into_inner());
    let slot = blobs.entry(key).or_default();
    if let Some(found) = slot.upgrade() {
        return Some(found);
    }
    let made = Arc::new(b.to_vec());
    *slot = Arc::downgrade(&made);
    count(9).fetch_add(b.len() as u64, Relaxed);
    Some(made)
}
unsafe fn register(pso: &ID3D12PipelineState, p: Pipeline) {
    let bytes = (p.stream.stream.len() + p.converted.len()) as u64;
    let raw = Box::into_raw(Box::new(PipelineCom {
        vtbl: &PIPELINE_VTBL,
        refs: AtomicU32::new(1),
        p,
    }));
    // One reference, released at the end of this function: the PSO keeps its own.
    let record = IUnknown::from_raw(raw as *mut c_void);
    if pso.SetPrivateDataInterface(&PIPELINE_KEY, &record).is_err() {
        return;
    }
    count(1).fetch_add(1, Relaxed);
    count(9).fetch_add(bytes, Relaxed);
}
fn new_pipeline(device: *mut c_void, create: CreateStreamFn, root: usize) -> Pipeline {
    Pipeline {
        device: device as usize,
        create,
        root,
        stream: OwnedStream::default(),
        graphics: None,
        converted: Vec::new(),
        vs: D3D12_SHADER_BYTECODE::default(),
        ps: D3D12_SHADER_BYTECODE::default(),
        _vs_blob: None,
        _ps_blob: None,
        slots: 0,
        state: [AtomicI32::new(0), AtomicI32::new(0)],
        replaced: AtomicI32::new(-1),
        twin: [OnceLock::new(), OnceLock::new()],
    }
}
unsafe fn input_slots(input: &D3D12_INPUT_LAYOUT_DESC) -> u32 {
    let mut slots = 0u32;
    if !input.pInputElementDescs.is_null() {
        for i in 0..input.NumElements as usize {
            slots |= 1u32 << ((*input.pInputElementDescs.add(i)).InputSlot & 31);
        }
    }
    slots
}

/// GraphicsCreated: a classic description's material pipeline registered (converted to an owned stream).
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_graphics_created(
    device: *mut c_void,
    desc: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    pso: *mut c_void,
    create: Option<CreateStreamFn>,
) {
    let (Some(desc), Some(pso), Some(create)) = (
        desc.as_ref(),
        ID3D12PipelineState::from_raw_borrowed(&pso),
        create,
    ) else {
        return;
    };
    let root: *mut c_void = core::mem::transmute_copy(&desc.pRootSignature);
    if cvr_single_pass_twin_root(root).is_null()
        || desc.VS.BytecodeLength == 0
        || desc.HS.BytecodeLength != 0
        || desc.DS.BytecodeLength != 0
        || desc.GS.BytecodeLength != 0
        || desc.StreamOutput.NumEntries != 0
    {
        return;
    }
    let mut p = new_pipeline(device, create, root as usize);
    let mut graphics = ps::own_graphics(desc);
    let vs_blob = shared(&desc.VS);
    let ps_blob = shared(&desc.PS);
    graphics.desc.VS = vs_blob
        .as_ref()
        .map_or(D3D12_SHADER_BYTECODE::default(), |b| code_of(b));
    graphics.desc.PS = ps_blob
        .as_ref()
        .map_or(D3D12_SHADER_BYTECODE::default(), |b| code_of(b));
    graphics.parts.shaders = Vec::new();
    if graphics.desc.VS.pShaderBytecode.is_null()
        || graphics.desc.VS.BytecodeLength == 0
        || graphics.desc.NumRenderTargets > 8
    {
        return;
    }
    p.converted = ps::graphics_stream(&graphics.desc);
    p.vs = graphics.desc.VS;
    p.ps = graphics.desc.PS;
    p._vs_blob = vs_blob;
    p._ps_blob = ps_blob;
    p.slots = input_slots(&desc.InputLayout);
    p.graphics = Some(graphics);
    register(pso, p);
}

/// StreamCreated: a stream's material pipeline registered.
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_stream_created(
    device: *mut c_void,
    desc: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    pso: *mut c_void,
    create: Option<CreateStreamFn>,
) {
    let (Some(desc), Some(pso), Some(create)) = (
        desc.as_ref(),
        ID3D12PipelineState::from_raw_borrowed(&pso),
        create,
    ) else {
        return;
    };
    if ROOT_COUNT.load(Acquire) == 0 {
        return;
    }
    let Some(info) = ps::read(desc) else { return };
    if cvr_single_pass_twin_root(info.root).is_null()
        || info.vs.BytecodeLength == 0
        || info.other_stages
        || info.view_instancing
        || info.stream_output
    {
        return;
    }
    let mut p = new_pipeline(device, create, info.root as usize);
    let Some(mut stream) = ps::own_stream(desc) else {
        return;
    };
    let base = stream.stream.as_mut_ptr();
    let PipelineStreamOffsets {
        vs: vs_at,
        ps: ps_at,
        ..
    } = stream.offsets;
    if vs_at == usize::MAX {
        return;
    }
    let vs: D3D12_SHADER_BYTECODE =
        core::ptr::read_unaligned(base.add(vs_at) as *const D3D12_SHADER_BYTECODE);
    let vs_blob = shared(&vs);
    p.vs = vs_blob
        .as_ref()
        .map_or(D3D12_SHADER_BYTECODE::default(), |b| code_of(b));
    core::ptr::write_unaligned(base.add(vs_at) as *mut D3D12_SHADER_BYTECODE, p.vs);
    p._vs_blob = vs_blob;
    if ps_at != usize::MAX {
        let mut ps_code: D3D12_SHADER_BYTECODE =
            core::ptr::read_unaligned(base.add(ps_at) as *const D3D12_SHADER_BYTECODE);
        if ps_code.BytecodeLength != 0 {
            let ps_blob = shared(&ps_code);
            ps_code = ps_blob
                .as_ref()
                .map_or(D3D12_SHADER_BYTECODE::default(), |b| code_of(b));
            core::ptr::write_unaligned(base.add(ps_at) as *mut D3D12_SHADER_BYTECODE, ps_code);
            p._ps_blob = ps_blob;
        }
    }
    stream.parts.shaders = Vec::new();
    p.slots = input_slots(&info.input);
    if ps_at != usize::MAX {
        p.ps = core::ptr::read_unaligned(base.add(ps_at) as *const D3D12_SHADER_BYTECODE);
    }
    p.stream = stream;
    register(pso, p);
}

// ---- the draws' side

/// Find: a material pipeline's record (it lives as long as the PSO), else null.
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_find(pso: *mut c_void) -> *mut c_void {
    let Some(pso) = ID3D12PipelineState::from_raw_borrowed(&pso) else {
        return core::ptr::null_mut();
    };
    if ROOT_COUNT.load(Relaxed) == 0 {
        return core::ptr::null_mut();
    }
    let mut p: *mut c_void = core::ptr::null_mut();
    let mut size = core::mem::size_of::<*mut c_void>() as u32;
    if pso
        .GetPrivateData(
            &PIPELINE_KEY,
            &mut size,
            Some(&mut p as *mut _ as *mut c_void),
        )
        .is_err()
        || size as usize != core::mem::size_of::<*mut c_void>()
        || p.is_null()
    {
        return core::ptr::null_mut();
    }
    // GetPrivateData added a reference; the PSO keeps the record alive.
    pipeline_release(p);
    p
}
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_root_of(p: *const c_void) -> *mut c_void {
    (p as *const PipelineCom)
        .as_ref()
        .map_or(core::ptr::null_mut(), |c| c.p.root as *mut c_void)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_input_slots(p: *const c_void) -> u32 {
    (p as *const PipelineCom)
        .as_ref()
        .map_or(!0u32, |c| c.p.slots)
}
/// Twin: the view-instanced twin for a draw bound with `bound` (the engine's pipeline or its hidden-area replacement):
/// null until built -- the first call queues the build -- or when it cannot be built.
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_twin(
    p: *mut c_void,
    engine: *mut c_void,
    bound: *mut c_void,
) -> *mut c_void {
    let raw = p as *mut PipelineCom;
    let Some(record) = raw.as_ref() else {
        return core::ptr::null_mut();
    };
    let p = &record.p;
    let mut variant = 0usize;
    if bound != engine {
        // Bound with a replacement: only a hidden-area twin is known here (a sight twin is not).
        let mut replaced = p.replaced.load(Relaxed);
        if replaced < 0 {
            replaced = (bound_code(&p.vs).pShaderBytecode != p.vs.pShaderBytecode
                || (p.ps.BytecodeLength != 0
                    && bound_code(&p.ps).pShaderBytecode != p.ps.pShaderBytecode))
                as i32;
            p.replaced.store(replaced, Relaxed);
        }
        if replaced == 0 {
            count(11).fetch_add(1, Relaxed);
            return core::ptr::null_mut();
        }
        variant = 1;
    }
    let s = p.state[variant].load(Acquire);
    if s == 2 {
        return p.twin[variant]
            .get()
            .map_or(core::ptr::null_mut(), |t| t.as_raw());
    }
    if s == 0
        && COMPILER_STATE.load(Relaxed) != 2
        && p.state[variant]
            .compare_exchange(0, 1, AcqRel, Acquire)
            .is_ok()
    {
        queue(raw, variant);
    }
    core::ptr::null_mut()
}
