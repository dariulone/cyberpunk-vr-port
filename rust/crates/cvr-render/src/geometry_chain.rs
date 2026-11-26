//! ONE FRAME'S WRITABLE DEPTH / G-BUFFER CHAIN (formerly src/Render/StereoGeometryChain.cpp): two-layer arrays of the
//! native geometry targets (two 10:10:10:2 colours, an 8:8:8:8 colour, a 16:16 float colour and the 32:8 depth), their
//! per-eye and both-eye RTV/DSV views; each eye imports its newly attached native targets once, binds either layer (or
//! both with a view-instanced PSO), restores the native targets and finally exports its layer back and seals.
//!
//! The caller establishes native target states, serialises recording, orders same-eye lists and exports before any
//! native reader, alias, clear or unredirected writer can consume the targets. This owns images, not geometry or
//! material admission or native command scheduling. The chain's storage is a COM object the recordings keep alive
//! (command_resources.rs).
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering::*};

use cvr_gpu::target_array::TargetArray;
use windows::core::{IUnknown_Vtbl, Interface as _, GUID, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// pass_targets::View (include/Render/NativePassTargets.hpp).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct View {
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
/// scene_state::GeometryTargetsRaw: the targets' resources (borrowed), views, handles and colour count.
#[repr(C)]
pub struct TargetsRaw {
    pub resources: [*mut c_void; 5],
    pub views: [View; 5],
    pub handles: [usize; 5],
    pub colors: u32,
}
const _: () =
    assert!(core::mem::size_of::<View>() == 48 && core::mem::size_of::<TargetsRaw>() == 328);

const FORMATS: [DXGI_FORMAT; 5] = [
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R16G16_FLOAT,
    DXGI_FORMAT_R32G8X24_TYPELESS,
];
fn color_count(colors: u32) -> bool {
    colors == 0 || colors == 3 || colors == 4
}
fn mask(colors: u32) -> u32 {
    ((1u32 << colors) - 1) | 16
}
fn writable(slot: usize) -> D3D12_RESOURCE_STATES {
    if slot == 4 {
        D3D12_RESOURCE_STATE_DEPTH_WRITE
    } else {
        D3D12_RESOURCE_STATE_RENDER_TARGET
    }
}
fn same_device(device: &ID3D12Device, child: Option<&ID3D12DeviceChild>) -> bool {
    // SAFETY: a live child.
    child.is_some_and(|c| {
        let mut d: Option<ID3D12Device> = None;
        unsafe { c.GetDevice(&mut d) }.is_ok() && d.is_some_and(|d| d.as_raw() == device.as_raw())
    })
}
fn borrowed<T: windows::core::Interface>(p: &*mut c_void) -> Option<&T> {
    // SAFETY: a live object of the caller's (or null).
    unsafe { T::from_raw_borrowed(p) }
}

#[derive(Default)]
struct NativeEye {
    resources: [Option<ID3D12Resource>; 5],
    views: [View; 5],
    handles: [usize; 5],
    colors: u32,
}
struct Inner {
    device: ID3D12Device,
    arrays: [TargetArray; 5],
    native: [NativeEye; 2],
    rtvs: ID3D12DescriptorHeap,
    dsvs: ID3D12DescriptorHeap,
    width: u32,
    height: u32,
    rtv_stride: u32,
    dsv_stride: u32,
    imported: [u32; 2],
    sealed: [bool; 2],
    private_only: [bool; 2],
    counts: [u64; 7],
}
impl Inner {
    fn rtv(&self, view: u32, slot: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        // SAFETY: the chain's heap.
        let start = unsafe { self.rtvs.GetCPUDescriptorHandleForHeapStart() }.ptr;
        D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: start + (view * 4 + slot) as usize * self.rtv_stride as usize,
        }
    }
    fn dsv(&self, view: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        // SAFETY: the chain's heap.
        let start = unsafe { self.dsvs.GetCPUDescriptorHandleForHeapStart() }.ptr;
        D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: start + view as usize * self.dsv_stride as usize,
        }
    }
    fn list_ok(&self, list: Option<&ID3D12GraphicsCommandList>) -> bool {
        // SAFETY: a live list.
        list.is_some_and(|l| {
            (unsafe { l.GetType() }) == D3D12_COMMAND_LIST_TYPE_DIRECT
                && same_device(&self.device, l.cast::<ID3D12DeviceChild>().ok().as_ref())
        })
    }
    /// A native target of `slot` for this chain: the device, a handle and view of the resource itself, and the
    /// single-view 2D shape of the slot's format.
    fn target_ok(&self, slot: usize, source: *mut c_void, view: &View, handle: usize) -> bool {
        let Some(resource) = borrowed::<ID3D12Resource>(&source) else {
            return false;
        };
        if !same_device(
            &self.device,
            resource.cast::<ID3D12DeviceChild>().ok().as_ref(),
        ) || handle == 0
            || view.identity == 0
            || view.resource != source as u64
            || view.view_flags != 0
        {
            return false;
        }
        // SAFETY: a live resource.
        let desc = unsafe { resource.GetDesc() };
        let depth_view = if slot == 4 {
            DXGI_FORMAT_D32_FLOAT_S8X24_UINT
        } else {
            FORMATS[slot]
        };
        let flag = if slot == 4 {
            D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL
        } else {
            D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
        };
        desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
            && desc.Width == self.width as u64
            && desc.Height == self.height
            && desc.DepthOrArraySize == 1
            && desc.MipLevels == 1
            && desc.SampleDesc.Count == 1
            && desc.SampleDesc.Quality == 0
            && desc.Layout == D3D12_TEXTURE_LAYOUT_UNKNOWN
            && view.width == desc.Width
            && view.height == desc.Height
            && view.format == desc.Format.0 as u32
            && view.resource_flags == desc.Flags.0 as u32
            && view.view_format == depth_view.0 as u32
            && (desc.Format == FORMATS[slot]
                || (slot == 4 && desc.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT))
            && (desc.Flags & flag) == flag
    }
}

