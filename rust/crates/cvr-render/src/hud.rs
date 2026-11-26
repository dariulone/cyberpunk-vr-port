//! GETTING THE INTERFACE INTO THE SECOND EYE (formerly src/Stereo/Hud.cpp): the HUD is drawn into its own surface
//! with premultiplied alpha, so the second eye snapshots that surface and composites it. The surface is recognised
//! by its description and named by the node that binds it (DrawComposition / CompositionPostProcess), the
//! descriptor path holds a producing surface against a newcomer, a new frame graph re-arms the identification;
//! the snapshots are copies appended to the engine's own list at the unbind; the composite's constants are found in
//! the upload ring and copied into a buffer of ours; and the second view is granted the HUD capability.
//!
//! Hud.cpp keeps the exports, the composite parameters (ColorBlit's struct) and the globals Capture.cpp and
//! CommandListCensus.cpp read -- the surface, its last mip, the HUD snapshot's description under its mutex, the
//! freshness and the demand ticks --, which this file writes through the runtime it is handed.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*,
};
use cvr_engine::code::{read, write};
use std::cell::UnsafeCell;
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_HudNodeRva: AtomicU32;
    static CyberpunkVR_HudByNode: AtomicI32;
    static CyberpunkVR_DebugHudNodeNames: AtomicU64;
    static CyberpunkVR_DebugHudRearms: AtomicU64;
    static CyberpunkVR_HudRearmOnGraphChange: AtomicI32;
    static CyberpunkVR_HudHoldMs: AtomicU32;
    static CyberpunkVR_DebugHudHolds: AtomicU64;
    static CyberpunkVR_HudMaxAgeMs: AtomicU32;
    static CyberpunkVR_VisionSnap: AtomicI32;
    static CyberpunkVR_VisionMaxAgeMs: AtomicU32;
    static CyberpunkVR_HudToSecondEye: AtomicI32;
    static CyberpunkVR_DebugHudSnapSkips: AtomicU64;
    static CyberpunkVR_DebugHudSnaps: AtomicU64;
    static CyberpunkVR_DebugHudCb: AtomicU64;
    static CyberpunkVR_HudCapWord: AtomicU32;
    static CyberpunkVR_HudCapBits: AtomicU64;
    static CyberpunkVR_DebugHudCapGrants: AtomicU64;
    static CyberpunkVR_HudGrantCap: AtomicI32;
    static g_game_device: *mut c_void;
    fn cvr_cpp_command_list_hook_entry(list: *mut c_void) -> *const crate::single_pass::Hook;
    fn cvr_hud_block_plausible(p: *const u8, w: f32, h: f32) -> bool;
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn diag(counter: &AtomicU64) {
    if diagnostics() {
        counter.fetch_add(1, SeqCst);
    }
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

/// sizeof(MappedUpload): the upload rings' maps.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MappedUpload {
    res: usize,
    ptr: usize,
    size: u64,
    va: u64,
}
extern "C-unwind" {
    /// The mapped upload heaps' snapshot (upload_maps.rs).
    fn cvr_upload_maps_snapshot(out: *mut MappedUpload, capacity: u32) -> u32;
}
/// What Hud.cpp shares with this file.
#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    /// g_hud_res: the HUD surface, AddRef'd (Capture.cpp compares bound targets with it).
    hud_res: *mut *mut c_void,
    hud_last_mip: *const AtomicU32,
    /// g_hud_snap_desc, g_hud_snap_fresh: the HUD snapshot's description (under g_hud_snap_mtx) and freshness.
    hud_snap_desc: *mut D3D12_RESOURCE_DESC,
    hud_snap_fresh: *const AtomicBool,
    /// g_hud_snap_mtx, which every snapshot slot is kept under.
    snap_lock: extern "C" fn(),
    snap_unlock: extern "C" fn(),
    /// g_hud_snap_tick (DeviceHooks.cpp): the HUD surface's last snapshot.
    hud_snap_tick: *const AtomicU64,
    /// g_hud_consumed_tick and g_vision_tick: the HUD's demand and the outline's last copy (Capture.cpp).
    hud_consumed_tick: *const AtomicU64,
    vision_tick: *const AtomicU64,
    /// mirror_get_resource_desc (SEH-guarded GetDesc).
    get_desc: extern "C" fn(*mut c_void, *mut D3D12_RESOURCE_DESC) -> bool,
    /// The composite constants: g_hud_cb_scan_tick, g_hud_cb_copy, g_hud_cb_copy_ptr (SyncStereo.cpp), g_hud_cb,
    /// g_hud_cb_from_ring; the upload ring's maps copied out under their lock.
    hud_cb_scan_tick: *mut u64,
    hud_cb_copy: *mut *mut c_void,
    hud_cb_copy_ptr: *mut *mut u8,
    hud_cb: *const AtomicPtr<c_void>,
    hud_cb_from_ring: *const AtomicBool,
    /// The composite inputs' report: the HUD node's binds and unresolved binds, the RTV map's fill and limit.
    hud_node_binds: *const AtomicU64,
    hud_node_unresolved: *const AtomicU64,
    rtv_dim_count: *const AtomicU32,
    rtv_dim_wrapped_logged: *const AtomicBool,
    /// The capability mask dump: g_hud_mask_dumped_main / _vrcam, HUD_REQUIRED_MASK_RVA.
    hud_mask_dumped_main: *mut bool,
    hud_mask_dumped_vrcam: *mut bool,
    hud_required_mask_rva: *const usize,
    stereo_log: extern "C" fn(*const c_char),
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    exe_base: (&raw const s::g_exe_base).cast(),
    hud_res: &raw mut s::g_hud_res,
    hud_last_mip: &raw const m::g_hud_last_mip,
    hud_snap_desc: &raw mut s::g_hud_snap_desc,
    hud_snap_fresh: &raw const m::g_hud_snap_fresh,
    snap_lock: s::hud_snap_take,
    snap_unlock: s::hud_snap_release,
    hud_snap_tick: &raw const m::g_hud_snap_tick,
    hud_consumed_tick: &raw const m::g_hud_consumed_tick,
    vision_tick: &raw const m::g_vision_tick,
    get_desc: s::get_desc,
    hud_cb_scan_tick: (&raw const m::g_hud_cb_scan_tick).cast_mut().cast(),
    hud_cb_copy: &raw mut s::g_hud_cb_copy,
    hud_cb_copy_ptr: &raw mut s::g_hud_cb_copy_ptr,
    hud_cb: &raw const s::g_hud_cb,
    hud_cb_from_ring: &raw const m::g_hud_cb_from_ring,
    hud_node_binds: &raw const m::g_hud_node_binds,
    hud_node_unresolved: &raw const m::g_hud_node_unresolved,
    rtv_dim_count: &raw const m::g_rtv_dim_count,
    rtv_dim_wrapped_logged: &raw const m::g_rtv_dim_wrapped_logged,
    hud_mask_dumped_main: (&raw const m::g_hud_mask_dumped_main).cast_mut().cast(),
    hud_mask_dumped_vrcam: (&raw const m::g_hud_mask_dumped_vrcam).cast_mut().cast(),
    hud_required_mask_rva: &raw const s::HUD_REQUIRED_MASK_RVA,
    stereo_log: s::stereo_log,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn base() -> usize {
    // SAFETY: attached.
    unsafe { *rt().exe_base }
}
fn get_desc(res: *mut c_void) -> Option<D3D12_RESOURCE_DESC> {
    let mut d = D3D12_RESOURCE_DESC::default();
    (!res.is_null() && (rt().get_desc)(res, &mut d)).then_some(d)
}
unsafe fn add_ref(res: *mut c_void) {
    if let Some(r) = ID3D12Resource::from_raw_borrowed(&res) {
        core::mem::forget(r.clone());
    }
}
unsafe fn release(res: *mut c_void) {
    if !res.is_null() {
        drop(ID3D12Resource::from_raw(res));
    }
}
const RGBA8: [DXGI_FORMAT; 3] = [
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
    DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R8G8B8A8_TYPELESS,
];
/// The HUD surface's shape: a mip-chained RGBA8 render target, never a small ink widget target.
fn hud_rt_signature(d: &D3D12_RESOURCE_DESC) -> bool {
    d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && d.MipLevels > 1
        && d.DepthOrArraySize == 1
        && d.SampleDesc.Count == 1
        && (d.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET.0) != 0
        && d.Width >= 640
        && d.Height >= 360
        && RGBA8.contains(&d.Format)
}
/// The engine's blurred-HUD pyramid: half the HUD's width, 4 mips, UAV.
#[no_mangle]
pub extern "C" fn cvr_hud_blur_signature(d: &D3D12_RESOURCE_DESC, hud_width: u64) -> bool {
    if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.MipLevels != 4
        || d.DepthOrArraySize != 1
        || d.SampleDesc.Count != 1
        || (d.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0) == 0
        || !RGBA8.contains(&d.Format)
        || hud_width == 0
    {
        return false;
    }
    let half = hud_width / 2;
    d.Width + 1 >= half && d.Width <= half + 1
}

