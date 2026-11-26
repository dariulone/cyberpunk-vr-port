//! Single pass's scene state (formerly src/Render/StereoSceneState.cpp; include/Render/StereoSceneState.hpp calls it):
//! the bindings each engine command list has set since its reset, the root signatures that are all descriptor tables,
//! the resolve-compatible pipelines, and weak metadata of every render-target/depth view the engine created (retained
//! only at an actual draw, where the engine owns every bound target; rewrites invalidate earlier entries, null views
//! included). A target's identity is a cookie stored in the resource's private data, so a recycled COM pointer is
//! never mistaken for the resource it replaced.
//!
//! The header filters the calls first: binding edits only while the native probe records (state 1) and not for the
//! port's own commands, view creation only while bindings are observed or the probe is enabled. Identity and shape
//! checks do not establish a resource state, alias lifetime, residency or contents.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::FastMap;
use core::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::{LazyLock, Mutex, MutexGuard};
use windows::core::{IUnknown, Interface, GUID};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The shading rate an engine list set (D3D12_SHADING_RATE, two combiners, the bound image as a weak pointer).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rate {
    pub value: i32,
    pub combiners: [i32; 2],
    pub image: usize,
    pub known: bool,
}
impl Default for Rate {
    fn default() -> Self {
        // D3D12_SHADING_RATE_1X1, D3D12_SHADING_RATE_COMBINER_PASSTHROUGH.
        Rate {
            value: 0,
            combiners: [0, 0],
            image: 0,
            known: false,
        }
    }
}
/// scene_state::Bindings (the same layout: 1136 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bindings {
    pub rate: Rate,
    pub root: usize,
    pub tables: [u64; 64],
    pub table_mask: u64,
    pub compute_root: usize,
    pub compute_tables: [u64; 64],
    pub compute_table_mask: u64,
    pub heaps: [usize; 2],
    pub heap_count: u32,
    pub stencil: u32,
    pub blend_factor: [f32; 4],
    pub topology: i32,
    pub reset_seen: bool,
    pub heaps_seen: bool,
}
impl Default for Bindings {
    fn default() -> Self {
        Bindings {
            rate: Rate::default(),
            root: 0,
            tables: [0; 64],
            table_mask: 0,
            compute_root: 0,
            compute_tables: [0; 64],
            compute_table_mask: 0,
            heaps: [0; 2],
            heap_count: 0,
            stencil: 0,
            blend_factor: [1.0; 4],
            topology: 0,
            reset_seen: false,
            heaps_seen: false,
        }
    }
}
const _: () =
    assert!(core::mem::size_of::<Rate>() == 32 && core::mem::size_of::<Bindings>() == 1136);

/// pass_targets::View: a target's weak identity, shape and view formats/flags (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TargetInfo {
    pub identity: u64,
    pub resource: u64,
    pub width: u64,
    pub height: u32,
    pub format: u32,
    pub view_format: u32,
    pub resource_flags: u32,
    pub view_flags: u32,
    pub reserved: u32,
}
const _: () = assert!(core::mem::size_of::<TargetInfo>() == 48);

/// The bound targets of a native draw (from native_probe::DrawRecord).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrawTargets {
    pub rt_count: u32,
    pub rt_contiguous: u32,
    pub targets: [u64; 8],
    pub depth: u64,
    pub viewport: D3D12_VIEWPORT,
}
/// A draw's captured state; `rate_image` is a reference the caller owns.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrawStateRaw {
    pub rate: Rate,
    pub rate_image: usize,
    pub topology: i32,
    pub stencil: u32,
    pub blend_factor: [f32; 4],
}
/// scene_state::GeometryTargets with its resources as references the caller owns.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GeometryTargetsRaw {
    pub resources: [usize; 5],
    pub views: [TargetInfo; 5],
    pub handles: [usize; 5],
    pub colors: u32,
}

