//! The desktop window's view of the backbuffer (formerly src/Render/DesktopMirror.cpp; TofuExpress fork,
//! 2026-10-16). The backbuffer is the VR render size, tall on every headset; the window is placed on the monitor it
//! is on at the render's own aspect -- the whole frame, as large as the monitor holds, never upscaled. Only a window
//! held at another shape gets the centre crop at its aspect, right before Present: just the visible crop is copied
//! out of the backbuffer and drawn back over it (the OpenXR capture has already copied the full frame, so the headset
//! keeps the whole image). The mouse mapping (Hooks/OsInput.cpp) uses the same cover transform, so the cursor lands
//! where it is drawn.
//!
//! The launcher's forced sizes are the C++ half's; it attaches its three queries here at its static initialisation.
//!
//! THE MONITOR'S EYE (main's 18ddbab51): with the launcher's monitor output on one eye, the desktop shows that eye
//! alone -- recorded by the XR capture from its composed and sharpened eye textures (record_captured_eye), or, when the
//! capture skipped the frame, cut from the engine's side-by-side backbuffer (packed_eye); an eye narrower than the
//! window is centred at its native aspect between black margins.
use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use cvr_host::log;
use windows::core::{s, w, Interface, BOOL};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, POINT, RECT, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_UNKNOWN};
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain, IDXGISwapChain3};
use windows::Win32::Graphics::Gdi::{ClientToScreen, GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, GetGUIThreadInfo, GetMenu, GetWindowLongPtrW, GetWindowThreadProcessId, IsIconic, GUITHREADINFO, GUI_INMOVESIZE, GWL_EXSTYLE,
    GWL_STYLE, SWP_NOACTIVATE, SWP_NOZORDER, WINDOW_EX_STYLE, WINDOW_STYLE,
};

// ---- the launcher's sizes (C++)

static DISPLAY_WIDTH: AtomicUsize = AtomicUsize::new(0);
static DISPLAY_HEIGHT: AtomicUsize = AtomicUsize::new(0);
static WINDOW_WIDTH: AtomicUsize = AtomicUsize::new(0);
fn query(f: &AtomicUsize) -> u32 {
    match f.load(Ordering::Acquire) {
        0 => 0,
        // SAFETY: a function the C++ half attached.
        p => unsafe { core::mem::transmute::<usize, extern "C" fn() -> u32>(p)() },
    }
}
/// GetForcedDisplayModeWidth/Height and GetForcedWindowWidth (Core/VrCore.cpp).
#[no_mangle]
pub extern "C" fn cvr_desktop_mirror_attach(
    display_width: extern "C" fn() -> u32,
    display_height: extern "C" fn() -> u32,
    window_width: extern "C" fn() -> u32,
) {
    DISPLAY_WIDTH.store(display_width as usize, Ordering::Release);
    DISPLAY_HEIGHT.store(display_height as usize, Ordering::Release);
    WINDOW_WIDTH.store(window_width as usize, Ordering::Release);
}

/// GetForcedDisplayModeWidth/Height: the render size the launcher forces (0 when none or not attached).
pub(crate) fn forced_display_mode() -> (u32, u32) {
    (query(&DISPLAY_WIDTH), query(&DISPLAY_HEIGHT))
}

// ---- the geometry

/// DesktopCoverTransform: the visible part of the source, in source pixels, and the destination.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct CoverTransform {
    pub source_x: f64,
    pub source_y: f64,
    pub source_width: f64,
    pub source_height: f64,
    pub destination_width: u32,
    pub destination_height: u32,
}
/// ComputeDesktopCoverTransform: the centre crop of a source to the destination's aspect ("cover").
pub fn cover_transform(source_width: u32, source_height: u32, destination_width: u32, destination_height: u32) -> Option<CoverTransform> {
    if source_width == 0 || source_height == 0 || destination_width == 0 || destination_height == 0 {
        return None;
    }
    let (sw, sh) = (source_width as f64, source_height as f64);
    let mut out = CoverTransform {
        source_width: sw,
        source_height: sh,
        destination_width,
        destination_height,
        ..Default::default()
    };
    let source_aspect = sw / sh;
    let destination_aspect = destination_width as f64 / destination_height as f64;
    if source_aspect < destination_aspect {
        out.source_height = sw / destination_aspect;
        out.source_y = (sh - out.source_height) * 0.5;
    } else if source_aspect > destination_aspect {
        out.source_width = sh * destination_aspect;
        out.source_x = (sw - out.source_width) * 0.5;
    }
    Some(out)
}

