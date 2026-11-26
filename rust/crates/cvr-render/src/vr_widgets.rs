//! THE VR MENU'S WIDGETS (formerly src/Overlay/VrWidgets.cpp, include/Overlay/VrWidgets.hpp: overlay::widgets): big
//! pointer-friendly sliders and combos -- a label, then a cut-corner field with arrows either side -- the section
//! header, the tab row and the bindings page. Each one is also an item of the VR menu's record pass (Overlay/UiRec.hpp):
//! recording, it hands its label, range and value to the recorder instead of drawing. ImGui through crate::imgui.
use core::ffi::{c_char, c_void, CStr};

use crate::imgui::{self, DrawList, Style, Vec2, Vec4};
use crate::ui;

use crate::ui_rec::{self, recording};

const RED: Vec4 = [1.0, 0.28, 0.29, 1.0];
const CYAN: Vec4 = [0.30, 0.88, 0.94, 1.0];
const YELLOW: Vec4 = [0.98, 0.94, 0.06, 1.0];
const CLEAR: Vec4 = [0.0, 0.0, 0.0, 0.0];

/// std::max / std::min / std::clamp, as they compare.
fn max(a: f32, b: f32) -> f32 {
    if a < b {
        b
    } else {
        a
    }
}
fn min(a: f32, b: f32) -> f32 {
    if b < a {
        b
    } else {
        a
    }
}
fn clamp<T: PartialOrd>(v: T, lo: T, hi: T) -> T {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// A cut-corner outline, or its fill.
fn outline(draw: DrawList, a: Vec2, b: Vec2, color: u32, fill: bool) {
    let cut = min(10.0, (b[1] - a[1]) * 0.22);
    let points = [
        [a[0], a[1]],
        [b[0], a[1]],
        [b[0], b[1] - cut],
        [b[0] - cut, b[1]],
        [a[0], b[1]],
    ];
    if fill {
        draw.convex_poly_filled(&points, color);
    } else {
        draw.polyline(&points, color, imgui::DRAW_CLOSED, 1.2);
    }
}
/// A text centred in a box, shrunk to fit its width.
fn center_text(draw: DrawList, a: Vec2, b: Vec2, text: *const c_char, color: u32) {
    let size = imgui::calc_text_size_ex(text, core::ptr::null(), false, -1.0);
    let fit = min(1.0, max(1.0, b[0] - a[0] - 12.0) / max(1.0, size[0]));
    draw.push_clip_rect(a, b, true);
    draw.text(
        imgui::font(),
        imgui::font_size() * fit,
        [
            a[0] + max(4.0, (b[0] - a[0] - size[0] * fit) * 0.5),
            a[1] + (b[1] - a[1] - size[1] * fit) * 0.5,
        ],
        color,
        text,
        core::ptr::null(),
        0.0,
    );
    draw.pop_clip_rect();
}

/// One widget's row: the label on the left (40% of the width), the field on the right, in a group under the label's
/// id; dropping it closes both.
struct Field {
    a: Vec2,
    b: Vec2,
    draw: DrawList,
    height: f32,
    arrow: f32,
}
impl Field {
    fn new(label: &CStr) -> Field {
        imgui::begin_group();
        imgui::push_id_str(label);
        let start = imgui::cursor_screen_pos();
        let available = max(160.0, imgui::content_region_avail()[0]);
        let end = imgui::find_rendered_text_end(label);
        let named = end != label.as_ptr();
        let label_width = if named { available * 0.40 } else { 0.0 };
        let gap = if named {
            Style::current().vec2(Style::ITEM_INNER_SPACING)[0]
        } else {
            0.0
        };
        let size = if named {
            imgui::calc_text_size_ex(label.as_ptr(), end, false, max(1.0, label_width - 8.0))
        } else {
            [0.0, 0.0]
        };
        let height = max(44.0, imgui::frame_height());
        let row_height = max(height, size[1] + 8.0);
        let draw = imgui::window_draw_list();
        if named {
            draw.text(
                imgui::font(),
                imgui::font_size(),
                [start[0], start[1] + (row_height - size[1]) * 0.5],
                imgui::get_color_u32(RED),
                label.as_ptr(),
                end,
                label_width - 8.0,
            );
        }
        imgui::dummy([label_width, row_height]);
        let a = [
            start[0] + label_width + gap,
            start[1] + (row_height - height) * 0.5,
        ];
        let width = max(100.0, available - label_width - gap);
        let b = [a[0] + width, a[1] + height];
        let arrow = min(height, width * 0.18);
        outline(
            draw,
            a,
            b,
            imgui::get_color_u32([0.065, 0.025, 0.040, 0.9]),
            true,
        );
        outline(
            draw,
            a,
            b,
            imgui::get_color_u32([0.62, 0.18, 0.21, 1.0]),
            false,
        );
        Field {
            a,
            b,
            draw,
            height,
            arrow,
        }
    }
    /// The step arrow on the left or right end; repeats while held.
    fn arrow(&self, right: bool, enabled: bool) -> bool {
        let p = [
            if right {
                self.b[0] - self.arrow
            } else {
                self.a[0]
            },
            self.a[1],
        ];
        imgui::set_cursor_screen_pos(p);
        imgui::push_style_color(imgui::COL_BUTTON, CLEAR);
        imgui::push_style_color(imgui::COL_BUTTON_HOVERED, [0.10, 0.24, 0.27, 0.4]);
        imgui::push_style_color(imgui::COL_BUTTON_ACTIVE, [0.16, 0.32, 0.34, 0.7]);
        imgui::push_style_var_f32(imgui::VAR_FRAME_BORDER_SIZE, 0.0);
        imgui::begin_disabled(!enabled);
        imgui::push_button_repeat(true);
        let clicked = imgui::button(
            if right { c"##next" } else { c"##previous" },
            [self.arrow, self.height],
        );
        imgui::pop_button_repeat();
        let color = imgui::get_color_u32(if imgui::is_item_active() {
            YELLOW
        } else {
            CYAN
        });
        let x = p[0] + self.arrow * 0.5;
        let y = p[1] + self.height * 0.5;
        let s = min(9.0, self.height * 0.16);
        let sign = if right { 1.0 } else { -1.0 };
        self.draw.triangle(
            [x + sign * s, y],
            [x - sign * s, y - s],
            [x - sign * s, y + s],
            color,
            1.4,
        );
        imgui::end_disabled();
        imgui::pop_style_var(1);
        imgui::pop_style_color(3);
        clicked
    }
    fn track_min(&self) -> Vec2 {
        [self.a[0] + self.arrow, self.a[1]]
    }
    fn track_max(&self) -> Vec2 {
        [self.b[0] - self.arrow, self.b[1]]
    }
}
impl Drop for Field {
    fn drop(&mut self) {
        imgui::pop_id();
        imgui::end_group();
    }
}

/// The slider's value type: ImGuiDataType_Float or ImGuiDataType_S32.
trait SliderValue: Copy + PartialEq + PartialOrd {
    const TYPE: i32;
    fn step(precision: i32) -> Self;
    fn plus(self, o: Self) -> Self;
    fn minus(self, o: Self) -> Self;
    fn to_f64(self) -> f64;
}
impl SliderValue for f32 {
    const TYPE: i32 = imgui::DATA_TYPE_FLOAT;
    fn step(precision: i32) -> f32 {
        10.0f64.powf(-(precision as f64)) as f32
    }
    fn plus(self, o: f32) -> f32 {
        self + o
    }
    fn minus(self, o: f32) -> f32 {
        self - o
    }
    fn to_f64(self) -> f64 {
        self as f64
    }
}
impl SliderValue for i32 {
    const TYPE: i32 = imgui::DATA_TYPE_S32;
    fn step(_: i32) -> i32 {
        1
    }
    fn plus(self, o: i32) -> i32 {
        self.wrapping_add(o)
    }
    fn minus(self, o: i32) -> i32 {
        self.wrapping_sub(o)
    }
    fn to_f64(self) -> f64 {
        self as f64
    }
}
fn ptr<T>(v: &T) -> *const c_void {
    v as *const T as *const c_void
}

/// The slider: arrows step it by its format's last digit (1 for ints), the track shows a grab with the value in it;
/// ImGui's own slider underneath, invisible, takes the drags and Ctrl+click typing.
fn slider<T: SliderValue>(
    label: &CStr,
    value: &mut T,
    minimum: T,
    maximum: T,
    format: &CStr,
    flags: i32,
) -> bool {
    let field = Field::new(label);
    let precision = if T::TYPE == imgui::DATA_TYPE_S32 {
        0
    } else {
        imgui::parse_format_precision(format, 3).clamp(0, 6)
    };
    let step = T::step(precision);
    let mut changed = false;
    if field.arrow(false, *value > minimum) {
        let next = clamp(value.minus(step), minimum, maximum);
        changed = next != *value;
        *value = next;
    }
    let a = field.track_min();
    let b = field.track_max();
    // SAFETY: values of the slider's type.
    let (low, high) = unsafe {
        (
            imgui::data_type_format_string(T::TYPE, ptr(&minimum), format),
            imgui::data_type_format_string(T::TYPE, ptr(&maximum), format),
        )
    };
    let width = |text: &[c_char; 96]| {
        imgui::calc_text_size_ex(text.as_ptr(), core::ptr::null(), false, -1.0)[0]
    };
    let grab = min(
        max(width(&low), width(&high)) + 24.0,
        max(24.0, b[0] - a[0] - 4.0),
    );
    field.draw.line(
        [a[0] + 2.0, (a[1] + b[1]) * 0.5],
        [b[0] - 2.0, (a[1] + b[1]) * 0.5],
        imgui::get_color_u32([0.44, 0.12, 0.16, 1.0]),
        2.0,
    );
    imgui::set_cursor_screen_pos(a);
    imgui::set_next_item_width(b[0] - a[0]);
    let id = imgui::get_id(c"##value");
    let editing = imgui::temp_input_is_active(id)
        || (imgui::key_ctrl() && imgui::is_mouse_hovering_rect(a, b));
    imgui::push_style_var_f32(imgui::VAR_GRAB_MIN_SIZE, grab);
    imgui::push_style_var_f32(imgui::VAR_FRAME_BORDER_SIZE, 0.0);
    imgui::push_style_var_vec2(
        imgui::VAR_FRAME_PADDING,
        [4.0, (field.height - imgui::font_size()) * 0.5],
    );
    imgui::push_style_color(imgui::COL_FRAME_BG, CLEAR);
    imgui::push_style_color(imgui::COL_FRAME_BG_HOVERED, CLEAR);
    imgui::push_style_color(imgui::COL_FRAME_BG_ACTIVE, CLEAR);
    imgui::push_style_color(imgui::COL_SLIDER_GRAB, CLEAR);
    imgui::push_style_color(imgui::COL_SLIDER_GRAB_ACTIVE, CLEAR);
    imgui::push_style_color(imgui::COL_TEXT, if editing { CYAN } else { CLEAR });
    // SAFETY: the value and its bounds, of the slider's type.
    changed |= unsafe {
        imgui::slider_scalar(
            c"##value",
            T::TYPE,
            value as *mut T as *mut c_void,
            ptr(&minimum),
            ptr(&maximum),
            format,
            flags | imgui::SLIDER_ALWAYS_CLAMP,
        )
    };
    let hot = imgui::is_item_hovered() || imgui::is_item_active();
    imgui::pop_style_color(6);
    imgui::pop_style_var(3);
    if !imgui::temp_input_is_active(id) {
        let ratio = if maximum > minimum {
            clamp(
                (value.minus(minimum).to_f64() / maximum.minus(minimum).to_f64()) as f32,
                0.0,
                1.0,
            )
        } else {
            0.0
        };
        let usable = max(0.0, b[0] - a[0] - 4.0 - grab);
        let left = [a[0] + 2.0 + usable * ratio, a[1] + 3.0];
        let right = [left[0] + grab, b[1] - 3.0];
        outline(
            field.draw,
            left,
            right,
            imgui::get_color_u32([0.15, 0.035, 0.05, 1.0]),
            true,
        );
        outline(
            field.draw,
            left,
            right,
            imgui::get_color_u32(if hot { YELLOW } else { RED }),
            false,
        );
        // SAFETY: a value of the slider's type.
        let text = unsafe { imgui::data_type_format_string(T::TYPE, ptr(value), format) };
        center_text(
            field.draw,
            left,
            right,
            text.as_ptr(),
            imgui::get_color_u32(CYAN),
        );
    }
    if field.arrow(true, *value < maximum) {
        let next = clamp(value.plus(step), minimum, maximum);
        changed |= next != *value;
        *value = next;
    }
    changed
}

/// widgets::SliderFloat.
pub fn slider_float(
    label: &CStr,
    value: &mut f32,
    minimum: f32,
    maximum: f32,
    format: &CStr,
) -> bool {
    if recording() {
        return ui_rec::slider_float(label, value, minimum, maximum, format);
    }
    slider(label, value, minimum, maximum, format, 0)
}
/// widgets::SliderInt.
pub fn slider_int(
    label: &CStr,
    value: &mut i32,
    minimum: i32,
    maximum: i32,
    format: &CStr,
) -> bool {
    if recording() {
        return ui_rec::slider_int(label, value, minimum, maximum, format);
    }
    slider(label, value, minimum, maximum, format, 0)
}
/// widgets::Combo: the selected item's text in the field, arrows step through the list, a click opens it.
pub fn combo(label: &CStr, selected: &mut i32, items: &[*const c_char]) -> bool {
    let count = items.len() as i32;
    if recording() {
        return ui_rec::combo(label, selected, items);
    }
    let field = Field::new(label);
    let mut changed = false;
    if field.arrow(false, count > 0 && *selected > 0) {
        *selected = (*selected - 1).clamp(0, count - 1);
        changed = true;
    }
    let a = field.track_min();
    let b = field.track_max();
    imgui::set_cursor_screen_pos(a);
    imgui::set_next_item_width(b[0] - a[0]);
    imgui::push_style_var_f32(imgui::VAR_FRAME_BORDER_SIZE, 0.0);
    imgui::push_style_var_vec2(
        imgui::VAR_FRAME_PADDING,
        [10.0, (field.height - imgui::font_size()) * 0.5],
    );
    imgui::push_style_color(imgui::COL_FRAME_BG, CLEAR);
    imgui::push_style_color(imgui::COL_FRAME_BG_HOVERED, [0.10, 0.24, 0.27, 0.25]);
    imgui::push_style_color(imgui::COL_FRAME_BG_ACTIVE, [0.10, 0.24, 0.27, 0.35]);
    let open = imgui::begin_combo(
        c"##list",
        c"",
        imgui::COMBO_NO_ARROW_BUTTON | imgui::COMBO_HEIGHT_LARGE,
    );
    imgui::pop_style_color(3);
    imgui::pop_style_var(2);
    if open {
        for (i, item) in items.iter().enumerate() {
            let i = i as i32;
            imgui::push_id_int(i);
            imgui::push_style_color(imgui::COL_TEXT, if i == *selected { YELLOW } else { CYAN });
            if imgui::selectable(
                *item,
                i == *selected,
                0,
                [0.0, max(44.0, imgui::frame_height())],
            ) {
                *selected = i;
                changed = true;
            }
            if i == *selected {
                imgui::set_item_default_focus();
            }
            imgui::pop_style_color(1);
            imgui::pop_id();
        }
        imgui::end_combo();
    }
    let shown = if *selected >= 0 && *selected < count {
        items[*selected as usize]
    } else {
        c"--".as_ptr()
    };
    center_text(field.draw, a, b, shown, imgui::get_color_u32(CYAN));
    if field.arrow(true, count > 0 && *selected < count - 1) {
        *selected = (*selected + 1).clamp(0, count - 1);
        changed = true;
    }
    changed
}
/// widgets::Combo over a zero-separated list (ending in an empty item, as the C literal does).
///
/// # Safety
/// `list` is a zero-separated list ending in two NULs.
pub unsafe fn combo_z(label: &CStr, selected: &mut i32, list: *const c_char) -> bool {
    let mut items = Vec::new();
    let mut p = list;
    // SAFETY: walking the caller's list item by item.
    unsafe {
        while *p != 0 {
            items.push(p);
            p = p.add(CStr::from_ptr(p).to_bytes().len() + 1);
        }
    }
    combo(label, selected, &items)
}
/// widgets::Section: a cyan title with a rule under it.
pub fn section(label: &CStr) {
    if recording() {
        ui_rec::header(label, true);
        return;
    }
    imgui::spacing();
    imgui::text_colored(CYAN, label);
    let p = imgui::cursor_screen_pos();
    imgui::window_draw_list().line(
        p,
        [p[0] + imgui::content_region_avail()[0], p[1]],
        imgui::get_color_u32([0.25, 0.72, 0.80, 0.65]),
        1.0,
    );
    imgui::dummy([0.0, 5.0]);
}
/// widgets::Tabs: a row of equal buttons, the selected one yellow and underlined.
pub fn tabs(id: &CStr, selected: &mut i32, labels: &[*const c_char]) {
    let count = labels.len() as i32;
    if recording() {
        ui_rec::tabs(id, selected, labels);
        return;
    }
    imgui::push_id_str(id);
    let width = (imgui::content_region_avail()[0]
        - Style::current().vec2(Style::ITEM_SPACING)[0] * (count - 1) as f32)
        / count as f32;
    for (i, label) in labels.iter().enumerate() {
        let i = i as i32;
        if i != 0 {
            imgui::same_line(0.0, -1.0);
        }
        imgui::push_id_int(i);
        imgui::push_style_color(imgui::COL_TEXT, if *selected == i { YELLOW } else { CYAN });
        // SAFETY: the caller's C string.
        if imgui::button(
            unsafe { CStr::from_ptr(*label) },
            [width, max(46.0, imgui::frame_height())],
        ) {
            *selected = i;
        }
        if *selected == i {
            let a = imgui::item_rect_min();
            let b = imgui::item_rect_max();
            imgui::window_draw_list().line(
                [a[0], b[1]],
                [b[0], b[1]],
                imgui::get_color_u32(YELLOW),
                2.0,
            );
        }
        imgui::pop_style_color(1);
        imgui::pop_id();
    }
    imgui::pop_id();
    imgui::spacing();
}

/// The bindings page's rows: an input and what it does.
type Binding = (&'static CStr, &'static CStr);
const FOOT: [Binding; 16] = [
    (
        c"LEFT STICK",
        c"Move. Hold fully forward for 0.2 seconds to sprint.",
    ),
    (
        c"RIGHT STICK  Left / right",
        c"Turn. Uses your snap-turn setting.",
    ),
    (c"RIGHT STICK  Full up", c"Dash / dodge once per push."),
    (c"RIGHT STICK  Full down", c"Crouch."),
    (
        c"RIGHT STICK  Press",
        c"Rack the weapon / release the slide.",
    ),
    (c"A  Right controller", c"Jump."),
    (
        c"B  Right controller",
        c"With a weapon: release the magazine. Holstered: Back / close.",
    ),
    (c"X  Left controller", c"Interact / reload."),
    (c"Y  Left controller", c"Switch weapon."),
    (c"RIGHT TRIGGER", c"Fire."),
    (c"LEFT TRIGGER", c"Aim / block in melee."),
    (
        c"RIGHT GRIP  At a holster",
        c"Equip or holster the weapon at the reached slot.",
    ),
    (c"LEFT GRIP", c"Grab the magazine during a reload."),
    (
        c"LEFT L3  Near left ear",
        c"Toggle the scanner with a fresh left-stick click. Release and click again to close. Always the physical left hand, including left-handed mode.",
    ),
    (
        c"L3 + LEFT GRIP  Hold 0.5 seconds",
        c"Activate cyberware once. Release both to use again.",
    ),
    (c"LEFT MENU BUTTON", c"Open the game's pause menu."),
];
const SWIMMING: [Binding; 3] = [
    (
        c"BOTH HANDS  Reach, sweep out, pull back",
        c"Swim in the direction you look. Movement starts during the pull; gentle strokes swim normally and fast strokes boost.",
    ),
    (c"BOTH HANDS  Raise, then push down", c"Swim upward."),
    (c"LEFT STICK  Back", c"Cancel gesture propulsion. Stick movement takes priority."),
];
const SCANNER: [Binding; 5] = [
    (
        c"LEFT STICK  Full up / down",
        c"Page through quickhacks. Partial movement still walks.",
    ),
    (c"X  Left controller", c"Apply the selected quickhack."),
    (c"RIGHT TRIGGER", c"Tag the target while scanning."),
    (c"RIGHT STICK  Press", c"Change scanner tab."),
    (c"LEFT TRIGGER + RIGHT STICK", c"Adjust scanner zoom."),
];
const DPAD: [Binding; 2] = [
    (
        c"Hold LEFT STICK press + RIGHT STICK",
        c"Push the right stick fully up, down, left or right to send that D-pad direction.",
    ),
    (
        c"Release without a direction",
        c"Send the normal left-stick press (L3).",
    ),
];
const DRIVING: [Binding; 5] = [
    (c"Hold X  Left controller", c"Exit the vehicle."),
    (c"A  Right controller", c"Confirm a dialogue choice. The normal handbrake action remains available."),
    (c"LEFT / RIGHT TRIGGER", c"Brake / throttle."),
    (c"GRIP  At the wheel", c"Grab the wheel or handlebars with that hand. Release to let go."),
    (
        c"GAME CAMERA",
        c"VR follows the camera chosen by the game, including third-person scenes. Driving and seated-camera options are in DRIVING.",
    ),
];
const OVERLAY: [Binding; 6] = [
    (
        c"L3 + R3  Hold 0.5 second",
        c"Open or close the VR menu in front of you (as the right-hand swipe down does).",
    ),
    (c"Point + TRIGGER", c"Select a control."),
    (c"STICK  Up / down", c"Scroll the pointed panel."),
    (c"GRIP", c"Drag the panel."),
    (
        c"GRIP + STICK  Up / down",
        c"Move the panel farther away / closer.",
    ),
    (
        c"F10 / INSERT",
        c"Open or close this overlay from the keyboard. ESC closes it.",
    ),
];

/// widgets::DrawBindings: the VR shortcuts and the default game actions, a table per group.
pub fn draw_bindings() {
    let recording = recording();
    let group = |title: &CStr, rows: &[Binding]| {
        section(title);
        if recording {
            for (input, action) in rows {
                ui_rec::binding(input, action);
            }
            return;
        }
        imgui::push_style_var_vec2(imgui::VAR_CELL_PADDING, [12.0, 12.0]);
        if imgui::begin_table(
            title,
            2,
            imgui::TABLE_SIZING_STRETCH_PROP | imgui::TABLE_BORDERS_INNER_H,
        ) {
            imgui::table_setup_column(c"INPUT", imgui::TABLE_COLUMN_WIDTH_STRETCH, 0.36);
            imgui::table_setup_column(c"ACTION", imgui::TABLE_COLUMN_WIDTH_STRETCH, 0.64);
            for (input, action) in rows {
                imgui::table_next_row(0, 44.0);
                imgui::table_next_column();
                imgui::push_style_color(imgui::COL_TEXT, CYAN);
                imgui::text_wrapped(input);
                imgui::pop_style_color(1);
                imgui::table_next_column();
                imgui::text_wrapped(action);
            }
            imgui::end_table();
        }
        imgui::pop_style_var(1);
    };
    ui::text_wrapped(
        c"VR shortcuts and default game actions. Button names follow the Touch / Xbox convention; game actions use your controller bindings.",
        &[],
    );
    group(c"ON FOOT", &FOOT);
    group(c"SWIMMING", &SWIMMING);
    group(c"SCANNER", &SCANNER);
    group(c"D-PAD SHORTCUT", &DPAD);
    group(c"DRIVING", &DRIVING);
    group(c"OVERLAY", &OVERLAY);
    ui::spacing();
    ui::text_wrapped(
        c"Change game actions in Settings > Key Bindings > Controller. VR gesture settings are in CONTROLS and AVATAR.",
        &[],
    );
}
