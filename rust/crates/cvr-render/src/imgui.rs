//! Dear ImGui (v1.91.1-docking), the external library, through its C boundary (src/Overlay/ImGuiAbi.cpp: one ImGui
//! call per function, nothing of the port's). The port's ImGui code -- the VR shell, its style -- calls these.
//! ImVec2 / ImVec4 are `[f32; 2]` / `[f32; 4]`; ImGuiStyle is written by field offset, checked by the C++ boundary.
use core::ffi::{c_char, c_void, CStr};

pub type Vec2 = [f32; 2];
pub type Vec4 = [f32; 4];
/// An ImDrawList*.
#[derive(Clone, Copy)]
pub struct DrawList(*mut c_void);

// ImGuiCol_*.
pub const COL_TEXT: i32 = 0;
pub const COL_TEXT_DISABLED: i32 = 1;
pub const COL_WINDOW_BG: i32 = 2;
pub const COL_CHILD_BG: i32 = 3;
pub const COL_POPUP_BG: i32 = 4;
pub const COL_BORDER: i32 = 5;
pub const COL_FRAME_BG: i32 = 7;
pub const COL_FRAME_BG_HOVERED: i32 = 8;
pub const COL_FRAME_BG_ACTIVE: i32 = 9;
pub const COL_SCROLLBAR_BG: i32 = 14;
pub const COL_SCROLLBAR_GRAB: i32 = 15;
pub const COL_CHECK_MARK: i32 = 18;
pub const COL_SLIDER_GRAB: i32 = 19;
pub const COL_SLIDER_GRAB_ACTIVE: i32 = 20;
pub const COL_BUTTON: i32 = 21;
pub const COL_BUTTON_HOVERED: i32 = 22;
pub const COL_BUTTON_ACTIVE: i32 = 23;
pub const COL_HEADER: i32 = 24;
pub const COL_HEADER_HOVERED: i32 = 25;
pub const COL_HEADER_ACTIVE: i32 = 26;
pub const COL_SEPARATOR: i32 = 27;
pub const COL_TABLE_BORDER_STRONG: i32 = 47;
pub const COL_TABLE_BORDER_LIGHT: i32 = 48;
pub const COL_TEXT_SELECTED_BG: i32 = 52;
const COL_COUNT: usize = 58;
// ImGuiStyleVar_*.
pub const VAR_FRAME_BORDER_SIZE: i32 = 13;
pub const VAR_ITEM_SPACING: i32 = 14;
pub const VAR_BUTTON_TEXT_ALIGN: i32 = 28;
// ImGuiWindowFlags_*.
pub const WINDOW_NO_MOVE: i32 = 4;
pub const WINDOW_NO_SAVED_SETTINGS: i32 = 256;
pub const WINDOW_NO_DECORATION: i32 = 43;
// ImDrawFlags_*.
pub const DRAW_CLOSED: i32 = 1;

/// IM_COL32.
pub const fn col32(r: u32, g: u32, b: u32, a: u32) -> u32 {
    (a << 24) | (b << 16) | (g << 8) | r
}

extern "C" {
    fn cvr_imgui_style() -> *mut u8;
    fn cvr_imgui_style_reset();
    fn cvr_imgui_push_style_color(idx: i32, r: f32, g: f32, b: f32, a: f32);
    fn cvr_imgui_pop_style_color(count: i32);
    fn cvr_imgui_push_style_var_float(idx: i32, value: f32);
    fn cvr_imgui_push_style_var_vec2(idx: i32, x: f32, y: f32);
    fn cvr_imgui_pop_style_var(count: i32);
    fn cvr_imgui_color_u32(r: f32, g: f32, b: f32, a: f32) -> u32;
    fn cvr_imgui_set_window_font_scale(scale: f32);
    fn cvr_imgui_push_item_width(width: f32);
    fn cvr_imgui_pop_item_width();
    fn cvr_imgui_set_next_window_pos(x: f32, y: f32);
    fn cvr_imgui_set_next_window_size(w: f32, h: f32);
    fn cvr_imgui_begin(name: *const c_char, flags: i32) -> bool;
    fn cvr_imgui_end();
    fn cvr_imgui_begin_child(
        id: *const c_char,
        w: f32,
        h: f32,
        child_flags: i32,
        window_flags: i32,
    ) -> bool;
    fn cvr_imgui_end_child();
    fn cvr_imgui_same_line(offset_from_start_x: f32, spacing: f32);
    fn cvr_imgui_separator();
    fn cvr_imgui_push_id_int(id: i32);
    fn cvr_imgui_pop_id();
    fn cvr_imgui_cursor_screen_pos(out: *mut Vec2);
    fn cvr_imgui_content_region_avail(out: *mut Vec2);
    fn cvr_imgui_calc_text_size(text: *const c_char, out: *mut Vec2);
    fn cvr_imgui_mouse_pos(out: *mut Vec2);
    fn cvr_imgui_button(label: *const c_char, w: f32, h: f32) -> bool;
    fn cvr_imgui_text_colored(r: f32, g: f32, b: f32, a: f32, text: *const c_char);
    fn cvr_imgui_text_disabled(text: *const c_char);
    fn cvr_imgui_is_item_hovered(flags: i32) -> bool;
    fn cvr_imgui_is_item_active() -> bool;
    fn cvr_imgui_item_rect_min(out: *mut Vec2);
    fn cvr_imgui_item_rect_max(out: *mut Vec2);
    fn cvr_imgui_is_mouse_hovering_rect(x0: f32, y0: f32, x1: f32, y1: f32) -> bool;
    fn cvr_imgui_window_draw_list() -> *mut c_void;
    fn cvr_imgui_foreground_draw_list() -> *mut c_void;
    fn cvr_imgui_draw_line(
        list: *mut c_void,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: u32,
        thickness: f32,
    );
    fn cvr_imgui_draw_rect_filled(
        list: *mut c_void,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: u32,
    );
    fn cvr_imgui_draw_rect_filled_multicolor(
        list: *mut c_void,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        upper_left: u32,
        upper_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    );
    fn cvr_imgui_draw_polyline(
        list: *mut c_void,
        points: *const Vec2,
        count: i32,
        color: u32,
        flags: i32,
        thickness: f32,
    );
    fn cvr_imgui_draw_triangle(
        list: *mut c_void,
        ax: f32,
        ay: f32,
        bx: f32,
        by: f32,
        cx: f32,
        cy: f32,
        color: u32,
        thickness: f32,
    );
    fn cvr_imgui_draw_triangle_filled(
        list: *mut c_void,
        ax: f32,
        ay: f32,
        bx: f32,
        by: f32,
        cx: f32,
        cy: f32,
        color: u32,
    );
}