// ---- which bound target IS the HUD

/// The surface's mip-0 RTV handles, and whether the node has named it.
struct Surface {
    handles: Vec<usize>,
}
static SURFACE: Mutex<Surface> = Mutex::new(Surface {
    handles: Vec::new(),
});
static NODE_NAMED: AtomicBool = AtomicBool::new(false);
fn surface() -> std::sync::MutexGuard<'static, Surface> {
    SURFACE.lock().unwrap_or_else(|e| e.into_inner())
}
/// Retire the held surface and hold `res` instead (AddRef'd), with its last mip.
unsafe fn adopt(s: &mut Surface, res: *mut c_void, d: &D3D12_RESOURCE_DESC) {
    let r = rt();
    s.handles.clear();
    release(*r.hud_res);
    add_ref(res);
    *r.hud_res = res;
    (*r.hud_last_mip).store(
        if d.MipLevels != 0 {
            d.MipLevels as u32 - 1
        } else {
            0
        },
        Release,
    );
}
/// A new frame graph: the node-named flag cleared (descriptor matching again), the held surface released, the
/// composite constants looked for on the next frame, and the snapshot clock started at the re-arm so the first
/// surface adopted holds the floor long enough to prove itself (off by default: CyberpunkVR_HudRearmOnGraphChange).
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_rearm_for_new_graph(key: u64) {
    if CyberpunkVR_HudRearmOnGraphChange.load(Relaxed) == 0 {
        return;
    }
    let r = rt();
    let was_named = NODE_NAMED.swap(false, AcqRel);
    let dropped = {
        let mut s = surface();
        let d = *r.hud_res;
        *r.hud_res = core::ptr::null_mut();
        s.handles.clear();
        d
    };
    release(dropped);
    *r.hud_cb_scan_tick = 0;
    (*r.hud_snap_tick).store(tick(), Release);
    diag(&CyberpunkVR_DebugHudRearms);
    stereo_log(&format!(
        "[hud] frame graph rebuilt under key {key:016X} -- identification re-armed (was {}, surface {})",
        if was_named { "node-named" } else { "unnamed" },
        if dropped.is_null() { "none held" } else { "dropped" }
    ));
}
/// Which node binds each HUD-shaped target (the answered survey; the two binders are named in the exports).
struct NodeCandidate {
    rva: u32,
    w: u32,
    h: u32,
    mips: u32,
    fmt: u32,
    hits: [u64; 2],
}
static NODE_CANDIDATES: Mutex<Vec<NodeCandidate>> = Mutex::new(Vec::new());
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_node_note(res: *mut c_void, vrcam: bool) {
    let Some(d) = get_desc(res) else { return };
    if !hud_rt_signature(&d) {
        return;
    }
    let work = crate::render_tls::current_node_work();
    let rva = if base() != 0 && work > base() {
        (work - base()) as u32
    } else {
        0
    };
    {
        let mut c = NODE_CANDIDATES.lock().unwrap_or_else(|e| e.into_inner());
        let i = match c.iter().position(|x| {
            x.rva == rva && x.w == d.Width as u32 && x.h == d.Height && x.mips == d.MipLevels as u32
        }) {
            Some(i) => i,
            None if c.len() < 24 => {
                c.push(NodeCandidate {
                    rva,
                    w: d.Width as u32,
                    h: d.Height,
                    mips: d.MipLevels as u32,
                    fmt: d.Format.0 as u32,
                    hits: [0; 2],
                });
                c.len() - 1
            }
            None => return,
        };
        c[i].hits[vrcam as usize] += 1;
    }
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now - last < 15000 {
        return;
    }
    LAST.store(now, Relaxed);
    let c = NODE_CANDIDATES.lock().unwrap_or_else(|e| e.into_inner());
    let mut line = String::new();
    for x in c.iter() {
        if line.len() >= 900 - 60 {
            break;
        }
        line.push_str(&format!(
            "{:X}:{}x{} m{} f{}(M{}/V{}) ",
            x.rva, x.w, x.h, x.mips, x.fmt, x.hits[0], x.hits[1]
        ));
    }
    stereo_log(&format!(
        "[hudnode] who binds a HUD-shaped target ({}): {line}",
        c.len()
    ));
}
/// The HUD node bound `res`: node AND shape, then descriptor matching off for good.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_adopt_by_node(res: *mut c_void) {
    let Some(d) = get_desc(res) else { return };
    if !hud_rt_signature(&d) {
        return;
    }
    {
        let mut s = surface();
        if res != *rt().hud_res {
            adopt(&mut s, res, &d);
            // Throttled: the engine POOLS these, so adoption is a heartbeat, not an event.
            static LAST: AtomicU64 = AtomicU64::new(0);
            static SKIPPED: AtomicU64 = AtomicU64::new(0);
            let now = tick();
            if now.saturating_sub(LAST.load(Relaxed)) >= 5000 {
                LAST.store(now, Relaxed);
                stereo_log(&format!(
                    "[hud] surface named by DrawHUD: res={:016X} {}x{} mips={} fmt={} (+{} more adoptions since the last line -- the engine pools these)",
                    res as usize,
                    d.Width,
                    d.Height,
                    d.MipLevels,
                    d.Format.0,
                    SKIPPED.swap(0, Relaxed)
                ));
            } else {
                SKIPPED.fetch_add(1, Relaxed);
            }
        }
    }
    if !NODE_NAMED.swap(true, AcqRel) {
        stereo_log(&format!(
            "[hud] identification switched to node 0x{:X} -- descriptor matching disabled",
            CyberpunkVR_HudNodeRva.load(Relaxed)
        ));
    }
    diag(&CyberpunkVR_DebugHudNodeNames);
}
/// An RTV created for `res` at `handle`: mip 0 of a HUD-shaped target becomes the surface (unless the node has
/// named one, or a producing surface holds the floor); a recycled handle of the surface is forgotten.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_register_rtv(
    res: *mut c_void,
    view: *const D3D12_RENDER_TARGET_VIEW_DESC,
    handle: usize,
) {
    if res.is_null() || handle == 0 {
        return;
    }
    if CyberpunkVR_HudByNode.load(Relaxed) != 0 && NODE_NAMED.load(Acquire) {
        return;
    }
    let r = rt();
    let held = *r.hud_res;
    let hold = CyberpunkVR_HudHoldMs.load(Relaxed);
    if hold != 0 && res != held && !held.is_null() {
        let t = (*r.hud_snap_tick).load(Acquire);
        if t != 0 && tick() - t < hold as u64 {
            diag(&CyberpunkVR_DebugHudHolds);
            return;
        }
    }
    // Mip 0 only: the glow chain's mips must not count as the HUD target.
    if let Some(v) = view.as_ref() {
        if v.ViewDimension != D3D12_RTV_DIMENSION_TEXTURE2D || v.Anonymous.Texture2D.MipSlice != 0 {
            return;
        }
    }
    let d = get_desc(res);
    let is_hud = d.is_some_and(|d| hud_rt_signature(&d));
    let mut s = surface();
    if !is_hud {
        // A descriptor slot of the surface reused for something else.
        if let Some(i) = s.handles.iter().position(|&h| h == handle) {
            s.handles.swap_remove(i);
        }
        return;
    }
    let d = d.unwrap();
    if res != *r.hud_res {
        adopt(&mut s, res, &d);
        stereo_log(&format!(
            "[hud] surface res={:016X} {}x{} mips={} fmt={}",
            res as usize, d.Width, d.Height, d.MipLevels, d.Format.0
        ));
    }
    if !s.handles.contains(&handle) && s.handles.len() < 8 {
        s.handles.push(handle);
    }
}

