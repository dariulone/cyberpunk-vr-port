//! The hidden-area mask in the frame (formerly the runtime of src/Render/HiddenArea.cpp; include/Render/HiddenArea.hpp
//! calls it; docs/hidden-area-mask-20261009.md).
//!
//! ClearDepthStencilView restricted to rectangles is the whole mechanism; it changes no pipeline, root signature,
//! target or barrier of the engine's command list, so nothing has to be restored. While geometry is drawn the hidden
//! cells hold the near plane (reverse-Z, GREATER_EQUAL), so no draw lands there. Whenever the scene depth leaves
//! DEPTH_WRITE they get the far plane instead: the passes that read it then see sky, which they skip on their own.
//! That reset is where the saving comes from; the near plane alone saves nothing, because the dominant G-buffer
//! materials discard and export SV_Depth and so still run their pixel shaders.
//!
//! The C++ half keeps the exports and what reads the rest of the port (the eye's frustum from OpenXR and the
//! asymmetric projection, the menu mode, engine stereo, the render target's aspect) and attaches both at load.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::FastMap;
use crate::hidden_area::{self, Fov, Rect};
use core::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const CELL: u32 = 8;
const MARGIN: u32 = 1;
const MAX_RECTS_PER_CLEAR: usize = 64;
/// Patched shaders skip the pixels outside a slightly larger ellipse, so whatever they leave out lies inside the
/// hidden area with this much of the radius to spare for filters reading neighbours.
const SHADER_MARGIN: f32 = 0.02;

/// One distinct native depth clear (CyberpunkVR_HiddenAreaClearLog).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ClearRecord {
    pub node: u32,
    pub side: i32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub flags: u32,
    pub depth: f32,
    pub count: u32,
    pub masked: u32,
}
/// One distinct barrier on a masked scene depth (CyberpunkVR_HiddenAreaBarrierLog); `before`/`after` are resource
/// states, or barrier layouts with the top bit set for enhanced barriers.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BarrierRecord {
    pub node: u32,
    pub side: i32,
    pub before: u32,
    pub after: u32,
    pub subresource: u32,
    pub flags: u32,
    pub count: u32,
    pub resets: u32,
}
const _: () = assert!(
    core::mem::size_of::<ClearRecord>() == 36 && core::mem::size_of::<BarrierRecord>() == 32
);

