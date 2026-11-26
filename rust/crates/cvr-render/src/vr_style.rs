//! THE VR SHELL (formerly src/Overlay/VrStyle.cpp): the VR menu's ImGui window on its 1440x1080 canvas -- the game's
//! actions on the left, the settings tabs on the right, the pointer -- its style, the angular buttons, and the OVERLAY
//! tab, which the VR menu's record pass (Overlay/UiRec.hpp) shares. ImGui is reached through its C boundary
//! (crate::imgui); the panels' widgets through crate::ui; the VR panel's state is cvr-body's.
use core::ffi::CStr;
use core::sync::atomic::{AtomicI32, Ordering::Relaxed};

use cvr_body::vr_overlay::{self as vrui, Settings};
use cvr_settings::live_controls_ui_state::LiveControlsUiState;

use crate::imgui::{self, col32, Vec2, Vec4};
use crate::ui;

extern "C" {
    /// Tools: the tab to select on the next frame, then -1 (Overlay/ImGuiOverlay.cpp's draw-data dump).
    static mut CyberpunkVR_VrShellTab: i32;
}

/// cvr::vrui::CanvasWidth / CanvasHeight.
const CANVAS_WIDTH: f32 = 1440.0;
const CANVAS_HEIGHT: f32 = 1080.0;
const RED: Vec4 = [1.0, 0.28, 0.29, 1.0];
const YELLOW: Vec4 = [0.98, 0.94, 0.06, 1.0];
const CYAN: Vec4 = [0.30, 0.88, 0.94, 1.0];

/// A flat button with a cut corner, yellow when pointed at or selected; `left` aligns its text left and draws the
/// outline only then.
fn angular_button(label: &CStr, size: Vec2, selected: bool, left: bool) -> bool {
    let cursor = imgui::cursor_screen_pos();
    let pointed = imgui::is_mouse_hovering_rect(cursor, [cursor[0] + size[0], cursor[1] + size[1]]);
    imgui::push_style_color(
        imgui::COL_TEXT,
        if selected || pointed { YELLOW } else { RED },
    );
    imgui::push_style_color(
        imgui::COL_BUTTON,
        [0.10, 0.025, 0.035, if selected { 0.8 } else { 0.0 }],
    );
    imgui::push_style_color(imgui::COL_BUTTON_HOVERED, [0.19, 0.09, 0.025, 1.0]);
    imgui::push_style_color(imgui::COL_BUTTON_ACTIVE, [0.28, 0.14, 0.025, 1.0]);
    if left {
        imgui::push_style_var_vec2(imgui::VAR_BUTTON_TEXT_ALIGN, [0.06, 0.5]);
        imgui::push_style_var_f32(imgui::VAR_FRAME_BORDER_SIZE, 0.0);
    }
    let clicked = imgui::button(label, size);
    if left {
        imgui::pop_style_var(2);
    }
    let hot = imgui::is_item_hovered() || imgui::is_item_active();
    let a = imgui::item_rect_min();
    let b = imgui::item_rect_max();
    let d = imgui::window_draw_list();
    let color = imgui::color_u32(if hot || selected {
        YELLOW
    } else {
        [0.48, 0.13, 0.16, 0.75]
    });
    let points = [
        [a[0], a[1]],
        [b[0], a[1]],
        [b[0], b[1] - 10.0],
        [b[0] - 10.0, b[1]],
        [a[0], b[1]],
    ];
    if !left || hot || selected {
        d.polyline(
            &points,
            color,
            imgui::DRAW_CLOSED,
            if hot || selected { 2.0 } else { 1.0 },
        );
    }
    if hot || selected {
        d.rect_filled([a[0], a[1] + 8.0], [a[0] + 3.0, b[1] - 8.0], color);
    }
    imgui::pop_style_color(4);
    clicked
}

/// The snapshot's VR panel settings: cvr::vrui::Settings, held as 12 u32 of the same alignment.
fn overlay_settings(st: &mut LiveControlsUiState) -> &mut Settings {
    const _: () =
        assert!(core::mem::size_of::<Settings>() == 48 && core::mem::align_of::<Settings>() == 4);
    // SAFETY: the same 48 bytes the C++ half's Settings occupies, written by it as Settings.
    unsafe { &mut *(st.overlay.as_mut_ptr() as *mut Settings) }
}