type GetClientRectFn = unsafe extern "system" fn(HWND, *mut RECT) -> BOOL;
type SetWindowPosFn = unsafe extern "system" fn(HWND, HWND, i32, i32, i32, i32, u32) -> BOOL;
/// The user32 originals, not the port's IAT hooks (Hooks/OsInput.cpp): GetClientRect is hooked to report the virtual
/// size, and SetWindowPos to place the game window -- both would answer about themselves.
fn user32(name: windows::core::PCSTR) -> usize {
    // SAFETY: plain Win32 lookups of user32's exports.
    unsafe {
        GetModuleHandleW(w!("user32.dll"))
            .ok()
            .and_then(|m| GetProcAddress(m, name))
            .map_or(0, |f| f as usize)
    }
}
fn original_get_client_rect() -> Option<GetClientRectFn> {
    static F: OnceLock<usize> = OnceLock::new();
    let p = *F.get_or_init(|| user32(s!("GetClientRect")));
    // SAFETY: user32's GetClientRect.
    (p != 0).then(|| unsafe { core::mem::transmute::<usize, GetClientRectFn>(p) })
}
fn original_set_window_pos() -> Option<SetWindowPosFn> {
    static F: OnceLock<usize> = OnceLock::new();
    let p = *F.get_or_init(|| user32(s!("SetWindowPos")));
    // SAFETY: user32's SetWindowPos.
    (p != 0).then(|| unsafe { core::mem::transmute::<usize, SetWindowPosFn>(p) })
}

