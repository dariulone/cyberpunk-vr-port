//! THE OVERLAY (formerly src/Overlay/ImGuiOverlay.cpp; include/Overlay/ImGuiOverlay.hpp is its contract): Dear ImGui
//! on the game's Present -- the context and its fonts, the backbuffers' render targets, the frame (the VR menu's shell
//! into a canvas the VR panel shows, or the world-space debug drawing and the Lua mods' windows into the backbuffer),
//! the settings mirror on the desktop, the overlay in the second eye, and the window procedure that feeds it the
//! mouse and keys.
//!
//! PACING (satyaloka93's measurements): drain every frame -- 43.8 fps, stable; previous-overlay fence -- 62.2 fps,
//! eight DXGI_ERROR_DEVICE_HUNG; the fence plus a guard -- 70.3 fps, stable. A frame waits only for the previous
//! overlay submission (fences are monotonic: that covers this backbuffer slot's allocator and ImGui's upload ring),
//! and for a few seconds after resource churn (a save load, a swapchain invalidate, a VRCAM re-bind) every submission
//! drains the queue. CyberpunkVR_OverlayPacing: 0 drain every frame, 2 the fence without the guard, 3 both (default).
//!
//! The exports the tools read and write stay the C++ half's (src/Overlay/ImGuiOverlay.cpp), as do the world-space
//! debug drawing, the Lua host and the game font, reached through the runtime below.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicIsize, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*,
};
use std::sync::{Arc, Mutex, MutexGuard};

use cvr_body::vr_overlay::{self as vrui, Hit, PointerEvent, CANVAS_HEIGHT, CANVAS_WIDTH};
use cvr_crt::{format, Arg};
use cvr_settings::live_controls_ui_state::LiveControlsUiState;
use cvr_xr::vr_panel::{self, Canvas};
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain, IDXGISwapChain3};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::SystemInformation::{GetTickCount64, GetWindowsDirectoryA};
use windows::Win32::System::Threading::{CreateEventA, WaitForSingleObject};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, VK_ESCAPE, VK_F10, VK_INSERT};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::imgui::{self, DrawData, Io};

// ---- the C++ half --------------------------------------------------------------------------------------------------

/// What the overlay asks of the C++ half.
#[repr(C)]
pub struct OverlayRuntime {
    /// overlay::LoadCyberpunkFont (Fbt/Game/Helpers.cpp): the game's typeface for the calibration texts.
    load_cyberpunk_font: extern "C" fn(),
    /// The world-space debug drawing (Overlay/OverlayDebugDraw.cpp, Fbt/Game/Helpers.cpp), in its order: the hand
    /// locator, the barrel crosshair, the left gun's dot, the full-body calibration's helpers, the ADS telemetry.
    debug_draws: extern "C" fn(),
    /// The Lua host (src/Lua): its windows, its window-message hook, whether its overlay is open, raw input.
    lua_draw: extern "C" fn(f32, f32),
    lua_on_wndproc: extern "C" fn(*mut c_void, u32, usize, isize),
    lua_overlay_open: extern "C" fn() -> bool,
    lua_pass_raw_input: extern "C" fn(isize) -> bool,
}
/// The table, built here (formerly src/Overlay/ImGuiOverlay.cpp's).
mod table {
    use super::OverlayRuntime;
    use core::ffi::c_void;
    unsafe extern "C" {
        /// overlay::LoadCyberpunkFont(ImGui::GetIO()) (src/Fbt/Game/Helpers.cpp).
        safe fn cvr_cpp_load_cyberpunk_font();
        /// The Lua host (src/Lua, kept C++): its windows, its window-message hook, its overlay, raw input.
        safe fn cvr_cpp_lua_draw(width: f32, height: f32);
        safe fn cvr_cpp_lua_on_wndproc(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize);
        safe fn cvr_cpp_lua_overlay_open() -> bool;
        safe fn cvr_cpp_lua_pass_raw_input(lparam: isize) -> bool;
    }
    /// The world-space debug drawing, in its order: the hand locator, the collision view, the barrel crosshair, the left gun's dot, the
    /// full-body calibration's helpers, the ADS telemetry.
    extern "C" fn debug_draws() {
        crate::overlay_hands::cvr_overlay_hand_locator();
        crate::overlay_debug_view::cvr_overlay_debug_view();
        crate::overlay_crosshair::cvr_overlay_barrel_crosshair();
        crate::overlay_barrel::cvr_overlay_left_barrel_dot();
        crate::overlay_fbt_helpers::cvr_overlay_fbt_helpers();
        crate::overlay_telemetry::cvr_overlay_compact_ads_telemetry();
    }
    pub(super) static RT: OverlayRuntime = OverlayRuntime {
        load_cyberpunk_font: cvr_cpp_load_cyberpunk_font,
        debug_draws,
        lua_draw: cvr_cpp_lua_draw,
        lua_on_wndproc: cvr_cpp_lua_on_wndproc,
        lua_overlay_open: cvr_cpp_lua_overlay_open,
        lua_pass_raw_input: cvr_cpp_lua_pass_raw_input,
    };
}
static RUNTIME: AtomicPtr<OverlayRuntime> = AtomicPtr::new((&raw const table::RT).cast_mut());
#[no_mangle]
pub extern "C" fn cvr_overlay_attach(runtime: *const OverlayRuntime) {
    RUNTIME.store(runtime as *mut OverlayRuntime, Release);
}
/// The Lua host's place in the overlay taken by another host (the Rust build without the Lua host: its CET mods'
/// windows, rust/crates/cvr-ffi/src/mechanics/dev_overlay.rs): its windows drawn, its window-message hook, whether
/// its overlay is open, which raw input passes while it is. The font and the debug drawing stay this table's.
pub fn attach_host(
    draw: extern "C" fn(f32, f32),
    on_wndproc: extern "C" fn(*mut c_void, u32, usize, isize),
    open: extern "C" fn() -> bool,
    pass_raw_input: extern "C" fn(isize) -> bool,
) {
    let runtime = Box::leak(Box::new(OverlayRuntime {
        load_cyberpunk_font: table::RT.load_cyberpunk_font,
        debug_draws: table::RT.debug_draws,
        lua_draw: draw,
        lua_on_wndproc: on_wndproc,
        lua_overlay_open: open,
        lua_pass_raw_input: pass_raw_input,
    }));
    cvr_overlay_attach(runtime);
}
fn rt() -> &'static OverlayRuntime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
}
fn lua_overlay_open() -> bool {
    (rt().lua_overlay_open)()
}
/// Whether the mods' windows are open, whoever draws them (the Lua host or the Rust build's own): the cursor hooks
/// (cvr-rig os_input) leave the desktop's pointer to them then.
#[no_mangle]
pub extern "C" fn cvr_overlay_host_open() -> bool {
    lua_overlay_open()
}

extern "C" {
    static mut CyberpunkVR_OverlayPacing: i32;
    static mut CyberpunkVR_OverlaySkipEmpty: i32;
    static mut CyberpunkVR_DebugOverlayWaitMs: f64;
    static mut CyberpunkVR_DebugOverlayDrains: u64;
    static mut CyberpunkVR_DebugOverlayWaitTimeouts: u64;
    static mut CyberpunkVR_DebugOverlayGuardArms: u64;
    static mut CyberpunkVR_DebugOverlayEmptyFrames: u64;
    static mut CyberpunkVR_DebugOverlaySubmissions: u64;
    static mut CyberpunkVR_DebugOverlayWaits: u64;
    static mut CyberpunkVR_DebugOverlayWaitTotalUs: u64;
    static mut CyberpunkVR_ImGuiDump: i32;
    static mut CyberpunkVR_ImGuiDumpTabs: i32;
    static mut CyberpunkVR_VrShellTab: i32;
    static mut CyberpunkVR_OverlaySecondEye: i32;
    static mut CyberpunkVR_DebugOverlaySecondEyeDraws: u64;
    static mut CyberpunkVR_DebugOverlaySecondEyeSkips: u64;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    fn GetLiveControlsUiState(out: *mut LiveControlsUiState);
    fn cvr_paths_dir_a(folder: u32) -> *const c_char;
}
macro_rules! g {
    ($name:ident) => {{
        // SAFETY: an export of the C++ half, alive for the process.
        #[allow(unused_unsafe)]
        let value = unsafe { core::ptr::read_volatile(&raw const $name) };
        value
    }};
}
macro_rules! s {
    ($name:ident, $value:expr) => {{
        let value = $value;
        // SAFETY: as g!.
        #[allow(unused_unsafe)]
        unsafe {
            core::ptr::write_volatile(&raw mut $name, value)
        }
    }};
}
/// CVR_DIAGNOSTIC(++counter): counted only while the runtime diagnostics are on.
macro_rules! diag_inc {
    ($name:ident) => {{
        if diagnostics() {
            s!($name, g!($name).wrapping_add(1));
        }
    }};
}
fn diagnostics() -> bool {
    g!(CyberpunkVR_RuntimeDiagnostics) != 0
}
fn verbose() -> bool {
    // SAFETY: the C++ half's volatile int.
    unsafe { core::ptr::read_volatile(crate::vr_core_settings::verbose_log_ptr()) != 0 }
}
fn log(fmt: &CStr, args: &[Arg]) {
    cvr_host::log::write(&format(fmt, args).to_string_lossy());
}
fn tick() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { GetTickCount64() }
}

