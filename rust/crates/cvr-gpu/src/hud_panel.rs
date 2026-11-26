//! The HUD panels' canvases (formerly src/Render/HudPanel.cpp; the file keeps `Publish`, `Capture`, `Latest`,
//! `SetConsumerReady`, `ConsumerReady` and `Shutdown` as forwarders, hands each published snapshot over with its
//! sprites' texture leases, which Rust holds until it is done and gives back to the C++ half, and attaches the engine's
//! HUD buffers).
//!
//! One pipeline per channel (the main HUD, interactions, the Basilisk, surveillance, hit markers, and since main's 0.2.4
//! the vehicle's minimap -- in the HUD, or projected over a car's hood or a bike's bars with the quest text under it). The HUD natives
//! publish a snapshot of the channel's separate-window sprites; each frame the capture draws the newest one into a
//! fenced slot of three -- the sprites over a cleared five-mip canvas, its mips and a half-size four-mip blur pyramid,
//! then the engine's own HUD composite (layer only, the panel's brightness, shadow and glow, the engine's curvature
//! when it has one) into the slot's canvas -- and publishes the slot as the channel's latest frame, which the XR quad
//! copies. A slot is reused once its fence passed and no consumer holds its frame. A snapshot whose windows show
//! nothing publishes an empty frame instead (no canvas, no GPU work); a snapshot older than 300 ms publishes none.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::color_blit::{Blit, HUD_PARAMS};
use crate::stage_profile;
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex};
use windows::core::{w, Interface};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

pub const CHANNELS: usize = 6;
/// cvr::hud::Channel::VehicleMinimap.
pub const VEHICLE_MINIMAP: u32 = 5;
/// cvr::hud::MaxSprites: the sprites one channel's snapshot takes.
pub const MAX_SPRITES: usize = 64;
const STAGE_HUD_SPRITES: u32 = 7;
const STAGE_HUD_BLUR: u32 = 8;
const STAGE_HUD_STYLE: u32 = 9;

/// What the capture asks of the C++ half (src/Render/HudPanel.cpp attaches it): the engine's buffers the composite
/// reads, the curvature export and the release of a sprite's texture lease.
#[repr(C)]
pub struct Runtime {
    pub frame_constants: unsafe extern "C" fn() -> *mut c_void,
    pub hud_constants: unsafe extern "C" fn() -> *mut c_void,
    pub exposure: unsafe extern "C" fn() -> *mut c_void,
    /// CyberpunkVR_HudEngineCurvature.
    pub curvature: *const [f32; 2],
    /// Gives back a lease handed over with a sprite (a held std::shared_ptr<cvr::hud::TextureLease>).
    pub lease_release: unsafe extern "C" fn(*mut c_void),
}
/// The plugin's table, built here (formerly the C++ half's); a test program attaches its own.
#[cfg(feature = "plugin-tables")]
mod plugin {
    use core::ffi::c_void;
    unsafe extern "C" {
        fn CyberpunkVR_GetFrameConstantBuffer() -> *mut c_void;
        fn CyberpunkVR_GetHudConstantBuffer() -> *mut c_void;
        fn CyberpunkVR_GetHudExposureBuffer() -> *mut c_void;
        static CyberpunkVR_HudEngineCurvature: [f32; 2];
        /// delete std::shared_ptr<cvr::hud::TextureLease>* (src/Render/HudPanel.cpp).
        fn cvr_cpp_texture_lease_release(lease: *mut c_void);
    }
    // SAFETY: process-lifetime exports and plain functions.
    unsafe impl Sync for super::Runtime {}
    pub(super) static RT: super::Runtime = super::Runtime {
        frame_constants: CyberpunkVR_GetFrameConstantBuffer,
        hud_constants: CyberpunkVR_GetHudConstantBuffer,
        exposure: CyberpunkVR_GetHudExposureBuffer,
        curvature: &raw const CyberpunkVR_HudEngineCurvature,
        lease_release: cvr_cpp_texture_lease_release,
    };
}
#[cfg(feature = "plugin-tables")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "plugin-tables"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
#[no_mangle]
pub extern "C" fn cvr_hud_panel_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Release);
}
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached at the C++ half's static initialisation, for the process.
    unsafe { RUNTIME.load(Acquire).as_ref() }
}
fn now() -> u64 {
    // SAFETY: plain Win32 call.
    unsafe { GetTickCount64() }
}