/// The C++ half: the exports and the reads of the rest of the port.
#[repr(C)]
pub struct Runtime {
    pub enabled: *const AtomicU32,
    pub radius: *const AtomicU32,
    pub node: *const AtomicU32,
    pub log: *const AtomicU32,
    pub clear_log: *mut ClearRecord,
    pub masked: *const AtomicU64,
    pub sky: *const AtomicU32,
    pub sky_node: *const AtomicU32,
    pub barrier_log: *mut BarrierRecord,
    pub sky_resets: *const AtomicU64,
    pub near_resets: *const AtomicU64,
    /// The eye's runtime FOV and the window the engine renders for it, in tangents (side 0 MAIN).
    pub frustum: unsafe extern "C" fn(side: i32, fov: *mut Fov, rendered: *mut Fov) -> bool,
    pub menu_mode: unsafe extern "C" fn() -> bool,
    pub engine_stereo: unsafe extern "C" fn() -> bool,
    /// The render target's aspect (the launcher's width over height, 1 when unknown).
    pub aspect: unsafe extern "C" fn() -> f32,
}
// SAFETY: pointers to process-lifetime exports and plain functions.
unsafe impl Sync for Runtime {}
/// The plugin's table is built in Rust (hidden_area_table.rs); a test program attaches its own.
#[cfg(feature = "rust-build")]
static RUNTIME: core::sync::atomic::AtomicPtr<Runtime> =
    core::sync::atomic::AtomicPtr::new((&raw const crate::hidden_area_table::RT).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: core::sync::atomic::AtomicPtr<Runtime> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `runtime` and everything it points to live for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Ordering::Release);
}
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: the attached runtime.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}
fn load(at: *const AtomicU32) -> u32 {
    // SAFETY: an attached export.
    unsafe { at.as_ref() }.map_or(0, |a| a.load(Ordering::Relaxed))
}
fn enabled(r: &Runtime) -> bool {
    load(r.enabled) != 0
}
fn radius(r: &Runtime) -> f32 {
    f32::from_bits(load(r.radius))
}
fn bump(counters: *const AtomicU64, side: usize) {
    // SAFETY: an attached two-counter export.
    if let Some(c) = unsafe { counters.add(side).as_ref() } {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
struct Target {
    format: u32,
    width: u32,
    height: u32,
    /// Identity only, not owned.
    resource: usize,
}
struct Cache {
    width: u32,
    height: u32,
    radius: f32,
    rendered: Fov,
    fov: Fov,
    rects: Arc<Vec<Rect>>,
}
/// The shader ellipse of an off-centre window is a fit (shader_ellipse_scales), kept until its inputs change.
#[derive(Clone, Copy)]
struct EllipseCache {
    radius: f32,
    rendered: Fov,
    fov: Fov,
    scales: Option<[f32; 4]>,
    set: bool,
}
const UNSET: Fov = Fov {
    left: -1.0,
    right: 1.0,
    up: 1.0,
    down: -1.0,
};
impl Default for EllipseCache {
    fn default() -> Self {
        EllipseCache {
            radius: 0.0,
            rendered: UNSET,
            fov: UNSET,
            scales: None,
            set: false,
        }
    }
}
#[derive(Default)]
struct Rects {
    cache: [Option<Cache>; 2],
    ellipse: [EllipseCache; 2],
}
static RECTS: Mutex<Option<Rects>> = Mutex::new(None);
static LOG: Mutex<()> = Mutex::new(());
static DSVS: Mutex<Option<FastMap<usize, Target>>> = Mutex::new(None);
static LAST_RECTS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static LAST_WIDTH: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static LAST_HEIGHT: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static LAST_HIDDEN: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
fn lock<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn same_fov(a: &Fov, b: &Fov) -> bool {
    a.left == b.left && a.right == b.right && a.up == b.up && a.down == b.down
}
fn scene_depth_format(f: u32) -> bool {
    [
        DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
        DXGI_FORMAT_R32G8X24_TYPELESS,
        DXGI_FORMAT_D32_FLOAT,
        DXGI_FORMAT_R32_TYPELESS,
        DXGI_FORMAT_R32_FLOAT,
    ]
    .iter()
    .any(|x| x.0 as u32 == f)
}
fn dsv_format(f: DXGI_FORMAT) -> DXGI_FORMAT {
    match f {
        DXGI_FORMAT_R32G8X24_TYPELESS | DXGI_FORMAT_D32_FLOAT_S8X24_UINT => {
            DXGI_FORMAT_D32_FLOAT_S8X24_UINT
        }
        DXGI_FORMAT_R32_TYPELESS | DXGI_FORMAT_D32_FLOAT => DXGI_FORMAT_D32_FLOAT,
        _ => DXGI_FORMAT_UNKNOWN,
    }
}
/// G-buffer nodes that take the scene depth back into DEPTH_WRITE: SetRenderTargetsGBuffer and
/// SetRenderTargetsGBufferWithVelocityBuffer. Forward and sky phases keep the far plane, so the sky still fills the
/// hidden cells and no stale colour stays there for bloom to spread.
fn gbuffer_node(node: u32) -> bool {
    node == 0xA00830 || node == 0xB2A87C
}

fn record(r: &Runtime, entry: ClearRecord) {
    let _guard = lock(&LOG);
    for i in 0..32 {
        // SAFETY: the attached 32-record export, written under the log lock.
        let at = unsafe { &mut *r.clear_log.add(i) };
        if at.count != 0
            && at.node == entry.node
            && at.side == entry.side
            && at.width == entry.width
            && at.height == entry.height
            && at.format == entry.format
            && at.flags == entry.flags
            && at.depth == entry.depth
        {
            at.count += 1;
            at.masked += entry.masked;
            return;
        }
        if at.count == 0 {
            *at = entry;
            return;
        }
    }
}
fn record_barrier(r: &Runtime, entry: BarrierRecord) {
    let _guard = lock(&LOG);
    for i in 0..32 {
        // SAFETY: the attached 32-record export, written under the log lock.
        let at = unsafe { &mut *r.barrier_log.add(i) };
        if at.count != 0
            && at.node == entry.node
            && at.side == entry.side
            && at.before == entry.before
            && at.after == entry.after
            && at.subresource == entry.subresource
            && at.flags == entry.flags
        {
            at.count += 1;
            at.resets += entry.resets;
            return;
        }
        if at.count == 0 {
            *at = entry;
            return;
        }
    }
}

// Scene depths the masked clear has seen. A barrier names its resource, not a descriptor, so the reset needs a DSV of
// its own for it. The render graph also transitions it between nodes, where no view is current: there the view is
// the one whose clear last masked the resource.
const SCENE_SLOTS: usize = 8;
static SCENES: [AtomicUsize; SCENE_SLOTS] = [const { AtomicUsize::new(0) }; SCENE_SLOTS];
static SCENE_SIDES: [AtomicI32; SCENE_SLOTS] = [const { AtomicI32::new(0) }; SCENE_SLOTS];
static SCENE_NEXT: AtomicU32 = AtomicU32::new(0);
fn remember_scene(resource: usize, side: i32) {
    if resource == 0 {
        return;
    }
    if let Some(i) = SCENES
        .iter()
        .position(|s| s.load(Ordering::Relaxed) == resource)
    {
        SCENE_SIDES[i].store(side, Ordering::Relaxed);
        return;
    }
    let slot = SCENE_NEXT.fetch_add(1, Ordering::Relaxed) as usize % SCENE_SLOTS;
    SCENE_SIDES[slot].store(side, Ordering::Relaxed);
    SCENES[slot].store(resource, Ordering::Relaxed);
}
/// -2: not a masked scene depth; otherwise the view to use.
fn scene_side(resource: usize, side: i32) -> i32 {
    if resource == 0 {
        return -2;
    }
    match SCENES
        .iter()
        .position(|s| s.load(Ordering::Relaxed) == resource)
    {
        Some(_) if side == 0 || side == 1 => side,
        Some(i) => SCENE_SIDES[i].load(Ordering::Relaxed),
        None => -2,
    }
}

// One descriptor per recording thread, written right before the clear that reads it.
const OWN_SLOTS: u32 = 64;
struct Own {
    device: Option<ID3D12Device>,
    heap: Option<ID3D12DescriptorHeap>,
    stride: u32,
}
// SAFETY: free-threaded D3D12 objects, under the mutex.
unsafe impl Send for Own {}
static OWN: Mutex<Own> = Mutex::new(Own {
    device: None,
    heap: None,
    stride: 0,
});
static OWN_NEXT: AtomicU32 = AtomicU32::new(0);
thread_local!(static OWN_SLOT: core::cell::Cell<i32> = const { core::cell::Cell::new(-1) });
unsafe fn own_dsv(
    resource: &ID3D12Resource,
    format: DXGI_FORMAT,
) -> Option<D3D12_CPU_DESCRIPTOR_HANDLE> {
    let slot = OWN_SLOT.with(|s| {
        if s.get() < 0 {
            s.set(OWN_NEXT.fetch_add(1, Ordering::Relaxed) as i32);
        }
        s.get()
    });
    if slot >= OWN_SLOTS as i32 {
        return None;
    }
    let mut own = lock(&OWN);
    if own.heap.is_none() {
        let mut device: Option<ID3D12Device> = None;
        resource.GetDevice(&mut device).ok()?;
        let device = device?;
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            NumDescriptors: OWN_SLOTS,
            ..Default::default()
        };
        let heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&desc).ok()?;
        own.stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
        own.heap = Some(heap);
        own.device = Some(device);
    }
    let start = own.heap.as_ref()?.GetCPUDescriptorHandleForHeapStart();
    let handle = D3D12_CPU_DESCRIPTOR_HANDLE {
        ptr: start.ptr + slot as usize * own.stride as usize,
    };
    let view = D3D12_DEPTH_STENCIL_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    own.device
        .as_ref()?
        .CreateDepthStencilView(resource, Some(&view), handle);
    Some(handle)
}