// ---- the snapshots: copies appended to the engine's own list at the unbind

const SLOT_HUD: usize = 0;
const SLOT_MAIN_OUT: usize = 2;
const SLOT_VISION: usize = 4;
const SLOT_NAMES: [&str; 5] = [
    "CyberpunkVR_HudSnapshot",
    "CyberpunkVR_HudBlurSnapshot",
    "CyberpunkVR_MainOutSnapshot",
    "CyberpunkVR_MainSceneSnapshot",
    "CyberpunkVR_VisionSnapshot",
];
/// The five slots' textures and descriptions (slot 0's description is Hud.cpp's g_hud_snap_desc), kept under
/// g_hud_snap_mtx.
struct Slots {
    tex: [*mut c_void; 5],
    desc: [D3D12_RESOURCE_DESC; 5],
    realloc: [u32; 5],
}
struct SlotCell(UnsafeCell<Slots>);
// SAFETY: only touched between snap_lock and snap_unlock.
unsafe impl Sync for SlotCell {}
static SLOTS: SlotCell = SlotCell(UnsafeCell::new(Slots {
    tex: [core::ptr::null_mut(); 5],
    desc: [D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION(0),
        Alignment: 0,
        Width: 0,
        Height: 0,
        DepthOrArraySize: 0,
        MipLevels: 0,
        Format: DXGI_FORMAT(0),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 0,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT(0),
        Flags: D3D12_RESOURCE_FLAGS(0),
    }; 5],
    realloc: [0; 5],
}));
static FRESH: [AtomicBool; 5] = [const { AtomicBool::new(false) }; 5];
/// g_vision_consumed_tick: the outline's demand.
static VISION_CONSUMED: AtomicU64 = AtomicU64::new(0);
struct SnapGuard;
impl SnapGuard {
    fn lock() -> SnapGuard {
        (rt().snap_lock)();
        SnapGuard
    }
    #[allow(clippy::mut_from_ref)]
    unsafe fn slots(&self) -> &mut Slots {
        &mut *SLOTS.0.get()
    }
}
impl Drop for SnapGuard {
    fn drop(&mut self) {
        (rt().snap_unlock)();
    }
}
fn fresh(slot: usize) -> &'static AtomicBool {
    if slot == SLOT_HUD {
        // SAFETY: a C++ global, attached.
        unsafe { &*rt().hud_snap_fresh }
    } else {
        &FRESH[slot]
    }
}
/// The D3D12 transition barrier's layout.
#[repr(C)]
pub(crate) struct Barrier {
    kind: u32,
    flags: u32,
    resource: *mut c_void,
    subresource: u32,
    before: u32,
    after: u32,
    _pad: u32,
}
impl Barrier {
    /// A transition of every subresource of `resource`.
    pub(crate) fn transition(resource: *mut c_void, before: u32, after: u32) -> Self {
        Barrier {
            kind: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION.0 as u32,
            flags: 0,
            resource,
            subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            before,
            after,
            _pad: 0,
        }
    }
}
const _: () =
    assert!(core::mem::size_of::<Barrier>() == core::mem::size_of::<D3D12_RESOURCE_BARRIER>());
