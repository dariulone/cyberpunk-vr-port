//! The optics probe (formerly src/Render/OpticsProbe.cpp; the file keeps the exports, the C++ names as forwarders and
//! hands a requested texture's lease over): one texture of the frame read back for the diagnostics.
//!
//! Two ways in. A request (VRPhysicalOpticsProbe: a world widget's texture or the optics camera's render target, as a
//! HUD texture lease) is copied on the next HUD capture's queue -- once the engine's pool still holds that resource in
//! its shader-read state -- and read once its fence passes. A stage (CyberpunkVR_OpticsProbeStage: 1 the vrcam view,
//! 2 MAIN, 3 the second eye's view) takes the render target the scope node binds, follows its barriers on that list,
//! and copies it inline at the node's end; the fence is signalled on the next capture. The pixels (rows without
//! padding) and their size and format are published through the exports; the state reads 1 requested, 2 copying,
//! 3 done, 4 failed.
#![allow(unsafe_op_in_unsafe_fn)]
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The exports and the lease release (src/Render/OpticsProbe.cpp attaches them).
#[repr(C)]
pub struct Runtime {
    pub state: *const AtomicU32,
    pub width: *mut u32,
    pub height: *mut u32,
    pub pixels: *mut usize,
    pub format: *mut u32,
    pub stage: *const AtomicU32,
    /// Gives back a lease handed over with a request (a held std::shared_ptr<cvr::hud::TextureLease>).
    pub lease_release: unsafe extern "C" fn(*mut c_void),
}
/// The plugin's table, built here (formerly the C++ half's); a test program attaches its own.
#[cfg(feature = "plugin-tables")]
mod plugin {
    use core::ffi::c_void;
    use core::sync::atomic::AtomicU32;
    unsafe extern "C" {
        static CyberpunkVR_OpticsProbeState: AtomicU32;
        static mut CyberpunkVR_OpticsProbeWidth: u32;
        static mut CyberpunkVR_OpticsProbeHeight: u32;
        static mut CyberpunkVR_OpticsProbePixels: usize;
        static mut CyberpunkVR_OpticsProbeFormat: u32;
        static CyberpunkVR_OpticsProbeStage: AtomicU32;
        /// delete std::shared_ptr<cvr::hud::TextureLease>* (src/Render/HudPanel.cpp).
        fn cvr_cpp_texture_lease_release(lease: *mut c_void);
    }
    // SAFETY: process-lifetime exports and a plain function.
    unsafe impl Sync for super::Runtime {}
    pub(super) static RT: super::Runtime = super::Runtime {
        state: &raw const CyberpunkVR_OpticsProbeState,
        width: &raw mut CyberpunkVR_OpticsProbeWidth,
        height: &raw mut CyberpunkVR_OpticsProbeHeight,
        pixels: &raw mut CyberpunkVR_OpticsProbePixels,
        format: &raw mut CyberpunkVR_OpticsProbeFormat,
        stage: &raw const CyberpunkVR_OpticsProbeStage,
        lease_release: cvr_cpp_texture_lease_release,
    };
}
#[cfg(feature = "plugin-tables")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "plugin-tables"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
#[no_mangle]
pub extern "C" fn cvr_optics_probe_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Release);
}
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached at the C++ half's static initialisation, for the process.
    unsafe { RUNTIME.load(Acquire).as_ref() }
}
fn set_state(value: u32) {
    if let Some(rt) = runtime() {
        // SAFETY: an export of the C++ half.
        unsafe { (*rt.state).store(value, Release) };
    }
}
fn state() -> u32 {
    // SAFETY: as above.
    runtime().map_or(0, |rt| unsafe { (*rt.state).load(Acquire) })
}
fn stage() -> u32 {
    // SAFETY: as above.
    runtime().map_or(0, |rt| unsafe { (*rt.stage).load(Relaxed) })
}

