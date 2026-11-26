//! The HUD panel's natives and hooks (formerly src/Natives/HudPanel.cpp; the file keeps the exports, the HUD's texture
//! lease -- cvr::hud::Lease, which other C++ readers share -- the hooks' registration, the asserts of the engine layouts
//! read here, and the adapters for the C++ half's state).
//!
//! THE HUD AS ITS OWN PANELS. Separate-window HUD entries (the phone, subtitles, interactions, notifications, the
//! Basilisk HUD, the scanner, the surveillance camera, hit markers) each render into a texture the engine draws into a
//! slot of the HUD layer. Every update the slots' geometry is walked up to the HUD root (their tint and opacity on the
//! way), each texture leased, laid out (latched against half-finished layout jobs, moved onto one eye's root under the
//! engine's stereo, the player's element edits applied) and published per channel to the HUD panel's capture
//! (cvr-gpu hud_panel); when a channel's XR consumer is ready, its native slots are masked -- their final draw hidden,
//! the game's own visibility requests kept (the slot visibility hook) and restored when the mask goes. Entries authored
//! without a separate window are moved to one before they spawn (the entries copy hook). Only these windows are moved;
//! projected HUD widgets stay native.
//!
//! The per-frame upkeep of the HUD roots the Lua module finds once a second (crop bounds, window sizes, the gaze dot's
//! translation, the loot test) runs here inside the update; its repaints are queued and run at the frame submit (the
//! frame submit hook), when the previous frame's UI composition has finished (a repaint during it crashed the pause
//! menu: a composition element's texture freed under it, 2026-10-17).
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::FastMap;
use crate::native_args::{args, borrowed};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_engine::code;
use cvr_gpu::hud_panel::{self, SnapshotView, SpriteView};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::function::BaseFunction;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::{object_class, Class, RttiSystem};
use red4ext_abi::stack::{CStack, StackFrame, StackType};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use windows_sys::Win32::System::Performance::QueryPerformanceCounter;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

#[allow(non_upper_case_globals)]
extern "C" {
    static mut CyberpunkVR_HudRootWidth: f32;
    static mut CyberpunkVR_HudRootHeight: f32;
    static mut CyberpunkVR_HudLayerAddress: [usize; 2];
    static CyberpunkVR_HudSkipEmpty: AtomicU32;
    static CyberpunkVR_HudEmptySprites: AtomicU64;
    static CyberpunkVR_HudPanelTiming: [AtomicU64; 8];
    static CyberpunkVR_HudRootsNative: AtomicU32;
    static CyberpunkVR_HudRootsStats: [AtomicU64; 8];
    static CyberpunkVR_HudRepaintAtSubmit: AtomicU32;
    static CyberpunkVR_HudRepaintStats: [AtomicU64; 3];
    static CyberpunkVR_RuntimeDiagnostics: i32;
    /// src/Core/VrCore.cpp: the launcher's window size.
    #[link_name = "?g_launcherWidth@@3HA"]
    static g_launcherWidth: i32;
    #[link_name = "?g_launcherHeight@@3HA"]
    static g_launcherHeight: i32;
    /// cvr-render's vrcore_exports.rs: the menu mode.
    static g_menuModeValue: core::sync::atomic::AtomicI32;
    /// The device camera, the head-aimed weapon, the sniper nest's head follow (cvr-render, cvr-rig) and the build
    /// check (engine.rs).
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_anim_is_head_aim_weapon_active() -> bool;
    fn cvr_sniper_head_follow_active() -> bool;
    fn cvr_game_build_supported() -> bool;
    /// cvr::gaze::headLook (Camera/GazeCenter.hpp).
    fn cvr_cpp_gaze_head_look(on: bool);
    /// cvr::gaze::CanvasOffset(cvr::gaze::Read(), height, designScale).
    fn cvr_cpp_gaze_canvas_offset(height: f32, design_scale: f32) -> f32;
    /// cvr::hud::Lease(texture, idle): a held std::shared_ptr<TextureLease> and its resource, null when the texture
    /// cannot be leased.
    fn cvr_cpp_hud_lease(
        texture: *mut c_void,
        idle: bool,
        resource: *mut *mut c_void,
    ) -> *mut c_void;
    fn cvr_cpp_hud_lease_release(lease: *mut c_void);
    /// cvr::optics::probe::Request(lease): takes the lease (null allowed).
    fn cvr_cpp_optics_probe_request(lease: *mut c_void) -> bool;
}
#[allow(non_snake_case)]
extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_DisableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

/// What the natives read of the C++ half.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GameState {
    pub hud_panel: i32,
    pub interaction_panel: i32,
    pub menu_rect: i32,
    pub menu_mode: i32,
    pub launcher_width: i32,
    pub launcher_height: i32,
    pub device_cam: bool,
    pub head_aim_weapon: bool,
    pub sniper_follow: bool,
    pub supported: bool,
}
/// The natives' game state (formerly Natives/HudPanel.cpp's cvr_cpp_hud_natives_state): the live HUD, interaction
/// panel and menu rect switches, the menu mode, the launcher's size, and the device camera, head-aimed weapon,
/// sniper follow and build checks.
fn game() -> GameState {
    // SAFETY: the live controls block and the globals live for the process; plain queries.
    unsafe {
        let controls = &*cvr_settings::live_controls_block::block();
        GameState {
            hud_panel: controls.xrHudPanel,
            interaction_panel: controls.xrInteractionPanel,
            menu_rect: controls.xrMenuRect,
            menu_mode: g_menuModeValue.load(Relaxed),
            launcher_width: core::ptr::read_volatile(&raw const g_launcherWidth),
            launcher_height: core::ptr::read_volatile(&raw const g_launcherHeight),
            device_cam: cvr_cpp_device_cam_active(),
            head_aim_weapon: cvr_anim_is_head_aim_weapon_active(),
            sniper_follow: cvr_sniper_head_follow_active(),
            supported: cvr_game_build_supported(),
        }
    }
}
use cvr_settings::hud_layout::{ElementSettings, ElementStatus};
/// cvr::hud::ElementCount.
const ELEMENTS: usize = cvr_settings::hud_layout::COUNT;

// ---- the engine's layouts (asserted against the SDK in src/Natives/HudPanel.cpp)
const WIDGET_TRANSLATION: usize = 0xB8; // renderTransform.translation
const WIDGET_LAYER_PROXY: usize = 0x148;
const WIDGET_PARENT: usize = 0x168;
const WIDGET_NAME: usize = 0x188;
const WIDGET_SIZE: usize = 0x198;
const WIDGET_TINT: usize = 0x1E0;
const WIDGET_OPACITY: usize = 0x1F0;
const WIDGET_VISIBLE: usize = 0x1F8;
const WIDGET_AFFECTS_LAYOUT: usize = 0x1FC;
const WIDGET_FLAGS: usize = 0x1FD;
/// inkCompoundWidget::children: after the widget, which this SDK lays out 0x210 long.
const COMPOUND_CHILDREN: usize = 0x210;
/// inkMultiChildren::children (DynArray<Handle<inkWidget>>), per the SDK's layout.
const MULTI_CHILDREN_LIST: usize = 0x38;
const LAYER_PROXY_LAYER: usize = 0x38;
const LAYER_WINDOW: usize = 0x40;
const HUD_LAYER_ENTRIES: usize = 0x150;
const HUD_LAYER_LOCK: usize = 0x198;
const ENTRY_STRIDE: usize = 0x130;
const ENTRY_NAME: usize = 0x00;
const ENTRY_SEPARATE_WINDOW: usize = 0xA0;
const ENTRY_SLOT_TEXTURE: usize = 0xB8;
const ENTRY_WINDOW: usize = 0xC8;
const ENTRY_SLOT_WIDGET: usize = 0xD8;
const ENTRY_ROOT_WIDGET: usize = 0xF8;
const INFO_SIZE: usize = 0x40;
const INFO_OFFSET: usize = 0x48;
const TEXTURE_WIDTH: usize = 0x40;
const TEXTURE_HEIGHT: usize = 0x44;
const COMPONENT_NAME: usize = 0x40;
/// WorldWidgetComponent's DynamicTexture (+0x230) and its vtable; entRenderToTextureCameraComponent's source (+0x1E8).
const WORLD_WIDGET_TEXTURE: usize = 0x230;
const DYNAMIC_TEXTURE_VTABLE: usize = 0x3064700;
const RTT_CAMERA_SOURCE: usize = 0x1E8;

// ---- the engine's functions
const CHILD_POSITION: usize = 0x15b19e4;
const CHILD_SIZE: usize = 0x15b1990;
const RELEASE: usize = 0x3e3248;
const VISIBILITY: usize = 0x3c9e0c;
const COPY_ENTRIES: usize = 0x482a90;
/// FlagForVisualInvalidation.
const INVALIDATE: usize = 0x2805fc;
const INVALIDATE_LAYOUT: usize = 0x2804e4;
const FRAME_SUBMIT: usize = 0x3d6d84;

const CHANNELS: usize = hud_panel::CHANNELS;
const MAIN: u32 = 0;
const INTERACTION: u32 = 1;
const BASILISK: u32 = 2;
const SURVEILLANCE: u32 = 3;
const HIT_MARKER: u32 = 4;
const VEHICLE_MINIMAP: u32 = 5;

/// Startup A/B diagnostic only (CVR_HUD_SEPARATE_WINDOWS=0|1): existing HUD entries are never rewritten here. 1 (the
/// default) keeps the normal UI path.
#[no_mangle]
pub static CyberpunkVR_HudSeparateWindows: AtomicU32 = AtomicU32::new(1);
/// CopyEntries calls, returned entries, whitelist matches, false->true changes, bypassed whitelist matches.
#[no_mangle]
pub static CyberpunkVR_HudSeparateWindowsCounts: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

const ROOT_SCANNER: u32 = 1;
const ROOT_WINDOW_ORIGIN: u32 = 2;
const ROOT_INTERACTION: u32 = 4;
const ROOT_GAZE_DOT: u32 = 8;