/// One sprite as the HUD natives hand it over.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SpriteView {
    /// The texture lease, given back through Runtime::lease_release.
    pub lease: *mut c_void,
    /// The lease's resource (an engine colour target in its shader-read state).
    pub resource: *mut c_void,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub tint: [f32; 4],
    pub name: u64,
}
/// A snapshot as the HUD natives hand it over.
#[repr(C)]
pub struct SnapshotView {
    pub width: u32,
    pub height: u32,
    pub stamp: u64,
    pub generation: u64,
    pub masked: bool,
    pub loot_visible: bool,
    pub empty_content: bool,
    pub sprites: *const SpriteView,
    pub count: usize,
    /// The vehicle map projected (the windshield style), the map's share of the canvas height, and the compact tile's
    /// normalized rectangle on the ordinary HUD canvas (x, y, width, height).
    pub windshield_map: bool,
    pub windshield_split: f32,
    pub hud_rect: [f32; 4],
}
struct Snapshot {
    width: u32,
    height: u32,
    stamp: u64,
    generation: u64,
    masked: bool,
    loot_visible: bool,
    empty_content: bool,
    sprites: Vec<SpriteView>,
    windshield_map: bool,
    windshield_split: f32,
    hud_rect: [f32; 4],
}
// SAFETY: the leases are the C++ half's shared pointers, released once from any thread.
unsafe impl Send for Snapshot {}
unsafe impl Sync for Snapshot {}
impl Drop for Snapshot {
    fn drop(&mut self) {
        if let Some(rt) = runtime() {
            for s in &self.sprites {
                if !s.lease.is_null() {
                    // SAFETY: a lease handed over once with this sprite.
                    unsafe { (rt.lease_release)(s.lease) };
                }
            }
        }
    }
}

/// cvr::hud::Frame: a slot's canvas and what it shows.
#[derive(Default)]
pub struct Frame {
    pub canvas: Option<ID3D12Resource>,
    pub stamp: u64,
    pub generation: u64,
    pub serial: u64,
    pub masked: bool,
    pub loot_visible: bool,
    /// No canvas: the channel shows nothing this frame (the snapshot's windows are empty).
    pub empty: bool,
    /// The producing queue's fence: the canvas is finished once it reaches `serial`. A reader on another queue copies it
    /// only after.
    pub fence: Option<ID3D12Fence>,
    pub windshield_map: bool,
    pub windshield_split: f32,
    pub hud_rect: [f32; 4],
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Frame {}
unsafe impl Sync for Frame {}

#[derive(Default)]
struct Slot {
    frame: Option<Arc<Frame>>,
    sources: Option<Arc<Snapshot>>,
    allocator: Option<ID3D12CommandAllocator>,
    commands: Option<ID3D12GraphicsCommandList>,
    rtv: Option<ID3D12DescriptorHeap>,
    // One ring per fenced slot: sprites, mips, blur and native HUD style.
    blit: Blit,
    raw: Option<ID3D12Resource>,
    blur: Option<ID3D12Resource>,
    blur_temp: Option<ID3D12Resource>,
    defaults: Option<ID3D12Resource>,
    default_data: usize,
    frame_constants: Option<ID3D12Resource>,
    hud_constants: Option<ID3D12Resource>,
    exposure: Option<ID3D12Resource>,
    fence_value: u64,
    pending: Option<PendingTextures>,
}
#[derive(Default)]
struct Render {
    latest: Option<Arc<Frame>>,
    slots: [Slot; 3],
    fence: Option<ID3D12Fence>,
    device_owner: Option<ID3D12Device>,
    submitted: u64,
    /// When the channel last had something to show (GetTickCount64 ms; 0 never).
    last_content: u64,
}
/// A channel with nothing to show this long gives its slots' textures back.
const IDLE_RELEASE_MS: u64 = 20_000;
/// ReleaseIdle: a channel empty for IDLE_RELEASE_MS gives back its slots' canvases, mip sources and blur targets -- three
/// slots of ~90 MB each at the eye's size, held for the whole session once the channel had shown (the interaction
/// window's, the scanner's, the hit marker's: ~0.8 GB of VRAM mostly idle; SteamVR leaves the game the least). They are
/// made again off the Present thread when it shows again (a frame or two later). Slots still on the GPU or still read
/// by XR are left for a later call.
fn release_idle(r: &mut Render, channel: u32) {
    if r.last_content == 0 || now().wrapping_sub(r.last_content) < IDLE_RELEASE_MS {
        return;
    }
    // SAFETY: the channel's own fence.
    let completed = r.fence.as_ref().map_or(u64::MAX, |f| unsafe { f.GetCompletedValue() });
    let mut released = 0u32;
    let mut held = 0u32;
    for slot in r.slots.iter_mut() {
        let Some(frame) = slot.frame.as_ref() else {
            slot.pending = None;
            continue;
        };
        if frame.canvas.is_none() {
            continue;
        }
        if slot.fence_value > completed || Arc::strong_count(frame) != 1 {
            held += 1;
            continue;
        }
        slot.frame = None;
        slot.raw = None;
        slot.blur = None;
        slot.blur_temp = None;
        slot.pending = None;
        released += 1;
    }
    if released > 0 {
        cvr_host::log!(
            "[hud-panel] channel {channel} empty {} s: {released} slot(s)' textures released ({held} still in use)\n",
            IDLE_RELEASE_MS / 1000
        );
    }
    if held == 0 {
        // all given back: nothing to look at until the channel shows again
        r.last_content = 0;
    }
}
// SAFETY: D3D12 objects are free-threaded; the render state is used under its lock.
unsafe impl Send for Render {}
struct Pipeline {
    snapshot: Mutex<Option<Arc<Snapshot>>>,
    render: Mutex<Render>,
    consumer_stamp: AtomicU64,
    /// NO FLAT COPY FIRST: a consumer that has shown its channel stays primed while it keeps answering (within a
    /// second) and has not failed outright, so the native slots stay masked between two images (2026-11-14).
    primed: core::sync::atomic::AtomicBool,
    consumer_seen: AtomicU64,
}
static PIPELINES: std::sync::LazyLock<[Pipeline; CHANNELS]> = std::sync::LazyLock::new(|| {
    core::array::from_fn(|_| Pipeline {
        snapshot: Mutex::new(None),
        render: Mutex::new(Render::default()),
        consumer_stamp: AtomicU64::new(0),
        primed: core::sync::atomic::AtomicBool::new(false),
        consumer_seen: AtomicU64::new(0),
    })
});
fn pipeline(channel: u32) -> Option<&'static Pipeline> {
    PIPELINES.get(channel as usize)
}

