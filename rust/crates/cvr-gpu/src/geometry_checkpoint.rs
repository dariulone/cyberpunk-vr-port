//! Single pass's geometry target checkpoint (formerly src/Render/GeometryTargetCheckpoint.cpp;
//! include/Render/GeometryTargetCheckpoint.hpp calls it): private copies of a native draw's G-buffer colors (none, 3 or
//! 4 of 10:10:10:2, 10:10:10:2, 8:8:8:8, 16:16 float) and its D32S8 depth, of the exact same shape, recorded once into
//! a direct list. The caller proves the native RT/DEPTH_WRITE states and the one-shot submission; captured means
//! recorded, not GPU-complete. The list's recording keeps the copies, the native sources and the views' heaps alive
//! until the GPU has run it.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::scene_state::{GeometryTargetsRaw, TargetInfo};
use core::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, E_UNEXPECTED};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const FORMATS: [DXGI_FORMAT; 5] = [
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R16G16_FLOAT,
    DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
];
static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);
fn identity() -> u64 {
    NEXT_IDENTITY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            (id != u64::MAX).then(|| id + 1)
        })
        .unwrap_or(0)
}
fn selected(slot: usize, colors: u32) -> bool {
    slot == 4 || slot < colors as usize
}
fn writable(slot: usize) -> D3D12_RESOURCE_STATES {
    if slot == 4 {
        D3D12_RESOURCE_STATE_DEPTH_WRITE
    } else {
        D3D12_RESOURCE_STATE_RENDER_TARGET
    }
}
fn target_flag(slot: usize) -> D3D12_RESOURCE_FLAGS {
    if slot == 4 {
        D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL
    } else {
        D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
    }
}
fn same_device<T: Interface>(device: &ID3D12Device, child: &T) -> bool {
    let Ok(child) = child.cast::<ID3D12DeviceChild>() else {
        return false;
    };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live device child writing its device.
    unsafe { child.GetDevice(&mut actual) }.is_ok()
        && actual.is_some_and(|a| a.as_raw() == device.as_raw())
}
unsafe fn borrowed(raw: &usize) -> Option<&ID3D12Resource> {
    ID3D12Resource::from_raw_borrowed(&*(raw as *const usize as *const *mut c_void))
}
/// The targets' exact shapes when they are one device's distinct whole 2D targets of one size, as their views say.
unsafe fn validate(
    device: &ID3D12Device,
    targets: &GeometryTargetsRaw,
) -> Option<[D3D12_RESOURCE_DESC; 5]> {
    if !matches!(targets.colors, 0 | 3 | 4) {
        return None;
    }
    let mut descs = [D3D12_RESOURCE_DESC::default(); 5];
    let mut size: Option<(u64, u32)> = None;
    for slot in 0..5usize {
        if !selected(slot, targets.colors) {
            continue;
        }
        let source = borrowed(&targets.resources[slot])?;
        let view = &targets.views[slot];
        if !same_device(device, source)
            || targets.handles[slot] == 0
            || view.identity == 0
            || view.resource != targets.resources[slot] as u64
            || view.view_flags != 0
        {
            return None;
        }
        let d = source.GetDesc();
        let format_ok =
            d.Format == FORMATS[slot] || (slot == 4 && d.Format == DXGI_FORMAT_R32G8X24_TYPELESS);
        if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
            || d.Width == 0
            || d.Height == 0
            || d.Width > 4096
            || d.Height > 4096
            || d.DepthOrArraySize != 1
            || d.MipLevels != 1
            || d.SampleDesc.Count != 1
            || d.SampleDesc.Quality != 0
            || d.Layout != D3D12_TEXTURE_LAYOUT_UNKNOWN
            || view.width != d.Width
            || view.height != d.Height
            || view.format != d.Format.0 as u32
            || view.resource_flags != d.Flags.0 as u32
            || view.view_format != FORMATS[slot].0 as u32
            || !format_ok
            || d.Flags.0 & target_flag(slot).0 == 0
        {
            return None;
        }
        if size.is_some_and(|s| s != (d.Width, d.Height)) {
            return None;
        }
        size = Some((d.Width, d.Height));
        if (0..slot)
            .any(|p| selected(p, targets.colors) && targets.resources[p] == targets.resources[slot])
        {
            return None;
        }
        descs[slot] = d;
    }
    Some(descs)
}
fn transition(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

pub struct Checkpoint {
    device: ID3D12Device,
    shape: [D3D12_RESOURCE_DESC; 5],
    resources: [Option<ID3D12Resource>; 5],
    views: [TargetInfo; 5],
    handles: [usize; 5],
    colors: u32,
    native: [Option<ID3D12Resource>; 5],
    rtvs: Option<ID3D12DescriptorHeap>,
    dsv: Option<ID3D12DescriptorHeap>,
    captured: bool,
}
impl Checkpoint {
    /// Private targets of `shape`'s exact shapes, in the native writable states.
    ///
    /// # Safety
    /// `shape`'s resources are live (borrowed).
    pub unsafe fn create(
        device: &ID3D12Device,
        shape: &GeometryTargetsRaw,
    ) -> Result<Checkpoint, HRESULT> {
        let descs = validate(device, shape).ok_or(E_INVALIDARG)?;
        let _internal = cvr_host::probe::Internal::new();
        let rtvs: Option<ID3D12DescriptorHeap> = if shape.colors != 0 {
            let desc = D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: shape.colors,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
                NodeMask: 0,
            };
            Some(device.CreateDescriptorHeap(&desc).map_err(|e| e.code())?)
        } else {
            None
        };
        let depth_heap = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            NumDescriptors: 1,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        };
        let dsv: ID3D12DescriptorHeap = device
            .CreateDescriptorHeap(&depth_heap)
            .map_err(|e| e.code())?;
        let memory = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let stride =
            device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize;
        let mut checkpoint = Checkpoint {
            device: device.clone(),
            shape: descs,
            resources: Default::default(),
            views: [TargetInfo::default(); 5],
            handles: [0; 5],
            colors: shape.colors,
            native: Default::default(),
            rtvs: None,
            dsv: None,
            captured: false,
        };
        for slot in 0..5usize {
            if !selected(slot, shape.colors) {
                continue;
            }
            let mut desc = descs[slot];
            desc.Alignment = 0;
            desc.Flags = target_flag(slot);
            let mut resource: Option<ID3D12Resource> = None;
            device
                .CreateCommittedResource(
                    &memory,
                    D3D12_HEAP_FLAG_NONE,
                    &desc,
                    writable(slot),
                    None,
                    &mut resource,
                )
                .map_err(|e| e.code())?;
            let resource = resource.ok_or(E_UNEXPECTED)?;
            let handle = if slot == 4 {
                let handle = dsv.GetCPUDescriptorHandleForHeapStart();
                let view = D3D12_DEPTH_STENCIL_VIEW_DESC {
                    Format: FORMATS[slot],
                    ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                };
                device.CreateDepthStencilView(&resource, Some(&view), handle);
                handle
            } else {
                let start = rtvs
                    .as_ref()
                    .expect("the color heap")
                    .GetCPUDescriptorHandleForHeapStart();
                let handle = D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: start.ptr + slot * stride,
                };
                let view = D3D12_RENDER_TARGET_VIEW_DESC {
                    Format: FORMATS[slot],
                    ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                };
                device.CreateRenderTargetView(&resource, Some(&view), handle);
                handle
            };
            let id = identity();
            if id == 0 {
                return Err(E_UNEXPECTED);
            }
            checkpoint.views[slot] = TargetInfo {
                identity: id,
                resource: resource.as_raw() as u64,
                width: desc.Width,
                height: desc.Height,
                format: desc.Format.0 as u32,
                view_format: FORMATS[slot].0 as u32,
                resource_flags: desc.Flags.0 as u32,
                view_flags: 0,
                reserved: 0,
            };
            checkpoint.handles[slot] = handle.ptr;
            checkpoint.resources[slot] = Some(resource);
        }
        checkpoint.rtvs = rtvs;
        checkpoint.dsv = Some(dsv);
        Ok(checkpoint)
    }
    /// One recording only: the native targets (of the created shapes, none of them a private copy) copied into the
    /// private ones, both returned to their writable states.
    ///
    /// # Safety
    /// `native`'s resources are live (borrowed); `list` is open.
    pub unsafe fn capture(
        &mut self,
        list: &ID3D12GraphicsCommandList,
        native: &GeometryTargetsRaw,
    ) -> bool {
        if self.captured
            || list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT
            || !same_device(&self.device, list)
        {
            return false;
        }
        if native.colors != self.colors {
            return false;
        }
        let Some(descs) = validate(&self.device, native) else {
            return false;
        };
        for slot in 0..5usize {
            if !selected(slot, native.colors) {
                continue;
            }
            let (expected, actual) = (&self.shape[slot], &descs[slot]);
            if actual.Width != expected.Width
                || actual.Height != expected.Height
                || actual.Format != expected.Format
                || actual.Flags != expected.Flags
            {
                return false;
            }
            if self
                .resources
                .iter()
                .flatten()
                .any(|d| d.as_raw() as usize == native.resources[slot])
            {
                return false;
            }
        }
        let mut kept: Vec<*mut c_void> = Vec::with_capacity(12);
        for slot in 0..5usize {
            if selected(slot, native.colors) {
                kept.push(native.resources[slot] as *mut c_void);
            }
        }
        kept.extend(self.resources.iter().flatten().map(|r| r.as_raw()));
        kept.extend(self.rtvs.iter().chain(self.dsv.iter()).map(|h| h.as_raw()));
        if !crate::command_resources::keep(list, &kept) {
            return false;
        }
        for slot in 0..5usize {
            self.native[slot] = if selected(slot, native.colors) {
                borrowed(&native.resources[slot]).cloned()
            } else {
                None
            };
        }
        let mut barriers = Vec::with_capacity(10);
        for slot in 0..5usize {
            if let (Some(source), Some(destination)) =
                (self.native[slot].as_ref(), self.resources[slot].as_ref())
            {
                barriers.push(transition(
                    source,
                    writable(slot),
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                ));
                barriers.push(transition(
                    destination,
                    writable(slot),
                    D3D12_RESOURCE_STATE_COPY_DEST,
                ));
            }
        }
        let _internal = cvr_host::probe::Internal::new();
        list.ResourceBarrier(&barriers);
        for slot in 0..5usize {
            if let (Some(source), Some(destination)) =
                (self.native[slot].as_ref(), self.resources[slot].as_ref())
            {
                list.CopyResource(destination, source);
            }
        }
        let mut back = Vec::with_capacity(10);
        for slot in 0..5usize {
            if let (Some(source), Some(destination)) =
                (self.native[slot].as_ref(), self.resources[slot].as_ref())
            {
                back.push(transition(
                    source,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                    writable(slot),
                ));
                back.push(transition(
                    destination,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    writable(slot),
                ));
            }
        }
        list.ResourceBarrier(&back);
        self.captured = true;
        true
    }
    pub fn captured(&self) -> bool {
        self.captured
    }
    /// The private targets as owned references, once captured.
    pub fn targets(&self) -> Option<GeometryTargetsRaw> {
        if !self.captured {
            return None;
        }
        Some(GeometryTargetsRaw {
            resources: self
                .resources
                .clone()
                .map(|r| r.map_or(0, |r| r.into_raw() as usize)),
            views: self.views,
            handles: self.handles,
            colors: self.colors,
        })
    }
}