// ---- the state -----------------------------------------------------------------------------------------------------

#[derive(Default)]
struct FrameContext {
    allocator: Option<ID3D12CommandAllocator>,
    desktop_allocator: Option<ID3D12CommandAllocator>,
    desktop_fence: u64,
    render_target: Option<ID3D12Resource>,
    rtv: D3D12_CPU_DESCRIPTOR_HANDLE,
    /// The overlay fence value that last used this slot's allocator.
    fence_value: u64,
}
struct Overlay {
    device: Option<ID3D12Device>,
    queue: Option<ID3D12CommandQueue>,
    swap_chain3: Option<IDXGISwapChain3>,
    rtv_heap: Option<ID3D12DescriptorHeap>,
    srv_heap: Option<ID3D12DescriptorHeap>,
    cmd_list: Option<ID3D12GraphicsCommandList>,
    fence: Option<ID3D12Fence>,
    /// The fence's event (a HANDLE), 0 when none.
    fence_event: isize,
    fence_value: u64,
    /// The last submission's value: what the next frame waits for.
    previous_overlay_fence_value: u64,
    rtv_descriptor_size: u32,
    frame_count: u32,
    rtv_format: DXGI_FORMAT,
    /// The format the ImGui pipeline state was built for (at Init), which the second-eye pass validates against.
    imgui_pso_format: DXGI_FORMAT,
    /// The background list (the world-projected markers), captured while the frame is still open.
    bg_draw_list: usize,
    /// A ring of four RTVs for the second eye, so a descriptor is never rewritten while a frame using it flies.
    eye_rtv_heap: Option<ID3D12DescriptorHeap>,
    eye_rtv_stride: u32,
    eye_rtv_slot: u32,
    frames: Vec<FrameContext>,
    draw_data_submitted: bool,
}
static OVERLAY: Mutex<Overlay> = Mutex::new(Overlay {
    device: None,
    queue: None,
    swap_chain3: None,
    rtv_heap: None,
    srv_heap: None,
    cmd_list: None,
    fence: None,
    fence_event: 0,
    fence_value: 0,
    previous_overlay_fence_value: 0,
    rtv_descriptor_size: 0,
    frame_count: 0,
    rtv_format: DXGI_FORMAT_UNKNOWN,
    imgui_pso_format: DXGI_FORMAT_UNKNOWN,
    bg_draw_list: 0,
    eye_rtv_heap: None,
    eye_rtv_stride: 0,
    eye_rtv_slot: 0,
    frames: Vec::new(),
    draw_data_submitted: false,
});
fn overlay() -> MutexGuard<'static, Overlay> {
    OVERLAY.lock().unwrap_or_else(|e| e.into_inner())
}
/// What the window procedure reads, on the window's thread.
static IMGUI_INITIALIZED: AtomicBool = AtomicBool::new(false);
static MENU_VISIBLE: AtomicBool = AtomicBool::new(false);
static HWND_: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL_WNDPROC: AtomicUsize = AtomicUsize::new(0);
static DESKTOP_WIDTH: AtomicU32 = AtomicU32::new(0);
static DESKTOP_HEIGHT: AtomicU32 = AtomicU32::new(0);
/// How long the full drain comes back for after a resource-churn event.
const LOAD_GUARD_MS: u64 = 5000;
static LOAD_GUARD_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static LOAD_GUARD_ENGAGED: AtomicBool = AtomicBool::new(false);
fn f32_of(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Relaxed))
}

// ---- the pointer: the VR ray or the desktop mouse (Overlay/VrImGuiInput.hpp) -----------------------------------------

fn inside(x: f32, y: f32) -> bool {
    x >= 0.0 && y >= 0.0 && x < CANVAS_WIDTH as f32 && y < CANVAS_HEIGHT as f32
}
/// ImGuiPointerFeed: one source's samples fed to ImGui, the button's edges kept.
#[derive(Default)]
struct PointerFeed {
    down: bool,
}
impl PointerFeed {
    fn release(&mut self, io: &mut Io) {
        if self.down {
            io.add_mouse_pos_event(-f32::MAX, -f32::MAX);
            io.add_mouse_button_event(0, false);
        }
        self.down = false;
    }
    fn submit(&mut self, io: &mut Io, samples: &[PointerEvent], x: f32, y: f32) {
        let mut wheel = 0.0f32;
        for e in samples {
            // Keep click edges and their hit positions, but coalesce continuous controller motion into the latest
            // position for this UI frame.
            if e.down != self.down {
                io.add_mouse_pos_event(
                    if e.x < 0.0 { -f32::MAX } else { e.x },
                    if e.y < 0.0 { -f32::MAX } else { e.y },
                );
                io.add_mouse_button_event(0, e.down);
                self.down = e.down;
            }
            wheel += e.wheel;
        }
        io.add_mouse_pos_event(
            if x < 0.0 { -f32::MAX } else { x },
            if y < 0.0 { -f32::MAX } else { y },
        );
        io.add_mouse_button_event(0, self.down);
        // ImGui trickles queued motion and wheel into separate frames: a continuously moving ray plus queued wheel
        // grows an unbounded backlog. The stick is a continuous axis: its frame delta goes to MouseWheel directly,
        // never more than a quarter notch (a stalled frame's whole scroll history is not replayed).
        if samples.last().is_none_or(|e| e.wheel == 0.0) || self.down || x < 0.0 || y < 0.0 {
            wheel = 0.0;
        }
        io.add_mouse_wheel(if wheel < -0.25 {
            -0.25
        } else if 0.25 < wheel {
            0.25
        } else {
            wheel
        });
    }
}
/// ImGuiPointerRouter: under the pointer mutex. Win32 messages only update this mailbox; the render thread chooses
/// one source before feeding ImGui.
struct PointerRouter {
    feed: PointerFeed,
    mouse: PointerEvent,
    mouse_edges: Vec<PointerEvent>,
    wheel: f32,
    wheel_h: f32,
    mouse_activity: u64,
    vr_seen: bool,
    desktop_claimed: bool,
    mouse_request: bool,
    desktop_owner: bool,
}
const NO_POINTER: PointerEvent = PointerEvent {
    x: -1.0,
    y: -1.0,
    wheel: 0.0,
    down: false,
};
impl PointerRouter {
    const fn new() -> PointerRouter {
        PointerRouter {
            feed: PointerFeed { down: false },
            mouse: NO_POINTER,
            mouse_edges: Vec::new(),
            wheel: 0.0,
            wheel_h: 0.0,
            mouse_activity: 0,
            vr_seen: false,
            desktop_claimed: false,
            mouse_request: false,
            desktop_owner: false,
        }
    }
    fn reset(&mut self, io: &mut Io) {
        self.feed.release(io);
        *self = PointerRouter::new();
    }
    fn mouse_move(&mut self, x: f32, y: f32, now: u64) {
        if x != self.mouse.x || y != self.mouse.y {
            self.mouse_activity = now;
        }
        self.mouse.x = x;
        self.mouse.y = y;
    }
    fn mouse_button(&mut self, x: f32, y: f32, down: bool, now: u64) {
        self.mouse_move(x, y, now);
        if down == self.mouse.down {
            return;
        }
        self.mouse.down = down;
        self.mouse_activity = now;
        if down && inside(x, y) {
            self.mouse_request = true;
        }
        // Keep short desktop clicks just like short controller trigger presses.
        if self.mouse_edges.len() < 64 {
            self.mouse_edges.push(self.mouse);
        } else {
            self.mouse_edges.clear();
            self.mouse.down = false;
            self.mouse_edges.push(NO_POINTER);
            self.mouse_request = false;
        }
    }
    fn mouse_wheel(&mut self, x: f32, y: f32, vertical: f32, horizontal: f32, now: u64) {
        self.mouse_move(x, y, now);
        if !inside(x, y) {
            return;
        }
        self.wheel += vertical;
        self.wheel_h += horizontal;
        self.mouse_activity = now;
        self.mouse_request = true;
    }
    fn mouse_leave(&mut self, now: u64) {
        self.mouse_move(-1.0, -1.0, now);
    }
    fn mouse_focus_lost(&mut self, now: u64) {
        self.mouse_button(-1.0, -1.0, false, now);
        self.mouse_request = false;
        self.desktop_claimed = false;
    }
    fn submit(
        &mut self,
        io: &mut Io,
        samples: &[PointerEvent],
        hit: &Hit,
        dragging: bool,
        now: u64,
    ) {
        self.vr_seen |= hit.valid;
        let mut vr_action = dragging;
        for e in samples {
            vr_action |= e.down || e.wheel != 0.0;
        }
        if vr_action {
            self.desktop_claimed = false;
        } else if self.mouse_request {
            self.desktop_claimed = true;
        } else if self.desktop_claimed
            && !self.mouse.down
            && (!inside(self.mouse.x, self.mouse.y)
                || now.wrapping_sub(self.mouse_activity) >= 1800)
        {
            self.desktop_claimed = false;
        }
        // Once a VR ray has reached this panel, passive / repeated WM_MOUSEMOVE and WM_MOUSELEAVE must not replace it,
        // including during tracking loss. A deliberate desktop click or wheel can still operate the mirror.
        let desktop = self.desktop_claimed || (!self.vr_seen && !vr_action);
        if desktop != self.desktop_owner {
            self.feed.release(io);
            self.desktop_owner = desktop;
        }
        if desktop {
            let edges = core::mem::take(&mut self.mouse_edges);
            self.feed.submit(io, &edges, self.mouse.x, self.mouse.y);
            self.mouse_edges = edges;
            if inside(self.mouse.x, self.mouse.y) && !self.mouse.down {
                io.add_mouse_wheel(self.wheel);
                io.add_mouse_wheel_h(self.wheel_h);
            }
        } else {
            self.feed.submit(
                io,
                samples,
                if hit.valid { hit.x } else { -1.0 },
                if hit.valid { hit.y } else { -1.0 },
            );
        }
        self.mouse_edges.clear();
        self.wheel = 0.0;
        self.wheel_h = 0.0;
        self.mouse_request = false;
    }
}
static POINTER: Mutex<PointerRouter> = Mutex::new(PointerRouter::new());
fn pointer() -> MutexGuard<'static, PointerRouter> {
    POINTER.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- the desktop placement and the draw data's transforms (Overlay/VrInteraction.hpp, VrDraw.hpp, DrawWork.hpp) ----