/// GetDesktopPhysicalClientRect: the real client rect (not the hooked virtual size) and its screen origin.
pub fn physical_client_rect(hwnd: HWND) -> Option<(RECT, POINT)> {
    let get = original_get_client_rect()?;
    if hwnd.0.is_null() {
        return None;
    }
    let (mut rect, mut origin) = (RECT::default(), POINT::default());
    // SAFETY: the caller's window; out-structures on the stack.
    unsafe {
        if !get(hwnd, &mut rect).as_bool() {
            return None;
        }
        ClientToScreen(hwnd, &mut origin).as_bool().then_some((rect, origin))
    }
}
/// GetDesktopMonitorWindowRect: the outer window rect for the automatic desktop window on the monitor the window is
/// on (and that monitor's rect and the client rect it targets) -- the whole frame at the render's own aspect, as large
/// as the monitor holds, never upscaled.
pub fn monitor_window_rect(hwnd: HWND) -> Option<(RECT, RECT, RECT)> {
    // SAFETY (whole body): plain Win32 queries of the caller's window.
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: core::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if hwnd.0.is_null() || monitor.0.is_null() || !GetMonitorInfoW(monitor, &mut mi).as_bool() {
            return None;
        }
        let monitor_width = (mi.rcMonitor.right - mi.rcMonitor.left) as u32;
        let monitor_height = (mi.rcMonitor.bottom - mi.rcMonitor.top) as u32;
        let source_width = query(&DISPLAY_WIDTH);
        let source_height = query(&DISPLAY_HEIGHT);
        if monitor_width == 0 || monitor_height == 0 || source_width == 0 || source_height == 0 {
            return None;
        }
        let scale = 1.0f64
            .min(monitor_width as f64 / source_width as f64)
            .min(monitor_height as f64 / source_height as f64);
        let client_width = ((source_width as f64 * scale).round() as i64 as u32).max(1);
        let client_height = ((source_height as f64 * scale).round() as i64 as u32).max(1);
        let client_left = mi.rcMonitor.left + (monitor_width as i32 - client_width as i32) / 2;
        let client_top = mi.rcMonitor.top + (monitor_height as i32 - client_height as i32) / 2;
        let desired = RECT {
            left: client_left,
            top: client_top,
            right: client_left + client_width as i32,
            bottom: client_top + client_height as i32,
        };
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: client_width as i32,
            bottom: client_height as i32,
        };
        let style = WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32);
        let ex_style = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        AdjustWindowRectEx(&mut outer, style, !GetMenu(hwnd).0.is_null(), ex_style).ok()?;
        let left = desired.left + outer.left;
        let top = desired.top + outer.top;
        let window = RECT {
            left,
            top,
            right: left + (outer.right - outer.left),
            bottom: top + (outer.bottom - outer.top),
        };
        Some((window, mi.rcMonitor, desired))
    }
}
/// A window size set in the launcher that differs from the render size is the player's; leave it alone.
fn uses_automatic_desktop_window() -> bool {
    let window_width = query(&WINDOW_WIDTH);
    window_width == 0 || window_width == query(&DISPLAY_WIDTH)
}
/// Follows the window to whichever monitor it was moved to: a handful of user32 reads, SetWindowPos only on a change.
fn update_window_for_current_monitor(hwnd: HWND) -> bool {
    // SAFETY (whole body): plain Win32 calls on the caller's window.
    unsafe {
        if hwnd.0.is_null() || !uses_automatic_desktop_window() || IsIconic(hwnd).as_bool() {
            return false;
        }
        // Not while the player is dragging it: the drop decides the monitor.
        let mut gui = GUITHREADINFO {
            cbSize: core::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(GetWindowThreadProcessId(hwnd, None), &mut gui).is_ok() && (gui.flags & GUI_INMOVESIZE).0 != 0 {
            return false;
        }
        let Some((outer, monitor, target)) = monitor_window_rect(hwnd) else {
            return false;
        };
        if let Some((client, origin)) = physical_client_rect(hwnd) {
            if origin.x == target.left
                && origin.y == target.top
                && client.right - client.left == target.right - target.left
                && client.bottom - client.top == target.bottom - target.top
            {
                return true;
            }
        }
        let Some(set) = original_set_window_pos() else { return false };
        if !set(
            hwnd,
            HWND::default(),
            outer.left,
            outer.top,
            outer.right - outer.left,
            outer.bottom - outer.top,
            (SWP_NOZORDER | SWP_NOACTIVATE).0,
        )
        .as_bool()
        {
            return false;
        }
        log!(
            "DesktopMirror: client placed at ({},{})-({},{}) on monitor ({},{})-({},{})\n",
            target.left,
            target.top,
            target.right,
            target.bottom,
            monitor.left,
            monitor.top,
            monitor.right,
            monitor.bottom
        );
        true
    }
}

// ---- the crop pass

struct Frame {
    allocator: ID3D12CommandAllocator,
    fence_value: u64,
}
struct State {
    device: Option<ID3D12Device>,
    queue: Option<ID3D12CommandQueue>,
    list: Option<ID3D12GraphicsCommandList>,
    crop: Option<ID3D12Resource>,
    crop_width: u32,
    crop_height: u32,
    fence: Option<ID3D12Fence>,
    fence_event: HANDLE,
    next_fence_value: u64,
    frames: Vec<Frame>,
    blit: crate::color_blit::Blit,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    /// The packed eye's copy (one half of a side-by-side backbuffer) and the RTV its margins are cleared through.
    packed_eye: Option<ID3D12Resource>,
    eye_rtv: Option<ID3D12DescriptorHeap>,
}
// SAFETY: free-threaded D3D12 objects and an event handle; every access is under the mutex.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
fn lock() -> MutexGuard<'static, Option<State>> {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if g.is_none() {
        *g = Some(State {
            device: None,
            queue: None,
            list: None,
            crop: None,
            crop_width: 0,
            crop_height: 0,
            fence: None,
            fence_event: HANDLE::default(),
            next_fence_value: 1,
            frames: Vec::new(),
            blit: Default::default(),
            width: 0,
            height: 0,
            format: DXGI_FORMAT_UNKNOWN,
            packed_eye: None,
            eye_rtv: None,
        });
    }
    g
}

