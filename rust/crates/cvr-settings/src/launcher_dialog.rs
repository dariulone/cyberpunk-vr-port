//! The launcher window (formerly src/Overlay/LauncherDialog.cpp, main's since 2026-11): desktop or VR, the headset,
//! the runtime, one eye's render size (the ladder, a slider, - / + and typed fields), the VR features (the lobby, the
//! interface, hand physics, the weapon plane), the monitor's eye and the debug switch, picked before the game starts; Start writes them through the C++ half's setters, which persist
//! them (src/Core/VrCore.cpp). The ladders are [`crate::launcher_presets`].
//!
//! A dark window in the game's colours: near-black surfaces and the yellow of its UI as the one accent (the start
//! button, a focused field, the entry under the pointer, the debug switch on). Every control is drawn here. The game's
//! process runs without visual styles, so the stock combo boxes, check box and button came out in the classic look;
//! and a theme would not take these colours anyway. The combo boxes stay real combo boxes (their keyboard, their list,
//! the CB_ messages the launcher tests send): the closed field is painted by a subclass, the list's items are
//! owner-drawn. Shapes go through GDI+ (anti-aliased corners), text through GDI (ClearType), every size through the
//! window's DPI.
//!
//! Everything here runs on the thread that shows the window, so its state lives in plain cells; only "is the launcher
//! open" is read elsewhere (the input hooks), and that is an atomic.
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{
    GetLastError, COLORREF, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT,
    WAIT_FAILED, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Graphics::GdiPlus as gp;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, Sleep, INFINITE,
};
use windows_sys::Win32::System::WindowsProgramming::MulDiv;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE,
    TRACKMOUSEEVENT, VK_DOWN, VK_END, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::launcher_presets::{self as presets, Hmd, Resolution};

// ---- what the C++ half provides --------------------------------------------------------------------------------

/// The launcher's values, read and persisted by src/Core/VrCore.cpp, and the game's window.
#[repr(C)]
pub struct Runtime {
    game_hwnd: *const HWND,
    set_window_resolution: extern "C" fn(i32, i32),
    window_width: extern "C" fn() -> i32,
    window_height: extern "C" fn() -> i32,
    set_runtime_mode: extern "C" fn(i32),
    runtime_mode: extern "C" fn() -> i32,
    set_hmd_type: extern "C" fn(i32),
    hmd_type: extern "C" fn() -> i32,
    set_debug: extern "C" fn(i32),
    debug: extern "C" fn() -> i32,
    /// 1 when this launch renders both eyes through engine stereo.
    engine_stereo: extern "C" fn() -> i32,
    /// The saved pick is the player's own size, not a ladder entry (custom_res in vrport-launcher.ini).
    custom_resolution: extern "C" fn() -> i32,
    set_custom_resolution: extern "C" fn(i32),
    /// The VR lobby (xr_lobby), the VR interface (xr_vr_interface), hand physics (xr_hand_physics).
    lobby: extern "C" fn() -> i32,
    set_lobby: extern "C" fn(i32),
    interface: extern "C" fn() -> i32,
    set_interface: extern "C" fn(i32),
    hand_physics: extern "C" fn() -> i32,
    set_hand_physics: extern "C" fn(i32),
    /// Desktop or VR, and the weapon plane; set together at Start.
    vr_enabled: extern "C" fn() -> i32,
    weapon_plane: extern "C" fn() -> i32,
    set_modes: extern "C" fn(i32, i32),
}
/// The plugin's table, built here (formerly the C++ half's); a test program attaches its own.
#[cfg(feature = "plugin-tables")]
mod plugin {
    use windows_sys::Win32::Foundation::HWND;
    unsafe extern "C" {
        /// HWND g_gameHwnd (src/Hooks/OsInput.cpp).
        #[link_name = "?g_gameHwnd@@3PEAUHWND__@@EA"]
        static g_gameHwnd: HWND;
        safe fn SetWindowResolutionAndPersist(width: i32, height: i32);
        safe fn GetCurrentWindowWidth() -> i32;
        safe fn GetCurrentWindowHeight() -> i32;
        safe fn SetRuntimeModeAndPersist(mode: i32);
        safe fn GetXrRuntimeMode() -> i32;
        safe fn SetHmdTypeAndPersist(hmd_type: i32);
        safe fn GetCurrentHmdType() -> i32;
        safe fn SetLauncherDebugAndPersist(on: i32);
        safe fn GetLauncherDebug() -> i32;
        safe fn GetLauncherEngineStereo() -> i32;
        safe fn GetLauncherCustomResolution() -> i32;
        safe fn SetLauncherCustomResolution(on: i32);
        safe fn GetLauncherLobby() -> i32;
        safe fn SetLauncherLobbyAndPersist(on: i32);
        safe fn GetLauncherInterface() -> i32;
        safe fn SetLauncherInterfaceAndPersist(on: i32);
        safe fn GetLauncherHandPhysics() -> i32;
        safe fn SetLauncherHandPhysicsAndPersist(on: i32);
        safe fn GetLauncherVrEnabled() -> i32;
        safe fn GetLauncherWeaponPlane() -> i32;
        safe fn SetLauncherModes(vr: i32, plane: i32);
    }
    // SAFETY: the game window's global and plain functions.
    unsafe impl Sync for super::Runtime {}
    pub(super) static RT: super::Runtime = super::Runtime {
        game_hwnd: &raw const g_gameHwnd,
        set_window_resolution: SetWindowResolutionAndPersist,
        window_width: GetCurrentWindowWidth,
        window_height: GetCurrentWindowHeight,
        set_runtime_mode: SetRuntimeModeAndPersist,
        runtime_mode: GetXrRuntimeMode,
        set_hmd_type: SetHmdTypeAndPersist,
        hmd_type: GetCurrentHmdType,
        set_debug: SetLauncherDebugAndPersist,
        debug: GetLauncherDebug,
        engine_stereo: GetLauncherEngineStereo,
        custom_resolution: GetLauncherCustomResolution,
        set_custom_resolution: SetLauncherCustomResolution,
        lobby: GetLauncherLobby,
        set_lobby: SetLauncherLobbyAndPersist,
        interface: GetLauncherInterface,
        set_interface: SetLauncherInterfaceAndPersist,
        hand_physics: GetLauncherHandPhysics,
        set_hand_physics: SetLauncherHandPhysicsAndPersist,
        vr_enabled: GetLauncherVrEnabled,
        weapon_plane: GetLauncherWeaponPlane,
        set_modes: SetLauncherModes,
    };
}
#[cfg(feature = "plugin-tables")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "plugin-tables"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(null_mut());
/// Attached at the C++ half's static initialisation.
#[no_mangle]
pub extern "C" fn cvr_launcher_dialog_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Ordering::Release);
}
fn rt() -> Option<&'static Runtime> {
    // SAFETY: null or the C++ half's static.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}
fn engine_stereo() -> bool {
    rt().is_some_and(|r| (r.engine_stereo)() != 0)
}

// ---- the window's state ----------------------------------------------------------------------------------------

/// A value only the launcher's thread touches.
struct Ui<T>(Cell<T>);
// SAFETY: read and written on the thread that shows the window only (its window procedures and ShowLauncherDialog).
unsafe impl<T> Sync for Ui<T> {}
impl<T: Copy> Ui<T> {
    const fn new(v: T) -> Self {
        Self(Cell::new(v))
    }
    fn get(&self) -> T {
        self.0.get()
    }
    fn set(&self, v: T) {
        self.0.set(v)
    }
}
const NO_RECT: RECT = RECT {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};

static OPEN: AtomicBool = AtomicBool::new(false);
static RESTORE_GAME_FOCUS: Ui<bool> = Ui::new(false);
static DPI: Ui<u32> = Ui::new(96);
static GDIPLUS: Ui<usize> = Ui::new(0);
static FONT_TITLE: Ui<HFONT> = Ui::new(null_mut());
static FONT_SUB: Ui<HFONT> = Ui::new(null_mut());
static FONT_LABEL: Ui<HFONT> = Ui::new(null_mut());
static FONT_BODY: Ui<HFONT> = Ui::new(null_mut());
static FONT_BUTTON: Ui<HFONT> = Ui::new(null_mut());
static FONT_HINT: Ui<HFONT> = Ui::new(null_mut());
static BRUSH_BG: Ui<HBRUSH> = Ui::new(null_mut());
static BRUSH_SURFACE: Ui<HBRUSH> = Ui::new(null_mut());
static H_RUNTIME: Ui<HWND> = Ui::new(null_mut());
static H_HMD: Ui<HWND> = Ui::new(null_mut());
static H_RES: Ui<HWND> = Ui::new(null_mut());
static H_DEBUG: Ui<HWND> = Ui::new(null_mut());
static H_START: Ui<HWND> = Ui::new(null_mut());
/// The control under the pointer.
static HOT: Ui<HWND> = Ui::new(null_mut());
static DEBUG_ON: Ui<bool> = Ui::new(false);
static CLIENT_W: Ui<i32> = Ui::new(0);
static CLIENT_H: Ui<i32> = Ui::new(0);
// Painted by the window itself: the header and the field labels.
static RC_TITLE: Ui<RECT> = Ui::new(NO_RECT);
static RC_SUB: Ui<RECT> = Ui::new(NO_RECT);
static RC_RULE: Ui<RECT> = Ui::new(NO_RECT);
static RC_LABEL_HMD: Ui<RECT> = Ui::new(NO_RECT);
static RC_LABEL_RUNTIME: Ui<RECT> = Ui::new(NO_RECT);
static RC_LABEL_RES: Ui<RECT> = Ui::new(NO_RECT);
static RC_NOTE: Ui<RECT> = Ui::new(NO_RECT);
static H_MONITOR: Ui<HWND> = Ui::new(null_mut());
static RC_LABEL_MONITOR: Ui<RECT> = Ui::new(NO_RECT);
/// How the VR menu is called (crate::launcher::MENU_CALL).
static H_MENU_CALL: Ui<HWND> = Ui::new(null_mut());
static RC_LABEL_MENU_CALL: Ui<RECT> = Ui::new(NO_RECT);
/// The VR menu's calls in MENU_CALL's order: automatic, the thumbrest, the thumbstick, the right grip (each with the swipe).
const MENU_CALL_OPTIONS: [&str; 4] = ["Auto (thumbrest on Quest 3)", "Thumbrest + gesture", "Thumbstick + gesture", "Grip + gesture"];
// THE RENDER SIZE, three ways: the headset's ladder (the combo box), a slider with - / + beside it, and the width,
// height and aspect typed. The slider runs over the pixel count (log scale) at the pick's aspect, the ladder's entries
// marked on it; - / + step the width by RES_STEP_PX at that aspect; a typed width or height sets that side alone (the
// aspect follows), a typed aspect keeps the pixel count. A size that is not a ladder entry is the player's own: the
// combo box shows it as its last item ("Custom"), and it is saved with custom_res=1 so the startup snap leaves it
// alone. Engine stereo renders any size; without it the second eye is a VRCAM view of one of the ladder's sizes, so
// every control lands on the ladder entry nearest in cost.
static H_RES_DOWN: Ui<HWND> = Ui::new(null_mut());
static H_RES_UP: Ui<HWND> = Ui::new(null_mut());
static H_SLIDER: Ui<HWND> = Ui::new(null_mut());
static H_EDIT_W: Ui<HWND> = Ui::new(null_mut());
static H_EDIT_H: Ui<HWND> = Ui::new(null_mut());
static H_EDIT_ASPECT: Ui<HWND> = Ui::new(null_mut());
static H_LOBBY: Ui<HWND> = Ui::new(null_mut());
static H_INTERFACE: Ui<HWND> = Ui::new(null_mut());
static H_PHYSICS: Ui<HWND> = Ui::new(null_mut());
static LOBBY_ON: Ui<bool> = Ui::new(true);
static INTERFACE_ON: Ui<bool> = Ui::new(true);
static PHYSICS_ON: Ui<bool> = Ui::new(true);
static VR_ON: Ui<bool> = Ui::new(true);
static PLANE_ON: Ui<bool> = Ui::new(false);
static RC_FEATURES: Ui<RECT> = Ui::new(NO_RECT);
/// The pick the controls show.
static RES_W: Ui<i32> = Ui::new(0);
static RES_H: Ui<i32> = Ui::new(0);
/// The size asked for: the saved one, or the last one the player set; and whether it was the player's own.
static REF_W: Ui<i32> = Ui::new(0);
static REF_H: Ui<i32> = Ui::new(0);
static REF_CUSTOM: Ui<bool> = Ui::new(false);
/// Width / height the slider and the steps keep.
static RES_ASPECT: Ui<f64> = Ui::new(1.0);
static SLIDER_POS: Ui<i32> = Ui::new(0);
static SLIDER_DRAG: Ui<bool> = Ui::new(false);
/// The pixel counts at the slider's ends.
static SLIDER_LO: Ui<f64> = Ui::new(1.0);
static SLIDER_HI: Ui<f64> = Ui::new(2.0);
static RC_CAP_W: Ui<RECT> = Ui::new(NO_RECT);
static RC_CAP_H: Ui<RECT> = Ui::new(NO_RECT);
static RC_CAP_ASPECT: Ui<RECT> = Ui::new(NO_RECT);
static RC_FRAME_W: Ui<RECT> = Ui::new(NO_RECT);
static RC_FRAME_H: Ui<RECT> = Ui::new(NO_RECT);
static RC_FRAME_ASPECT: Ui<RECT> = Ui::new(NO_RECT);
const SLIDER_STEPS: i32 = 1000;
const RES_MIN_PX: i32 = 512;
const RES_MAX_PX: i32 = 8192;
const RES_STEP_PX: i32 = 32;
const SLIDER_CLASS: &str = "CyberpunkVRPortLauncherSlider";

const ID_COMBO_RES: i32 = 101;
const ID_BUTTON_START: i32 = 102;
const ID_COMBO_RUNTIME: i32 = 103;
const ID_COMBO_HMD: i32 = 104;
const ID_CHECK_DEBUG: i32 = 105;
const ID_BUTTON_RES_DOWN: i32 = 106;
const ID_BUTTON_RES_UP: i32 = 107;
const ID_SLIDER_RES: i32 = 108;
const ID_EDIT_WIDTH: i32 = 109;
const ID_EDIT_HEIGHT: i32 = 110;
const ID_EDIT_ASPECT: i32 = 111;
const ID_CHECK_LOBBY: i32 = 112;
const ID_CHECK_INTERFACE: i32 = 113;
const ID_CHECK_PHYSICS: i32 = 114;
const ID_CHECK_VR: i32 = 115;
const ID_CHECK_PLANE: i32 = 116;
const ID_COMBO_MONITOR: i32 = 117;
const ID_COMBO_MENU_CALL: i32 = 118;