/// cvr::vrui::DesktopRect.
#[derive(Clone, Copy)]
struct DesktopRect {
    x: f32,
    y: f32,
    scale: f32,
}
/// cvr::vrui::DesktopPlacement: the canvas fitted into w x h at 92%, centred.
fn desktop_placement(w: f32, h: f32) -> DesktopRect {
    let fit = {
        let (a, b) = (w / CANVAS_WIDTH as f32, h / CANVAS_HEIGHT as f32);
        if b < a {
            b
        } else {
            a
        }
    } * 0.92;
    let s = if 0.01 < fit { fit } else { 0.01 };
    DesktopRect {
        x: (w - CANVAS_WIDTH as f32 * s) * 0.5,
        y: (h - CANVAS_HEIGHT as f32 * s) * 0.5,
        scale: s,
    }
}
/// overlay::HasDrawWork: callback-only lists also need submission, even with no vertices.
fn has_draw_work(draw: DrawData) -> bool {
    if draw.is_null() {
        return false;
    }
    // SAFETY: this frame's draw data.
    let d = unsafe { draw.fields() };
    d.valid && d.display_size[0] > 0.0 && d.display_size[1] > 0.0 && d.cmd_lists_count > 0
}
/// Every vertex and clip rectangle scaled by s and moved by (x, y).
fn transform(d: &mut imgui::DrawDataFields, s: f32, x: f32, y: f32) {
    for &list in d.cmd_lists.as_slice() {
        // SAFETY: the frame's draw lists.
        let list = unsafe { &mut *list };
        for v in list.vtx_buffer.as_mut_slice() {
            v.pos = [v.pos[0] * s + x, v.pos[1] * s + y];
        }
        for c in list.cmd_buffer.as_mut_slice() {
            let r = c.clip_rect;
            c.clip_rect = [r[0] * s + x, r[1] * s + y, r[2] * s + x, r[3] * s + y];
        }
    }
}
/// cvr::vrui::DesktopDraw: the canvas's draw data placed on the desktop for one render (this ImGui's DX12 backend
/// ignores FramebufferScale), then put back for XR.
fn with_desktop_draw(draw: DrawData, w: f32, h: f32, rect: DesktopRect, f: impl FnOnce()) {
    // SAFETY: this frame's draw data, edited only here.
    let d = unsafe { draw.fields() };
    let (pos, size, scale) = (d.display_pos, d.display_size, d.framebuffer_scale);
    transform(d, rect.scale, rect.x, rect.y);
    d.display_pos = [0.0, 0.0];
    d.display_size = [w, h];
    d.framebuffer_scale = [1.0, 1.0];
    f();
    // SAFETY: as above.
    let d = unsafe { draw.fields() };
    transform(
        d,
        1.0 / rect.scale,
        -rect.x / rect.scale,
        -rect.y / rect.scale,
    );
    d.display_pos = pos;
    d.display_size = size;
    d.framebuffer_scale = scale;
}

/// A transition barrier on a borrowed resource.
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
                // SAFETY: a borrowed reference for the call that records the barrier; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

// ---- the draw-data dump (A/B diagnostics: CyberpunkVR_ImGuiDump) --------------------------------------------------

fn dump_draw_data(draw: DrawData, name: &str) {
    use std::io::Write;
    if draw.is_null() {
        return;
    }
    // SAFETY: a C string the paths module keeps.
    let dir = unsafe { CStr::from_ptr(cvr_paths_dir_a(5)) }
        .to_bytes()
        .to_vec();
    let mut path = dir;
    path.extend_from_slice(name.as_bytes());
    let mut out: Vec<u8> = Vec::new();
    // SAFETY: this frame's draw data.
    let d = unsafe { draw.fields() };
    let line = |fmt: &CStr, args: &[Arg]| format(fmt, args).into_bytes();
    let _ = out.write_all(&line(
        c"display %.3f %.3f %.3f %.3f lists %d vtx %d idx %d\n",
        &[
            Arg::F(d.display_pos[0] as f64),
            Arg::F(d.display_pos[1] as f64),
            Arg::F(d.display_size[0] as f64),
            Arg::F(d.display_size[1] as f64),
            Arg::I(d.cmd_lists_count),
            Arg::I(d.total_vtx_count),
            Arg::I(d.total_idx_count),
        ],
    ));
    // The default font's glyphs, so a reader can turn the text quads back into text.
    let mut index = 0;
    while let Some((codepoint, u0, v0)) = imgui::font_glyph(index) {
        let _ = out.write_all(&line(
            c"glyph %u %.5f %.5f\n",
            &[
                Arg::U(codepoint as u64),
                Arg::F(u0 as f64),
                Arg::F(v0 as f64),
            ],
        ));
        index += 1;
    }
    for (l, &list) in d.cmd_lists.as_slice().iter().enumerate() {
        // SAFETY: the frame's draw lists.
        let list = unsafe { &*list };
        let owner = list.owner_name();
        let _ = out.write_all(&line(
            c"list %d %s cmds %d vtx %d idx %d\n",
            &[
                Arg::I(l as i32),
                Arg::S(if owner.is_null() {
                    c"-".as_ptr()
                } else {
                    owner
                }),
                Arg::I(list.cmd_buffer.size),
                Arg::I(list.vtx_buffer.size),
                Arg::I(list.idx_buffer.size),
            ],
        ));
        for cmd in list.cmd_buffer.as_slice() {
            let _ = out.write_all(&line(
                c"cmd elem %u vtx %u idx %u clip %.3f %.3f %.3f %.3f tex %llu cb %d\n",
                &[
                    Arg::U(cmd.elem_count as u64),
                    Arg::U(cmd.vtx_offset as u64),
                    Arg::U(cmd.idx_offset as u64),
                    Arg::F(cmd.clip_rect[0] as f64),
                    Arg::F(cmd.clip_rect[1] as f64),
                    Arg::F(cmd.clip_rect[2] as f64),
                    Arg::F(cmd.clip_rect[3] as f64),
                    Arg::U((!cmd.texture_id.is_null()) as u64),
                    Arg::I((!cmd.user_callback.is_null()) as i32),
                ],
            ));
        }
        for v in list.vtx_buffer.as_slice() {
            let _ = out.write_all(&line(
                c"v %.3f %.3f %.5f %.5f %08X\n",
                &[
                    Arg::F(v.pos[0] as f64),
                    Arg::F(v.pos[1] as f64),
                    Arg::F(v.uv[0] as f64),
                    Arg::F(v.uv[1] as f64),
                    Arg::U(v.col as u64),
                ],
            ));
        }
        for tri in list.idx_buffer.as_slice().chunks(3) {
            let mut text = String::from("i");
            for i in tri {
                text.push_str(&format!(" {i}"));
            }
            text.push('\n');
            let _ = out.write_all(text.as_bytes());
        }
    }
    // As fopen("w") wrote it: text mode, CRLF.
    let mut crlf = Vec::with_capacity(out.len() + out.len() / 16);
    for b in out {
        if b == b'\n' {
            crlf.push(b'\r');
        }
        crlf.push(b);
    }
    let _ = std::fs::write(String::from_utf8_lossy(&path).as_ref(), crlf);
}
/// After ImGui::Render: the frame dumped when the tools asked (every N-th frame in a row the menu is drawn, through
/// the VR shell's tabs when CyberpunkVR_ImGuiDumpTabs names some).
fn dump_if_asked(menu_visible: bool) {
    static MENU_FRAMES: AtomicI32 = AtomicI32::new(0);
    static DUMP_TAB: AtomicI32 = AtomicI32::new(-1);
    let menu_frames = if menu_visible {
        MENU_FRAMES.load(Relaxed) + 1
    } else {
        0
    };
    MENU_FRAMES.store(menu_frames, Relaxed);
    let every = g!(CyberpunkVR_ImGuiDump);
    if every <= 0 || menu_frames <= 0 || menu_frames % every != 0 {
        return;
    }
    let tabs = g!(CyberpunkVR_ImGuiDumpTabs);
    if tabs == 0 {
        dump_draw_data(DrawData::get(), "imgui_dump.txt");
        s!(CyberpunkVR_ImGuiDump, 0);
        return;
    }
    let dump_tab = DUMP_TAB.load(Relaxed);
    if dump_tab >= 0 {
        dump_draw_data(DrawData::get(), &format!("imgui_dump_{dump_tab}.txt"));
    }
    let mut next = dump_tab + 1;
    while next < 7 && tabs & (1 << next) == 0 {
        next += 1;
    }
    if next >= 7 {
        s!(CyberpunkVR_ImGuiDump, 0);
        DUMP_TAB.store(-1, Relaxed);
    } else {
        DUMP_TAB.store(next, Relaxed);
        s!(CyberpunkVR_VrShellTab, next);
    }
}