impl State {
    fn wait_for_fence(&self, value: u64) -> bool {
        let Some(fence) = &self.fence else { return true };
        // SAFETY (whole body): the live fence and the state's event.
        unsafe {
            if value == 0 || fence.GetCompletedValue() >= value {
                return true;
            }
            if self.fence_event.is_invalid() || fence.SetEventOnCompletion(value, self.fence_event).is_err() {
                return false;
            }
            WaitForSingleObject(self.fence_event, 1000) == WAIT_OBJECT_0 && fence.GetCompletedValue() >= value
        }
    }
    /// A resize or a rebind releases the allocators and the packed eye: their last use is drained first -- a
    /// submission whose own Signal failed included -- unless the device is gone. False: still in use, kept.
    fn reset_resources(&mut self) -> bool {
        let latest = self.next_fence_value - 1;
        if let (Some(fence), true) = (&self.fence, latest != 0) {
            // SAFETY: the live fence, queue and device.
            unsafe {
                if fence.GetCompletedValue() < latest {
                    let drained = self.queue.as_ref().is_some_and(|q| q.Signal(fence, latest).is_ok())
                        && fence.SetEventOnCompletion(latest, HANDLE::default()).is_ok();
                    if !drained && self.device.as_ref().is_some_and(|d| d.GetDeviceRemovedReason().is_ok()) {
                        return false;
                    }
                }
            }
        }
        self.frames.clear();
        self.list = None;
        self.crop = None;
        self.packed_eye = None;
        self.eye_rtv = None;
        self.crop_width = 0;
        self.crop_height = 0;
        self.fence = None;
        if !self.fence_event.is_invalid() {
            // SAFETY: the state's own event, closed once.
            let _ = unsafe { CloseHandle(self.fence_event) };
            self.fence_event = HANDLE::default();
        }
        self.blit.shutdown();
        self.next_fence_value = 1;
        self.width = 0;
        self.height = 0;
        self.format = DXGI_FORMAT_UNKNOWN;
        true
    }
    fn ensure_resources(&mut self, swap_chain: &IDXGISwapChain3, back_buffer: &ID3D12Resource) -> bool {
        let Some(device) = self.device.clone() else { return false };
        if self.queue.is_none() {
            return false;
        }
        // SAFETY (whole body): the live device, swapchain and backbuffer.
        unsafe {
            let Ok(swap_desc) = swap_chain.GetDesc() else { return false };
            if swap_desc.BufferCount == 0 {
                return false;
            }
            let desc = back_buffer.GetDesc();
            let (width, height, format) = (desc.Width as u32, desc.Height, swap_desc.BufferDesc.Format);
            if self.list.is_some()
                && self.width == width
                && self.height == height
                && self.format == format
                && self.frames.len() == swap_desc.BufferCount as usize
            {
                return true;
            }
            if !self.reset_resources() {
                return false;
            }
            self.width = width;
            self.height = height;
            self.format = format;
            for _ in 0..swap_desc.BufferCount {
                match device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) {
                    Ok(allocator) => self.frames.push(Frame { allocator, fence_value: 0 }),
                    Err(_) => {
                        self.reset_resources();
                        return false;
                    }
                }
            }
            let list: Option<ID3D12GraphicsCommandList> = device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &self.frames[0].allocator, None)
                .ok();
            let fence: Option<ID3D12Fence> = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).ok();
            match (list, fence) {
                (Some(list), Some(fence)) if list.Close().is_ok() => {
                    self.list = Some(list);
                    self.fence = Some(fence);
                }
                _ => {
                    self.reset_resources();
                    return false;
                }
            }
            self.fence_event = CreateEventW(None, false, false, None).unwrap_or_default();
            if self.fence_event.is_invalid() {
                self.reset_resources();
                return false;
            }
            log!(
                "DesktopMirror: initialized desktop pass {}x{} format={} buffers={}\n",
                width,
                height,
                format.0,
                swap_desc.BufferCount
            );
        }
        true
    }
    /// The crop texture follows the window's aspect, so it is rebuilt only when the window changes shape.
    fn ensure_crop(&mut self, back: &D3D12_RESOURCE_DESC, width: u32, height: u32) -> bool {
        if self.crop.is_some() && self.crop_width == width && self.crop_height == height {
            return true;
        }
        if !self.wait_for_fence(self.next_fence_value - 1) {
            return false;
        }
        self.crop = None;
        self.crop_width = 0;
        self.crop_height = 0;
        let Some(device) = &self.device else { return false };
        let desc = D3D12_RESOURCE_DESC {
            Width: width as u64,
            Height: height,
            MipLevels: 1,
            Flags: D3D12_RESOURCE_FLAG_NONE,
            ..*back
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut crop: Option<ID3D12Resource> = None;
        // SAFETY: the live device; descriptions on the stack.
        if unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, None, &mut crop) }.is_err()
            || crop.is_none()
        {
            log!("DesktopMirror: failed to create crop copy {}x{} format={}\n", width, height, back.Format.0);
            return false;
        }
        self.crop = crop;
        self.crop_width = width;
        self.crop_height = height;
        true
    }
    /// The frame's allocator once its last use is done, and the list reset on it.
    fn begin_frame(&self, index: usize) -> Option<(ID3D12GraphicsCommandList, ID3D12CommandAllocator)> {
        let list = self.list.clone()?;
        let allocator = self.frames.get(index)?.allocator.clone();
        // SAFETY: the state's own list and allocator.
        unsafe {
            if !self.wait_for_fence(self.frames[index].fence_value) || allocator.Reset().is_err() || list.Reset(&allocator, None).is_err() {
                return None;
            }
        }
        Some((list, allocator))
    }
    /// The list closed and run, the frame stamped with its fence value.
    fn submit_frame(&mut self, index: usize, list: &ID3D12GraphicsCommandList) {
        let (Some(queue), Some(fence)) = (self.queue.clone(), self.fence.clone()) else { return };
        // SAFETY: the state's own list, queue and fence.
        unsafe {
            if list.Close().is_err() {
                return;
            }
            queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>().unwrap_or_else(|_| unreachable!()))]);
            let value = self.next_fence_value;
            self.next_fence_value += 1;
            self.frames[index].fence_value = value;
            if queue.Signal(&fence, value).is_err() {
                log!("DesktopMirror: queue signal failed for frame {}\n", value);
            }
        }
    }
}