/// The chain's storage: a COM object the recordings that use it keep alive.
#[repr(C)]
struct Storage {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    inner: UnsafeCell<Inner>,
}
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn storage_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Storage.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        storage_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn storage_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Storage.
    unsafe { (*(this as *const Storage)).refs.fetch_add(1, Relaxed) + 1 }
}
unsafe extern "system" fn storage_release(this: *mut c_void) -> u32 {
    // SAFETY: a live Storage; the last release frees it.
    unsafe {
        let n = (*(this as *const Storage)).refs.fetch_sub(1, AcqRel) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Storage));
        }
        n
    }
}
static STORAGE_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: storage_query,
    AddRef: storage_add_ref,
    Release: storage_release,
};

fn inner<'a>(storage: *mut c_void) -> Option<&'a mut Inner> {
    // SAFETY: the face's storage (one reference held), used by its one caller at a time.
    (!storage.is_null()).then(|| unsafe { &mut *(*(storage as *const Storage)).inner.get() })
}
fn keep(list: &ID3D12GraphicsCommandList, objects: &[*mut c_void]) -> bool {
    cvr_gpu::command_resources::keep(list, objects)
}

/// StereoGeometryChain::Create: the five two-layer arrays of `width` x `height` and their per-eye and both-eye views;
/// `out` gets the storage (one reference).
///
/// # Safety
/// `device` is a live ID3D12Device or null; `out` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_chain_create(
    device: *mut c_void,
    width: u32,
    height: u32,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: the caller's output.
    unsafe { *out = core::ptr::null_mut() };
    let Some(device) = borrowed::<ID3D12Device>(&device) else {
        return E_INVALIDARG;
    };
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return E_INVALIDARG;
    }
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY (whole block): the live device.
    unsafe {
        let mut heap = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: 12,
            ..Default::default()
        };
        let rtvs: ID3D12DescriptorHeap = match device.CreateDescriptorHeap(&heap) {
            Ok(h) => h,
            Err(e) => return e.code(),
        };
        heap.Type = D3D12_DESCRIPTOR_HEAP_TYPE_DSV;
        heap.NumDescriptors = 3;
        let dsvs: ID3D12DescriptorHeap = match device.CreateDescriptorHeap(&heap) {
            Ok(h) => h,
            Err(e) => return e.code(),
        };
        let mut s = Inner {
            device: device.clone(),
            arrays: Default::default(),
            native: Default::default(),
            rtvs,
            dsvs,
            width,
            height,
            rtv_stride: device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV),
            dsv_stride: device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV),
            imported: [0; 2],
            sealed: [false; 2],
            private_only: [false; 2],
            counts: [0; 7],
        };
        for slot in 0..5 {
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: width as u64,
                Height: height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Format: FORMATS[slot],
                Flags: if slot == 4 {
                    D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL
                } else {
                    D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
                },
                ..Default::default()
            };
            let hr = s.arrays[slot].initialize(device, &desc, writable(slot), None);
            if hr.is_err() {
                return hr;
            }
            let resource = s.arrays[slot].resource().cloned();
            for view in 0..3u32 {
                let first = if view == 2 { 0 } else { view };
                let size = if view == 2 { 2 } else { 1 };
                if slot == 4 {
                    let mut dsv = D3D12_DEPTH_STENCIL_VIEW_DESC {
                        Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
                        ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2DARRAY,
                        ..Default::default()
                    };
                    dsv.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_DSV {
                        MipSlice: 0,
                        FirstArraySlice: first,
                        ArraySize: size,
                    };
                    device.CreateDepthStencilView(resource.as_ref(), Some(&dsv), s.dsv(view));
                } else {
                    let mut rtv = D3D12_RENDER_TARGET_VIEW_DESC {
                        Format: FORMATS[slot],
                        ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2DARRAY,
                        ..Default::default()
                    };
                    rtv.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_RTV {
                        MipSlice: 0,
                        FirstArraySlice: first,
                        ArraySize: size,
                        PlaneSlice: 0,
                    };
                    device.CreateRenderTargetView(
                        resource.as_ref(),
                        Some(&rtv),
                        s.rtv(view, slot as u32),
                    );
                }
            }
        }
        let storage = Box::into_raw(Box::new(Storage {
            vtbl: &STORAGE_VTBL,
            refs: AtomicU32::new(1),
            inner: UnsafeCell::new(s),
        }));
        *out = storage.cast();
    }
    S_OK
}
/// # Safety
/// `storage` carries the face's reference, released once.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_chain_release(storage: *mut c_void) {
    if !storage.is_null() {
        // SAFETY: as the caller promises.
        unsafe { storage_release(storage) };
    }
}