/// DrawOverlayPage: the OVERLAY tab -- the panel's distance, size and text, the pointer hand, free look, centring and
/// resetting it -- then the FPS overlay's controls.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_overlay_page(state: *mut LiveControlsUiState) -> bool {
    // SAFETY: the caller's snapshot.
    let Some(st) = (unsafe { state.as_mut() }) else {
        return false;
    };
    let button = |label: &CStr, size: Vec2| {
        if crate::ui_rec::recording() {
            crate::ui_rec::button(label)
        } else {
            angular_button(label, size, false, false)
        }
    };
    let section = crate::vr_widgets::section;
    let mut changed = false;
    section(c"OVERLAY PANEL");
    {
        let v = overlay_settings(st);
        let (lo, hi) = (v.min_distance, v.max_distance);
        changed |= ui::slider_float(c"Distance", &mut v.distance, lo, hi, c"%.2f m");
        changed |= ui::slider_float(
            c"Minimum distance",
            &mut v.min_distance,
            0.35,
            2.0,
            c"%.2f m",
        );
        let lo = v.min_distance + 0.1;
        changed |= ui::slider_float(c"Maximum distance", &mut v.max_distance, lo, 5.0, c"%.2f m");
        changed |= ui::slider_float(c"Panel size", &mut v.scale, 0.6, 1.5, c"%.2fx");
        changed |= ui::slider_float(c"Text size", &mut v.font_scale, 0.8, 1.4, c"%.2fx");
        changed |= ui::combo_z(c"Pointer hand", &mut v.hand, b"Left\0Right\0Automatic\0\0");
        changed |= ui::checkbox(c"Free look", &mut v.follow);
        changed |= ui::slider_float(c"Free-look cone", &mut v.cone, 5.0, 90.0, c"%.0f deg");
        changed |= ui::slider_float(c"Distance speed", &mut v.depth_speed, 0.1, 2.0, c"%.2f m/s");
    }
    if button(c"CENTER PANEL", [260.0, 60.0]) {
        vrui::cvr_vrui_recenter();
        let mut now = Settings::default();
        // SAFETY: a slot to fill.
        unsafe { vrui::cvr_vrui_get_settings(&mut now) };
        *overlay_settings(st) = now;
        changed = true;
    }
    ui::same_line();
    if button(c"RESET", [200.0, 60.0]) {
        let v = overlay_settings(st);
        *v = Settings::default();
        // SAFETY: the settings just written.
        unsafe { vrui::cvr_vrui_set_settings(v) };
        vrui::cvr_vrui_recenter();
        changed = true;
    }
    ui::text_wrapped(
        c"Catch up after 3 seconds at rest when the offset is over 10 degrees inside the cone.",
        &[],
    );
    section(c"FPS OVERLAY");
    // SAFETY: the caller's snapshot.
    changed |= unsafe { crate::overlay_panels::cvr_draw_fps_overlay_controls(st) };
    changed
}

