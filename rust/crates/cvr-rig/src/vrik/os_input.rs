//! The Win32 calls the game makes about the cursor and the window, answered for VR (formerly src/Hooks/OsInput.cpp).
//! The game lives in the VIRTUAL client -- the render size, which GetClientRect reports -- and the desktop window shows
//! the centre crop of it at the window's aspect (cvr-gpu desktop_mirror), so every position crossing between the two
//! goes through the same crop. Hooked by IAT, not by detour: the game's imports, the plugin's own (its overlay needs the
//! same virtual answers) and CET's for its Win32 ImGui backend; the system DLLs stay untouched. A function a module did
//! not import is reported and skipped. src/Hooks/OsInput.cpp keeps g_gameHwnd, the cursor-map export and the C++ names
//! the swapchain hooks call.
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, GetMonitorInfoA, IntersectRect, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleA, GetModuleHandleExA, GetProcAddress, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, CURSORINFO, GA_ROOT, SM_CXFULLSCREEN, SM_CXSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYFULLSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SWP_NOMOVE, SWP_NOSIZE,
};

/// cvr-gpu desktop_mirror.rs's DesktopCoverTransform: the visible part of the source, in source pixels.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Cover {
    source_x: f64,
    source_y: f64,
    source_width: f64,
    source_height: f64,
    destination_width: u32,
    destination_height: u32,
}
const _: () = assert!(core::mem::size_of::<Cover>() == 40);

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_DebugCursorMap: [AtomicI32; 4];
    static CyberpunkVR_RuntimeDiagnostics: i32;
    fn GetForcedDisplayModeWidth() -> u32;
    fn GetForcedDisplayModeHeight() -> u32;
    fn GetForcedWindowWidth() -> u32;
    fn GetMenuMode() -> i32;
    fn cvr_desktop_cover_transform(sw: u32, sh: u32, dw: u32, dh: u32, out: *mut Cover) -> bool;
    fn cvr_desktop_physical_client_rect(hwnd: HWND, rect: *mut RECT, origin: *mut POINT) -> bool;
    fn cvr_desktop_monitor_window_rect(hwnd: HWND, window: *mut RECT, monitor: *mut RECT, target: *mut RECT) -> bool;
    fn cvr_cpp_verbose_log() -> i32;
    fn cvr_cpp_overlay_visible() -> bool;
    fn cvr_cpp_launcher_open() -> bool;
    fn cvr_overlay_host_open() -> bool;
    fn cvr_cpp_game_hwnd() -> HWND;
    fn cvr_cpp_cursor_clipped() -> *mut bool;
}

fn log(text: &str) {
    super::env::log(text);
}
fn verbose() -> bool {
    // SAFETY: a plain read of the launcher's DEBUG box.
    unsafe { cvr_cpp_verbose_log() != 0 }
}
fn launcher_open() -> bool {
    // SAFETY: a C++ service.
    unsafe { cvr_cpp_launcher_open() }
}
fn game_hwnd() -> HWND {
    // SAFETY: a C++ service.
    unsafe { cvr_cpp_game_hwnd() }
}
fn p(v: *const c_void) -> String {
    format!("{:016X}", v as usize)
}

// ---- the originals ----
type GetCursorPosFn = unsafe extern "system" fn(*mut POINT) -> i32;
type SetCursorPosFn = unsafe extern "system" fn(i32, i32) -> i32;
type SetWindowPosFn = unsafe extern "system" fn(HWND, HWND, i32, i32, i32, i32, u32) -> i32;
type MoveWindowFn = unsafe extern "system" fn(HWND, i32, i32, i32, i32, i32) -> i32;
type RectFn = unsafe extern "system" fn(HWND, *mut RECT) -> i32;
type GetCursorInfoFn = unsafe extern "system" fn(*mut CURSORINFO) -> i32;
type ClipCursorFn = unsafe extern "system" fn(*const RECT) -> i32;
type GetSystemMetricsFn = unsafe extern "system" fn(i32) -> i32;
type GetMessagePosFn = unsafe extern "system" fn() -> u32;