// ---- the style ----------------------------------------------------------------------------------------------------

/// ImGui's style, reset to ImGuiStyle's defaults, to be written field by field.
pub struct Style(*mut u8);
impl Style {
    // ImGuiStyle's fields, by offset (src/Overlay/ImGuiAbi.cpp checks them).
    pub const WINDOW_PADDING: usize = 8;
    pub const WINDOW_ROUNDING: usize = 16;
    pub const WINDOW_BORDER_SIZE: usize = 20;
    pub const CHILD_ROUNDING: usize = 44;
    pub const CHILD_BORDER_SIZE: usize = 48;
    pub const POPUP_ROUNDING: usize = 52;
    pub const POPUP_BORDER_SIZE: usize = 56;
    pub const FRAME_PADDING: usize = 60;
    pub const FRAME_ROUNDING: usize = 68;
    pub const FRAME_BORDER_SIZE: usize = 72;
    pub const ITEM_SPACING: usize = 76;
    pub const ITEM_INNER_SPACING: usize = 84;
    pub const TOUCH_EXTRA_PADDING: usize = 100;
    pub const INDENT_SPACING: usize = 108;
    pub const SCROLLBAR_SIZE: usize = 116;
    pub const GRAB_MIN_SIZE: usize = 124;
    const COLORS: usize = 244;

    /// `ImGui::GetStyle() = ImGuiStyle{}`.
    pub fn reset() -> Style {
        // SAFETY: plain calls on the current context.
        unsafe {
            cvr_imgui_style_reset();
            Style(cvr_imgui_style())
        }
    }
    pub fn set_f32(&mut self, offset: usize, value: f32) {
        // SAFETY: a float field of ImGuiStyle (the offsets above).
        unsafe { self.0.add(offset).cast::<f32>().write_unaligned(value) }
    }
    pub fn set_vec2(&mut self, offset: usize, value: Vec2) {
        // SAFETY: an ImVec2 field of ImGuiStyle.
        unsafe { self.0.add(offset).cast::<Vec2>().write_unaligned(value) }
    }
    pub fn color(&self, idx: i32) -> Vec4 {
        assert!((idx as usize) < COL_COUNT);
        // SAFETY: Colors[idx].
        unsafe {
            self.0
                .add(Self::COLORS + idx as usize * 16)
                .cast::<Vec4>()
                .read_unaligned()
        }
    }
    pub fn set_color(&mut self, idx: i32, value: Vec4) {
        assert!((idx as usize) < COL_COUNT);
        // SAFETY: Colors[idx].
        unsafe {
            self.0
                .add(Self::COLORS + idx as usize * 16)
                .cast::<Vec4>()
                .write_unaligned(value)
        }
    }
}
pub fn push_style_color(idx: i32, c: Vec4) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_push_style_color(idx, c[0], c[1], c[2], c[3]) }
}
pub fn pop_style_color(count: i32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_pop_style_color(count) }
}
pub fn push_style_var_f32(idx: i32, value: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_push_style_var_float(idx, value) }
}
pub fn push_style_var_vec2(idx: i32, value: Vec2) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_push_style_var_vec2(idx, value[0], value[1]) }
}
pub fn pop_style_var(count: i32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_pop_style_var(count) }
}
/// ImGui::ColorConvertFloat4ToU32.
pub fn color_u32(c: Vec4) -> u32 {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_color_u32(c[0], c[1], c[2], c[3]) }
}
pub fn set_window_font_scale(scale: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_set_window_font_scale(scale) }
}
pub fn push_item_width(width: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_push_item_width(width) }
}
pub fn pop_item_width() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_pop_item_width() }
}

// ---- windows and layout -------------------------------------------------------------------------------------------

pub fn set_next_window_pos(p: Vec2) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_set_next_window_pos(p[0], p[1]) }
}
/// SetNextWindowPos every frame (ImGuiCond_Always), at a pivot.
pub fn set_next_window_pos_pivot(p: Vec2, pivot: Vec2) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_set_next_window_pos_pivot(p[0], p[1], pivot[0], pivot[1]) }
}
pub fn set_next_window_bg_alpha(alpha: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_set_next_window_bg_alpha(alpha) }
}
pub fn set_next_window_size(s: Vec2) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_set_next_window_size(s[0], s[1]) }
}
/// ImGui::Begin without a close button.
pub fn begin(name: &CStr, flags: i32) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_begin(name.as_ptr(), flags) }
}
pub fn end() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_end() }
}
pub fn begin_child(id: &CStr, size: Vec2, child_flags: i32, window_flags: i32) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_begin_child(id.as_ptr(), size[0], size[1], child_flags, window_flags) }
}
pub fn end_child() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_end_child() }
}
/// ImGui::SameLine(offset_from_start_x, spacing); `same_line(0.0, -1.0)` is ImGui::SameLine().
pub fn same_line(offset_from_start_x: f32, spacing: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_same_line(offset_from_start_x, spacing) }
}
pub fn separator() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_separator() }
}
pub fn push_id_int(id: i32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_push_id_int(id) }
}
pub fn pop_id() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_pop_id() }
}
fn vec2(f: unsafe extern "C" fn(*mut Vec2)) -> Vec2 {
    let mut v = [0.0; 2];
    // SAFETY: a slot to fill.
    unsafe { f(&mut v) };
    v
}
pub fn cursor_screen_pos() -> Vec2 {
    vec2(cvr_imgui_cursor_screen_pos)
}
pub fn content_region_avail() -> Vec2 {
    vec2(cvr_imgui_content_region_avail)
}
pub fn mouse_pos() -> Vec2 {
    vec2(cvr_imgui_mouse_pos)
}
pub fn item_rect_min() -> Vec2 {
    vec2(cvr_imgui_item_rect_min)
}
pub fn item_rect_max() -> Vec2 {
    vec2(cvr_imgui_item_rect_max)
}
pub fn calc_text_size(text: &CStr) -> Vec2 {
    let mut v = [0.0; 2];
    // SAFETY: a C string and a slot to fill.
    unsafe { cvr_imgui_calc_text_size(text.as_ptr(), &mut v) };
    v
}