pub type ClearFn = unsafe extern "system" fn(
    *mut c_void,
    D3D12_CPU_DESCRIPTOR_HANDLE,
    D3D12_CLEAR_FLAGS,
    f32,
    u8,
    u32,
    *const RECT,
);

unsafe fn clear_rects(
    list: *mut c_void,
    dsv: D3D12_CPU_DESCRIPTOR_HANDLE,
    clear: ClearFn,
    depth: f32,
    rects: &[Rect],
) {
    // Batched: one clear with 177 or 205 rectangles terminated the game without a crash report, while 127 ran for
    // minutes (2026-10-09). 64 per call stays well under that.
    for chunk in rects.chunks(MAX_RECTS_PER_CLEAR) {
        clear(
            list,
            dsv,
            D3D12_CLEAR_FLAG_DEPTH,
            depth,
            0,
            chunk.len() as u32,
            chunk.as_ptr().cast(),
        );
    }
}

fn frustum(r: &Runtime, side: i32) -> Option<(Fov, Fov)> {
    let (mut fov, mut rendered) = (UNSET, UNSET);
    // SAFETY: the attached C++ read.
    unsafe { (r.frustum)(side, &mut fov, &mut rendered) }.then_some((fov, rendered))
}
fn rects_for(r: &Runtime, side: usize, w: u32, h: u32) -> Option<Arc<Vec<Rect>>> {
    let (fov, rendered) = frustum(r, side as i32)?;
    let radius = radius(r);
    let mut guard = lock(&RECTS);
    let rects = guard.get_or_insert_with(Rects::default);
    let cache = &mut rects.cache[side];
    let stale = cache.as_ref().is_none_or(|c| {
        c.width != w
            || c.height != h
            || c.radius != radius
            || !same_fov(&c.rendered, &rendered)
            || !same_fov(&c.fov, &fov)
    });
    if stale {
        let built = Arc::new(hidden_area::build_rects(
            &fov, radius, w, h, &rendered, CELL, MARGIN,
        ));
        let hidden = (100.0 * hidden_area::coverage(&built, w, h)) as f32;
        LAST_HIDDEN[side].store(hidden.to_bits(), Ordering::Relaxed);
        *cache = Some(Cache {
            width: w,
            height: h,
            radius,
            rendered,
            fov,
            rects: built,
        });
    }
    cache.as_ref().map(|c| c.rects.clone())
}
fn publish_shader_ellipse(r: &Runtime, side: usize) {
    let Some((fov, rendered)) = frustum(r, side as i32) else {
        return;
    };
    let radius = radius(r) / 100.0 * (1.0 + SHADER_MARGIN);
    let scales = if rendered.left == -rendered.right && rendered.down == -rendered.up {
        // NDC x and y (y down) to the quadrant-normalised tangents the visible ellipse is defined in.
        let (tan_x, tan_y) = (rendered.right, rendered.up);
        let s = [
            tan_x / -fov.left,
            tan_x / fov.right,
            tan_y / fov.up,
            tan_y / -fov.down,
        ];
        s.map(|v| (v / radius) * (v / radius))
    } else {
        // The patched shaders centre their ellipse on NDC 0, which is not straight ahead in an off-centre window:
        // they get the smallest centred ellipse that still holds the visible area.
        let mut guard = lock(&RECTS);
        let c = &mut guard.get_or_insert_with(Rects::default).ellipse[side];
        if !c.set
            || c.radius != radius
            || !same_fov(&c.rendered, &rendered)
            || !same_fov(&c.fov, &fov)
        {
            *c = EllipseCache {
                radius,
                rendered,
                fov,
                scales: hidden_area::shader_ellipse_scales(&fov, radius * 100.0, &rendered),
                set: true,
            };
        }
        let Some(scales) = c.scales else { return };
        scales
    };
    // SAFETY: four floats.
    unsafe { crate::shader_patches::cvr_shader_patches_publish(side as i32, &scales) };
}
/// The hidden cells of `resource` to `depth`, only while the resource is in DEPTH_WRITE: before the barrier that ends
/// its depth writes, or after the one that starts them.
unsafe fn clear_hidden(
    r: &Runtime,
    list: *mut c_void,
    resource: usize,
    side: i32,
    clear: Option<ClearFn>,
    depth: f32,
) -> bool {
    // Engine stereo puts both eyes in one target; the mask is laid out for one eye per target.
    let Some(clear) = clear else { return false };
    if !(0..=1).contains(&side) || !enabled(r) || (r.menu_mode)() || (r.engine_stereo)() {
        return false;
    }
    let raw = resource as *mut c_void;
    let Some(res) = ID3D12Resource::from_raw_borrowed(&raw) else {
        return false;
    };
    let d = res.GetDesc();
    let format = dsv_format(d.Format);
    if format == DXGI_FORMAT_UNKNOWN
        || d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.SampleDesc.Count != 1
    {
        return false;
    }
    let Some(cover) =
        rects_for(r, side as usize, d.Width as u32, d.Height).filter(|c| !c.is_empty())
    else {
        return false;
    };
    let _internal = cvr_host::probe::Internal::new();
    let Some(dsv) = own_dsv(res, format) else {
        return false;
    };
    clear_rects(list, dsv, clear, depth, &cover);
    bump(
        if depth == 0.0 {
            r.sky_resets
        } else {
            r.near_resets
        },
        side as usize,
    );
    true
}
fn to_far(r: &Runtime, node: u32) -> bool {
    let only = load(r.sky_node);
    load(r.sky) >= 1 && (only == 0 || only == node)
}
fn to_near(r: &Runtime, node: u32) -> bool {
    load(r.sky) >= 2 && gbuffer_node(node)
}