const TARGET_IDENTITY_KEY: GUID = GUID::from_values(
    0x52fbc840,
    0x27d7,
    0x46ce,
    [0x82, 0xe5, 0x58, 0xc5, 0x1c, 0xe5, 0x8f, 0x39],
);
const GEOMETRY_FORMATS: [DXGI_FORMAT; 4] = [
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R16G16_FLOAT,
];

#[derive(Clone, Copy)]
struct View {
    resource: usize,
    depth: bool,
    desc: D3D12_RESOURCE_DESC,
    format: u32,
    flags: u32,
    stride: u32,
    identity: u64,
    whole: bool,
}
impl Default for View {
    fn default() -> Self {
        View {
            resource: 0,
            depth: false,
            desc: D3D12_RESOURCE_DESC::default(),
            format: 0,
            flags: 0,
            stride: 0,
            identity: 0,
            whole: true,
        }
    }
}
#[derive(Default)]
struct State {
    roots: FastMap<usize, u64>,
    pipelines: FastMap<usize, bool>,
    bindings: FastMap<usize, Bindings>,
    targets: FastMap<usize, View>,
    next_target_identity: u64,
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(Mutex::default);
fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn same_desc(a: &D3D12_RESOURCE_DESC, b: &D3D12_RESOURCE_DESC) -> bool {
    a.Dimension == b.Dimension
        && a.Alignment == b.Alignment
        && a.Width == b.Width
        && a.Height == b.Height
        && a.DepthOrArraySize == b.DepthOrArraySize
        && a.MipLevels == b.MipLevels
        && a.Format == b.Format
        && a.SampleDesc.Count == b.SampleDesc.Count
        && a.SampleDesc.Quality == b.SampleDesc.Quality
        && a.Layout == b.Layout
        && a.Flags == b.Flags
}
fn shape(d: &D3D12_RESOURCE_DESC) -> bool {
    d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && d.DepthOrArraySize == 1
        && d.MipLevels == 1
        && d.SampleDesc.Count == 1
        && d.Width != 0
        && d.Height != 0
}
/// A weak resource pointer, borrowed for one call.
unsafe fn resource(raw: &usize) -> Option<&ID3D12Resource> {
    ID3D12Resource::from_raw_borrowed(&*(raw as *const usize as *const *mut c_void))
}
unsafe fn cookie(r: &ID3D12Resource) -> Option<u64> {
    let (mut identity, mut bytes) = (0u64, 8u32);
    r.GetPrivateData(
        &TARGET_IDENTITY_KEY,
        &mut bytes,
        Some(&mut identity as *mut u64 as *mut c_void),
    )
    .ok()
    .filter(|_| bytes == 8 && identity != 0)
    .map(|_| identity)
}
unsafe fn cookie_is(r: &ID3D12Resource, expected: u64) -> bool {
    expected != 0 && cookie(r) == Some(expected)
}

fn edit(list: usize, f: impl FnOnce(&mut Bindings)) {
    let mut s = state();
    if let Some(b) = s.bindings.get_mut(&list) {
        f(b);
    } else if s.bindings.len() < 512 {
        f(s.bindings.entry(list).or_default());
    }
}
fn store(handle: usize, mut view: View) {
    let mut s = state();
    if view.resource == 0 {
        s.targets.remove(&handle);
        return;
    }
    // SAFETY: the resource the engine just created a view of.
    let r = unsafe { resource(&view.resource) }.expect("a live resource");
    match unsafe { cookie(r) } {
        Some(identity) => view.identity = identity,
        None => {
            s.next_target_identity += 1;
            view.identity = s.next_target_identity;
            let set = unsafe {
                r.SetPrivateData(
                    &TARGET_IDENTITY_KEY,
                    8,
                    Some(&view.identity as *const u64 as *const c_void),
                )
            };
            if set.is_err() {
                s.targets.remove(&handle);
                return;
            }
        }
    }
    if s.targets.len() >= 32768 && !s.targets.contains_key(&handle) {
        s.targets.clear();
    }
    s.targets.insert(handle, view);
}

/// RootCreated: a root signature of 1..64 descriptor tables records their mask (others record 0).
///
/// # Safety
/// `data` holds `bytes` bytes of a serialized root signature.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_root_created(root: usize, data: *const c_void, bytes: usize) {
    let mut raw: *mut c_void = core::ptr::null_mut();
    let created = D3D12CreateVersionedRootSignatureDeserializer(
        data,
        bytes,
        &ID3D12VersionedRootSignatureDeserializer::IID,
        &mut raw,
    );
    if created.is_err() || raw.is_null() {
        return;
    }
    let deserializer = ID3D12VersionedRootSignatureDeserializer::from_raw(raw);
    let Ok(desc) = deserializer.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1)
    else {
        return;
    };
    let desc = &(*desc).Anonymous.Desc_1_1;
    let n = desc.NumParameters;
    let mut valid = n > 0 && n <= 64;
    for i in 0..n as usize {
        if !valid {
            break;
        }
        valid =
            (*desc.pParameters.add(i)).ParameterType == D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    }
    let mask = match (valid, n) {
        (false, _) => 0,
        (true, 64) => u64::MAX,
        (true, n) => (1u64 << n) - 1,
    };
    let mut s = state();
    if s.roots.len() < 128 || s.roots.contains_key(&root) {
        s.roots.insert(root, mask);
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_pipeline_created(pso: usize, compatible: bool) {
    let mut s = state();
    if s.pipelines.len() < 8 {
        s.pipelines.insert(pso, compatible);
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_compatible(pso: usize) -> bool {
    state().pipelines.get(&pso).copied().unwrap_or(false)
}
#[no_mangle]
pub extern "C" fn cvr_scene_reset_capture() {
    state().bindings.clear();
}
#[no_mangle]
pub extern "C" fn cvr_scene_reset(list: usize, rate_known: bool) {
    edit(list, |b| {
        *b = Bindings::default();
        b.reset_seen = true;
        b.rate.known = rate_known;
    });
}
#[no_mangle]
pub extern "C" fn cvr_scene_root(list: usize, root: usize) {
    edit(list, |b| {
        if b.root != root {
            b.root = root;
            b.table_mask = 0;
            b.tables = [0; 64];
        }
    });
}
#[no_mangle]
pub extern "C" fn cvr_scene_table(list: usize, index: u32, handle: u64) {
    if index < 64 {
        edit(list, |b| {
            b.tables[index as usize] = handle;
            b.table_mask |= 1 << index;
        });
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_compute_root(list: usize, root: usize) {
    edit(list, |b| {
        if b.compute_root != root {
            b.compute_root = root;
            b.compute_table_mask = 0;
            b.compute_tables = [0; 64];
        }
    });
}
#[no_mangle]
pub extern "C" fn cvr_scene_compute_table(list: usize, index: u32, handle: u64) {
    if index < 64 {
        edit(list, |b| {
            b.compute_tables[index as usize] = handle;
            b.compute_table_mask |= 1 << index;
        });
    }
}
/// # Safety
/// `heaps` holds `count` heap pointers (or is null for none).
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_heaps(list: usize, count: u32, heaps: *const usize) {
    edit(list, |b| {
        b.heaps_seen = count <= 2 && (count == 0 || !heaps.is_null());
        if !b.heaps_seen {
            return;
        }
        let incoming: &[usize] = if count == 0 {
            &[]
        } else {
            core::slice::from_raw_parts(heaps, count as usize)
        };
        let changed =
            b.heap_count != count || incoming.iter().enumerate().any(|(i, &h)| b.heaps[i] != h);
        if changed {
            b.table_mask = 0;
            b.compute_table_mask = 0;
            b.tables = [0; 64];
            b.compute_tables = [0; 64];
        }
        b.heap_count = count;
        b.heaps = [0; 2];
        b.heaps[..incoming.len()].copy_from_slice(incoming);
    });
}
#[no_mangle]
pub extern "C" fn cvr_scene_topology(list: usize, topology: i32) {
    edit(list, |b| b.topology = topology);
}
#[no_mangle]
pub extern "C" fn cvr_scene_stencil(list: usize, stencil: u32) {
    edit(list, |b| b.stencil = stencil & 255);
}
/// # Safety
/// `factors` holds four floats (or is null for the default).
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_blend_factor(list: usize, factors: *const f32) {
    edit(list, |b| {
        b.blend_factor = if factors.is_null() {
            [1.0; 4]
        } else {
            *(factors as *const [f32; 4])
        };
    });
}
/// # Safety
/// `combiners` holds two combiners (or is null for passthrough).
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_shading_rate(list: usize, rate: i32, combiners: *const i32) {
    edit(list, |b| {
        b.rate.value = rate;
        b.rate.combiners = if combiners.is_null() {
            [0, 0]
        } else {
            *(combiners as *const [i32; 2])
        };
    });
}
#[no_mangle]
pub extern "C" fn cvr_scene_shading_rate_image(list: usize, image: usize) {
    edit(list, |b| b.rate.image = image);
}
fn root_mask(s: &State, root: usize) -> u64 {
    s.roots.get(&root).copied().unwrap_or(0)
}
/// Snapshot: the list's bindings, when every bound table belongs to an all-tables root and the reset, heaps,
/// topology and a non-255 stencil were seen. Reset was observed, so a table never bound since then is known-unbound
/// (the native depth prepass leaves the unused pixel-stage tables unbound).
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_snapshot(list: usize, out: *mut Bindings) -> bool {
    let s = state();
    let Some(b) = s.bindings.get(&list) else {
        return false;
    };
    let mask = root_mask(&s, b.root);
    if mask == 0
        || b.table_mask & !mask != 0
        || !b.reset_seen
        || !b.heaps_seen
        || b.heap_count == 0
        || b.topology == 0
        || b.stencil == 255
    {
        return false;
    }
    if b.compute_root != 0 && root_mask(&s, b.compute_root) == 0 {
        return false;
    }
    *out = *b;
    true
}
/// CaptureSourceBindings: the actual source-command bindings, a legal stencil 255 included, all initialized 64-slot
/// tables captured (the shader-required coverage and descriptor contents are separate receipts).
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_capture_source_bindings(
    list: usize,
    out: *mut Bindings,
) -> bool {
    let s = state();
    let Some(b) = s.bindings.get(&list) else {
        return false;
    };
    let mask = root_mask(&s, b.root);
    if mask == 0
        || b.table_mask & !mask != 0
        || !b.reset_seen
        || !b.heaps_seen
        || b.heap_count == 0
        || b.heap_count > 2
        || b.topology == 0
        || !b.rate.known
    {
        return false;
    }
    if b.compute_root != 0 {
        let compute = root_mask(&s, b.compute_root);
        if compute == 0 || b.compute_table_mask & !compute != 0 {
            return false;
        }
    }
    if b.heaps[..b.heap_count as usize].iter().any(|&h| h == 0) {
        return false;
    }
    *out = *b;
    true
}
/// CaptureDrawState: only at an actual native draw, while its PSO and bound image are live.
///
/// # Safety
/// `out` is writable; the bound rate image is live.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_capture_draw_state(list: usize, out: *mut DrawStateRaw) -> bool {
    let s = state();
    let Some(b) = s.bindings.get(&list) else {
        return false;
    };
    if !b.reset_seen || b.topology == 0 || !b.rate.known {
        return false;
    }
    let image = resource(&b.rate.image)
        .cloned()
        .map_or(0, |i| i.into_raw() as usize);
    *out = DrawStateRaw {
        rate: b.rate,
        rate_image: image,
        topology: b.topology,
        stencil: b.stencil,
        blend_factor: b.blend_factor,
    };
    true
}
/// Bindings::Restore: the heaps, the graphics root and its tables, the compute root and its tables (switching
/// heaps invalidates those too, though the resolve never dispatches compute), topology, stencil and blend factor.
///
/// # Safety
/// The bindings' objects are live; `list` is a graphics command list.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_bindings_restore(b: *const Bindings, list: *mut c_void) {
    let (b, Some(list)) = (&*b, ID3D12GraphicsCommandList::from_raw_borrowed(&list)) else {
        return;
    };
    let heaps: ManuallyDrop<[Option<ID3D12DescriptorHeap>; 2]> = core::mem::transmute(b.heaps);
    list.SetDescriptorHeaps(&heaps[..b.heap_count.min(2) as usize]);
    let root = b.root as *mut c_void;
    list.SetGraphicsRootSignature(ID3D12RootSignature::from_raw_borrowed(&root));
    for i in 0..64u32 {
        if b.table_mask & (1 << i) != 0 {
            list.SetGraphicsRootDescriptorTable(
                i,
                D3D12_GPU_DESCRIPTOR_HANDLE {
                    ptr: b.tables[i as usize],
                },
            );
        }
    }
    if b.compute_root != 0 {
        let compute = b.compute_root as *mut c_void;
        list.SetComputeRootSignature(ID3D12RootSignature::from_raw_borrowed(&compute));
        for i in 0..64u32 {
            if b.compute_table_mask & (1 << i) != 0 {
                list.SetComputeRootDescriptorTable(
                    i,
                    D3D12_GPU_DESCRIPTOR_HANDLE {
                        ptr: b.compute_tables[i as usize],
                    },
                );
            }
        }
    }
    list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY(b.topology));
    list.OMSetStencilRef(b.stencil);
    list.OMSetBlendFactor(Some(&b.blend_factor));
}
/// RtvCreated: the view's weak metadata (a whole 2D view when the view is absent or its mip and plane are 0).
///
/// # Safety
/// `resource` is null or the live resource of the view; `view` is null or valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_rtv_created(
    r: usize,
    view: *const D3D12_RENDER_TARGET_VIEW_DESC,
    handle: usize,
) {
    let mut result = View::default();
    if let Some(res) = resource(&r) {
        let d = res.GetDesc();
        let view = view.as_ref();
        let whole = shape(&d)
            && view.is_none_or(|v| {
                v.ViewDimension == D3D12_RTV_DIMENSION_TEXTURE2D
                    && v.Anonymous.Texture2D.MipSlice == 0
                    && v.Anonymous.Texture2D.PlaneSlice == 0
            });
        let mut device: Option<ID3D12Device> = None;
        if res.GetDevice(&mut device).is_ok() {
            if let Some(device) = device {
                result = View {
                    resource: r,
                    depth: false,
                    desc: d,
                    format: view.map_or(d.Format, |v| v.Format).0 as u32,
                    flags: 0,
                    stride: device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV),
                    identity: 0,
                    whole,
                };
            }
        }
    }
    store(handle, result);
}
/// DsvCreated: the view's weak metadata (a whole view keeps the D32S8 format, 2D, mip 0 and read-only flags only).
///
/// # Safety
/// `resource` is null or the live resource of the view; `view` is null or valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_dsv_created(
    r: usize,
    view: *const D3D12_DEPTH_STENCIL_VIEW_DESC,
    handle: usize,
) {
    let mut result = View::default();
    if let Some(res) = resource(&r) {
        let d = res.GetDesc();
        let view = view.as_ref();
        let typeless = d.Format == DXGI_FORMAT_R32G8X24_TYPELESS;
        let typed = d.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
        let whole = shape(&d)
            && (typeless || typed)
            && match view {
                None => typed,
                Some(v) => {
                    v.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT
                        && v.ViewDimension == D3D12_DSV_DIMENSION_TEXTURE2D
                        && v.Flags.0 as u32 & !3 == 0
                        && v.Anonymous.Texture2D.MipSlice == 0
                }
            };
        result = View {
            resource: r,
            depth: true,
            desc: d,
            format: view.map_or(d.Format, |v| v.Format).0 as u32,
            flags: view.map_or(0, |v| v.Flags.0 as u32),
            stride: 0,
            identity: 0,
            whole,
        };
    }
    store(handle, result);
}
/// The handle of render target `i` (or the depth, `i == depth_slot`), or None when its offset overflows.
fn bound_handle(draw: &DrawTargets, i: usize, depth_slot: usize, stride: u32) -> Option<u64> {
    if i == depth_slot {
        return Some(draw.depth);
    }
    if draw.rt_contiguous != 0 {
        let offset = i as u64 * stride as u64;
        (draw.targets[0] <= u64::MAX - offset).then(|| draw.targets[0] + offset)
    } else {
        Some(draw.targets[i])
    }
}
/// The contiguous render targets' stride (0 when not contiguous), None when the first view is unknown.
fn contiguous_stride(s: &State, draw: &DrawTargets) -> Option<u32> {
    if draw.rt_count == 0 || draw.rt_contiguous == 0 {
        return Some(0);
    }
    let first = s.targets.get(&(draw.targets[0] as usize))?;
    (!first.depth && first.stride != 0).then_some(first.stride)
}
/// CaptureBoundResources: at an actual OM/clear call while its caller owns the bound resources, the resources only
/// (array/mip views and internal private targets included). Slots 0..7 colors, 8 depth; owned references out.
///
/// # Safety
/// `handles` holds `count` handles (when nonzero); `depth` is null or valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_capture_bound_resources(
    count: u32,
    handles: *const usize,
    contiguous: bool,
    depth: *const usize,
    out: *mut [usize; 9],
) -> bool {
    *out = [0; 9];
    if count > 8 || (count != 0 && handles.is_null()) || (!depth.is_null() && *depth == 0) {
        return false;
    }
    let mut next: [Option<ID3D12Resource>; 9] = Default::default();
    let s = state();
    let mut stride = 0u32;
    if count != 0 && contiguous {
        let Some(first) = s.targets.get(&*handles) else {
            return false;
        };
        if first.depth || first.stride == 0 {
            return false;
        }
        stride = first.stride;
    }
    for i in 0..9usize {
        if (i < 8 && i >= count as usize) || (i == 8 && depth.is_null()) {
            continue;
        }
        let offset = i as u64 * stride as u64;
        let handle = if i == 8 {
            *depth as u64
        } else if contiguous {
            let first = *handles as u64;
            if first > u64::MAX - offset {
                return false;
            }
            first + offset
        } else {
            *handles.add(i) as u64
        };
        if handle == 0 {
            return false;
        }
        let Some(saved) = s.targets.get(&(handle as usize)).copied() else {
            return false;
        };
        if saved.depth != (i == 8)
            || saved.resource == 0
            || saved.identity == 0
            || (i < 8 && contiguous && saved.stride != stride)
        {
            return false;
        }
        let Some(r) = resource(&saved.resource) else {
            return false;
        };
        let matches = || cookie_is(r, saved.identity) && same_desc(&r.GetDesc(), &saved.desc);
        if !matches() {
            return false;
        }
        next[i] = Some(r.clone());
        if !matches() {
            return false;
        }
    }
    *out = next.map(|r| r.map_or(0, |r| r.into_raw() as usize));
    true
}
/// CaptureTargets: the native G-buffer's three colors (10:10:10:2, 10:10:10:2, 8:8:8:8) or none, and its depth, as
/// whole views of the viewport's size up to 2048. Owned references out.
///
/// # Safety
/// `draw` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_capture_targets(
    draw: *const DrawTargets,
    out: *mut [usize; 4],
) -> bool {
    let draw = &*draw;
    let v = &draw.viewport;
    if (draw.rt_count != 3 && draw.rt_count != 0)
        || (draw.rt_count != 0 && draw.rt_contiguous != 0)
        || draw.depth == 0
        || v.TopLeftX != 0.0
        || v.TopLeftY != 0.0
        || v.MinDepth != 0.0
        || v.MaxDepth != 1.0
    {
        return false;
    }
    let mut pending: [Option<ID3D12Resource>; 4] = Default::default();
    let s = state();
    for i in if draw.rt_count != 0 { 0 } else { 3 }..4usize {
        let handle = if i < 3 { draw.targets[i] } else { draw.depth };
        let Some(view) = s.targets.get(&(handle as usize)) else {
            return false;
        };
        if !view.whole || view.depth != (i == 3) || view.flags != 0 {
            return false;
        }
        let Some(r) = resource(&view.resource) else {
            return false;
        };
        let d = r.GetDesc();
        let format_ok = match i {
            0 | 1 => d.Format == DXGI_FORMAT_R10G10B10A2_UNORM && view.format == 24,
            2 => d.Format == DXGI_FORMAT_R8G8B8A8_UNORM && view.format == 28,
            _ => {
                d.Format == DXGI_FORMAT_R32G8X24_TYPELESS
                    || d.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT
            }
        };
        if !shape(&d)
            || d.Width as f32 != v.Width
            || d.Height as f32 != v.Height
            || d.Width > 2048
            || d.Height > 2048
            || !format_ok
        {
            return false;
        }
        pending[i] = Some(r.clone());
    }
    *out = pending.map(|r| r.map_or(0, |r| r.into_raw() as usize));
    true
}
/// CaptureGeometryTargets: only at an actual native draw while the engine owns all bound resources. Slots 0..3
/// colors (0, 3 or 4 of them), slot 4 depth (read-only DSV flags preserved), each a whole view of one device, of the
/// viewport's size up to 4096, with an unchanged identity cookie and shape before and after its reference is taken.
///
/// # Safety
/// `draw` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_capture_geometry_targets(
    draw: *const DrawTargets,
    out: *mut GeometryTargetsRaw,
) -> bool {
    *out = GeometryTargetsRaw::default();
    let draw = &*draw;
    let v = &draw.viewport;
    if (draw.rt_count != 0 && draw.rt_count != 3 && draw.rt_count != 4)
        || draw.depth == 0
        || v.TopLeftX != 0.0
        || v.TopLeftY != 0.0
        || v.MinDepth != 0.0
        || v.MaxDepth != 1.0
        || !(v.Width >= 1.0 && v.Width <= 4096.0)
        || !(v.Height >= 1.0 && v.Height <= 4096.0)
    {
        return false;
    }
    let mut resources: [Option<ID3D12Resource>; 5] = Default::default();
    let mut pending = GeometryTargetsRaw {
        colors: draw.rt_count,
        ..GeometryTargetsRaw::default()
    };
    let mut saved = [View::default(); 5];
    let mut device_identity: Option<IUnknown> = None;
    let s = state();
    let Some(stride) = contiguous_stride(&s, draw) else {
        return false;
    };
    for i in 0..5usize {
        if i < 4 && i >= draw.rt_count as usize {
            continue;
        }
        let Some(handle) = bound_handle(draw, i, 4, stride) else {
            return false;
        };
        if handle == 0 {
            return false;
        }
        let Some(view) = s.targets.get(&(handle as usize)).copied() else {
            return false;
        };
        if !view.whole || view.depth != (i == 4) {
            return false;
        }
        let Some(r) = resource(&view.resource) else {
            return false;
        };
        if resources[..i].iter().any(|p| {
            p.as_ref()
                .is_some_and(|p| p.as_raw() as usize == view.resource)
        }) {
            return false;
        }
        let view_ok = if i < 4 {
            view.flags == 0
                && view.format == GEOMETRY_FORMATS[i].0 as u32
                && (draw.rt_contiguous == 0 || view.stride == stride)
        } else {
            view.format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT.0 as u32 && view.flags & !3 == 0
        };
        if !cookie_is(r, view.identity) || !view_ok {
            return false;
        }
        let desc = r.GetDesc();
        let kind_ok = if i < 4 {
            desc.Format == GEOMETRY_FORMATS[i]
                && desc.Flags.contains(D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)
        } else {
            (desc.Format == DXGI_FORMAT_R32G8X24_TYPELESS
                || desc.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT)
                && desc.Flags.contains(D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL)
        };
        if !same_desc(&desc, &view.desc)
            || !shape(&desc)
            || desc.SampleDesc.Quality != 0
            || desc.Layout != D3D12_TEXTURE_LAYOUT_UNKNOWN
            || desc.Width as f32 != v.Width
            || desc.Height as f32 != v.Height
            || !kind_ok
        {
            return false;
        }
        let held = r.clone();
        if held.as_raw() as usize != view.resource
            || !cookie_is(&held, view.identity)
            || !same_desc(&held.GetDesc(), &view.desc)
        {
            return false;
        }
        let mut device: Option<ID3D12Device> = None;
        if r.GetDevice(&mut device).is_err() {
            return false;
        }
        let Some(identity) = device.and_then(|d| d.cast::<IUnknown>().ok()) else {
            return false;
        };
        if device_identity
            .as_ref()
            .is_some_and(|d| d.as_raw() != identity.as_raw())
        {
            return false;
        }
        device_identity.get_or_insert(identity);
        resources[i] = Some(held);
        saved[i] = view;
        pending.handles[i] = handle as usize;
        pending.views[i] = TargetInfo {
            identity: view.identity,
            resource: view.resource as u64,
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
            view_format: view.format,
            resource_flags: desc.Flags.0 as u32,
            view_flags: view.flags,
            reserved: 0,
        };
    }
    for i in 0..5usize {
        let Some(held) = resources[i].as_ref() else {
            continue;
        };
        let view = &saved[i];
        let Some(found) = s.targets.get(&pending.handles[i]) else {
            return false;
        };
        if found.resource != view.resource
            || found.identity != view.identity
            || found.depth != view.depth
            || found.format != view.format
            || found.flags != view.flags
            || found.stride != view.stride
            || !same_desc(&found.desc, &view.desc)
            || !cookie_is(held, view.identity)
            || !same_desc(&held.GetDesc(), &view.desc)
        {
            return false;
        }
    }
    pending.resources = resources.map(|r| r.map_or(0, |r| r.into_raw() as usize));
    *out = pending;
    true
}
/// DescribeTargets: weak metadata of up to 8 colors (slots 0..7) and the depth (slot 8) as whole views, tied to the
/// COM resources' lifetime; nothing is retained.
///
/// # Safety
/// `draw` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_describe_targets(
    draw: *const DrawTargets,
    out: *mut [TargetInfo; 9],
) -> bool {
    *out = [TargetInfo::default(); 9];
    let draw = &*draw;
    if draw.rt_count > 8 || (draw.rt_count == 0 && draw.depth == 0) {
        return false;
    }
    let s = state();
    let Some(stride) = contiguous_stride(&s, draw) else {
        return false;
    };
    let mut pending = [TargetInfo::default(); 9];
    for i in 0..9usize {
        if (i < 8 && i >= draw.rt_count as usize) || (i == 8 && draw.depth == 0) {
            continue;
        }
        let Some(handle) = bound_handle(draw, i, 8, stride) else {
            return false;
        };
        let Some(view) = s.targets.get(&(handle as usize)) else {
            return false;
        };
        if !view.whole || view.depth != (i == 8) {
            return false;
        }
        pending[i] = TargetInfo {
            identity: view.identity,
            resource: view.resource as u64,
            width: view.desc.Width,
            height: view.desc.Height,
            format: view.desc.Format.0 as u32,
            view_format: view.format,
            resource_flags: view.desc.Flags.0 as u32,
            view_flags: view.flags,
            reserved: 0,
        };
    }
    *out = pending;
    true
}
