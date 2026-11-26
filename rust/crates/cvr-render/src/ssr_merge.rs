//! The two eyes' screen-space effects computed as ONE wide display (formerly src/Render/SsrWideMerge.cpp; the
//! reasoning is in its header, include/Render/SsrWideMerge.hpp and tools/asym_tests).
//!
//! The SSR and planar-reflection traces, the cone AO and the shadow mask can only use what their own picture shows;
//! under the asymmetric projection each eye's lens window reaches further outwards than the other's, so the eyes
//! disagree near their edges. The shader twins keep every pixel's result in a store of their own (u16) and look the
//! same surface point up in the OTHER eye's store (u17). This is the CPU side: the store pairs (one per channel), the
//! rows of the engine's blocks the twins read, and the binding of u16/u17 on the node's own command context through
//! the engine's binder, so the engine copies them into the pass's descriptor table like any of its own. Without the
//! tag the twins never touch the stores. Engine stereo has no second view to merge with: both eyes are one view.
//!
//! The auto-exposure's environment meter keeps its cells in a store pair of the same kind (channel `EXPOSURE`), bound
//! from the exposure blocks' rewrite (src/Stereo/Grading.cpp).
//!
//! The switches and counters are the C++ half's exports; the game's device and the internal-commands flag are asked
//! of it.
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cvr_engine::code;
use cvr_host::log;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::core::{Interface, PCWSTR};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

use crate::engine_stereo;

pub const ENGINE_BLOCK_BYTES: u32 = 656;
pub const BLOCK_BYTES: usize = 704;
pub const PIXEL_SIZE_ROW: usize = 32;
pub const CAMERA_ROW: usize = 41;
pub const STORE_ROW: usize = 42;
pub const MODE_ROW: usize = 43;
pub const TAG: u32 = 0x4173_4D31;
pub const HEADER_ENTRIES: u32 = 52;
pub const ENTRY_BYTES: u64 = 12;
pub const OWN_SLOT: u32 = 16;
pub const OTHER_SLOT: u32 = 17;
pub const REFLECTIONS: usize = 0;
pub const PLANAR: usize = 1;
pub const CONE_AO: usize = 2;
pub const SHADOW_MASK: usize = 3;
pub const EXPOSURE: usize = 4;
const CHANNELS: usize = 5;
pub const AO_BLOCK_BYTES: u32 = 240;
const AO_MERGE_ROW: usize = 11;
const AO_MODE_ROW: usize = 13;
const AO_CENTRE_ROW: usize = 12;
const AO_CENTRE_TAG: u32 = 0x4173_4331;
const AO_TAG: u32 = 0x4173_4D41;
pub const SHADOW_BLOCK_BYTES: u32 = 64;
const SHADOW_TAG: u32 = 0x4173_4D50;
pub const EXPOSURE_CELLS: u32 = 60;
const DESCRIPTOR_PAIRS: u32 = 8;
/// Render/ViewCameraLayout.hpp: the view's scene camera descriptor.
const VIEW_CAMERA: usize = 0xE0;
/// The engine's command context of the node being recorded (sub_140337230) and its binder (sub_140336D3C).
const CURRENT_CONTEXT_RVA: usize = 0x337230;
const BIND_RVA: usize = 0x336D3C;
const STORE_NAMES: [[&str; 2]; CHANNELS] = [
    [
        "CyberpunkVR SSR store MAIN",
        "CyberpunkVR SSR store second view",
    ],
    [
        "CyberpunkVR SSPR store MAIN",
        "CyberpunkVR SSPR store second view",
    ],
    [
        "CyberpunkVR cone AO store MAIN",
        "CyberpunkVR cone AO store second view",
    ],
    [
        "CyberpunkVR shadow mask store MAIN",
        "CyberpunkVR shadow mask store second view",
    ],
    [
        "CyberpunkVR exposure cells MAIN",
        "CyberpunkVR exposure cells second view",
    ],
];