/// The finished `src` (resting in `rest`) copied into the slot's committed texture of ours on `list`, between two
/// transitions -- in queue order after the last write and before any aliasing barrier can recycle the heap.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_snapshot_copy(
    list: *mut c_void,
    src: *mut c_void,
    which: i32,
    rest: u32,
) {
    let device = g_game_device;
    if device.is_null() || list.is_null() || src.is_null() {
        return;
    }
    let hook = cvr_cpp_command_list_hook_entry(list).as_ref();
    let d = get_desc(src);
    let (Some(hook), Some(d)) = (hook, d) else {
        diag(&CyberpunkVR_DebugHudSnapSkips);
        return;
    };
    if hook.barrier_call == 0 || hook.copyres == 0 {
        diag(&CyberpunkVR_DebugHudSnapSkips);
        return;
    }
    if !(0..5).contains(&which) {
        return;
    }
    let which = which as usize;
    let snap = {
        let guard = SnapGuard::lock();
        let s = guard.slots();
        let held = if which == SLOT_HUD {
            *rt().hud_snap_desc
        } else {
            s.desc[which]
        };
        if !s.tex[which].is_null()
            && (held.Width != d.Width
                || held.Height != d.Height
                || held.Format != d.Format
                || held.MipLevels != d.MipLevels)
        {
            // A caller alternating between two descriptions is a bug in the caller: capped, not leaked away.
            s.realloc[which] += 1;
            if s.realloc[which] > 8 {
                if s.realloc[which] == 9 {
                    stereo_log(&format!(
                        "[hud] snapshot[{which}] REFUSED: caller alternates between descs ({}x{} fmt={} vs {}x{} fmt={}) -- ignoring further changes",
                        held.Width, held.Height, held.Format.0, d.Width, d.Height, d.Format.0
                    ));
                }
                diag(&CyberpunkVR_DebugHudSnapSkips);
                return;
            }
            // The old one is leaked on purpose: a copy recorded this frame may still reference it.
            s.tex[which] = core::ptr::null_mut();
            fresh(which).store(false, Release);
        }
        if s.tex[which].is_null() {
            let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
                return;
            };
            let heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                CreationNodeMask: 1,
                VisibleNodeMask: 1,
                ..Default::default()
            };
            let mut nd = d;
            nd.Flags = D3D12_RESOURCE_FLAG_NONE;
            // MAIN's finished frame holds sRGB-encoded bytes: typeless lets the reader decode.
            if which == SLOT_MAIN_OUT && nd.Format == DXGI_FORMAT_R8G8B8A8_UNORM {
                nd.Format = DXGI_FORMAT_R8G8B8A8_TYPELESS;
            }
            let mut tex: Option<ID3D12Resource> = None;
            if dev
                .CreateCommittedResource(
                    &heap,
                    D3D12_HEAP_FLAG_NONE,
                    &nd,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut tex,
                )
                .is_err()
            {
                diag(&CyberpunkVR_DebugHudSnapSkips);
                return;
            }
            let Some(tex) = tex else {
                diag(&CyberpunkVR_DebugHudSnapSkips);
                return;
            };
            let name: Vec<u16> = SLOT_NAMES[which].encode_utf16().chain([0]).collect();
            let _ = tex.SetName(windows::core::PCWSTR(name.as_ptr()));
            let raw = tex.into_raw();
            s.tex[which] = raw;
            if which == SLOT_HUD {
                *rt().hud_snap_desc = d;
            } else {
                s.desc[which] = d;
            }
            stereo_log(&format!(
                "[hud] snapshot[{which}]={:016X} {}x{} mips={} fmt={}",
                raw as usize, d.Width, d.Height, d.MipLevels, d.Format.0
            ));
        }
        s.tex[which]
    };
    let barrier = |resource, before, after| Barrier {
        kind: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION.0 as u32,
        flags: 0,
        resource,
        subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
        before,
        after,
        _pad: 0,
    };
    let copy_source = D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
    let copy_dest = D3D12_RESOURCE_STATE_COPY_DEST.0 as u32;
    let common = D3D12_RESOURCE_STATE_COMMON.0 as u32;
    let barrier_call: unsafe extern "system" fn(*mut c_void, u32, *const Barrier) =
        core::mem::transmute(hook.barrier_call);
    let copy: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) =
        core::mem::transmute(hook.copyres);
    let before = [
        barrier(src, rest, copy_source),
        barrier(snap, common, copy_dest),
    ];
    barrier_call(list, 2, before.as_ptr());
    copy(list, snap, src);
    let after = [
        barrier(src, copy_source, rest),
        barrier(snap, copy_dest, common),
    ];
    barrier_call(list, 2, after.as_ptr());
    fresh(which).store(true, Release);
    if which == SLOT_HUD {
        (*rt().hud_snap_tick).store(tick(), Release);
    }
    diag(&CyberpunkVR_DebugHudSnaps);
}
unsafe fn slot_texture(slot: usize) -> *mut c_void {
    let guard = SnapGuard::lock();
    guard.slots().tex[slot]
}
/// The second eye's own outline layer: demand first and unconditionally, then freshness, then liveness.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_get_vision_texture() -> *mut c_void {
    if CyberpunkVR_VisionSnap.load(Relaxed) == 0 {
        return core::ptr::null_mut();
    }
    VISION_CONSUMED.store(tick(), Release);
    if !FRESH[SLOT_VISION].load(Acquire) {
        return core::ptr::null_mut();
    }
    let max_age = CyberpunkVR_VisionMaxAgeMs.load(Relaxed);
    if max_age != 0 {
        let t = (*rt().vision_tick).load(Acquire);
        if t == 0 || tick() - t > max_age as u64 {
            return core::ptr::null_mut();
        }
    }
    slot_texture(SLOT_VISION)
}
/// The finished HUD: DEMAND IS RECORDED FIRST, AND UNCONDITIONALLY (the producer keys on it, so it must mean "the
/// consumer asked", never "the consumer got something"), then freshness and liveness.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_get_hud_texture() -> *mut c_void {
    if CyberpunkVR_HudToSecondEye.load(Relaxed) == 0 {
        return core::ptr::null_mut();
    }
    let r = rt();
    (*r.hud_consumed_tick).store(tick(), Release);
    if !fresh(SLOT_HUD).load(Acquire) {
        return core::ptr::null_mut();
    }
    let max_age = CyberpunkVR_HudMaxAgeMs.load(Relaxed);
    if max_age != 0 {
        let t = (*r.hud_snap_tick).load(Acquire);
        if t == 0 || tick() - t > max_age as u64 {
            return core::ptr::null_mut();
        }
    }
    slot_texture(SLOT_HUD)
}
/// The blur pyramid (1), MAIN's finished frame (2) and the scene under it (3).
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_get_slot_texture(slot: u32) -> *mut c_void {
    let slot = slot as usize;
    if CyberpunkVR_HudToSecondEye.load(Relaxed) == 0
        || !(1..=3).contains(&slot)
        || !FRESH[slot].load(Acquire)
    {
        return core::ptr::null_mut();
    }
    slot_texture(slot)
}