/// ApplyVrStyle: ImGui's style for the VR canvas -- big paddings and grabs for a pointer, square corners, the red,
/// yellow and cyan palette.
#[no_mangle]
pub extern "C" fn cvr_apply_vr_style() {
    use imgui::*;
    let mut s = Style::reset();
    s.set_vec2(Style::WINDOW_PADDING, [24.0, 22.0]);
    s.set_vec2(Style::FRAME_PADDING, [12.0, 10.0]);
    s.set_vec2(Style::ITEM_SPACING, [14.0, 12.0]);
    s.set_vec2(Style::ITEM_INNER_SPACING, [12.0, 10.0]);
    for rounding in [
        Style::POPUP_ROUNDING,
        Style::FRAME_ROUNDING,
        Style::CHILD_ROUNDING,
        Style::WINDOW_ROUNDING,
    ] {
        s.set_f32(rounding, 0.0);
    }
    s.set_f32(Style::SCROLLBAR_SIZE, 26.0);
    s.set_f32(Style::GRAB_MIN_SIZE, 30.0);
    s.set_f32(Style::WINDOW_BORDER_SIZE, 0.0);
    s.set_f32(Style::CHILD_BORDER_SIZE, 0.0);
    s.set_f32(Style::POPUP_BORDER_SIZE, 1.0);
    s.set_f32(Style::FRAME_BORDER_SIZE, 1.0);
    s.set_vec2(Style::TOUCH_EXTRA_PADDING, [3.0, 3.0]);
    s.set_f32(Style::INDENT_SPACING, 28.0);
    s.set_color(COL_TEXT, [1.0, 0.41, 0.42, 1.0]);
    s.set_color(COL_TEXT_DISABLED, [0.57, 0.29, 0.33, 1.0]);
    s.set_color(COL_WINDOW_BG, [0.027, 0.032, 0.055, 0.99]);
    s.set_color(COL_CHILD_BG, [0.043, 0.033, 0.053, 0.98]);
    s.set_color(COL_POPUP_BG, [0.055, 0.025, 0.04, 1.0]);
    s.set_color(COL_BORDER, [0.48, 0.12, 0.17, 0.85]);
    s.set_color(COL_FRAME_BG, [0.12, 0.027, 0.043, 1.0]);
    s.set_color(COL_FRAME_BG_HOVERED, [0.23, 0.065, 0.075, 1.0]);
    s.set_color(COL_FRAME_BG_ACTIVE, [0.28, 0.10, 0.05, 1.0]);
    s.set_color(COL_BUTTON, s.color(COL_FRAME_BG));
    s.set_color(COL_BUTTON_HOVERED, s.color(COL_FRAME_BG_HOVERED));
    s.set_color(COL_BUTTON_ACTIVE, s.color(COL_FRAME_BG_ACTIVE));
    s.set_color(COL_CHECK_MARK, CYAN);
    s.set_color(COL_SLIDER_GRAB, RED);
    s.set_color(COL_SLIDER_GRAB_ACTIVE, YELLOW);
    s.set_color(COL_HEADER, [0.16, 0.035, 0.055, 1.0]);
    s.set_color(COL_HEADER_HOVERED, [0.26, 0.08, 0.09, 1.0]);
    s.set_color(COL_HEADER_ACTIVE, s.color(COL_HEADER_HOVERED));
    s.set_color(COL_SEPARATOR, [0.52, 0.12, 0.16, 1.0]);
    s.set_color(COL_SCROLLBAR_BG, [0.035, 0.018, 0.025, 1.0]);
    s.set_color(COL_SCROLLBAR_GRAB, [0.62, 0.14, 0.18, 1.0]);
    s.set_color(COL_TEXT_SELECTED_BG, [0.36, 0.12, 0.08, 1.0]);
    s.set_color(COL_TABLE_BORDER_LIGHT, [0.43, 0.13, 0.17, 0.55]);
    s.set_color(COL_TABLE_BORDER_STRONG, [0.25, 0.72, 0.80, 0.65]);
}

/// cvr::vrui::MenuItems: the game's actions, one per line.
fn menu_items() -> Vec<std::ffi::CString> {
    let mut text = vec![0u8; 4096];
    loop {
        // SAFETY: a buffer of its length.
        let n = unsafe { vrui::cvr_vrui_menu_items(text.as_mut_ptr(), text.len()) };
        if n <= text.len() {
            text.truncate(n);
            break;
        }
        text = vec![0u8; n];
    }
    // std::getline's lines: none for an empty text, none after a final newline.
    let mut lines: Vec<&[u8]> = text.split(|&c| c == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|l| {
            std::ffi::CString::new(l.split(|&c| c == 0).next().unwrap_or(&[])).unwrap_or_default()
        })
        .collect()
}

/// The VR shell's selected tab: `static int selected=6;` (OVERLAY).
static SELECTED: AtomicI32 = AtomicI32::new(6);
const TABS: [&CStr; 7] = [
    c"GENERAL",
    c"FRAMEGEN",
    c"HUD",
    c"CONTROLS",
    c"STEREO",
    c"AVATAR",
    c"OVERLAY",
];

