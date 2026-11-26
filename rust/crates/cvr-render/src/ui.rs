//! The settings panels' widgets (include/Overlay/UiRec.hpp `UI::` and include/Overlay/VrWidgets.hpp
//! `overlay::widgets`): ImGui while the panel draws (crate::imgui), the VR menu's recorder while it reads the page
//! (crate::ui_rec). Text is formatted here as the C runtime formats it; ImGui takes it as "%s", verbatim, and the
//! recorder as the C++ half's vsnprintf into 4096 bytes left it.
use core::ffi::{c_char, CStr};
use std::ffi::CString;

use cvr_crt::{format, Arg};

use crate::{imgui, ui_rec};

/// A label or text, as a C string (an interior NUL cuts it, as it would in C).
fn c(text: &str) -> CString {
    CString::new(text.split('\0').next().unwrap_or("")).unwrap_or_default()
}
fn recording() -> bool {
    ui_rec::recording()
}
/// What vsnprintf(buffer, 4096, "%s", text) leaves.
fn record_text(text: &CStr) -> &[u8] {
    let b = text.to_bytes();
    &b[..b.len().min(4095)]
}
/// UI::TextColored's colour for the recorder: 0xRRGGBB.
fn pack(c: [f32; 4]) -> u32 {
    let b = |v: f32| {
        ((if v < 0.0 {
            0.0
        } else if 1.0 < v {
            1.0
        } else {
            v
        }) * 255.0
            + 0.5) as u32
    };
    (b(c[0]) << 16) | (b(c[1]) << 8) | b(c[2])
}