const fn entry_hash(name: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    let mut i = 0;
    while i < name.len() {
        h ^= name[i] as u64;
        h = h.wrapping_mul(1099511628211);
        i += 1;
    }
    h
}
/// cvr::hud::EntryChannel (Runtimes/HudChannels.hpp).
fn entry_channel(name: u64, sniper_nest: bool) -> u32 {
    if name == entry_hash(b"minimap") {
        // In the HUD at its tile's place, or projected while driving (main's 0.2.4).
        VEHICLE_MINIMAP
    } else if name == entry_hash(b"interactions_root") {
        INTERACTION
    } else if name == entry_hash(b"hud tank") {
        BASILISK
    } else if name == entry_hash(b"camera_hud")
        || name == entry_hash(b"scanner")
        || name == entry_hash(b"scanner_details")
    {
        SURVEILLANCE
    } else if sniper_nest && name == entry_hash(b"briefing_sequence_player") {
        SURVEILLANCE
    } else if name == entry_hash(b"hit marker") {
        // Hit and kill markers: their own head-locked window (free-look cone 0), not the main HUD's cone.
        HIT_MARKER
    } else {
        MAIN
    }
}
/// cvr::hud::NeedsSeparateWindow.
fn needs_separate_window(name: u64) -> bool {
    const NAMES: [&[u8]; 14] = [
        b"new_phone",
        b"songbird_holocall",
        b"subtitles",
        b"interactions_root",
        b"Notifications_Journal",
        b"items_notification_queue",
        b"hud tank",
        b"q304_dossier",
        b"q304_imprint_active_indicator",
        b"briefing_sequence_player",
        b"scanner_details",
        b"camera_hud",
        b"scanner",
        b"hit marker",
    ];
    NAMES.iter().any(|n| entry_hash(n) == name)
}

fn base() -> usize {
    code::image_base()
}
unsafe fn at<T: Copy>(p: usize, offset: usize) -> T {
    core::ptr::read_unaligned((p + offset) as *const T)
}
unsafe fn set<T>(p: usize, offset: usize, value: T) {
    core::ptr::write_unaligned((p + offset) as *mut T, value)
}
/// A strong copy of the handle at `p + offset`.
unsafe fn handle_at(p: usize, offset: usize) -> Handle {
    (*borrowed((p + offset) as *const [usize; 2])).clone()
}
unsafe fn lock_weak(p: usize, offset: usize) -> Handle {
    (*((p + offset) as *const WeakHandle)).lock()
}
fn class_of(object: usize) -> Option<&'static Class> {
    // SAFETY: a live scriptable.
    unsafe { object_class(object as *const c_void) }
}
fn is_a(object: usize, name: &str) -> bool {
    object != 0 && class_of(object).is_some_and(|c| c.is_a(CName::hashed(name)))
}
fn cmax(a: f32, b: f32) -> f32 {
    if a < b {
        b
    } else {
        a
    }
}
fn cmin(a: f32, b: f32) -> f32 {
    if b < a {
        b
    } else {
        a
    }
}
/// std::clamp.
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}
fn now_qpc() -> i64 {
    let mut t = 0i64;
    // SAFETY: plain Win32 call.
    unsafe { QueryPerformanceCounter(&mut t) };
    t
}
fn timing(slot: usize, ticks: i64) {
    // SAFETY: an export of the C++ half.
    unsafe { CyberpunkVR_HudPanelTiming[slot].fetch_add(ticks as u64, Relaxed) };
}
fn roots_stat(slot: usize) {
    // SAFETY: as above.
    unsafe { CyberpunkVR_HudRootsStats[slot].fetch_add(1, Relaxed) };
}

fn verified() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        // The natives stay callable on any build.
        if !game().supported {
            return false;
        }
        let layout: [u8; 13] = [
            0x40, 0x53, 0x48, 0x83, 0xec, 0x70, 0x48, 0x8b, 0x89, 0xd0, 0x01, 0, 0,
        ];
        let release: [u8; 22] = [
            0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x15, 0x4f,
            0xaa, 0x7e, 0x03, 0x48, 0x8b, 0xf9, 0x80, 0x7a,
        ];
        bytes_are(base() + CHILD_POSITION, &layout)
            && bytes_are(base() + CHILD_SIZE, &layout)
            && bytes_are(base() + RELEASE, &release)
    })
}
fn bytes_are(address: usize, expected: &[u8]) -> bool {
    let mut got = vec![0u8; expected.len()];
    code::read_bytes(address, &mut got) && got == expected
}

// ---- the masks

struct Mask {
    widget: Handle,
    wanted_visible: AtomicBool,
    original_affects_layout: bool,
    channel: AtomicU32,
}
// SAFETY: the widget handle is the engine's refcounted pointer; the mask is shared with the engine's UI threads.
unsafe impl Send for Mask {}
unsafe impl Sync for Mask {}
impl Mask {
    fn new(widget: Handle, channel: u32) -> Mask {
        let w = widget.instance as usize;
        // SAFETY: a live widget.
        let (visible, affects) = unsafe {
            (
                at::<bool>(w, WIDGET_VISIBLE),
                at::<bool>(w, WIDGET_AFFECTS_LAYOUT),
            )
        };
        Mask {
            widget,
            wanted_visible: AtomicBool::new(visible),
            original_affects_layout: affects,
            channel: AtomicU32::new(channel),
        }
    }
}
/// The masks the visibility hook reads: an immutable snapshot whose handles keep the widgets alive while an engine UI
/// job is inside the hook.
static PUBLISHED: RwLock<Option<Arc<Vec<Arc<Mask>>>>> = RwLock::new(None);

static ORIGINAL_VISIBLE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_COPY_ENTRIES: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_FRAME_SUBMIT: AtomicUsize = AtomicUsize::new(0);
static HOOKS_READY: AtomicBool = AtomicBool::new(false);
type VisibleFn = unsafe extern "C" fn(usize, bool) -> usize;
type CopyEntriesFn = unsafe extern "C" fn(usize, usize) -> usize;
type FrameSubmitFn = unsafe extern "C" fn(*mut c_void, *mut c_void, i8);
type InvalidateFn = unsafe extern "C" fn(usize);
type ChildRectFn = unsafe extern "C" fn(usize, *mut [f32; 2], *const Handle) -> *mut [f32; 2];

unsafe fn invalidate(widget: usize) {
    core::mem::transmute::<usize, InvalidateFn>(base() + INVALIDATE)(widget)
}
unsafe fn original_visible(widget: usize, visible: bool) -> usize {
    core::mem::transmute::<usize, VisibleFn>(ORIGINAL_VISIBLE.load(Acquire))(widget, visible)
}
fn child_rect(rva: usize, parent: usize, child: &Handle) -> [f32; 2] {
    let mut out = [0.0f32; 2];
    // SAFETY: the engine's child-rect function on a live parent and its child.
    unsafe { core::mem::transmute::<usize, ChildRectFn>(base() + rva)(parent, &mut out, child) };
    out
}
fn child_position(parent: usize, child: &Handle) -> [f32; 2] {
    child_rect(CHILD_POSITION, parent, child)
}
fn child_size(parent: usize, child: &Handle) -> [f32; 2] {
    child_rect(CHILD_SIZE, parent, child)
}

unsafe fn hide_slot(widget: usize) {
    set(widget, WIDGET_AFFECTS_LAYOUT, true);
    if at::<bool>(widget, WIDGET_VISIBLE) {
        // ImageWidget::OnVisibilityChanged(false), RVA4FC34C, unloads the image and invalidates its layout even with
        // affectsLayoutWhenHidden=true. Mask only the final draw and visually invalidate the ancestors instead. The
        // separate window/texture stays live and its layout keeps its size.
        set(widget, WIDGET_VISIBLE, false);
        invalidate(widget);
    }
}
/// MaskNative: the native slot is hidden while the channel's consumer is ready or primed (no flat copy first).
fn mask_native(channel: u32) -> bool {
    hud_panel::mask_native(channel)
}

/// The game updates slot visibility after CET onUpdate too. Intercept its request, retain the intended visibility, and
/// keep only our final slot image from drawing. Source windows and their controllers are not hidden.
unsafe extern "C" fn visible_detour(widget: usize, visible: bool) -> usize {
    let targets = PUBLISHED.read().ok().and_then(|p| p.clone());
    if let Some(targets) = targets {
        if let Some(mask) = targets
            .iter()
            .find(|m| m.widget.instance as usize == widget)
        {
            mask.wanted_visible.store(visible, Relaxed);
            let channel = mask.channel.load(Acquire);
            let s = game();
            if s.hud_panel != 0
                && s.menu_mode == 0
                && s.menu_rect == 0
                && (channel != INTERACTION || s.interaction_panel != 0)
                && mask_native(channel)
            {
                hide_slot(widget);
                return 0;
            }
        }
    }
    original_visible(widget, visible)
}
/// These HUD entries are authored without separate windows. Move them through the engine's own separate-window
/// creation path BEFORE spawn. Never convert projected widgets (crosshair, mappins, scanner borders, etc.).
unsafe extern "C" fn copy_entries_detour(resource: usize, entries: usize) -> usize {
    static STARTUP: std::sync::Once = std::sync::Once::new();
    STARTUP.call_once(|| {
        let mut value = [0u16; 2];
        // SAFETY: a two-character buffer.
        let size = unsafe {
            windows_sys::Win32::System::Environment::GetEnvironmentVariableW(windows_sys::core::w!("CVR_HUD_SEPARATE_WINDOWS"), value.as_mut_ptr(), 2)
        };
        if size == 1 && (value[0] == u16::from(b'0') || value[0] == u16::from(b'1')) {
            CyberpunkVR_HudSeparateWindows.store(u32::from(value[0] == u16::from(b'1')), Relaxed);
        } else if size != 0 {
            log::write("[hud-panel] invalid CVR_HUD_SEPARATE_WINDOWS; expected 0 or 1, retaining diagnostic mode\n");
        }
        log::write(&format!(
            "[hud-panel] startup separate-window override={}; only new spawn entries are affected\n",
            CyberpunkVR_HudSeparateWindows.load(Relaxed)
        ));
    });
    let counts = &CyberpunkVR_HudSeparateWindowsCounts;
    counts[0].fetch_add(1, Relaxed);
    let result = core::mem::transmute::<usize, CopyEntriesFn>(ORIGINAL_COPY_ENTRIES.load(Acquire))(
        resource, entries,
    );
    if entries != 0 {
        let data: usize = at(entries, 0);
        let size: u32 = at(entries, 0xC);
        counts[1].fetch_add(u64::from(size), Relaxed);
        let separate = CyberpunkVR_HudSeparateWindows.load(Relaxed) != 0;
        for i in 0..size as usize {
            let e = data + i * ENTRY_STRIDE;
            let name: u64 = at(e, ENTRY_NAME);
            if needs_separate_window(name) {
                counts[2].fetch_add(1, Relaxed);
                if !separate {
                    counts[4].fetch_add(1, Relaxed);
                    continue;
                }
                if !at::<bool>(e, ENTRY_SEPARATE_WINDOW) {
                    counts[3].fetch_add(1, Relaxed);
                }
                set(e, ENTRY_SEPARATE_WINDOW, true);
                log::write(&format!(
                    "[hud-panel] separate window enabled for {} before spawn\n",
                    CName(name).text()
                ));
            }
        }
    }
    result
}
unsafe extern "C" fn frame_submit_detour(scene: *mut c_void, views: *mut c_void, flag: i8) {
    flush_repaints_at_submit();
    // the carried things' pictures sent once more with the hand as animated for this frame (main's HudPanel.cpp)
    #[cfg(feature = "rig")]
    cvr_rig::vrik::physhands::heldbody::at_frame_submit();
    core::mem::transmute::<usize, FrameSubmitFn>(ORIGINAL_FRAME_SUBMIT.load(Acquire))(
        scene, views, flag,
    );
}