/// PrepareEye: each newly attached native target of the eye imported once into its layer; repeated stages keep the
/// layers' contents; the targets stay RT / DEPTH_WRITE.
///
/// # Safety
/// `targets` is valid; the list live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_chain_prepare_eye(
    storage: *mut c_void,
    list: *mut c_void,
    eye: u32,
    targets: *const TargetsRaw,
) -> HRESULT {
    // SAFETY: the caller's targets.
    let (Some(s), Some(t)) = (inner(storage), unsafe { targets.as_ref() }) else {
        return E_INVALIDARG;
    };
    if eye > 1 || !color_count(t.colors) {
        return E_INVALIDARG;
    }
    let e = eye as usize;
    let list_ref = borrowed::<ID3D12GraphicsCommandList>(&list);
    if s.sealed[e] || s.private_only[e] || !s.list_ok(list_ref) {
        return E_INVALIDARG;
    }
    let m = mask(t.colors);
    for slot in 0..5 {
        if m & (1 << slot) == 0 {
            continue;
        }
        let source = t.resources[slot];
        let view = &t.views[slot];
        if !s.target_ok(slot, source, view, t.handles[slot]) {
            return E_INVALIDARG;
        }
        if s.imported[e] & (1 << slot) != 0 {
            let native = &s.native[e];
            if native.resources[slot]
                .as_ref()
                .map_or(core::ptr::null_mut(), |r| r.as_raw())
                != source
                || native.views[slot].identity != view.identity
            {
                return E_INVALIDARG;
            }
        }
        if (0..slot).any(|previous| m & (1 << previous) != 0 && t.resources[previous] == source) {
            return E_INVALIDARG;
        }
        if s.native[1 - e]
            .resources
            .iter()
            .any(|other| other.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw()) == source)
        {
            return E_INVALIDARG;
        }
    }
    // All caller inputs have been checked before recording or changing ownership.
    let list = list_ref.unwrap();
    if !keep(list, &[storage]) {
        return E_OUTOFMEMORY;
    }
    let _internal = cvr_host::probe::Internal::new();
    for slot in 0..5 {
        if m & (1 << slot) == 0 {
            continue;
        }
        let native = &mut s.native[e];
        native.resources[slot] = borrowed::<ID3D12Resource>(&t.resources[slot]).cloned();
        native.views[slot] = t.views[slot];
        native.handles[slot] = t.handles[slot];
        if s.imported[e] & (1 << slot) != 0 {
            continue;
        }
        let hr = s.arrays[slot].import_eye_layer(
            list,
            eye,
            writable(slot),
            s.native[e].resources[slot].as_ref(),
            writable(slot),
        );
        if hr.is_err() {
            return hr;
        }
        s.imported[e] |= 1 << slot;
        s.counts[e] += 1;
    }
    s.native[e].colors = t.colors;
    S_OK
}