// ---- items --------------------------------------------------------------------------------------------------------

pub fn button(label: &CStr, size: Vec2) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_button(label.as_ptr(), size[0], size[1]) }
}
/// ImGui::TextColored of a text, not a format.
pub fn text_colored(c: Vec4, text: &CStr) {
    // SAFETY: a C string.
    unsafe { cvr_imgui_text_colored(c[0], c[1], c[2], c[3], text.as_ptr()) }
}
/// ImGui::TextDisabled of a text, not a format.
pub fn text_disabled(text: &CStr) {
    // SAFETY: a C string.
    unsafe { cvr_imgui_text_disabled(text.as_ptr()) }
}
pub fn is_item_hovered() -> bool {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_is_item_hovered(0) }
}
pub fn is_item_active() -> bool {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_is_item_active() }
}
pub fn is_mouse_hovering_rect(min: Vec2, max: Vec2) -> bool {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_is_mouse_hovering_rect(min[0], min[1], max[0], max[1]) }
}

// ---- draw lists ---------------------------------------------------------------------------------------------------

pub fn window_draw_list() -> DrawList {
    // SAFETY: a plain call.
    DrawList(unsafe { cvr_imgui_window_draw_list() })
}
pub fn foreground_draw_list() -> DrawList {
    // SAFETY: a plain call.
    DrawList(unsafe { cvr_imgui_foreground_draw_list() })
}
impl DrawList {
    pub fn line(self, a: Vec2, b: Vec2, color: u32, thickness: f32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_line(self.0, a[0], a[1], b[0], b[1], color, thickness) }
    }
    pub fn rect_filled(self, a: Vec2, b: Vec2, color: u32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_rect_filled(self.0, a[0], a[1], b[0], b[1], color) }
    }
    pub fn rect_filled_multicolor(
        self,
        a: Vec2,
        b: Vec2,
        upper_left: u32,
        upper_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) {
        // SAFETY: the frame's draw list.
        unsafe {
            cvr_imgui_draw_rect_filled_multicolor(
                self.0,
                a[0],
                a[1],
                b[0],
                b[1],
                upper_left,
                upper_right,
                bottom_right,
                bottom_left,
            )
        }
    }
    pub fn polyline(self, points: &[Vec2], color: u32, flags: i32, thickness: f32) {
        // SAFETY: the frame's draw list and the caller's points.
        unsafe {
            cvr_imgui_draw_polyline(
                self.0,
                points.as_ptr(),
                points.len() as i32,
                color,
                flags,
                thickness,
            )
        }
    }
    pub fn triangle(self, a: Vec2, b: Vec2, c: Vec2, color: u32, thickness: f32) {
        // SAFETY: the frame's draw list.
        unsafe {
            cvr_imgui_draw_triangle(self.0, a[0], a[1], b[0], b[1], c[0], c[1], color, thickness)
        }
    }
    pub fn triangle_filled(self, a: Vec2, b: Vec2, c: Vec2, color: u32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_triangle_filled(self.0, a[0], a[1], b[0], b[1], c[0], c[1], color) }
    }
}

// ---- what the VR widgets use (crate::vr_widgets) -------------------------------------------------------------------

// ImGuiStyleVar_*, ImGuiDataType_*, ImGuiSliderFlags_*, ImGuiComboFlags_*, ImGuiTable*Flags_*.
pub const VAR_FRAME_PADDING: i32 = 11;
pub const VAR_CELL_PADDING: i32 = 17;
pub const VAR_GRAB_MIN_SIZE: i32 = 20;
pub const DATA_TYPE_S32: i32 = 4;
pub const DATA_TYPE_FLOAT: i32 = 8;
pub const SLIDER_ALWAYS_CLAMP: i32 = 16;
pub const COMBO_HEIGHT_LARGE: i32 = 8;
pub const COMBO_NO_ARROW_BUTTON: i32 = 32;
pub const TABLE_BORDERS_INNER_H: i32 = 128;
pub const TABLE_SIZING_STRETCH_PROP: i32 = 24576;
pub const TABLE_COLUMN_WIDTH_STRETCH: i32 = 8;

/// An ImFont*.
#[derive(Clone, Copy)]
pub struct Font(*mut c_void);