/// The texture read: a request's lease (its engine pool handle) or a staged render target (no lease).
struct Texture {
    lease: *mut c_void,
    handle: u32,
    resource: ID3D12Resource,
}
impl Drop for Texture {
    fn drop(&mut self) {
        if !self.lease.is_null() {
            if let Some(rt) = runtime() {
                // SAFETY: a lease handed over once with the request.
                unsafe { (rt.lease_release)(self.lease) };
            }
        }
    }
}
struct Pending {
    texture: Texture,
    allocator: Option<ID3D12CommandAllocator>,
    commands: Option<ID3D12GraphicsCommandList>,
    readback: Option<ID3D12Resource>,
    fence: Option<ID3D12Fence>,
    layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    bytes: u64,
    width: u32,
    height: u32,
    pixel_bytes: u32,
    submitted: bool,
    fence_pending: bool,
    format: DXGI_FORMAT,
    source_state: D3D12_RESOURCE_STATES,
}
impl Pending {
    fn new(texture: Texture) -> Pending {
        Pending {
            texture,
            allocator: None,
            commands: None,
            readback: None,
            fence: None,
            layout: Default::default(),
            bytes: 0,
            width: 0,
            height: 0,
            pixel_bytes: 4,
            submitted: false,
            fence_pending: false,
            format: DXGI_FORMAT_UNKNOWN,
            source_state: D3D12_RESOURCE_STATES(0xC0),
        }
    }
}
struct Probe {
    pending: Option<Pending>,
    pixels: Vec<u8>,
}
// SAFETY: D3D12 objects are free-threaded; the probe is used under PROBE's lock.
unsafe impl Send for Probe {}
static PROBE: Mutex<Probe> = Mutex::new(Probe {
    pending: None,
    pixels: Vec::new(),
});

const READS: i32 =
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0 | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0;

thread_local! {
    static STAGE_RESOURCE: Cell<usize> = const { Cell::new(0) };
    static STAGE_LIST: Cell<usize> = const { Cell::new(0) };
    static STAGE_STATE: Cell<i32> = const { Cell::new(0) };
}

/// The engine's pool still holds the request's resource under its handle, in the shader-read state.
fn native_source_still_readable(texture: &Texture) -> bool {
    let base = cvr_engine::code::image_base();
    let Some(pool) = cvr_engine::code::read::<usize>(base + 0x3bcdca8) else {
        return false;
    };
    if pool == 0 || texture.handle == 0 || texture.handle > 131072 {
        return false;
    }
    let entry = pool + 0xb0 * (texture.handle as usize - 1);
    cvr_engine::code::read::<usize>(entry + 0x2f1d8) == Some(texture.resource.as_raw() as usize)
        && cvr_engine::code::read::<u32>(entry + 0x2f220) == Some(READS as u32)
}

fn barrier(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    let b = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: a borrowed interface pointer, not released by the barrier.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    };
    // SAFETY: a recording list.
    unsafe { list.ResourceBarrier(&[b]) };
}

/// The copy into a readback recorded on `inline_list`, or on a list of its own (closed).
unsafe fn record(
    p: &mut Pending,
    device: &ID3D12Device,
    inline_list: Option<&ID3D12GraphicsCommandList>,
) -> bool {
    let desc = p.texture.resource.GetDesc();
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.DepthOrArraySize != 1
        || desc.MipLevels != 1
        || desc.SampleDesc.Count != 1
        || desc.Width == 0
        || desc.Width > 4096
        || desc.Height == 0
        || desc.Height > 4096
        || !matches!(
            desc.Format,
            DXGI_FORMAT_R8G8B8A8_UNORM
                | DXGI_FORMAT_R8G8B8A8_TYPELESS
                | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
                | DXGI_FORMAT_R11G11B10_FLOAT
                | DXGI_FORMAT_R16G16B16A16_FLOAT
                | DXGI_FORMAT_R16G16B16A16_TYPELESS
        )
    {
        return false;
    }
    p.format = desc.Format;
    if matches!(
        desc.Format,
        DXGI_FORMAT_R16G16B16A16_FLOAT | DXGI_FORMAT_R16G16B16A16_TYPELESS
    ) {
        p.pixel_bytes = 8;
    }
    p.width = desc.Width as u32;
    p.height = desc.Height;
    device.GetCopyableFootprints(
        &desc,
        0,
        1,
        0,
        Some(&mut p.layout),
        None,
        None,
        Some(&mut p.bytes),
    );
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    let buffer = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: p.bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut readback: Option<ID3D12Resource> = None;
    if device
        .CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &buffer,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut readback,
        )
        .is_err()
    {
        return false;
    }
    p.readback = readback;
    match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
        Ok(f) => p.fence = Some(f),
        Err(_) => return false,
    }
    if inline_list.is_none() {
        let Ok(allocator) =
            device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
        else {
            return false;
        };
        let Ok(commands) = device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &allocator,
            None,
        ) else {
            return false;
        };
        p.allocator = Some(allocator);
        p.commands = Some(commands);
    }
    let Some(list) = inline_list.cloned().or_else(|| p.commands.clone()) else {
        return false;
    };
    let Some(readback) = p.readback.as_ref() else {
        return false;
    };
    let resource = &p.texture.resource;
    barrier(
        &list,
        resource,
        p.source_state,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
    );
    let from = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute_copy(resource),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let to = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute_copy(readback),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: p.layout,
        },
    };
    list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
    barrier(
        &list,
        resource,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
        p.source_state,
    );
    inline_list.is_some() || p.commands.as_ref().is_some_and(|c| c.Close().is_ok())
}