/// ClearPrivateEye: the alternative isolated initialisation -- no native attachment, import or export.
#[no_mangle]
pub extern "C" fn cvr_geometry_chain_clear_private_eye(
    storage: *mut c_void,
    list: *mut c_void,
    eye: u32,
    colors: u32,
    depth: f32,
    stencil: u8,
) -> bool {
    let Some(s) = inner(storage) else {
        return false;
    };
    if eye > 1 || !color_count(colors) || !depth.is_finite() || !(0.0..=1.0).contains(&depth) {
        return false;
    }
    let e = eye as usize;
    let list_ref = borrowed::<ID3D12GraphicsCommandList>(&list);
    if !s.list_ok(list_ref)
        || s.imported[e] != 0
        || s.sealed[e]
        || !keep(list_ref.unwrap(), &[storage])
    {
        return false;
    }
    let list = list_ref.unwrap();
    let _internal = cvr_host::probe::Internal::new();
    let zero = [0.0f32; 4];
    // SAFETY: the chain's views on a live list.
    unsafe {
        for slot in 0..colors {
            list.ClearRenderTargetView(s.rtv(eye, slot), &zero, None);
        }
        list.ClearDepthStencilView(
            s.dsv(eye),
            D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL,
            depth,
            stencil,
            None,
        );
    }
    s.private_only[e] = true;
    s.imported[e] = mask(colors);
    true
}

/// Bind: mask 1/2 one layer with native PSOs, 3 both (a proven view-instanced PSO); no shaders, roots, viewport or
/// view mask changed.
#[no_mangle]
pub extern "C" fn cvr_geometry_chain_bind(
    storage: *mut c_void,
    list: *mut c_void,
    eye_mask: u32,
    colors: u32,
) -> bool {
    let Some(s) = inner(storage) else {
        return false;
    };
    if eye_mask == 0 || eye_mask > 3 || !color_count(colors) {
        return false;
    }
    let list_ref = borrowed::<ID3D12GraphicsCommandList>(&list);
    if !s.list_ok(list_ref) {
        return false;
    }
    for eye in 0..2 {
        if eye_mask & (1 << eye) != 0
            && (s.sealed[eye] || s.imported[eye] & mask(colors) != mask(colors))
        {
            return false;
        }
    }
    let list = list_ref.unwrap();
    if !keep(list, &[storage]) {
        return false;
    }
    let view = if eye_mask == 3 { 2 } else { eye_mask - 1 };
    let (rtv, dsv) = (s.rtv(view, 0), s.dsv(view));
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the chain's views on a live list.
    unsafe {
        list.OMSetRenderTargets(
            colors,
            (colors != 0).then_some(&rtv as *const _),
            true,
            Some(&dsv),
        )
    };
    s.counts[if eye_mask == 3 { 4 } else { view as usize + 2 }] += 1;
    true
}

