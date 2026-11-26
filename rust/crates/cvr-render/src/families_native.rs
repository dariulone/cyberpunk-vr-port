//! THE STEREO FAMILIES' REGISTRY (formerly src/Render/NativeStereoFamilies.cpp's): the development package's assets
//! (the manifest read through the .cpp's jsoncpp, kept here once), a root signature's identity (its blob, node and
//! digest as the root's private data), an engine pipeline -- classic or stream -- observed (its native depth and native
//! geometry status words) and, when its shaders are a family's, the family registered: its paired root (shared per root
//! and pair key while a family holds it), its view-instanced pipelines made, its record published and the family kept
//! as the pipeline's private data; a list's bound pipeline and root, the bound family and its draws counted.
//!
//! The C++ half keeps a MIRROR of each family and paired root (families::Family, RootVariant) for its consumers, made
//! through its host when the family is registered; families::Lookup and families::Bound hand out the mirror.
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_NOINTERFACE, E_POINTER, S_OK};
/// E_PENDING.
const E_PENDING: HRESULT = HRESULT(0x8000000Au32 as i32);
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Performance::QueryPerformanceCounter;

use crate::native_state_exports::*;
use crate::stereo_family_contracts::{PairedBinding, PassContract, ResourceUse};
use crate::stereo_family_manifest::{FamilyBytes, FamilyManifestView};

/// A byte string lent to the C++ mirror.
#[repr(C)]
pub struct Bytes {
    pub p: *const u8,
    pub n: usize,
}
fn lend(s: &str) -> Bytes {
    Bytes { p: s.as_ptr(), n: s.len() }
}

/// families::Record.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Record {
    pub id: u64,
    pub original: u64,
    pub root: u64,
    pub paired_root: u64,
    pub pipeline: u64,
    pub draws: [u64; 2],
    pub asset: u32,
    pub pairs: u32,
    pub uses: u32,
    pub result: i32,
}
const _: () = assert!(core::mem::size_of::<Record>() == 72);
const CAPACITY: u32 = 4096;

#[allow(non_upper_case_globals)]
extern "C" {
    /// The .cpp's records (its Record type).
    static mut CyberpunkVR_StereoFamilyRecords: [Record; 4096];
    /// include/Render/ObservationProfile.hpp's (C++ inline variables).
    static CyberpunkVR_ObservationProfileEnable: AtomicU32;
    static CyberpunkVR_ObservationProfileStats: [AtomicU64; 16];
}

// ---- the C++ half's host

/// The paired root a RootVariant mirror is made of.
#[repr(C)]
pub struct VariantView {
    pub root: *mut c_void,
    pub serialized: *const u8,
    pub serialized_length: usize,
    pub bindings: *const crate::paired_root::Location,
    pub binding_count: usize,
    /// parameters[2], descriptors[2], originalParameters, rootDwords.
    pub scalars: [u32; 6],
}
/// The family a Family mirror is made of.
#[repr(C)]
pub struct FamilyMirror {
    pub id: u64,
    pub asset: u32,
    pub record: u32,
    pub root_sha: Bytes,
    pub vertex_sha: Bytes,
    pub pixel_sha: Bytes,
    pub source_root: *mut c_void,
    pub pipeline: *mut c_void,
    pub native_camera_pipeline: *mut c_void,
    pub native_camera_stages: u32,
    pub skinning_read_contract: u32,
    pub alpha_depth_contract: u32,
    pub instance_rebase_safe: bool,
    pub instance_ordinal_independent: bool,
    pub uses: *const ResourceUse,
    pub use_count: usize,
    pub pass: PassContract,
}
const _: () = assert!(core::mem::size_of::<VariantView>() == 64 && core::mem::size_of::<FamilyMirror>() == 232);
#[repr(C)]
pub struct Host {
    /// Load(path): the manifest read through jsoncpp and loaded by stereo_family_manifest.rs -- its handle (kept for the
    /// process) and view, or null with the failure's text (NUL-terminated, at most `capacity` bytes).
    pub load: unsafe extern "C" fn(*const u16, usize, *mut *const FamilyManifestView, *mut u8, usize) -> *mut c_void,
    /// A RootVariant mirror (a heap std::shared_ptr<const RootVariant>), null when it could not be made. None (the
    /// plugin: no C++ consumer reads the mirrors) makes none -- the C++ mirrors are the Rust test variants'.
    pub variant: Option<unsafe extern "C" fn(*const VariantView) -> *mut c_void>,
    /// A Family mirror (a heap std::shared_ptr<const Family>) holding a copy of the variant mirror; null when it could not.
    pub family: Option<unsafe extern "C" fn(*const FamilyMirror, *const c_void) -> *mut c_void>,
    /// Frees a mirror: 0 a variant's, 1 a family's.
    pub free: Option<unsafe extern "C" fn(u32, *mut c_void)>,
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(null_mut());
/// A made-without-mirror marker (never dereferenced): the host lends no mirror functions.
const NO_MIRROR: *mut c_void = usize::MAX as *mut c_void;
/// # Safety
/// `host` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_attach(host: *const Host) {
    HOST.store(host.cast_mut(), Release);
}
fn host() -> Option<&'static Host> {
    // SAFETY: attached with static storage by the .cpp's initializer.
    unsafe { HOST.load(Acquire).as_ref() }
}

// ---- the private data keys and the owners