/// # Safety
/// `device` is null or live; `shape` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_checkpoint_create(
    device: *mut c_void,
    shape: *const GeometryTargetsRaw,
    out: *mut *mut Checkpoint,
) -> HRESULT {
    *out = core::ptr::null_mut();
    let Some(device) = ID3D12Device::from_raw_borrowed(&device) else {
        return E_INVALIDARG;
    };
    match Checkpoint::create(device, &*shape) {
        Ok(checkpoint) => {
            *out = Box::into_raw(Box::new(checkpoint));
            HRESULT(0)
        }
        Err(hr) => hr,
    }
}
/// # Safety
/// `checkpoint` came from create; `list` is null or open; `native` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_checkpoint_capture(
    checkpoint: *mut Checkpoint,
    list: *mut c_void,
    native: *const GeometryTargetsRaw,
) -> bool {
    let (Some(checkpoint), Some(list)) = (
        checkpoint.as_mut(),
        ID3D12GraphicsCommandList::from_raw_borrowed(&list),
    ) else {
        return false;
    };
    checkpoint.capture(list, &*native)
}
/// # Safety
/// `checkpoint` came from create; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_checkpoint_targets(
    checkpoint: *const Checkpoint,
    out: *mut GeometryTargetsRaw,
) -> bool {
    match checkpoint.as_ref().and_then(Checkpoint::targets) {
        Some(targets) => {
            *out = targets;
            true
        }
        None => false,
    }
}
/// # Safety
/// `checkpoint` came from create (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_checkpoint_free(checkpoint: *mut Checkpoint) {
    if !checkpoint.is_null() {
        drop(Box::from_raw(checkpoint));
    }
}