/// The exports and the questions the C++ half answers.
#[repr(C)]
pub struct Runtime {
    pub ssr_merge: *const AtomicU32,
    pub sspr_merge: *const AtomicU32,
    pub ssr_merge_mode: *const u32,
    pub ssr_merge_tolerance: *const f32,
    pub debug_ssr: *mut u64,
    pub debug_sspr: *mut u64,
    pub debug_ssr_blocks: *mut f32,
    pub debug_sspr_blocks: *mut f32,
    pub asym_ssr_patch: *const AtomicU32,
    pub asym_ssr_patch_loaded: *const AtomicU32,
    pub asym_ao_patch: *const AtomicU32,
    pub asym_ao_patch_loaded: *const AtomicU32,
    pub ao_merge: *const AtomicU32,
    pub ao_merge_mode: *const u32,
    pub ao_merge_tolerance: *const f32,
    pub ao_merge_strength: *const f32,
    pub debug_ao: *mut u64,
    pub debug_ao_blocks: *mut f32,
    pub asym_shadow_patch: *const AtomicU32,
    pub asym_shadow_patch_loaded: *const AtomicU32,
    pub shadow_merge: *const AtomicU32,
    pub shadow_merge_mode: *const u32,
    pub debug_shadow: *mut u64,
    pub debug_shadow_blocks: *mut u32,
    pub expo_cells_request: *mut u32,
    pub expo_cells_seq: *mut u32,
    pub expo_cells: *mut u32,
    pub probe_layout_node: *const u32,
    pub layout_count: *mut u32,
    pub layout_bytes: *mut u8,
    /// cvr::detail::g_game_device.
    pub game_device: extern "C" fn() -> *mut c_void,
}
// SAFETY: exported variables and functions of the C++ half, alive for the process.
unsafe impl Sync for Runtime {}
unsafe impl Send for Runtime {}

static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());

fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached once with a pointer the C++ half keeps for the process.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}

#[no_mangle]
pub extern "C" fn cvr_ssr_merge_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}

fn on(p: *const AtomicU32) -> bool {
    // SAFETY: an exported switch.
    unsafe { (*p).load(Ordering::Relaxed) != 0 }
}
unsafe fn get<T: Copy>(p: *const T) -> T {
    // SAFETY: an export.
    unsafe { core::ptr::read_volatile(p) }
}
fn bump(debug: *mut u64, i: usize) {
    // SAFETY: an exported counter array of eight.
    unsafe {
        let p = debug.add(i);
        core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
    }
}
fn set(debug: *mut u64, i: usize, v: u64) {
    // SAFETY: as above.
    unsafe { core::ptr::write_volatile(debug.add(i), v) };
}
fn now() -> u64 {
    // SAFETY: no arguments.
    unsafe { GetTickCount64() }
}
fn put_u32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_ne_bytes());
}
fn put_f32(out: &mut [u8], at: usize, v: f32) {
    out[at..at + 4].copy_from_slice(&v.to_ne_bytes());
}
fn get_f32(b: &[u8], at: usize) -> f32 {
    f32::from_ne_bytes(b[at..at + 4].try_into().unwrap_or([0; 4]))
}
fn get_u32(b: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(b[at..at + 4].try_into().unwrap_or([0; 4]))
}

// ---- the stores

/// Two RWStructuredBuffer<uint3>, zeroed at creation (no tag in the header, so nothing reads as valid), in COMMON:
/// promoted to UNORDERED_ACCESS on use and decaying back between submissions.
struct StoreSet {
    buffers: [ID3D12Resource; 2],
    uav: [usize; 2],
    capacity: u32,
}

struct Stores {
    heap: Option<ID3D12DescriptorHeap>,
    stride: u32,
    next_pair: u32,
    current: [Option<Arc<StoreSet>>; CHANNELS],
    refused: [u32; CHANNELS],
    retired: Vec<(u64, Arc<StoreSet>)>,
}
static STORES: Mutex<Stores> = Mutex::new(Stores {
    heap: None,
    stride: 0,
    next_pair: 0,
    current: [None, None, None, None, None],
    refused: [0; CHANNELS],
    retired: Vec::new(),
});
static STAMPS: [[AtomicU32; 2]; CHANNELS] = [const { [const { AtomicU32::new(0) }; 2] }; CHANNELS];