/// InstallHudPanelHooks: the slot visibility and the entries copy (both required), the frame submit (optional:
/// without it the repaints run in the update as before).
#[no_mangle]
pub extern "C" fn cvr_hud_natives_install_hooks() -> bool {
    let visibility: [u8; 16] = [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x20, 0x40, 0x8a, 0xfa, 0x48, 0x8b,
        0xd9,
    ];
    let entries: [u8; 20] = [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18,
        0x57, 0x48, 0x83, 0xec, 0x20,
    ];
    let invalidate_bytes: [u8; 18] = [
        0x48, 0x89, 0x5c, 0x24, 0x18, 0x55, 0x48, 0x8b, 0xec, 0x48, 0x83, 0xec, 0x60, 0x80, 0x89,
        0xfd, 0x01, 0x00,
    ];
    let invalidate_layout: [u8; 23] = [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x7c, 0x24, 0x10, 0x55, 0x48, 0x8d, 0x6c, 0x24,
        0xa9, 0x48, 0x81, 0xec, 0xa0, 0, 0, 0,
    ];
    let b = base();
    if !verified()
        || !bytes_are(b + VISIBILITY, &visibility)
        || !bytes_are(b + COPY_ENTRIES, &entries)
        || !bytes_are(b + INVALIDATE, &invalidate_bytes)
        || !bytes_are(b + INVALIDATE_LAYOUT, &invalidate_layout)
    {
        return false;
    }
    // SAFETY: MinHook on the engine functions verified above.
    unsafe {
        let vis = (b + VISIBILITY) as *mut c_void;
        let copy = (b + COPY_ENTRIES) as *mut c_void;
        let mut original: *mut c_void = core::ptr::null_mut();
        if MH_CreateHook(vis, visible_detour as *mut c_void, &mut original) != 0 {
            return false;
        }
        ORIGINAL_VISIBLE.store(original as usize, Release);
        let mut original: *mut c_void = core::ptr::null_mut();
        if MH_CreateHook(copy, copy_entries_detour as *mut c_void, &mut original) != 0 {
            MH_RemoveHook(vis);
            return false;
        }
        ORIGINAL_COPY_ENTRIES.store(original as usize, Release);
        if MH_EnableHook(vis) != 0 || MH_EnableHook(copy) != 0 {
            MH_DisableHook(vis);
            MH_DisableHook(copy);
            return false;
        }
        let submit: [u8; 19] = [
            0x48, 0x89, 0x5c, 0x24, 0x10, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56,
            0x41, 0x57, 0x48, 0x8b, 0xec,
        ];
        let frame_submit = (b + FRAME_SUBMIT) as *mut c_void;
        let mut original: *mut c_void = core::ptr::null_mut();
        if bytes_are(b + FRAME_SUBMIT, &submit)
            && MH_CreateHook(
                frame_submit,
                frame_submit_detour as *mut c_void,
                &mut original,
            ) == 0
        {
            ORIGINAL_FRAME_SUBMIT.store(original as usize, Release);
            if MH_EnableHook(frame_submit) != 0 {
                ORIGINAL_FRAME_SUBMIT.store(0, Release);
            }
        }
    }
    HOOKS_READY.store(true, Release);
    true
}

// ---- the game thread's state

struct UpkeepRoot {
    root: Handle,
    window: Handle,
    info: Handle,
    padding: f32,
    flags: u32,
    // Unknown after every scan, as the script's fresh tables were.
    was_active: i8,
}
struct UpkeepSubtitle {
    root: Handle,
    window: Handle,
    panel: Handle,
    info: Handle,
    // The authored crop the root had when first seen.
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    was_active: i8,
}
#[derive(Clone, Copy, Default)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}
#[derive(Clone, Copy, Default)]
struct LayoutLatch {
    last: Rect,
    candidate: Rect,
    valid: bool,
    have_candidate: bool,
}
impl LayoutLatch {
    fn same(a: Rect, b: Rect) -> bool {
        (a.x - b.x).abs() < 0.05
            && (a.y - b.y).abs() < 0.05
            && (a.width - b.width).abs() < 0.05
            && (a.height - b.height).abs() < 0.05
    }
    /// UI layout jobs may be between updating the slot and its parents. Keep a committed rectangle until layout settles
    /// or repeats coherently.
    fn update(&mut self, value: Rect, dirty: bool) -> Rect {
        if !self.valid || !dirty || (self.have_candidate && Self::same(self.candidate, value)) {
            self.last = value;
            self.valid = true;
        }
        self.candidate = value;
        self.have_candidate = true;
        self.last
    }
}
struct State {
    masks: FastMap<usize, Arc<Mask>>,
    generation: u64,
    last_root: usize,
    last_masked: [bool; CHANNELS],
    layout_cache: FastMap<u64, LayoutLatch>,
    layout_width: u32,
    layout_height: u32,
    upkeep_roots: Vec<UpkeepRoot>,
    pending_roots: Vec<UpkeepRoot>,
    upkeep_subtitles: Vec<UpkeepSubtitle>,
    pending_subtitles: Vec<UpkeepSubtitle>,
    pending_scan: bool,
    repaints: Vec<Handle>,
}
// SAFETY: the engine's refcounted handles, used on the game thread under STATE's lock.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static REPAINT_THREAD: AtomicU32 = AtomicU32::new(0);
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let s = guard.get_or_insert_with(|| State {
        masks: cvr_host::fast::fast_map(),
        generation: 0,
        last_root: 0,
        last_masked: [false; CHANNELS],
        layout_cache: cvr_host::fast::fast_map(),
        layout_width: 0,
        layout_height: 0,
        upkeep_roots: Vec::new(),
        pending_roots: Vec::new(),
        upkeep_subtitles: Vec::new(),
        pending_subtitles: Vec::new(),
        pending_scan: false,
        repaints: Vec::new(),
    });
    f(s)
}

fn publish_masks(s: &State) {
    let next: Vec<Arc<Mask>> = s.masks.values().cloned().collect();
    if let Ok(mut p) = PUBLISHED.write() {
        *p = Some(Arc::new(next));
    }
}
/// Calling the original bypasses our interception, so it cannot mistake our own hide/restore for a new game-side
/// visibility request.
unsafe fn restore_mask(mask: &Mask) {
    let widget = mask.widget.instance as usize;
    let wanted = mask.wanted_visible.load(Relaxed);
    let layout_changed = at::<bool>(widget, WIDGET_AFFECTS_LAYOUT) != mask.original_affects_layout;
    set(widget, WIDGET_AFFECTS_LAYOUT, mask.original_affects_layout);
    // Our hidden bit is only a draw mask. Deliver the real visibility transition now, including ImageWidget's
    // resource/layout callback when the game hid it.
    set(widget, WIDGET_VISIBLE, !wanted);
    original_visible(widget, wanted);
    if layout_changed {
        core::mem::transmute::<usize, InvalidateFn>(base() + INVALIDATE_LAYOUT)(widget);
    }
}
unsafe fn restore(s: &mut State) {
    if let Ok(mut p) = PUBLISHED.write() {
        *p = None;
    }
    for mask in s.masks.values() {
        restore_mask(mask);
    }
    s.masks.clear();
}

fn widget_visible(w: usize) -> bool {
    // SAFETY: a live widget.
    unsafe { at::<bool>(w, WIDGET_VISIBLE) }
}
fn widget_opacity(w: usize) -> f32 {
    // SAFETY: as above.
    unsafe { at::<f32>(w, WIDGET_OPACITY) }
}
fn widget_size(w: usize) -> [f32; 2] {
    // SAFETY: as above.
    unsafe { at::<[f32; 2]>(w, WIDGET_SIZE) }
}
fn active(w: usize) -> bool {
    widget_visible(w) && widget_opacity(w) > 0.0
}
fn within(v: f32) -> bool {
    v.abs() < 16384.0
}
/// The children of a compound widget (DynArray<Handle<inkWidget>>), or none.
unsafe fn children(widget: usize) -> &'static [[usize; 2]] {
    if !is_a(widget, "inkCompoundWidget") {
        return &[];
    }
    let list: usize = at(widget, COMPOUND_CHILDREN);
    if list == 0 {
        return &[];
    }
    crate::native_args::handles((list + MULTI_CHILDREN_LIST) as *const c_void)
}