extern "C" {
    fn cvr_imgui_begin_group();
    fn cvr_imgui_end_group();
    fn cvr_imgui_push_id_str(id: *const c_char);
    fn cvr_imgui_get_id(id: *const c_char) -> u32;
    fn cvr_imgui_find_rendered_text_end(text: *const c_char) -> *const c_char;
    fn cvr_imgui_calc_text_size_ex(
        text: *const c_char,
        end: *const c_char,
        hide_after_double_hash: bool,
        wrap_width: f32,
        out: *mut Vec2,
    );
    fn cvr_imgui_frame_height() -> f32;
    fn cvr_imgui_font_size() -> f32;
    fn cvr_imgui_font() -> *mut c_void;
    fn cvr_imgui_get_color_u32(r: f32, g: f32, b: f32, a: f32) -> u32;
    fn cvr_imgui_dummy(w: f32, h: f32);
    fn cvr_imgui_spacing();
    fn cvr_imgui_set_cursor_screen_pos(x: f32, y: f32);
    fn cvr_imgui_set_next_item_width(width: f32);
    fn cvr_imgui_begin_disabled(disabled: bool);
    fn cvr_imgui_end_disabled();
    fn cvr_imgui_push_button_repeat(repeat: bool);
    fn cvr_imgui_pop_button_repeat();
    fn cvr_imgui_key_ctrl() -> bool;
    fn cvr_imgui_parse_format_precision(format: *const c_char, default_value: i32) -> i32;
    fn cvr_imgui_data_type_format_string(
        buffer: *mut c_char,
        size: i32,
        data_type: i32,
        data: *const c_void,
        format: *const c_char,
    ) -> i32;
    fn cvr_imgui_temp_input_is_active(id: u32) -> bool;
    fn cvr_imgui_slider_scalar(
        label: *const c_char,
        data_type: i32,
        data: *mut c_void,
        minimum: *const c_void,
        maximum: *const c_void,
        format: *const c_char,
        flags: i32,
    ) -> bool;
    fn cvr_imgui_begin_combo(label: *const c_char, preview: *const c_char, flags: i32) -> bool;
    fn cvr_imgui_end_combo();
    fn cvr_imgui_selectable(
        label: *const c_char,
        selected: bool,
        flags: i32,
        w: f32,
        h: f32,
    ) -> bool;
    fn cvr_imgui_set_item_default_focus();
    fn cvr_imgui_text_wrapped(text: *const c_char);
    fn cvr_imgui_begin_table(id: *const c_char, columns: i32, flags: i32) -> bool;
    fn cvr_imgui_table_setup_column(label: *const c_char, flags: i32, weight: f32);
    fn cvr_imgui_table_next_row(flags: i32, min_height: f32);
    fn cvr_imgui_table_next_column() -> bool;
    fn cvr_imgui_end_table();
    fn cvr_imgui_draw_convex_poly_filled(
        list: *mut c_void,
        points: *const Vec2,
        count: i32,
        color: u32,
    );
    fn cvr_imgui_draw_text(
        list: *mut c_void,
        font: *mut c_void,
        size: f32,
        x: f32,
        y: f32,
        color: u32,
        begin: *const c_char,
        end: *const c_char,
        wrap_width: f32,
    );
    fn cvr_imgui_draw_push_clip_rect(
        list: *mut c_void,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        intersect: bool,
    );
    fn cvr_imgui_draw_pop_clip_rect(list: *mut c_void);
    fn cvr_imgui_draw_circle_filled(list: *mut c_void, x: f32, y: f32, radius: f32, color: u32, segments: i32);
    fn cvr_imgui_draw_circle(list: *mut c_void, x: f32, y: f32, radius: f32, color: u32, segments: i32, thickness: f32);
    fn cvr_imgui_set_next_window_pos_pivot(x: f32, y: f32, pivot_x: f32, pivot_y: f32);
    fn cvr_imgui_set_next_window_bg_alpha(alpha: f32);
    fn cvr_imgui_draw_rect(list: *mut c_void, x0: f32, y0: f32, x1: f32, y1: f32, color: u32, rounding: f32, flags: i32, thickness: f32);
    fn cvr_imgui_draw_rect_filled_rounded(list: *mut c_void, x0: f32, y0: f32, x1: f32, y1: f32, color: u32, rounding: f32, flags: i32);
    fn cvr_imgui_font_calc_text_size_a(
        font: *mut c_void,
        size: f32,
        max_width: f32,
        wrap_width: f32,
        begin: *const c_char,
        end: *const c_char,
        out: *mut Vec2,
    );
}
impl Font {
    /// An ImFont* the C++ half holds (a font it added to the atlas).
    pub fn from_raw(font: *mut c_void) -> Font {
        Font(font)
    }
    /// ImFont::CalcTextSizeA(size, max_width, wrap_width, text).
    pub fn calc_text_size_a(self, size: f32, max_width: f32, wrap_width: f32, text: &CStr) -> Vec2 {
        let mut v = [0.0; 2];
        // SAFETY: a live font, a C string and a slot to fill.
        unsafe { cvr_imgui_font_calc_text_size_a(self.0, size, max_width, wrap_width, text.as_ptr(), core::ptr::null(), &mut v) };
        v
    }
}