const ROOT_KEY: GUID = GUID::from_values(0x83b20c11, 0x913f, 0x4406, [0xa4, 0xa2, 0xfa, 0x7d, 0x8b, 0x98, 0x54, 0x60]);
const FAMILY_KEY: GUID = GUID::from_values(0x721b72f4, 0xda5d, 0x4562, [0x96, 0x3a, 0x23, 0xd9, 0x9b, 0x25, 0xb6, 0x13]);
const BOUND_FAMILY_KEY: GUID = GUID::from_values(0x753cd6f2, 0xa2af, 0x44f8, [0x96, 0x0b, 0x73, 0x06, 0x88, 0x0b, 0x79, 0x4c]);
const BOUND_ROOT_KEY: GUID = GUID::from_values(0x47b7d283, 0x9884, 0x46f8, [0x99, 0x4d, 0xe2, 0x4e, 0x3e, 0x8e, 0xa0, 0x43]);
const NATIVE_DEPTH_KEY: GUID = GUID::from_values(0x0db3e624, 0x37df, 0x4491, [0xa6, 0x91, 0xa1, 0x1e, 0x6d, 0x2d, 0x5e, 0x50]);
const NATIVE_GEOMETRY_KEY: GUID = GUID::from_values(0x0db3e624, 0x37df, 0x4491, [0xa6, 0x91, 0xa1, 0x1e, 0x6d, 0x2d, 0x5e, 0x51]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

/// A root signature's identity (RootData).
struct RootData {
    node: u32,
    bytes: Vec<u8>,
    digest: String,
    variants: Mutex<BTreeMap<Vec<u32>, Weak<RootVariant>>>,
}
/// A paired root and its C++ mirror.
pub struct RootVariant {
    pub root: ID3D12RootSignature,
    pub plan: crate::paired_root::Plan,
    mirror: *mut c_void,
}
// SAFETY: the root is a free-threaded D3D12 object; the mirror is only copied (a C++ shared_ptr copy is thread-safe).
unsafe impl Send for RootVariant {}
// SAFETY: as above.
unsafe impl Sync for RootVariant {}
impl Drop for RootVariant {
    fn drop(&mut self) {
        if let (Some(free), false) = (host().and_then(|h| h.free), self.mirror.is_null()) {
            // SAFETY: the variant's own mirror, freed once.
            unsafe { free(0, self.mirror) };
        }
    }
}
/// A registered family and its C++ mirror.
pub struct Family {
    pub id: u64,
    pub asset: u32,
    pub record: u32,
    pub root_sha: String,
    pub vertex_sha: String,
    pub pixel_sha: String,
    pub source_root: ID3D12RootSignature,
    pub paired_root: Arc<RootVariant>,
    pub pipeline: ID3D12PipelineState,
    pub native_camera_pipeline: Option<ID3D12PipelineState>,
    pub native_camera_stages: u32,
    pub skinning_read_contract: u32,
    pub alpha_depth_contract: u32,
    pub uses: Vec<ResourceUse>,
    pub instance_rebase_safe: bool,
    pub instance_ordinal_independent: bool,
    pub pass: PassContract,
    mirror: *mut c_void,
}
// SAFETY: as RootVariant's.
unsafe impl Send for Family {}
// SAFETY: as above.
unsafe impl Sync for Family {}
impl Drop for Family {
    fn drop(&mut self) {
        if let (Some(free), false) = (host().and_then(|h| h.free), self.mirror.is_null()) {
            // SAFETY: the family's own mirror, freed once.
            unsafe { free(1, self.mirror) };
        }
    }
}
enum OwnerData {
    Root(RootData),
    Family(Arc<Family>),
}
/// Owner<T>: the COM object the private data keeps (it answers IUnknown only).
#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    data: OwnerData,
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: owner_query, AddRef: owner_add_ref, Release: owner_release };
unsafe extern "system" fn owner_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` a live Owner.
    unsafe {
        *out = null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        owner_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn owner_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Owner.
    unsafe { (*(this as *const Owner)).refs.fetch_add(1, Relaxed) + 1 }
}
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    // SAFETY: a live Owner; the last release frees it.
    unsafe {
        let n = (*(this as *const Owner)).refs.fetch_sub(1, AcqRel) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Owner));
        }
        n
    }
}
fn owner(data: OwnerData) -> IUnknown {
    let boxed = Box::new(Owner { vtbl: &OWNER_VTBL, refs: AtomicU32::new(1), data });
    // SAFETY: a fresh COM object with its one reference.
    unsafe { IUnknown::from_raw(Box::into_raw(boxed).cast()) }
}
/// Private<T>: the interface the object keeps under `key` (its reference ours), if any.
fn private(object: &ID3D12Object, key: &GUID) -> Option<IUnknown> {
    let mut raw: *mut c_void = null_mut();
    let mut size = core::mem::size_of::<*mut c_void>() as u32;
    // SAFETY: a pointer-sized output.
    let read = unsafe { object.GetPrivateData(key, &mut size, Some((&raw mut raw).cast())) };
    // SAFETY: the interface the data held, AddRef'd by the read.
    (read.is_ok() && size as usize == core::mem::size_of::<*mut c_void>() && !raw.is_null()).then(|| unsafe { IUnknown::from_raw(raw) })
}
/// The owner behind an interface this module keeps (only its owners are kept under its keys).
fn data(unknown: &IUnknown) -> &OwnerData {
    // SAFETY: an Owner (see above), alive while `unknown` holds its reference.
    unsafe { &(*(unknown.as_raw() as *const Owner)).data }
}
/// A live object of interface `T` (or null) as itself, its reference ours.
fn object<T: Interface + Clone>(raw: *mut c_void) -> Option<T> {
    // SAFETY: a live object of that interface or null.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}
fn root_data(root: &ID3D12Object) -> Option<IUnknown> {
    // (every D3D12 object's private data is ID3D12Object's: the interfaces deref to it, no QueryInterface)
    private(root, &ROOT_KEY).filter(|u| matches!(data(u), OwnerData::Root(_)))
}
fn family_of(unknown: &IUnknown) -> Option<Arc<Family>> {
    match data(unknown) {
        OwnerData::Family(f) => Some(f.clone()),
        OwnerData::Root(_) => None,
    }
}

// ---- the manifest

struct Asset {
    index: u32,
    vs_bytes: u32,
    ps_bytes: u32,
    camera_stages: u32,
    vs: Vec<u8>,
    ps: Vec<u8>,
    camera_vs: Vec<u8>,
    camera_ps: Vec<u8>,
    pairs: Vec<PairedBinding>,
    uses: Vec<ResourceUse>,
}
type Assets = BTreeMap<(String, String), Asset>;
static ASSETS: OnceLock<Option<Assets>> = OnceLock::new();
fn bytes(b: &FamilyBytes) -> Vec<u8> {
    if b.p.is_null() || b.n == 0 {
        Vec::new()
    } else {
        // SAFETY: the manifest's bytes (kept for the process).
        unsafe { core::slice::from_raw_parts(b.p, b.n) }.to_vec()
    }
}
fn items<T: Copy>(p: *const T, n: usize) -> Vec<T> {
    if p.is_null() || n == 0 {
        Vec::new()
    } else {
        // SAFETY: the manifest's items (kept for the process).
        unsafe { core::slice::from_raw_parts(p, n) }.to_vec()
    }
}
fn error(text: &[u8]) {
    let n = text.iter().position(|&c| c == 0).unwrap_or(text.len()).min(255);
    // SAFETY: the export, written once while the families load.
    unsafe {
        let e = &mut *(&raw mut CyberpunkVR_StereoFamilyError);
        e[..n].copy_from_slice(&text[..n]);
        e[n] = 0;
    }
}
/// Load: the manifest's assets keyed by their shaders' hashes, the exports set.
fn load(path: &[u16]) -> Result<Assets, Vec<u8>> {
    let h = host().ok_or_else(|| b"Families host missing".to_vec())?;
    let mut view: *const FamilyManifestView = core::ptr::null();
    let mut failure = [0u8; 512];
    // SAFETY: the path and the writable view and buffer.
    let handle = unsafe { (h.load)(path.as_ptr(), path.len(), &mut view, failure.as_mut_ptr(), failure.len()) };
    if handle.is_null() || view.is_null() {
        return Err(failure.to_vec());
    }
    // SAFETY: the handle's view, alive for the process (the handle is never freed).
    let view = unsafe { &*view };
    let mut assets = Assets::new();
    let views: &[crate::stereo_family_manifest::FamilyAssetView] = if view.assets.is_null() || view.asset_count == 0 {
        &[]
    } else {
        // SAFETY: the handle's views, alive for the process.
        unsafe { core::slice::from_raw_parts(view.assets, view.asset_count) }
    };
    for v in views {
        let key = (String::from_utf8_lossy(&bytes(&v.vs_hash)).into_owned(), String::from_utf8_lossy(&bytes(&v.ps_hash)).into_owned());
        assets.entry(key).or_insert(
            Asset {
                index: v.index,
                vs_bytes: v.vs_bytes,
                ps_bytes: v.ps_bytes,
                camera_stages: v.camera_stages,
                vs: bytes(&v.vs),
                ps: bytes(&v.ps),
                camera_vs: bytes(&v.camera_vs),
                camera_ps: bytes(&v.camera_ps),
                pairs: items(v.pairs, v.pair_count),
                uses: items(v.uses, v.use_count),
            },
        );
    }
    CyberpunkVR_StereoFamilyAssetCount.store(assets.len() as u32, SeqCst);
    CyberpunkVR_StereoFamilyNativeCameras.store(assets.values().filter(|a| !a.camera_vs.is_empty()).count() as u32, SeqCst);
    // SAFETY: the export, written once while the families load.
    unsafe { (*(&raw mut CyberpunkVR_StereoFamilyManifestHash)).copy_from_slice(&view.manifest_hash) };
    Ok(assets)
}
fn assets() -> Option<&'static Assets> {
    ASSETS
        .get_or_init(|| {
            let path: Vec<u16> = std::env::var_os("CVR_STEREO_FAMILY_MANIFEST").map(|p| std::os::windows::ffi::OsStrExt::encode_wide(p.as_os_str()).collect())?;
            if path.is_empty() {
                return None;
            }
            CyberpunkVR_StereoFamilyAssetStatus.store(1, SeqCst);
            let loaded = if path.len() >= 32768 { Err(b"Family manifest path too long".to_vec()) } else { load(&path) };
            match loaded {
                Ok(assets) => {
                    CyberpunkVR_StereoFamilyAssetStatus.store(2, SeqCst);
                    Some(assets)
                }
                Err(text) => {
                    error(&text);
                    CyberpunkVR_StereoFamilyAssetStatus.store(3, SeqCst);
                    None
                }
            }
        })
        .as_ref()
}
/// families::Enabled: a verified development package named by CVR_STEREO_FAMILY_MANIFEST (loaded once).
pub fn enabled() -> bool {
    assets().is_some()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nsf2_enabled() -> bool {
    enabled()
}
fn hash(data: &[u8]) -> Result<String, String> {
    crate::stereo_family_manifest::sha256_hex(data)
}
fn code(c: &D3D12_SHADER_BYTECODE) -> &[u8] {
    if c.pShaderBytecode.is_null() || c.BytecodeLength == 0 {
        &[]
    } else {
        // SAFETY: the engine's bytecode.
        unsafe { core::slice::from_raw_parts(c.pShaderBytecode.cast(), c.BytecodeLength) }
    }
}

// ---- the records

static RECORDS: Mutex<()> = Mutex::new(());
/// Publish: the record (with the family's contracts and native camera pipeline); its index, CAPACITY when full.
fn publish(row: Record, family: Option<(u32, u32, u64, u32)>) -> u32 {
    let _held = RECORDS.lock().unwrap_or_else(|p| p.into_inner());
    let index = CyberpunkVR_StereoFamilyCount.load(SeqCst);
    if index >= CAPACITY {
        CyberpunkVR_StereoFamilyOverflow.store(1, SeqCst);
        return CAPACITY;
    }
    CyberpunkVR_StereoFamilySeq.fetch_add(1, SeqCst);
    // SAFETY: the exports, written under the records' lock.
    unsafe {
        (*(&raw mut CyberpunkVR_StereoFamilyRecords))[index as usize] = row;
        if let Some((skinning, alpha, camera, stages)) = family {
            (*(&raw mut CyberpunkVR_StereoFamilySkinningReadContracts))[index as usize] = skinning;
            (*(&raw mut CyberpunkVR_StereoFamilyAlphaDepthContracts))[index as usize] = alpha;
            (*(&raw mut CyberpunkVR_StereoFamilyNativeCameraPipelines))[index as usize] = [camera, u64::from(stages)];
        }
    }
    CyberpunkVR_StereoFamilyCount.store(index + 1, SeqCst);
    CyberpunkVR_StereoFamilySeq.fetch_add(1, SeqCst);
    index
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

// ---- the paired roots

/// PairedRoot: the root's paired variant for the asset's pairs -- the one a family still holds, else built (at most
/// 256 per root) with its C++ mirror.
fn paired_root(device: &ID3D12Device, root: &ID3D12Object, asset: &Asset) -> Option<Arc<RootVariant>> {
    let owner = root_data(root)?;
    let OwnerData::Root(r) = data(&owner) else { return None };
    let mut ordered = asset.pairs.clone();
    let mut key = vec![0u32; ordered.len() * 4];
    // SAFETY: the pairs (ordered in place) and a key of four words each.
    unsafe { crate::stereo_family_contracts::cvr_nsf_paired_key(ordered.as_mut_ptr(), ordered.len(), key.as_mut_ptr()) };
    let mut variants = r.variants.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(old) = variants.get(&key) {
        if let Some(cached) = old.upgrade() {
            CyberpunkVR_StereoFamilyRootHits.fetch_add(1, SeqCst);
            return Some(cached);
        }
        variants.remove(&key);
    }
    variants.retain(|_, v| v.strong_count() != 0);
    if variants.len() >= 256 {
        return None;
    }
    let mut plan: *mut crate::paired_root::Plan = null_mut();
    // SAFETY: the root's blob and the ordered pairs (the same 16-byte binding).
    let built = unsafe { crate::paired_root::cvr_paired_root_build(r.bytes.as_ptr(), r.bytes.len(), ordered.as_ptr().cast(), ordered.len(), &mut plan) };
    if built.is_err() || plan.is_null() {
        return None;
    }
    // SAFETY: the plan just built, freed once.
    let plan = unsafe { Box::from_raw(plan) };
    let made = {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: the serialized root on its node.
        unsafe { device.CreateRootSignature::<ID3D12RootSignature>(r.node, &plan.serialized) }
    };
    let root = made.ok()?;
    let view = VariantView {
        root: root.as_raw(),
        serialized: plan.serialized.as_ptr(),
        serialized_length: plan.serialized.len(),
        bindings: plan.bindings.as_ptr(),
        binding_count: plan.bindings.len(),
        scalars: [plan.parameters[0], plan.parameters[1], plan.descriptors[0], plan.descriptors[1], plan.original_parameters, plan.root_dwords],
    };
    // The C++ mirror (none in the plugin: a host without the function).
    let mirror = match host().map(|h| h.variant) {
        // SAFETY: the view of the live root and plan.
        Some(Some(make)) => unsafe { make(&view) },
        Some(None) => NO_MIRROR,
        None => null_mut(),
    };
    if mirror.is_null() {
        return None;
    }
    let mirror = if mirror == NO_MIRROR { null_mut() } else { mirror };
    let candidate = Arc::new(RootVariant { root, plan: *plan, mirror });
    variants.insert(key, Arc::downgrade(&candidate));
    Some(candidate)
}

// ---- the pipelines' observation and the registration

/// ObserveNativeDepth: the pipeline's native depth and native geometry status words.
fn observe_native_depth(original: &ID3D12PipelineState, pass: &PassContract, vs: &D3D12_SHADER_BYTECODE, ps: &D3D12_SHADER_BYTECODE, other: bool, stream: bool) {
    use crate::depth_policy as policy;
    let assets = assets();
    let depth = || -> Result<u32, String> {
        // SAFETY: the pass contract.
        if !unsafe { crate::stereo_family_contracts::cvr_nsf_depth_pass(pass, !vs.pShaderBytecode.is_null() && vs.BytecodeLength != 0, other, stream) } {
            return Ok(2);
        }
        let key = (hash(code(vs))?, if ps.BytecodeLength != 0 { hash(code(ps))? } else { String::new() });
        let known = assets.and_then(|a| a.get(&key));
        // SAFETY: the engine's bytecode (each as its pointer and length).
        let status = unsafe {
            if !policy::cvr_depth_policy_explicit_shader_resources(vs.pShaderBytecode, vs.BytecodeLength)
                || !policy::cvr_depth_policy_explicit_shader_resources(ps.pShaderBytecode, ps.BytecodeLength)
            {
                6
            } else if let Some(k) = known {
                if !crate::stereo_family_contracts::cvr_nsf_uses_uav(k.uses.as_ptr(), k.uses.len()) { 1 } else { 3 }
            } else if !policy::cvr_depth_policy_shader_read_only(vs.pShaderBytecode, vs.BytecodeLength) {
                4
            } else if policy::cvr_depth_policy_shader_read_only(ps.pShaderBytecode, ps.BytecodeLength) {
                1
            } else {
                5
            }
        };
        Ok(status)
    };
    let status: u32 = depth().unwrap_or(2);
    // SAFETY: a local word and its size.
    let _ = unsafe { original.SetPrivateData(&NATIVE_DEPTH_KEY, 4, Some((&raw const status).cast())) };
    // SAFETY: the pass contract; the engine's bytecode.
    let geometry: u32 = unsafe {
        if crate::stereo_family_contracts::cvr_nsf_geometry_pass(pass, other, stream) {
            if !policy::cvr_depth_policy_shader_single_layer_read_only(vs.pShaderBytecode, vs.BytecodeLength, true) {
                3
            } else if !policy::cvr_depth_policy_shader_single_layer_read_only(ps.pShaderBytecode, ps.BytecodeLength, false) {
                4
            } else {
                1
            }
        } else {
            2
        }
    };
    // SAFETY: as above.
    let _ = unsafe { original.SetPrivateData(&NATIVE_GEOMETRY_KEY, 4, Some((&raw const geometry).cast())) };
}
/// The family's pipelines' creation: (device, the asset's VS and PS, the root) -> the pipeline or the HRESULT.
type Create<'a> = dyn Fn(&ID3D12Device2, &[u8], &[u8], &ID3D12RootSignature) -> Result<ID3D12PipelineState, HRESULT> + 'a;
fn shader(bytes: &[u8]) -> D3D12_SHADER_BYTECODE {
    if bytes.is_empty() {
        D3D12_SHADER_BYTECODE::default()
    } else {
        D3D12_SHADER_BYTECODE { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() }
    }
}
/// Lookup's Rust half: the pipeline's family.
pub fn lookup(pso: &ID3D12PipelineState) -> Option<Arc<Family>> {
    private(pso, &FAMILY_KEY).and_then(|u| family_of(&u))
}
#[allow(clippy::too_many_arguments)]
fn register(
    device: &ID3D12Device,
    root: *mut c_void,
    original: &ID3D12PipelineState,
    vs: &D3D12_SHADER_BYTECODE,
    ps: &D3D12_SHADER_BYTECODE,
    input: &D3D12_INPUT_LAYOUT_DESC,
    pass: &PassContract,
    create: &Create<'_>,
) {
    // SAFETY: a live root signature or null.
    let Some(root_signature) = (unsafe { ID3D12RootSignature::from_raw_borrowed(&root) }).cloned() else { return };
    if vs.pShaderBytecode.is_null() || vs.BytecodeLength == 0 || (ps.BytecodeLength != 0 && ps.pShaderBytecode.is_null()) {
        return;
    }
    let mut record = Record {
        id: NEXT_ID.fetch_add(1, SeqCst),
        original: original.as_raw() as u64,
        root: root as u64,
        result: E_FAIL.0,
        ..Record::default()
    };
    let attempt = |record: &mut Record| -> Result<(), String> {
        let key = (hash(code(vs))?, if ps.BytecodeLength != 0 { hash(code(ps))? } else { String::new() });
        let Some(asset) = assets().and_then(|a| a.get(&key)) else {
            CyberpunkVR_StereoFamilyMisses.fetch_add(1, SeqCst);
            return Ok(());
        };
        if vs.BytecodeLength != asset.vs_bytes as usize || ps.BytecodeLength != asset.ps_bytes as usize {
            return Err("Native shader size differs".into());
        }
        record.asset = asset.index;
        if lookup(original).is_some_and(|old| old.source_root.as_raw() == root && old.asset == asset.index) {
            CyberpunkVR_StereoFamilyHits.fetch_add(1, SeqCst);
            return Ok(());
        }
        let root_object: &ID3D12Object = &root_signature;
        let root_owner = root_data(root_object).ok_or_else(|| String::from("Missing original root identity"))?;
        let OwnerData::Root(identity) = data(&root_owner) else { return Err("Missing original root identity".into()) };
        let root_sha = identity.digest.clone();
        // SAFETY: the engine's VS and input layout.
        let rebase = unsafe { crate::instance_rebase::cvr_ir_supports_rebase(vs, input) };
        // SAFETY: as above.
        let ordinal = rebase && unsafe { crate::instance_rebase::cvr_ir_independent_ordinal(vs) };
        // SAFETY: the hashes' text and the input layout.
        let skinning = unsafe { crate::skinning_read_plan::cvr_sk_depth_contract(key.0.as_ptr(), key.0.len(), input) };
        // SAFETY: as above.
        let alpha = unsafe { crate::skinning_read_plan::cvr_sk_alpha_contract(key.0.as_ptr(), key.0.len(), key.1.as_ptr(), key.1.len(), input) };
        let mut contract = *pass;
        contract.pixel_shader = ps.BytecodeLength != 0;
        let paired = paired_root(device, root_object, asset).ok_or_else(|| String::from("Missing or incompatible native root metadata"))?;
        let device2 = device.cast::<ID3D12Device2>();
        record.result = device2.as_ref().map_or_else(|e| e.code().0, |_| S_OK.0);
        let pipeline = match &device2 {
            Ok(d) => {
                let _internal = cvr_host::probe::Internal::new();
                create(d, &asset.vs, &asset.ps, &paired.root)
            }
            Err(e) => Err(e.code()),
        };
        let pipeline = match pipeline {
            Ok(p) => {
                record.result = S_OK.0;
                p
            }
            Err(hr) => {
                if record.result >= 0 {
                    record.result = hr.0;
                }
                CyberpunkVR_StereoFamilyFailures.fetch_add(1, SeqCst);
                publish(*record, None);
                return Ok(());
            }
        };
        let mut camera = None;
        let mut stages = 0;
        if !asset.camera_vs.is_empty() {
            let d = device2.as_ref().expect("the pipeline was made with it");
            let _internal = cvr_host::probe::Internal::new();
            camera = Some(create(d, &asset.camera_vs, &asset.camera_ps, &root_signature).map_err(|_| String::from("Native camera pipeline failed"))?);
            stages = asset.camera_stages;
        }
        record.paired_root = paired.root.as_raw() as u64;
        record.pipeline = pipeline.as_raw() as u64;
        record.pairs = asset.pairs.len() as u32;
        record.uses = asset.uses.len() as u32;
        record.result = E_PENDING.0;
        let camera_raw = camera.as_ref().map_or(0, |c| c.as_raw() as u64);
        let index = publish(*record, Some((skinning, alpha, camera_raw, stages)));
        let mirror_view = FamilyMirror {
            id: record.id,
            asset: asset.index,
            record: if index < CAPACITY { index } else { u32::MAX },
            root_sha: lend(&root_sha),
            vertex_sha: lend(&key.0),
            pixel_sha: lend(&key.1),
            source_root: root,
            pipeline: pipeline.as_raw(),
            native_camera_pipeline: camera.as_ref().map_or(null_mut(), |c| c.as_raw()),
            native_camera_stages: stages,
            skinning_read_contract: skinning,
            alpha_depth_contract: alpha,
            instance_rebase_safe: rebase,
            instance_ordinal_independent: ordinal,
            uses: asset.uses.as_ptr(),
            use_count: asset.uses.len(),
            pass: contract,
        };
        // The C++ mirror (none in the plugin: a host without the function).
        let mirror = match host().map(|h| h.family) {
            // SAFETY: the view of the live objects; the variant's mirror.
            Some(Some(make)) => unsafe { make(&mirror_view, paired.mirror) },
            Some(None) => NO_MIRROR,
            None => null_mut(),
        };
        if mirror.is_null() {
            CyberpunkVR_StereoFamilyFailures.fetch_add(1, SeqCst);
            return Ok(());
        }
        let mirror = if mirror == NO_MIRROR { null_mut() } else { mirror };
        let family = Arc::new(Family {
            id: record.id,
            asset: asset.index,
            record: mirror_view.record,
            root_sha,
            vertex_sha: key.0.clone(),
            pixel_sha: key.1.clone(),
            source_root: root_signature.clone(),
            paired_root: paired,
            pipeline,
            native_camera_pipeline: camera,
            native_camera_stages: stages,
            skinning_read_contract: skinning,
            alpha_depth_contract: alpha,
            uses: asset.uses.clone(),
            instance_rebase_safe: rebase,
            instance_ordinal_independent: ordinal,
            pass: contract,
            mirror,
        });
        let kept = owner(OwnerData::Family(family));
        // SAFETY: the original pipeline keeps the owner.
        let attached = unsafe { original.SetPrivateDataInterface(&FAMILY_KEY, &kept) };
        let attached = attached.map_or_else(|e| e.code().0, |_| S_OK.0);
        if attached < 0 {
            CyberpunkVR_StereoFamilyFailures.fetch_add(1, SeqCst);
        }
        if index < CAPACITY {
            let _held = RECORDS.lock().unwrap_or_else(|p| p.into_inner());
            CyberpunkVR_StereoFamilySeq.fetch_add(1, SeqCst);
            // SAFETY: the export, written under the records' lock.
            unsafe { (*(&raw mut CyberpunkVR_StereoFamilyRecords))[index as usize].result = attached };
            CyberpunkVR_StereoFamilySeq.fetch_add(1, SeqCst);
        }
        Ok(())
    };
    if attempt(&mut record).is_err() {
        CyberpunkVR_StereoFamilyFailures.fetch_add(1, SeqCst);
        publish(record, None);
    }
}
const LOCATIONS: [D3D12_VIEW_INSTANCE_LOCATION; 2] = [
    D3D12_VIEW_INSTANCE_LOCATION { ViewportArrayIndex: 0, RenderTargetArrayIndex: 0 },
    D3D12_VIEW_INSTANCE_LOCATION { ViewportArrayIndex: 0, RenderTargetArrayIndex: 1 },
];
fn made(hr: HRESULT, out: *mut c_void) -> Result<ID3D12PipelineState, HRESULT> {
    if hr.is_ok() && !out.is_null() {
        // SAFETY: the created pipeline's reference.
        Ok(unsafe { ID3D12PipelineState::from_raw(out) })
    } else {
        Err(if hr.is_ok() { E_FAIL } else { hr })
    }
}

// ---- the C ABI the detours and the .cpp call

/// RootCreated: the root signature's identity (its blob, node and digest) kept as its private data, once.
///
/// # Safety
/// `root` is live or null; `bytes` readable for `size`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_root_created(root: *mut c_void, node: u32, bytes: *const c_void, size: usize) {
    if cvr_host::probe::internal() || !enabled() || root.is_null() || bytes.is_null() || size == 0 || size > 1024 * 1024 {
        return;
    }
    let Some(object) = object::<ID3D12RootSignature>(root) else { return };
    if root_data(&object).is_some() {
        return;
    }
    // SAFETY: the caller's blob.
    let blob = unsafe { core::slice::from_raw_parts(bytes.cast::<u8>(), size) };
    let Ok(digest) = hash(blob) else {
        CyberpunkVR_StereoFamilyFailures.fetch_add(1, SeqCst);
        return;
    };
    let kept = owner(OwnerData::Root(RootData { node, bytes: blob.to_vec(), digest, variants: Mutex::new(BTreeMap::new()) }));
    // SAFETY: the root keeps the owner.
    if unsafe { object.SetPrivateDataInterface(&ROOT_KEY, &kept) }.is_err() {
        CyberpunkVR_StereoFamilyFailures.fetch_add(1, SeqCst);
    }
}
/// GraphicsCreated: a classic pipeline observed and, without other stages or stream output, registered.
///
/// # Safety
/// `device` and `original` are live; `source` is the engine's description.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_graphics_created(device: *mut c_void, source: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC, original: *mut c_void) {
    if cvr_host::probe::internal() || !enabled() {
        return;
    }
    // SAFETY: as documented.
    let (Some(device), Some(source), Some(original)) =
        (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe { source.as_ref() }, unsafe { ID3D12PipelineState::from_raw_borrowed(&original) })
    else {
        // ObserveNativeDepth leaves no word without the pipeline; Register needs the device.
        return;
    };
    let pass = PassContract {
        targets: D3D12_RT_FORMAT_ARRAY { RTFormats: source.RTVFormats, NumRenderTargets: source.NumRenderTargets },
        depth: source.DSVFormat,
        samples: source.SampleDesc,
        topology: source.PrimitiveTopologyType,
        fixed_raster_state: source.Flags == D3D12_PIPELINE_STATE_FLAG_NONE && source.RasterizerState.ForcedSampleCount == 0,
        pixel_shader: true,
        depth_state: source.DepthStencilState,
        depth_state_known: true,
    };
    let other = source.GS.BytecodeLength != 0 || source.HS.BytecodeLength != 0 || source.DS.BytecodeLength != 0;
    let stream = source.StreamOutput.NumEntries != 0 || source.StreamOutput.NumStrides != 0;
    observe_native_depth(original, &pass, &source.VS, &source.PS, other, stream);
    if other || stream {
        return;
    }
    let root = source.pRootSignature.as_ref().map_or(null_mut(), |r| r.as_raw());
    let create = |d: &ID3D12Device2, vs: &[u8], ps: &[u8], root: &ID3D12RootSignature| {
        // SAFETY: a copy of the engine's description; the root reference stays the caller's (ManuallyDrop).
        let mut desc: D3D12_GRAPHICS_PIPELINE_STATE_DESC = unsafe { core::ptr::read(source) };
        desc.pRootSignature = core::mem::ManuallyDrop::new(Some(root.clone()));
        desc.VS = shader(vs);
        desc.PS = shader(ps);
        let mut out = null_mut();
        // SAFETY: the device, the description and the output.
        let hr = unsafe { crate::pipeline_stream::cvr_pipeline_stream_create_view_instanced_graphics(d.as_raw(), &desc, LOCATIONS.as_ptr(), true, &mut out) };
        // SAFETY: the clone's reference, released once.
        unsafe { core::mem::ManuallyDrop::drop(&mut desc.pRootSignature) };
        made(hr, out)
    };
    register(device, root, original, &source.VS, &source.PS, &source.InputLayout, &pass, &create);
}
/// StreamCreated: a stream pipeline observed and, without other stages, view instancing or stream output, registered.
///
/// # Safety
/// `device` and `original` are live; `source` is the engine's stream.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_stream_created(device: *mut c_void, source: *const D3D12_PIPELINE_STATE_STREAM_DESC, original: *mut c_void) {
    if cvr_host::probe::internal() || !enabled() {
        return;
    }
    // SAFETY: as documented.
    let (Some(device), Some(stream_desc), Some(original)) =
        (unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe { source.as_ref() }, unsafe { ID3D12PipelineState::from_raw_borrowed(&original) })
    else {
        return;
    };
    // SAFETY: the engine's stream.
    let Some(info) = (unsafe { crate::pipeline_stream::read(stream_desc) }) else { return };
    let pass = PassContract {
        targets: info.targets,
        depth: info.depth_format,
        samples: info.samples,
        topology: info.topology,
        fixed_raster_state: !info.depth_bounds && info.forced_samples == 0 && info.flags == D3D12_PIPELINE_STATE_FLAG_NONE,
        pixel_shader: true,
        depth_state: info.depth,
        depth_state_known: info.have_depth,
    };
    observe_native_depth(original, &pass, &info.vs, &info.ps, info.other_stages || info.view_instancing, info.stream_output);
    if info.other_stages || info.view_instancing || info.stream_output {
        return;
    }
    let create = |d: &ID3D12Device2, vs: &[u8], ps: &[u8], root: &ID3D12RootSignature| {
        let (vs, ps) = (shader(vs), shader(ps));
        let mut out = null_mut();
        // SAFETY: the device, the engine's stream, the asset's shaders, the root and the output.
        let hr = unsafe { crate::pipeline_stream::cvr_pipeline_stream_create_view_instanced(d.as_raw(), source, &vs, LOCATIONS.as_ptr(), true, &mut out, &ps, root.as_raw()) };
        made(hr, out)
    };
    register(device, info.root, original, &info.vs, &info.ps, &info.input, &pass, &create);
}
/// observation_profile::Scope.
struct Profile {
    index: usize,
    begin: Option<i64>,
}
fn qpc() -> i64 {
    let mut now = 0i64;
    // SAFETY: one output value.
    let _ = unsafe { QueryPerformanceCounter(&mut now) };
    now
}
impl Profile {
    fn new(phase: usize) -> Self {
        // SAFETY: the C++ inline export, alive for the process.
        let on = unsafe { CyberpunkVR_ObservationProfileEnable.load(Relaxed) } != 0;
        Profile { index: phase * 2, begin: on.then(qpc) }
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        if let Some(begin) = self.begin {
            let end = qpc();
            // SAFETY: as above.
            unsafe {
                CyberpunkVR_ObservationProfileStats[self.index].fetch_add(1, Relaxed);
                CyberpunkVR_ObservationProfileStats[self.index + 1].fetch_add(end.wrapping_sub(begin) as u64, Relaxed);
            }
        }
    }
}
const PHASE_BIND_PIPELINE: usize = 6;
const PHASE_BIND_ROOT: usize = 7;
/// PipelineBound: the list keeps the pipeline's family (none: cleared).
///
/// # Safety
/// `list` is live or null; `pso` live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_pipeline_bound(list: *mut c_void, pso: *mut c_void) {
    if cvr_host::probe::internal() || !enabled() || list.is_null() {
        return;
    }
    let _profile = Profile::new(PHASE_BIND_PIPELINE);
    let Some(list) = object::<ID3D12GraphicsCommandList>(list) else { return };
    let family = object::<ID3D12PipelineState>(pso).and_then(|p| private(&p, &FAMILY_KEY));
    // SAFETY: the list keeps the owner (or drops its own).
    if unsafe { list.SetPrivateDataInterface(&BOUND_FAMILY_KEY, family.as_ref()) }.is_err() {
        CyberpunkVR_StereoFamilyBindingFailures.fetch_add(1, SeqCst);
    }
}
/// RootBound: the list keeps its bound root.
///
/// # Safety
/// As cvr_nsf2_pipeline_bound's.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_root_bound(list: *mut c_void, root: *mut c_void) {
    if cvr_host::probe::internal() || !enabled() || list.is_null() {
        return;
    }
    let _profile = Profile::new(PHASE_BIND_ROOT);
    let Some(list) = object::<ID3D12GraphicsCommandList>(list) else { return };
    // The root as the IUnknown it is (no QueryInterface: the list keeps this very pointer).
    let root = object::<ID3D12RootSignature>(root);
    let unknown: Option<&IUnknown> = root.as_ref().map(|r| {
        let u: &IUnknown = r.into();
        u
    });
    // SAFETY: the list keeps the root (or drops its own).
    if unsafe { list.SetPrivateDataInterface(&BOUND_ROOT_KEY, unknown) }.is_err() {
        CyberpunkVR_StereoFamilyBindingFailures.fetch_add(1, SeqCst);
    }
}
/// Reset: the reset's pipeline bound, no root.
///
/// # Safety
/// As cvr_nsf2_pipeline_bound's.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_reset(list: *mut c_void, pso: *mut c_void) {
    // SAFETY: as documented.
    unsafe {
        cvr_nsf2_pipeline_bound(list, pso);
        cvr_nsf2_root_bound(list, null_mut());
    }
}
/// The list's bound family and whether its bound root matches the family's (the same root, or one with its digest);
/// None without a bound family.
fn bound_family(list: &ID3D12Object) -> Option<(Arc<Family>, bool)> {
    let family = private(list, &BOUND_FAMILY_KEY).and_then(|u| family_of(&u))?;
    let root = private(list, &BOUND_ROOT_KEY);
    let root_raw = root.as_ref().map_or(null_mut(), |r| r.as_raw());
    if root_raw == family.source_root.as_raw() {
        return Some((family, true));
    }
    let digest = |raw: *mut c_void| -> Option<String> {
        let o = object::<ID3D12RootSignature>(raw)?;
        let owner = root_data(&o)?;
        match data(&owner) {
            OwnerData::Root(r) => Some(r.digest.clone()),
            OwnerData::Family(_) => None,
        }
    };
    let matches = matches!((digest(root_raw), digest(family.source_root.as_raw())), (Some(a), Some(b)) if a == b);
    Some((family, matches))
}
/// Bound: the list's family, its bound root compatible (exact, or the same digest).
pub fn bound(list: *mut c_void) -> Option<Arc<Family>> {
    if list.is_null() || CyberpunkVR_StereoFamilyBindingFailures.load(Relaxed) != 0 {
        return None;
    }
    let list = object::<ID3D12GraphicsCommandList>(list)?;
    bound_family(&list).and_then(|(f, ok)| ok.then_some(f))
}
/// ObserveDraw: a VRCAM draw (node 0x33DDA4) counted on its bound family's record.
///
/// # Safety
/// `list` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_observe_draw(list: *mut c_void, node: u32, side: i32) {
    if CyberpunkVR_StereoFamilyObserve.load(Relaxed) == 0
        || CyberpunkVR_StereoFamilyBindingFailures.load(Relaxed) != 0
        || cvr_host::probe::internal()
        || node != 0x33DDA4
        || !(0..=1).contains(&side)
        || !enabled()
    {
        return;
    }
    let side = side as usize;
    let Some(list) = object::<ID3D12GraphicsCommandList>(list) else { return };
    let Some((family, root_ok)) = bound_family(&list) else {
        CyberpunkVR_StereoFamilyUnmatched[side].fetch_add(1, SeqCst);
        return;
    };
    if !root_ok {
        CyberpunkVR_StereoFamilyRootMismatch[side].fetch_add(1, SeqCst);
        return;
    }
    if family.record >= CAPACITY {
        return;
    }
    let _held = RECORDS.lock().unwrap_or_else(|p| p.into_inner());
    CyberpunkVR_StereoFamilySeq.fetch_add(1, SeqCst);
    // SAFETY: the export, written under the records' lock.
    unsafe { (*(&raw mut CyberpunkVR_StereoFamilyRecords))[family.record as usize].draws[side] += 1 };
    CyberpunkVR_StereoFamilySeq.fetch_add(1, SeqCst);
}
/// Lookup (`which` 0: the pipeline's) and Bound (1: the list's): the family's C++ mirror (a std::shared_ptr<const
/// Family>*), its owner's reference in `*held` (released by the caller once the mirror is copied); null without one.
///
/// # Safety
/// `object` is live or null; `held` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_lookup(target: *mut c_void, which: u32, held: *mut *mut c_void) -> *const c_void {
    // SAFETY: the caller's output.
    unsafe { *held = null_mut() };
    let family = match which {
        0 => object::<ID3D12PipelineState>(target).and_then(|p| private(&p, &FAMILY_KEY)).filter(|u| family_of(u).is_some()),
        _ => {
            if target.is_null() || CyberpunkVR_StereoFamilyBindingFailures.load(Relaxed) != 0 {
                return core::ptr::null();
            }
            let Some(list) = object::<ID3D12GraphicsCommandList>(target) else { return core::ptr::null() };
            match bound_family(&list) {
                Some((_, true)) => private(&list, &BOUND_FAMILY_KEY),
                _ => None,
            }
        }
    };
    let Some(owner) = family else { return core::ptr::null() };
    let Some(f) = family_of(&owner) else { return core::ptr::null() };
    let mirror = f.mirror.cast_const();
    // SAFETY: the caller's output takes the owner's reference.
    unsafe { *held = owner.into_raw() };
    mirror
}

/// paired_geometry_native.rs' view of the bound family (formerly the .cpp's cvr_cpp_family_bound).
#[repr(C)]
pub struct BoundView {
    pub identity: *const c_void,
    pub id: u64,
    pub vertex: *const u8,
    pub vertex_length: usize,
    pub pixel: *const u8,
    pub pixel_length: usize,
    pub native_camera_pipeline: *mut c_void,
    pub native_camera_stages: u32,
    pub source_root: *mut c_void,
}
const _: () = assert!(core::mem::size_of::<BoundView>() == 72);
/// The bound family as a held handle (freed by bound_release) and its view; null without one.
pub fn bound_view(list: *mut c_void, out: &mut BoundView) -> *mut c_void {
    let Some(f) = bound(list) else { return null_mut() };
    *out = BoundView {
        identity: Arc::as_ptr(&f).cast(),
        id: f.id,
        vertex: f.vertex_sha.as_ptr(),
        vertex_length: f.vertex_sha.len(),
        pixel: f.pixel_sha.as_ptr(),
        pixel_length: f.pixel_sha.len(),
        native_camera_pipeline: f.native_camera_pipeline.as_ref().map_or(null_mut(), |p| p.as_raw()),
        native_camera_stages: f.native_camera_stages,
        source_root: f.source_root.as_raw(),
    };
    Box::into_raw(Box::new(f)).cast()
}
/// # Safety
/// `handle` came from bound_view, released once.
pub unsafe fn bound_release(handle: *mut c_void) {
    if !handle.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(handle.cast::<Arc<Family>>()) });
    }
}
impl Family {
    /// The family's C++ mirror (a heap std::shared_ptr<const Family>), alive with the family.
    pub fn mirror(&self) -> *const c_void {
        self.mirror
    }
}
/// families::Lookup for Rust: the pipeline's family (null or a pipeline without one: None).
pub fn lookup_raw(pso: *mut c_void) -> Option<Arc<Family>> {
    object::<ID3D12PipelineState>(pso).and_then(|p| lookup(&p))
}
/// The same as C symbols (paired_geometry_native.rs declares them).
///
/// # Safety
/// `list` is live; `out` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_bound_view(list: *mut c_void, out: *mut BoundView) -> *mut c_void {
    // SAFETY: the caller's output.
    bound_view(list, unsafe { &mut *out })
}
/// # Safety
/// `handle` came from cvr_nsf2_bound_view, released once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsf2_bound_release(handle: *mut c_void) {
    // SAFETY: as documented.
    unsafe { bound_release(handle) };
}