/// Whether anything under a separate window's root can reach its texture: a path of visible, non-transparent widgets
/// ending in something that is not a container (image, text, shape -- counted as content without looking further).
/// Containers draw nothing themselves, so one whose visible children all lead nowhere is empty. An empty window
/// renders a fully transparent texture (the interaction window measured 0 non-zero pixels of 6.5 million), so
/// skipping it is exact.
fn has_visible_content(widget: usize, depth: u32) -> bool {
    if widget == 0 {
        return false;
    }
    if depth > 64 {
        return true;
    }
    // SAFETY: a live widget.
    let alpha = unsafe { at::<[f32; 4]>(widget, WIDGET_TINT) }[3];
    if !widget_visible(widget) || !(widget_opacity(widget) > 0.0) || !(alpha > 0.0) {
        return false;
    }
    if !is_a(widget, "inkCompoundWidget") {
        return true;
    }
    // SAFETY: as above.
    let list: usize = unsafe { at(widget, COMPOUND_CHILDREN) };
    if list == 0 {
        return false;
    }
    // SAFETY: the compound's own children, unchanged during the walk.
    let kids =
        unsafe { crate::native_args::handles((list + MULTI_CHILDREN_LIST) as *const c_void) };
    kids.iter().any(|c| has_visible_content(c[0], depth + 1))
}
/// The direct child with exactly this name: what the script's GetWidgetByPathName plus its name check gave.
fn child_named(widget: usize, name: CName) -> Handle {
    if widget == 0 {
        return Handle::null();
    }
    // SAFETY: a live compound's children.
    for c in unsafe { children(widget) } {
        // SAFETY: a live child.
        if c[0] != 0 && unsafe { at::<u64>(c[0], WIDGET_NAME) } == name.0 {
            // SAFETY: the child's handle in the list.
            return unsafe { handle_at(c.as_ptr() as usize, 0) };
        }
    }
    Handle::null()
}

/// RED4ext::ExecuteFunction(instance, f, out, args): a null argument type taken from the function's parameter, and a
/// function with a result not called without a place for it.
unsafe fn execute(
    instance: usize,
    f: &BaseFunction,
    out: *mut c_void,
    args: &mut [StackType],
) -> bool {
    let Some(rtti) = RttiSystem::get() else {
        return false;
    };
    let mut result = StackType {
        ty: core::ptr::null(),
        value: out,
    };
    let result_ptr = match f.return_type.as_ref() {
        Some(_) if out.is_null() => return false,
        Some(r) => {
            result.ty = r.ty;
            &mut result as *mut StackType
        }
        None => core::ptr::null_mut(),
    };
    for (arg, param) in args.iter_mut().zip(f.params.as_slice()) {
        if arg.ty.is_null() {
            if let Some(param) = param.as_ref() {
                arg.ty = param.ty;
            }
        }
    }
    let Some(mut stack) = CStack::new(rtti, instance as *mut c_void, args, result_ptr) else {
        return false;
    };
    let Some(address) = f.execute_address() else {
        return false;
    };
    code::guarded_call3(
        address,
        f as *const BaseFunction as *mut c_void,
        &mut stack as *mut CStack as *mut c_void,
        core::ptr::null_mut(),
    )
    .unwrap_or(false)
}
fn param_type_name(f: &BaseFunction, i: usize) -> Option<CName> {
    let p = *f.params.as_slice().get(i)?;
    // SAFETY: a registered function's parameters and their types.
    let p = unsafe { p.as_ref() }?;
    Some(unsafe { p.ty.as_ref() }?.name())
}
/// A widget's (x, y) or (Vector2) setter called through RTTI: it relayouts, which a field write would not.
fn call_vector2(widget: usize, function: &str, value: [f32; 2]) -> Option<bool> {
    let f = class_of(widget)?.function(CName::hashed(function))?;
    let mut v = value;
    let (mut x, mut y) = (value[0], value[1]);
    let mut args: Vec<StackType> = Vec::new();
    if f.params.len() == 2 {
        args.push(StackType {
            ty: core::ptr::null(),
            value: &mut x as *mut f32 as *mut c_void,
        });
        args.push(StackType {
            ty: core::ptr::null(),
            value: &mut y as *mut f32 as *mut c_void,
        });
    } else if f.params.len() == 1 && param_type_name(f, 0) == Some(CName::hashed("Vector2")) {
        args.push(StackType {
            ty: core::ptr::null(),
            value: v.as_mut_ptr() as *mut c_void,
        });
    } else {
        return None;
    }
    // SAFETY: the widget's own function.
    Some(unsafe { execute(widget, f, core::ptr::null_mut(), &mut args) })
}
/// The widget's own SetSize, through RTTI.
fn set_widget_size(widget: usize, size: [f32; 2]) -> bool {
    call_vector2(widget, "SetSize", size).unwrap_or(false)
}

/// Root pixels per design unit: the HUD layer fits its 3840x2160 design into the root (0.6896 at 2648x2864, bound by
/// the width; the scanner's 3084-unit crop made a 2126-pixel texture). 0 until the root is seen.
fn hud_design_scale() -> f32 {
    // SAFETY: exports of the C++ half.
    let (width, height) = unsafe {
        (
            core::ptr::read_volatile(&raw const CyberpunkVR_HudRootWidth),
            core::ptr::read_volatile(&raw const CyberpunkVR_HudRootHeight),
        )
    };
    if !(width > 0.0) || !(height > 0.0) {
        return 0.0;
    }
    cmin(width / 3840.0, height / 2160.0)
}
/// Native separate-window textures grow but do not shrink: a smaller requested crop is drawn at their top-left while
/// the HUD slot still uses the allocation's full size. Keep the content's centre in that actual texture.
fn keep_crop_centered(
    mut crop: Rect,
    texture_width: f32,
    texture_height: f32,
    design_scale: f32,
) -> Rect {
    if !design_scale.is_finite()
        || design_scale <= 0.0
        || !texture_width.is_finite()
        || !texture_height.is_finite()
        || texture_width <= 0.0
        || texture_height <= 0.0
        || texture_width > 16384.0
        || texture_height > 16384.0
    {
        return crop;
    }
    let cx = crop.x + crop.width * 0.5;
    let cy = crop.y + crop.height * 0.5;
    // Stay below the next texel when the engine rounds the request (std::nextafter toward zero).
    let below = |v: f32| {
        if v > 0.0 {
            f32::from_bits(v.to_bits() - 1)
        } else {
            v
        }
    };
    crop.width = cmax(crop.width, below(texture_width / design_scale));
    crop.height = cmax(crop.height, below(texture_height / design_scale));
    crop.x = cx - crop.width * 0.5;
    crop.y = cy - crop.height * 0.5;
    crop
}
/// cvr::hud::Sanitize.
fn sanitize(mut s: ElementSettings) -> ElementSettings {
    s.x = if s.x.is_finite() {
        clamp(s.x, -100.0, 100.0)
    } else {
        0.0
    };
    s.y = if s.y.is_finite() {
        clamp(s.y, -100.0, 100.0)
    } else {
        0.0
    };
    s.scale = if s.scale.is_finite() {
        clamp(s.scale, 0.1, 3.0)
    } else {
        1.0
    };
    s.opacity = if s.opacity.is_finite() {
        clamp(s.opacity, 0.0, 1.0)
    } else {
        1.0
    };
    s.visible = (s.visible != 0) as i32;
    s
}
/// cvr::hud::ApplyElement: the player's offset and scale about the element's centre, kept on the canvas joined with the
/// element's own authored rect.
fn apply_element(r: Rect, s: ElementSettings, canvas_w: f32, canvas_h: f32) -> Rect {
    let s = sanitize(s);
    let mut w = r.width * s.scale;
    let mut h = r.height * s.scale;
    let mut x = r.x + (r.width - w) * 0.5 + s.x * canvas_w * 0.01;
    let mut y = r.y + (r.height - h) * 0.5 + s.y * canvas_h * 0.01;
    if canvas_w > 0.0 && canvas_h > 0.0 {
        let min_x = cmin(0.0, r.x);
        let min_y = cmin(0.0, r.y);
        let max_x = cmax(canvas_w, r.x + r.width);
        let max_y = cmax(canvas_h, r.y + r.height);
        let bound_w = max_x - min_x;
        let bound_h = max_y - min_y;
        if w > bound_w || h > bound_h {
            let k = cmin(bound_w / w, bound_h / h);
            let cx = x + w * 0.5;
            let cy = y + h * 0.5;
            w *= k;
            h *= k;
            x = cx - w * 0.5;
            y = cy - h * 0.5;
        }
        x = clamp(x, min_x, max_x - w);
        y = clamp(y, min_y, max_y - h);
    }
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
/// A slot's rectangle fitted to its texture's aspect, centred (unchanged within 2 %, or without a size).
fn fit_aspect(r: Rect, texture_width: f32, texture_height: f32) -> Rect {
    if !(texture_width > 0.0 && texture_height > 0.0 && r.width > 0.0 && r.height > 0.0) {
        return r;
    }
    let want = texture_width / texture_height;
    let have = r.width / r.height;
    if (have / want - 1.0).abs() <= 0.02 {
        return r;
    }
    if have > want {
        let width = r.height * want;
        Rect { x: r.x + (r.width - width) * 0.5, width, ..r }
    } else {
        let height = r.width / want;
        Rect { y: r.y + (r.height - height) * 0.5, height, ..r }
    }
}
/// A slot whose aspect is not its texture's, once per entry (the subtitles' is fitted; the others are reported).
fn note_fit(name: u64, r: Rect, texture_width: f32, texture_height: f32) {
    static SEEN: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen.contains(&name) || seen.len() >= 64 {
        return;
    }
    seen.push(name);
    let index = cvr_settings::hud_layout::element_index(name);
    log!(
        "[hud-panel] the slot of entry {name:#x} (element {index}{}) is {:.0} x {:.0} (aspect {:.3}), its texture {texture_width:.0} x {texture_height:.0} (aspect {:.3})\n",
        if name == entry_hash(b"subtitles") { ", the subtitles: fitted" } else { "" },
        r.width,
        r.height,
        r.width / r.height,
        texture_width / texture_height
    );
}
/// THE ENGINE'S STEREO LAYS THE HUD OUT TWO EYES WIDE: the panel is built on the one-eye root instead. The HUD's
/// windows hang from its 3840 x 2160 design area, which the layer centres in either root, so the move is one scale about
/// the roots' centres.
fn to_eye_root(r: Rect, root_width: f32, root_height: f32, eye_width: f32, ratio: f32) -> Rect {
    Rect {
        x: eye_width * 0.5 + (r.x - root_width * 0.5) * ratio,
        y: root_height * 0.5 + (r.y - root_height * 0.5) * ratio,
        width: r.width * ratio,
        height: r.height * ratio,
    }
}

// ---- repaints at the frame submit

fn repaint(s: &mut State, widget: &Handle) {
    if widget.is_null() {
        return;
    }
    // SAFETY: an export of the C++ half.
    let at_submit = unsafe { CyberpunkVR_HudRepaintAtSubmit.load(Relaxed) } != 0;
    if !at_submit || ORIGINAL_FRAME_SUBMIT.load(Acquire) == 0 {
        // SAFETY: a live widget.
        unsafe { invalidate(widget.instance as usize) };
        return;
    }
    s.repaints.push(widget.clone());
    // SAFETY: as above.
    unsafe { CyberpunkVR_HudRepaintStats[0].fetch_add(1, Relaxed) };
}
fn flush_repaints(s: &mut State, stat: usize) {
    for widget in &s.repaints {
        if !widget.is_null() {
            // SAFETY: a widget the queue holds.
            unsafe { invalidate(widget.instance as usize) };
        }
    }
    if !s.repaints.is_empty() {
        // SAFETY: an export of the C++ half.
        unsafe { CyberpunkVR_HudRepaintStats[stat].fetch_add(s.repaints.len() as u64, Relaxed) };
    }
    s.repaints.clear();
}
fn flush_repaints_at_submit() {
    let thread = REPAINT_THREAD.load(Relaxed);
    // SAFETY: plain Win32 call.
    if thread == 0 || unsafe { GetCurrentThreadId() } != thread {
        return;
    }
    // The update holds the lock only on this same thread, never across a frame submit.
    if let Ok(mut guard) = STATE.try_lock() {
        if let Some(s) = guard.as_mut() {
            flush_repaints(s, 1);
        }
    }
}

fn commit_scan(s: &mut State) {
    if !s.pending_scan {
        return;
    }
    s.pending_scan = false;
    // A subtitle root seen in the previous scan keeps its first authored crop, as the script's did.
    for next in s.pending_subtitles.iter_mut() {
        if let Some(old) = s
            .upkeep_subtitles
            .iter()
            .find(|o| o.root.instance == next.root.instance)
        {
            next.x = old.x;
            next.y = old.y;
            next.width = old.width;
            next.height = old.height;
        }
    }
    core::mem::swap(&mut s.upkeep_roots, &mut s.pending_roots);
    s.pending_roots.clear();
    core::mem::swap(&mut s.upkeep_subtitles, &mut s.pending_subtitles);
    s.pending_subtitles.clear();
}

/// SharedSpinLock::LockShared / UnlockShared on the HUD layer's spawning lock.
struct SharedLock(&'static core::sync::atomic::AtomicI8);
impl SharedLock {
    unsafe fn new(lock: usize) -> SharedLock {
        let l = &*(lock as *const core::sync::atomic::AtomicI8);
        let mut spins = 0u32;
        loop {
            let state = l.load(Relaxed);
            if state != -1
                && l.compare_exchange(state, state.wrapping_add(1), Acquire, Relaxed)
                    .is_ok()
            {
                break;
            }
            spins = spins.wrapping_add(1);
            if spins & 511 == 0 {
                std::thread::yield_now();
            }
        }
        SharedLock(l)
    }
}
impl Drop for SharedLock {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Release);
    }
}