// ---- the composite's constants, found in the upload ring and copied into a buffer of ours

static HELD_BLOCK: AtomicUsize = AtomicUsize::new(0);
/// Sweep the mapped upload rings for the composite's constants (by fingerprint, staying on the held block while it
/// qualifies) and take a private copy, so what is bound cannot be recycled by the engine's ring allocator.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_cb_rescan() {
    let device = g_game_device;
    if device.is_null() {
        return;
    }
    let r = rt();
    let now = tick();
    if *r.hud_cb_scan_tick != 0 && now - *r.hud_cb_scan_tick < 2000 {
        return;
    }
    *r.hud_cb_scan_tick = now;
    let (w, h) = {
        let _guard = SnapGuard::lock();
        let d = *r.hud_snap_desc;
        (d.Width as f32, d.Height as f32)
    };
    if !(w > 0.0 && h > 0.0) {
        return;
    }
    let mut maps = [MappedUpload::default(); 64];
    // SAFETY: 64 entries of upload_maps.rs's layout (the same 32 bytes).
    let n = unsafe { cvr_upload_maps_snapshot(maps.as_mut_ptr(), 64) }.min(64) as usize;
    let held = HELD_BLOCK.load(Relaxed);
    let mut found = if held != 0 && cvr_hud_block_plausible(held as *const u8, w, h) {
        held
    } else {
        0
    };
    'maps: for m in maps.iter().take(n) {
        if found != 0 {
            break;
        }
        if m.ptr == 0 || m.size < 512 {
            continue;
        }
        let mut off = 0u64;
        while off + 512 <= m.size {
            let p = m.ptr + off as usize;
            if cvr_hud_block_plausible(p as *const u8, w, h) {
                found = p;
                break 'maps;
            }
            off += 256;
        }
    }
    HELD_BLOCK.store(found, Relaxed);
    if found == 0 {
        return;
    }
    if (*r.hud_cb_copy).is_null() {
        let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
            return;
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 512,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut buffer: Option<ID3D12Resource> = None;
        if dev
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut buffer,
            )
            .is_err()
        {
            return;
        }
        let Some(buffer) = buffer else { return };
        let mut mapped: *mut c_void = core::ptr::null_mut();
        let none = D3D12_RANGE { Begin: 0, End: 0 };
        if buffer.Map(0, Some(&none), Some(&mut mapped)).is_err() || mapped.is_null() {
            return;
        }
        let name: Vec<u16> = "CyberpunkVR_HudConstants"
            .encode_utf16()
            .chain([0])
            .collect();
        let _ = buffer.SetName(windows::core::PCWSTR(name.as_ptr()));
        *r.hud_cb_copy = buffer.into_raw();
        *r.hud_cb_copy_ptr = mapped.cast();
    }
    let copy_ptr = *r.hud_cb_copy_ptr;
    core::ptr::copy_nonoverlapping(found as *const u8, copy_ptr, 512);
    // Take over even from an earlier capture; only the first hand-over logs.
    if !(*r.hud_cb_from_ring).load(Acquire) {
        let copy = *r.hud_cb_copy;
        let previous = (*r.hud_cb).swap(copy, AcqRel);
        (*r.hud_cb_from_ring).store(true, Release);
        if !previous.is_null() && previous != copy {
            release(previous);
        }
        CyberpunkVR_DebugHudCb.store(copy as u64, Relaxed);
        let f = |i: usize| (copy_ptr as *const f32).add(i).read_unaligned();
        stereo_log(&format!(
            "[hud] composite constants found in the upload ring{}: target={:.0}x{:.0} curvature=({:.6}, {:.6}) glow=({:.3}, {:.3}, {:.3}) aberration={:.6} halo=({:.3} lod {:.2})",
            if previous.is_null() { "" } else { " (replacing the copy-path capture)" },
            f(16 * 4 + 2),
            f(16 * 4 + 3),
            f(3 * 4),
            f(3 * 4 + 1),
            f(8 * 4 + 2),
            f(8 * 4 + 3),
            f(9 * 4),
            f(6 * 4 + 3),
            f(5 * 4),
            f(5 * 4 + 1)
        ));
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_get_constant_buffer() -> *mut c_void {
    cvr_hud_cb_rescan();
    (*rt().hud_cb).load(Acquire)
}
/// WHICH INPUT IS THE HUD STILL WAITING FOR: one line per changed combination; when the surface is the missing one,
/// why, in the same breath.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_note_composite_inputs(
    hud: *const c_void,
    blur: *const c_void,
    expo: *const c_void,
    frame_cb: *const c_void,
    hud_cb: *const c_void,
) {
    let mask = (!hud.is_null()) as u32
        | ((!blur.is_null()) as u32) << 1
        | ((!expo.is_null()) as u32) << 2
        | ((!frame_cb.is_null()) as u32) << 3
        | ((!hud_cb.is_null()) as u32) << 4;
    static LAST: AtomicU32 = AtomicU32::new(0xFFFF_FFFF);
    if LAST.swap(mask, AcqRel) == mask {
        return;
    }
    if mask == 31 {
        stereo_log("[hud] composite inputs complete -- the second eye gets the HUD");
        return;
    }
    let missing = |bit: u32, name: &'static str| if mask & bit != 0 { "" } else { name };
    if mask & 1 == 0 {
        let r = rt();
        stereo_log(&format!(
            "[hud] composite waiting on: surface{}{}{}{} | hud node binds={} of which unresolved={} | rtv map {}/{}{}",
            missing(2, " blur-pyramid"),
            missing(4, " exposure"),
            missing(8, " frame-constants"),
            missing(16, " composite-constants"),
            (*r.hud_node_binds).load(Relaxed),
            (*r.hud_node_unresolved).load(Relaxed),
            (*r.rtv_dim_count).load(Relaxed),
            crate::capture_rtv::cvr_capture_rtv_descriptor_limit(),
            if (*r.rtv_dim_wrapped_logged).load(Relaxed) { " (wrapped)" } else { "" }
        ));
        return;
    }
    stereo_log(&format!(
        "[hud] composite waiting on:{}{}{}{}{}",
        missing(1, " surface"),
        missing(2, " blur-pyramid"),
        missing(4, " exposure"),
        missing(8, " frame-constants"),
        missing(16, " composite-constants")
    ));
}