/// DsvCreated: the engine clears COPIES of its depth-stencil descriptors, so the descriptor -> target map follows both
/// creation and copies.
///
/// # Safety
/// `resource` is null or live; `desc` is null or valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_dsv_created(
    resource: *mut c_void,
    desc: *const D3D12_DEPTH_STENCIL_VIEW_DESC,
    handle: usize,
) {
    if cvr_host::probe::internal() || handle == 0 {
        return;
    }
    let mut target = None;
    if let Some(r) = ID3D12Resource::from_raw_borrowed(&resource) {
        let d = r.GetDesc();
        let view = desc.as_ref();
        if d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
            && d.SampleDesc.Count == 1
            && view.is_none_or(|v| {
                v.ViewDimension == D3D12_DSV_DIMENSION_TEXTURE2D
                    && v.Anonymous.Texture2D.MipSlice == 0
            })
        {
            target = Some(Target {
                format: view.map_or(d.Format, |v| v.Format).0 as u32,
                width: d.Width as u32,
                height: d.Height,
                resource: resource as usize,
            });
        }
    }
    let mut dsvs = lock(&DSVS);
    let dsvs = dsvs.get_or_insert_with(cvr_host::fast::fast_map);
    if dsvs.len() >= 65536 && !dsvs.contains_key(&handle) {
        dsvs.clear();
    }
    match target.filter(|t| t.width != 0) {
        Some(t) => {
            dsvs.insert(handle, t);
        }
        None => {
            dsvs.remove(&handle);
        }
    }
}
/// CopyDescriptors: DSV copies carry the source descriptors' targets (an unknown source clears the destination).
///
/// # Safety
/// The arguments are CopyDescriptors'.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_copy_descriptors(
    device: *mut c_void,
    destinations: u32,
    dest: *const usize,
    sizes: *const u32,
    sources: u32,
    src: *const usize,
    counts: *const u32,
    kind: i32,
) {
    if kind != D3D12_DESCRIPTOR_HEAP_TYPE_DSV.0
        || dest.is_null()
        || src.is_null()
        || cvr_host::probe::internal()
    {
        return;
    }
    let Some(device) = ID3D12Device::from_raw_borrowed(&device) else {
        return;
    };
    let stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV) as usize;
    let mut dsvs = lock(&DSVS);
    let dsvs = dsvs.get_or_insert_with(cvr_host::fast::fast_map);
    let mut copy: Vec<Option<Target>> = Vec::new();
    for i in 0..sources as usize {
        let n = if counts.is_null() { 1 } else { *counts.add(i) };
        for k in 0..n as usize {
            if copy.len() >= 65536 {
                return;
            }
            copy.push(dsvs.get(&(*src.add(i) + k * stride)).copied());
        }
    }
    let mut at = 0;
    for i in 0..destinations as usize {
        let n = if sizes.is_null() { 1 } else { *sizes.add(i) };
        for k in 0..n as usize {
            let handle = *dest.add(i) + k * stride;
            match copy.get(at).copied().flatten() {
                Some(t) => {
                    dsvs.insert(handle, t);
                }
                None => {
                    dsvs.remove(&handle);
                }
            }
            at += 1;
        }
    }
}
/// AfterDepthClear: after every native ClearDepthStencilView with the current render-graph node and view side. The
/// view's own scene depth -- a full clear to the reverse-Z far plane, a 32-bit depth format and the eye's aspect
/// (shadow maps are 16-bit or a different shape; menus show the eye image flat) -- gets its hidden cells at the near
/// plane.
///
/// # Safety
/// The arguments are the clear's; `clear` is the original ClearDepthStencilView.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_after_depth_clear(
    list: *mut c_void,
    dsv: usize,
    flags: i32,
    depth: f32,
    rects: u32,
    side: i32,
    node: u32,
    clear: Option<ClearFn>,
) {
    let Some(r) = runtime() else { return };
    let Some(clear) = clear else { return };
    if cvr_host::probe::internal() || list.is_null() {
        return;
    }
    let log = load(r.log) != 0;
    let on = enabled(r);
    if (!log && !on) || flags & D3D12_CLEAR_FLAG_DEPTH.0 == 0 {
        return;
    }
    // An unknown descriptor is logged as 0x0.
    let target = lock(&DSVS).as_ref().and_then(|m| m.get(&dsv).copied());
    if target.is_none() && !log {
        return;
    }
    let t = target.unwrap_or(Target {
        format: 0,
        width: 0,
        height: 0,
        resource: 0,
    });
    let aspect = (r.aspect)();
    let only = load(r.node);
    let mut masked = on
        && (0..=1).contains(&side)
        && rects == 0
        && depth == 0.0
        && scene_depth_format(t.format)
        && t.width >= 256
        && t.height >= 256
        && (t.width as f32 / t.height as f32 - aspect).abs() <= 0.02 * aspect
        && (only == 0 || only == node)
        && !(r.menu_mode)();
    if masked {
        let cover = rects_for(r, side as usize, t.width, t.height).filter(|c| !c.is_empty());
        masked = cover.is_some();
        if let Some(cover) = cover {
            let _internal = cvr_host::probe::Internal::new();
            clear_rects(
                list,
                D3D12_CPU_DESCRIPTOR_HANDLE { ptr: dsv },
                clear,
                1.0,
                &cover,
            );
            bump(r.masked, side as usize);
            remember_scene(t.resource, side);
            publish_shader_ellipse(r, side as usize);
            LAST_RECTS[side as usize].store(cover.len() as u32, Ordering::Relaxed);
            LAST_WIDTH[side as usize].store(t.width, Ordering::Relaxed);
            LAST_HEIGHT[side as usize].store(t.height, Ordering::Relaxed);
        }
    }
    if log {
        record(
            r,
            ClearRecord {
                node,
                side,
                width: t.width,
                height: t.height,
                format: t.format,
                flags: flags as u32,
                depth,
                count: 1,
                masked: masked as u32,
            },
        );
    }
}
fn depth_write(state: D3D12_RESOURCE_STATES) -> bool {
    state.0 & D3D12_RESOURCE_STATE_DEPTH_WRITE.0 != 0
}
fn whole_or_first(subresource: u32) -> bool {
    subresource == D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES || subresource == 0
}
/// BeforeBarriers: when the masked scene depth leaves DEPTH_WRITE, the hidden cells go back to the far plane (not on
/// an END_ONLY half: between the halves of a split barrier the resource is unusable).
///
/// # Safety
/// `barriers` holds `count` barriers; `clear` is the original ClearDepthStencilView.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_before_barriers(
    list: *mut c_void,
    count: u32,
    barriers: *const D3D12_RESOURCE_BARRIER,
    side: i32,
    node: u32,
    clear: Option<ClearFn>,
) {
    let Some(r) = runtime() else { return };
    if cvr_host::probe::internal()
        || list.is_null()
        || barriers.is_null()
        || SCENES[0].load(Ordering::Relaxed) == 0
    {
        return;
    }
    let log = load(r.log) != 0;
    for i in 0..count as usize {
        let b = &*barriers.add(i);
        if b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION {
            continue;
        }
        let t = &b.Anonymous.Transition;
        let resource: usize = core::mem::transmute_copy(&t.pResource);
        let view = scene_side(resource, side);
        if view == -2 {
            continue;
        }
        let leaves = depth_write(t.StateBefore)
            && !depth_write(t.StateAfter)
            && b.Flags.0 & D3D12_RESOURCE_BARRIER_FLAG_END_ONLY.0 == 0
            && whole_or_first(t.Subresource);
        let reset = leaves && to_far(r, node) && clear_hidden(r, list, resource, view, clear, 0.0);
        if log {
            record_barrier(
                r,
                BarrierRecord {
                    node,
                    side,
                    before: t.StateBefore.0 as u32,
                    after: t.StateAfter.0 as u32,
                    subresource: t.Subresource,
                    flags: b.Flags.0 as u32,
                    count: 1,
                    resets: reset as u32,
                },
            );
        }
    }
}
fn depth_plane(s: &D3D12_BARRIER_SUBRESOURCE_RANGE) -> bool {
    s.IndexOrFirstMipLevel == 0xffff_ffff
        || (s.IndexOrFirstMipLevel == 0 && (s.NumMipLevels == 0 || s.FirstPlane == 0))
}
unsafe fn texture_barriers(group: &D3D12_BARRIER_GROUP) -> &[D3D12_TEXTURE_BARRIER] {
    let barriers = group.Anonymous.pTextureBarriers;
    if group.Type != D3D12_BARRIER_TYPE_TEXTURE || barriers.is_null() {
        return &[];
    }
    core::slice::from_raw_parts(barriers, group.NumBarriers as usize)
}
/// BeforeEnhancedBarriers: the same for enhanced barriers; the barrier must also wait for depth writes, which then
/// covers the clear recorded here.
///
/// # Safety
/// `groups` holds `count` groups; `clear` is the original ClearDepthStencilView.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_before_enhanced_barriers(
    list: *mut c_void,
    count: u32,
    groups: *const D3D12_BARRIER_GROUP,
    side: i32,
    node: u32,
    clear: Option<ClearFn>,
) {
    let Some(r) = runtime() else { return };
    if cvr_host::probe::internal()
        || list.is_null()
        || groups.is_null()
        || SCENES[0].load(Ordering::Relaxed) == 0
    {
        return;
    }
    let log = load(r.log) != 0;
    let waits =
        D3D12_BARRIER_SYNC_ALL.0 | D3D12_BARRIER_SYNC_DRAW.0 | D3D12_BARRIER_SYNC_DEPTH_STENCIL.0;
    for g in 0..count as usize {
        for b in texture_barriers(&*groups.add(g)) {
            let resource: usize = core::mem::transmute_copy(&b.pResource);
            let view = scene_side(resource, side);
            if view == -2 {
                continue;
            }
            let s = &b.Subresources;
            let leaves = b.LayoutBefore == D3D12_BARRIER_LAYOUT_DEPTH_STENCIL_WRITE
                && b.LayoutAfter != D3D12_BARRIER_LAYOUT_DEPTH_STENCIL_WRITE
                && depth_plane(s)
                && b.SyncBefore.0 & waits != 0;
            let reset =
                leaves && to_far(r, node) && clear_hidden(r, list, resource, view, clear, 0.0);
            if log {
                record_barrier(
                    r,
                    BarrierRecord {
                        node,
                        side,
                        before: 0x8000_0000 | b.LayoutBefore.0 as u32,
                        after: 0x8000_0000 | b.LayoutAfter.0 as u32,
                        subresource: s.IndexOrFirstMipLevel,
                        flags: b.Flags.0 as u32,
                        count: 1,
                        resets: reset as u32,
                    },
                );
            }
        }
    }
}
/// AfterBarriers: a G-buffer node taking the scene depth back into DEPTH_WRITE gets the near plane in the hidden cells
/// again, so its geometry stays out of them.
///
/// # Safety
/// `barriers` holds `count` barriers; `clear` is the original ClearDepthStencilView.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_after_barriers(
    list: *mut c_void,
    count: u32,
    barriers: *const D3D12_RESOURCE_BARRIER,
    side: i32,
    node: u32,
    clear: Option<ClearFn>,
) {
    let Some(r) = runtime() else { return };
    if cvr_host::probe::internal()
        || list.is_null()
        || barriers.is_null()
        || !to_near(r, node)
        || SCENES[0].load(Ordering::Relaxed) == 0
    {
        return;
    }
    for i in 0..count as usize {
        let b = &*barriers.add(i);
        if b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION {
            continue;
        }
        let t = &b.Anonymous.Transition;
        let resource: usize = core::mem::transmute_copy(&t.pResource);
        let view = scene_side(resource, side);
        if view != -2
            && !depth_write(t.StateBefore)
            && depth_write(t.StateAfter)
            && b.Flags.0 & D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY.0 == 0
            && whole_or_first(t.Subresource)
        {
            clear_hidden(r, list, resource, view, clear, 1.0);
        }
    }
}
/// # Safety
/// `groups` holds `count` groups; `clear` is the original ClearDepthStencilView.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_after_enhanced_barriers(
    list: *mut c_void,
    count: u32,
    groups: *const D3D12_BARRIER_GROUP,
    side: i32,
    node: u32,
    clear: Option<ClearFn>,
) {
    let Some(r) = runtime() else { return };
    if cvr_host::probe::internal()
        || list.is_null()
        || groups.is_null()
        || !to_near(r, node)
        || SCENES[0].load(Ordering::Relaxed) == 0
    {
        return;
    }
    for g in 0..count as usize {
        for b in texture_barriers(&*groups.add(g)) {
            let resource: usize = core::mem::transmute_copy(&b.pResource);
            let view = scene_side(resource, side);
            if view != -2
                && b.LayoutBefore != D3D12_BARRIER_LAYOUT_DEPTH_STENCIL_WRITE
                && b.LayoutAfter == D3D12_BARRIER_LAYOUT_DEPTH_STENCIL_WRITE
                && depth_plane(&b.Subresources)
            {
                clear_hidden(r, list, resource, view, clear, 1.0);
            }
        }
    }
}
/// The last masked clear per view: its rectangles, the share they hide (percent) and the target's size.
///
/// # Safety
/// The outputs hold two values each.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_last(
    rects: *mut u32,
    hidden: *mut f32,
    width: *mut u32,
    height: *mut u32,
) {
    for i in 0..2 {
        *rects.add(i) = LAST_RECTS[i].load(Ordering::Relaxed);
        *hidden.add(i) = f32::from_bits(LAST_HIDDEN[i].load(Ordering::Relaxed));
        *width.add(i) = LAST_WIDTH[i].load(Ordering::Relaxed);
        *height.add(i) = LAST_HEIGHT[i].load(Ordering::Relaxed);
    }
}