/// StageRequested.
#[no_mangle]
pub extern "C" fn cvr_optics_probe_stage_requested() -> bool {
    stage() != 0
}
/// StageView: the view the requested stage reads (1 vrcam, 2 MAIN, 3 the second eye's view).
#[no_mangle]
pub extern "C" fn cvr_optics_probe_stage_view(key: u64, eye_key: u64) -> bool {
    let s = stage();
    (s == 1 && key == 0x1e3f0afde930b6b1)
        || (s == 2 && key == 0)
        || (s == 3 && eye_key != 0 && key == eye_key)
}
/// StageTarget: the render target the scope node binds on `list`.
#[no_mangle]
pub extern "C" fn cvr_optics_probe_stage_target(list: *mut c_void, resource: *mut c_void) {
    if stage() == 0 || resource.is_null() {
        return;
    }
    STAGE_RESOURCE.with(|c| c.set(resource as usize));
    STAGE_LIST.with(|c| c.set(list as usize));
    STAGE_STATE.with(|c| c.set(D3D12_RESOURCE_STATE_RENDER_TARGET.0));
}
/// StageBarriers: the staged target's state followed through the list's barriers.
///
/// # Safety
/// `barriers` holds `count` barriers or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_probe_stage_barriers(
    list: *mut c_void,
    count: u32,
    barriers: *const D3D12_RESOURCE_BARRIER,
) {
    let resource = STAGE_RESOURCE.with(|c| c.get());
    if resource == 0 || list as usize != STAGE_LIST.with(|c| c.get()) || barriers.is_null() {
        return;
    }
    for b in core::slice::from_raw_parts(barriers, count as usize) {
        if b.Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION {
            let t = &b.Anonymous.Transition;
            let target: usize = core::mem::transmute_copy(&t.pResource);
            if target == resource {
                STAGE_STATE.with(|c| c.set(t.StateAfter.0));
            }
        }
    }
}
/// StageEnd: the staged target copied inline at the scope node's end.
///
/// # Safety
/// `device` is the game's device or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_probe_stage_end(device: *mut c_void) {
    let source = STAGE_RESOURCE.with(|c| c.replace(0));
    let list = STAGE_LIST.with(|c| c.replace(0));
    let state = STAGE_STATE.with(|c| c.get());
    let source_ptr = source as *mut c_void;
    let list_ptr = list as *mut c_void;
    let (Some(resource), Some(list), Some(device)) = (
        ID3D12Resource::from_raw_borrowed(&source_ptr),
        ID3D12GraphicsCommandList::from_raw_borrowed(&list_ptr),
        ID3D12Device::from_raw_borrowed(&device),
    ) else {
        return;
    };
    if stage() == 0 {
        return;
    }
    let Ok(mut probe) = PROBE.lock() else { return };
    if probe.pending.is_some() {
        return;
    }
    let mut p = Pending::new(Texture {
        lease: core::ptr::null_mut(),
        handle: 0,
        resource: resource.clone(),
    });
    p.source_state = D3D12_RESOURCE_STATES(state);
    if !record(&mut p, device, Some(list)) {
        set_state(4);
    } else {
        p.submitted = true;
        p.fence_pending = true;
        probe.pending = Some(p);
        set_state(2);
    }
    if let Some(rt) = runtime() {
        (*rt.stage).store(0, Relaxed);
    }
}
/// Request: a HUD texture lease to read (its pool handle and resource); false when one is pending already. The lease
/// is the probe's from here on, given back when it is done with it (or at once when refused).
///
/// # Safety
/// `lease` is a lease handed over once, `resource` its live resource.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_probe_request(
    lease: *mut c_void,
    resource: *mut c_void,
    handle: u32,
) -> bool {
    let Some(resource) = ID3D12Resource::from_raw_borrowed(&resource).cloned() else {
        if let (Some(rt), false) = (runtime(), lease.is_null()) {
            (rt.lease_release)(lease);
        }
        return false;
    };
    let texture = Texture {
        lease,
        handle,
        resource,
    };
    let Ok(mut probe) = PROBE.lock() else {
        return false;
    };
    if probe.pending.is_some() {
        return false;
    }
    probe.pending = Some(Pending::new(texture));
    set_state(1);
    true
}
/// Capture: the pending read advanced on the HUD's queue -- the staged copy's fence signalled, a request's copy
/// recorded and submitted, or a finished copy's pixels published.
///
/// # Safety
/// `device` and `queue` are live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_probe_capture(device: *mut c_void, queue: *mut c_void) {
    let (Some(device), Some(queue)) = (
        ID3D12Device::from_raw_borrowed(&device),
        ID3D12CommandQueue::from_raw_borrowed(&queue),
    ) else {
        return;
    };
    if state() == 0 {
        return;
    }
    let Ok(mut guard) = PROBE.lock() else { return };
    let probe = &mut *guard;
    let Some(p) = probe.pending.as_mut() else {
        return;
    };
    let direct = queue.GetDesc().Type == D3D12_COMMAND_LIST_TYPE_DIRECT;
    if p.fence_pending {
        if !direct {
            return;
        }
        if p.fence.as_ref().is_none_or(|f| queue.Signal(f, 1).is_err()) {
            set_state(4);
            return;
        }
        p.fence_pending = false;
        return;
    }
    if !p.submitted {
        if !direct || !native_source_still_readable(&p.texture) || !record(p, device, None) {
            probe.pending = None;
            set_state(4);
            return;
        }
        if let Some(commands) = p.commands.as_ref() {
            queue.ExecuteCommandLists(&[Some(
                commands
                    .cast::<ID3D12CommandList>()
                    .unwrap_or_else(|_| unreachable!()),
            )]);
        }
        p.submitted = true;
        if p.fence.as_ref().is_none_or(|f| queue.Signal(f, 1).is_err()) {
            // A submitted list must retain its storage even if signalling fails.
            set_state(4);
            return;
        }
        set_state(2);
        return;
    }
    let done = p.fence.as_ref().map_or(0, |f| f.GetCompletedValue());
    if done == u64::MAX {
        probe.pending = None;
        set_state(4);
        return;
    }
    if done < 1 {
        return;
    }
    let Some(readback) = p.readback.clone() else {
        return;
    };
    let range = D3D12_RANGE {
        Begin: 0,
        End: p.bytes as usize,
    };
    let mut data: *mut c_void = core::ptr::null_mut();
    if readback.Map(0, Some(&range), Some(&mut data)).is_err() {
        probe.pending = None;
        set_state(4);
        return;
    }
    let row = p.width as usize * p.pixel_bytes as usize;
    probe.pixels.resize(row * p.height as usize, 0);
    for y in 0..p.height as usize {
        let src = (data as *const u8)
            .add(p.layout.Offset as usize + y * p.layout.Footprint.RowPitch as usize);
        core::ptr::copy_nonoverlapping(src, probe.pixels.as_mut_ptr().add(y * row), row);
    }
    readback.Unmap(0, Some(&D3D12_RANGE::default()));
    if let Some(rt) = runtime() {
        *rt.width = p.width;
        *rt.height = p.height;
        *rt.format = p.format.0 as u32;
        *rt.pixels = probe.pixels.as_ptr() as usize;
    }
    probe.pending = None;
    set_state(3);
}