// ---- the overlay's resources and pacing ----------------------------------------------------------------------------

impl Overlay {
    fn event(&self) -> HANDLE {
        HANDLE(self.fence_event as *mut c_void)
    }
    /// A bounded wait on an already-signalled value: on the Present thread a lost fence must degrade into a dropped
    /// overlay frame, not a hung game.
    fn wait_for_fence(&self, target: u64, timeout_ms: u32, reason: &CStr) -> bool {
        let Some(fence) = self.fence.as_ref() else {
            return true;
        };
        if self.fence_event == 0 || target == 0 {
            return true;
        }
        // SAFETY: plain calls on the overlay's fence and event.
        unsafe {
            if fence.GetCompletedValue() >= target {
                return true;
            }
            let diagnostics = diagnostics();
            let mut t0 = 0i64;
            if diagnostics {
                let _ = QueryPerformanceCounter(&mut t0);
            }
            if fence.SetEventOnCompletion(target, self.event()).is_err() {
                return false;
            }
            let r = WaitForSingleObject(self.event(), timeout_ms);
            let (mut t1, mut freq) = (0i64, 0i64);
            if diagnostics {
                let _ = QueryPerformanceCounter(&mut t1);
                let _ = QueryPerformanceFrequency(&mut freq);
            }
            if diagnostics && freq > 0 {
                let ms = (t1 - t0) as f64 * 1000.0 / freq as f64;
                s!(CyberpunkVR_DebugOverlayWaitMs, ms);
                s!(
                    CyberpunkVR_DebugOverlayWaits,
                    g!(CyberpunkVR_DebugOverlayWaits).wrapping_add(1)
                );
                s!(
                    CyberpunkVR_DebugOverlayWaitTotalUs,
                    g!(CyberpunkVR_DebugOverlayWaitTotalUs).wrapping_add((ms * 1000.0) as u64)
                );
            }
            if r != WAIT_OBJECT_0 {
                diag_inc!(CyberpunkVR_DebugOverlayWaitTimeouts);
                static LOGGED: AtomicU64 = AtomicU64::new(0);
                if LOGGED.fetch_add(1, Relaxed) + 1 <= 4 {
                    log(
                        c"Overlay fence wait timed out (%s, %lu ms) -- continuing.\n",
                        &[Arg::S(reason.as_ptr()), Arg::U(timeout_ms as u64)],
                    );
                }
                return false;
            }
        }
        true
    }
    /// The queue drained: a fresh value signalled and waited for (teardown, resize, the guard window).
    fn drain(&mut self, timeout_ms: u32, reason: &CStr) -> bool {
        let (Some(queue), Some(fence)) = (self.queue.as_ref(), self.fence.as_ref()) else {
            return true;
        };
        if self.fence_event == 0 {
            return true;
        }
        self.fence_value += 1;
        let target = self.fence_value;
        // SAFETY: a plain signal on the overlay's queue.
        if unsafe { queue.Signal(fence, target) }.is_err() {
            return false;
        }
        diag_inc!(CyberpunkVR_DebugOverlayDrains);
        self.wait_for_fence(target, timeout_ms, reason)
    }
    fn release_render_targets(&mut self) {
        // Ordinary frames no longer drain, so this is the one place that must: backbuffers, allocators, the command
        // list and ImGui's upload resources are about to be released and the GPU may still be reading them. Bounded.
        if self.queue.is_some()
            && self.fence.is_some()
            && (self.cmd_list.is_some() || !self.frames.is_empty())
        {
            if !self.drain(2000, c"resource teardown") {
                log(
                    c"Overlay teardown drain timed out; continuing (the device may be gone).\n",
                    &[],
                );
            }
        }
        self.frames.clear();
        self.draw_data_submitted = false;
        self.previous_overlay_fence_value = 0;
        self.rtv_heap = None;
        self.cmd_list = None;
        self.swap_chain3 = None;
        self.frame_count = 0;
        self.rtv_format = DXGI_FORMAT_UNKNOWN;
    }
    fn shutdown(&mut self) {
        *pointer() = PointerRouter::new();
        self.release_render_targets();
        if IMGUI_INITIALIZED.load(Acquire) {
            imgui::dx12_shutdown();
            imgui::win32_shutdown();
            imgui::destroy_context();
            IMGUI_INITIALIZED.store(false, Release);
        }
        self.srv_heap = None;
        self.eye_rtv_heap = None;
        self.eye_rtv_stride = 0;
        self.eye_rtv_slot = 0;
        self.imgui_pso_format = DXGI_FORMAT_UNKNOWN;
        self.bg_draw_list = 0;
        self.draw_data_submitted = false;
        self.fence = None;
        if self.fence_event != 0 {
            // SAFETY: the overlay's own event.
            let _ = unsafe { CloseHandle(self.event()) };
            self.fence_event = 0;
        }
    }
    /// The backbuffers' render targets, allocators and the command list, (re)made when the swapchain's buffer count
    /// or format changed.
    fn ensure_swapchain_resources(&mut self, swap_chain: &IDXGISwapChain) -> bool {
        let Some(device) = self.device.clone() else {
            return false;
        };
        if self.queue.is_none() || HWND_.load(Relaxed) == 0 {
            return false;
        }
        // SAFETY: plain calls on the game's swapchain and the overlay's device.
        unsafe {
            let Ok(desc) = swap_chain.GetDesc() else {
                return false;
            };
            if desc.BufferCount == 0 {
                return false;
            }
            let Ok(swap_chain3) = swap_chain.cast::<IDXGISwapChain3>() else {
                return false;
            };
            let needs = self.swap_chain3.is_none()
                || self.frame_count != desc.BufferCount
                || self.rtv_format != desc.BufferDesc.Format;
            if !needs {
                return true;
            }
            self.release_render_targets();
            self.swap_chain3 = Some(swap_chain3);
            self.frame_count = desc.BufferCount;
            self.rtv_format = desc.BufferDesc.Format;
            let rtv_desc = D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: self.frame_count,
                ..Default::default()
            };
            let Ok(heap) = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&rtv_desc) else {
                self.release_render_targets();
                return false;
            };
            self.rtv_descriptor_size =
                device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            let mut rtv = heap.GetCPUDescriptorHandleForHeapStart();
            self.rtv_heap = Some(heap);
            self.frames = (0..self.frame_count)
                .map(|_| FrameContext::default())
                .collect();
            for i in 0..self.frame_count {
                let Ok(allocator) = device.CreateCommandAllocator::<ID3D12CommandAllocator>(
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                ) else {
                    self.release_render_targets();
                    return false;
                };
                self.frames[i as usize].allocator = Some(allocator);
                let Ok(target) = swap_chain.GetBuffer::<ID3D12Resource>(i) else {
                    self.release_render_targets();
                    return false;
                };
                device.CreateRenderTargetView(&target, None, rtv);
                let frame = &mut self.frames[i as usize];
                frame.render_target = Some(target);
                frame.rtv = rtv;
                rtv.ptr += self.rtv_descriptor_size as usize;
            }
            let first = self.frames[0].allocator.clone().expect("made above");
            let Ok(list) = device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &first,
                None::<&ID3D12PipelineState>,
            ) else {
                self.release_render_targets();
                return false;
            };
            let name: Vec<u16> = "CVR_imgui_command_list\0".encode_utf16().collect();
            let _ = list.SetName(PCWSTR(name.as_ptr()));
            let _ = list.Close();
            self.cmd_list = Some(list);
        }
        true
    }
    /// The context, its fonts and style, the backends, the fence -- once; then the swapchain's resources.
    fn ensure_imgui(&mut self, swap_chain: &IDXGISwapChain) -> bool {
        let Some(device) = self.device.clone() else {
            return false;
        };
        let hwnd = HWND_.load(Relaxed);
        if self.queue.is_none() || hwnd == 0 {
            return false;
        }
        // SAFETY: plain calls on the game's swapchain and the overlay's device.
        unsafe {
            let Ok(desc) = swap_chain.GetDesc() else {
                return false;
            };
            if !IMGUI_INITIALIZED.load(Acquire) {
                let srv_desc = D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                    NumDescriptors: 1,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                    ..Default::default()
                };
                let Ok(srv_heap) = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&srv_desc)
                else {
                    return false;
                };
                self.srv_heap = Some(srv_heap.clone());

                imgui::create_context();
                let mut io = Io::get();
                io.add_config_flags(imgui::CONFIG_NAV_ENABLE_KEYBOARD);
                io.set_mouse_draw_cursor(true);
                io.clear_ini_filename();
                let mut windows = [0u8; 260];
                let n = GetWindowsDirectoryA(Some(&mut windows)) as usize;
                let mut font = windows[..n.min(windows.len())].to_vec();
                font.extend_from_slice(b"\\Fonts\\segoeui.ttf");
                let font = std::ffi::CString::new(font).unwrap_or_default();
                if !imgui::add_font_file_cyrillic(&font, 28.0) {
                    imgui::add_font_default(28.0);
                }
                (rt().load_cyberpunk_font)();
                crate::vr_style::cvr_apply_vr_style();

                if !imgui::win32_init(hwnd as *mut c_void) {
                    self.shutdown();
                    return false;
                }
                // TWICE the frames in flight: the overlay is rendered twice per frame (the backbuffer and the second
                // eye) and the backend takes one buffer set from its ring per RenderDrawData call.
                let frames = 2 * desc.BufferCount.max(2) as i32;
                if !imgui::dx12_init(
                    device.as_raw(),
                    frames,
                    desc.BufferDesc.Format.0,
                    srv_heap.as_raw(),
                    srv_heap.GetCPUDescriptorHandleForHeapStart().ptr as u64,
                    srv_heap.GetGPUDescriptorHandleForHeapStart().ptr,
                ) {
                    self.shutdown();
                    return false;
                }
                match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
                    Ok(f) => self.fence = Some(f),
                    Err(_) => {
                        self.shutdown();
                        return false;
                    }
                }
                match CreateEventA(None, false, false, None) {
                    Ok(e) if !e.is_invalid() => self.fence_event = e.0 as isize,
                    _ => {
                        self.shutdown();
                        return false;
                    }
                }
                self.imgui_pso_format = desc.BufferDesc.Format;
                IMGUI_INITIALIZED.store(true, Release);
                if verbose() {
                    log(
                        c"ImGui overlay initialized. Toggle with F10 or Insert.\n",
                        &[],
                    );
                }
            }
        }
        self.ensure_swapchain_resources(swap_chain)
    }
}