static ORIG_GET_CURSOR_POS: AtomicUsize = AtomicUsize::new(0);
static ORIG_SET_CURSOR_POS: AtomicUsize = AtomicUsize::new(0);
static ORIG_SET_WINDOW_POS: AtomicUsize = AtomicUsize::new(0);
static ORIG_MOVE_WINDOW: AtomicUsize = AtomicUsize::new(0);
static ORIG_GET_CLIENT_RECT: AtomicUsize = AtomicUsize::new(0);
static ORIG_GET_WINDOW_RECT: AtomicUsize = AtomicUsize::new(0);
static ORIG_GET_CURSOR_INFO: AtomicUsize = AtomicUsize::new(0);
static ORIG_CLIP_CURSOR: AtomicUsize = AtomicUsize::new(0);
static ORIG_GET_SYSTEM_METRICS: AtomicUsize = AtomicUsize::new(0);
static ORIG_GET_MESSAGE_POS: AtomicUsize = AtomicUsize::new(0);

fn orig<F: Copy>(slot: &AtomicUsize) -> Option<F> {
    const { assert!(core::mem::size_of::<F>() == 8) };
    let a = slot.load(Ordering::Acquire);
    // SAFETY: the slot holds a user32 function of type F (GetProcAddress or the import it replaced).
    (a != 0).then(|| unsafe { core::mem::transmute_copy::<usize, F>(&a) })
}