/// The HUD layer's entries (stride 0x130), read under its spawning lock by the caller.
unsafe fn entries(hud: usize) -> impl Iterator<Item = usize> {
    let data: usize = at(hud, HUD_LAYER_ENTRIES);
    let size: u32 = at(hud, HUD_LAYER_ENTRIES + 0xC);
    (0..size as usize).map(move |i| data + i * ENTRY_STRIDE)
}

/// The per-frame upkeep of the HUD roots; returns whether the interaction hub's loot plate is up.
fn upkeep_roots(s: &mut State, hud: usize, g: &GameState) -> bool {
    let scanner_name = CName::hashed("scanner").0;
    let details_name = CName::hashed("scanner_details").0;
    let mut scanner_textures: [(usize, [f32; 2]); 2] = [(0, [0.0; 2]); 2];
    if hud != 0 {
        // SAFETY: the live HUD layer, its entries read under its spawning lock.
        unsafe {
            let _lock = SharedLock::new(hud + HUD_LAYER_LOCK);
            for e in entries(hud) {
                let name: u64 = at(e, ENTRY_NAME);
                let index = if name == scanner_name {
                    0
                } else if name == details_name {
                    1
                } else {
                    continue;
                };
                let root: usize = at(e, ENTRY_ROOT_WIDGET);
                let texture: usize = at(e, ENTRY_SLOT_TEXTURE);
                if root != 0 && texture != 0 {
                    scanner_textures[index] = (
                        root,
                        [
                            at::<u32>(texture, TEXTURE_WIDTH) as f32,
                            at::<u32>(texture, TEXTURE_HEIGHT) as f32,
                        ],
                    );
                }
            }
        }
    }
    roots_stat(0);
    let mut loot_visible = false;
    let top_name = CName::hashed("topWidgets");
    let loot_name = CName::hashed("looting");
    let border_name = CName::hashed("border");
    let overlay_name = CName::hashed("scanner_overlay");
    let mut roots = core::mem::take(&mut s.upkeep_roots);
    for r in roots.iter_mut() {
        let root = r.root.instance as usize;
        let window = r.window.instance as usize;
        let info = r.info.instance as usize;
        if root == 0 || window == 0 {
            continue;
        }
        if r.flags & ROOT_GAZE_DOT != 0 {
            // The persistent dot and the braindance HUD: full-screen roots laid out about the screen's middle, which
            // under the asymmetric projection is not where the head looks. Moved onto the gaze line like the scanner's
            // crop.
            let y = if g.menu_mode == 0 {
                // SAFETY: the adapter; the export read.
                unsafe {
                    cvr_cpp_gaze_canvas_offset(
                        core::ptr::read_volatile(&raw const CyberpunkVR_HudRootHeight),
                        hud_design_scale(),
                    )
                }
            } else {
                0.0
            };
            // SAFETY: a live widget.
            let current = unsafe { at::<[f32; 2]>(root, WIDGET_TRANSLATION) }[1];
            if (current - y).abs() > 0.01 {
                let _ = call_vector2(root, "SetTranslation", [0.0, y]);
            }
            continue;
        }
        if info == 0 {
            continue;
        }
        roots_stat(1);
        // Hidden panels need no crop work or redraw; the last visible image is cleared once on hide.
        let is_active = active(root);
        if !is_active && r.was_active != 0 {
            repaint(s, &r.root);
            repaint(s, &r.window);
        }
        r.was_active = is_active as i8;
        if !is_active {
            continue;
        }
        roots_stat(2);
        if r.flags & ROOT_INTERACTION != 0 {
            // The loot plate and tooltip stay in their native window; only the XR follow policy changes.
            let top = child_named(root, top_name);
            let loot = if top.is_null() {
                Handle::null()
            } else {
                child_named(top.instance as usize, loot_name)
            };
            if !loot.is_null() && active(loot.instance as usize) && active(top.instance as usize) {
                loot_visible = true;
            }
        }
        let size = widget_size(root);
        if !(size[0] > 0.0 && size[1] > 0.0 && size[0] <= 16384.0 && size[1] <= 16384.0) {
            continue;
        }
        // SAFETY: the live inkHudEntryInfo.
        let (bounds, offset) = unsafe {
            (
                at::<[f32; 2]>(info, INFO_SIZE),
                at::<[f32; 2]>(info, INFO_OFFSET),
            )
        };
        let padding = r.padding;
        let (mut x, mut y, mut width, mut height) = (
            -padding,
            -padding,
            size[0] + padding * 2.0,
            size[1] + padding * 2.0,
        );
        if r.flags & ROOT_WINDOW_ORIGIN != 0 {
            // Centered roots with anchorPoint (0,0): the crop starts at the root's offset within its window.
            let origin = child_position(window, &r.root);
            if within(origin[0]) && within(origin[1]) {
                x = origin[0] - padding;
                y = origin[1] - padding;
            } else {
                x = offset[0];
                y = offset[1];
            }
        }
        if r.flags & ROOT_SCANNER != 0 {
            // The scanner's 1920x1080 root holds a 3840x2160 overlay at (-960,-540). The crop is CENTRED ON THE
            // OVERLAY'S MIDDLE, the reticle; clamped to the root and never smaller than before (the engine re-creates
            // the texture only when the crop grows).
            let scale = hud_design_scale();
            let border = child_named(root, border_name);
            let panel = if border.is_null() {
                Handle::null()
            } else {
                child_named(border.instance as usize, overlay_name)
            };
            let mut placed = false;
            if !panel.is_null() {
                let a = child_position(root, &border);
                let b = child_position(border.instance as usize, &panel);
                let sz = child_size(border.instance as usize, &panel);
                let origin = child_position(window, &r.root);
                let px = origin[0] + a[0] + b[0];
                let py = origin[1] + a[1] + b[1];
                if sz[0] > 0.0
                    && sz[1] > 0.0
                    && sz[0] < 16384.0
                    && sz[1] < 16384.0
                    && within(px)
                    && within(py)
                {
                    let cx = (px + sz[0] * 0.5).floor();
                    let cy = (py + sz[1] * 0.5).floor();
                    let mut half_w = cmax(
                        cmax(sz[0] * 0.5 + 32.0, cx - origin[0]),
                        origin[0] + size[0] - cx,
                    );
                    let mut half_h = cmax(
                        cmax(sz[1] * 0.5 + 32.0, cy - origin[1]),
                        origin[1] + size[1] - cy,
                    );
                    if bounds[0] > 0.0
                        && bounds[1] > 0.0
                        && bounds[0] < 16384.0
                        && bounds[1] < 16384.0
                    {
                        half_w = cmax(half_w, bounds[0] * 0.5);
                        half_h = cmax(half_h, bounds[1] * 0.5);
                    }
                    if scale > 0.0 {
                        // SAFETY: exports of the C++ half.
                        let (rw, rh) = unsafe {
                            (
                                core::ptr::read_volatile(&raw const CyberpunkVR_HudRootWidth),
                                core::ptr::read_volatile(&raw const CyberpunkVR_HudRootHeight),
                            )
                        };
                        half_w = cmin(half_w, (rw / scale).round() * 0.5);
                        half_h = cmin(half_h, (rh / scale).round() * 0.5);
                    }
                    half_w = half_w.ceil();
                    half_h = half_h.ceil();
                    x = cx - half_w;
                    y = cy - half_h;
                    width = 2.0 * half_w;
                    height = 2.0 * half_h;
                    placed = true;
                }
            }
            if !placed && bounds[0] > 0.0 && bounds[1] > 0.0 {
                x = offset[0];
                y = offset[1];
                width = bounds[0];
                height = bounds[1];
            }
        }
        if let Some((_, texture_size)) = scanner_textures.iter().find(|t| t.0 == root) {
            let crop = keep_crop_centered(
                Rect {
                    x,
                    y,
                    width,
                    height,
                },
                texture_size[0],
                texture_size[1],
                hud_design_scale(),
            );
            x = crop.x;
            y = crop.y;
            width = crop.width;
            height = crop.height;
        }
        if bounds[0] != width || bounds[1] != height {
            // SAFETY: the live inkHudEntryInfo.
            unsafe { set(info, INFO_SIZE, [width, height]) };
            repaint(s, &r.root);
            roots_stat(3);
        }
        if (padding > 0.0 || r.flags & (ROOT_SCANNER | ROOT_WINDOW_ORIGIN) != 0)
            && (offset[0] != x || offset[1] != y)
        {
            // SAFETY: as above.
            unsafe { set(info, INFO_OFFSET, [x, y]) };
            repaint(s, &r.root);
            roots_stat(4);
        }
        let window_size = widget_size(window);
        if window_size[0] != size[0] || window_size[1] != size[1] {
            roots_stat(5);
            if !set_widget_size(window, size) {
                roots_stat(6);
            }
        }
        // Fullscreen controllers cache their draw data; without a repaint every frame quest-icon animation periodically
        // leaves an empty offscreen image though the window stays visible.
        repaint(s, &r.root);
        repaint(s, &r.window);
    }
    s.upkeep_roots = roots;
    let mut subtitles = core::mem::take(&mut s.upkeep_subtitles);
    for sub in subtitles.iter_mut() {
        let root = sub.root.instance as usize;
        let window = sub.window.instance as usize;
        let info = sub.info.instance as usize;
        if root == 0 || window == 0 || info == 0 || sub.panel.is_null() {
            continue;
        }
        let is_active = active(root);
        if !is_active && sub.was_active != 0 {
            repaint(s, &sub.root);
            repaint(s, &sub.window);
        }
        sub.was_active = is_active as i8;
        if !is_active {
            continue;
        }
        // The game puts mainPanel above the 100px root (Y=-255); the authored crop starts at Y=0 and would clip the
        // whole line once the subtitles are a window of their own.
        let p = child_position(root, &sub.panel);
        let sz = child_size(root, &sub.panel);
        if !(sz[0] > 0.0
            && sz[1] > 0.0
            && sz[0] < 16384.0
            && sz[1] < 16384.0
            && within(p[0])
            && within(p[1]))
        {
            continue;
        }
        let x = cmin(sub.x, (p[0] - 32.0).floor());
        let y = cmin(sub.y, (p[1] - 32.0).floor());
        let width = cmax(sub.width, (p[0] + sz[0] + 32.0 - x).ceil());
        let height = cmax(sub.height, (p[1] + sz[1] + 32.0 - y).ceil());
        // SAFETY: the live inkHudEntryInfo.
        unsafe {
            let offset = at::<[f32; 2]>(info, INFO_OFFSET);
            let bounds = at::<[f32; 2]>(info, INFO_SIZE);
            if offset[0] != x || offset[1] != y || bounds[0] != width || bounds[1] != height {
                set(info, INFO_OFFSET, [x, y]);
                set(info, INFO_SIZE, [width, height]);
                roots_stat(7);
            }
        }
        repaint(s, &sub.root);
        repaint(s, &sub.window);
    }
    s.upkeep_subtitles = subtitles;
    loot_visible
}