fn stores() -> MutexGuard<'static, Stores> {
    STORES.lock().unwrap_or_else(|p| p.into_inner())
}

fn channel_name(channel: usize) -> &'static str {
    match channel {
        PLANAR => "planar",
        CONE_AO => "cone AO",
        SHADOW_MASK => "shadow mask",
        EXPOSURE => "exposure",
        _ => "SSR",
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn ensure(rt: &Runtime, channel: usize, capacity: u32) -> Option<Arc<StoreSet>> {
    let mut s = stores();
    let now = now();
    s.retired.retain(|(at, _)| now.wrapping_sub(*at) <= 10000);
    if let Some(current) = &s.current[channel] {
        if current.capacity >= capacity {
            return Some(current.clone());
        }
    }
    if s.refused[channel] == capacity {
        return None;
    }
    let device = (rt.game_device)();
    // SAFETY: the game's device, alive while it renders.
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }?;
    if s.heap.is_none() {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: DESCRIPTOR_PAIRS * 2,
            ..Default::default()
        };
        // SAFETY: a plain descriptor heap.
        match unsafe { device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) } {
            Ok(heap) => {
                // SAFETY: a query of the device.
                s.stride = unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) };
                s.heap = Some(heap);
            }
            Err(_) => {
                s.refused[channel] = capacity;
                return None;
            }
        }
    }
    let properties = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: capacity as u64 * ENTRY_BYTES,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let pair = s.next_pair % DESCRIPTOR_PAIRS;
    s.next_pair = s.next_pair.wrapping_add(1);
    let heap = s.heap.clone()?;
    // SAFETY: the heap's own start.
    let start = unsafe { heap.GetCPUDescriptorHandleForHeapStart() }.ptr;
    let mut buffers: Vec<ID3D12Resource> = Vec::with_capacity(2);
    let mut uav = [0usize; 2];
    for side in 0..2 {
        let mut buffer: Option<ID3D12Resource> = None;
        // SAFETY: a committed buffer of the device.
        let created = unsafe {
            device.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut buffer,
            )
        };
        let (Ok(()), Some(buffer)) = (created, buffer) else {
            s.refused[channel] = capacity;
            log!(
                "[ssr-merge] store of {} entries could not be created (channel {})\n",
                capacity,
                channel
            );
            return None;
        };
        let name = wide(STORE_NAMES[channel][side]);
        // SAFETY: a NUL-terminated name.
        let _ = unsafe { buffer.SetName(PCWSTR(name.as_ptr())) };
        let view = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            ViewDimension: D3D12_UAV_DIMENSION_BUFFER,
            Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                Buffer: D3D12_BUFFER_UAV {
                    NumElements: capacity,
                    StructureByteStride: ENTRY_BYTES as u32,
                    ..Default::default()
                },
            },
            ..Default::default()
        };
        uav[side] = start + (pair * 2 + side as u32) as usize * s.stride as usize;
        // SAFETY: the view of this buffer, into the heap's own slot.
        unsafe {
            device.CreateUnorderedAccessView(
                &buffer,
                None,
                Some(&view),
                D3D12_CPU_DESCRIPTOR_HANDLE { ptr: uav[side] },
            )
        };
        buffers.push(buffer);
    }
    let [b0, b1]: [ID3D12Resource; 2] = buffers.try_into().ok()?;
    let set = Arc::new(StoreSet {
        buffers: [b0, b1],
        uav,
        capacity,
    });
    if let Some(old) = s.current[channel].take() {
        s.retired.push((now, old));
    }
    s.current[channel] = Some(set.clone());
    log!(
        "[ssr-merge] {} stores of {} entries ({:.1} MB each)\n",
        channel_name(channel),
        capacity,
        capacity as f64 * ENTRY_BYTES as f64 / 1048576.0
    );
    Some(set)
}