/// Whether this submission must be followed by a full drain: always in mode 0, in mode 3 only while the churn window
/// is open. Logs the two edges rather than once a frame.
fn should_drain_this_frame() -> bool {
    let mode = g!(CyberpunkVR_OverlayPacing);
    if mode == 0 {
        return true;
    }
    if mode != 3 {
        return false;
    }
    let active = tick() < LOAD_GUARD_UNTIL_MS.load(Relaxed);
    if active != LOAD_GUARD_ENGAGED.swap(active, Relaxed) {
        log(
            c"Overlay load guard %s.\n",
            &[Arg::S(
                if active {
                    c"engaged -- full drain"
                } else {
                    c"released -- back to previous-overlay pacing"
                }
                .as_ptr(),
            )],
        );
    }
    active
}

/// overlay::ReleaseGameMouseCapture: the cursor freed and shown while the menu or the Lua windows have it.
pub fn release_game_mouse_capture() {
    // SAFETY: plain Win32 calls.
    unsafe {
        let _ = ClipCursor(None);
        let _ = ReleaseCapture();
        let mut info = CURSORINFO {
            cbSize: core::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if GetCursorInfo(&mut info).is_ok() && (info.flags.0 & CURSOR_SHOWING.0) == 0 {
            while ShowCursor(true) < 0 {}
        }
    }
}

/// The physical client rect (GetClientRect is hooked to report the virtual size) and the desktop crop's transform.
fn physical_client(hwnd: isize) -> Option<RECT> {
    let (mut rect, mut origin) = (RECT::default(), POINT::default());
    // SAFETY: the window and two slots to fill.
    unsafe {
        cvr_gpu::desktop_mirror::cvr_desktop_physical_client_rect(
            hwnd as *mut c_void,
            &mut rect,
            &mut origin,
        )
    }
    .then_some(rect)
}
fn cover(sw: u32, sh: u32, dw: u32, dh: u32) -> Option<cvr_gpu::desktop_mirror::CoverTransform> {
    let mut out = cvr_gpu::desktop_mirror::CoverTransform::default();
    // SAFETY: a slot to fill.
    unsafe { cvr_gpu::desktop_mirror::cvr_desktop_cover_transform(sw, sh, dw, dh, &mut out) }
        .then_some(out)
}

// ---- the entry points (include/Overlay/ImGuiOverlay.hpp) -----------------------------------------------------------

/// OverlaySetDeviceAndQueue: a new device or queue tears the overlay down first.
///
/// # Safety
/// `device` and `queue` are an ID3D12Device and an ID3D12CommandQueue, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_overlay_set_device_and_queue(device: *mut c_void, queue: *mut c_void) {
    let mut o = overlay();
    let same_device = o
        .device
        .as_ref()
        .map(|d| d.as_raw())
        .unwrap_or(core::ptr::null_mut())
        == device;
    let same_queue = o
        .queue
        .as_ref()
        .map(|q| q.as_raw())
        .unwrap_or(core::ptr::null_mut())
        == queue;
    if same_device && same_queue {
        return;
    }
    o.shutdown();
    // SAFETY: the caller's objects, referenced for as long as the overlay keeps them.
    unsafe {
        o.device = ID3D12Device::from_raw_borrowed(&device).cloned();
        o.queue = ID3D12CommandQueue::from_raw_borrowed(&queue).cloned();
    }
}

/// OverlaySetWindow: the game's window subclassed (the previous one given its procedure back).
#[no_mangle]
pub extern "C" fn cvr_overlay_set_window(hwnd: *mut c_void) {
    let previous = HWND_.load(Relaxed);
    if verbose() {
        log(
            c"OverlaySetWindow called: hwnd=%p (previous g_hwnd=%p)\n",
            &[Arg::U(hwnd as u64), Arg::U(previous as u64)],
        );
    }
    if hwnd.is_null() || hwnd as isize == previous {
        return;
    }
    let original = ORIGINAL_WNDPROC.load(Relaxed);
    // SAFETY: plain Win32 calls on the game's window.
    unsafe {
        if previous != 0 && original != 0 {
            SetWindowLongPtrA(
                HWND(previous as *mut c_void),
                GWLP_WNDPROC,
                original as isize,
            );
            if verbose() {
                log(
                    c"OverlaySetWindow: Restored original WndProc on old hwnd %p\n",
                    &[Arg::U(previous as u64)],
                );
            }
        }
        HWND_.store(hwnd as isize, Relaxed);
        let original = SetWindowLongPtrA(
            HWND(hwnd),
            GWLP_WNDPROC,
            overlay_wnd_proc as *const () as usize as isize,
        );
        ORIGINAL_WNDPROC.store(original as usize, Relaxed);
        if verbose() {
            log(
                c"OverlaySetWindow: Subclassed hwnd %p, original WndProc=%p, new WndProc=%p\n",
                &[
                    Arg::U(hwnd as u64),
                    Arg::U(original as u64),
                    Arg::U(overlay_wnd_proc as *const () as usize as u64),
                ],
            );
        }
    }
}