/// A sprite with its texture lease, given back unless published.
struct Sprite {
    lease: *mut c_void,
    resource: *mut c_void,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    tint: [f32; 4],
    name: u64,
}
impl Default for Sprite {
    fn default() -> Self {
        Sprite {
            lease: core::ptr::null_mut(),
            resource: core::ptr::null_mut(),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            tint: [1.0; 4],
            name: 0,
        }
    }
}
impl Drop for Sprite {
    fn drop(&mut self) {
        if !self.lease.is_null() {
            // SAFETY: a lease this sprite holds.
            unsafe { cvr_cpp_hud_lease_release(self.lease) };
        }
    }
}
#[derive(Default)]
struct Snapshot {
    width: u32,
    height: u32,
    stamp: u64,
    generation: u64,
    masked: bool,
    /// The vehicle map projected; the map's share of the canvas height (the quest text fills the rest); the compact
    /// tile's normalized rectangle on the ordinary HUD canvas.
    windshield_map: bool,
    windshield_split: f32,
    hud_rect: [f32; 4],
    loot_visible: bool,
    empty_content: bool,
    sprites: Vec<Sprite>,
}
/// The snapshot handed to the HUD panel's capture, its leases with it.
fn publish(snapshot: &mut Snapshot, channel: u32) {
    let views: Vec<SpriteView> = snapshot
        .sprites
        .iter_mut()
        .map(|s| SpriteView {
            lease: core::mem::replace(&mut s.lease, core::ptr::null_mut()),
            resource: s.resource,
            x: s.x,
            y: s.y,
            width: s.width,
            height: s.height,
            tint: s.tint,
            name: s.name,
        })
        .collect();
    let view = SnapshotView {
        width: snapshot.width,
        height: snapshot.height,
        stamp: snapshot.stamp,
        generation: snapshot.generation,
        masked: snapshot.masked,
        loot_visible: snapshot.loot_visible,
        empty_content: snapshot.empty_content,
        sprites: views.as_ptr(),
        count: views.len(),
        windshield_map: snapshot.windshield_map,
        windshield_split: snapshot.windshield_split,
        hud_rect: snapshot.hud_rect,
    };
    // SAFETY: the views and their leases, handed over once.
    unsafe { hud_panel::cvr_hud_panel_publish(&view, channel) };
}

/// The slot's place on the root: its geometry walked up the parents, its tint and opacity on the way.
fn geometry(
    s: &State,
    mut child: Handle,
    root: usize,
    sprite: &mut Sprite,
    dirty: &mut bool,
) -> bool {
    let mut first = true;
    for _ in 0..24 {
        if child.is_null() {
            return false;
        }
        let c = child.instance as usize;
        // SAFETY: a live widget.
        let flags: u8 = unsafe { at(c, WIDGET_FLAGS) };
        *dirty |= flags & 0x06 != 0;
        let visible = match s.masks.get(&c) {
            Some(mask) => mask.wanted_visible.load(Relaxed),
            None => widget_visible(c),
        };
        let opacity = widget_opacity(c);
        if !visible || opacity <= 0.0 {
            return false;
        }
        // SAFETY: as above.
        let t: [f32; 4] = unsafe { at(c, WIDGET_TINT) };
        let alpha = clamp(opacity * t[3], 0.0, 1.0);
        sprite.tint[0] *= t[0] * alpha;
        sprite.tint[1] *= t[1] * alpha;
        sprite.tint[2] *= t[2] * alpha;
        sprite.tint[3] *= alpha;
        if c == root {
            return !first
                && (sprite.x + sprite.y + sprite.width + sprite.height).is_finite()
                && sprite.width > 0.0
                && sprite.height > 0.0;
        }
        // SAFETY: the widget's weak parent.
        let parent = unsafe { lock_weak(c, WIDGET_PARENT) };
        if parent.is_null() {
            return false;
        }
        let position = child_position(parent.instance as usize, &child);
        if first {
            let size = child_size(parent.instance as usize, &child);
            sprite.width = size[0];
            sprite.height = size[1];
            first = false;
        }
        sprite.x += position[0];
        sprite.y += position[1];
        child = parent;
    }
    false
}

struct CallTimer(i64);
impl Drop for CallTimer {
    fn drop(&mut self) {
        timing(0, 1);
        timing(1, now_qpc() - self.0);
    }
}