fn transition(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
    mip: u32,
) {
    let b = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: a borrowed interface pointer, not released by the barrier.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: mip,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    };
    // SAFETY: a recording list.
    unsafe { list.ResourceBarrier(&[b]) };
}

/// ColorBlit::HudParams' defaults (the engine's values read out of its b6 buffer).
fn hud_defaults() -> [f32; HUD_PARAMS] {
    [
        0.009017, 0.084242, // curvature
        0.0008, 0.0008, // shadowUV
        -0.0050, -0.0025, // mipOffset1
        -0.0108, -0.0068, // mipOffset2
        -0.0126, -0.0073, // mipOffset4
        0.08, 0.07, 0.06,   // glowWeight1, 2, 4
        0.0001, // aberration
        2.0,    // hudGain
        1.0,    // glowGain
        1.0,    // shadowStrength
        0.65,   // bloomGain
        1.8,    // bloomLod
        0.036,  // shadowMip
        0.0,    // time
        1.0,    // flicker
        0.0, 0.0, // targetW, targetH
        0.0, // debugMode
        0.0, // hudShiftU
        0.0, // layerOnly
        1.0, 1.0, 1.0, // panelBrightness, panelShadow, panelGlow
        0.0, // panelEngineCurve
        0.0, 1.0, // windshieldMap, windshieldSplit
    ]
}
const LAYER_ONLY: usize = 26;
const PANEL_BRIGHTNESS: usize = 27;
const PANEL_SHADOW: usize = 28;
const PANEL_GLOW: usize = 29;
const PANEL_ENGINE_CURVE: usize = 30;
const WINDSHIELD_MAP: usize = 31;
const WINDSHIELD_SPLIT: usize = 32;

/// THE PANEL IS FLAT. The engine bends its HUD as if on a curved visor (a barrel warp, strongest in the corners); on a
/// flat panel in a headset that tilts every corner element 5 to 9 degrees alike in both eyes: a crooked HUD
/// (2026-11-14). 1 bends the panel with the engine's values again.
#[no_mangle]
pub static mut CyberpunkVR_HudPanelCurve: i32 = 0;

unsafe fn committed(
    device: &ID3D12Device,
    heap: D3D12_HEAP_TYPE,
    desc: &D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
    clear: Option<&D3D12_CLEAR_VALUE>,
) -> Option<ID3D12Resource> {
    let properties = D3D12_HEAP_PROPERTIES {
        Type: heap,
        ..Default::default()
    };
    let mut out: Option<ID3D12Resource> = None;
    device
        .CreateCommittedResource(
            &properties,
            D3D12_HEAP_FLAG_NONE,
            desc,
            state,
            clear.map(|c| c as *const _),
            &mut out,
        )
        .ok()?;
    out
}