impl Style {
    /// ImGui::GetStyle(), as it is.
    pub fn current() -> Style {
        // SAFETY: a plain call on the current context.
        Style(unsafe { cvr_imgui_style() })
    }
    pub fn vec2(&self, offset: usize) -> Vec2 {
        // SAFETY: an ImVec2 field of ImGuiStyle.
        unsafe { self.0.add(offset).cast::<Vec2>().read_unaligned() }
    }
}
pub fn begin_group() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_begin_group() }
}
pub fn end_group() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_end_group() }
}
pub fn push_id_str(id: &CStr) {
    // SAFETY: a C string.
    unsafe { cvr_imgui_push_id_str(id.as_ptr()) }
}
pub fn get_id(id: &CStr) -> u32 {
    // SAFETY: a C string.
    unsafe { cvr_imgui_get_id(id.as_ptr()) }
}
/// ImGui::FindRenderedTextEnd: where the label's shown part ends (its "##").
pub fn find_rendered_text_end(text: &CStr) -> *const c_char {
    // SAFETY: a C string; the result points into it.
    unsafe { cvr_imgui_find_rendered_text_end(text.as_ptr()) }
}
/// ImGui::CalcTextSize(text, end, hide_text_after_double_hash, wrap_width).
pub fn calc_text_size_ex(
    text: *const c_char,
    end: *const c_char,
    hide_after_double_hash: bool,
    wrap_width: f32,
) -> Vec2 {
    let mut v = [0.0; 2];
    // SAFETY: the caller's text and its end, a slot to fill.
    unsafe { cvr_imgui_calc_text_size_ex(text, end, hide_after_double_hash, wrap_width, &mut v) };
    v
}
pub fn frame_height() -> f32 {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_frame_height() }
}
pub fn font_size() -> f32 {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_font_size() }
}
pub fn font() -> Font {
    // SAFETY: a plain call.
    Font(unsafe { cvr_imgui_font() })
}
/// ImGui::GetColorU32(ImVec4): the colour with the style's alpha applied.
pub fn get_color_u32(c: Vec4) -> u32 {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_get_color_u32(c[0], c[1], c[2], c[3]) }
}
pub fn dummy(size: Vec2) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_dummy(size[0], size[1]) }
}
pub fn spacing() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_spacing() }
}
pub fn set_cursor_screen_pos(p: Vec2) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_set_cursor_screen_pos(p[0], p[1]) }
}
pub fn set_next_item_width(width: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_set_next_item_width(width) }
}
pub fn begin_disabled(disabled: bool) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_begin_disabled(disabled) }
}
pub fn end_disabled() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_end_disabled() }
}
pub fn push_button_repeat(repeat: bool) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_push_button_repeat(repeat) }
}
pub fn pop_button_repeat() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_pop_button_repeat() }
}
/// ImGui::GetIO().KeyCtrl.
pub fn key_ctrl() -> bool {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_key_ctrl() }
}
/// ImParseFormatPrecision.
pub fn parse_format_precision(format: &CStr, default_value: i32) -> i32 {
    // SAFETY: a C string.
    unsafe { cvr_imgui_parse_format_precision(format.as_ptr(), default_value) }
}
/// ImGui::DataTypeFormatString into a 96-byte buffer.
///
/// # Safety
/// `data` points to a value of `data_type`.
pub unsafe fn data_type_format_string(
    data_type: i32,
    data: *const c_void,
    format: &CStr,
) -> [c_char; 96] {
    let mut buffer = [0 as c_char; 96];
    // SAFETY: the buffer and its size, the caller's value.
    unsafe {
        cvr_imgui_data_type_format_string(buffer.as_mut_ptr(), 96, data_type, data, format.as_ptr())
    };
    buffer
}
pub fn temp_input_is_active(id: u32) -> bool {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_temp_input_is_active(id) }
}
/// ImGui::SliderScalar.
///
/// # Safety
/// `data`, `minimum` and `maximum` point to values of `data_type`.
pub unsafe fn slider_scalar(
    label: &CStr,
    data_type: i32,
    data: *mut c_void,
    minimum: *const c_void,
    maximum: *const c_void,
    format: &CStr,
    flags: i32,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe {
        cvr_imgui_slider_scalar(
            label.as_ptr(),
            data_type,
            data,
            minimum,
            maximum,
            format.as_ptr(),
            flags,
        )
    }
}
pub fn begin_combo(label: &CStr, preview: &CStr, flags: i32) -> bool {
    // SAFETY: C strings.
    unsafe { cvr_imgui_begin_combo(label.as_ptr(), preview.as_ptr(), flags) }
}
pub fn end_combo() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_end_combo() }
}
pub fn selectable(label: *const c_char, selected: bool, flags: i32, size: Vec2) -> bool {
    // SAFETY: the caller's C string.
    unsafe { cvr_imgui_selectable(label, selected, flags, size[0], size[1]) }
}
pub fn set_item_default_focus() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_set_item_default_focus() }
}
/// ImGui::TextWrapped of a text, not a format.
pub fn text_wrapped(text: &CStr) {
    // SAFETY: a C string.
    unsafe { cvr_imgui_text_wrapped(text.as_ptr()) }
}
pub fn begin_table(id: &CStr, columns: i32, flags: i32) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_begin_table(id.as_ptr(), columns, flags) }
}
pub fn table_setup_column(label: &CStr, flags: i32, weight: f32) {
    // SAFETY: a C string.
    unsafe { cvr_imgui_table_setup_column(label.as_ptr(), flags, weight) }
}
pub fn table_next_row(flags: i32, min_height: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_table_next_row(flags, min_height) }
}
pub fn table_next_column() -> bool {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_table_next_column() }
}
pub fn end_table() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_end_table() }
}
impl DrawList {
    pub fn convex_poly_filled(self, points: &[Vec2], color: u32) {
        // SAFETY: the frame's draw list and the caller's points.
        unsafe {
            cvr_imgui_draw_convex_poly_filled(self.0, points.as_ptr(), points.len() as i32, color)
        }
    }
    /// ImDrawList::AddText(font, size, pos, color, begin, end, wrap_width); `end` may be null.
    pub fn text(
        self,
        font: Font,
        size: f32,
        pos: Vec2,
        color: u32,
        begin: *const c_char,
        end: *const c_char,
        wrap_width: f32,
    ) {
        // SAFETY: the frame's draw list, its font and the caller's text.
        unsafe {
            cvr_imgui_draw_text(
                self.0, font.0, size, pos[0], pos[1], color, begin, end, wrap_width,
            )
        }
    }
    pub fn push_clip_rect(self, a: Vec2, b: Vec2, intersect: bool) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_push_clip_rect(self.0, a[0], a[1], b[0], b[1], intersect) }
    }
    pub fn pop_clip_rect(self) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_pop_clip_rect(self.0) }
    }
    /// AddCircleFilled with ImGui's automatic segment count.
    pub fn circle_filled(self, centre: Vec2, radius: f32, color: u32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_circle_filled(self.0, centre[0], centre[1], radius, color, 0) }
    }
    /// AddCircle with ImGui's automatic segment count.
    pub fn circle(self, centre: Vec2, radius: f32, color: u32, thickness: f32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_circle(self.0, centre[0], centre[1], radius, color, 0, thickness) }
    }
    /// AddCircleFilled with this many segments.
    pub fn circle_filled_segments(self, centre: Vec2, radius: f32, color: u32, segments: i32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_circle_filled(self.0, centre[0], centre[1], radius, color, segments) }
    }
    /// AddCircle with this many segments.
    pub fn circle_segments(self, centre: Vec2, radius: f32, color: u32, segments: i32, thickness: f32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_circle(self.0, centre[0], centre[1], radius, color, segments, thickness) }
    }
    /// AddRect(a, b, color, rounding, flags, thickness).
    pub fn rect(self, a: Vec2, b: Vec2, color: u32, rounding: f32, flags: i32, thickness: f32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_rect(self.0, a[0], a[1], b[0], b[1], color, rounding, flags, thickness) }
    }
    /// AddRectFilled(a, b, color, rounding, flags).
    pub fn rect_filled_rounded(self, a: Vec2, b: Vec2, color: u32, rounding: f32, flags: i32) {
        // SAFETY: the frame's draw list.
        unsafe { cvr_imgui_draw_rect_filled_rounded(self.0, a[0], a[1], b[0], b[1], color, rounding, flags) }
    }
    /// AddText(pos, color, text) in the list's current font and size.
    pub fn text_default(self, pos: Vec2, color: u32, text: &CStr) {
        // SAFETY: the frame's draw list and a C string (a null font and size select the list's own).
        unsafe { cvr_imgui_draw_text(self.0, core::ptr::null_mut(), 0.0, pos[0], pos[1], color, text.as_ptr(), core::ptr::null(), 0.0) }
    }
}

// ---- ImGui's own widgets, for the panels' facade (crate::ui) -------------------------------------------------------