pub fn checkbox(label: &CStr, value: &mut bool) -> bool {
    if recording() {
        ui_rec::checkbox(label, value)
    } else {
        imgui::checkbox(label, value)
    }
}
pub fn checkbox_str(label: &str, value: &mut bool) -> bool {
    checkbox(&c(label), value)
}
pub fn button(label: &CStr) -> bool {
    button_sized(label, 0.0, 0.0)
}
pub fn button_sized(label: &CStr, width: f32, height: f32) -> bool {
    if recording() {
        ui_rec::button(label)
    } else {
        imgui::button(label, [width, height])
    }
}
/// 0 Text, 1 TextDisabled, 2 TextWrapped, 3 TextColored, 4 TextUnformatted -- of a finished text.
fn text_kind(kind: i32, color: [f32; 4], text: &CStr) {
    if recording() {
        match kind {
            0 => ui_rec::text(0, 0, record_text(text)),
            1 => ui_rec::text(1, 0, record_text(text)),
            2 => ui_rec::text(2, 0, record_text(text)),
            3 => ui_rec::text(4, pack(color), record_text(text)),
            _ => ui_rec::text(0, 0, text.to_bytes()),
        }
    } else {
        match kind {
            0 => imgui::text(text),
            1 => imgui::text_disabled(text),
            2 => imgui::text_wrapped(text),
            3 => imgui::text_colored(color, text),
            _ => imgui::text_unformatted(text),
        }
    }
}
/// UI::Text(format, args...).
pub fn text(fmt: &CStr, args: &[Arg]) {
    text_kind(0, [0.0; 4], &format(fmt, args));
}
pub fn text_disabled(fmt: &CStr, args: &[Arg]) {
    text_kind(1, [0.0; 4], &format(fmt, args));
}
pub fn text_wrapped(fmt: &CStr, args: &[Arg]) {
    text_kind(2, [0.0; 4], &format(fmt, args));
}
pub fn text_colored(color: [f32; 4], fmt: &CStr, args: &[Arg]) {
    text_kind(3, color, &format(fmt, args));
}
/// UI::TextUnformatted: taken as it is, no format.
pub fn text_unformatted(text: &CStr) {
    text_kind(4, [0.0; 4], text);
}
fn set_tooltip_text(text: &CStr) {
    if recording() {
        ui_rec::tooltip(record_text(text));
    } else {
        imgui::set_tooltip(text);
    }
}
/// UI::SetTooltip(format, args...).
pub fn set_tooltip(fmt: &CStr, args: &[Arg]) {
    set_tooltip_text(&format(fmt, args));
}
/// Recording, every item counts as pointed at -- that is what makes each SetTooltip in the panels land on its item --
/// and none as being dragged.
pub fn is_item_hovered() -> bool {
    is_item_hovered_flags(0)
}
/// UI::IsItemHovered(flags).
pub fn is_item_hovered_flags(flags: i32) -> bool {
    recording() || imgui::is_item_hovered_flags(flags)
}
pub fn is_item_active() -> bool {
    !recording() && imgui::is_item_active()
}
pub fn is_any_item_active() -> bool {
    !recording() && imgui::is_any_item_active()
}
/// `if (UI::IsItemHovered()) UI::SetTooltip(text)`.
pub fn tooltip(fmt: &CStr) {
    if is_item_hovered() {
        set_tooltip(fmt, &[]);
    }
}
pub fn begin_disabled(disabled: bool) {
    if recording() {
        ui_rec::begin_disabled(disabled)
    } else {
        imgui::begin_disabled(disabled)
    }
}
pub fn end_disabled() {
    if recording() {
        ui_rec::end_disabled()
    } else {
        imgui::end_disabled()
    }
}
pub fn separator() {
    if recording() {
        ui_rec::separator()
    } else {
        imgui::separator()
    }
}
pub fn same_line() {
    if !recording() {
        imgui::same_line(0.0, -1.0)
    }
}
pub fn set_next_item_width(width: f32) {
    if !recording() {
        imgui::set_next_item_width(width)
    }
}
pub fn progress_bar(fraction: f32, width: f32, height: f32) {
    if recording() {
        ui_rec::progress(fraction)
    } else {
        imgui::progress_bar(fraction, [width, height])
    }
}
/// widgets::Combo over a list of items.
pub fn combo(label: &CStr, selected: &mut i32, items: &[&CStr]) -> bool {
    let ptrs: Vec<*const c_char> = items.iter().map(|s| s.as_ptr()).collect();
    crate::vr_widgets::combo(label, selected, &ptrs)
}
pub fn combo_str(label: &str, selected: &mut i32, items: &[&CStr]) -> bool {
    combo(&c(label), selected, items)
}
/// widgets::SliderFloat.
pub fn slider_float(label: &CStr, value: &mut f32, min: f32, max: f32, fmt: &CStr) -> bool {
    crate::vr_widgets::slider_float(label, value, min, max, fmt)
}
/// ImGuiTreeNodeFlags_DefaultOpen.
const DEFAULT_OPEN: i32 = 1 << 5;
/// UI::CollapsingHeader (ImGuiTreeNodeFlags).
pub fn collapsing_header(label: &CStr, flags: i32) -> bool {
    if recording() {
        ui_rec::header(label, flags & DEFAULT_OPEN != 0)
    } else {
        imgui::collapsing_header(label, flags)
    }
}
pub fn indent() {
    if recording() {
        ui_rec::indent(1)
    } else {
        imgui::indent(0.0)
    }
}
pub fn unindent() {
    if recording() {
        ui_rec::indent(-1)
    } else {
        imgui::unindent(0.0)
    }
}
/// widgets::SliderInt with its default format.
pub fn slider_int(label: &CStr, value: &mut i32, min: i32, max: i32) -> bool {
    crate::vr_widgets::slider_int(label, value, min, max, c"%d")
}
/// widgets::SliderInt with a format.
pub fn slider_int_fmt(label: &CStr, value: &mut i32, min: i32, max: i32, fmt: &CStr) -> bool {
    crate::vr_widgets::slider_int(label, value, min, max, fmt)
}
/// UI::SliderFloat: ImGui's own slider (or the recorder's), not the VR menu's widget.
pub fn ui_slider_float(label: &CStr, value: &mut f32, min: f32, max: f32, fmt: &CStr) -> bool {
    if recording() {
        ui_rec::slider_float(label, value, min, max, fmt)
    } else {
        imgui::slider_float(label, value, min, max, fmt, 0)
    }
}
/// widgets::Tabs.
pub fn tabs(id: &CStr, selected: &mut i32, labels: &[&CStr]) {
    let ptrs: Vec<*const c_char> = labels.iter().map(|s| s.as_ptr()).collect();
    crate::vr_widgets::tabs(id, selected, &ptrs)
}
/// UI::BeginTabBar / BeginTabItem / EndTabItem / EndTabBar (the recorder draws no tab bars).
pub fn begin_tab_bar(id: &CStr) -> bool {
    !recording() && imgui::begin_tab_bar(id)
}
pub fn begin_tab_item(label: &CStr) -> bool {
    !recording() && imgui::begin_tab_item(label)
}
pub fn end_tab_item() {
    if !recording() {
        imgui::end_tab_item()
    }
}
pub fn end_tab_bar() {
    if !recording() {
        imgui::end_tab_bar()
    }
}
/// widgets::DrawBindings.
pub fn draw_bindings() {
    crate::vr_widgets::draw_bindings()
}
pub fn spacing() {
    if !recording() {
        imgui::spacing()
    }
}
/// widgets::Combo over a zero-separated list (`b"a\0b\0\0"`, ending in two NULs as the C literal does).
pub fn combo_z(label: &CStr, selected: &mut i32, items: &[u8]) -> bool {
    debug_assert!(items.ends_with(b"\0\0"));
    // SAFETY: a zero-separated list ending in two NULs.
    unsafe { crate::vr_widgets::combo_z(label, selected, items.as_ptr() as *const c_char) }
}
/// UI::PlotLines(label, values, count, 0, nullptr, min, max, ImVec2(width, height)).
pub fn plot_lines(
    label: &CStr,
    values: *const f32,
    count: i32,
    min: f32,
    max: f32,
    width: f32,
    height: f32,
) {
    if !recording() {
        // SAFETY: `count` floats the caller keeps alive for the call.
        unsafe { imgui::plot_lines(label, values, count, min, max, [width, height]) }
    }
}
pub fn push_id(id: &CStr) {
    if recording() {
        ui_rec::push_id_str(id)
    } else {
        imgui::push_id_str(id)
    }
}
pub fn push_id_int(id: i32) {
    if recording() {
        ui_rec::push_id_int(id)
    } else {
        imgui::push_id_int(id)
    }
}
pub fn pop_id() {
    if recording() {
        ui_rec::pop_id()
    } else {
        imgui::pop_id()
    }
}