/// The command context the node records on: its binding table (+0x60) and its list (+0x30).
fn current_context() -> Option<(usize, usize)> {
    let base = code::image_base();
    // SAFETY: the engine's own accessor, inside the SEH adapter.
    let context = unsafe {
        code::guarded_call4(
            base + CURRENT_CONTEXT_RVA,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    }? as usize;
    if context == 0 {
        return None;
    }
    let table = code::read::<usize>(context + 0x60)?;
    let list = code::read::<usize>(context + 0x30)?;
    (table != 0 && list != 0).then_some((table, list))
}

/// The binder's own lookup: the handle now in UAV slot `slot` of `stage`.
fn bound_uav(table: usize, slot: u32, stage: u8) -> u64 {
    let read = || -> Option<u64> {
        let layout = code::read::<usize>(table + 13 * 8)?;
        let range = code::read::<u8>(layout + 2243 + 38 * stage as usize + 2 * slot as usize)?;
        if range == 0xFF {
            return Some(0);
        }
        let entry = layout + 56 + 16 * range as usize;
        let index = slot
            .wrapping_add(code::read::<u32>(entry + 4)?)
            .wrapping_sub(code::read::<u16>(entry + 8)? as u32);
        let handles = code::read::<usize>(table + 5 * 8)?;
        code::read::<u64>(handles + 8 * index as usize)
    };
    read().unwrap_or(0)
}

/// D3D12_RESOURCE_BARRIER as written here (transition and UAV).
#[repr(C)]
struct Barrier {
    kind: u32,
    flags: u32,
    resource: usize,
    subresource: u32,
    before: u32,
    after: u32,
    pad: u32,
}

unsafe fn resource_barrier(list: usize, barriers: &[Barrier]) {
    // SAFETY: ID3D12GraphicsCommandList::ResourceBarrier (slot 26) on the node's own list.
    unsafe {
        let table = *(list as *const *const usize);
        let f: unsafe extern "system" fn(usize, u32, *const Barrier) =
            core::mem::transmute(*table.add(26));
        f(list, barriers.len() as u32, barriers.as_ptr());
    }
}

/// The channel's stores bound as u16 (this eye's) and u17 (the other eye's) on the command context the node records
/// on, and ordered against the dispatches that touched them before. None (counted) when anything refused. `stage`: the
/// binder's shader stage, 2 compute, 1 pixel.
fn bind_stores(
    rt: &Runtime,
    channel: usize,
    side: usize,
    capacity: u32,
    debug: *mut u64,
    stage: u8,
) -> Option<Arc<StoreSet>> {
    let Some(stores) = ensure(rt, channel, capacity) else {
        bump(debug, 4);
        return None;
    };
    let Some((table, list)) = current_context() else {
        bump(debug, 3);
        return None;
    };
    let own = stores.uav[side];
    let other = stores.uav[side ^ 1];
    // SAFETY: the engine's binder on its own table.
    unsafe {
        let bind: unsafe extern "C" fn(usize, u8, u32, u32, *const usize, bool) =
            core::mem::transmute(code::image_base() + BIND_RVA);
        bind(table, stage, 1, OWN_SLOT, &own, true);
        bind(table, stage, 1, OTHER_SLOT, &other, true);
    }
    // A slot the binder did not take would leave the twins' slots pointing at whatever the table held before; they
    // must then not see the tag.
    if bound_uav(table, OWN_SLOT, stage) != own as u64
        || bound_uav(table, OTHER_SLOT, stage) != other as u64
    {
        bump(debug, 3);
        return None;
    }
    let _internal = cvr_host::probe::Internal::new();
    let barriers = [0, 1].map(|i| Barrier {
        kind: D3D12_RESOURCE_BARRIER_TYPE_UAV.0 as u32,
        flags: 0,
        resource: stores.buffers[i].as_raw() as usize,
        subresource: 0,
        before: 0,
        after: 0,
        pad: 0,
    });
    // SAFETY: the node's own list.
    unsafe { resource_barrier(list, &barriers) };
    Some(stores)
}

fn read_camera(view: usize) -> Option<[f32; 3]> {
    let fixed = code::read::<[i32; 3]>(view + VIEW_CAMERA)?;
    let c = fixed.map(|x| (x as f64 / 131072.0) as f32);
    c.iter().all(|v| v.is_finite()).then_some(c)
}

/// Prepare: the SSR or planar-reflection trace's block grown to 704 bytes with the camera, the store and the mode.
///
/// # Safety
/// `src` is the engine's 656-byte block; `out` has room for 704 bytes; `ship` is a u32.
#[no_mangle]
pub unsafe extern "C" fn cvr_ssr_merge_prepare(
    channel: i32,
    side: i32,
    view: usize,
    src: *const u8,
    size: u32,
    out: *mut u8,
    ship: *mut u32,
) -> bool {
    let Some(rt) = runtime() else { return false };
    let channel = channel as usize;
    if size != ENGINE_BLOCK_BYTES
        || src.is_null()
        || out.is_null()
        || ship.is_null()
        || (channel != REFLECTIONS && channel != PLANAR)
    {
        return false;
    }
    let debug = if channel == PLANAR {
        rt.debug_sspr
    } else {
        rt.debug_ssr
    };
    if side != 0 && side != 1 {
        bump(debug, 2);
        return false;
    }
    let enabled = if channel == PLANAR {
        rt.sspr_merge
    } else {
        rt.ssr_merge
    };
    if !on(enabled)
        || !on(rt.asym_ssr_patch)
        || !on(rt.asym_ssr_patch_loaded)
        || code::image_base() == 0
        || engine_stereo::enabled()
    {
        return false;
    }
    // SAFETY: the caller's buffer.
    let out = unsafe { core::slice::from_raw_parts_mut(out, BLOCK_BYTES) };
    out.fill(0);
    if !code::read_bytes(src as usize, &mut out[..size as usize]) {
        bump(debug, 5);
        return false;
    }
    // Row 32.zw is the pixel size in NDC, 2 / width and 2 / height.
    let pixel = [
        get_f32(out, PIXEL_SIZE_ROW * 16 + 8),
        get_f32(out, PIXEL_SIZE_ROW * 16 + 12),
    ];
    let w = if pixel[0] > 0.0 {
        2.0 / pixel[0] as f64
    } else {
        0.0
    };
    let h = if pixel[1] > 0.0 {
        2.0 / pixel[1] as f64
    } else {
        0.0
    };
    let (width, height) = (w.round() as i64 as u32, h.round() as i64 as u32);
    let camera = if view != 0 { read_camera(view) } else { None };
    if !((16..=16384).contains(&width) && (16..=16384).contains(&height))
        || (w - width as f64).abs() > 0.01
        || (h - height as f64).abs() > 0.01
    {
        bump(debug, 5);
        return false;
    }
    let Some(camera) = camera else {
        bump(debug, 5);
        return false;
    };
    let capacity = HEADER_ENTRIES + width * height;
    let stamp = STAMPS[channel][side as usize]
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    if bind_stores(rt, channel, side as usize, capacity, debug, 2).is_none() {
        return false;
    }
    for (k, v) in camera.iter().enumerate() {
        put_f32(out, CAMERA_ROW * 16 + 4 * k, *v);
    }
    put_u32(out, CAMERA_ROW * 16 + 12, TAG);
    for (k, v) in [stamp, width, height, capacity].iter().enumerate() {
        put_u32(out, STORE_ROW * 16 + 4 * k, *v);
    }
    // SAFETY: exports.
    let (mode, mut tolerance) = unsafe { (get(rt.ssr_merge_mode), get(rt.ssr_merge_tolerance)) };
    if !tolerance.is_finite() || tolerance < 0.0 {
        tolerance = 0.0;
    }
    put_u32(out, MODE_ROW * 16, mode);
    put_f32(out, MODE_ROW * 16 + 4, tolerance);
    // SAFETY: the caller's size slot and the debug block (704 bytes per side).
    unsafe {
        *ship = BLOCK_BYTES as u32;
        bump(debug, side as usize);
        set(debug, 6, capacity as u64);
        set(debug, 7, ((width as u64) << 16) | height as u64);
        let blocks = if channel == PLANAR {
            rt.debug_sspr_blocks
        } else {
            rt.debug_ssr_blocks
        };
        core::ptr::copy_nonoverlapping(
            out.as_ptr(),
            (blocks as *mut u8).add(side as usize * BLOCK_BYTES),
            BLOCK_BYTES,
        );
    }
    true
}

/// PrepareConeAo: the cone AO's block (already centred) given the merge's stamp, tag, tolerance, strength and mode.
///
/// # Safety
/// `block` is a 240-byte block of the caller's.
#[no_mangle]
pub unsafe extern "C" fn cvr_ssr_merge_prepare_cone_ao(
    side: i32,
    block: *mut u8,
    size: u32,
) -> bool {
    let Some(rt) = runtime() else { return false };
    let debug = rt.debug_ao;
    if size != AO_BLOCK_BYTES || block.is_null() {
        return false;
    }
    if side != 0 && side != 1 {
        bump(debug, 2);
        return false;
    }
    if !on(rt.ao_merge)
        || !on(rt.asym_ao_patch)
        || !on(rt.asym_ao_patch_loaded)
        || code::image_base() == 0
        || engine_stereo::enabled()
    {
        return false;
    }
    // SAFETY: the caller's block.
    let b = unsafe { core::slice::from_raw_parts_mut(block, AO_BLOCK_BYTES as usize) };
    // The header is the block's own view->world rows and centred tangents: without the centre the twins'
    // reconstruction is not the view space those rows describe.
    let centre_tag = get_u32(b, AO_CENTRE_ROW * 16 + 12);
    let dims = [get_f32(b, 16 + 8), get_f32(b, 16 + 12)];
    let (width, height) = (dims[0].round() as i64 as u32, dims[1].round() as i64 as u32);
    if centre_tag != AO_CENTRE_TAG
        || !((16..=16384).contains(&width) && (16..=16384).contains(&height))
        || (dims[0] - width as f32).abs() > 0.01
        || (dims[1] - height as f32).abs() > 0.01
    {
        bump(debug, 5);
        return false;
    }
    let capacity = HEADER_ENTRIES + width * height;
    if bind_stores(rt, CONE_AO, side as usize, capacity, debug, 2).is_none() {
        return false;
    }
    let stamp = STAMPS[CONE_AO][side as usize]
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    // SAFETY: exports.
    let (mut tolerance, mut strength, mode) = unsafe {
        (
            get(rt.ao_merge_tolerance),
            get(rt.ao_merge_strength),
            get(rt.ao_merge_mode),
        )
    };
    if !tolerance.is_finite() || tolerance < 0.0 {
        tolerance = 0.0;
    }
    if !strength.is_finite() || strength < 0.0 {
        strength = 0.0;
    }
    put_u32(b, AO_MERGE_ROW * 16, stamp);
    put_u32(b, AO_MERGE_ROW * 16 + 4, AO_TAG);
    put_f32(b, AO_MERGE_ROW * 16 + 8, tolerance);
    put_f32(b, AO_MERGE_ROW * 16 + 12, strength);
    put_u32(b, AO_MODE_ROW * 16 + 8, mode);
    bump(debug, side as usize);
    set(debug, 6, capacity as u64);
    set(debug, 7, ((width as u64) << 16) | height as u64);
    // SAFETY: the debug block, 240 bytes per side.
    unsafe {
        core::ptr::copy_nonoverlapping(
            b.as_ptr(),
            (rt.debug_ao_blocks as *mut u8).add(side as usize * AO_BLOCK_BYTES as usize),
            AO_BLOCK_BYTES as usize,
        )
    };
    true
}

/// PrepareShadowMask: the shadow mask's block given the store's stamp and the tag (low four bits: the mode). The mask
/// is drawn at half width (checkerboard); its store holds one entry per drawn pixel.
///
/// # Safety
/// `block` is a 64-byte block of the caller's.
#[no_mangle]
pub unsafe extern "C" fn cvr_ssr_merge_prepare_shadow_mask(
    side: i32,
    view: usize,
    block: *mut u8,
    size: u32,
) -> bool {
    let Some(rt) = runtime() else { return false };
    let debug = rt.debug_shadow;
    if size != SHADOW_BLOCK_BYTES || block.is_null() {
        return false;
    }
    if side != 0 && side != 1 {
        bump(debug, 2);
        return false;
    }
    if !on(rt.shadow_merge)
        || !on(rt.asym_shadow_patch)
        || !on(rt.asym_shadow_patch_loaded)
        || code::image_base() == 0
        || engine_stereo::enabled()
    {
        return false;
    }
    // The view's render size: its scene camera descriptor's +0x378 width, +0x37C height.
    let dims = if view != 0 {
        code::read::<[u32; 2]>(view + VIEW_CAMERA + 0x378)
    } else {
        None
    };
    let Some(dims) = dims.filter(|d| (16..=16384).contains(&d[0]) && (16..=16384).contains(&d[1]))
    else {
        bump(debug, 5);
        return false;
    };
    let capacity = HEADER_ENTRIES + (dims[0] / 2 + 1) * dims[1];
    if bind_stores(rt, SHADOW_MASK, side as usize, capacity, debug, 1).is_none() {
        return false;
    }
    let stamp = STAMPS[SHADOW_MASK][side as usize]
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    // SAFETY: the caller's block; an export.
    let b = unsafe { core::slice::from_raw_parts_mut(block, SHADOW_BLOCK_BYTES as usize) };
    let tag = SHADOW_TAG | (unsafe { get(rt.shadow_merge_mode) } & 15);
    put_u32(b, 3 * 16 + 8, stamp);
    put_u32(b, 3 * 16 + 12, tag);
    bump(debug, side as usize);
    set(debug, 6, capacity as u64);
    set(debug, 7, ((dims[0] as u64) << 16) | dims[1] as u64);
    // SAFETY: the debug block, 64 bytes per side.
    unsafe {
        core::ptr::copy_nonoverlapping(
            b.as_ptr(),
            (rt.debug_shadow_blocks as *mut u8).add(side as usize * SHADOW_BLOCK_BYTES as usize),
            SHADOW_BLOCK_BYTES as usize,
        )
    };
    true
}

// ---- the exposure cells

struct Snapshot {
    readback: Option<ID3D12Resource>,
    pending: i32,
}
static SNAPSHOT: Mutex<Snapshot> = Mutex::new(Snapshot {
    readback: None,
    pending: 0,
});

/// After the luminance image's samples went in (the adaptation's bind): a request copies the cells and the frame's
/// sums to a readback buffer; a few binds later they are in CyberpunkVR_DebugExpoCells.
fn snapshot_cells(rt: &Runtime, cells: &ID3D12Resource) {
    let bytes = 2 * EXPOSURE_CELLS as u64 * ENTRY_BYTES;
    let mut s = SNAPSHOT.lock().unwrap_or_else(|p| p.into_inner());
    if s.pending > 0 {
        s.pending -= 1;
        if s.pending == 0 {
            if let Some(readback) = &s.readback {
                let mut p: *mut c_void = core::ptr::null_mut();
                let range = D3D12_RANGE {
                    Begin: 0,
                    End: bytes as usize,
                };
                // SAFETY: the readback buffer, mapped for its whole size; the export holds 2 x 60 x 3 words.
                unsafe {
                    if readback.Map(0, Some(&range), Some(&mut p)).is_ok() && !p.is_null() {
                        core::ptr::copy_nonoverlapping(
                            p as *const u8,
                            rt.expo_cells as *mut u8,
                            bytes as usize,
                        );
                        let none = D3D12_RANGE { Begin: 0, End: 0 };
                        readback.Unmap(0, Some(&none));
                        core::ptr::write_volatile(
                            rt.expo_cells_seq,
                            core::ptr::read_volatile(rt.expo_cells_seq).wrapping_add(1),
                        );
                    }
                }
            }
        }
    }
    // SAFETY: the request export.
    if unsafe { get(rt.expo_cells_request) } == 0 || s.pending != 0 {
        return;
    }
    // SAFETY: as above.
    unsafe { core::ptr::write_volatile(rt.expo_cells_request, 0) };
    let device = (rt.game_device)();
    // SAFETY: the game's device.
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return;
    };
    if s.readback.is_none() {
        let properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: bytes,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut readback: Option<ID3D12Resource> = None;
        // SAFETY: a readback buffer of the device.
        let created = unsafe {
            device.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut readback,
            )
        };
        if created.is_err() || readback.is_none() {
            return;
        }
        s.readback = readback;
    }
    let Some((_, list)) = current_context() else {
        return;
    };
    let Some(readback) = s.readback.clone() else {
        return;
    };
    // The luminance image's dispatch used the cells as a UAV in this list, which promoted them.
    let _internal = cvr_host::probe::Internal::new();
    let transition = |before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES| Barrier {
        kind: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION.0 as u32,
        flags: 0,
        resource: cells.as_raw() as usize,
        subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
        before: before.0 as u32,
        after: after.0 as u32,
        pad: 0,
    };
    // SAFETY: the node's own list; the cells and the readback buffer.
    unsafe {
        resource_barrier(
            list,
            &[transition(
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )],
        );
        if let Some(gl) = ID3D12GraphicsCommandList::from_raw_borrowed(&(list as *mut c_void)) {
            gl.CopyBufferRegion(&readback, 0, cells, 0, bytes);
        }
        resource_barrier(
            list,
            &[transition(
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            )],
        );
    }
    s.pending = 8;
}