extern "C" {
    fn cvr_imgui_checkbox(label: *const c_char, value: *mut bool) -> bool;
    fn cvr_imgui_set_next_window_pos_cond(x: f32, y: f32, cond: i32);
    fn cvr_imgui_set_next_window_size_cond(w: f32, h: f32, cond: i32);
    fn cvr_imgui_tree_node(label: *const c_char) -> bool;
    fn cvr_imgui_tree_pop();
    fn cvr_imgui_drag_float(label: *const c_char, v: *mut f32, speed: f32, lo: f32, hi: f32, format: *const c_char) -> bool;
    fn cvr_imgui_drag_int(label: *const c_char, v: *mut i32, speed: f32, lo: i32, hi: i32) -> bool;
    fn cvr_imgui_slider_int_plain(label: *const c_char, v: *mut i32, lo: i32, hi: i32) -> bool;
    fn cvr_imgui_radio_button(label: *const c_char, active: bool) -> bool;
    fn cvr_imgui_small_button(label: *const c_char) -> bool;
    fn cvr_imgui_input_text(label: *const c_char, buffer: *mut c_char, size: usize) -> bool;
    fn cvr_imgui_slider_float(
        label: *const c_char,
        value: *mut f32,
        minimum: f32,
        maximum: f32,
        format: *const c_char,
        flags: i32,
    ) -> bool;
    fn cvr_imgui_input_int(
        label: *const c_char,
        value: *mut i32,
        step: i32,
        fast_step: i32,
        flags: i32,
    ) -> bool;
    fn cvr_imgui_collapsing_header(label: *const c_char, flags: i32) -> bool;
    fn cvr_imgui_begin_tab_bar(id: *const c_char) -> bool;
    fn cvr_imgui_end_tab_bar();
    fn cvr_imgui_begin_tab_item(label: *const c_char) -> bool;
    fn cvr_imgui_end_tab_item();
    fn cvr_imgui_text(text: *const c_char);
    fn cvr_imgui_text_unformatted(text: *const c_char);
    fn cvr_imgui_set_tooltip(text: *const c_char);
    fn cvr_imgui_is_any_item_active() -> bool;
    fn cvr_imgui_indent(width: f32);
    fn cvr_imgui_unindent(width: f32);
    fn cvr_imgui_progress_bar(fraction: f32, w: f32, h: f32, overlay: *const c_char);
    fn cvr_imgui_plot_lines(
        label: *const c_char,
        values: *const f32,
        count: i32,
        offset: i32,
        overlay: *const c_char,
        minimum: f32,
        maximum: f32,
        w: f32,
        h: f32,
        stride: i32,
    );
}
/// SetNextWindowPos with an ImGuiCond (FirstUseEver: 4).
pub fn set_next_window_pos_cond(p: Vec2, cond: i32) {
    // SAFETY: plain values.
    unsafe { cvr_imgui_set_next_window_pos_cond(p[0], p[1], cond) }
}
/// SetNextWindowSize with an ImGuiCond.
pub fn set_next_window_size_cond(s: Vec2, cond: i32) {
    // SAFETY: plain values.
    unsafe { cvr_imgui_set_next_window_size_cond(s[0], s[1], cond) }
}
pub fn tree_node(label: &CStr) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_tree_node(label.as_ptr()) }
}
pub fn tree_pop() {
    // SAFETY: after a tree_node that opened.
    unsafe { cvr_imgui_tree_pop() }
}
pub fn drag_float(label: &CStr, v: &mut f32, speed: f32, lo: f32, hi: f32, format: &CStr) -> bool {
    // SAFETY: C strings and the caller's float.
    unsafe { cvr_imgui_drag_float(label.as_ptr(), v, speed, lo, hi, format.as_ptr()) }
}
pub fn drag_int(label: &CStr, v: &mut i32, speed: f32, lo: i32, hi: i32) -> bool {
    // SAFETY: a C string and the caller's int.
    unsafe { cvr_imgui_drag_int(label.as_ptr(), v, speed, lo, hi) }
}
/// ImGui's own SliderInt (the VR shell's is ui::slider_int).
pub fn slider_int_plain(label: &CStr, v: &mut i32, lo: i32, hi: i32) -> bool {
    // SAFETY: a C string and the caller's int.
    unsafe { cvr_imgui_slider_int_plain(label.as_ptr(), v, lo, hi) }
}
pub fn radio_button(label: &CStr, active: bool) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_radio_button(label.as_ptr(), active) }
}
pub fn small_button(label: &CStr) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_small_button(label.as_ptr()) }
}
/// InputText over a NUL-terminated buffer the caller keeps.
pub fn input_text(label: &CStr, buffer: &mut [u8]) -> bool {
    // SAFETY: a C string and the caller's buffer of its length.
    unsafe { cvr_imgui_input_text(label.as_ptr(), buffer.as_mut_ptr().cast(), buffer.len()) }
}
pub fn checkbox(label: &CStr, value: &mut bool) -> bool {
    // SAFETY: a C string and the caller's bool.
    unsafe { cvr_imgui_checkbox(label.as_ptr(), value) }
}
pub fn slider_float(
    label: &CStr,
    value: &mut f32,
    minimum: f32,
    maximum: f32,
    format: &CStr,
    flags: i32,
) -> bool {
    // SAFETY: C strings and the caller's float.
    unsafe {
        cvr_imgui_slider_float(
            label.as_ptr(),
            value,
            minimum,
            maximum,
            format.as_ptr(),
            flags,
        )
    }
}
pub fn input_int(label: &CStr, value: &mut i32, step: i32, fast_step: i32, flags: i32) -> bool {
    // SAFETY: a C string and the caller's int.
    unsafe { cvr_imgui_input_int(label.as_ptr(), value, step, fast_step, flags) }
}
pub fn collapsing_header(label: &CStr, flags: i32) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_collapsing_header(label.as_ptr(), flags) }
}
pub fn begin_tab_bar(id: &CStr) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_begin_tab_bar(id.as_ptr()) }
}
pub fn end_tab_bar() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_end_tab_bar() }
}
pub fn begin_tab_item(label: &CStr) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_begin_tab_item(label.as_ptr()) }
}
pub fn end_tab_item() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_end_tab_item() }
}
/// ImGui::Text of a text, not a format.
pub fn text(text: &CStr) {
    // SAFETY: a C string.
    unsafe { cvr_imgui_text(text.as_ptr()) }
}
pub fn text_unformatted(text: &CStr) {
    // SAFETY: a C string.
    unsafe { cvr_imgui_text_unformatted(text.as_ptr()) }
}
/// ImGui::SetTooltip of a text, not a format.
pub fn set_tooltip(text: &CStr) {
    // SAFETY: a C string.
    unsafe { cvr_imgui_set_tooltip(text.as_ptr()) }
}
pub fn is_item_hovered_flags(flags: i32) -> bool {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_is_item_hovered(flags) }
}
pub fn is_any_item_active() -> bool {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_is_any_item_active() }
}
pub fn indent(width: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_indent(width) }
}
pub fn unindent(width: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_unindent(width) }
}
/// ImGui::ProgressBar without an overlay text.
pub fn progress_bar(fraction: f32, size: Vec2) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_progress_bar(fraction, size[0], size[1], core::ptr::null()) }
}
/// ImGui::PlotLines(label, values, count, 0, nullptr, minimum, maximum, size, sizeof(float)).
///
/// # Safety
/// `values` points to `count` floats.
pub unsafe fn plot_lines(
    label: &CStr,
    values: *const f32,
    count: i32,
    minimum: f32,
    maximum: f32,
    size: Vec2,
) {
    // SAFETY: as the caller promises.
    unsafe {
        cvr_imgui_plot_lines(
            label.as_ptr(),
            values,
            count,
            0,
            core::ptr::null(),
            minimum,
            maximum,
            size[0],
            size[1],
            4,
        )
    }
}