/// DesktopMirrorSetDeviceAndQueue: the game's device and queue (resources rebuilt on a change).
pub fn set_device_and_queue(device: Option<&ID3D12Device>, queue: Option<&ID3D12CommandQueue>) {
    let mut g = lock();
    let Some(s) = g.as_mut() else { return };
    let same = |a: &Option<ID3D12Device>, b: Option<&ID3D12Device>| a.as_ref().map(Interface::as_raw) == b.map(Interface::as_raw);
    if same(&s.device, device) && s.queue.as_ref().map(Interface::as_raw) == queue.map(Interface::as_raw) {
        return;
    }
    if !s.reset_resources() {
        return;
    }
    s.device = device.cloned();
    s.queue = queue.cloned();
}
/// DesktopMirrorInvalidate.
pub fn invalidate() {
    if let Some(s) = lock().as_mut() {
        s.reset_resources();
    }
}

fn transition(resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
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

/// DesktopMirrorRender: before the game's Present, the centre crop at the window's aspect when the window is held at
/// another shape than the render (nothing at all otherwise).
pub fn render(swap_chain: &IDXGISwapChain, hwnd: HWND) {
    if hwnd.0.is_null() {
        return;
    }
    update_window_for_current_monitor(hwnd);
    let Some((client, _)) = physical_client_rect(hwnd) else { return };
    let destination_width = (client.right - client.left).max(0) as u32;
    let destination_height = (client.bottom - client.top).max(0) as u32;
    if destination_width == 0 || destination_height == 0 {
        return;
    }
    let Ok(swap_chain) = swap_chain.cast::<IDXGISwapChain3>() else { return };
    // SAFETY (whole block): the live swapchain and its buffers, the state's objects, the recording list.
    unsafe {
        let frame_index = swap_chain.GetCurrentBackBufferIndex();
        let Ok(back_buffer) = swap_chain.GetBuffer::<ID3D12Resource>(frame_index) else {
            return;
        };
        let back = back_buffer.GetDesc();
        let back_width = back.Width as u32;
        let Some(t) = cover_transform(back_width, back.Height, destination_width, destination_height) else {
            return;
        };
        // Same aspect: the stretch alone is already right (a window rounded to whole pixels is off by up to half a
        // window pixel, not worth a crop).
        if t.source_width > back_width as f64 - 1.5 && t.source_height > back.Height as f64 - 1.5 {
            return;
        }
        let x0 = t.source_x.floor() as u32;
        let y0 = t.source_y.floor() as u32;
        let x1 = back_width.min((t.source_x + t.source_width).ceil() as u32);
        let y1 = back.Height.min((t.source_y + t.source_height).ceil() as u32);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let (crop_width, crop_height) = (x1 - x0, y1 - y0);

        let mut g = lock();
        let Some(s) = g.as_mut() else { return };
        if !s.ensure_resources(&swap_chain, &back_buffer) || frame_index as usize >= s.frames.len() {
            return;
        }
        if !s.ensure_crop(&back, crop_width, crop_height) {
            return;
        }
        let Some(device) = s.device.clone() else { return };
        if !s.blit.ensure(&device, s.format, back_width, back.Height) {
            return;
        }
        let Some(crop) = s.crop.clone() else { return };
        let Some((list, _allocator)) = s.begin_frame(frame_index as usize) else { return };
        list.ResourceBarrier(&[
            transition(&back_buffer, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition(&crop, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST),
        ]);
        let location = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(r),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        let source_box = D3D12_BOX {
            left: x0,
            top: y0,
            front: 0,
            right: x1,
            bottom: y1,
            back: 1,
        };
        list.CopyTextureRegion(&location(&crop), 0, 0, 0, &location(&back_buffer), Some(&source_box));
        list.ResourceBarrier(&[
            transition(&back_buffer, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET),
            transition(&crop, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE),
        ]);
        // The crop's exact (sub-pixel) rectangle inside the whole-pixel copy.
        let uv = [
            ((t.source_x - x0 as f64) / crop_width as f64) as f32,
            ((t.source_y - y0 as f64) / crop_height as f64) as f32,
            ((t.source_x + t.source_width - x0 as f64) / crop_width as f64) as f32,
            ((t.source_y + t.source_height - y0 as f64) / crop_height as f64) as f32,
        ];
        let blitted = s.blit.record_blit(&list, &crop, &back_buffer, uv);
        list.ResourceBarrier(&[transition(&back_buffer, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT)]);
        // Submitted even when the blit failed: the barriers must still reach the queue, or the backbuffer is left in
        // COPY_SOURCE for Present.
        s.submit_frame(frame_index as usize, &list);
        if !blitted {
            static LOGGED: AtomicBool = AtomicBool::new(false);
            if !LOGGED.swap(true, Ordering::Relaxed) {
                log!("DesktopMirror: crop blit failed; the desktop shows the uncropped frame\n");
            }
        }
    }
}

// ---- the monitor's eye

/// `eye` into the backbuffer (it enters and leaves in PRESENT): an eye-sized image centred at its native aspect in a
/// wider buffer, the margins cleared black through a CPU RTV of `clear_heap` (made on first need, reusable once the
/// clear is recorded). The composed second eye is an RGBA8 typeless texture holding display-encoded pixels.
///
/// # Safety
/// `list` is recording; `eye` is in COPY_SOURCE for the list's span.
unsafe fn record_eye(device: &ID3D12Device, clear_heap: &mut Option<ID3D12DescriptorHeap>, list: &ID3D12GraphicsCommandList,
    back: &ID3D12Resource, eye: &ID3D12Resource) -> bool {
    // SAFETY (whole body): as the caller promises.
    unsafe {
        let (dst, src) = (back.GetDesc(), eye.GetDesc());
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB};
        let rgba_view = src.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS
            && (dst.Format == DXGI_FORMAT_R8G8B8A8_UNORM || dst.Format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB);
        if (src.Format != dst.Format && !rgba_view) || src.Width > dst.Width || src.Height != dst.Height
            || src.SampleDesc.Count != 1 || dst.SampleDesc.Count != 1
        {
            return false;
        }
        let mut state = D3D12_RESOURCE_STATE_PRESENT;
        let x = ((dst.Width - src.Width) / 2) as u32;
        if src.Width != dst.Width {
            if clear_heap.is_none() {
                let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 1, ..Default::default() };
                match device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) {
                    Ok(heap) => *clear_heap = Some(heap),
                    Err(_) => return false,
                }
            }
            let Some(heap) = clear_heap.as_ref() else { return false };
            let rtv = heap.GetCPUDescriptorHandleForHeapStart();
            device.CreateRenderTargetView(back, None, rtv);
            list.ResourceBarrier(&[transition(back, state, D3D12_RESOURCE_STATE_RENDER_TARGET)]);
            state = D3D12_RESOURCE_STATE_RENDER_TARGET;
            let black = [0.0f32, 0.0, 0.0, 1.0];
            let height = dst.Height as i32;
            let margins = [
                RECT { left: 0, top: 0, right: x as i32, bottom: height },
                RECT { left: (x as u64 + src.Width) as i32, top: 0, right: dst.Width as i32, bottom: height },
            ];
            list.ClearRenderTargetView(rtv, &black, Some(if x != 0 { &margins[..] } else { &margins[1..] }));
        }
        list.ResourceBarrier(&[transition(back, state, D3D12_RESOURCE_STATE_COPY_DEST)]);
        let location = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(r),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        list.CopyTextureRegion(&location(back), x, 0, 0, &location(eye), None);
        list.ResourceBarrier(&[transition(back, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT)]);
        true
    }
}