/// OverlayRender: the overlay's frame on the game's Present.
///
/// # Safety
/// `swap_chain` is the game's IDXGISwapChain.
#[no_mangle]
pub unsafe extern "C" fn cvr_overlay_render(swap_chain: *mut c_void) {
    // SAFETY: the caller's swapchain.
    let Some(swap_chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&swap_chain) }) else {
        return;
    };
    let mut o = overlay();
    o.draw_data_submitted = false;
    if !o.ensure_imgui(swap_chain) {
        return;
    }
    // The VR menu's settings page: recorded here, on the panel's own thread (Overlay/UiRec.hpp).
    crate::ui_rec::cvr_uirec_run_pending_pass();
    // SAFETY: plain calls on the game's swapchain and the overlay's objects.
    unsafe {
        let Ok(desc) = swap_chain.GetDesc() else {
            return;
        };
        let frame_index = o
            .swap_chain3
            .as_ref()
            .map(|s| s.GetCurrentBackBufferIndex())
            .unwrap_or(0) as usize;
        if frame_index >= o.frames.len() {
            return;
        }
        let (Some(render_target), Some(allocator), Some(cmd_list)) = (
            o.frames[frame_index].render_target.clone(),
            o.frames[frame_index].allocator.clone(),
            o.cmd_list.clone(),
        ) else {
            return;
        };
        let (Some(device), Some(queue)) = (o.device.clone(), o.queue.clone()) else {
            return;
        };
        let menu_visible = vrui::cvr_vrui_visible();
        MENU_VISIBLE.store(menu_visible, Relaxed);
        let mut vr_canvas: Option<Arc<Canvas>> = None;
        if menu_visible {
            vr_canvas = vr_panel::acquire_canvas(&device, o.imgui_pso_format);
            if vr_canvas.is_none() {
                return;
            }
        }

        imgui::dx12_new_frame();
        imgui::win32_new_frame();

        let mut io = Io::get();
        let mut backbuffer_width = desc.BufferDesc.Width as f32;
        let mut backbuffer_height = desc.BufferDesc.Height as f32;
        if backbuffer_width <= 1.0 || backbuffer_height <= 1.0 {
            let rd = render_target.GetDesc();
            backbuffer_width = rd.Width as f32;
            backbuffer_height = rd.Height as f32;
        }
        if backbuffer_width > 1.0 && backbuffer_height > 1.0 {
            DESKTOP_WIDTH.store(backbuffer_width.to_bits(), Relaxed);
            DESKTOP_HEIGHT.store(backbuffer_height.to_bits(), Relaxed);
            io.set_display_size([backbuffer_width, backbuffer_height]);
            io.set_display_framebuffer_scale([1.0, 1.0]);
        }
        if menu_visible {
            release_game_mouse_capture();
            io.set_display_size([CANVAS_WIDTH as f32, CANVAS_HEIGHT as f32]);
            let mut settings = vrui::Settings::default();
            vrui::cvr_vrui_get_settings(&mut settings);
            io.set_font_global_scale(settings.font_scale);
            let mut events = [PointerEvent {
                x: -1.0,
                y: -1.0,
                wheel: 0.0,
                down: false,
            }; 80];
            let n = vrui::cvr_vrui_consume_pointer_events(events.as_mut_ptr(), 80);
            let mut view: vrui::View = core::mem::zeroed();
            vrui::cvr_vrui_get_view(&mut view);
            pointer().submit(
                &mut io,
                &events[..n.min(80)],
                &view.hit,
                view.dragging,
                tick(),
            );
        } else {
            if !lua_overlay_open() {
                pointer().reset(&mut io);
            }
            io.set_font_global_scale(1.0);
            vr_panel::clear_canvas();
        }
        // The Lua mods' windows on the desktop: ImGui draws the pointer the game hides.
        let lua_overlay = !menu_visible && lua_overlay_open();
        if lua_overlay {
            release_game_mouse_capture();
        }
        io.set_mouse_draw_cursor(lua_overlay);
        if lua_overlay {
            // The frame is two eyes wide and the mirror shrinks it into the window: about 18 px of text on the monitor.
            if let Some(client) = physical_client(HWND_.load(Relaxed)) {
                if client.right > client.left {
                    let scale =
                        io.display_size()[0] / (client.right - client.left) as f32 * 18.0 / 28.0;
                    io.set_font_global_scale(if 1.0 < scale { scale } else { 1.0 });
                }
            }
        }

        imgui::new_frame();

        if !menu_visible {
            (rt().debug_draws)();
            let size = io.display_size();
            (rt().lua_draw)(size[0], size[1]);
        }

        let placement_save = vrui::cvr_vrui_consume_placement_save();
        let mut foveated_editing = false;
        if menu_visible || placement_save {
            let mut state: LiveControlsUiState = core::mem::zeroed();
            GetLiveControlsUiState(&mut state);
            let changed = menu_visible
                && crate::vr_style::cvr_draw_vr_shell(&mut state, &mut foveated_editing);
            crate::foveated::cvr_foveated_set_ui_editing(foveated_editing);
            if changed || placement_save {
                crate::live_controls_ui::SetLiveControlsUiState(&state, 1);
            }
        } else {
            crate::foveated::cvr_foveated_set_ui_editing(false);
        }

        // Captured BEFORE Render(), so the second-eye pass never calls an ImGui function after the frame is closed.
        o.bg_draw_list = imgui::background_draw_list() as usize;

        imgui::render();
        dump_if_asked(menu_visible);

        let drain = should_drain_this_frame();
        let draw = DrawData::get();
        if g!(CyberpunkVR_OverlaySkipEmpty) != 0 && vr_canvas.is_none() && !has_draw_work(draw) {
            diag_inc!(CyberpunkVR_DebugOverlayEmptyFrames);
            // An empty CPU frame still clears stale draw data; the resource-churn guard holds during loads / resizes.
            if drain {
                o.drain(2000, c"load guard (empty overlay)");
            }
            return;
        }

        // The DX12 backend writes its upload ring in RenderDrawData: wait before that, and before resetting the
        // allocator, only when there is work to submit.
        let owned = o.frames[frame_index]
            .fence_value
            .max(o.previous_overlay_fence_value);
        if !o.wait_for_fence(owned, 100, c"overlay frame ownership") {
            return;
        }
        if allocator.Reset().is_err()
            || cmd_list
                .Reset(&allocator, None::<&ID3D12PipelineState>)
                .is_err()
        {
            return;
        }
        let (resource, before) = match vr_canvas.as_ref() {
            Some(c) => (
                c.texture.clone(),
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
            ),
            None => (render_target.clone(), D3D12_RESOURCE_STATE_PRESENT),
        };
        cmd_list.ResourceBarrier(&[transition(
            &resource,
            before,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        )]);
        let draw_rtv = match vr_canvas.as_ref() {
            Some(c) => c.rtv.GetCPUDescriptorHandleForHeapStart(),
            None => o.frames[frame_index].rtv,
        };
        if vr_canvas.is_some() {
            cmd_list.ClearRenderTargetView(draw_rtv, &[0.0, 0.0, 0.0, 0.0], None);
        }
        cmd_list.OMSetRenderTargets(1, Some(&draw_rtv), false, None);
        cmd_list.SetDescriptorHeaps(&[o.srv_heap.clone()]);
        imgui::dx12_render(draw, cmd_list.as_raw());
        cmd_list.ResourceBarrier(&[transition(
            &resource,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            before,
        )]);
        if cmd_list.Close().is_err() {
            return;
        }
        queue.ExecuteCommandLists(&[Some(
            cmd_list
                .cast::<ID3D12CommandList>()
                .expect("a command list"),
        )]);

        // SIGNAL, DO NOT WAIT: the value is remembered on this backbuffer slot (whose allocator is reused) and as the
        // previous submission, which the next frame waits for before it records anything.
        let Some(fence) = o.fence.clone() else { return };
        o.fence_value += 1;
        let submitted = o.fence_value;
        if queue.Signal(&fence, submitted).is_ok() {
            o.draw_data_submitted = true;
            diag_inc!(CyberpunkVR_DebugOverlaySubmissions);
            if let Some(c) = vr_canvas.as_ref() {
                vr_panel::publish_canvas(c, &fence, submitted);
            }
            o.frames[frame_index].fence_value = submitted;
            o.previous_overlay_fence_value = submitted;
            // The full drain, only while the churn guard is open (or in mode 0).
            if drain {
                o.wait_for_fence(submitted, 2000, c"load guard");
            }
        }
    }
}