// ---- UI:: for the C++ half's own callers (src/Overlay/UiRec.cpp forwards; Fbt/Game/Menu.cpp) ----------------------

/// # Safety
/// `label` is a C string, `value` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_ui_checkbox(label: *const c_char, value: *mut bool) -> bool {
    // SAFETY: as the caller promises.
    unsafe { checkbox(CStr::from_ptr(label), &mut *value) }
}
/// # Safety
/// `label` is a C string.
#[no_mangle]
pub unsafe extern "C" fn cvr_ui_button(label: *const c_char, width: f32, height: f32) -> bool {
    // SAFETY: as the caller promises.
    button_sized(unsafe { CStr::from_ptr(label) }, width, height)
}
/// # Safety
/// `label` is a C string.
#[no_mangle]
pub unsafe extern "C" fn cvr_ui_collapsing_header(label: *const c_char, flags: i32) -> bool {
    // SAFETY: as the caller promises.
    collapsing_header(unsafe { CStr::from_ptr(label) }, flags)
}
/// A finished text: 0 Text, 1 TextDisabled, 2 TextWrapped, 3 TextColored, 4 TextUnformatted.
///
/// # Safety
/// `text` is a C string.
#[no_mangle]
pub unsafe extern "C" fn cvr_ui_text(
    kind: i32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    text: *const c_char,
) {
    // SAFETY: as the caller promises.
    text_kind(kind, [r, g, b, a], unsafe { CStr::from_ptr(text) })
}
/// # Safety
/// `text` is a C string.
#[no_mangle]
pub unsafe extern "C" fn cvr_ui_set_tooltip(text: *const c_char) {
    // SAFETY: as the caller promises.
    set_tooltip_text(unsafe { CStr::from_ptr(text) })
}
#[no_mangle]
pub extern "C" fn cvr_ui_is_item_hovered(flags: i32) -> bool {
    is_item_hovered_flags(flags)
}
#[no_mangle]
pub extern "C" fn cvr_ui_begin_disabled(disabled: bool) {
    begin_disabled(disabled)
}
#[no_mangle]
pub extern "C" fn cvr_ui_end_disabled() {
    end_disabled()
}
#[no_mangle]
pub extern "C" fn cvr_uirec_recording() -> bool {
    recording()
}