unsafe fn is_game_window_foreground(hwnd: HWND) -> bool {
    if hwnd.is_null() || IsWindow(hwnd) == 0 {
        return false;
    }
    let foreground = GetForegroundWindow();
    if foreground.is_null() {
        return false;
    }
    if foreground == hwnd || GetAncestor(foreground, GA_ROOT) == hwnd {
        return true;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(foreground, &mut pid);
    pid == GetCurrentProcessId()
}

/// One import of `module` pointed at `detour`; the import's previous target into `original`.
unsafe fn hook_iat(module: usize, dll: &str, function: &str, detour: usize, original: &AtomicUsize) -> bool {
    if module == 0 {
        log("HookIAT: Invalid module handle\n");
        return false;
    }
    let base = module as *const u8;
    let read16 = |o: usize| core::ptr::read_unaligned(base.add(o) as *const u16);
    let read32 = |o: usize| core::ptr::read_unaligned(base.add(o) as *const u32);
    let read64 = |o: usize| core::ptr::read_unaligned(base.add(o) as *const u64);
    if read16(0) != 0x5A4D {
        log(&format!("HookIAT: Invalid DOS signature for module {}\n", p(base as *const c_void)));
        return false;
    }
    let nt = read32(0x3C) as usize;
    if read32(nt) != 0x0000_4550 {
        log(&format!("HookIAT: Invalid NT signature for module {}\n", p(base as *const c_void)));
        return false;
    }
    // IMAGE_OPTIONAL_HEADER64 at +0x18, DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT] at +0x70 + 8.
    let import_dir = read32(nt + 0x18 + 0x78) as usize;
    if import_dir == 0 {
        log(&format!("HookIAT: No import directory for module {}\n", p(base as *const c_void)));
        return false;
    }
    let mut descriptor = import_dir;
    let mut dll_found = false;
    while read32(descriptor + 12) != 0 {
        let name = CStr::from_ptr(base.add(read32(descriptor + 12) as usize) as *const c_char);
        if name.to_bytes().eq_ignore_ascii_case(dll.as_bytes()) {
            dll_found = true;
            let mut thunk = read32(descriptor + 16) as usize;
            let mut orig_thunk = read32(descriptor) as usize;
            while read64(orig_thunk) != 0 {
                let data = read64(orig_thunk);
                if data & (1u64 << 63) == 0 {
                    // IMAGE_IMPORT_BY_NAME: Hint, then the name.
                    let import = CStr::from_ptr(base.add(data as usize + 2) as *const c_char);
                    if import.to_bytes() == function.as_bytes() {
                        let slot = base.add(thunk) as *mut usize;
                        // Our detour already in the slot: capturing it as "the original" would make it call itself.
                        if *slot == detour {
                            log(&format!(
                                "HookIAT: '{dll}!{function}' in module {} already hooked -- left as is\n",
                                p(base as *const c_void)
                            ));
                            return true;
                        }
                        let mut old = 0u32;
                        VirtualProtect(slot as *const c_void, 8, PAGE_EXECUTE_READWRITE, &mut old);
                        original.store(*slot, Ordering::Release);
                        *slot = detour;
                        VirtualProtect(slot as *const c_void, 8, old, &mut old);
                        log(&format!(
                            "HookIAT: Successfully hooked '{dll}!{function}' in module {} (original={}, hooked={})\n",
                            p(base as *const c_void),
                            p(original.load(Ordering::Acquire) as *const c_void),
                            p(detour as *const c_void)
                        ));
                        return true;
                    }
                }
                thunk += 8;
                orig_thunk += 8;
            }
        }
        descriptor += 20;
    }
    log(&format!(
        "HookIAT: Failed to find function '{function}' in DLL '{dll}' for module {}{}\n",
        p(base as *const c_void),
        if dll_found { "" } else { " (DLL import descriptor not found)" }
    ));
    false
}

// ---- the desktop cover transform ----
#[derive(Default)]
struct CursorTransform {
    cover: Cover,
    origin: POINT,
    width: i32,
    height: i32,
}
unsafe fn cursor_transform() -> Option<CursorTransform> {
    let hwnd = game_hwnd();
    if hwnd.is_null() {
        return None;
    }
    let (vw, vh) = (GetForcedDisplayModeWidth(), GetForcedDisplayModeHeight());
    let mut t = CursorTransform::default();
    let mut client = RECT::default();
    if vw == 0 || vh == 0 || !cvr_desktop_physical_client_rect(hwnd, &mut client, &mut t.origin) {
        return None;
    }
    t.width = client.right - client.left;
    t.height = client.bottom - client.top;
    (t.width > 0 && t.height > 0 && cvr_desktop_cover_transform(vw, vh, t.width as u32, t.height as u32, &mut t.cover)).then_some(t)
}
/// std::lround: half away from zero.
fn lround(v: f64) -> i32 {
    v.round() as i32
}
/// Screen point on the physical client -> screen point in the virtual client (same origin).
fn physical_to_virtual(point: POINT, t: &CursorTransform) -> POINT {
    let x = t.cover.source_x + (point.x - t.origin.x) as f64 * t.cover.source_width / t.width as f64;
    let y = t.cover.source_y + (point.y - t.origin.y) as f64 * t.cover.source_height / t.height as f64;
    POINT {
        x: t.origin.x + lround(x),
        y: t.origin.y + lround(y),
    }
}
/// And back, clamped into the client: a virtual point outside the crop has no place on the screen.
fn virtual_to_physical(point: POINT, t: &CursorTransform) -> POINT {
    let x = ((point.x - t.origin.x) as f64 - t.cover.source_x) * t.width as f64 / t.cover.source_width;
    let y = ((point.y - t.origin.y) as f64 - t.cover.source_y) * t.height as f64 / t.cover.source_height;
    let x = 0f64.max(((t.width - 1) as f64).min(x));
    let y = 0f64.max(((t.height - 1) as f64).min(y));
    POINT {
        x: t.origin.x + lround(x),
        y: t.origin.y + lround(y),
    }
}
/// A clip-rectangle edge: the right/bottom edge may be the client's own (exclusive) edge.
fn virtual_edge_to_physical(value: i32, horizontal: bool, t: &CursorTransform) -> i32 {
    let origin = if horizontal { t.origin.x } else { t.origin.y } as f64;
    let crop_origin = if horizontal { t.cover.source_x } else { t.cover.source_y };
    let crop_size = if horizontal { t.cover.source_width } else { t.cover.source_height };
    let destination = if horizontal { t.width } else { t.height } as f64;
    let mapped = (value as f64 - origin - crop_origin) * destination / crop_size;
    let mapped = 0f64.max(destination.min(mapped));
    lround(origin + mapped)
}
/// The largest size with the render's aspect that fits the bounds (src/Hooks/SwapChain.cpp's
/// FitRenderSizeInsideBounds): clamping each side alone would distort the stretched mirror.
fn fit_render_size_inside_bounds(rw: u32, rh: u32, mw: u32, mh: u32) -> (u32, u32) {
    if rw == 0 || rh == 0 || mw == 0 || mh == 0 || (rw <= mw && rh <= mh) {
        return (rw, rh);
    }
    if rw as u64 * mh as u64 > rh as u64 * mw as u64 {
        (mw, ((rh as u64 * mw as u64 + rw as u64 / 2) / rw as u64) as u32)
    } else {
        (((rw as u64 * mh as u64 + rh as u64 / 2) / rh as u64) as u32, mh)
    }
}

// ---- the hooks ----
static GET_CURSOR_POS_CALLS: AtomicI32 = AtomicI32::new(0);
static SET_CURSOR_POS_CALLS: AtomicI32 = AtomicI32::new(0);
static GET_CURSOR_INFO_CALLS: AtomicI32 = AtomicI32::new(0);
static GET_CLIENT_RECT_CALLS: AtomicI32 = AtomicI32::new(0);
static GET_WINDOW_RECT_CALLS: AtomicI32 = AtomicI32::new(0);
/// `callCount++ % n == 0` counted only while the DEBUG box is ticked; the count after the increment.
fn every(counter: &AtomicI32, n: i32) -> Option<i32> {
    if !verbose() {
        return None;
    }
    let before = counter.fetch_add(1, Ordering::Relaxed);
    (before % n == 0).then_some(before + 1)
}

unsafe extern "system" fn get_cursor_pos(point: *mut POINT) -> i32 {
    let res = orig::<GetCursorPosFn>(&ORIG_GET_CURSOR_POS).map_or(0, |f| f(point));
    if launcher_open() {
        return res;
    }
    if let Some(count) = every(&GET_CURSOR_POS_CALLS, 1000) {
        let (x, y) = point.as_ref().map_or((0, 0), |q| (q.x, q.y));
        log(&format!(
            "HookedGetCursorPos: called {count} times. lpPoint={}, pos=({x},{y}) g_gameHwnd={}\n",
            p(point as *const c_void),
            p(game_hwnd())
        ));
    }
    if res != 0 && !point.is_null() && !game_hwnd().is_null() {
        if let Some(t) = cursor_transform() {
            let physical = *point;
            *point = physical_to_virtual(physical, &t);
            if CyberpunkVR_RuntimeDiagnostics != 0 {
                let map = &CyberpunkVR_DebugCursorMap;
                map[0].store(physical.x, Ordering::Relaxed);
                map[1].store(physical.y, Ordering::Relaxed);
                map[2].store((*point).x, Ordering::Relaxed);
                map[3].store((*point).y, Ordering::Relaxed);
            }
        }
    }
    res
}
unsafe extern "system" fn set_cursor_pos(mut x: i32, mut y: i32) -> i32 {
    // The settings panel and the Lua mods' windows leave simulation running: the game's relative-mouse recenter must
    // not fight desktop pointing.
    if launcher_open() || cvr_cpp_overlay_visible() || cvr_overlay_host_open() {
        return 1;
    }
    if let Some(count) = every(&SET_CURSOR_POS_CALLS, 100) {
        log(&format!(
            "HookedSetCursorPos: called {count} times, target=({x},{y}) g_gameHwnd={}\n",
            p(game_hwnd())
        ));
    }
    if !game_hwnd().is_null() {
        if let Some(t) = cursor_transform() {
            let mapped = virtual_to_physical(POINT { x, y }, &t);
            x = mapped.x;
            y = mapped.y;
        }
    }
    orig::<SetCursorPosFn>(&ORIG_SET_CURSOR_POS).map_or(0, |f| f(x, y))
}
unsafe extern "system" fn get_cursor_info(info: *mut CURSORINFO) -> i32 {
    let res = orig::<GetCursorInfoFn>(&ORIG_GET_CURSOR_INFO).map_or(0, |f| f(info));
    if launcher_open() {
        return res;
    }
    if let Some(count) = every(&GET_CURSOR_INFO_CALLS, 1000) {
        let (x, y) = if res != 0 {
            info.as_ref().map_or((0, 0), |i| (i.ptScreenPos.x, i.ptScreenPos.y))
        } else {
            (0, 0)
        };
        log(&format!("HookedGetCursorInfo: called {count} times. ptScreenPos=({x},{y})\n"));
    }
    if res != 0 && !info.is_null() && !game_hwnd().is_null() {
        if let Some(t) = cursor_transform() {
            (*info).ptScreenPos = physical_to_virtual((*info).ptScreenPos, &t);
        }
    }
    res
}
unsafe extern "system" fn clip_cursor(rect: *const RECT) -> i32 {
    let original = orig::<ClipCursorFn>(&ORIG_CLIP_CURSOR);
    let call = |r: *const RECT| original.map_or(0, |f| f(r));
    if launcher_open() {
        return call(core::ptr::null());
    }
    if verbose() {
        let r = rect.as_ref().copied().unwrap_or_default();
        log(&format!(
            "HookedClipCursor called: rect={} ({},{})-({},{}) g_gameHwnd={}\n",
            p(rect as *const c_void),
            r.left,
            r.top,
            r.right,
            r.bottom,
            p(game_hwnd())
        ));
    }
    let mut scaled = RECT::default();
    let mut use_rect = rect;
    if let Some(r) = rect.as_ref() {
        if !game_hwnd().is_null() {
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            if w <= 1 && h <= 1 {
                let menu = GetMenuMode() != 0 || cvr_cpp_overlay_visible() || cvr_overlay_host_open();
                if menu {
                    if verbose() {
                        log(&format!(
                            "HookedClipCursor: Ignored centering clip ({},{}) because menu mode is active\n",
                            r.left, r.top
                        ));
                    }
                    return call(core::ptr::null());
                }
            }
            if let Some(t) = cursor_transform() {
                scaled.left = virtual_edge_to_physical(r.left, true, &t);
                scaled.top = virtual_edge_to_physical(r.top, false, &t);
                scaled.right = virtual_edge_to_physical(r.right, true, &t);
                scaled.bottom = virtual_edge_to_physical(r.bottom, false, &t);
                use_rect = &scaled;
                if verbose() {
                    log(&format!(
                        "ClipCursor cover map: ({},{})-({},{}) -> ({},{})-({},{})\n",
                        r.left, r.top, r.right, r.bottom, scaled.left, scaled.top, scaled.right, scaled.bottom
                    ));
                }
            }
        }
    }
    call(use_rect)
}
/// The game's window (or, before it is known, the foreground one of this process).
unsafe fn is_game(hwnd: HWND) -> bool {
    let game = game_hwnd();
    !hwnd.is_null() && (hwnd == game || (game.is_null() && is_game_window_foreground(hwnd)))
}
unsafe extern "system" fn get_client_rect(hwnd: HWND, rect: *mut RECT) -> i32 {
    let res = orig::<RectFn>(&ORIG_GET_CLIENT_RECT).map_or(0, |f| f(hwnd, rect));
    if launcher_open() {
        return res;
    }
    if let Some(count) = every(&GET_CLIENT_RECT_CALLS, 100) {
        log(&format!(
            "HookedGetClientRect: called {count} times. hWnd={} g_gameHwnd={}\n",
            p(hwnd),
            p(game_hwnd())
        ));
    }
    if res != 0 && !rect.is_null() && is_game(hwnd) {
        let (vw, vh) = (GetForcedDisplayModeWidth(), GetForcedDisplayModeHeight());
        if vw > 0 && vh > 0 {
            *rect = RECT {
                left: 0,
                top: 0,
                right: vw as i32,
                bottom: vh as i32,
            };
        }
    }
    res
}
unsafe extern "system" fn get_window_rect(hwnd: HWND, rect: *mut RECT) -> i32 {
    let res = orig::<RectFn>(&ORIG_GET_WINDOW_RECT).map_or(0, |f| f(hwnd, rect));
    if launcher_open() {
        return res;
    }
    if let Some(count) = every(&GET_WINDOW_RECT_CALLS, 100) {
        log(&format!(
            "HookedGetWindowRect: called {count} times. hWnd={} g_gameHwnd={}\n",
            p(hwnd),
            p(game_hwnd())
        ));
    }
    if res != 0 && !rect.is_null() && is_game(hwnd) {
        let (vw, vh) = (GetForcedDisplayModeWidth(), GetForcedDisplayModeHeight());
        if vw > 0 && vh > 0 {
            (*rect).right = (*rect).left + vw as i32;
            (*rect).bottom = (*rect).top + vh as i32;
        }
    }
    res
}
/// The game's window in the automatic mode (no window size of its own in the launcher).
unsafe fn is_automatic_game_window(hwnd: HWND) -> bool {
    let forced = GetForcedWindowWidth();
    !hwnd.is_null() && hwnd == game_hwnd() && (forced == 0 || forced == GetForcedDisplayModeWidth())
}
/// Where a window of this size goes: the desktop mirror's place for the automatic game window, else fitted inside the
/// monitor at its own aspect, centred. None leaves the call as it was.
unsafe fn place(hwnd: HWND, cx: i32, cy: i32) -> Option<(i32, i32, i32, i32)> {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = core::mem::zeroed();
    mi.cbSize = core::mem::size_of::<MONITORINFO>() as u32;
    if cx <= 0 || cy <= 0 || GetMonitorInfoA(monitor, &mut mi) == 0 {
        return None;
    }
    let mon_w = (mi.rcMonitor.right - mi.rcMonitor.left) as u32;
    let mon_h = (mi.rcMonitor.bottom - mi.rcMonitor.top) as u32;
    let mut window = RECT::default();
    if is_automatic_game_window(hwnd) && cvr_desktop_monitor_window_rect(hwnd, &mut window, core::ptr::null_mut(), core::ptr::null_mut()) {
        return Some((window.left, window.top, window.right - window.left, window.bottom - window.top));
    }
    let (fw, fh) = fit_render_size_inside_bounds(cx as u32, cy as u32, mon_w, mon_h);
    if fw != cx as u32 || fh != cy as u32 {
        let (w, h) = (fw as i32, fh as i32);
        return Some((mi.rcMonitor.left + (mon_w as i32 - w) / 2, mi.rcMonitor.top + (mon_h as i32 - h) / 2, w, h));
    }
    None
}
unsafe extern "system" fn set_window_pos(hwnd: HWND, after: HWND, mut x: i32, mut y: i32, mut cx: i32, mut cy: i32, mut flags: u32) -> i32 {
    if flags & SWP_NOSIZE == 0 {
        if let Some((px, py, w, h)) = place(hwnd, cx, cy) {
            (x, y, cx, cy) = (px, py, w, h);
            flags &= !SWP_NOMOVE;
        }
    }
    orig::<SetWindowPosFn>(&ORIG_SET_WINDOW_POS).map_or(0, |f| f(hwnd, after, x, y, cx, cy, flags))
}
unsafe extern "system" fn move_window(hwnd: HWND, mut x: i32, mut y: i32, mut w: i32, mut h: i32, repaint: i32) -> i32 {
    if let Some((px, py, pw, ph)) = place(hwnd, w, h) {
        (x, y, w, h) = (px, py, pw, ph);
    }
    orig::<MoveWindowFn>(&ORIG_MOVE_WINDOW).map_or(0, |f| f(hwnd, x, y, w, h, repaint))
}
unsafe extern "system" fn get_system_metrics(index: i32) -> i32 {
    let res = orig::<GetSystemMetricsFn>(&ORIG_GET_SYSTEM_METRICS).map_or(0, |f| f(index));
    if launcher_open() {
        return res;
    }
    let (vw, vh) = (GetForcedDisplayModeWidth(), GetForcedDisplayModeHeight());
    if vw > 0 && vh > 0 {
        if index == SM_CXSCREEN || index == SM_CXFULLSCREEN || index == SM_CXVIRTUALSCREEN {
            if verbose() {
                log(&format!("GetSystemMetrics({index}) override: {res} -> {vw}\n"));
            }
            return vw as i32;
        }
        if index == SM_CYSCREEN || index == SM_CYFULLSCREEN || index == SM_CYVIRTUALSCREEN {
            if verbose() {
                log(&format!("GetSystemMetrics({index}) override: {res} -> {vh}\n"));
            }
            return vh as i32;
        }
    }
    res
}
unsafe extern "system" fn get_message_pos() -> u32 {
    let res = orig::<GetMessagePosFn>(&ORIG_GET_MESSAGE_POS).map_or(0, |f| f());
    if launcher_open() {
        return res;
    }
    if !game_hwnd().is_null() {
        if let Some(t) = cursor_transform() {
            let source = POINT {
                x: (res & 0xFFFF) as u16 as i16 as i32,
                y: (res >> 16) as u16 as i16 as i32,
            };
            let mapped = physical_to_virtual(source, &t);
            return (mapped.x as u16 as u32) | ((mapped.y as u16 as u32) << 16);
        }
    }
    res
}

/// InstallOSHooks (from every swapchain creation): exactly once -- a second pass would find its own detours in the
/// slots and keep them as the originals.
#[no_mangle]
pub unsafe extern "C" fn cvr_os_input_install() {
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    if verbose() {
        log("InstallOSHooks: Installing IAT hooks for user32.dll functions...\n");
    }
    let user32 = GetModuleHandleA(c"user32.dll".as_ptr() as *const u8);
    let hooks: [(&str, usize, &AtomicUsize); 10] = [
        ("GetCursorPos", get_cursor_pos as *const () as usize, &ORIG_GET_CURSOR_POS),
        ("SetCursorPos", set_cursor_pos as *const () as usize, &ORIG_SET_CURSOR_POS),
        ("SetWindowPos", set_window_pos as *const () as usize, &ORIG_SET_WINDOW_POS),
        ("MoveWindow", move_window as *const () as usize, &ORIG_MOVE_WINDOW),
        ("GetClientRect", get_client_rect as *const () as usize, &ORIG_GET_CLIENT_RECT),
        ("GetWindowRect", get_window_rect as *const () as usize, &ORIG_GET_WINDOW_RECT),
        ("GetCursorInfo", get_cursor_info as *const () as usize, &ORIG_GET_CURSOR_INFO),
        ("ClipCursor", clip_cursor as *const () as usize, &ORIG_CLIP_CURSOR),
        ("GetSystemMetrics", get_system_metrics as *const () as usize, &ORIG_GET_SYSTEM_METRICS),
        ("GetMessagePos", get_message_pos as *const () as usize, &ORIG_GET_MESSAGE_POS),
    ];
    if !user32.is_null() {
        for (name, _, slot) in hooks.iter() {
            let c = std::ffi::CString::new(*name).unwrap_or_default();
            let f = GetProcAddress(user32, c.as_ptr() as *const u8);
            slot.store(f.map_or(0, |f| f as usize), Ordering::Release);
        }
        if verbose() {
            log("InstallOSHooks: Dynamically resolved all user32 original functions.\n");
        }
    } else {
        log("InstallOSHooks: Warning: user32.dll not found in process!\n");
    }
    // BOTH modules: the game's, and our own (the overlay needs the same virtual answers the game gets).
    let main_exe = GetModuleHandleA(core::ptr::null()) as usize;
    let mut this_dll: *mut c_void = core::ptr::null_mut();
    GetModuleHandleExA(
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        cvr_os_input_install as *const u8,
        &mut this_dll,
    );
    for module in [main_exe, this_dll as usize] {
        if module == 0 {
            continue;
        }
        if verbose() {
            log(&format!("InstallOSHooks: Hooking module {}...\n", p(module as *const c_void)));
        }
        for (name, detour, slot) in hooks.iter() {
            hook_iat(module, "user32.dll", name, *detour, slot);
        }
    }
    // CET's Win32 ImGui backend gets the same virtual client and cursor as the game.
    let cet = GetModuleHandleA(c"cyber_engine_tweaks.asi".as_ptr() as *const u8) as usize;
    if cet != 0 {
        for (name, detour, slot) in [hooks[4], hooks[0], hooks[1]] {
            hook_iat(cet, "user32.dll", name, detour, slot);
        }
    } else {
        log("InstallOSHooks: CET module not loaded; CET cursor hooks unavailable\n");
    }
}

/// Our own cursor clip, in physical coordinates (past our ClipCursor hook, which would map it a second time).
unsafe fn set_physical_cursor_clip(rect: *const RECT) -> i32 {
    static CLIP: AtomicUsize = AtomicUsize::new(0);
    let mut f = CLIP.load(Ordering::Acquire);
    if f == 0 {
        let user32 = GetModuleHandleA(c"user32.dll".as_ptr() as *const u8);
        f = GetProcAddress(user32, c"ClipCursor".as_ptr() as *const u8).map_or(0, |f| f as usize);
        CLIP.store(f, Ordering::Release);
    }
    if f == 0 {
        return 0;
    }
    core::mem::transmute::<usize, ClipCursorFn>(f)(rect)
}
unsafe fn clamped_client_rect(hwnd: HWND) -> Option<RECT> {
    if hwnd.is_null() || IsWindow(hwnd) == 0 || IsIconic(hwnd) != 0 {
        return None;
    }
    let mut client = RECT::default();
    let get = orig::<RectFn>(&ORIG_GET_CLIENT_RECT).unwrap_or(windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect);
    if get(hwnd, &mut client) == 0 {
        return None;
    }
    let mut top_left = POINT { x: client.left, y: client.top };
    let mut bottom_right = POINT {
        x: client.right,
        y: client.bottom,
    };
    if ClientToScreen(hwnd, &mut top_left) == 0 || ClientToScreen(hwnd, &mut bottom_right) == 0 {
        return None;
    }
    let mut screen = RECT {
        left: top_left.x,
        top: top_left.y,
        right: bottom_right.x,
        bottom: bottom_right.y,
    };
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = core::mem::zeroed();
    mi.cbSize = core::mem::size_of::<MONITORINFO>() as u32;
    if !monitor.is_null() && GetMonitorInfoA(monitor, &mut mi) != 0 {
        let mut clipped = RECT::default();
        if IntersectRect(&mut clipped, &screen, &mi.rcMonitor) != 0 {
            screen = clipped;
        } else {
            screen = mi.rcMonitor;
        }
    }
    (screen.right > screen.left && screen.bottom > screen.top).then_some(screen)
}
/// UpdateCursorCapture (each Present): the cursor clipped to the game's client while it is in front, released for the
/// launcher and the overlay.
#[no_mangle]
pub unsafe extern "C" fn cvr_os_input_update_cursor_capture(hwnd: HWND) {
    let clipped = &mut *cvr_cpp_cursor_clipped();
    if launcher_open() || cvr_cpp_overlay_visible() {
        if *clipped {
            set_physical_cursor_clip(core::ptr::null());
            *clipped = false;
        }
        return;
    }
    if is_game_window_foreground(hwnd) {
        if let Some(rect) = clamped_client_rect(hwnd) {
            set_physical_cursor_clip(&rect);
            *clipped = true;
            return;
        }
    }
    if *clipped {
        set_physical_cursor_clip(core::ptr::null());
        *clipped = false;
    }
}