/// BindExposureCells: the cells (then this frame's sums) bound for the exposure twins.
///
/// # Safety
/// `debug` is the caller's eight counters.
#[no_mangle]
pub unsafe extern "C" fn cvr_ssr_merge_bind_exposure_cells(
    side: i32,
    debug: *mut u64,
    snapshot: bool,
) -> bool {
    let Some(rt) = runtime() else { return false };
    if (side != 0 && side != 1) || debug.is_null() || code::image_base() == 0 {
        return false;
    }
    let Some(stores) = bind_stores(rt, EXPOSURE, side as usize, 2 * EXPOSURE_CELLS, debug, 2)
    else {
        return false;
    };
    if snapshot {
        snapshot_cells(rt, &stores.buffers[side as usize]);
    }
    true
}

/// ProbeLayout (diagnostics): the binding layout the first draw or dispatch of node CyberpunkVR_ProbeLayoutNode
/// records with -- the CBV, SRV and UAV range bytes per stage.
#[no_mangle]
pub extern "C" fn cvr_ssr_merge_probe_layout(node: u32) {
    let Some(rt) = runtime() else { return };
    // SAFETY: exports.
    unsafe {
        if node == 0
            || node != get(rt.probe_layout_node)
            || get(rt.layout_count) != 0
            || code::image_base() == 0
        {
            return;
        }
        let Some((table, _)) = current_context() else {
            return;
        };
        let copy = || -> Option<()> {
            let layout = code::read::<usize>(table + 13 * 8)?;
            let out = core::slice::from_raw_parts_mut(rt.layout_bytes, 4 * 14 + 4 * 128 + 4 * 19);
            let (cbv, rest) = out.split_at_mut(4 * 14);
            let (srv, uav) = rest.split_at_mut(4 * 128);
            (code::read_bytes(layout + 1107, cbv)
                && code::read_bytes(layout + 1219, srv)
                && code::read_bytes(layout + 2243, uav))
            .then_some(())
        };
        core::ptr::write_volatile(rt.layout_count, if copy().is_some() { 1 } else { 2 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_without_the_cpp_half() {
        let mut block = [0u8; 704];
        let mut ship = 0u32;
        // SAFETY: local buffers.
        unsafe {
            assert!(!cvr_ssr_merge_prepare(
                0,
                0,
                0,
                block.as_ptr(),
                656,
                block.as_mut_ptr(),
                &mut ship
            ));
            assert!(!cvr_ssr_merge_prepare_cone_ao(0, block.as_mut_ptr(), 240));
            assert!(!cvr_ssr_merge_prepare_shadow_mask(
                0,
                0,
                block.as_mut_ptr(),
                64
            ));
        }
        assert_eq!(channel_name(EXPOSURE), "exposure");
        assert_eq!(channel_name(REFLECTIONS), "SSR");
    }
}