/// The slot's canvas and work targets for `width` x `height` (kept when the size is the same).
unsafe fn initialize(slot: &mut Slot, device: &ID3D12Device, width: u32, height: u32) -> Init {
    if let Some(canvas) = slot.frame.as_ref().and_then(|f| f.canvas.as_ref()) {
        let d = canvas.GetDesc();
        if d.Width == width as u64 && d.Height == height {
            return Init::Ready;
        }
    }
    slot.frame = None;
    slot.raw = None;
    slot.blur = None;
    slot.blur_temp = None;
    if slot.defaults.is_none() {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 1024,
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
        let Some(defaults) = committed(
            device,
            D3D12_HEAP_TYPE_UPLOAD,
            &desc,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
        ) else {
            return Init::Failed;
        };
        let mut mapped: *mut c_void = core::ptr::null_mut();
        if defaults.Map(0, None, Some(&mut mapped)).is_err() {
            // The buffer is kept (as the C++ kept it) and mapped again on the next try.
            slot.defaults = Some(defaults);
            return Init::Failed;
        }
        core::ptr::write_bytes(mapped as *mut u8, 0, 1024);
        *(mapped as *mut f32).add(6) = 1.0;
        slot.default_data = mapped as usize;
        slot.defaults = Some(defaults);
    } else if slot.default_data == 0 {
        let mut mapped: *mut c_void = core::ptr::null_mut();
        if slot
            .defaults
            .as_ref()
            .is_some_and(|d| d.Map(0, None, Some(&mut mapped)).is_err())
        {
            return Init::Failed;
        }
        core::ptr::write_bytes(mapped as *mut u8, 0, 1024);
        *(mapped as *mut f32).add(6) = 1.0;
        slot.default_data = mapped as usize;
    }
    if slot.allocator.is_none() {
        match device
            .CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
        {
            Ok(a) => slot.allocator = Some(a),
            Err(_) => return Init::Failed,
        }
    }
    if slot.commands.is_none() {
        let Some(allocator) = slot.allocator.as_ref() else {
            return Init::Failed;
        };
        match device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            allocator,
            None,
        ) {
            Ok(l) => {
                let _ = l.Close();
                slot.commands = Some(l);
            }
            Err(_) => return Init::Failed,
        }
    }
    if slot.rtv.is_none() {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: 1,
            ..Default::default()
        };
        match device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) {
            Ok(h) => slot.rtv = Some(h),
            Err(_) => return Init::Failed,
        }
    }
    // THE TEXTURES ARE MADE OFF THE PRESENT THREAD: a slot's four (~90 MB at the eye's size) took 5-10 ms there at a
    // channel's first show, 175 ms when VRAM was tight (SteamVR, 2026-11-26), three slots a channel -- the interaction
    // window's, the scanner's and the hit marker's first appearances froze the game. A worker makes them; until they are
    // there the slot is skipped (the channel shows a frame or two later).
    let textures = match slot.pending.as_ref().filter(|p| p.width == width && p.height == height) {
        Some(p) => match p.result.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(made) => made,
            None => return Init::Pending,
        },
        None => {
            let cell: Arc<Mutex<Option<Option<Textures>>>> = Arc::new(Mutex::new(None));
            let job = PendingTextures { width, height, result: cell.clone() };
            slot.pending = Some(job);
            let owned = SendDevice(device.clone());
            let spawned = std::thread::Builder::new().name("cvr-hud-textures".into()).spawn(move || {
                let owned = owned;
                let made = make_textures(&owned.0, width, height);
                *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(made);
            });
            if spawned.is_err() {
                // no thread: made here, as before
                slot.pending = None;
                make_textures(device, width, height)
            } else {
                return Init::Pending;
            }
        }
    };
    slot.pending = None;
    let Some(Textures { canvas, raw, blur, blur_temp, .. }) = textures else {
        return Init::Failed;
    };
    slot.raw = Some(raw.clone());
    slot.blur = Some(blur);
    slot.blur_temp = Some(blur_temp);
    let clear = D3D12_CLEAR_VALUE {
        Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        Anonymous: D3D12_CLEAR_VALUE_0 { Color: [0.0; 4] },
    };
    let view = D3D12_RENDER_TARGET_VIEW_DESC {
        Format: clear.Format,
        ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
        Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_RTV {
                MipSlice: 0,
                PlaneSlice: 0,
            },
        },
    };
    let Some(rtv) = slot.rtv.as_ref() else {
        return Init::Failed;
    };
    device.CreateRenderTargetView(&raw, Some(&view), rtv.GetCPUDescriptorHandleForHeapStart());
    if !slot.blit.ensure(device, clear.Format, width, height) {
        return Init::Failed;
    }
    slot.frame = Some(Arc::new(Frame {
        canvas: Some(canvas),
        ..Default::default()
    }));
    Init::Ready
}
/// The device handed to the texture worker (free-threaded).
struct SendDevice(ID3D12Device);
// SAFETY: ID3D12Device is free-threaded.
unsafe impl Send for SendDevice {}
/// A slot's canvas, mip source and blur targets for one size.
struct Textures {
    canvas: ID3D12Resource,
    raw: ID3D12Resource,
    blur: ID3D12Resource,
    blur_temp: ID3D12Resource,
}
// SAFETY: D3D12 resources are free-threaded.
unsafe impl Send for Textures {}
/// A worker making a slot's textures for `width` x `height`: its answer (None while it runs; then the textures, or None
/// when one could not be made).
struct PendingTextures {
    width: u32,
    height: u32,
    result: Arc<Mutex<Option<Option<Textures>>>>,
}
/// The slot's state after initialize: ready to record, its textures still being made, or unusable.
#[derive(PartialEq, Eq)]
enum Init {
    Ready,
    Pending,
    Failed,
}
/// The four textures of a slot for `width` x `height` (any thread).
fn make_textures(device: &ID3D12Device, width: u32, height: u32) -> Option<Textures> {
    let span = cvr_host::hitch::start();
    let mut desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: width as u64,
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R8G8B8A8_TYPELESS,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        ..Default::default()
    };
    let clear = D3D12_CLEAR_VALUE {
        Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        Anonymous: D3D12_CLEAR_VALUE_0 { Color: [0.0; 4] },
    };
    // SAFETY: the live device and our descriptions.
    unsafe {
        let canvas = committed(device, D3D12_HEAP_TYPE_DEFAULT, &desc, D3D12_RESOURCE_STATE_COPY_SOURCE, Some(&clear))?;
        let _ = canvas.SetName(w!("CyberpunkVR separate HUD panel"));
        desc.MipLevels = 5;
        let raw = committed(device, D3D12_HEAP_TYPE_DEFAULT, &desc, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, Some(&clear))?;
        desc.Width = (width as u64 / 2).max(1);
        desc.Height = (height / 2).max(1);
        desc.MipLevels = 4;
        let blur = committed(device, D3D12_HEAP_TYPE_DEFAULT, &desc, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, Some(&clear))?;
        desc.MipLevels = 1;
        let blur_temp = committed(device, D3D12_HEAP_TYPE_DEFAULT, &desc, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, Some(&clear))?;
        cvr_host::hitch::end(span, 0.0, || format!("HUD panel slot textures made off the Present thread for {width}x{height}"));
        Some(Textures { canvas, raw, blur, blur_temp })
    }
}