// ---- the eye's frustum (HiddenArea.cpp, Frustum: the C++ half asks OpenXR, the asymmetric projection and the game)

/// Frustum's arithmetic: the eye's runtime FOV (`own`, OpenXR angles left, right, up, down) as tangents -- refused unless
/// left/down negative and right/up positive; with the asymmetric projection its own window (`window`, l, r, d, u) the
/// rendered frustum and the FOV's sides moved towards the other eye's (`other`) by the inner guard (clamped 0..1, 0
/// when not finite); else the game's symmetric render FOV (`h0`, `v0` degrees, sane only in 1..170 and 1..179).
pub fn eye_frustum(own: Option<[f32; 4]>, window: Option<[f32; 4]>, guard: f32, other: Option<[f32; 4]>, h0: f32, v0: f32) -> Option<(Fov, Fov)> {
    let own = own?;
    let mut fov = Fov { left: own[0].tan(), right: own[1].tan(), up: own[2].tan(), down: own[3].tan() };
    if !(fov.left < 0.0 && fov.right > 0.0 && fov.up > 0.0 && fov.down < 0.0) {
        return None;
    }
    if let Some(w) = window {
        let rendered = Fov { left: w[0], right: w[1], up: w[3], down: w[2] };
        let guard = 1.0f32.min(0.0f32.max(if guard.is_finite() { guard } else { 0.0 }));
        if guard > 0.0 {
            if let Some(o) = other {
                let (ul, ur) = (fov.left.min(o[0].tan()), fov.right.max(o[1].tan()));
                fov.left += guard * (ul - fov.left);
                fov.right += guard * (ur - fov.right);
            }
        }
        return Some((fov, rendered));
    }
    if !(h0 > 1.0 && h0 < 170.0 && v0 > 1.0 && v0 < 179.0) {
        return None;
    }
    let (tx, ty) = ((h0 * 0.5 * 3.14159265 / 180.0).tan(), (v0 * 0.5 * 3.14159265 / 180.0).tan());
    Some((fov, Fov { left: -tx, right: tx, up: ty, down: -ty }))
}
/// Frustum's arithmetic into `fov` and `rendered` (null `own` / `window` / `other`: not had).
///
/// # Safety
/// The non-null inputs hold four floats; the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ha_frustum(own: *const [f32; 4], window: *const [f32; 4], guard: f32, other: *const [f32; 4], h0: f32, v0: f32, fov: *mut Fov, rendered: *mut Fov) -> bool {
    // SAFETY: as documented.
    match unsafe { eye_frustum(own.as_ref().copied(), window.as_ref().copied(), guard, other.as_ref().copied(), h0, v0) } {
        Some((f, r)) => {
            unsafe {
                *fov = f;
                *rendered = r;
            }
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod frustum_tests {
    use super::eye_frustum;

    #[test]
    fn eye_frustum_rules() {
        let own = [-0.8f32, 0.7, 0.75, -0.8];
        let (fov, rendered) = eye_frustum(Some(own), None, 0.5, None, 90.0, 60.0).unwrap();
        assert_eq!((fov.left, fov.right), ((-0.8f32).tan(), 0.7f32.tan()));
        assert!((rendered.right - 1.0).abs() < 1e-6 && (rendered.up - 30f32.to_radians().tan()).abs() < 1e-6, "the symmetric frustum");
        assert!(eye_frustum(Some(own), None, 0.5, None, 180.0, 60.0).is_none(), "an insane render FOV");
        assert!(eye_frustum(Some([0.1, 0.7, 0.75, -0.8]), None, 0.0, None, 90.0, 60.0).is_none(), "a left side right of centre");
        assert!(eye_frustum(None, None, 0.0, None, 90.0, 60.0).is_none());
        let window = [-1.0f32, 0.9, -0.95, 0.85];
        let other = [-0.6f32, 0.9, 0.75, -0.8];
        let (f, r) = eye_frustum(Some(own), Some(window), 0.5, Some(other), 0.0, 0.0).unwrap();
        assert_eq!((r.left, r.right, r.up, r.down), (-1.0, 0.9, 0.85, -0.95), "the window as l, r, u, d");
        let (ul, ur) = ((-0.8f32).tan().min((-0.6f32).tan()), 0.7f32.tan().max(0.9f32.tan()));
        assert_eq!(f.left, (-0.8f32).tan() + 0.5 * (ul - (-0.8f32).tan()));
        assert_eq!(f.right, 0.7f32.tan() + 0.5 * (ur - 0.7f32.tan()), "towards the other eye by the guard");
        let (g, _) = eye_frustum(Some(own), Some(window), f32::NAN, Some(other), 0.0, 0.0).unwrap();
        assert_eq!(g.right, 0.7f32.tan(), "a guard not finite is none");
        let (h, _) = eye_frustum(Some(own), Some(window), 7.0, Some(other), 0.0, 0.0).unwrap();
        assert_eq!(h.right, 0.7f32.tan().max(0.9f32.tan()), "the guard clamped to 1");
    }
}