// ---- the overlay's: the context, the backends, the IO, the frame's draw data (crate::imgui_overlay) ---------------

pub const CONFIG_NAV_ENABLE_KEYBOARD: i32 = 1;

extern "C" {
    fn cvr_imgui_check_version();
    fn cvr_imgui_create_context();
    fn cvr_imgui_destroy_context();
    fn cvr_imgui_io() -> *mut u8;
    fn cvr_imgui_add_font_file_cyrillic(path: *const c_char, size: f32) -> bool;
    fn cvr_imgui_add_font_default(size: f32);
    fn cvr_imgui_font_glyph(index: i32, codepoint: *mut u32, u0: *mut f32, v0: *mut f32) -> bool;
    fn cvr_imgui_io_add_mouse_pos_event(x: f32, y: f32);
    fn cvr_imgui_io_add_mouse_button_event(button: i32, down: bool);
    fn cvr_imgui_new_frame();
    fn cvr_imgui_render();
    fn cvr_imgui_get_draw_data() -> *mut u8;
    fn cvr_imgui_background_draw_list() -> *mut c_void;
    fn cvr_imgui_win32_init(hwnd: *mut c_void) -> bool;
    fn cvr_imgui_win32_shutdown();
    fn cvr_imgui_win32_new_frame();
    fn cvr_imgui_win32_wndproc(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn cvr_imgui_dx12_init(
        device: *mut c_void,
        frames: i32,
        format: i32,
        heap: *mut c_void,
        cpu: u64,
        gpu: u64,
    ) -> bool;
    fn cvr_imgui_dx12_shutdown();
    fn cvr_imgui_dx12_new_frame();
    fn cvr_imgui_dx12_render(data: *mut u8, list: *mut c_void);
}
/// IMGUI_CHECKVERSION, then ImGui::CreateContext.
pub fn create_context() {
    // SAFETY: plain calls.
    unsafe {
        cvr_imgui_check_version();
        cvr_imgui_create_context();
    }
}
pub fn destroy_context() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_destroy_context() }
}
/// io.Fonts->AddFontFromFileTTF(path, size, nullptr, the Cyrillic ranges); false when the file could not be added.
pub fn add_font_file_cyrillic(path: &CStr, size: f32) -> bool {
    // SAFETY: a C string.
    unsafe { cvr_imgui_add_font_file_cyrillic(path.as_ptr(), size) }
}
/// io.Fonts->AddFontDefault with SizePixels = size.
pub fn add_font_default(size: f32) {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_add_font_default(size) }
}
/// The default font's glyph `index` (its codepoint and atlas corner), none past the last.
pub fn font_glyph(index: i32) -> Option<(u32, f32, f32)> {
    let (mut c, mut u, mut v) = (0u32, 0f32, 0f32);
    // SAFETY: slots to fill.
    unsafe { cvr_imgui_font_glyph(index, &mut c, &mut u, &mut v) }.then_some((c, u, v))
}
pub fn new_frame() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_new_frame() }
}
pub fn render() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_render() }
}
/// ImGui::GetBackgroundDrawList, as the draw data's lists hold it.
/// The background draw list, when there is one.
pub fn background_draw_list_handle() -> Option<DrawList> {
    let list = background_draw_list();
    (!list.is_null()).then_some(DrawList(list))
}
pub fn background_draw_list() -> *mut c_void {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_background_draw_list() }
}
pub fn win32_init(hwnd: *mut c_void) -> bool {
    // SAFETY: the caller's window.
    unsafe { cvr_imgui_win32_init(hwnd) }
}
pub fn win32_shutdown() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_win32_shutdown() }
}
pub fn win32_new_frame() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_win32_new_frame() }
}
/// ImGui_ImplWin32_WndProcHandler: non-zero when ImGui took the message.
pub fn win32_wndproc(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: the window procedure's own arguments.
    unsafe { cvr_imgui_win32_wndproc(hwnd, msg, wparam, lparam) }
}
/// ImGui_ImplDX12_Init.
///
/// # Safety
/// `device` and `heap` are an ID3D12Device and its shader-visible SRV heap; `cpu` / `gpu` the heap's start.
pub unsafe fn dx12_init(
    device: *mut c_void,
    frames: i32,
    format: i32,
    heap: *mut c_void,
    cpu: u64,
    gpu: u64,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { cvr_imgui_dx12_init(device, frames, format, heap, cpu, gpu) }
}
pub fn dx12_shutdown() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_dx12_shutdown() }
}
pub fn dx12_new_frame() {
    // SAFETY: a plain call.
    unsafe { cvr_imgui_dx12_new_frame() }
}
/// ImGui_ImplDX12_RenderDrawData.
///
/// # Safety
/// `data` is this frame's draw data; `list` a recording ID3D12GraphicsCommandList.
pub unsafe fn dx12_render(data: DrawData, list: *mut c_void) {
    // SAFETY: as the caller promises.
    unsafe { cvr_imgui_dx12_render(data.0, list) }
}