/// Publish: the channel's newest snapshot (the HUD natives', once per update); its leases are Rust's from here on.
///
/// # Safety
/// `view` is null or a snapshot whose `count` sprites are readable; each lease is handed over once.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_panel_publish(view: *const SnapshotView, channel: u32) {
    let Some(p) = pipeline(channel) else { return };
    let value = view.as_ref().map(|v| {
        Arc::new(Snapshot {
            width: v.width,
            height: v.height,
            stamp: v.stamp,
            generation: v.generation,
            masked: v.masked,
            loot_visible: v.loot_visible,
            empty_content: v.empty_content,
            sprites: if v.sprites.is_null() {
                Vec::new()
            } else {
                core::slice::from_raw_parts(v.sprites, v.count).to_vec()
            },
            windshield_map: v.windshield_map,
            windshield_split: v.windshield_split,
            hud_rect: v.hud_rect,
        })
    });
    let previous = {
        let mut s = p.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        core::mem::replace(&mut *s, value)
    };
    drop(previous);
}
/// ConsumerReady: the channel's XR consumer acknowledged it within 300 ms.
#[no_mangle]
pub extern "C" fn cvr_hud_panel_consumer_ready(channel: u32) -> bool {
    pipeline(channel).is_some_and(|p| {
        let stamp = p.consumer_stamp.load(Acquire);
        stamp != 0 && now().wrapping_sub(stamp) < 300
    })
}
/// SetConsumerReady: every answer says the consumer is alive; a yes primes it.
#[no_mangle]
pub extern "C" fn cvr_hud_panel_set_consumer_ready(ready: bool, channel: u32) {
    if let Some(p) = pipeline(channel) {
        let t = now();
        p.consumer_stamp.store(if ready { t } else { 0 }, Release);
        p.consumer_seen.store(t, Release);
        if ready {
            p.primed.store(true, Release);
        }
    }
}
/// ConsumerPrimed: it has shown the channel and is still answering.
#[no_mangle]
pub extern "C" fn cvr_hud_panel_consumer_primed(channel: u32) -> bool {
    pipeline(channel).is_some_and(|p| {
        let seen = p.consumer_seen.load(Acquire);
        p.primed.load(Acquire) && seen != 0 && now().wrapping_sub(seen) < 1000
    })
}
/// DropConsumer: a consumer that cannot show the channel at all (switched off, no swapchain, another device): the game's
/// own slots come back.
#[no_mangle]
pub extern "C" fn cvr_hud_panel_drop_consumer(channel: u32) {
    if let Some(p) = pipeline(channel) {
        p.primed.store(false, Release);
        p.consumer_stamp.store(0, Release);
    }
}
/// MaskNative: the native slots are hidden while the consumer is ready or primed.
pub fn mask_native(channel: u32) -> bool {
    cvr_hud_panel_consumer_ready(channel) || cvr_hud_panel_consumer_primed(channel)
}