/// The capture's share (DesktopMirrorRecordCapturedEye): on the capture's own list, while it still owns the eyes in
/// COPY_SOURCE, the chosen eye into the backbuffer. A shared mono frame uses MAIN for both eyes; a native stereo frame
/// can still give the other eye from the packed backbuffer when its separate capture was unavailable.
///
/// # Safety
/// `list` is the capture's recording list; `main` and `second` (or null) its eye textures in COPY_SOURCE.
pub unsafe fn record_captured_eye(device: &ID3D12Device, clear_heap: &mut Option<ID3D12DescriptorHeap>,
    list: &ID3D12GraphicsCommandList, back: &ID3D12Resource, eye_index: u32, main_eye_index: u32,
    main: Option<&ID3D12Resource>, second: Option<&ID3D12Resource>, packed: bool) -> bool {
    let mut eye = main;
    if eye_index != main_eye_index && (second.is_some() || packed) {
        eye = second;
    }
    // SAFETY: as the caller promises.
    eye.is_some_and(|e| unsafe { record_eye(device, clear_heap, list, back, e) })
}

/// DesktopMirrorPackedEye: a native side-by-side frame still has both eyes when the XR capture skipped it -- the chosen
/// half copied out of the backbuffer and back over it, centred, on the mirror's own list.
pub fn packed_eye(swap_chain: &IDXGISwapChain, eye_index: u32) {
    let Ok(chain) = swap_chain.cast::<IDXGISwapChain3>() else { return };
    // SAFETY (whole block): the live swapchain and its buffers, the state's objects, the recording list.
    unsafe {
        let index = chain.GetCurrentBackBufferIndex();
        let Ok(back) = chain.GetBuffer::<ID3D12Resource>(index) else { return };
        let mut desc = back.GetDesc();
        let width = (desc.Width / 2) as u32;
        if width == 0 || desc.SampleDesc.Count != 1 {
            return;
        }
        let x = if eye_index == 1 { desc.Width as u32 - width } else { 0 };
        let mut g = lock();
        let Some(s) = g.as_mut() else { return };
        if !s.ensure_resources(&chain, &back) || index as usize >= s.frames.len() {
            return;
        }
        let Some(device) = s.device.clone() else { return };
        if s.packed_eye.is_none() {
            desc.Width = width as u64;
            desc.Flags = D3D12_RESOURCE_FLAG_NONE;
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            let mut made: Option<ID3D12Resource> = None;
            if device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_SOURCE, None, &mut made).is_err() {
                return;
            }
            s.packed_eye = made;
        }
        let Some(eye) = s.packed_eye.clone() else { return };
        let Some((list, _allocator)) = s.begin_frame(index as usize) else { return };
        list.ResourceBarrier(&[
            transition(&back, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition(&eye, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST),
        ]);
        let source_box = D3D12_BOX { left: x, top: 0, front: 0, right: x + width, bottom: desc.Height, back: 1 };
        let location = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(r),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        list.CopyTextureRegion(&location(&eye), 0, 0, 0, &location(&back), Some(&source_box));
        list.ResourceBarrier(&[
            transition(&eye, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition(&back, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT),
        ]);
        let mut heap = s.eye_rtv.take();
        let recorded = record_eye(&device, &mut heap, &list, &back, &eye);
        s.eye_rtv = heap;
        if !recorded {
            let _ = list.Close();
            return;
        }
        s.submit_frame(index as usize, &list);
    }
}

// ---- the C ABI (include/Render/DesktopMirror.hpp)

/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_desktop_cover_transform(sw: u32, sh: u32, dw: u32, dh: u32, out: *mut CoverTransform) -> bool {
    let t = cover_transform(sw, sh, dw, dh);
    // SAFETY: the caller's out-structure.
    if let Some(o) = unsafe { out.as_mut() } {
        *o = t.unwrap_or_default();
    }
    t.is_some()
}
/// # Safety
/// `rect`/`origin` are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_desktop_physical_client_rect(hwnd: *mut c_void, rect: *mut RECT, origin: *mut POINT) -> bool {
    let r = physical_client_rect(HWND(hwnd));
    // SAFETY: the caller's out-structures.
    unsafe {
        let (a, b) = r.unwrap_or_default();
        if let Some(o) = rect.as_mut() {
            *o = a;
        }
        if let Some(o) = origin.as_mut() {
            *o = b;
        }
    }
    r.is_some()
}
/// # Safety
/// `window` is writable; `monitor`/`target` writable or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_desktop_monitor_window_rect(hwnd: *mut c_void, window: *mut RECT, monitor: *mut RECT, target: *mut RECT) -> bool {
    let r = monitor_window_rect(HWND(hwnd));
    // SAFETY: the caller's out-structures.
    unsafe {
        if let Some(o) = window.as_mut() {
            *o = r.map_or(RECT::default(), |v| v.0);
        }
        if let (Some(o), Some(v)) = (monitor.as_mut(), r) {
            *o = v.1;
        }
        if let (Some(o), Some(v)) = (target.as_mut(), r) {
            *o = v.2;
        }
    }
    r.is_some()
}
/// # Safety
/// `device`/`queue` are an ID3D12Device/ID3D12CommandQueue or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_desktop_mirror_set_device_and_queue(device: *mut c_void, queue: *mut c_void) {
    // SAFETY: the caller's objects.
    unsafe { set_device_and_queue(ID3D12Device::from_raw_borrowed(&device), ID3D12CommandQueue::from_raw_borrowed(&queue)) }
}
/// # Safety
/// `swap_chain` is an IDXGISwapChain or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_desktop_mirror_render(swap_chain: *mut c_void, hwnd: *mut c_void) {
    // SAFETY: the caller's swapchain.
    if let Some(sc) = unsafe { IDXGISwapChain::from_raw_borrowed(&swap_chain) } {
        render(sc, HWND(hwnd));
    }
}
#[no_mangle]
pub extern "C" fn cvr_desktop_mirror_invalidate() {
    invalidate()
}

#[cfg(test)]
mod tests {
    use super::cover_transform;
    #[test]
    fn cover_crops_to_the_destination_aspect() {
        let t = cover_transform(2648, 2864, 2560, 1440).unwrap_or_default();
        assert_eq!((t.source_x, t.source_width), (0.0, 2648.0));
        assert!((t.source_height - 2648.0 / (2560.0 / 1440.0)).abs() < 1e-9);
        assert!((t.source_y - (2864.0 - t.source_height) * 0.5).abs() < 1e-9);
        let same = cover_transform(1000, 500, 200, 100).unwrap_or_default();
        assert_eq!((same.source_x, same.source_y, same.source_width, same.source_height), (0.0, 0.0, 1000.0, 500.0));
        assert!(cover_transform(0, 1, 1, 1).is_none());
    }
}