/// SeedPrivateDepth: a completed depth into a private eye's depth layer.
#[no_mangle]
pub extern "C" fn cvr_geometry_chain_seed_private_depth(
    storage: *mut c_void,
    list: *mut c_void,
    eye: u32,
    source: *mut c_void,
) -> bool {
    let Some(s) = inner(storage) else {
        return false;
    };
    let Some(resource) = borrowed::<ID3D12Resource>(&source) else {
        return false;
    };
    if eye > 1 {
        return false;
    }
    let e = eye as usize;
    let list_ref = borrowed::<ID3D12GraphicsCommandList>(&list);
    if !s.private_only[e]
        || s.sealed[e]
        || !s.list_ok(list_ref)
        || !same_device(
            &s.device,
            resource.cast::<ID3D12DeviceChild>().ok().as_ref(),
        )
    {
        return false;
    }
    // SAFETY: a live resource.
    let desc = unsafe { resource.GetDesc() };
    let list = list_ref.unwrap();
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.Width != s.width as u64
        || desc.Height != s.height
        || desc.DepthOrArraySize != 1
        || desc.MipLevels != 1
        || desc.SampleDesc.Count != 1
        || (desc.Format != FORMATS[4] && desc.Format != DXGI_FORMAT_D32_FLOAT_S8X24_UINT)
        || (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL)
            != D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL
        || !keep(list, &[storage, source])
    {
        return false;
    }
    let _internal = cvr_host::probe::Internal::new();
    s.arrays[4]
        .import_eye_layer(
            list,
            eye,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
            Some(resource),
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
        )
        .is_ok()
}

/// SeedPrivateEye: a full private baseline, its colours and DEPTH_WRITE established by the caller's ordered checkpoint
/// producer; no clear or native attachment.
///
/// # Safety
/// `baseline` is valid; the list live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_chain_seed_private_eye(
    storage: *mut c_void,
    list: *mut c_void,
    eye: u32,
    baseline: *const TargetsRaw,
) -> bool {
    // SAFETY: the caller's baseline.
    let (Some(s), Some(b)) = (inner(storage), unsafe { baseline.as_ref() }) else {
        return false;
    };
    if eye > 1 || (b.colors != 3 && b.colors != 4) {
        return false;
    }
    let e = eye as usize;
    let list_ref = borrowed::<ID3D12GraphicsCommandList>(&list);
    if !s.list_ok(list_ref) || s.imported[e] != 0 || s.sealed[e] || s.private_only[e] {
        return false;
    }
    let m = mask(b.colors);
    for slot in 0..5 {
        if m & (1 << slot) == 0 {
            continue;
        }
        if !s.target_ok(slot, b.resources[slot], &b.views[slot], b.handles[slot]) {
            return false;
        }
        if (0..slot)
            .any(|previous| m & (1 << previous) != 0 && b.resources[previous] == b.resources[slot])
        {
            return false;
        }
    }
    let list = list_ref.unwrap();
    if !keep(list, &[storage]) {
        return false;
    }
    s.private_only[e] = true;
    s.native[e] = NativeEye {
        resources: core::array::from_fn(|slot| {
            borrowed::<ID3D12Resource>(&b.resources[slot]).cloned()
        }),
        views: b.views,
        handles: b.handles,
        colors: b.colors,
    };
    let _internal = cvr_host::probe::Internal::new();
    for slot in 0..5 {
        if m & (1 << slot) == 0 {
            continue;
        }
        if s.arrays[slot]
            .import_eye_layer(
                list,
                eye,
                writable(slot),
                s.native[e].resources[slot].as_ref(),
                writable(slot),
            )
            .is_err()
        {
            return false;
        }
        s.imported[e] |= 1 << slot;
        s.counts[e] += 1;
    }
    true
}