/// Capture: the channel's newest snapshot drawn into a free slot on `queue` and published as its latest frame.
/// `style` is the panel's brightness, shadow and glow (xr_hud_brightness, xr_hud_shadow, xr_hud_glow).
///
/// # Safety
/// `device` and `queue` are live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_panel_capture(
    device: *mut c_void,
    queue: *mut c_void,
    channel: u32,
    style: *const [f32; 3],
) {
    let Some(p) = pipeline(channel) else { return };
    let (Some(device), Some(queue)) = (
        ID3D12Device::from_raw_borrowed(&device),
        ID3D12CommandQueue::from_raw_borrowed(&queue),
    ) else {
        return;
    };
    // The slots' blit pipelines, shared by every slot of every channel, made at the first call (the boot's frames): built
    // at a channel's first show they froze that frame for ~50 ms.
    static WARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !WARMED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        crate::color_blit::prewarm(device, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB);
    }
    let source = p.snapshot.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut guard = p.render.lock().unwrap_or_else(|e| e.into_inner());
    let r = &mut *guard;
    // A channel can disappear for the rest of the session (the vehicle map, a turret, surveillance): its last output may
    // still be leased by XR, but the native inputs are needed only until the producing copy finishes -- retired here,
    // before the empty and stale returns, instead of waiting for the slot's reuse.
    if let Some(fence) = r.fence.as_ref() {
        let completed = fence.GetCompletedValue();
        for slot in r.slots.iter_mut() {
            if slot.fence_value != 0 && slot.fence_value <= completed {
                slot.sources = None;
                slot.frame_constants = None;
                slot.hud_constants = None;
                slot.exposure = None;
            }
        }
    }
    if let Some(s) = source
        .as_ref()
        .filter(|s| s.sprites.is_empty() && s.empty_content && now().wrapping_sub(s.stamp) <= 300)
    {
        // Nothing visible in the channel: publish an empty frame instead of building a canvas of zeros, so the
        // consumer stays ready and the native slots stay masked.
        r.latest = Some(Arc::new(Frame {
            empty: true,
            stamp: s.stamp,
            generation: s.generation,
            masked: s.masked,
            loot_visible: s.loot_visible,
            ..Default::default()
        }));
        release_idle(r, channel);
        return;
    }
    let Some(source) =
        source.filter(|s| !s.sprites.is_empty() && now().wrapping_sub(s.stamp) <= 300)
    else {
        r.latest = None;
        release_idle(r, channel);
        return;
    };
    r.last_content = now();
    if r.fence.is_none() {
        match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
            Ok(f) => r.fence = Some(f),
            Err(_) => return,
        }
        r.device_owner = Some(device.clone());
    }
    if r.device_owner.as_ref().map(|d| d.as_raw()) != Some(device.as_raw()) {
        cvr_hud_panel_drop_consumer(channel);
        return;
    }
    let Some(fence) = r.fence.clone() else { return };
    let completed = fence.GetCompletedValue();
    let style = style.as_ref().copied().unwrap_or([1.0; 3]);
    let Render {
        latest,
        slots,
        submitted,
        ..
    } = r;
    for slot in slots.iter_mut() {
        // The XR reader holds Frame until its copy is queued on this same queue. Thus a free slot cannot be
        // overwritten before that read. Its own producer fence protects both the command allocator/descriptors and
        // all native source texture leases.
        if slot.fence_value > completed
            || slot
                .frame
                .as_ref()
                .is_some_and(|f| Arc::strong_count(f) != 1)
        {
            continue;
        }
        slot.sources = None;
        slot.frame_constants = None;
        slot.hud_constants = None;
        slot.exposure = None;
        // CVR_HITCH_TRACE: every remake of a slot's canvas, blur and mip chain (a new canvas size) logged with its cost
        let remade = cvr_host::hitch::on()
            && !slot.frame.as_ref().and_then(|f| f.canvas.as_ref()).is_some_and(|c| {
                let d = c.GetDesc();
                d.Width == source.width as u64 && d.Height == source.height
            });
        let span = if remade { cvr_host::hitch::start() } else { 0.0 };
        match initialize(slot, device, source.width, source.height) {
            Init::Ready => {}
            // its textures are still being made: another slot may be ready
            Init::Pending => continue,
            Init::Failed => {
                cvr_hud_panel_drop_consumer(channel);
                return;
            }
        }
        cvr_host::hitch::end(span, 0.0, || format!("HUD panel channel {channel}: a slot's textures remade for {}x{}", source.width, source.height));
        let (Some(allocator), Some(cmd)) = (slot.allocator.clone(), slot.commands.clone()) else {
            return;
        };
        if allocator.Reset().is_err() || cmd.Reset(&allocator, None).is_err() {
            return;
        }
        let (Some(raw), Some(blur), Some(blur_temp), Some(rtv)) = (
            slot.raw.clone(),
            slot.blur.clone(),
            slot.blur_temp.clone(),
            slot.rtv.clone(),
        ) else {
            return;
        };
        let Some(canvas) = slot.frame.as_ref().and_then(|f| f.canvas.clone()) else {
            return;
        };
        let profile = stage_profile::begin(&cmd);
        let scope = |stage: u32| {
            profile
                .as_ref()
                .and_then(|b| stage_profile::scope_begin(b, stage))
        };
        let end = |index: Option<usize>| {
            if let (Some(b), Some(i)) = (profile.as_ref(), index) {
                stage_profile::scope_end(b, i);
            }
        };
        let psr = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        let rt = D3D12_RESOURCE_STATE_RENDER_TARGET;
        let mut ok = true;
        {
            let sprites = scope(STAGE_HUD_SPRITES);
            transition(&cmd, &raw, psr, rt, 0);
            cmd.ClearRenderTargetView(rtv.GetCPUDescriptorHandleForHeapStart(), &[0.0; 4], None);
            for sprite in &source.sprites {
                // Native separate-window textures rest in PIXEL|NON_PIXEL_SHADER_RESOURCE. Sampling needs no
                // transition and leaves the game's resource-state tracker intact.
                let resource = ID3D12Resource::from_raw_borrowed(&sprite.resource);
                ok &= resource.is_some_and(|src| {
                    slot.blit.record_overlay(
                        &cmd,
                        src,
                        &raw,
                        0,
                        true,
                        [sprite.x, sprite.y],
                        [sprite.width, sprite.height],
                        Some(sprite.tint),
                        0,
                        0,
                        0,
                        0.0,
                    )
                });
            }
            transition(&cmd, &raw, rt, psr, 0);
            end(sprites);
        }
        {
            let blurring = scope(STAGE_HUD_BLUR);
            let mut pass = |src: &ID3D12Resource,
                            src_mip: u32,
                            dst: &ID3D12Resource,
                            dst_mip: u32,
                            axis: i32| {
                let d = dst.GetDesc();
                transition(&cmd, dst, psr, rt, dst_mip);
                ok &= slot.blit.record_overlay(
                    &cmd,
                    src,
                    dst,
                    4,
                    true,
                    [0.0, 0.0],
                    [
                        (d.Width >> dst_mip).max(1) as f32,
                        (d.Height >> dst_mip).max(1) as f32,
                    ],
                    None,
                    src_mip,
                    dst_mip,
                    axis,
                    0.0,
                );
                transition(&cmd, dst, rt, psr, dst_mip);
            };
            for mip in 1..5 {
                pass(&raw, mip - 1, &raw, mip, 0);
            }
            pass(&raw, 1, &blur_temp, 0, 1);
            pass(&blur_temp, 0, &blur, 0, 2);
            for mip in 1..4 {
                pass(&blur, mip - 1, &blur, mip, 0);
            }
            end(blurring);
        }
        {
            let styling = scope(STAGE_HUD_STYLE);
            transition(
                &cmd,
                &canvas,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                rt,
                D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            );
            let held = |f: Option<unsafe extern "C" fn() -> *mut c_void>| {
                f.and_then(|f| {
                    let p = f();
                    ID3D12Resource::from_raw_borrowed(&p).cloned()
                })
            };
            let rt_ = runtime();
            slot.frame_constants = held(rt_.map(|r| r.frame_constants));
            slot.hud_constants = held(rt_.map(|r| r.hud_constants));
            slot.exposure = held(rt_.map(|r| r.exposure));
            // Defaults remain immutable while this slot is in flight. A missing optional native buffer cannot prevent
            // the HUD from appearing after a cold start.
            if slot.default_data != 0 {
                *(slot.default_data as *mut f32) = (now() % 100000) as f32 * 0.001;
            }
            let mut params = hud_defaults();
            params[LAYER_ONLY] = 1.0;
            let curvature = rt_
                .and_then(|r| r.curvature.as_ref())
                .map(|c| core::ptr::read_volatile(c))
                .unwrap_or([0.0; 2]);
            if core::ptr::read_volatile(&raw const CyberpunkVR_HudPanelCurve) != 0 && curvature[1] != 0.0 {
                params[0] = curvature[0];
                params[1] = curvature[1];
                params[PANEL_ENGINE_CURVE] = 1.0;
            }
            params[WINDSHIELD_MAP] = if source.windshield_map { 1.0 } else { 0.0 };
            params[WINDSHIELD_SPLIT] = source.windshield_split;
            params[PANEL_BRIGHTNESS] = style[0];
            params[PANEL_SHADOW] = style[1];
            params[PANEL_GLOW] = style[2];
            let defaults = slot.defaults.clone();
            let pick = |r: &Option<ID3D12Resource>| r.clone().or_else(|| defaults.clone());
            match (
                pick(&slot.exposure),
                pick(&slot.frame_constants),
                pick(&slot.hud_constants),
            ) {
                (Some(e), Some(f), Some(h)) => {
                    ok &= slot.blit.record_hud_composite(
                        &cmd, &raw, &raw, &blur, &e, &f, &h, &canvas, &params,
                    );
                }
                _ => ok = false,
            }
            transition(
                &cmd,
                &canvas,
                rt,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            );
            end(styling);
        }
        if cmd.Close().is_err() {
            return;
        }
        queue.ExecuteCommandLists(&[Some(
            cmd.cast::<ID3D12CommandList>()
                .unwrap_or_else(|_| unreachable!()),
        )]);
        *submitted += 1;
        slot.fence_value = *submitted;
        if queue.Signal(&fence, *submitted).is_ok() {
            if let Some(b) = profile.as_ref() {
                stage_profile::submitted(b, queue, &fence, *submitted);
            }
        }
        slot.sources = Some(source.clone());
        if let Some(frame) = slot.frame.as_mut().and_then(Arc::get_mut) {
            frame.stamp = source.stamp;
            frame.generation = source.generation;
            frame.serial = *submitted;
            frame.masked = source.masked;
            frame.loot_visible = source.loot_visible;
            frame.fence = Some(fence.clone());
            frame.hud_rect = source.hud_rect;
            frame.windshield_map = source.windshield_map;
            frame.windshield_split = source.windshield_split;
        }
        if ok {
            *latest = slot.frame.clone();
        } else {
            cvr_hud_panel_set_consumer_ready(false, channel);
        }
        return;
    }
}