/// OverlayRenderDesktop: the settings mirror on the desktop, after XR has captured the clean backbuffer -- the
/// canvas's draw data placed inside the part of the backbuffer the desktop crop keeps.
///
/// # Safety
/// `swap_chain` is the game's IDXGISwapChain.
#[no_mangle]
pub unsafe extern "C" fn cvr_overlay_render_desktop(_swap_chain: *mut c_void) {
    let mut o = overlay();
    if !o.draw_data_submitted
        || !vrui::cvr_vrui_visible()
        || !MENU_VISIBLE.load(Relaxed)
        || !IMGUI_INITIALIZED.load(Acquire)
    {
        return;
    }
    let (Some(swap_chain3), Some(cmd_list), Some(device), Some(queue)) = (
        o.swap_chain3.clone(),
        o.cmd_list.clone(),
        o.device.clone(),
        o.queue.clone(),
    ) else {
        return;
    };
    // SAFETY: plain calls on the overlay's objects.
    unsafe {
        let index = swap_chain3.GetCurrentBackBufferIndex() as usize;
        if index >= o.frames.len() {
            return;
        }
        let Some(render_target) = o.frames[index].render_target.clone() else {
            return;
        };
        if o.fence
            .as_ref()
            .is_some_and(|f| f.GetCompletedValue() < o.frames[index].desktop_fence)
        {
            return;
        }
        if o.frames[index].desktop_allocator.is_none() {
            match device
                .CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
            {
                Ok(a) => o.frames[index].desktop_allocator = Some(a),
                Err(_) => return,
            }
        }
        let draw = DrawData::get();
        if draw.is_null() || !draw.fields().valid {
            return;
        }
        let rd = render_target.GetDesc();
        let (w, h) = (rd.Width as f32, rd.Height as f32);
        let allocator = o.frames[index]
            .desktop_allocator
            .clone()
            .expect("made above");
        if allocator.Reset().is_err()
            || cmd_list
                .Reset(&allocator, None::<&ID3D12PipelineState>)
                .is_err()
        {
            return;
        }
        cmd_list.ResourceBarrier(&[transition(
            &render_target,
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        )]);
        let rtv = o.frames[index].rtv;
        cmd_list.OMSetRenderTargets(1, Some(&rtv), false, None);
        cmd_list.SetDescriptorHeaps(&[o.srv_heap.clone()]);
        // Placed inside the part of the backbuffer the desktop crop keeps (the mirror renders after this), so the
        // whole panel stays on screen and the pointer mapping in the window procedure matches it.
        let mut placement = desktop_placement(w, h);
        if let Some(client) = physical_client(HWND_.load(Relaxed)) {
            let cw = (client.right - client.left).max(0) as u32;
            let ch = (client.bottom - client.top).max(0) as u32;
            if let Some(c) = cover(w as u32, h as u32, cw, ch) {
                placement = desktop_placement(c.source_width as f32, c.source_height as f32);
                placement.x += c.source_x as f32;
                placement.y += c.source_y as f32;
            }
        }
        with_desktop_draw(draw, w, h, placement, || {
            imgui::dx12_render(draw, cmd_list.as_raw())
        });
        cmd_list.ResourceBarrier(&[transition(
            &render_target,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_PRESENT,
        )]);
        if cmd_list.Close().is_err() {
            return;
        }
        queue.ExecuteCommandLists(&[Some(
            cmd_list
                .cast::<ID3D12CommandList>()
                .expect("a command list"),
        )]);
        let Some(fence) = o.fence.clone() else { return };
        o.fence_value += 1;
        let value = o.fence_value;
        if queue.Signal(&fence, value).is_ok() {
            o.frames[index].desktop_fence = value;
            o.previous_overlay_fence_value = value;
        }
    }
}

/// OverlayArmLoadGuard: the full-drain window opened for a few seconds. Counted, not logged (the window's own edges
/// are); a counter climbing while "released" never appears means the guard is stuck open.
#[no_mangle]
pub extern "C" fn cvr_overlay_arm_load_guard(_reason: *const c_char) {
    diag_inc!(CyberpunkVR_DebugOverlayGuardArms);
    LOAD_GUARD_UNTIL_MS.store(tick() + LOAD_GUARD_MS, Relaxed);
}
/// OverlayInvalidateSwapchainResources: the backbuffers are going away; the guard covers the frames after the rebuild.
#[no_mangle]
pub extern "C" fn cvr_overlay_invalidate_swapchain_resources() {
    cvr_overlay_arm_load_guard(c"swapchain invalidate".as_ptr());
    overlay().release_render_targets();
}

/// Same family = same typeless parent, which is all a render target view cares about; an unknown pairing is refused.
fn rtv_format_fits(resource: DXGI_FORMAT, view: DXGI_FORMAT) -> bool {
    let family = |f: DXGI_FORMAT| match f {
        DXGI_FORMAT_R8G8B8A8_TYPELESS
        | DXGI_FORMAT_R8G8B8A8_UNORM
        | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => 1,
        DXGI_FORMAT_B8G8R8A8_TYPELESS
        | DXGI_FORMAT_B8G8R8A8_UNORM
        | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => 2,
        DXGI_FORMAT_R10G10B10A2_TYPELESS | DXGI_FORMAT_R10G10B10A2_UNORM => 3,
        DXGI_FORMAT_R16G16B16A16_TYPELESS | DXGI_FORMAT_R16G16B16A16_FLOAT => 4,
        _ => 0,
    };
    let (a, b) = (family(resource), family(view));
    a != 0 && a == b
}

/// OverlayRecordIntoTarget: this frame's draw data recorded a second time into the second eye (the background list --
/// the world-projected markers, true for one eye only -- left out; slid by `shift_px` for a panel's distance).
/// Settings menus never: they are a both-eyes XR quad.
///
/// # Safety
/// `cmd_list` is a recording ID3D12GraphicsCommandList, `target` an ID3D12Resource in RENDER_TARGET.
#[no_mangle]
pub unsafe extern "C" fn cvr_overlay_record_into_target(
    cmd_list: *mut c_void,
    target: *mut c_void,
    shift_px: f32,
) -> bool {
    let mut o = overlay();
    if !o.draw_data_submitted || MENU_VISIBLE.load(Relaxed) || vrui::cvr_vrui_visible() {
        return false;
    }
    if g!(CyberpunkVR_OverlaySecondEye) == 0 {
        return false;
    }
    // SAFETY: the caller's objects.
    let (Some(list), Some(target)) = (
        unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&cmd_list) },
        unsafe { ID3D12Resource::from_raw_borrowed(&target) },
    ) else {
        return false;
    };
    let Some(device) = o.device.clone() else {
        return false;
    };
    if o.srv_heap.is_none()
        || !IMGUI_INITIALIZED.load(Acquire)
        || o.imgui_pso_format == DXGI_FORMAT_UNKNOWN
    {
        return false;
    }
    let draw = DrawData::get();
    if draw.is_null() {
        return false;
    }
    // SAFETY: this frame's draw data.
    let d = unsafe { draw.fields() };
    if !d.valid || d.cmd_lists_count <= 0 {
        return false;
    }
    // SAFETY: plain calls on the caller's resource and the overlay's device.
    unsafe {
        let desc = target.GetDesc();
        let want_w = d.display_size[0] as u32;
        let want_h = d.display_size[1] as u32;
        // REFUSED, NOT STRETCHED: the draw data carries no scale.
        if desc.Width as u32 != want_w
            || desc.Height != want_h
            || !rtv_format_fits(desc.Format, o.imgui_pso_format)
        {
            static TOLD: AtomicBool = AtomicBool::new(false);
            if !TOLD.swap(true, Relaxed) {
                log(
                    c"Overlay[eye]: refusing the second-eye pass -- target %ux%u fmt=%d against draw data %ux%u pso fmt=%d. Same size and same format family are required.\n",
                    &[
                        Arg::U(desc.Width as u32 as u64),
                        Arg::U(desc.Height as u64),
                        Arg::I(desc.Format.0),
                        Arg::U(want_w as u64),
                        Arg::U(want_h as u64),
                        Arg::I(o.imgui_pso_format.0),
                    ],
                );
            }
            diag_inc!(CyberpunkVR_DebugOverlaySecondEyeSkips);
            return false;
        }
        if o.eye_rtv_heap.is_none() {
            let hd = D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 4,
                ..Default::default()
            };
            match device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&hd) {
                Ok(h) => o.eye_rtv_heap = Some(h),
                Err(_) => {
                    diag_inc!(CyberpunkVR_DebugOverlaySecondEyeSkips);
                    return false;
                }
            }
            o.eye_rtv_stride =
                device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        }
        let mut rtv = o
            .eye_rtv_heap
            .as_ref()
            .expect("made above")
            .GetCPUDescriptorHandleForHeapStart();
        rtv.ptr += o.eye_rtv_slot as usize * o.eye_rtv_stride as usize;
        o.eye_rtv_slot = (o.eye_rtv_slot + 1) % 4;
        let rtv_desc = D3D12_RENDER_TARGET_VIEW_DESC {
            Format: o.imgui_pso_format,
            ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        device.CreateRenderTargetView(target, Some(&rtv_desc), rtv);

        // THE TWO EDITS TO THE DRAW DATA, both undone before returning: DisplayPos slides the panel (a distance), and
        // the background list is dropped for this pass (erased and put back in place: no allocation).
        let saved_pos = d.display_pos;
        d.display_pos[0] += shift_px;
        let bg = o.bg_draw_list as *mut imgui::DrawListData;
        let mut bg_index = None;
        if !bg.is_null() {
            bg_index = d.cmd_lists.as_slice().iter().position(|&l| l == bg);
        }
        if let Some(i) = bg_index {
            let data = d.cmd_lists.data;
            let n = d.cmd_lists.size as usize;
            core::ptr::copy(data.add(i + 1), data.add(i), n - i - 1);
            d.cmd_lists.size -= 1;
            d.cmd_lists_count = d.cmd_lists.size;
        }
        let anything = d.cmd_lists_count > 0;
        if anything {
            list.OMSetRenderTargets(1, Some(&rtv), false, None);
            list.SetDescriptorHeaps(&[o.srv_heap.clone()]);
            imgui::dx12_render(draw, list.as_raw());
        }
        if let Some(i) = bg_index {
            let data = d.cmd_lists.data;
            let n = d.cmd_lists.size as usize;
            core::ptr::copy(data.add(i), data.add(i + 1), n - i);
            *data.add(i) = bg;
            d.cmd_lists.size += 1;
            d.cmd_lists_count = d.cmd_lists.size;
        }
        d.display_pos = saved_pos;
        if anything {
            diag_inc!(CyberpunkVR_DebugOverlaySecondEyeDraws);
        }
        anything
    }
}