/// VRHudPanelUpdate(root, lootVisible) -> Int32: the HUD's separate-window slots as sprites, published per channel and
/// masked where their consumer is ready; the number of sprites, negative when some are not masked.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRHudPanelUpdate(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let _timer = CallTimer(now_qpc());
    let mut a = args(frame);
    let root = a.get::<Handle>();
    let loot_visible = a.get::<bool>();
    a.finish();
    let result = panel_update_untimed(root, loot_visible);
    if !out.is_null() {
        *(out as *mut i32) = result;
    }
}
/// VRHudPanelUpdate's work, for the plugin's own callers (the Stereo mod's hud_panel in Rust): the number of sprites,
/// negative when some are not masked.
///
/// # Safety
/// On the game's main thread; `root` is null or a live inkWidget's handle.
pub(crate) unsafe fn panel_update(root: Handle, loot_visible: bool) -> i32 {
    let _timer = CallTimer(now_qpc());
    panel_update_untimed(root, loot_visible)
}
unsafe fn panel_update_untimed(root: Handle, mut loot_visible: bool) -> i32 {
    if !verified() || !HOOKS_READY.load(Acquire) {
        return 0;
    }
    REPAINT_THREAD.store(GetCurrentThreadId(), Relaxed);
    let g = game();
    let result = with_state(|s| {
        flush_repaints(s, 2); // still queued: no frame submit since the last update
                              // Before anything below can return: the script's upkeep ran whatever the HUD panel's state.
        commit_scan(s);
        // Dedicated camera/turret weapons retain their own optical aiming axis.
        cvr_cpp_gaze_head_look(g.menu_mode == 0 && !g.device_cam && !g.head_aim_weapon);
        let r = root.instance as usize;
        let proxy: usize = if r != 0 { at(r, WIDGET_LAYER_PROXY) } else { 0 };
        let layer = if proxy != 0 {
            lock_weak(proxy, LAYER_PROXY_LAYER)
        } else {
            Handle::null()
        };
        let l = layer.instance as usize;
        let hud_layer =
            l != 0 && class_of(l).is_some_and(|c| c.class_name() == CName::hashed("inkHUDLayer"));
        let root_size = if r != 0 { widget_size(r) } else { [0.0; 2] };
        if hud_layer
            && root_size[0] > 0.0
            && root_size[1] > 0.0
            && root_size[0] <= 16384.0
            && root_size[1] <= 16384.0
        {
            core::ptr::write_volatile(&raw mut CyberpunkVR_HudRootWidth, root_size[0]);
            core::ptr::write_volatile(&raw mut CyberpunkVR_HudRootHeight, root_size[1]);
        }
        if hud_layer {
            let window: usize = at(l, LAYER_WINDOW);
            core::ptr::write_volatile(&raw mut CyberpunkVR_HudLayerAddress, [l, window]);
        }
        if CyberpunkVR_HudRootsNative.load(Relaxed) != 0
            && upkeep_roots(s, if hud_layer { l } else { 0 }, &g)
        {
            loot_visible = true;
        }
        let stamp = GetTickCount64();
        let mut snapshots: Vec<Snapshot> = (0..CHANNELS)
            .map(|_| Snapshot {
                stamp,
                windshield_split: 1.0,
                ..Default::default()
            })
            .collect();
        snapshots[INTERACTION as usize].loot_visible = loot_visible;
        let publish_all = |snapshots: &mut Vec<Snapshot>| {
            for (i, snapshot) in snapshots.iter_mut().enumerate() {
                publish(snapshot, i as u32);
            }
        };
        if g.hud_panel == 0 || g.menu_mode != 0 || g.menu_rect != 0 || !hud_layer {
            restore(s);
            s.last_root = 0;
            s.last_masked = [false; CHANNELS];
            s.generation += 1;
            publish_all(&mut snapshots);
            return 0;
        }
        if r != s.last_root {
            restore(s);
            s.layout_cache.clear();
            s.last_root = r;
            s.generation += 1;
        }
        let hud = l;
        let root_width = root_size[0] as u32;
        let height = root_size[1] as u32;
        if root_width == 0 || height == 0 || root_width > 8192 || height > 8192 {
            restore(s);
            publish_all(&mut snapshots);
            return 0;
        }
        // The canvas: the root, or one eye's root under the engine's stereo (to_eye_root).
        let design_ratio = cvr_render::engine_stereo::cvr_engine_stereo_hud_design_ratio(
            root_width as f32,
            height as f32,
        );
        let width = if design_ratio != 1.0 && g.launcher_height > 0 {
            (height as f64 * g.launcher_width as f64 / g.launcher_height as f64).round() as i64
                as u32
        } else {
            root_width
        };
        for snapshot in snapshots.iter_mut() {
            snapshot.width = width;
            snapshot.height = height;
        }
        if s.layout_width != root_width || s.layout_height != height {
            s.layout_cache.clear();
            s.layout_width = root_width;
            s.layout_height = height;
        }
        let mut slots: Vec<Vec<Handle>> = (0..CHANNELS).map(|_| Vec::new()).collect();
        let mut complete = [true; CHANNELS];
        let settings = cvr_settings::hud_layout::settings();
        let sniper_nest = g.sniper_follow;
        let mut status = [ElementStatus::default(); ELEMENTS];
        // DRIVING WITH THE MAP PROJECTED (over a car's hood or a bike's bars): the quest text goes with it, under the map.
        let mut cabin = cvr_body::vehicle_minimap::VehicleMinimapAnchor::default();
        let projected_map = cvr_rig::vrik::wheel_grab::cvr_anim_wheel_read_minimap_anchor(cvr_xr::tracking::origin_serial(), &mut cabin);
        let (mut map_rect, mut quest_rect) = (Rect::default(), Rect::default());
        let mut quest_sprite: Option<usize> = None;
        {
            let _lock = SharedLock::new(hud + HUD_LAYER_LOCK);
            for e in entries(hud) {
                let slot_widget: usize = at(e, ENTRY_SLOT_WIDGET);
                let slot_texture: usize = at(e, ENTRY_SLOT_TEXTURE);
                let window: usize = at(e, ENTRY_WINDOW);
                if !at::<bool>(e, ENTRY_SEPARATE_WINDOW)
                    || slot_widget == 0
                    || slot_texture == 0
                    || window == 0
                {
                    continue;
                }
                let name: u64 = at(e, ENTRY_NAME);
                let quest_in_map = projected_map && name == entry_hash(b"quest_list");
                let channel = if quest_in_map { VEHICLE_MINIMAP } else { entry_channel(name, sniper_nest) };
                if channel == INTERACTION && g.interaction_panel == 0 {
                    continue;
                }
                let group = channel as usize;
                let slot = handle_at(e, ENTRY_SLOT_WIDGET);
                let mut sprite = Sprite::default();
                let mut dirty = false;
                timing(5, 1);
                let t_geometry = now_qpc();
                let placed = geometry(s, slot.clone(), r, &mut sprite, &mut dirty);
                let t_texture = now_qpc();
                timing(2, t_texture - t_geometry);
                if !placed {
                    continue;
                }
                let mut resource: *mut c_void = core::ptr::null_mut();
                sprite.lease = cvr_cpp_hud_lease(slot_texture as *mut c_void, false, &mut resource);
                sprite.resource = resource;
                timing(3, now_qpc() - t_texture);
                if sprite.lease.is_null()
                    || snapshots[group].sprites.len() >= hud_panel::MAX_SPRITES
                {
                    complete[group] = false;
                    continue;
                }
                sprite.name = name;
                let mut rect = s.layout_cache.entry(name).or_default().update(
                    Rect {
                        x: sprite.x,
                        y: sprite.y,
                        width: sprite.width,
                        height: sprite.height,
                    },
                    dirty,
                );
                if width != root_width {
                    rect = to_eye_root(
                        rect,
                        root_width as f32,
                        height as f32,
                        width as f32,
                        design_ratio,
                    );
                }
                // THE TEXTURE'S OWN ASPECT (2026-11-24): the subtitles' slot fills the root's width (1864 x 145 on a
                // 1864-wide canvas) while their window renders the crop (1900 x 300 design units: a 922 x 145
                // texture) -- drawn into the slot, every line was stretched to twice its width. A slot wider or taller
                // than its texture keeps the texture's aspect, centred in it.
                let tw = at::<u32>(slot_texture, TEXTURE_WIDTH) as f32;
                let th = at::<u32>(slot_texture, TEXTURE_HEIGHT) as f32;
                let fitted = fit_aspect(rect, tw, th);
                if fitted.width != rect.width || fitted.height != rect.height {
                    note_fit(name, rect, tw, th);
                    if name == entry_hash(b"subtitles") {
                        rect = fitted;
                    }
                }
                let index = cvr_settings::hud_layout::element_index(name);
                if (0..ELEMENTS as i32).contains(&index) {
                    let i = index as usize;
                    status[i] = ElementStatus {
                        available: true,
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    };
                    rect = apply_element(rect, settings[i], width as f32, height as f32);
                    let opacity = if settings[i].visible != 0 {
                        settings[i].opacity
                    } else {
                        0.0
                    };
                    for c in sprite.tint.iter_mut() {
                        *c *= opacity;
                    }
                }
                if name == entry_hash(b"subtitles") {
                    // THE SUBTITLES' PLACEMENT (a diagnostic of their aspect): the slot's rectangle, its texture, the
                    // canvas, logged when they change
                    static LAST: Mutex<[i32; 8]> = Mutex::new([0; 8]);
                    static SAID: AtomicU32 = AtomicU32::new(0);
                    let tw = at::<u32>(slot_texture, TEXTURE_WIDTH) as i32;
                    let th = at::<u32>(slot_texture, TEXTURE_HEIGHT) as i32;
                    let now = [rect.x as i32, rect.y as i32, rect.width as i32, rect.height as i32, tw, th, width as i32, height as i32];
                    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
                    if *last != now && SAID.fetch_add(1, Relaxed) < 24 {
                        *last = now;
                        log!(
                            "[hud-panel] subtitles: the slot at ({}, {}) {} x {} (aspect {:.3}), its texture {} x {} (aspect {:.3}), the canvas {} x {} (root {} x {}, ratio {:.3})\n",
                            now[0], now[1], now[2], now[3], rect.width / rect.height.max(1.0), tw, th, tw as f32 / (th.max(1) as f32), width, height, root_width, height, design_ratio
                        );
                    }
                }
                sprite.x = rect.x;
                sprite.y = rect.y;
                sprite.width = rect.width;
                sprite.height = rect.height;
                if quest_in_map {
                    quest_rect = rect; // placed under the map once both are known (below)
                } else if channel == VEHICLE_MINIMAP {
                    // One texture, two placements: never a second minimap in Main. Its authored (or the user's)
                    // layout kept for the exact restoration outside the car.
                    let map = &mut snapshots[group];
                    map.hud_rect = [rect.x / width as f32, rect.y / height as f32, rect.width / width as f32, rect.height / height as f32];
                    map_rect = rect;
                    map.windshield_map = projected_map;
                    map.width = at::<u32>(slot_texture, TEXTURE_WIDTH).clamp(32, 2048);
                    map.height = at::<u32>(slot_texture, TEXTURE_HEIGHT).clamp(32, 2048);
                    sprite.x = 0.0;
                    sprite.y = 0.0;
                    sprite.width = map.width as f32;
                    sprite.height = map.height as f32;
                }
                // A window with nothing visible contributes only zeros: keep its slot (masked), skip its draw.
                let t_content = now_qpc();
                let content = CyberpunkVR_HudSkipEmpty.load(Relaxed) == 0
                    || has_visible_content(at::<usize>(e, ENTRY_ROOT_WIDGET), 0);
                timing(4, now_qpc() - t_content);
                if content {
                    snapshots[group].sprites.push(sprite);
                    timing(7, 1);
                    if quest_in_map {
                        quest_sprite = Some(snapshots[group].sprites.len() - 1);
                    }
                } else {
                    CyberpunkVR_HudEmptySprites.fetch_add(1, Relaxed);
                }
                slots[group].push(slot);
            }
        }
        // THE QUEST TEXT UNDER THE PROJECTED MAP, left-aligned with it as on the HUD, at three quarters of its HUD size
        // against the map's (the map texture's pixels per HUD pixel), its rows stretched as the map's are on the
        // projected image so the letters keep their shape there (vehicle_minimap.rs, hud_quad.rs). The quest list's
        // widget is far taller than its text, which starts at its top: the canvas shows a band of half the map's height
        // under it, about three lines, and clips the rest. The band is there whenever the map is projected, so the
        // image keeps its size when the quest text comes and goes.
        {
            const TEXT: f32 = 0.75;
            const GAP: f32 = 0.04;
            const BAND: f32 = 0.5;
            let map = &mut snapshots[VEHICLE_MINIMAP as usize];
            let quest = quest_sprite.filter(|&q| q < map.sprites.len());
            if projected_map && map.width != 0 && map.height != 0 && map.windshield_map && map_rect.width > 1.0 && map_rect.height > 1.0 {
                let place = cvr_body::vehicle_minimap::minimap_placement_for(cabin.bike);
                let (map_w, map_h) = (map.width as f32, map.height as f32);
                let scale = map_w / map_rect.width * TEXT;
                map.height = (map_h * (1.0 + GAP + BAND)).round() as u32;
                map.windshield_split = map_h / map.height as f32;
                if let Some(q) = quest {
                    if quest_rect.width > 1.0 && quest_rect.height > 1.0 {
                        let (mut w, mut h) = (quest_rect.width * scale, quest_rect.height * scale);
                        if w > map_w {
                            h *= map_w / w;
                            w = map_w;
                        }
                        let text = &mut map.sprites[q];
                        text.x = 0.0;
                        text.y = map_h * (1.0 + GAP);
                        text.width = w;
                        text.height = h * (place.width / map_w) / (place.height / map_h);
                    } else {
                        map.sprites.remove(q);
                    }
                }
            } else if let Some(q) = quest {
                map.sprites.remove(q); // no map to hang under this frame
            }
        }
        let t_tail = now_qpc();
        cvr_settings::hud_layout::set_status(&status);
        // Each XR consumer acknowledges only its own group. A missing dialogue texture must not hide ordinary HUD, or
        // mask a native prompt without a copy.
        let mut desired: FastMap<usize, u32> = cvr_host::fast::fast_map();
        let mut total = 0i32;
        let mut all_masked = true;
        for i in 0..CHANNELS {
            let snapshot = &mut snapshots[i];
            snapshot.masked = complete[i] && !slots[i].is_empty() && mask_native(i as u32);
            if snapshot.masked {
                for slot in &slots[i] {
                    desired.entry(slot.instance as usize).or_insert(i as u32);
                }
            }
            if snapshot.masked != s.last_masked[i] {
                s.generation += 1;
                s.last_masked[i] = snapshot.masked;
            }
            snapshot.generation = s.generation;
            if !complete[i] {
                snapshot.sprites.clear();
            }
            snapshot.empty_content =
                complete[i] && snapshot.sprites.is_empty() && !slots[i].is_empty();
            total += snapshot.sprites.len() as i32;
            if !snapshot.sprites.is_empty() && !snapshot.masked {
                all_masked = false;
            }
        }
        let mut retired: Vec<Arc<Mask>> = Vec::new();
        let mut changed = false;
        s.masks.retain(|widget, mask| match desired.get(widget) {
            None => {
                retired.push(mask.clone());
                changed = true;
                false
            }
            Some(channel) => {
                mask.channel.store(*channel, Release);
                true
            }
        });
        for (i, group) in slots.iter().enumerate() {
            if !snapshots[i].masked {
                continue;
            }
            for slot in group {
                let w = slot.instance as usize;
                if !s.masks.contains_key(&w) {
                    s.masks
                        .insert(w, Arc::new(Mask::new(slot.clone(), i as u32)));
                    changed = true;
                }
            }
        }
        if changed {
            publish_masks(s);
        }
        for old in &retired {
            restore_mask(old);
        }
        for widget in desired.keys() {
            hide_slot(*widget);
        }
        let current = total * if all_masked { 1 } else { -1 };
        if core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 {
            static PREVIOUS: core::sync::atomic::AtomicI32 =
                core::sync::atomic::AtomicI32::new(-1000);
            if PREVIOUS.swap(current, Relaxed) != current {
                log::write(&format!(
                    "[hud-panel] main={} interactions={} masked={}/{} canvas={}x{}\n",
                    snapshots[0].sprites.len(),
                    snapshots[1].sprites.len(),
                    snapshots[0].masked as i32,
                    snapshots[1].masked as i32,
                    width,
                    height
                ));
            }
        }
        publish_all(&mut snapshots);
        timing(6, now_qpc() - t_tail);
        current
    });
    result
}