/// Latest: the channel's newest frame if under 300 ms old; `completed_only` the newest one its queue has finished (a
/// reader on another queue: inline it runs right after the capture, when the newest is usually still on the GPU, and
/// waiting for it kept the copy refused -- the HUD blinked on every runtime with a queue of its own, main's 6f33af62c).
pub fn latest(channel: u32, completed_only: bool) -> Option<Arc<Frame>> {
    let p = pipeline(channel)?;
    let r = p.render.lock().unwrap_or_else(|e| e.into_inner());
    let t = now();
    let latest = r.latest.clone().filter(|f| t.wrapping_sub(f.stamp) < 300)?;
    if !completed_only || latest.empty {
        return Some(latest);
    }
    // SAFETY: a live fence.
    let completed = r.fence.as_ref().map_or(0, |f| unsafe { f.GetCompletedValue() });
    if completed == u64::MAX {
        return None;
    }
    if latest.serial <= completed {
        return Some(latest);
    }
    // A completed fenced slot instead: its shared ownership keeps the capture from reusing it.
    r.slots
        .iter()
        .filter_map(|s| s.frame.as_ref())
        .filter(|f| f.serial != 0 && f.serial <= completed && t.wrapping_sub(f.stamp) < 300)
        .max_by_key(|f| f.serial)
        .cloned()
}
/// The channel's newest captured frame, as the XR quad holds it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FrameView {
    /// A hold on the frame, its own per latest_view call (released with frame_release), or null for none.
    pub handle: *mut c_void,
    pub canvas: *mut c_void,
    pub width: u32,
    pub height: u32,
    pub stamp: u64,
    pub serial: u64,
    pub masked: bool,
    pub loot_visible: bool,
    pub empty: bool,
    /// The producing queue's fence (ID3D12Fence, borrowed for the hold), null for none.
    pub fence: *mut c_void,
    pub windshield_map: bool,
    pub windshield_split: f32,
    pub hud_rect: [f32; 4],
}
/// The channel's newest frame into `out` (the newest finished one with `completed_only`), held until frame_release;
/// false (and `out` empty) without one.
pub fn latest_view(channel: u32, completed_only: bool, out: &mut FrameView) -> bool {
    *out = FrameView {
        handle: core::ptr::null_mut(),
        canvas: core::ptr::null_mut(),
        width: 0,
        height: 0,
        stamp: 0,
        serial: 0,
        masked: false,
        loot_visible: false,
        empty: false,
        fence: core::ptr::null_mut(),
        windshield_map: false,
        windshield_split: 1.0,
        hud_rect: [0.0; 4],
    };
    let Some(frame) = latest(channel, completed_only) else {
        return false;
    };
    if let Some(canvas) = frame.canvas.as_ref() {
        out.canvas = canvas.as_raw();
        // SAFETY: a live resource.
        let d = unsafe { canvas.GetDesc() };
        out.width = d.Width as u32;
        out.height = d.Height;
    }
    out.stamp = frame.stamp;
    out.serial = frame.serial;
    out.masked = frame.masked;
    out.loot_visible = frame.loot_visible;
    out.empty = frame.empty;
    out.fence = frame.fence.as_ref().map_or(core::ptr::null_mut(), |f| f.as_raw());
    out.windshield_map = frame.windshield_map;
    out.windshield_split = frame.windshield_split;
    out.hud_rect = frame.hud_rect;
    // A handle of its own per call, as the C++ adapter's new std::shared_ptr was: the quad tells a frame its copy slot
    // still holds from one it was just given by the handle, so two holds of one frame must not compare equal.
    out.handle = Box::into_raw(Box::new(frame)) as *mut c_void;
    true
}
/// The release of a frame latest_view held.
///
/// # Safety
/// `handle` came from latest_view and is released once, or is null.
pub unsafe fn frame_release(handle: *mut c_void) {
    if !handle.is_null() {
        drop(Box::from_raw(handle as *mut Arc<Frame>));
    }
}
/// Shutdown: the channel's slots released once the GPU passed them (kept otherwise: the manager drains first).
#[no_mangle]
pub extern "C" fn cvr_hud_panel_shutdown(channel: u32) {
    let Some(p) = pipeline(channel) else { return };
    cvr_hud_panel_drop_consumer(channel);
    {
        let mut guard = p.render.lock().unwrap_or_else(|e| e.into_inner());
        let r = &mut *guard;
        // Shutdown follows the manager's GPU drain. If the device still has pending work, retain these bounded
        // resources rather than release native heaps underneath a command list.
        // SAFETY: a live fence.
        if r.fence
            .as_ref()
            .is_some_and(|f| unsafe { f.GetCompletedValue() } < r.submitted)
        {
            return;
        }
        r.latest = None;
        for slot in r.slots.iter_mut() {
            slot.sources = None;
            slot.frame = None;
            slot.commands = None;
            slot.allocator = None;
            slot.rtv = None;
            slot.blit.shutdown();
            slot.raw = None;
            slot.blur = None;
            slot.blur_temp = None;
            slot.frame_constants = None;
            slot.hud_constants = None;
            slot.exposure = None;
            if let Some(d) = slot.defaults.as_ref() {
                if slot.default_data != 0 {
                    // SAFETY: mapped once by initialize.
                    unsafe { d.Unmap(0, None) };
                }
            }
            slot.default_data = 0;
            slot.defaults = None;
            slot.fence_value = 0;
        }
        r.fence = None;
        r.device_owner = None;
        r.submitted = 0;
    }
    let previous = p.snapshot.lock().unwrap_or_else(|e| e.into_inner()).take();
    drop(previous);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_defaults_have_the_headers_layout() {
        let d = hud_defaults();
        assert_eq!(d.len(), 33);
        assert_eq!((d[31], d[32]), (0.0, 1.0));
        assert_eq!((d[0], d[1]), (0.009017, 0.084242));
        assert_eq!((d[17], d[18], d[21]), (0.65, 1.8, 1.0));
        assert_eq!((d[22], d[23]), (0.0, 0.0));
        assert_eq!((d[27], d[28], d[29], d[30]), (1.0, 1.0, 1.0, 0.0));
        assert_eq!(core::mem::size_of::<SpriteView>(), 56);
        assert_eq!(core::mem::size_of::<SnapshotView>(), 72);
    }
}