/// DrawVrShell: the VR menu's window on the canvas -- the title and CLOSE, the game's actions, the settings tabs and
/// the selected one, the controls' legend, then the pointer on top. True when the snapshot changed.
///
/// # Safety
/// `state` is the panel's LiveControlsUiState; `foveated_editing` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_draw_vr_shell(
    state: *mut LiveControlsUiState,
    foveated_editing: *mut bool,
) -> bool {
    let mut selected = SELECTED.load(Relaxed);
    // SAFETY: an export of the C++ half, written by tools.
    unsafe {
        let tab = core::ptr::read_volatile(&raw const CyberpunkVR_VrShellTab);
        if (0..7).contains(&tab) {
            selected = tab;
            core::ptr::write_volatile(&raw mut CyberpunkVR_VrShellTab, -1);
        }
    }
    imgui::set_next_window_pos([0.0, 0.0]);
    imgui::set_next_window_size([CANVAS_WIDTH, CANVAS_HEIGHT]);
    imgui::begin(
        c"##VR controls",
        imgui::WINDOW_NO_DECORATION | imgui::WINDOW_NO_MOVE | imgui::WINDOW_NO_SAVED_SETTINGS,
    );
    let background = imgui::window_draw_list();
    background.rect_filled_multicolor(
        [12.0, 12.0],
        [358.0, CANVAS_HEIGHT - 90.0],
        col32(60, 10, 18, 130),
        col32(22, 12, 22, 60),
        col32(14, 10, 18, 60),
        col32(46, 6, 14, 100),
    );
    background.line(
        [12.0, 12.0],
        [12.0, CANVAS_HEIGHT - 91.0],
        col32(242, 70, 75, 240),
        2.0,
    );
    imgui::text_colored(YELLOW, c"cyberpunk-vr-port");
    imgui::same_line(1170.0, -1.0);
    if angular_button(c"CLOSE##close_overlay", [220.0, 55.0], false, false) {
        vrui::cvr_vrui_close(true);
    }
    {
        let a = imgui::item_rect_min();
        let b = imgui::item_rect_max();
        let center = [b[0] - 25.0, (a[1] + b[1]) * 0.5];
        let color = imgui::color_u32(if imgui::is_item_hovered() {
            YELLOW
        } else {
            RED
        });
        background.line(
            [center[0] - 7.0, center[1] - 7.0],
            [center[0] + 7.0, center[1] + 7.0],
            color,
            2.5,
        );
        background.line(
            [center[0] - 7.0, center[1] + 7.0],
            [center[0] + 7.0, center[1] - 7.0],
            color,
            2.5,
        );
    }
    imgui::separator();
    imgui::begin_child(c"game-actions", [335.0, -78.0], 0, 0);
    imgui::push_style_var_vec2(imgui::VAR_ITEM_SPACING, [14.0, 8.0]);
    for (i, item) in menu_items().iter().enumerate() {
        imgui::push_id_int(i as i32);
        let width = imgui::calc_text_size(item)[0];
        let fit = (285.0 / if 1.0 < width { width } else { 1.0 }).min(1.0);
        imgui::set_window_font_scale(fit);
        if angular_button(item, [311.0, 60.0], false, true) {
            vrui::cvr_vrui_select_menu_item(i as u32);
        }
        imgui::set_window_font_scale(1.0);
        imgui::pop_id();
    }
    imgui::pop_style_var(1);
    imgui::end_child();
    imgui::same_line(0.0, -1.0);
    imgui::begin_child(c"vr-settings", [0.0, -78.0], 0, 0);
    let mut occupied = 0.0f32;
    let available = imgui::content_region_avail()[0];
    for (i, tab) in TABS.iter().enumerate() {
        let i = i as i32;
        let width = imgui::calc_text_size(tab)[0] + 30.0;
        if i != 0 && occupied + 14.0 + width <= available {
            imgui::same_line(0.0, -1.0);
            occupied += 14.0;
        } else {
            occupied = 0.0;
        }
        if angular_button(tab, [width, 54.0], i == selected, false) {
            selected = i;
        }
        occupied += width;
    }
    SELECTED.store(selected, Relaxed);
    imgui::separator();
    imgui::begin_child(c"content", [0.0, 0.0], 0, 0);
    imgui::push_item_width(360.0);
    let changed = if selected == 6 {
        // SAFETY: the caller's snapshot.
        unsafe { cvr_draw_overlay_page(state) }
    } else {
        // SAFETY: the caller's snapshot and flag.
        unsafe { crate::overlay_panels::cvr_draw_live_controls(state, selected, foveated_editing) }
    };
    imgui::pop_item_width();
    imgui::end_child();
    imgui::end_child();
    imgui::separator();
    imgui::text_colored(
        CYAN,
        c"TRIGGER  Select     STICK  Scroll     GRIP  Drag     GRIP + STICK  Distance",
    );
    imgui::text_disabled(c"F10 / INSERT  Toggle     ESC  Close     L3 + R3  Hold 0.5 sec  VR menu");
    imgui::end();
    let draw = imgui::foreground_draw_list();
    draw.rect_filled([0.0, 0.0], [4.0, 4.0], col32(76, 224, 240, 255));
    let p = imgui::mouse_pos();
    if p[0] >= 0.0 && p[0] < CANVAS_WIDTH && p[1] >= 0.0 && p[1] < CANVAS_HEIGHT {
        // SAFETY: a plain value filled by the call.
        let mut view: vrui::View = unsafe { core::mem::zeroed() };
        // SAFETY: a slot to fill.
        unsafe { vrui::cvr_vrui_get_view(&mut view) };
        let color = if view.dragging {
            col32(252, 240, 16, 255)
        } else {
            col32(76, 224, 240, 255)
        };
        let (a, b, c) = (p, [p[0] + 23.0, p[1] + 8.0], [p[0] + 11.0, p[1] + 13.0]);
        draw.triangle_filled(a, b, c, col32(3, 18, 25, 240));
        draw.triangle(a, b, c, color, 2.0);
    }
    changed
}