/// The on / off switches' own check state (an owner-drawn button keeps none).
fn switch_state(id: i32) -> Option<&'static Ui<bool>> {
    match id {
        ID_CHECK_DEBUG => Some(&DEBUG_ON),
        ID_CHECK_LOBBY => Some(&LOBBY_ON),
        ID_CHECK_INTERFACE => Some(&INTERFACE_ON),
        ID_CHECK_PHYSICS => Some(&PHYSICS_ON),
        ID_CHECK_VR => Some(&VR_ON),
        ID_CHECK_PLANE => Some(&PLANE_ON),
        _ => None,
    }
}

/// The runtimes offered: the mode matches xr_runtime (0 the OpenXR default, 1 SteamVR).
const RUNTIME_OPTIONS: [(i32, &str); 2] =
    [(1, "OpenVR  (SteamVR)"), (0, "OpenXR  (VDXR, PimaxOpenXR)")];

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
const COL_BG: COLORREF = rgb(14, 16, 21);
const COL_SURFACE: COLORREF = rgb(23, 26, 33);
const COL_SURFACE_HOT: COLORREF = rgb(31, 35, 44);
const COL_BORDER: COLORREF = rgb(43, 48, 59);
const COL_BORDER_HOT: COLORREF = rgb(66, 73, 88);
const COL_TEXT: COLORREF = rgb(236, 238, 242);
const COL_MUTED: COLORREF = rgb(139, 147, 162);
const COL_ACCENT: COLORREF = rgb(252, 238, 10);
const COL_ACCENT_HOT: COLORREF = rgb(255, 246, 112);
const COL_ACCENT_DOWN: COLORREF = rgb(214, 201, 0);
const COL_ON_ACCENT: COLORREF = rgb(16, 17, 20);
const COL_LIST_TEXT: COLORREF = rgb(205, 209, 217);

const CLASS_NAME: &str = "CyberpunkVRPortLauncherClass";

/// `text` as a NUL-terminated wide string.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(core::iter::once(0)).collect()
}
fn log(text: &str) {
    cvr_host::log::write(text);
}

// ---- painting --------------------------------------------------------------------------------------------------

fn px(v: i32) -> i32 {
    // SAFETY: plain arithmetic.
    unsafe { MulDiv(v, DPI.get() as i32, 96) }
}

unsafe fn make_font(tenths_of_point: i32, weight: u32) -> HFONT {
    let face = wide("Segoe UI");
    // SAFETY: a font request; the face is a live wide string.
    unsafe {
        CreateFontW(
            -MulDiv(tenths_of_point, DPI.get() as i32, 720),
            0,
            0,
            0,
            weight as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH | FF_DONTCARE) as u32,
            face.as_ptr(),
        )
    }
}

/// GDI+'s `Color(255, R, G, B)`.
fn argb(c: COLORREF) -> u32 {
    0xFF00_0000 | (c & 0xFF) << 16 | (c >> 8 & 0xFF) << 8 | (c >> 16 & 0xFF)
}

/// GDI+'s `Graphics` on a DC, anti-aliased.
struct Graphics(*mut gp::GpGraphics);
impl Graphics {
    unsafe fn new(dc: HDC) -> Self {
        let mut g = null_mut();
        // SAFETY: the caller's DC.
        unsafe {
            gp::GdipCreateFromHDC(dc, &mut g);
            gp::GdipSetSmoothingMode(g, gp::SmoothingModeAntiAlias);
        }
        Self(g)
    }
}
impl Drop for Graphics {
    fn drop(&mut self) {
        // SAFETY: created above.
        unsafe { gp::GdipDeleteGraphics(self.0) };
    }
}
/// GDI+'s `Pen(Color, width)`.
struct Pen(*mut gp::GpPen);
impl Pen {
    unsafe fn new(color: COLORREF, width: f32) -> Self {
        let mut p = null_mut();
        // SAFETY: a new pen.
        unsafe { gp::GdipCreatePen1(argb(color), width, gp::UnitWorld, &mut p) };
        Self(p)
    }
}
impl Drop for Pen {
    fn drop(&mut self) {
        // SAFETY: created above.
        unsafe { gp::GdipDeletePen(self.0) };
    }
}

/// A rounded rectangle, filled and outlined (no outline when border == fill); GDI's own when GDI+ did not start.
unsafe fn rounded(dc: HDC, r: &RECT, radius: i32, fill: COLORREF, border: COLORREF) {
    // SAFETY: the caller's DC; every object made here is released here.
    unsafe {
        if GDIPLUS.get() == 0 {
            let brush = CreateSolidBrush(fill);
            let pen = CreatePen(PS_SOLID, 1, border);
            let old_brush = SelectObject(dc, brush);
            let old_pen = SelectObject(dc, pen);
            RoundRect(dc, r.left, r.top, r.right, r.bottom, radius * 2, radius * 2);
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
            DeleteObject(brush);
            DeleteObject(pen);
            return;
        }
        let g = Graphics::new(dc);
        let x = r.left as f32 + 0.5;
        let y = r.top as f32 + 0.5;
        let w = (r.right - r.left) as f32 - 1.0;
        let h = (r.bottom - r.top) as f32 - 1.0;
        let d = (2.0 * radius as f32).min(w).min(h);
        let mut path = null_mut();
        gp::GdipCreatePath(gp::FillModeAlternate, &mut path);
        gp::GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
        gp::GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
        gp::GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
        gp::GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
        gp::GdipClosePathFigure(path);
        let mut brush = null_mut();
        gp::GdipCreateSolidFill(argb(fill), &mut brush);
        gp::GdipFillPath(g.0, brush as *mut gp::GpBrush, path);
        if border != fill {
            let pen = Pen::new(border, 1.0);
            gp::GdipDrawPath(g.0, pen.0, path);
        }
        gp::GdipDeleteBrush(brush as *mut gp::GpBrush);
        gp::GdipDeletePath(path);
    }
}

/// The field's arrow: a chevron, pointing up while the list is open.
unsafe fn chevron(dc: HDC, cx: i32, cy: i32, up: bool, color: COLORREF) {
    let s = px(4) as f32;
    let t = if up { -1.0f32 } else { 1.0 };
    let p = [
        gp::PointF {
            X: cx as f32 - s,
            Y: cy as f32 - t * s / 2.0,
        },
        gp::PointF {
            X: cx as f32,
            Y: cy as f32 + t * s / 2.0,
        },
        gp::PointF {
            X: cx as f32 + s,
            Y: cy as f32 - t * s / 2.0,
        },
    ];
    // SAFETY: the caller's DC; every object made here is released here.
    unsafe {
        if GDIPLUS.get() == 0 {
            let pen = CreatePen(PS_SOLID, px(2), color);
            let old = SelectObject(dc, pen);
            let q = p.map(|p| POINT {
                x: p.X as i32,
                y: p.Y as i32,
            });
            Polyline(dc, q.as_ptr(), 3);
            SelectObject(dc, old);
            DeleteObject(pen);
            return;
        }
        let g = Graphics::new(dc);
        let pen = Pen::new(color, 1.6f32 * DPI.get() as f32 / 96.0);
        gp::GdipSetPenLineJoin(pen.0, gp::LineJoinRound);
        gp::GdipSetPenStartCap(pen.0, gp::LineCapRound);
        gp::GdipSetPenEndCap(pen.0, gp::LineCapRound);
        gp::GdipDrawLines(g.0, pen.0, p.as_ptr(), 3);
    }
}

/// `text` (NUL-terminated) in `r`, transparent, `spacing` pixels between characters.
unsafe fn text(
    dc: HDC,
    text: &[u16],
    r: RECT,
    font: HFONT,
    color: COLORREF,
    flags: u32,
    spacing: i32,
) {
    let mut r = r;
    // SAFETY: the caller's DC and a live string.
    unsafe {
        let old = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        SetTextCharacterExtra(dc, spacing);
        DrawTextW(dc, text.as_ptr(), -1, &mut r, flags | DT_NOPREFIX);
        SetTextCharacterExtra(dc, 0);
        SelectObject(dc, old);
    }
}

/// Paints `paint` into an off-screen copy of the window's client area, then onto `dc` in one go: no flicker.
unsafe fn buffered(hwnd: HWND, dc: HDC, paint: impl FnOnce(HDC, &RECT)) {
    let mut rc = NO_RECT;
    // SAFETY: the window's own DC; the copy is released here.
    unsafe {
        GetClientRect(hwnd, &mut rc);
        let mem = CreateCompatibleDC(dc);
        let bmp = CreateCompatibleBitmap(dc, rc.right.max(1), rc.bottom.max(1));
        let old = SelectObject(mem, bmp);
        paint(mem, &rc);
        BitBlt(dc, 0, 0, rc.right, rc.bottom, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
    }
}

unsafe fn set_hot(control: HWND) {
    if HOT.get() == control {
        return;
    }
    let previous = HOT.get();
    HOT.set(control);
    // SAFETY: the launcher's own controls.
    unsafe {
        if !previous.is_null() {
            InvalidateRect(previous, core::ptr::null(), 0);
        }
        if !control.is_null() {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: control,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut track);
            InvalidateRect(control, core::ptr::null(), 0);
        }
    }
}

unsafe fn send(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: one of the launcher's own controls.
    unsafe { SendMessageW(hwnd, msg, wparam, lparam) }
}

/// The text of a combo box's entry, NUL-terminated (just the NUL for none).
unsafe fn combo_item_text(combo: HWND, index: i32) -> Vec<u16> {
    // SAFETY: the launcher's own combo box; the buffer holds the length it reports and the NUL.
    unsafe {
        let length = if index < 0 {
            CB_ERR as LRESULT
        } else {
            send(combo, CB_GETLBTEXTLEN, index as WPARAM, 0)
        };
        if length == CB_ERR as LRESULT {
            return vec![0];
        }
        let mut text = vec![0u16; length as usize + 1];
        send(
            combo,
            CB_GETLBTEXT,
            index as WPARAM,
            text.as_mut_ptr() as LPARAM,
        );
        text[length as usize] = 0;
        text
    }
}

/// The closed combo box: a rounded field with the picked entry and a chevron. The accent outlines it while it has
/// the focus or its list is open.
unsafe fn paint_field(combo: HWND, dc: HDC, rc: &RECT) {
    // SAFETY: the launcher's own combo box and DC.
    unsafe {
        FillRect(dc, rc, BRUSH_BG.get());
        let open = send(combo, CB_GETDROPPEDSTATE, 0, 0) != 0;
        let focus = GetFocus() == combo;
        let hot = HOT.get() == combo;
        rounded(
            dc,
            rc,
            px(6),
            if hot || open {
                COL_SURFACE_HOT
            } else {
                COL_SURFACE
            },
            if focus || open {
                COL_ACCENT
            } else if hot {
                COL_BORDER_HOT
            } else {
                COL_BORDER
            },
        );
        let mut r = *rc;
        r.left += px(14);
        r.right -= px(40);
        let label = combo_item_text(combo, send(combo, CB_GETCURSEL, 0, 0) as i32);
        text(
            dc,
            &label,
            r,
            FONT_BODY.get(),
            if IsWindowEnabled(combo) != 0 { COL_TEXT } else { COL_MUTED },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            0,
        );
        chevron(
            dc,
            rc.right - px(20),
            (rc.top + rc.bottom) / 2,
            open,
            if focus || open { COL_ACCENT } else { COL_MUTED },
        );
    }
}