// ---- the capability grant

/// Idempotent OR into the view's own capability mask (deliberately not restored: a capability that exists only
/// while one node runs is the half-measure that crashed).
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_grant_capability(ctx: usize) {
    if CyberpunkVR_HudGrantCap.load(Relaxed) == 0 || ctx == 0 {
        return;
    }
    let (w, bits) = (
        CyberpunkVR_HudCapWord.load(Relaxed),
        CyberpunkVR_HudCapBits.load(Relaxed),
    );
    if w >= 32 || bits == 0 {
        return;
    }
    let slot = ctx + 0x2688 + 8 * w as usize;
    if let Some(have) = read::<u64>(slot) {
        if have & bits != bits && write(slot, have | bits) {
            diag(&CyberpunkVR_DebugHudCapGrants);
        }
    }
}
/// What a view's work context asked for against what it was given, once per view, and the addresses for a write
/// breakpoint on the block-list slot.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_dump_capability_mask(work_context: usize, vrcam: bool) {
    let r = rt();
    let dumped = if vrcam {
        r.hud_mask_dumped_vrcam
    } else {
        r.hud_mask_dumped_main
    };
    if *dumped || work_context == 0 || base() == 0 {
        return;
    }
    let who = if vrcam { "VRCAM" } else { "MAIN " };
    let _ = (|| -> Option<()> {
        let ctx: usize = read(work_context + 0x18)?;
        if ctx == 0 {
            return None;
        }
        let required = base() + *r.hud_required_mask_rva + 8;
        *dumped = true;
        let mut missing_words = 0;
        for i in 0..32 {
            let req: u64 = read(required + 8 * i)?;
            let have: u64 = read(ctx + 0x2688 + 8 * i)?;
            if req == 0 && have == 0 {
                continue;
            }
            let missing = req & !have;
            if req != 0 || missing != 0 {
                stereo_log(&format!("[hudmask] {who} w{i:02} req={req:016X} have={have:016X} missing={missing:016X}"));
            }
            if missing != 0 {
                missing_words += 1;
            }
        }
        stereo_log(&format!("[hudmask] {who} summary: qwords with missing bits = {missing_words} (0 means this view would pass)"));
        let null = core::ptr::null_mut();
        let vd = cvr_engine::code::guarded_call4(
            base() + 0x33C248,
            work_context as *mut c_void,
            null,
            null,
            null,
        )
        .unwrap_or(null) as usize;
        let blocks: usize = if vd != 0 { read(vd + 0x168)? } else { 0 };
        let layer_tag: u32 = read(work_context + 0x14)?;
        stereo_log(&format!(
            "[hudmask] {who} ctx={ctx:016X} viewData={vd:016X} blocks(+0x168)={blocks:016X} watch={:016X} layerTag=0x{layer_tag:02X}",
            if vd != 0 { vd + 0x168 } else { 0 }
        ));
        Some(())
    })();
}