/// VRHudPanelRootsNative() -> Bool: whether VRHudPanelUpdate does the per-frame root upkeep (false when its hooks
/// could not be verified, so the script keeps doing it).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRHudPanelRootsNative(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    args(frame).finish();
    if !out.is_null() {
        *(out as *mut bool) = panel_roots_native();
    }
}
/// VRHudPanelRootsNative's answer.
pub(crate) fn panel_roots_native() -> bool {
    // SAFETY: the plugin's own settings flag, an atomic.
    verified() && HOOKS_READY.load(Acquire) && unsafe { CyberpunkVR_HudRootsNative.load(Relaxed) } != 0
}
/// VRHudPanelRootsBegin(): a new scan; the roots that follow replace the current ones at the next VRHudPanelUpdate.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRHudPanelRootsBegin(
    _context: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    args(frame).finish();
    panel_roots_begin();
}
/// VRHudPanelRootsBegin's work: a new scan.
pub(crate) fn panel_roots_begin() {
    with_state(|s| {
        s.pending_roots.clear();
        s.pending_subtitles.clear();
        s.pending_scan = true;
    });
}
/// GetUserData hands the script an inkUserData reference; the crop is only ever an inkHudEntryInfo.
fn entry_info(data: &Handle) -> Handle {
    if is_a(data.instance as usize, "inkHudEntryInfo") {
        data.clone()
    } else {
        Handle::null()
    }
}
fn rejected_once(what: &str, root: bool, window: bool, data: bool, info: bool) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, SeqCst) {
        log::write(&format!(
            "[hud-panel] {what} could not be taken (root={} window={} userData={} inkHudEntryInfo={}); the script keeps the upkeep\n",
            root as i32, window as i32, data as i32, info as i32
        ));
    }
}
/// VRHudPanelRoot(root, window, userData, flags, padding) -> Bool: false when a root cannot be taken (the script then
/// keeps doing its upkeep itself).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRHudPanelRoot(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = args(frame);
    let root = a.get::<Handle>();
    let window = a.get::<Handle>();
    let data = a.get::<Handle>();
    let flags = a.get::<i32>();
    let padding = a.get::<f32>();
    a.finish();
    let taken = panel_root(root, window, data, flags, padding);
    if !out.is_null() {
        *(out as *mut bool) = taken;
    }
}
/// VRHudPanelRoot's work: false when the root cannot be taken.
pub(crate) fn panel_root(root: Handle, window: Handle, data: Handle, flags: i32, padding: f32) -> bool {
    let info = entry_info(&data);
    if root.is_null() || window.is_null() || (info.is_null() && (flags as u32 & ROOT_GAZE_DOT) == 0)
    {
        rejected_once(
            "a root",
            !root.is_null(),
            !window.is_null(),
            !data.is_null(),
            !info.is_null(),
        );
        return false;
    }
    with_state(|s| {
        s.pending_roots.push(UpkeepRoot {
            root,
            window,
            info,
            padding,
            flags: flags as u32,
            was_active: -1,
        });
    });
    true
}
/// VRHudPanelSubtitle(root, window, userData, panel) -> Bool.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRHudPanelSubtitle(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = args(frame);
    let root = a.get::<Handle>();
    let window = a.get::<Handle>();
    let data = a.get::<Handle>();
    let panel = a.get::<Handle>();
    a.finish();
    let taken = panel_subtitle(root, window, data, panel);
    if !out.is_null() {
        *(out as *mut bool) = taken;
    }
}
/// VRHudPanelSubtitle's work: false when the subtitle root cannot be taken.
pub(crate) fn panel_subtitle(root: Handle, window: Handle, data: Handle, panel: Handle) -> bool {
    let info = entry_info(&data);
    if root.is_null() || window.is_null() || info.is_null() || panel.is_null() {
        rejected_once(
            "a subtitle root",
            !root.is_null(),
            !window.is_null(),
            !data.is_null(),
            !info.is_null(),
        );
        return false;
    }
    let i = info.instance as usize;
    // SAFETY: the inkHudEntryInfo checked above, held.
    let (offset, size) = unsafe { (at::<[f32; 2]>(i, INFO_OFFSET), at::<[f32; 2]>(i, INFO_SIZE)) };
    with_state(|s| {
        s.pending_subtitles.push(UpkeepSubtitle {
            root,
            window,
            panel,
            info,
            x: offset[0],
            y: offset[1],
            width: size[0],
            height: size[1],
            was_active: -1,
        });
    });
    true
}
/// VRPhysicalOpticsProbe(component) -> Bool: the optics probe asked to read a component's texture -- a world widget's
/// (the original scope UI as well as the port's panels, so a captured artifact can be attributed to its layer) or the
/// optics camera's render target.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRPhysicalOpticsProbe(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = args(frame);
    let component = a.get::<Handle>();
    a.finish();
    if !out.is_null() {
        *(out as *mut bool) = false;
    }
    if !verified() || component.is_null() {
        return;
    }
    let c = component.instance as usize;
    let request = |texture: usize| -> bool {
        let mut resource: *mut c_void = core::ptr::null_mut();
        let lease = cvr_cpp_hud_lease(texture as *mut c_void, false, &mut resource);
        cvr_cpp_optics_probe_request(lease)
    };
    if is_a(c, "WorldWidgetComponent") {
        let texture: usize = at(c, WORLD_WIDGET_TEXTURE);
        if texture == 0 || at::<usize>(texture, 0) != base() + DYNAMIC_TEXTURE_VTABLE {
            return;
        }
        let source = Handle::from_instance(texture as *mut c_void);
        if !out.is_null() {
            *(out as *mut bool) = request(source.instance as usize);
        }
        return;
    }
    if at::<u64>(c, COMPONENT_NAME) != CName::hashed("vr_optics_camera").0 {
        return;
    }
    if !is_a(c, "entRenderToTextureCameraComponent") {
        return;
    }
    let source = handle_at(c, RTT_CAMERA_SOURCE);
    if !source.is_null() && !out.is_null() {
        *(out as *mut bool) = request(source.instance as usize);
    }
}