fn restore_native(
    storage: *mut c_void,
    s: &mut Inner,
    list: *mut c_void,
    eye: u32,
    colors: u32,
) -> bool {
    if eye > 1 || !color_count(colors) {
        return false;
    }
    let e = eye as usize;
    if s.private_only[e] {
        return false;
    }
    let list_ref = borrowed::<ID3D12GraphicsCommandList>(&list);
    if !s.list_ok(list_ref)
        || s.imported[e] & mask(colors) != mask(colors)
        || !keep(list_ref.unwrap(), &[storage])
    {
        return false;
    }
    let list = list_ref.unwrap();
    let native = &s.native[e];
    let handles: [D3D12_CPU_DESCRIPTOR_HANDLE; 5] =
        core::array::from_fn(|i| D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: native.handles[i],
        });
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the native targets' handles on a live list.
    unsafe {
        list.OMSetRenderTargets(
            colors,
            (colors != 0).then_some(handles.as_ptr()),
            false,
            Some(&handles[4]),
        )
    };
    true
}
/// RestoreNative: the eye's native targets bound again.
#[no_mangle]
pub extern "C" fn cvr_geometry_chain_restore_native(
    storage: *mut c_void,
    list: *mut c_void,
    eye: u32,
    colors: u32,
) -> bool {
    inner(storage).is_some_and(|s| restore_native(storage, s, list, eye, colors))
}

/// ExportEye: every attached image exported once and this eye sealed; the native states restored, the private layer
/// then COPY_SOURCE.
#[no_mangle]
pub extern "C" fn cvr_geometry_chain_export_eye(
    storage: *mut c_void,
    list: *mut c_void,
    eye: u32,
) -> bool {
    let Some(s) = inner(storage) else {
        return false;
    };
    if eye > 1 {
        return false;
    }
    let e = eye as usize;
    let list_ref = borrowed::<ID3D12GraphicsCommandList>(&list);
    if s.private_only[e]
        || s.sealed[e]
        || s.imported[e] & 16 == 0
        || !s.list_ok(list_ref)
        || !keep(list_ref.unwrap(), &[storage])
    {
        return false;
    }
    let list = list_ref.unwrap();
    let _internal = cvr_host::probe::Internal::new();
    for slot in 0..5 {
        if s.imported[e] & (1 << slot) == 0 {
            continue;
        }
        let target = s.native[e].resources[slot].clone();
        if s.arrays[slot]
            .copy_eye_layer_uniform(
                list,
                eye,
                writable(slot),
                target.as_ref(),
                writable(slot),
                writable(slot),
            )
            .is_err()
            || s.arrays[slot]
                .transition_eye(list, eye, writable(slot), D3D12_RESOURCE_STATE_COPY_SOURCE)
                .is_err()
        {
            return false;
        }
        s.counts[5 + e] += 1;
    }
    s.sealed[e] = true;
    let colors = s.native[e].colors;
    restore_native(storage, s, list.as_raw(), eye, colors)
}

/// Resource: the four colour arrays, the depth array in slot 4 (borrowed).
#[no_mangle]
pub extern "C" fn cvr_geometry_chain_resource(storage: *mut c_void, slot: u32) -> *mut c_void {
    inner(storage)
        .filter(|_| slot < 5)
        .and_then(|s| s.arrays[slot as usize].resource().map(|r| r.as_raw()))
        .unwrap_or(core::ptr::null_mut())
}
/// Counts: attachment imports 0/1, single-eye binds 0/1, both-eye binds, attachment exports 0/1.
///
/// # Safety
/// `out` holds seven values.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_chain_counts(storage: *mut c_void, out: *mut [u64; 7]) {
    // SAFETY: the caller's output.
    unsafe { *out = inner(storage).map_or([0; 7], |s| s.counts) };
}