/// ImGui::GetIO(), for the fields the overlay sets (by offset, checked by src/Overlay/ImGuiAbi.cpp).
pub struct Io(*mut u8);
impl Io {
    const CONFIG_FLAGS: usize = 0;
    const DISPLAY_SIZE: usize = 8;
    const INI_FILENAME: usize = 24;
    const FONT_GLOBAL_SCALE: usize = 56;
    const DISPLAY_FRAMEBUFFER_SCALE: usize = 72;
    const MOUSE_DRAW_CURSOR: usize = 88;
    const MOUSE_WHEEL: usize = 240;
    const MOUSE_WHEEL_H: usize = 244;
    pub fn get() -> Io {
        // SAFETY: a plain call on the current context.
        Io(unsafe { cvr_imgui_io() })
    }
    fn at<T>(&self, offset: usize) -> *mut T {
        // SAFETY: a field of ImGuiIO.
        unsafe { self.0.add(offset).cast::<T>() }
    }
    pub fn add_config_flags(&mut self, flags: i32) {
        // SAFETY: ImGuiIO::ConfigFlags.
        unsafe { *self.at::<i32>(Self::CONFIG_FLAGS) |= flags }
    }
    pub fn clear_ini_filename(&mut self) {
        // SAFETY: ImGuiIO::IniFilename.
        unsafe { *self.at::<*const c_char>(Self::INI_FILENAME) = core::ptr::null() }
    }
    pub fn display_size(&self) -> Vec2 {
        // SAFETY: ImGuiIO::DisplaySize.
        unsafe { *self.at::<Vec2>(Self::DISPLAY_SIZE) }
    }
    pub fn set_display_size(&mut self, v: Vec2) {
        // SAFETY: ImGuiIO::DisplaySize.
        unsafe { *self.at::<Vec2>(Self::DISPLAY_SIZE) = v }
    }
    pub fn set_display_framebuffer_scale(&mut self, v: Vec2) {
        // SAFETY: ImGuiIO::DisplayFramebufferScale.
        unsafe { *self.at::<Vec2>(Self::DISPLAY_FRAMEBUFFER_SCALE) = v }
    }
    pub fn set_font_global_scale(&mut self, v: f32) {
        // SAFETY: ImGuiIO::FontGlobalScale.
        unsafe { *self.at::<f32>(Self::FONT_GLOBAL_SCALE) = v }
    }
    pub fn set_mouse_draw_cursor(&mut self, v: bool) {
        // SAFETY: ImGuiIO::MouseDrawCursor.
        unsafe { *self.at::<bool>(Self::MOUSE_DRAW_CURSOR) = v }
    }
    /// io.MouseWheel += v.
    pub fn add_mouse_wheel(&mut self, v: f32) {
        // SAFETY: ImGuiIO::MouseWheel.
        unsafe { *self.at::<f32>(Self::MOUSE_WHEEL) += v }
    }
    /// io.MouseWheelH += v.
    pub fn add_mouse_wheel_h(&mut self, v: f32) {
        // SAFETY: ImGuiIO::MouseWheelH.
        unsafe { *self.at::<f32>(Self::MOUSE_WHEEL_H) += v }
    }
    pub fn add_mouse_pos_event(&mut self, x: f32, y: f32) {
        // SAFETY: a plain call.
        unsafe { cvr_imgui_io_add_mouse_pos_event(x, y) }
    }
    pub fn add_mouse_button_event(&mut self, button: i32, down: bool) {
        // SAFETY: a plain call.
        unsafe { cvr_imgui_io_add_mouse_button_event(button, down) }
    }
}

/// ImGui's ImVector: Size, Capacity, Data.
#[repr(C)]
pub struct ImVector<T> {
    pub size: i32,
    pub capacity: i32,
    pub data: *mut T,
}
impl<T> ImVector<T> {
    pub fn as_slice(&self) -> &[T] {
        if self.data.is_null() || self.size <= 0 {
            return &[];
        }
        // SAFETY: the vector's live elements.
        unsafe { core::slice::from_raw_parts(self.data, self.size as usize) }
    }
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        if self.data.is_null() || self.size <= 0 {
            return &mut [];
        }
        // SAFETY: the vector's live elements.
        unsafe { core::slice::from_raw_parts_mut(self.data, self.size as usize) }
    }
}
/// ImDrawVert.
#[repr(C)]
pub struct DrawVert {
    pub pos: Vec2,
    pub uv: Vec2,
    pub col: u32,
}
/// ImDrawCmd (56 bytes; the fields read).
#[repr(C)]
pub struct DrawCmd {
    pub clip_rect: Vec4,
    pub texture_id: *mut c_void,
    pub vtx_offset: u32,
    pub idx_offset: u32,
    pub elem_count: u32,
    pub user_callback: *mut c_void,
    _rest: [u8; 8],
}
/// ImDrawList's head (the buffers) and its owner's name.
#[repr(C)]
pub struct DrawListData {
    pub cmd_buffer: ImVector<DrawCmd>,
    pub idx_buffer: ImVector<u16>,
    pub vtx_buffer: ImVector<DrawVert>,
}
impl DrawListData {
    pub fn owner_name(&self) -> *const c_char {
        // SAFETY: ImDrawList::_OwnerName, at 192.
        unsafe {
            *(self as *const Self as *const u8)
                .add(192)
                .cast::<*const c_char>()
        }
    }
}
/// ImDrawData (64 bytes).
#[repr(C)]
pub struct DrawDataFields {
    pub valid: bool,
    pub cmd_lists_count: i32,
    pub total_idx_count: i32,
    pub total_vtx_count: i32,
    pub cmd_lists: ImVector<*mut DrawListData>,
    pub display_pos: Vec2,
    pub display_size: Vec2,
    pub framebuffer_scale: Vec2,
    _owner_viewport: *mut c_void,
}
const _: () = assert!(
    core::mem::size_of::<DrawCmd>() == 56
        && core::mem::size_of::<DrawVert>() == 20
        && core::mem::size_of::<DrawDataFields>() == 64
        && core::mem::offset_of!(DrawDataFields, cmd_lists) == 16
        && core::mem::offset_of!(DrawDataFields, display_pos) == 32
);
/// ImGui::GetDrawData(): this frame's draw data, or null.
#[derive(Clone, Copy)]
pub struct DrawData(*mut u8);
impl DrawData {
    pub fn get() -> DrawData {
        // SAFETY: a plain call.
        DrawData(unsafe { cvr_imgui_get_draw_data() })
    }
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }
    /// The fields, while the frame's draw data lives (until the next ImGui::Render).
    ///
    /// # Safety
    /// Not null, and no other reference to it held.
    pub unsafe fn fields<'a>(self) -> &'a mut DrawDataFields {
        // SAFETY: as the caller promises.
        unsafe { &mut *self.0.cast::<DrawDataFields>() }
    }
}