unsafe extern "system" fn field_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    // SAFETY: the subclassed combo box's own messages.
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = core::mem::zeroed::<PAINTSTRUCT>();
                let dc = BeginPaint(hwnd, &mut ps);
                buffered(hwnd, dc, |mem, rc| paint_field(hwnd, mem, rc));
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_PRINTCLIENT => {
                let mut rc = NO_RECT;
                GetClientRect(hwnd, &mut rc);
                paint_field(hwnd, wparam as HDC, &rc);
                return 0;
            }
            WM_ERASEBKGND => return 1,
            WM_MOUSEMOVE => set_hot(hwnd),
            WM_MOUSELEAVE => {
                if HOT.get() == hwnd {
                    set_hot(null_mut());
                }
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                InvalidateRect(hwnd, core::ptr::null(), 0);
                return result;
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(field_proc), 1);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// The open list: its one-pixel frame in the window's border colour instead of black.
unsafe extern "system" fn list_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    // SAFETY: the subclassed list's own messages.
    unsafe {
        if msg == WM_NCPAINT {
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            let dc = GetWindowDC(hwnd);
            let mut r = NO_RECT;
            GetWindowRect(hwnd, &mut r);
            OffsetRect(&mut r, -r.left, -r.top);
            let frame = CreateSolidBrush(COL_BORDER_HOT);
            FrameRect(dc, &r, frame);
            DeleteObject(frame);
            ReleaseDC(hwnd, dc);
            return result;
        }
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(list_proc), 1);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// The start button and the debug switch: pointer tracking, and the switch's own check state (an owner-drawn button
/// keeps none) behind BM_GETCHECK / BM_SETCHECK.
unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    // SAFETY: the subclassed button's own messages.
    unsafe {
        match msg {
            BM_GETCHECK => {
                if let Some(state) = switch_state(GetDlgCtrlID(hwnd)) {
                    return if state.get() {
                        BST_CHECKED as LRESULT
                    } else {
                        BST_UNCHECKED as LRESULT
                    };
                }
            }
            BM_SETCHECK => {
                if let Some(state) = switch_state(GetDlgCtrlID(hwnd)) {
                    state.set(wparam == BST_CHECKED as WPARAM);
                    if GetDlgCtrlID(hwnd) == ID_CHECK_VR && !H_MONITOR.get().is_null() {
                        EnableWindow(H_MONITOR.get(), VR_ON.get() as BOOL);
                    }
                    if GetDlgCtrlID(hwnd) == ID_CHECK_VR && !H_MENU_CALL.get().is_null() {
                        EnableWindow(H_MENU_CALL.get(), VR_ON.get() as BOOL);
                    }
                    InvalidateRect(hwnd, core::ptr::null(), 0);
                    return 0;
                }
            }
            WM_MOUSEMOVE => set_hot(hwnd),
            WM_MOUSELEAVE => {
                if HOT.get() == hwnd {
                    set_hot(null_mut());
                }
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(button_proc), 1);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

unsafe fn make_combo(parent: HWND, id: i32, x: i32, y: i32, w: i32) -> HWND {
    let empty = [0u16];
    // SAFETY: a child of the launcher window.
    unsafe {
        let combo = CreateWindowExW(
            0,
            WC_COMBOBOXW,
            empty.as_ptr(),
            (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32
                | WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_VSCROLL,
            x,
            y,
            w,
            px(300),
            parent,
            id as isize as HMENU,
            null_mut(),
            core::ptr::null(),
        );
        send(combo, WM_SETFONT, FONT_BODY.get() as WPARAM, 1);
        SetWindowSubclass(combo, Some(field_proc), 1, 0);
        let mut info = core::mem::zeroed::<COMBOBOXINFO>();
        info.cbSize = size_of::<COMBOBOXINFO>() as u32;
        if GetComboBoxInfo(combo, &mut info) != 0 && !info.hwndList.is_null() {
            SetWindowSubclass(info.hwndList, Some(list_proc), 1, 0);
        }
        combo
    }
}

unsafe fn make_button(parent: HWND, id: i32, label: &str, r: RECT) -> HWND {
    let class = wide("BUTTON");
    let label = wide(label);
    // SAFETY: a child of the launcher window.
    unsafe {
        let button = CreateWindowExW(
            0,
            class.as_ptr(),
            label.as_ptr(),
            WS_TABSTOP | WS_VISIBLE | WS_CHILD | BS_OWNERDRAW as u32,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            parent,
            id as isize as HMENU,
            null_mut(),
            core::ptr::null(),
        );
        send(button, WM_SETFONT, FONT_BODY.get() as WPARAM, 1);
        SetWindowSubclass(button, Some(button_proc), 1, 0);
        button
    }
}

/// The open list shows every entry it can (no scroll bar on a short ladder): a classic combo box takes the list's
/// height from its window's, a themed one from CB_SETMINVISIBLE.
unsafe fn size_list(combo: HWND) {
    // SAFETY: the launcher's own combo box.
    unsafe {
        let count = (send(combo, CB_GETCOUNT, 0, 0) as i32).max(1);
        let visible = count.min(20);
        send(combo, CB_SETMINVISIBLE, visible as WPARAM, 0);
        let mut r = NO_RECT;
        GetWindowRect(combo, &mut r);
        let item = send(combo, CB_GETITEMHEIGHT, 0, 0) as i32;
        SetWindowPos(
            combo,
            null_mut(),
            0,
            0,
            r.right - r.left,
            (r.bottom - r.top) + visible * item + 2,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

unsafe fn selected_hmd() -> Option<&'static Hmd> {
    let combo = H_HMD.get();
    // SAFETY: the launcher's own combo box.
    let index = if combo.is_null() {
        -1
    } else {
        unsafe { send(combo, CB_GETCURSEL, 0, 0) as i32 }
    };
    usize::try_from(index)
        .ok()
        .and_then(|i| presets::presets().get(i))
}

/// The ladder entry the resolution field shows (its item data: the entry's index in the headset's ladder).
unsafe fn selected_resolution() -> Option<&'static Resolution> {
    // SAFETY: the launcher's own combo boxes.
    unsafe {
        let hmd = selected_hmd();
        let combo = H_RES.get();
        let item = if combo.is_null() {
            -1
        } else {
            send(combo, CB_GETCURSEL, 0, 0) as i32
        };
        let hmd = hmd?;
        if item < 0 {
            return None;
        }
        let entry = send(combo, CB_GETITEMDATA, item as WPARAM, 0) as i32;
        usize::try_from(entry)
            .ok()
            .and_then(|e| hmd.ladder().get(e))
    }
}

// ---- the render size: the pick and the controls that show it ----------------------------------------------------

fn own_sizes_allowed() -> bool {
    engine_stereo()
}

/// Both sides multiples of 8, as every ladder entry.
fn round_side(v: f64) -> i32 {
    ((v / 8.0).round() as i32 * 8).clamp(RES_MIN_PX, RES_MAX_PX)
}

/// The offered ladder entry of exactly w x h.
fn ladder_entry_of(hmd: Option<&Hmd>, w: i32, h: i32) -> Option<usize> {
    let hmd = hmd?;
    let stereo = engine_stereo();
    hmd.ladder().iter().position(|r| presets::offered(r, stereo) && r.width == w && r.height == h)
}

unsafe fn combo_item_of_entry(entry: Option<usize>) -> i32 {
    let Some(entry) = entry else { return -1 };
    // SAFETY: the launcher's own combo box.
    unsafe {
        let count = send(H_RES.get(), CB_GETCOUNT, 0, 0) as i32;
        (0..count).find(|&item| send(H_RES.get(), CB_GETITEMDATA, item as WPARAM, 0) as i32 == entry as i32).unwrap_or(-1)
    }
}

fn cost(r: &Resolution) -> f64 {
    f64::from(r.width) * f64::from(r.height)
}

/// The slider's ends: the ladder's cheapest and dearest offered entries, with room on both sides for own sizes when
/// engine stereo takes them, and always the pick itself.
unsafe fn reset_slider_range() {
    // SAFETY: the launcher's own combo box.
    let hmd = unsafe { selected_hmd() };
    let (mut lo, mut hi) = (0.0f64, 0.0f64);
    if let Some(hmd) = hmd {
        let stereo = engine_stereo();
        for r in hmd.ladder().iter().filter(|r| presets::offered(r, stereo)) {
            let c = cost(r);
            lo = if lo > 0.0 { lo.min(c) } else { c };
            hi = hi.max(c);
        }
    }
    if hi <= 0.0 {
        lo = 1024.0 * 1024.0;
        hi = 6144.0 * 6144.0;
    }
    if own_sizes_allowed() {
        lo *= 0.5;
        hi *= 1.3;
    }
    let pick = f64::from(RES_W.get()) * f64::from(RES_H.get());
    if pick > 0.0 {
        lo = lo.min(pick);
        hi = hi.max(pick);
    }
    if hi <= lo * 1.01 {
        hi = lo * 2.0;
    }
    SLIDER_LO.set(lo);
    SLIDER_HI.set(hi);
}

fn slider_pos_of(cost: f64) -> i32 {
    let (lo, hi) = (SLIDER_LO.get().ln(), SLIDER_HI.get().ln());
    let t = (cost.ln() - lo) / (hi - lo);
    ((t * f64::from(SLIDER_STEPS)).round() as i32).clamp(0, SLIDER_STEPS)
}

fn slider_cost_at(pos: i32) -> f64 {
    let (lo, hi) = (SLIDER_LO.get().ln(), SLIDER_HI.get().ln());
    (lo + (hi - lo) * f64::from(pos) / f64::from(SLIDER_STEPS)).exp()
}

unsafe fn set_edit_text(edit: HWND, value: &str) {
    if !edit.is_null() {
        let text = wide(value);
        // SAFETY: the launcher's own edit control.
        unsafe { SetWindowTextW(edit, text.as_ptr()) };
    }
}

/// The fields show the pick (the aspect to four places).
unsafe fn show_pick_in_fields() {
    let aspect = cvr_crt::format(c"%.4f", &[cvr_crt::Arg::F(RES_ASPECT.get())]);
    // SAFETY: the launcher's own edit controls.
    unsafe {
        set_edit_text(H_EDIT_W.get(), &RES_W.get().to_string());
        set_edit_text(H_EDIT_H.get(), &RES_H.get().to_string());
        set_edit_text(H_EDIT_ASPECT.get(), &aspect.to_string_lossy());
    }
}

/// Drops the combo box's "Custom" item (item data -1).
unsafe fn drop_custom_items() {
    // SAFETY: the launcher's own combo box.
    unsafe {
        let combo = H_RES.get();
        let mut item = send(combo, CB_GETCOUNT, 0, 0) as i32 - 1;
        while item >= 0 {
            if (send(combo, CB_GETITEMDATA, item as WPARAM, 0) as i32) < 0 {
                send(combo, CB_DELETESTRING, item as WPARAM, 0);
            }
            item -= 1;
        }
    }
}

/// The combo box shows the pick: its ladder entry, or a last item "Custom  W x H" (item data -1) for an own size.
unsafe fn show_pick_in_combo() {
    let combo = H_RES.get();
    if combo.is_null() {
        return;
    }
    // SAFETY: the launcher's own combo box.
    unsafe {
        drop_custom_items();
        let entry = ladder_entry_of(selected_hmd(), RES_W.get(), RES_H.get());
        let mut item = combo_item_of_entry(entry);
        if item < 0 {
            let label = wide(&format!("Custom  {} x {}", RES_W.get(), RES_H.get()));
            item = send(combo, CB_ADDSTRING, 0, label.as_ptr() as LPARAM) as i32;
            send(combo, CB_SETITEMDATA, item as WPARAM, -1isize as LPARAM);
        }
        send(combo, CB_SETCURSEL, item as WPARAM, 0);
        size_list(combo);
        InvalidateRect(combo, core::ptr::null(), 0);
    }
}

unsafe fn repaint_resolution_text() {
    // SAFETY: the launcher's own window.
    unsafe {
        let parent = if H_RES.get().is_null() { null_mut() } else { GetParent(H_RES.get()) };
        if !parent.is_null() {
            InvalidateRect(parent, &RC_LABEL_RES.get(), 0);
            InvalidateRect(parent, &RC_NOTE.get(), 0);
        }
    }
}

/// Where a change to the pick came from: each source leaves its own control as the player set it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PickFrom {
    Ladder,
    Slider,
    Step,
    Side,
    Aspect,
}

/// Makes w x h the pick. Without engine stereo it lands on the ladder entry nearest in cost.
unsafe fn set_pick(w: i32, h: i32, from: PickFrom, aspect: f64) {
    // SAFETY: the launcher's own controls throughout.
    unsafe {
        let hmd = selected_hmd();
        let (mut w, mut h, mut aspect) = (round_side(f64::from(w)), round_side(f64::from(h)), aspect);
        if !own_sizes_allowed() && hmd.is_some() && ladder_entry_of(hmd, w, h).is_none() {
            if let Some(nearest) = hmd.and_then(|hmd| presets::nearest_cost_entry(hmd, w, h, engine_stereo())) {
                let r = &hmd.unwrap().ladder()[nearest];
                w = r.width;
                h = r.height;
                aspect = 0.0;
            }
        }
        RES_W.set(w);
        RES_H.set(h);
        if aspect > 0.0 {
            RES_ASPECT.set(aspect);
        } else if from != PickFrom::Slider && from != PickFrom::Step {
            RES_ASPECT.set(f64::from(w) / f64::from(h));
        }
        REF_W.set(w);
        REF_H.set(h);
        REF_CUSTOM.set(ladder_entry_of(hmd, w, h).is_none());
        if from != PickFrom::Ladder {
            show_pick_in_combo();
        } else if !H_RES.get().is_null() {
            // a ladder entry picked: an own size listed before goes from the list
            drop_custom_items();
            let item = combo_item_of_entry(ladder_entry_of(hmd, w, h));
            if item >= 0 {
                send(H_RES.get(), CB_SETCURSEL, item as WPARAM, 0);
            }
            size_list(H_RES.get());
        }
        if from != PickFrom::Slider {
            reset_slider_range();
            SLIDER_POS.set(slider_pos_of(f64::from(w) * f64::from(h)));
        }
        if !H_SLIDER.get().is_null() {
            InvalidateRect(H_SLIDER.get(), core::ptr::null(), 0);
        }
        show_pick_in_fields();
        repaint_resolution_text();
    }
}

/// The slider moved to `pos`: that pixel count at the kept aspect. Within 2 % of a ladder entry of the same aspect it
/// takes the entry itself (the marks hold the knob).
unsafe fn slider_moved(pos: i32) {
    SLIDER_POS.set(pos.clamp(0, SLIDER_STEPS));
    let (cost_at, aspect) = (slider_cost_at(SLIDER_POS.get()), RES_ASPECT.get());
    let mut w = round_side((cost_at * aspect).sqrt());
    let mut h = round_side(f64::from(w) / aspect);
    // SAFETY: the launcher's own controls.
    unsafe {
        if let Some(hmd) = selected_hmd() {
            let stereo = engine_stereo();
            for r in hmd.ladder().iter().filter(|r| presets::offered(r, stereo)) {
                let (c, a) = (cost(r), f64::from(r.width) / f64::from(r.height));
                if (c / cost_at).ln().abs() < 0.02 && (a / aspect - 1.0).abs() < 0.004 {
                    w = r.width;
                    h = r.height;
                }
            }
        }
        if !own_sizes_allowed() {
            set_pick(w, h, PickFrom::Ladder, 0.0); // the nearest entry; the knob jumps onto its mark
            SLIDER_POS.set(slider_pos_of(f64::from(RES_W.get()) * f64::from(RES_H.get())));
            show_pick_in_combo();
            return;
        }
        set_pick(w, h, PickFrom::Slider, 0.0);
    }
}

/// - / +: the width by RES_STEP_PX at the kept aspect; without engine stereo the next ladder entry by cost.
unsafe fn step_pick(direction: i32) {
    // SAFETY: the launcher's own controls.
    unsafe {
        let hmd = selected_hmd();
        if let (false, Some(hmd)) = (own_sizes_allowed(), hmd) {
            let now = f64::from(RES_W.get()) * f64::from(RES_H.get());
            let stereo = engine_stereo();
            let mut best: Option<&Resolution> = None;
            for r in hmd.ladder() {
                let c = cost(r);
                if !presets::offered(r, stereo) || if direction > 0 { c <= now } else { c >= now } {
                    continue;
                }
                if best.is_none_or(|b| if direction > 0 { c < cost(b) } else { c > cost(b) }) {
                    best = Some(r);
                }
            }
            if let Some(b) = best {
                set_pick(b.width, b.height, PickFrom::Ladder, 0.0);
            }
            show_pick_in_combo();
            return;
        }
        let w = round_side(f64::from(RES_W.get() + direction * RES_STEP_PX));
        set_pick(w, round_side(f64::from(w) / RES_ASPECT.get()), PickFrom::Step, 0.0);
    }
}

/// `_wtof`: the longest number at the text's start (after blanks); 0 when there is none.
fn leading_f64(text: &str) -> f64 {
    let t = text.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let s = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > s
    };
    let mut any = digits(&mut i);
    if i < b.len() && b[i] == b'.' {
        i += 1;
        any |= digits(&mut i);
    }
    if !any {
        return 0.0;
    }
    let mantissa = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if digits(&mut j) {
            i = j;
        } else {
            i = mantissa;
        }
    }
    t[..i].parse().unwrap_or(0.0)
}
/// `_wtoi`: the integer at the text's start (after blanks); 0 when there is none.
fn leading_i32(text: &str) -> i32 {
    let t = text.trim_start();
    let b = t.as_bytes();
    let mut i = usize::from(!b.is_empty() && (b[0] == b'+' || b[0] == b'-'));
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    t[..i].parse::<i64>().map_or(0, |v| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

/// "16:9", "16/9", "16x9", "0.926" or "0,926" -> width / height; 0 when it is none of them or out of range.
fn parse_aspect(text: &str) -> f64 {
    let text = text.replace(',', ".");
    let value = match text.find([':', '/', 'x', 'X']) {
        Some(split) => {
            let (a, b) = (leading_f64(&text[..split]), leading_f64(&text[split + 1..]));
            if b > 0.0 { a / b } else { 0.0 }
        }
        None => leading_f64(&text),
    };
    if (0.25..=4.0).contains(&value) { value } else { 0.0 }
}

unsafe fn window_text(hwnd: HWND) -> String {
    // SAFETY: the launcher's own control; the buffer holds the length it reports and the NUL.
    unsafe {
        let length = GetWindowTextLengthW(hwnd).max(0);
        let mut text = vec![0u16; length as usize + 1];
        let got = GetWindowTextW(hwnd, text.as_mut_ptr(), length + 1).max(0);
        String::from_utf16_lossy(&text[..got as usize])
    }
}

/// A field's text taken (Enter or the field left): a width or a height sets that side, an aspect keeps the pixel
/// count. Anything unreadable puts the pick back in the field.
unsafe fn commit_field(edit: HWND) {
    // SAFETY: the launcher's own controls.
    unsafe {
        let text = window_text(edit);
        if edit == H_EDIT_ASPECT.get() {
            let aspect = parse_aspect(&text);
            if aspect > 0.0 && (aspect - RES_ASPECT.get()).abs() > 1e-6 {
                let c = f64::from(RES_W.get()) * f64::from(RES_H.get());
                let w = round_side((c * aspect).sqrt());
                set_pick(w, round_side(f64::from(w) / aspect), PickFrom::Aspect, aspect);
                return;
            }
        } else {
            let value = leading_i32(&text);
            if value > 0 {
                let w = if edit == H_EDIT_W.get() { value } else { RES_W.get() };
                let h = if edit == H_EDIT_H.get() { value } else { RES_H.get() };
                if round_side(f64::from(w)) != RES_W.get() || round_side(f64::from(h)) != RES_H.get() {
                    set_pick(w, h, PickFrom::Side, 0.0);
                    return;
                }
            }
        }
        show_pick_in_fields();
    }
}

/// The resolution field lists the headset's ladder, the entries this launch offers, and the pick shows in every
/// resolution control. The size asked for (REF_*) carries over a change of headset: kept when it is in the new ladder
/// or the player's own (with engine stereo), else the entry nearest in cost, so the pick keeps its frame time.
unsafe fn populate_resolution_combo(hmd_index: i32) {
    let combo = H_RES.get();
    if combo.is_null() {
        return;
    }
    // SAFETY: the launcher's own combo box; the labels are the ladders' static wide strings.
    unsafe {
        send(combo, CB_RESETCONTENT, 0, 0);
        let all = presets::presets();
        let hmd_index = if hmd_index < 0 || hmd_index as usize >= all.len() {
            0
        } else {
            hmd_index as usize
        };
        let hmd = &all[hmd_index];
        let stereo = engine_stereo();
        for (i, r) in hmd.ladder().iter().enumerate() {
            if !presets::offered(r, stereo) {
                continue;
            }
            let item = send(combo, CB_ADDSTRING, 0, r.label.as_ptr() as LPARAM) as i32;
            send(combo, CB_SETITEMDATA, item as WPARAM, i as LPARAM);
        }
        let (mut w, mut h) = (REF_W.get(), REF_H.get());
        let keep_own = REF_CUSTOM.get() && own_sizes_allowed() && w > 0 && h > 0;
        if ladder_entry_of(Some(hmd), w, h).is_none() && !keep_own && hmd.resolution_count > 0 {
            // Not in this headset's ladder (another headset's pick, a size the ladders retired, or one only engine
            // stereo offers): the entry nearest in cost.
            let nearest = if w > 0 && h > 0 { presets::nearest_cost_entry(hmd, w, h, stereo) } else { None };
            let entry = nearest.map_or_else(|| send(combo, CB_GETITEMDATA, 0, 0) as i32, |n| n as i32);
            if let Some(r) = usize::try_from(entry).ok().and_then(|e| hmd.ladder().get(e)) {
                w = r.width;
                h = r.height;
            }
        }
        if w <= 0 || h <= 0 {
            w = 2048;
            h = 2048;
        }
        RES_W.set(w);
        RES_H.set(h);
        RES_ASPECT.set(f64::from(w) / f64::from(h));
        show_pick_in_combo();
        reset_slider_range();
        SLIDER_POS.set(slider_pos_of(f64::from(w) * f64::from(h)));
        if !H_SLIDER.get().is_null() {
            InvalidateRect(H_SLIDER.get(), core::ptr::null(), 0);
        }
        show_pick_in_fields();
        repaint_resolution_text();
    }
}

// ---- the slider -------------------------------------------------------------------------------------------------

fn slider_track(rc: &RECT) -> (i32, i32, i32) {
    let knob = px(9);
    (rc.left + knob + px(1), rc.right - knob - px(1), (rc.top + rc.bottom) / 2)
}

unsafe fn paint_slider(hwnd: HWND, dc: HDC, rc: &RECT) {
    // SAFETY: the slider's (buffered) DC.
    unsafe {
        FillRect(dc, rc, BRUSH_BG.get());
        let (left, right, cy) = slider_track(rc);
        let hot = HOT.get() == hwnd;
        let focus = GetFocus() == hwnd;
        let track = RECT { left, top: cy - px(3), right, bottom: cy + px(3) };
        rounded(dc, &track, px(3), COL_SURFACE, if hot { COL_BORDER_HOT } else { COL_BORDER });
        let x = left + MulDiv(right - left, SLIDER_POS.get(), SLIDER_STEPS);
        if x > left + px(2) {
            let fill = RECT { left, top: cy - px(3), right: x, bottom: cy + px(3) };
            rounded(dc, &fill, px(3), COL_ACCENT, COL_ACCENT);
        }
        // the ladder's entries, marked under the track
        if let Some(hmd) = selected_hmd() {
            let mark = CreateSolidBrush(COL_BORDER_HOT);
            let stereo = engine_stereo();
            for r in hmd.ladder().iter().filter(|r| presets::offered(r, stereo)) {
                let tx = left + MulDiv(right - left, slider_pos_of(cost(r)), SLIDER_STEPS);
                let tick = RECT { left: tx, top: cy + px(7), right: tx + px(1).max(1), bottom: cy + px(11) };
                FillRect(dc, &tick, mark);
            }
            DeleteObject(mark);
        }
        let k = px(9);
        let knob = RECT { left: x - k, top: cy - k, right: x + k, bottom: cy + k };
        let fill = if SLIDER_DRAG.get() || hot { COL_ACCENT_HOT } else { COL_TEXT };
        rounded(dc, &knob, k, fill, if focus { COL_ACCENT } else { fill });
        if focus && GDIPLUS.get() != 0 {
            let mut ring = knob;
            InflateRect(&mut ring, px(3), px(3));
            let g = Graphics::new(dc);
            let pen = Pen::new(COL_ACCENT, 1.5 * DPI.get() as f32 / 96.0);
            gp::GdipDrawEllipse(g.0, pen.0, ring.left as f32, ring.top as f32, (ring.right - ring.left - 1) as f32,
                (ring.bottom - ring.top - 1) as f32);
        }
    }
}

unsafe fn slider_to_point(hwnd: HWND, x: i32) {
    let mut rc = NO_RECT;
    // SAFETY: the slider's own window.
    unsafe {
        GetClientRect(hwnd, &mut rc);
        let (left, right, _) = slider_track(&rc);
        slider_moved(if right > left { MulDiv(x.clamp(left, right) - left, SLIDER_STEPS, right - left) } else { 0 });
    }
}

unsafe extern "system" fn slider_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let x = i32::from(lparam as u16 as i16);
    // SAFETY: the slider's own messages.
    unsafe {
        match msg {
            WM_GETDLGCODE => return DLGC_WANTARROWS as LRESULT,
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                let mut ps = core::mem::zeroed::<PAINTSTRUCT>();
                let dc = BeginPaint(hwnd, &mut ps);
                buffered(hwnd, dc, |mem, rc| paint_slider(hwnd, mem, rc));
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_LBUTTONDOWN => {
                SetFocus(hwnd);
                SetCapture(hwnd);
                SLIDER_DRAG.set(true);
                slider_to_point(hwnd, x);
                return 0;
            }
            WM_MOUSEMOVE => {
                set_hot(hwnd);
                if SLIDER_DRAG.get() {
                    slider_to_point(hwnd, x);
                }
                return 0;
            }
            WM_LBUTTONUP => {
                if SLIDER_DRAG.get() {
                    SLIDER_DRAG.set(false);
                    ReleaseCapture();
                    InvalidateRect(hwnd, core::ptr::null(), 0);
                }
                return 0;
            }
            WM_CAPTURECHANGED => {
                SLIDER_DRAG.set(false);
                InvalidateRect(hwnd, core::ptr::null(), 0);
                return 0;
            }
            WM_MOUSELEAVE => {
                if HOT.get() == hwnd {
                    set_hot(null_mut());
                }
                return 0;
            }
            WM_KEYDOWN => {
                // arrows: one step of the - / + buttons; Page Up / Down: a tenth of the slider; Home / End: its ends
                let key = wparam as u16;
                if key == VK_LEFT || key == VK_DOWN {
                    step_pick(-1);
                    return 0;
                } else if key == VK_RIGHT || key == VK_UP {
                    step_pick(1);
                    return 0;
                } else if key == VK_PRIOR {
                    slider_moved(SLIDER_POS.get() + SLIDER_STEPS / 10);
                    return 0;
                } else if key == VK_NEXT {
                    slider_moved(SLIDER_POS.get() - SLIDER_STEPS / 10);
                    return 0;
                } else if key == VK_HOME {
                    slider_moved(0);
                    return 0;
                } else if key == VK_END {
                    slider_moved(SLIDER_STEPS);
                    return 0;
                }
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                InvalidateRect(hwnd, core::ptr::null(), 0);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

// ---- the size fields ----------------------------------------------------------------------------------------------

/// Enter takes the field (not the dialog's Start), as does leaving it; focus outlines the field's frame.
unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    // SAFETY: the subclassed edit's own messages.
    unsafe {
        match msg {
            WM_GETDLGCODE => {
                if lparam != 0
                    && (*(lparam as *const MSG)).message == WM_KEYDOWN
                    && wparam == VK_RETURN as WPARAM
                {
                    return DLGC_WANTALLKEYS as LRESULT | DefSubclassProc(hwnd, msg, wparam, lparam);
                }
            }
            WM_KEYDOWN => {
                if wparam == VK_RETURN as WPARAM {
                    commit_field(hwnd);
                    send(hwnd, EM_SETSEL, 0, -1);
                    return 0;
                }
            }
            WM_CHAR => {
                if wparam == '\r' as WPARAM {
                    return 0;
                }
            }
            WM_SETFOCUS => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                PostMessageW(hwnd, EM_SETSEL, 0, -1);
                InvalidateRect(GetParent(hwnd), core::ptr::null(), 0);
                return result;
            }
            WM_KILLFOCUS => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                commit_field(hwnd);
                InvalidateRect(GetParent(hwnd), core::ptr::null(), 0);
                return result;
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(edit_proc), 1);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

unsafe fn make_edit(parent: HWND, id: i32, frame: RECT) -> HWND {
    let class = wide("EDIT");
    let empty = [0u16];
    let text_h = px(22);
    let top = (frame.top + frame.bottom - text_h) / 2;
    // SAFETY: a child of the launcher window.
    unsafe {
        let edit = CreateWindowExW(
            0,
            class.as_ptr(),
            empty.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_CENTER as u32 | ES_AUTOHSCROLL as u32,
            frame.left + px(8),
            top,
            (frame.right - frame.left) - px(16),
            text_h,
            parent,
            id as isize as HMENU,
            null_mut(),
            core::ptr::null(),
        );
        send(edit, WM_SETFONT, FONT_BODY.get() as WPARAM, 1);
        send(edit, EM_SETLIMITTEXT, 12, 0);
        SetWindowSubclass(edit, Some(edit_proc), 1, 0);
        edit
    }
}

unsafe fn paint_window(dc: HDC, rc: &RECT) {
    // SAFETY: the window's (buffered) DC.
    unsafe {
        FillRect(dc, rc, BRUSH_BG.get());
        text(
            dc,
            &wide("cyberpunk-vr-port"),
            RC_TITLE.get(),
            FONT_TITLE.get(),
            COL_TEXT,
            DT_LEFT | DT_TOP | DT_SINGLELINE,
            0,
        );
        text(
            dc,
            &wide("Choose desktop or VR and start the game."),
            RC_SUB.get(),
            FONT_SUB.get(),
            COL_MUTED,
            DT_LEFT | DT_TOP | DT_WORDBREAK,
            0,
        );
        let rule = CreateSolidBrush(COL_BORDER);
        FillRect(dc, &RC_RULE.get(), rule);
        DeleteObject(rule);

        let label = DT_LEFT | DT_BOTTOM | DT_SINGLELINE;
        text(dc, &wide("VR FEATURES"), RC_FEATURES.get(), FONT_LABEL.get(), COL_MUTED, label, px(1));
        text(dc, &wide("MONITOR OUTPUT"), RC_LABEL_MONITOR.get(), FONT_LABEL.get(), COL_MUTED, label, px(1));
        text(dc, &wide("VR MENU CALL"), RC_LABEL_MENU_CALL.get(), FONT_LABEL.get(), COL_MUTED, label, px(1));
        text(
            dc,
            &wide("VR HEADSET"),
            RC_LABEL_HMD.get(),
            FONT_LABEL.get(),
            COL_MUTED,
            label,
            px(1),
        );
        text(
            dc,
            &wide("VR RUNTIME"),
            RC_LABEL_RUNTIME.get(),
            FONT_LABEL.get(),
            COL_MUTED,
            label,
            px(1),
        );
        text(
            dc,
            &wide("RENDER RESOLUTION"),
            RC_LABEL_RES.get(),
            FONT_LABEL.get(),
            COL_MUTED,
            label,
            px(1),
        );
        if RES_W.get() > 0 && RES_H.get() > 0 {
            let cost = cvr_crt::format(
                c"%.1f MP per eye",
                &[cvr_crt::Arg::F(f64::from(RES_W.get()) * f64::from(RES_H.get()) / 1e6)],
            );
            text(
                dc,
                &wide(&cost.to_string_lossy()),
                RC_LABEL_RES.get(),
                FONT_HINT.get(),
                COL_MUTED,
                DT_RIGHT | DT_BOTTOM | DT_SINGLELINE,
                0,
            );
        }
        // the size fields: their captions and frames (the edits sit inside, borderless)
        text(dc, &wide("WIDTH"), RC_CAP_W.get(), FONT_LABEL.get(), COL_MUTED, label, px(1));
        text(dc, &wide("HEIGHT"), RC_CAP_H.get(), FONT_LABEL.get(), COL_MUTED, label, px(1));
        text(dc, &wide("ASPECT  W / H"), RC_CAP_ASPECT.get(), FONT_LABEL.get(), COL_MUTED, label, px(1));
        for (frame, edit) in [(&RC_FRAME_W, &H_EDIT_W), (&RC_FRAME_H, &H_EDIT_H), (&RC_FRAME_ASPECT, &H_EDIT_ASPECT)] {
            let frame = frame.get();
            if edit.get().is_null() || IsRectEmpty(&frame) != 0 {
                continue;
            }
            let focus = GetFocus() == edit.get();
            rounded(dc, &frame, px(6), COL_SURFACE, if focus { COL_ACCENT } else { COL_BORDER });
        }
        let hmd = selected_hmd();
        let lens = hmd
            .and_then(|h| h.ladder().first())
            .map_or(0.0, |r| f64::from(r.width) / f64::from(r.height));
        let note = if !own_sizes_allowed() {
            "Engine stereo is off: only the listed sizes.".to_string()
        } else if lens > 0.0 && (RES_ASPECT.get() / lens - 1.0).abs() > 0.005 {
            let lens = cvr_crt::format(c"%.4f", &[cvr_crt::Arg::F(lens)]);
            format!("Headset aspect {}; another renders past the lenses.", lens.to_string_lossy())
        } else {
            "Slide, step with - / +, or type a size or an aspect (16:9).".to_string()
        };
        text(
            dc,
            &wide(&note),
            RC_NOTE.get(),
            FONT_HINT.get(),
            COL_MUTED,
            DT_LEFT | DT_TOP | DT_SINGLELINE | DT_END_ELLIPSIS,
            0,
        );
    }
}

unsafe fn draw_start(d: &DRAWITEMSTRUCT) {
    // SAFETY: the button's DC for this paint.
    unsafe {
        FillRect(d.hDC, &d.rcItem, BRUSH_BG.get());
        let down = d.itemState & ODS_SELECTED != 0;
        let hot = HOT.get() == d.hwndItem;
        let fill = if down {
            COL_ACCENT_DOWN
        } else if hot {
            COL_ACCENT_HOT
        } else {
            COL_ACCENT
        };
        rounded(d.hDC, &d.rcItem, px(6), fill, fill);
        if d.itemState & ODS_FOCUS != 0 {
            let mut ring = d.rcItem;
            InflateRect(&mut ring, -px(3), -px(3));
            rounded(d.hDC, &ring, px(4), fill, COL_ON_ACCENT);
        }
        text(
            d.hDC,
            &wide("START GAME"),
            d.rcItem,
            FONT_BUTTON.get(),
            COL_ON_ACCENT,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            px(1),
        );
    }
}

/// An on / off switch with its title and a line under it (the debug mode, the VR features).
unsafe fn draw_switch(d: &DRAWITEMSTRUCT, on: bool, title_text: &str, sub_text: &str) {
    // SAFETY: the button's DC for this paint.
    unsafe {
        FillRect(d.hDC, &d.rcItem, BRUSH_BG.get());
        let hot = HOT.get() == d.hwndItem;
        let focus = d.itemState & ODS_FOCUS != 0;
        let h = px(22);
        let w = px(40);
        let top = (d.rcItem.top + d.rcItem.bottom - h) / 2;
        let track = RECT {
            left: d.rcItem.left,
            top,
            right: d.rcItem.left + w,
            bottom: top + h,
        };
        let fill = match (on, hot) {
            (true, true) => COL_ACCENT_HOT,
            (true, false) => COL_ACCENT,
            (false, true) => COL_SURFACE_HOT,
            (false, false) => COL_SURFACE,
        };
        let border = if on {
            fill
        } else if focus {
            COL_ACCENT
        } else if hot {
            COL_BORDER_HOT
        } else {
            COL_BORDER
        };
        rounded(d.hDC, &track, h / 2, fill, border);
        let knob = h - px(8);
        let kx = if on {
            track.right - px(4) - knob
        } else {
            track.left + px(4)
        };
        let dot = RECT {
            left: kx,
            top: top + px(4),
            right: kx + knob,
            bottom: top + px(4) + knob,
        };
        let knob_color = if on { COL_ON_ACCENT } else { COL_MUTED };
        rounded(d.hDC, &dot, knob / 2, knob_color, knob_color);
        if focus && on {
            let mut ring = track;
            InflateRect(&mut ring, px(2), px(2));
            let frame = CreateSolidBrush(COL_TEXT);
            FrameRect(d.hDC, &ring, frame);
            DeleteObject(frame);
        }
        let mut title = d.rcItem;
        title.left = track.right + px(14);
        title.bottom = (d.rcItem.top + d.rcItem.bottom) / 2 + px(1);
        text(
            d.hDC,
            &wide(title_text),
            title,
            FONT_BODY.get(),
            COL_TEXT,
            DT_LEFT | DT_BOTTOM | DT_SINGLELINE,
            0,
        );
        let mut sub = d.rcItem;
        sub.left = title.left;
        sub.top = title.bottom + px(1);
        text(
            d.hDC,
            &wide(sub_text),
            sub,
            FONT_HINT.get(),
            COL_MUTED,
            DT_LEFT | DT_TOP | DT_SINGLELINE | DT_END_ELLIPSIS,
            0,
        );
    }
}

/// - / + beside the slider: a small field-coloured button with its sign.
unsafe fn draw_step(d: &DRAWITEMSTRUCT, up: bool) {
    // SAFETY: the button's DC for this paint.
    unsafe {
        FillRect(d.hDC, &d.rcItem, BRUSH_BG.get());
        let hot = HOT.get() == d.hwndItem;
        let down = d.itemState & ODS_SELECTED != 0;
        rounded(
            d.hDC,
            &d.rcItem,
            px(6),
            if down || hot { COL_SURFACE_HOT } else { COL_SURFACE },
            if d.itemState & ODS_FOCUS != 0 {
                COL_ACCENT
            } else if hot {
                COL_BORDER_HOT
            } else {
                COL_BORDER
            },
        );
        let (cx, cy, s) = ((d.rcItem.left + d.rcItem.right) / 2, (d.rcItem.top + d.rcItem.bottom) / 2, px(6));
        let sign = CreateSolidBrush(if hot { COL_ACCENT } else { COL_TEXT });
        let t = px(2).max(1) / 2;
        let thick = px(2).max(2);
        let bar = RECT { left: cx - s, top: cy - t, right: cx + s, bottom: cy - t + thick };
        FillRect(d.hDC, &bar, sign);
        if up {
            let post = RECT { left: cx - t, top: cy - s, right: cx - t + thick, bottom: cy + s };
            FillRect(d.hDC, &post, sign);
        }
        DeleteObject(sign);
    }
}

unsafe fn draw_list_item(d: &DRAWITEMSTRUCT) {
    // SAFETY: the combo box's DC for this paint.
    unsafe {
        if d.itemState & ODS_COMBOBOXEDIT != 0 {
            // The combo box repaints its field straight to the screen on a pick or a focus change; the subclass
            // paints the whole field, so it just repaints.
            InvalidateRect(d.hwndItem, core::ptr::null(), 0);
            return;
        }
        if d.itemID == u32::MAX {
            return;
        }
        let selected = d.itemState & ODS_SELECTED != 0;
        let fill = CreateSolidBrush(if selected {
            COL_SURFACE_HOT
        } else {
            COL_SURFACE
        });
        FillRect(d.hDC, &d.rcItem, fill);
        DeleteObject(fill);
        if selected {
            let bar = RECT {
                left: d.rcItem.left,
                top: d.rcItem.top + px(6),
                right: d.rcItem.left + px(3),
                bottom: d.rcItem.bottom - px(6),
            };
            let accent = CreateSolidBrush(COL_ACCENT);
            FillRect(d.hDC, &bar, accent);
            DeleteObject(accent);
        }
        let mut r = d.rcItem;
        r.left += px(14);
        r.right -= px(8);
        let label = combo_item_text(d.hwndItem, d.itemID as i32);
        text(
            d.hDC,
            &label,
            r,
            FONT_BODY.get(),
            if selected { COL_TEXT } else { COL_LIST_TEXT },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            0,
        );
    }
}

unsafe fn start_game(hwnd: HWND) {
    let Some(rt) = rt() else { return };
    // SAFETY: the launcher's own window and controls.
    unsafe {
        if VR_ON.get() {
            let selection = send(H_MONITOR.get(), CB_GETCURSEL, 0, 0) as i32;
            if selection != CB_ERR {
                crate::launcher::MONITOR_OUTPUT.store(crate::launcher::parse_monitor_output(selection), Ordering::Relaxed);
            }
            let call = send(H_MENU_CALL.get(), CB_GETCURSEL, 0, 0) as i32;
            if call != CB_ERR {
                crate::launcher::MENU_CALL.store(crate::launcher::parse_menu_call(call), Ordering::Relaxed);
            }
        }
        (rt.set_modes)(VR_ON.get() as i32, PLANE_ON.get() as i32);
        if !VR_ON.get() {
            RESTORE_GAME_FOCUS.set(GetForegroundWindow() == hwnd);
            DestroyWindow(hwnd);
            return;
        }
        // a field still being typed in counts as taken
        let focus = GetFocus();
        if !focus.is_null() && (focus == H_EDIT_W.get() || focus == H_EDIT_H.get() || focus == H_EDIT_ASPECT.get()) {
            commit_field(focus);
        }
        let checked = |h: HWND| (send(h, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT) as i32;
        (rt.set_debug)(checked(H_DEBUG.get()));
        (rt.set_lobby)(checked(H_LOBBY.get()));
        (rt.set_interface)(checked(H_INTERFACE.get()));
        (rt.set_hand_physics)(checked(H_PHYSICS.get()));
        if let Some(hmd) = selected_hmd() {
            (rt.set_hmd_type)(hmd.mhd_type);
        }
        let r_idx = send(H_RUNTIME.get(), CB_GETCURSEL, 0, 0) as i32;
        if r_idx >= 0 && (r_idx as usize) < RUNTIME_OPTIONS.len() {
            (rt.set_runtime_mode)(RUNTIME_OPTIONS[r_idx as usize].0);
        }
        if let Some(res) = selected_resolution() {
            (rt.set_custom_resolution)(0);
            (rt.set_window_resolution)(res.width, res.height);
        } else if RES_W.get() > 0 && RES_H.get() > 0 {
            // the combo box's "Custom" item: the player's own size
            (rt.set_custom_resolution)(1);
            (rt.set_window_resolution)(RES_W.get(), RES_H.get());
        }
        RESTORE_GAME_FOCUS.set(GetForegroundWindow() == hwnd);
        DestroyWindow(hwnd);
    }
}

/// WM_CREATE: the fonts, the layout and the controls, every size at the window's DPI.
unsafe fn create(hwnd: HWND) {
    let Some(rt) = rt() else { return };
    // SAFETY: the new launcher window.
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        DPI.set(if dpi == 0 { 96 } else { dpi });
        FONT_TITLE.set(make_font(200, FW_SEMIBOLD));
        FONT_SUB.set(make_font(95, FW_NORMAL));
        FONT_LABEL.set(make_font(80, FW_SEMIBOLD));
        FONT_BODY.set(make_font(105, FW_NORMAL));
        FONT_BUTTON.set(make_font(105, FW_BOLD));
        FONT_HINT.set(make_font(90, FW_NORMAL));
        BRUSH_SURFACE.set(CreateSolidBrush(COL_SURFACE));
        HOT.set(null_mut());

        let margin = px(28);
        CLIENT_W.set(px(820));
        let right = px(392);
        let field_w = right - margin;
        let feature_left = px(440);
        let feature_right = CLIENT_W.get() - margin;
        let mut y = px(26);
        RC_TITLE.set(RECT {
            left: margin,
            top: y,
            right,
            bottom: y + px(38),
        });
        RC_SUB.set(RECT {
            left: margin,
            top: y + px(44),
            right: feature_right,
            bottom: y + px(84),
        });
        y += px(98);
        RC_RULE.set(RECT {
            left: margin,
            top: y,
            right: feature_right,
            bottom: y + 1,
        });
        y += px(22);

        // One field: its label, then the combo box (its closed height set by its owner-drawn field height).
        let mut field = |label_rect: &Ui<RECT>, id: i32| {
            label_rect.set(RECT {
                left: margin,
                top: y,
                right,
                bottom: y + px(18),
            });
            y += px(26);
            let combo = make_combo(hwnd, id, margin, y, field_w);
            let mut r = NO_RECT;
            GetWindowRect(combo, &mut r);
            y += (r.bottom - r.top) + px(20);
            combo
        };
        H_HMD.set(field(&RC_LABEL_HMD, ID_COMBO_HMD));
        let current_hmd = (rt.hmd_type)();
        let mut hmd_sel = 0usize;
        for (i, hmd) in presets::presets().iter().enumerate() {
            send(H_HMD.get(), CB_ADDSTRING, 0, hmd.label.as_ptr() as LPARAM);
            if hmd.mhd_type == current_hmd {
                hmd_sel = i;
            }
        }
        send(H_HMD.get(), CB_SETCURSEL, hmd_sel, 0);
        size_list(H_HMD.get());

        H_RUNTIME.set(field(&RC_LABEL_RUNTIME, ID_COMBO_RUNTIME));
        let current_runtime = (rt.runtime_mode)();
        let mut runtime_sel = 0usize;
        for (i, (mode, label)) in RUNTIME_OPTIONS.iter().enumerate() {
            let label = wide(label);
            send(H_RUNTIME.get(), CB_ADDSTRING, 0, label.as_ptr() as LPARAM);
            if *mode == current_runtime {
                runtime_sel = i;
            }
        }
        send(H_RUNTIME.get(), CB_SETCURSEL, runtime_sel, 0);
        size_list(H_RUNTIME.get());

        H_RES.set(field(&RC_LABEL_RES, ID_COMBO_RES));
        y -= px(8);
        // - [slider] +
        let (step_w, row_h) = (px(34), px(30));
        H_RES_DOWN.set(make_button(hwnd, ID_BUTTON_RES_DOWN, "-", RECT { left: margin, top: y, right: margin + step_w, bottom: y + row_h }));
        let slider_class = wide(SLIDER_CLASS);
        let empty = [0u16];
        H_SLIDER.set(CreateWindowExW(
            0,
            slider_class.as_ptr(),
            empty.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            margin + step_w + px(10),
            y,
            field_w - 2 * (step_w + px(10)),
            row_h,
            hwnd,
            ID_SLIDER_RES as isize as HMENU,
            null_mut(),
            core::ptr::null(),
        ));
        H_RES_UP.set(make_button(hwnd, ID_BUTTON_RES_UP, "+", RECT { left: right - step_w, top: y, right, bottom: y + row_h }));
        y += row_h + px(12);
        // WIDTH   HEIGHT   ASPECT: captions over borderless edits in painted frames
        let (gap, frame_h) = (px(12), px(34));
        let col_w = (field_w - 2 * gap) / 3;
        let col_x = [margin, margin + col_w + gap, right - col_w];
        RC_CAP_W.set(RECT { left: col_x[0], top: y, right: col_x[0] + col_w, bottom: y + px(16) });
        RC_CAP_H.set(RECT { left: col_x[1], top: y, right: col_x[1] + col_w, bottom: y + px(16) });
        RC_CAP_ASPECT.set(RECT { left: col_x[2], top: y, right: col_x[2] + col_w, bottom: y + px(16) });
        y += px(22);
        RC_FRAME_W.set(RECT { left: col_x[0], top: y, right: col_x[0] + col_w, bottom: y + frame_h });
        RC_FRAME_H.set(RECT { left: col_x[1], top: y, right: col_x[1] + col_w, bottom: y + frame_h });
        RC_FRAME_ASPECT.set(RECT { left: col_x[2], top: y, right: col_x[2] + col_w, bottom: y + frame_h });
        H_EDIT_W.set(make_edit(hwnd, ID_EDIT_WIDTH, RC_FRAME_W.get()));
        H_EDIT_H.set(make_edit(hwnd, ID_EDIT_HEIGHT, RC_FRAME_H.get()));
        H_EDIT_ASPECT.set(make_edit(hwnd, ID_EDIT_ASPECT, RC_FRAME_ASPECT.get()));
        if !own_sizes_allowed() {
            send(H_EDIT_ASPECT.get(), EM_SETREADONLY, 1, 0);
        }
        y += frame_h + px(8);
        RC_NOTE.set(RECT { left: margin, top: y, right, bottom: y + px(18) });
        y += px(18) + px(18);

        // the pick: the saved size (its ladder entry, or the player's own when engine stereo takes it)
        REF_W.set((rt.window_width)());
        REF_H.set((rt.window_height)());
        REF_CUSTOM.set((rt.custom_resolution)() != 0);
        populate_resolution_combo(hmd_sel as i32);

        // Independent feature switches retain each feature's detailed preferences.
        let mut fy = px(146);
        RC_FEATURES.set(RECT { left: feature_left, top: fy, right: feature_right, bottom: fy + px(18) });
        fy += px(30);
        LOBBY_ON.set((rt.lobby)() != 0);
        INTERFACE_ON.set((rt.interface)() != 0);
        PHYSICS_ON.set((rt.hand_physics)() != 0);
        VR_ON.set((rt.vr_enabled)() != 0);
        PLANE_ON.set((rt.weapon_plane)() != 0);
        let feature = |id: i32, label: &str, fy: i32| {
            make_button(hwnd, id, label, RECT { left: feature_left, top: fy, right: feature_right, bottom: fy + px(48) })
        };
        feature(ID_CHECK_VR, "Launch in VR", fy);
        fy += px(58);
        H_LOBBY.set(feature(ID_CHECK_LOBBY, "VR Lobby", fy));
        fy += px(58);
        H_INTERFACE.set(feature(ID_CHECK_INTERFACE, "VR Interface", fy));
        fy += px(58);
        H_PHYSICS.set(feature(ID_CHECK_PHYSICS, "Hand physics", fy));
        fy += px(58);
        feature(ID_CHECK_PLANE, "Weapon plane", fy);
        fy += px(64);
        RC_LABEL_MONITOR.set(RECT { left: feature_left, top: fy, right: feature_right, bottom: fy + px(18) });
        H_MONITOR.set(make_combo(hwnd, ID_COMBO_MONITOR, feature_left, fy + px(26), feature_right - feature_left));
        for label in ["Default", "Left eye", "Right eye"] {
            let label = wide(label);
            send(H_MONITOR.get(), CB_ADDSTRING, 0, label.as_ptr() as LPARAM);
        }
        send(H_MONITOR.get(), CB_SETCURSEL, crate::launcher::MONITOR_OUTPUT.load(Ordering::Relaxed) as WPARAM, 0);
        size_list(H_MONITOR.get());
        EnableWindow(H_MONITOR.get(), VR_ON.get() as BOOL);
        // the VR menu's call, under the monitor's output
        let mut r = NO_RECT;
        GetWindowRect(H_MONITOR.get(), &mut r);
        fy += px(26) + (r.bottom - r.top) + px(20);
        RC_LABEL_MENU_CALL.set(RECT { left: feature_left, top: fy, right: feature_right, bottom: fy + px(18) });
        H_MENU_CALL.set(make_combo(hwnd, ID_COMBO_MENU_CALL, feature_left, fy + px(26), feature_right - feature_left));
        for label in MENU_CALL_OPTIONS {
            let label = wide(label);
            send(H_MENU_CALL.get(), CB_ADDSTRING, 0, label.as_ptr() as LPARAM);
        }
        send(H_MENU_CALL.get(), CB_SETCURSEL, crate::launcher::MENU_CALL.load(Ordering::Relaxed) as WPARAM, 0);
        size_list(H_MENU_CALL.get());
        EnableWindow(H_MENU_CALL.get(), VR_ON.get() as BOOL);

        // Master switch for every probe, census and dump in the mod (cvr_host::debug_gate). Off is the normal way to
        // play: on, the log grows by tens of MB and the frame time with it, because probes like SightAxisProbe take a
        // mutex thousands of times a frame.
        DEBUG_ON.set((rt.debug)() != 0);
        H_DEBUG.set(make_button(
            hwnd,
            ID_CHECK_DEBUG,
            "Debug mode",
            RECT {
                left: margin,
                top: y,
                right,
                bottom: y + px(42),
            },
        ));
        y += px(42) + px(24);

        H_START.set(make_button(
            hwnd,
            ID_BUTTON_START,
            "Start Game",
            RECT {
                left: margin,
                top: y,
                right,
                bottom: y + px(48),
            },
        ));
        y += px(48) + px(28);
        // the right column's last field (the VR menu's call: its label's line, the closed box) when it runs longer
        CLIENT_H.set(y.max(fy + px(26) + px(40) + px(28)));
    }
}

unsafe extern "system" fn launcher_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the launcher window's own messages.
    unsafe {
        match msg {
            WM_CREATE => create(hwnd),
            WM_MEASUREITEM => {
                let m = &mut *(lparam as *mut MEASUREITEMSTRUCT);
                if m.CtlType == ODT_COMBOBOX {
                    // The closed field (item -1) sets the box's height; the classic frame adds 3 pixels a side.
                    m.itemHeight = if m.itemID == u32::MAX {
                        (px(40) - 6) as u32
                    } else {
                        px(32) as u32
                    };
                    return 1;
                }
            }
            WM_DRAWITEM => {
                let d = &*(lparam as *const DRAWITEMSTRUCT);
                if d.CtlType == ODT_COMBOBOX {
                    draw_list_item(d);
                } else if d.CtlID == ID_BUTTON_START as u32 {
                    draw_start(d);
                } else {
                    let on = |state: &Ui<bool>| state.get();
                    match d.CtlID as i32 {
                        ID_CHECK_DEBUG => draw_switch(d, on(&DEBUG_ON), "Debug mode", "Probes and a verbose log. Slower; for bug reports."),
                        ID_CHECK_VR => draw_switch(d, on(&VR_ON), "Launch in VR", "Turn off for the desktop game."),
                        ID_CHECK_PLANE => draw_switch(d, on(&PLANE_ON), "Weapon plane", "Draw equipped weapons in a separate plane."),
                        ID_CHECK_LOBBY => draw_switch(d, on(&LOBBY_ON), "VR Lobby", "The main menu in its own VR world."),
                        ID_CHECK_INTERFACE => draw_switch(d, on(&INTERFACE_ON), "VR Interface", "SAO menu, shops and storage in VR."),
                        ID_CHECK_PHYSICS => draw_switch(d, on(&PHYSICS_ON), "Hand physics", "Collisions, object/NPC grabs and climbing."),
                        ID_BUTTON_RES_DOWN | ID_BUTTON_RES_UP => draw_step(d, d.CtlID as i32 == ID_BUTTON_RES_UP),
                        _ => {}
                    }
                }
                return 1;
            }
            WM_CTLCOLORLISTBOX => {
                let dc = wparam as HDC;
                SetTextColor(dc, COL_TEXT);
                SetBkColor(dc, COL_SURFACE);
                return BRUSH_SURFACE.get() as LRESULT;
            }
            WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
                // the size fields (a read-only one sends the STATIC message): the frame's colour, muted when read-only
                let dc = wparam as HDC;
                let read_only = GetWindowLongPtrW(lparam as HWND, GWL_STYLE) & ES_READONLY as isize != 0;
                SetTextColor(dc, if read_only { COL_MUTED } else { COL_TEXT });
                SetBkColor(dc, COL_SURFACE);
                return BRUSH_SURFACE.get() as LRESULT;
            }
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                let mut ps = core::mem::zeroed::<PAINTSTRUCT>();
                let dc = BeginPaint(hwnd, &mut ps);
                buffered(hwnd, dc, |mem, rc| paint_window(mem, rc));
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_MOUSEMOVE => set_hot(null_mut()),
            WM_COMMAND => command(
                hwnd,
                (wparam >> 16 & 0xFFFF) as u32,
                (wparam & 0xFFFF) as i32,
                lparam,
            ),
            WM_CLOSE => {
                RESTORE_GAME_FOCUS.set(GetForegroundWindow() == hwnd);
                DestroyWindow(hwnd);
                return 0;
            }
            WM_DESTROY => {
                for font in [
                    &FONT_TITLE,
                    &FONT_SUB,
                    &FONT_LABEL,
                    &FONT_BODY,
                    &FONT_BUTTON,
                    &FONT_HINT,
                ] {
                    if !font.get().is_null() {
                        DeleteObject(font.get());
                        font.set(null_mut());
                    }
                }
                if !BRUSH_SURFACE.get().is_null() {
                    DeleteObject(BRUSH_SURFACE.get());
                    BRUSH_SURFACE.set(null_mut());
                }
                for h in [
                    &H_HMD, &H_RUNTIME, &H_MONITOR, &H_MENU_CALL, &H_RES, &H_DEBUG, &H_START, &HOT, &H_RES_DOWN, &H_RES_UP, &H_SLIDER,
                    &H_EDIT_W, &H_EDIT_H, &H_EDIT_ASPECT, &H_LOBBY, &H_INTERFACE, &H_PHYSICS,
                ] {
                    h.set(null_mut());
                }
                SLIDER_DRAG.set(false);
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

unsafe fn command(hwnd: HWND, notif: u32, id: i32, lparam: LPARAM) {
    // SAFETY: the launcher's own window and controls.
    unsafe {
        if id == ID_COMBO_HMD && notif == CBN_SELCHANGE {
            populate_resolution_combo(send(H_HMD.get(), CB_GETCURSEL, 0, 0) as i32);
            InvalidateRect(hwnd, &RC_LABEL_RES.get(), 0);
            return;
        }
        if id == ID_COMBO_RES && notif == CBN_SELCHANGE {
            if let Some(res) = selected_resolution() {
                set_pick(res.width, res.height, PickFrom::Ladder, 0.0);
            }
            InvalidateRect(hwnd, &RC_LABEL_RES.get(), 0);
        }
        if (notif == CBN_DROPDOWN || notif == CBN_CLOSEUP || notif == CBN_SELCHANGE)
            && (id == ID_COMBO_HMD || id == ID_COMBO_RUNTIME || id == ID_COMBO_RES || id == ID_COMBO_MONITOR || id == ID_COMBO_MENU_CALL)
        {
            InvalidateRect(lparam as HWND, core::ptr::null(), 0);
        }
        if (id == ID_BUTTON_RES_DOWN || id == ID_BUTTON_RES_UP) && notif == BN_CLICKED {
            step_pick(if id == ID_BUTTON_RES_UP { 1 } else { -1 });
        } else if (id == ID_CHECK_VR || id == ID_CHECK_PLANE) && notif == BN_CLICKED {
            if let Some(state) = switch_state(id) {
                state.set(!state.get());
            }
            if id == ID_CHECK_VR {
                EnableWindow(H_MONITOR.get(), VR_ON.get() as BOOL);
                EnableWindow(H_MENU_CALL.get(), VR_ON.get() as BOOL);
            }
            InvalidateRect(GetDlgItem(hwnd, id), core::ptr::null(), 0);
        } else if id == ID_CHECK_LOBBY && notif == BN_CLICKED {
            LOBBY_ON.set(!LOBBY_ON.get());
            InvalidateRect(H_LOBBY.get(), core::ptr::null(), 0);
        } else if id == ID_CHECK_INTERFACE && notif == BN_CLICKED {
            INTERFACE_ON.set(!INTERFACE_ON.get());
            InvalidateRect(H_INTERFACE.get(), core::ptr::null(), 0);
        } else if id == ID_CHECK_PHYSICS && notif == BN_CLICKED {
            PHYSICS_ON.set(!PHYSICS_ON.get());
            InvalidateRect(H_PHYSICS.get(), core::ptr::null(), 0);
        } else if id == ID_CHECK_DEBUG && notif == BN_CLICKED {
            DEBUG_ON.set(!DEBUG_ON.get());
            InvalidateRect(H_DEBUG.get(), core::ptr::null(), 0);
        } else if id == ID_BUTTON_START || id == IDOK {
            start_game(hwnd);
        } else if id == IDCANCEL {
            RESTORE_GAME_FOCUS.set(GetForegroundWindow() == hwnd);
            DestroyWindow(hwnd);
        }
    }
}

// ---- the C API (include/Overlay/LauncherDialog.hpp) ------------------------------------------------------------

/// IsLauncherOpen: 1 while the window is up (the input hooks let the pointer and the keyboard through to it).
#[no_mangle]
pub extern "C" fn cvr_launcher_is_open() -> i32 {
    OPEN.load(Ordering::Acquire) as i32
}

/// WaitForLauncherStartup: with the launcher off, the startup wait still pumps this thread's messages (the visible
/// launcher naturally pumps startup and resize messages; unattended, that chance is kept instead of sleeping the UI
/// thread). At most 10 s.
#[no_mangle]
pub extern "C" fn cvr_launcher_wait_startup(milliseconds: u32) {
    if milliseconds == 0 {
        return;
    }
    let milliseconds = milliseconds.min(10000);
    // SAFETY: this thread's message queue.
    unsafe {
        let deadline = GetTickCount64() + u64::from(milliseconds);
        while GetTickCount64() < deadline {
            let mut message = core::mem::zeroed::<MSG>();
            let mut n = 0;
            while n < 64 && PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                n += 1;
                if message.message == WM_QUIT {
                    PostQuitMessage(message.wParam as i32);
                    return;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
                if GetTickCount64() >= deadline {
                    return;
                }
            }
            let now = GetTickCount64();
            if now >= deadline {
                return;
            }
            let remaining = (deadline - now) as u32;
            if MsgWaitForMultipleObjectsEx(
                0,
                core::ptr::null(),
                remaining,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            ) == WAIT_FAILED
            {
                Sleep(remaining.min(10));
            }
        }
    }
}

/// While the window is up: the game's window unmapped (hiding keeps its size, unlike minimizing, and releases
/// Wine/X11 fullscreen grabs), the pointer free and shown. Undone when the window goes, the game's window shown
/// again and given the focus back if the launcher had it.
struct StartupWindow {
    game: HWND,
    visible: bool,
    same_thread: bool,
    cursor_shows: u32,
}
impl StartupWindow {
    unsafe fn new(game_hwnd: HWND) -> Self {
        // SAFETY: window queries and the pointer's state.
        unsafe {
            let mut pid = 0u32;
            let thread = GetWindowThreadProcessId(game_hwnd, &mut pid);
            let game = if thread != 0 && pid == GetCurrentProcessId() {
                game_hwnd
            } else {
                null_mut()
            };
            let same_thread = thread == GetCurrentThreadId();
            let visible = !game.is_null() && IsWindowVisible(game) != 0 && IsIconic(game) == 0;
            if visible {
                if same_thread {
                    ShowWindow(game, SW_HIDE);
                } else {
                    ShowWindowAsync(game, SW_HIDE);
                }
            }
            ClipCursor(core::ptr::null());
            ReleaseCapture();
            let mut cursor_shows = 0;
            loop {
                cursor_shows += 1;
                if !(ShowCursor(1) < 0 && cursor_shows < 16) {
                    break;
                }
            }
            Self {
                game,
                visible,
                same_thread,
                cursor_shows,
            }
        }
    }
}
impl Drop for StartupWindow {
    fn drop(&mut self) {
        OPEN.store(false, Ordering::Release);
        // SAFETY: the game's window and the pointer's state, as `new` left them.
        unsafe {
            if self.visible && IsWindow(self.game) != 0 {
                if self.same_thread {
                    ShowWindow(self.game, SW_SHOWNA);
                } else {
                    ShowWindowAsync(self.game, SW_SHOWNA);
                }
                if RESTORE_GAME_FOCUS.get() {
                    SetForegroundWindow(self.game);
                }
            }
            while self.cursor_shows != 0 {
                self.cursor_shows -= 1;
                ShowCursor(0);
            }
        }
    }
}

struct GdiplusEnd;
impl Drop for GdiplusEnd {
    fn drop(&mut self) {
        if GDIPLUS.get() != 0 {
            // SAFETY: started in cvr_launcher_show.
            unsafe { gp::GdiplusShutdown(GDIPLUS.get()) };
            GDIPLUS.set(0);
        }
    }
}

fn delete_bg_brush() {
    if !BRUSH_BG.get().is_null() {
        // SAFETY: created in cvr_launcher_show.
        unsafe { DeleteObject(BRUSH_BG.get()) };
        BRUSH_BG.set(null_mut());
    }
}

/// ShowLauncherDialog: the window on the game's monitor, modal on this thread until Start, Escape or close.
#[no_mangle]
pub extern "C" fn cvr_launcher_show() {
    if OPEN.swap(true, Ordering::AcqRel) {
        return;
    }
    RESTORE_GAME_FOCUS.set(false);
    // SAFETY: null or the C++ half's global, read on the thread that owns the game's window.
    let game_hwnd = rt().map_or(null_mut(), |r| unsafe { *r.game_hwnd });
    // SAFETY: Win32 on this thread; every object made here is released before returning.
    unsafe {
        let startup = StartupWindow::new(game_hwnd);
        BRUSH_BG.set(CreateSolidBrush(COL_BG));
        let input = gp::GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token = 0usize;
        GDIPLUS.set(
            if gp::GdiplusStartup(&mut token, &input, null_mut()) != gp::Ok {
                0
            } else {
                token
            },
        );
        let _gdiplus_end = GdiplusEnd;
        let class = wide(CLASS_NAME);
        let instance = GetModuleHandleW(core::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(launcher_wnd_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hbrBackground: BRUSH_BG.get(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..core::mem::zeroed()
        };
        let registered = RegisterClassW(&wc);
        if registered == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            log(&format!(
                "Launcher: RegisterClassW failed ({})\n",
                GetLastError()
            ));
            delete_bg_brush();
            return;
        }
        // the resolution slider's own class (painted and driven in slider_proc)
        let slider_class = wide(SLIDER_CLASS);
        let slider_wc = WNDCLASSW {
            lpfnWndProc: Some(slider_proc),
            hInstance: instance,
            lpszClassName: slider_class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_HAND),
            ..core::mem::zeroed()
        };
        struct SliderClassEnd(u16, Vec<u16>, windows_sys::Win32::Foundation::HINSTANCE);
        impl Drop for SliderClassEnd {
            fn drop(&mut self) {
                if self.0 != 0 {
                    // SAFETY: the class registered below, its windows gone with the launcher's.
                    unsafe { UnregisterClassW(self.1.as_ptr(), self.2) };
                }
            }
        }
        let _slider_class_end = SliderClassEnd(RegisterClassW(&slider_wc), slider_class.clone(), instance);

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let extended = WS_EX_TOPMOST | WS_EX_APPWINDOW;
        let mut monitor = core::mem::zeroed::<MONITORINFO>();
        monitor.cbSize = size_of::<MONITORINFO>() as u32;
        let mut work = NO_RECT;
        if GetMonitorInfoW(
            MonitorFromWindow(startup.game, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        ) != 0
        {
            work = monitor.rcWork;
        } else {
            SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut work).cast::<c_void>(), 0);
        }

        // Created on the game's monitor, then sized from the client area WM_CREATE laid out at that monitor's DPI.
        let title = wide("cyberpunk-vr-port");
        let hwnd = CreateWindowExW(
            extended,
            class.as_ptr(),
            title.as_ptr(),
            style,
            work.left,
            work.top,
            400,
            600,
            null_mut(),
            null_mut(),
            instance,
            core::ptr::null(),
        );
        if hwnd.is_null() {
            log(&format!(
                "Launcher: CreateWindowExW failed ({})\n",
                GetLastError()
            ));
            if registered != 0 {
                UnregisterClassW(class.as_ptr(), instance);
            }
            delete_bg_brush();
            return;
        }

        let mut rc = RECT {
            left: 0,
            top: 0,
            right: CLIENT_W.get(),
            bottom: CLIENT_H.get(),
        };
        AdjustWindowRectExForDpi(&mut rc, style, 0, extended, DPI.get());
        let win_w = rc.right - rc.left;
        let win_h = rc.bottom - rc.top;
        let x_pos = work.left + ((work.right - work.left - win_w) / 2).max(0);
        let y_pos = work.top + ((work.bottom - work.top - win_h) / 2).max(0);
        SetWindowPos(
            hwnd,
            null_mut(),
            x_pos,
            y_pos,
            win_w,
            win_h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        // A dark title bar (DWMWA_USE_IMMERSIVE_DARK_MODE: 20, 19 before Windows 10 20H1) in the window's colour
        // (DWMWA_CAPTION_COLOR / DWMWA_BORDER_COLOR, Windows 11). Older systems keep theirs.
        let dark: BOOL = 1;
        let dark_ptr = (&raw const dark).cast::<c_void>();
        if DwmSetWindowAttribute(hwnd, 20, dark_ptr, size_of::<BOOL>() as u32) < 0 {
            DwmSetWindowAttribute(hwnd, 19, dark_ptr, size_of::<BOOL>() as u32);
        }
        let caption: COLORREF = COL_BG;
        let frame: COLORREF = COL_BORDER;
        DwmSetWindowAttribute(
            hwnd,
            35,
            (&raw const caption).cast::<c_void>(),
            size_of::<COLORREF>() as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            34,
            (&raw const frame).cast::<c_void>(),
            size_of::<COLORREF>() as u32,
        );

        ShowWindow(hwnd, SW_SHOW);
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        SetForegroundWindow(hwnd);
        SetFocus(H_HMD.get());
        UpdateWindow(hwnd);

        let mut quit = false;
        let mut quit_code = 0;
        while IsWindow(hwnd) != 0 && !quit {
            if MsgWaitForMultipleObjectsEx(
                0,
                core::ptr::null(),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            ) == WAIT_FAILED
            {
                break;
            }
            let mut msg = core::mem::zeroed::<MSG>();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    quit = true;
                    quit_code = msg.wParam as i32;
                    break;
                }
                if IsDialogMessageW(hwnd, &msg) == 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if IsWindow(hwnd) == 0 {
                    break;
                }
            }
        }
        if IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
        if quit {
            PostQuitMessage(quit_code);
        }
        if registered != 0 {
            UnregisterClassW(class.as_ptr(), instance);
        }
        delete_bg_brush();
    }
}

/// main's tools/launcher_tests (the window's half), run against the Rust window: on a desktop of its own, a timer on
/// the window's own loop drives the controls as the C++ test drives LauncherDialog.cpp's, then Start (or a close)
/// ends each round; the setters below stand in for VrCore's and keep what they were given.
#[cfg(all(test, not(feature = "plugin-tables")))]
mod window_tests {
    use super::*;
    use core::sync::atomic::{AtomicI32, AtomicU64};
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::GENERIC_ALL;
    use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, CreateDesktopW, GetThreadDesktop, SetThreadDesktop};

    static SAVED_W: AtomicI32 = AtomicI32::new(2048);
    static SAVED_H: AtomicI32 = AtomicI32::new(2048);
    static SAVED_HMD: AtomicI32 = AtomicI32::new(0);
    static SAVED_RUNTIME: AtomicI32 = AtomicI32::new(0);
    static SAVED_DEBUG: AtomicI32 = AtomicI32::new(0);
    static SAVES: AtomicI32 = AtomicI32::new(0);
    static STEREO: AtomicI32 = AtomicI32::new(1);
    static CUSTOM: AtomicI32 = AtomicI32::new(0);
    static LOBBY: AtomicI32 = AtomicI32::new(1);
    static INTERFACE: AtomicI32 = AtomicI32::new(1);
    static PHYSICS: AtomicI32 = AtomicI32::new(1);
    static VR: AtomicI32 = AtomicI32::new(1);
    static PLANE: AtomicI32 = AtomicI32::new(0);
    static MODE: AtomicI32 = AtomicI32::new(0);
    static DEADLINE: AtomicU64 = AtomicU64::new(0);
    static TIMER: AtomicU64 = AtomicU64::new(0);
    static FAILURE: Mutex<String> = Mutex::new(String::new());
    static mut NO_GAME: HWND = null_mut();

    fn g(a: &AtomicI32) -> i32 {
        a.load(Ordering::Relaxed)
    }
    fn s(a: &AtomicI32, v: i32) {
        a.store(v, Ordering::Relaxed)
    }
    extern "C" fn set_window_resolution(w: i32, h: i32) {
        s(&SAVED_W, w);
        s(&SAVED_H, h);
        SAVES.fetch_add(1, Ordering::Relaxed);
    }
    extern "C" fn window_width() -> i32 { g(&SAVED_W) }
    extern "C" fn window_height() -> i32 { g(&SAVED_H) }
    extern "C" fn set_runtime_mode(v: i32) { s(&SAVED_RUNTIME, v) }
    extern "C" fn runtime_mode() -> i32 { g(&SAVED_RUNTIME) }
    extern "C" fn set_hmd_type(v: i32) { s(&SAVED_HMD, v) }
    extern "C" fn hmd_type() -> i32 { g(&SAVED_HMD) }
    extern "C" fn set_debug(v: i32) { s(&SAVED_DEBUG, v) }
    extern "C" fn debug() -> i32 { g(&SAVED_DEBUG) }
    extern "C" fn engine_stereo_on() -> i32 { g(&STEREO) }
    extern "C" fn custom() -> i32 { g(&CUSTOM) }
    extern "C" fn set_custom(v: i32) { s(&CUSTOM, v) }
    extern "C" fn lobby() -> i32 { g(&LOBBY) }
    extern "C" fn set_lobby(v: i32) { s(&LOBBY, v) }
    extern "C" fn interface() -> i32 { g(&INTERFACE) }
    extern "C" fn set_interface(v: i32) { s(&INTERFACE, v) }
    extern "C" fn physics() -> i32 { g(&PHYSICS) }
    extern "C" fn set_physics(v: i32) { s(&PHYSICS, v) }
    extern "C" fn vr_enabled() -> i32 { g(&VR) }
    extern "C" fn weapon_plane() -> i32 { g(&PLANE) }
    extern "C" fn set_modes(vr: i32, plane: i32) {
        s(&VR, vr);
        s(&PLANE, plane);
    }
    struct Table(Runtime);
    // SAFETY: plain functions and the address of a null window handle.
    unsafe impl Sync for Table {}
    static TABLE: Table = Table(Runtime {
        game_hwnd: &raw const NO_GAME,
        set_window_resolution,
        window_width,
        window_height,
        set_runtime_mode,
        runtime_mode,
        set_hmd_type,
        hmd_type,
        set_debug,
        debug,
        engine_stereo: engine_stereo_on,
        custom_resolution: custom,
        set_custom_resolution: set_custom,
        lobby,
        set_lobby,
        interface,
        set_interface,
        hand_physics: physics,
        set_hand_physics: set_physics,
        vr_enabled,
        weapon_plane,
        set_modes,
    });

    unsafe fn item(window: HWND, id: i32) -> HWND {
        unsafe { GetDlgItem(window, id) }
    }
    unsafe fn combo_text(combo: HWND, index: i32) -> String {
        let t = unsafe { combo_item_text(combo, index) };
        String::from_utf16_lossy(&t[..t.len() - 1])
    }
    unsafe fn select_hmd(window: HWND, index: i32) {
        unsafe {
            send(item(window, ID_COMBO_HMD), CB_SETCURSEL, index as WPARAM, 0);
            send(window, WM_COMMAND, (CBN_SELCHANGE as usize) << 16 | ID_COMBO_HMD as usize, item(window, ID_COMBO_HMD) as LPARAM);
        }
    }
    unsafe fn resolutions(window: HWND, headset: i32) -> Vec<String> {
        unsafe {
            select_hmd(window, headset);
            let combo = item(window, ID_COMBO_RES);
            (0..send(combo, CB_GETCOUNT, 0, 0) as i32).map(|i| combo_text(combo, i)).collect()
        }
    }
    unsafe fn type_field(window: HWND, id: i32, text: &str) {
        unsafe {
            let edit = item(window, id);
            let t = wide(text);
            SetWindowTextW(edit, t.as_ptr());
            send(edit, WM_KEYDOWN, VK_RETURN as WPARAM, 0);
        }
    }
    unsafe fn field_text(window: HWND, id: i32) -> String {
        unsafe { window_text(item(window, id)) }
    }
    unsafe fn field_int(window: HWND, id: i32) -> i32 {
        unsafe { field_text(window, id) }.parse().unwrap_or(0)
    }
    unsafe fn click(window: HWND, id: i32) {
        unsafe { send(item(window, id), BM_CLICK, 0, 0) };
    }
    unsafe fn checked(window: HWND, id: i32) -> bool {
        unsafe { send(item(window, id), BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT }
    }
    fn check(ok: bool, why: &str) -> Result<(), String> {
        if ok { Ok(()) } else { Err(why.to_string()) }
    }

    unsafe fn round(window: HWND, mode: i32) -> Result<(), String> {
        unsafe {
            let mut title = [0u16; 100];
            GetWindowTextW(window, title.as_mut_ptr(), 100);
            check(String::from_utf16_lossy(&title[..title.iter().position(|&c| c == 0).unwrap_or(0)]) == "cyberpunk-vr-port", "Unicode title was truncated")?;
            check(IsWindowVisible(window) != 0 && GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOPMOST as isize != 0, "launcher is not visible/topmost")?;
            cvr_launcher_show();
            check(IsWindow(window) != 0, "reentrant request destroyed the active launcher")?;
            match mode {
                0 => {
                    check(!item(window, ID_BUTTON_START).is_null() && !item(window, ID_COMBO_HMD).is_null(), "launcher controls missing")?;
                    let headset = item(window, ID_COMBO_HMD);
                    check(send(headset, CB_GETCOUNT, 0, 0) == 19, "headset count changed unexpectedly")?;
                    check(combo_text(headset, 3) == "Pico4" && combo_text(headset, 12) == "Bigscreen Beyond 2/2e", "existing headset IDs moved")?;
                    check(combo_text(headset, 13) == "Steam Frame" && combo_text(headset, 14) == "HP Reverb G2", "new headset entries missing")?;
                    let galaxy = resolutions(window, 16);
                    check(galaxy.len() == 3 && galaxy[0] == "4080 x 4768 (VD Rampage, native)" && galaxy[2] == "3552 x 4144 (VD Monster)", "Samsung Galaxy XR resolution ladder incorrect")?;
                    let (vive, vive_pro) = (resolutions(window, 17), resolutions(window, 18));
                    check(vive.len() == 8 && vive[0] == "1080 x 1200 (panel)" && vive[4] == "1512 x 1680 (SteamVR 100%)", "HTC Vive resolution ladder incorrect")?;
                    check(vive_pro.len() == 10 && vive_pro[0] == "1440 x 1600 (panel)" && vive_pro[6] == "2016 x 2240 (SteamVR 100%)", "HTC Vive Pro resolution ladder incorrect")?;
                    let (meganex, quest3) = (resolutions(window, 15), resolutions(window, 2));
                    check(meganex.len() == 11 && meganex[0] == "3824 x 3568 (cost of the 3552 x 3840 panel)" && meganex[10] == "6216 x 5792", "MeganeX superlight 8K resolution ladder incorrect")?;
                    check(quest3.len() == 10 && quest3[4] == "2648 x 2864", "Quest 3 resolution ladder incorrect")?;
                    let (pico, reverb, frame) = (resolutions(window, 3), resolutions(window, 14), resolutions(window, 13));
                    check(pico.len() == 11 && pico[0] == "2160 x 2160 (Native)" && pico[3] == "2352 x 2352" && pico[7] == "3384 x 3384", "Pico 4 resolution ladder incorrect")?;
                    check(reverb.len() == 11 && reverb[0] == "2192 x 2136 (cost of the 2160 x 2160 panel)" && reverb[10] == "6080 x 5928", "Reverb G2 resolution ladder incorrect")?;
                    check(frame.len() == 11 && frame[0] == "2152 x 2168 (cost of the 2160 x 2160 panel)" && frame[10] == "5984 x 6024", "Steam Frame resolution ladder incorrect")?;
                    // a saved size no ladder of this headset offers selects the entry nearest in cost
                    check(send(item(window, ID_COMBO_RES), CB_GETCURSEL, 0, 0) == 2, "retired 2048x2048 did not select the Steam Frame entry nearest in cost")?;
                    s(&STEREO, 0);
                    check(resolutions(window, 3).len() == 8, "engine-stereo steps listed without engine stereo")?;
                    s(&STEREO, 1);
                    select_hmd(window, 1);
                    send(item(window, ID_CHECK_DEBUG), BM_SETCHECK, BST_CHECKED as WPARAM, 0);
                    click(window, ID_BUTTON_START);
                }
                1 => {
                    PostMessageW(window, WM_CLOSE, 0, 0);
                }
                2 => PostQuitMessage(19),
                3 => {
                    select_hmd(window, 13);
                    send(item(window, ID_COMBO_RES), CB_SETCURSEL, 6, 0);
                    click(window, ID_BUTTON_START);
                    check(g(&SAVED_HMD) == 13 && g(&SAVED_W) == 3064 && g(&SAVED_H) == 3080, "Steam Frame selection was not saved")?;
                }
                4 => {
                    check(send(item(window, ID_COMBO_HMD), CB_GETCURSEL, 0, 0) == 13 && send(item(window, ID_COMBO_RES), CB_GETCURSEL, 0, 0) == 6, "Steam Frame selection did not reload")?;
                    select_hmd(window, 14);
                    send(item(window, ID_COMBO_RES), CB_SETCURSEL, 10, 0);
                    click(window, ID_BUTTON_START);
                    check(g(&SAVED_HMD) == 14 && g(&SAVED_W) == 6080 && g(&SAVED_H) == 5928, "Reverb G2 selection was not saved")?;
                }
                5 => {
                    check(send(item(window, ID_COMBO_HMD), CB_GETCURSEL, 0, 0) == 14 && send(item(window, ID_COMBO_RES), CB_GETCURSEL, 0, 0) == 10, "Reverb G2 selection did not reload")?;
                    check(field_text(window, ID_EDIT_WIDTH) == "6080" && field_text(window, ID_EDIT_HEIGHT) == "5928", "the size fields do not show the pick")?;
                    PostMessageW(window, WM_CLOSE, 0, 0);
                }
                6 => {
                    let monitor = item(window, ID_COMBO_MONITOR);
                    check(send(monitor, CB_GETCOUNT, 0, 0) == 3 && send(monitor, CB_GETCURSEL, 0, 0) == 0 && combo_text(monitor, 1) == "Left eye" && combo_text(monitor, 2) == "Right eye", "monitor choices missing")?;
                    send(monitor, CB_SETCURSEL, 1, 0);
                    // OWN SIZES (engine stereo): typed sides, a typed aspect, - / +, the slider's keys, the switches
                    select_hmd(window, 2); // Quest 3
                    let combo = item(window, ID_COMBO_RES);
                    let ladder = send(combo, CB_GETCOUNT, 0, 0) as i32;
                    type_field(window, ID_EDIT_WIDTH, "3001");
                    type_field(window, ID_EDIT_HEIGHT, "2999");
                    check(field_text(window, ID_EDIT_WIDTH) == "3000" && field_text(window, ID_EDIT_HEIGHT) == "3000", "typed sides were not rounded to multiples of 8")?;
                    check(send(combo, CB_GETCOUNT, 0, 0) as i32 == ladder + 1 && send(combo, CB_GETCURSEL, 0, 0) as i32 == ladder && combo_text(combo, ladder) == "Custom  3000 x 3000", "an own size is not the combo box's Custom item")?;
                    type_field(window, ID_EDIT_ASPECT, "16:9");
                    let (w, h) = (field_int(window, ID_EDIT_WIDTH), field_int(window, ID_EDIT_HEIGHT));
                    check(w % 8 == 0 && h % 8 == 0 && (f64::from(w) / f64::from(h) - 16.0 / 9.0).abs() < 0.01 && (f64::from(w) * f64::from(h) / 9e6 - 1.0).abs() < 0.02, "a typed aspect did not keep the pixel count")?;
                    check(field_text(window, ID_EDIT_ASPECT) == "1.7778", "the aspect field does not show the typed aspect")?;
                    click(window, ID_BUTTON_RES_UP);
                    check(field_int(window, ID_EDIT_WIDTH) == w + 32, "+ did not step the width by 32")?;
                    click(window, ID_BUTTON_RES_DOWN);
                    check(field_int(window, ID_EDIT_WIDTH) == w, "- did not step the width back")?;
                    send(item(window, ID_SLIDER_RES), WM_KEYDOWN, VK_PRIOR as WPARAM, 0);
                    check(field_int(window, ID_EDIT_WIDTH) > w, "Page Up on the slider did not grow the size")?;
                    let (sw, sh) = (field_int(window, ID_EDIT_WIDTH), field_int(window, ID_EDIT_HEIGHT));
                    check((f64::from(sw) / f64::from(sh) - 16.0 / 9.0).abs() < 0.01, "the slider did not keep the aspect")?;
                    // a ladder entry picked again: the Custom item goes
                    send(combo, CB_SETCURSEL, 4, 0);
                    send(window, WM_COMMAND, (CBN_SELCHANGE as usize) << 16 | ID_COMBO_RES as usize, combo as LPARAM);
                    check(send(combo, CB_GETCOUNT, 0, 0) as i32 == ladder && field_text(window, ID_EDIT_WIDTH) == "2648", "a ladder pick did not replace the own size")?;
                    type_field(window, ID_EDIT_HEIGHT, "2400");
                    send(item(window, ID_CHECK_LOBBY), BM_SETCHECK, BST_UNCHECKED as WPARAM, 0);
                    let mut left = NO_RECT;
                    GetWindowRect(item(window, ID_COMBO_HMD), &mut left);
                    for id in [ID_CHECK_LOBBY, ID_CHECK_INTERFACE, ID_CHECK_PHYSICS, ID_CHECK_VR, ID_CHECK_PLANE] {
                        let toggle = item(window, id);
                        check(!toggle.is_null(), "feature toggle missing")?;
                        let mut right_column = NO_RECT;
                        GetWindowRect(toggle, &mut right_column);
                        check(right_column.left > left.right, "feature toggle is not in the right column")?;
                    }
                    click(window, ID_CHECK_INTERFACE);
                    click(window, ID_CHECK_PHYSICS);
                    check(!checked(window, ID_CHECK_PLANE), "new installation enabled the weapon plane")?;
                    click(window, ID_CHECK_PLANE);
                    click(window, ID_BUTTON_START);
                    check(g(&SAVED_HMD) == 2 && g(&SAVED_W) == 2648 && g(&SAVED_H) == 2400 && g(&CUSTOM) == 1, "an own size was not saved as custom")?;
                    check(g(&LOBBY) == 0 && g(&INTERFACE) == 0 && g(&PHYSICS) == 0, "feature switches were not saved")?;
                    check(g(&VR) == 1 && g(&PLANE) == 1, "VR or weapon-plane selection lost")?;
                }
                7 => {
                    check(send(item(window, ID_COMBO_MONITOR), CB_GETCURSEL, 0, 0) == 1, "left eye did not reload")?;
                    send(item(window, ID_COMBO_MONITOR), CB_SETCURSEL, 2, 0);
                    // the own size reloads as the Custom item; without engine stereo it lands on the ladder
                    let combo = item(window, ID_COMBO_RES);
                    let count = send(combo, CB_GETCOUNT, 0, 0);
                    check(combo_text(combo, send(combo, CB_GETCURSEL, 0, 0) as i32) == "Custom  2648 x 2400" && count == 11, "the own size did not reload")?;
                    for id in [ID_CHECK_LOBBY, ID_CHECK_INTERFACE, ID_CHECK_PHYSICS] {
                        check(!checked(window, id), "feature switch did not reload")?;
                    }
                    s(&STEREO, 0);
                    select_hmd(window, 2);
                    type_field(window, ID_EDIT_WIDTH, "3000");
                    let (w, h) = (field_int(window, ID_EDIT_WIDTH), field_int(window, ID_EDIT_HEIGHT));
                    check(presets::snap(2, w, h, false).is_none(), "without engine stereo a typed size left the ladder")?;
                    s(&STEREO, 1);
                    for id in [ID_CHECK_LOBBY, ID_CHECK_INTERFACE, ID_CHECK_PHYSICS] {
                        send(item(window, id), BM_SETCHECK, BST_CHECKED as WPARAM, 0);
                    }
                    click(window, ID_BUTTON_START);
                    check(g(&CUSTOM) == 0 && g(&LOBBY) == 1 && g(&INTERFACE) == 1 && g(&PHYSICS) == 1, "feature re-enable was not saved")?;
                }
                8 => {
                    check(send(item(window, ID_COMBO_MONITOR), CB_GETCURSEL, 0, 0) == 2, "right eye did not reload")?;
                    send(item(window, ID_COMBO_MONITOR), CB_SETCURSEL, 0, 0);
                    check(checked(window, ID_CHECK_PLANE), "weapon plane choice did not reload")?;
                    click(window, ID_CHECK_VR);
                    click(window, ID_BUTTON_START);
                    check(g(&VR) == 0, "desktop selection was not saved")?;
                    check(crate::launcher::MONITOR_OUTPUT.load(Ordering::Relaxed) == 2, "desktop launch replaced VR mirror choice")?;
                }
                _ => {
                    check(IsWindowEnabled(item(window, ID_COMBO_MONITOR)) == 0, "desktop launch enabled eye selection")?;
                    check(!checked(window, ID_CHECK_VR), "desktop choice did not reload")?;
                    PostMessageW(window, WM_CLOSE, 0, 0);
                }
            }
            Ok(())
        }
    }

    unsafe extern "system" fn inspect(_: HWND, _: u32, _: usize, _: u32) {
        unsafe {
            let class = wide(CLASS_NAME);
            let window = FindWindowW(class.as_ptr(), core::ptr::null());
            if window.is_null() && GetTickCount64() < DEADLINE.load(Ordering::Relaxed) {
                return;
            }
            KillTimer(null_mut(), TIMER.load(Ordering::Relaxed) as usize);
            let result = if window.is_null() || cvr_launcher_is_open() == 0 {
                Err("launcher never became available".to_string())
            } else {
                round(window, g(&MODE))
            };
            if let Err(why) = result {
                *FAILURE.lock().unwrap() = why;
                if window.is_null() {
                    PostQuitMessage(20);
                } else {
                    PostMessageW(window, WM_CLOSE, 0, 0);
                }
            }
        }
    }

    #[test]
    fn the_window_as_mains_launcher_tests() {
        cvr_launcher_dialog_attach(&TABLE.0);
        // SAFETY: Win32 on this test's thread, on a desktop of its own (nothing shows on the player's).
        unsafe {
            let original = GetThreadDesktop(GetCurrentThreadId());
            let name = wide(&format!("CVRLauncherTests-{}", GetCurrentProcessId()));
            let desktop = CreateDesktopW(name.as_ptr(), core::ptr::null(), core::ptr::null(), 0, GENERIC_ALL, core::ptr::null());
            assert!(!desktop.is_null() && SetThreadDesktop(desktop) != 0, "cannot create isolated test desktop");
            for mode in 0..10 {
                s(&MODE, mode);
                DEADLINE.store(GetTickCount64() + 3000, Ordering::Relaxed);
                let timer = SetTimer(null_mut(), 0, 20, Some(inspect));
                assert!(timer != 0, "timer unavailable");
                TIMER.store(timer as u64, Ordering::Relaxed);
                cvr_launcher_show();
                KillTimer(null_mut(), timer);
                let failure = FAILURE.lock().unwrap().clone();
                assert!(failure.is_empty(), "mode {mode}: {failure}");
                assert_eq!(cvr_launcher_is_open(), 0, "mode {mode}: the launcher stayed open");
                let class = wide(CLASS_NAME);
                assert!(FindWindowW(class.as_ptr(), core::ptr::null()).is_null(), "mode {mode}: launcher window leaked");
                let mut message = core::mem::zeroed::<MSG>();
                let quit = PeekMessageW(&mut message, null_mut(), WM_QUIT, WM_QUIT, PM_REMOVE) != 0;
                if mode == 2 {
                    assert!(quit && message.wParam == 19, "application WM_QUIT was swallowed");
                } else {
                    assert!(!quit, "mode {mode}: closing launcher posted application WM_QUIT");
                }
            }
            assert!(g(&SAVES) == 5 && g(&SAVED_DEBUG) == 1, "Start did not save once per selection");
            assert!(SetThreadDesktop(original) != 0, "cannot restore test thread desktop");
            CloseDesktop(desktop);
        }
    }
}