// ---- the window procedure ------------------------------------------------------------------------------------------

/// WM_MOUSELEAVE and WM_NCMOUSELEAVE (winuser.h; not in the windows crate's WindowsAndMessaging).
const WM_MOUSELEAVE_: u32 = 0x02A3;
const WM_NCMOUSELEAVE_: u32 = 0x02A2;
fn is_blocked_input_message(msg: u32) -> bool {
    matches!(
        msg,
        WM_INPUT
            | WM_INPUT_DEVICE_CHANGE
            | WM_MOUSEMOVE
            | WM_NCMOUSEMOVE
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_MBUTTONDOWN
            | WM_MBUTTONUP
            | WM_MOUSEWHEEL
            | WM_MOUSEHWHEEL
            | WM_KEYDOWN
            | WM_KEYUP
            | WM_SYSKEYDOWN
            | WM_SYSKEYUP
            | WM_CHAR
    )
}
fn loword(l: isize) -> i16 {
    (l as usize & 0xFFFF) as u16 as i16
}
fn hiword(l: isize) -> i16 {
    ((l as usize >> 16) & 0xFFFF) as u16 as i16
}

/// OverlayWndProc: the desktop mouse mapped through the mirror's crop for the game and ImGui, the Lua mods' input,
/// F10 / Insert toggling the VR menu, Esc closing it, and while it is open the mouse routed to its pointer and the
/// game's input held back.
unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let mut lparam = lparam.0;
    let client_mouse_x = loword(lparam) as f32;
    let client_mouse_y = hiword(lparam) as f32;
    let hwnd_raw = hwnd.0;
    static TOTAL_MSG_COUNT: AtomicI32 = AtomicI32::new(0);
    if verbose() {
        let count = TOTAL_MSG_COUNT.fetch_add(1, Relaxed);
        if count % 5000 == 0 {
            log(
                c"OverlayWndProc: msg=%u, hwnd=%p, count=%d\n",
                &[
                    Arg::U(msg as u64),
                    Arg::U(hwnd_raw as u64),
                    Arg::I(count + 1),
                ],
            );
        }
    }

    // 1. Mouse coordinates scaled first, through the desktop mirror's centre crop from the PHYSICAL client, so ImGui
    // and the game receive the scaled input.
    if matches!(
        msg,
        WM_MOUSEMOVE
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_MBUTTONDOWN
            | WM_MBUTTONUP
    ) {
        let virtual_width = crate::vr_core_settings::GetForcedDisplayModeWidth();
        let virtual_height = crate::vr_core_settings::GetForcedDisplayModeHeight();
        if virtual_width > 0 && virtual_height > 0 {
            if let Some(rect) = physical_client(hwnd_raw as isize) {
                let win_width = rect.right - rect.left;
                let win_height = rect.bottom - rect.top;
                let mut x = loword(lparam) as i32;
                let mut y = hiword(lparam) as i32;
                let (old_x, old_y) = (x, y);
                if win_width > 0 && win_height > 0 {
                    if let Some(c) = cover(
                        virtual_width,
                        virtual_height,
                        win_width as u32,
                        win_height as u32,
                    ) {
                        x = (c.source_x + x as f64 * c.source_width / win_width as f64).round()
                            as i32;
                        y = (c.source_y + y as f64 * c.source_height / win_height as f64).round()
                            as i32;
                        lparam = ((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize;
                    }
                }
                static MOUSE_LOG_COUNT: AtomicI32 = AtomicI32::new(0);
                if verbose() && MOUSE_LOG_COUNT.fetch_add(1, Relaxed) % 100 == 0 {
                    log(
                        c"OverlayWndProc (scaled): msg=%u physical=(%d,%d) -> scaled=(%d,%d) win=%dx%d virt=%ux%u g_menuVisible=%d\n",
                        &[
                            Arg::U(msg as u64),
                            Arg::I(old_x),
                            Arg::I(old_y),
                            Arg::I(x),
                            Arg::I(y),
                            Arg::I(win_width),
                            Arg::I(win_height),
                            Arg::U(virtual_width as u64),
                            Arg::U(virtual_height as u64),
                            Arg::I(MENU_VISIBLE.load(Relaxed) as i32),
                        ],
                    );
                }
            }
        }
    }
    let wp = wparam.0;

    // The Lua mods' key binds and their overlay key (raw input); while their windows are open ImGui has the input --
    // the game reads the mouse and keys from raw input, and would turn, walk and fire behind them.
    (rt().lua_on_wndproc)(hwnd_raw, msg, wp, lparam);
    if lua_overlay_open() && !vrui::cvr_vrui_visible() {
        if IMGUI_INITIALIZED.load(Acquire) && imgui::win32_wndproc(hwnd_raw, msg, wp, lparam) != 0 {
            return LRESULT(1);
        }
        let blocked = if msg == WM_INPUT {
            !(rt().lua_pass_raw_input)(lparam)
        } else {
            is_blocked_input_message(msg)
        };
        if blocked {
            return LRESULT(0);
        }
    }

    // 2. The menu's toggle.
    let key_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    if key_up && (wp == VK_F10.0 as usize || wp == VK_INSERT.0 as usize) {
        vrui::cvr_vrui_toggle();
        let visible = vrui::cvr_vrui_visible();
        MENU_VISIBLE.store(visible, Relaxed);
        if visible {
            release_game_mouse_capture();
        }
        if verbose() {
            log(
                c"ImGui overlay %s.\n",
                &[Arg::S(if visible { c"shown" } else { c"hidden" }.as_ptr())],
            );
        }
        return LRESULT(0);
    }

    // 3. Fed to the menu while it is open.
    if key_up && wp == VK_ESCAPE.0 as usize && vrui::cvr_vrui_visible() {
        vrui::cvr_vrui_close(true);
        return LRESULT(0);
    }
    if vrui::cvr_vrui_visible() {
        if matches!(
            msg,
            WM_MOUSEMOVE
                | WM_LBUTTONDOWN
                | WM_LBUTTONDBLCLK
                | WM_LBUTTONUP
                | WM_MOUSEWHEEL
                | WM_MOUSEHWHEEL
        ) {
            let width = f32_of(&DESKTOP_WIDTH);
            let height = f32_of(&DESKTOP_HEIGHT);
            let rect = desktop_placement(width, height);
            // The physical client, and the panel as it is placed inside the desktop crop (cvr_overlay_render_desktop).
            let client = physical_client(hwnd_raw as isize).unwrap_or_else(|| {
                let mut c = RECT::default();
                let _ = GetClientRect(hwnd, &mut c);
                c
            });
            let mut position = POINT {
                x: client_mouse_x as i32,
                y: client_mouse_y as i32,
            };
            if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut position);
            }
            let client_w = (client.right - client.left).max(1);
            let client_h = (client.bottom - client.top).max(1);
            let cropped = cover(
                width as u32,
                height as u32,
                client_w as u32,
                client_h as u32,
            );
            let placement = match cropped {
                Some(c) => desktop_placement(c.source_width as f32, c.source_height as f32),
                None => rect,
            };
            let span_w = match cropped {
                Some(c) => c.source_width as f32,
                None => width,
            };
            let span_h = match cropped {
                Some(c) => c.source_height as f32,
                None => height,
            };
            let x = (position.x as f32 * span_w / client_w as f32 - placement.x) / placement.scale;
            let y = (position.y as f32 * span_h / client_h as f32 - placement.y) / placement.scale;
            let now = tick();
            let mut router = pointer();
            if matches!(msg, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP) {
                router.mouse_button(x, y, msg != WM_LBUTTONUP, now);
            } else if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
                let delta = ((wp >> 16) & 0xFFFF) as u16 as i16 as f32 / WHEEL_DELTA as f32;
                router.mouse_wheel(
                    x,
                    y,
                    if msg == WM_MOUSEWHEEL { delta } else { 0.0 },
                    if msg == WM_MOUSEHWHEEL { -delta } else { 0.0 },
                    now,
                );
            } else {
                router.mouse_move(x, y, now);
            }
            return LRESULT(0);
        }
        if msg == WM_MOUSELEAVE_ || msg == WM_NCMOUSELEAVE_ || msg == WM_NCMOUSEMOVE {
            pointer().mouse_leave(tick());
            return LRESULT(0);
        }
        if msg == WM_KILLFOCUS {
            pointer().mouse_focus_lost(tick());
        }
        if IMGUI_INITIALIZED.load(Acquire) && imgui::win32_wndproc(hwnd_raw, msg, wp, lparam) != 0 {
            return LRESULT(1);
        }
        if is_blocked_input_message(msg) {
            return LRESULT(0);
        }
    }

    let original = ORIGINAL_WNDPROC.load(Relaxed);
    if original != 0 {
        // SAFETY: the window's own procedure, as it was before the subclass.
        let proc: WNDPROC = core::mem::transmute::<usize, WNDPROC>(original);
        CallWindowProcA(proc, hwnd, msg, wparam, LPARAM(lparam))
    } else {
        DefWindowProcA(hwnd, msg, wparam, LPARAM(lparam))
    }
}
