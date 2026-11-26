// THE IMGUI BOUNDARY. Dear ImGui (v1.91.1-docking, CMakeLists.txt) stays the external library it is; the port's own
// ImGui code is Rust's (rust/crates/cvr-render/src/imgui.rs and its callers) and reaches the library through these
// plain C forwarders -- one ImGui call each, nothing of the port's. ImGuiStyle is written by field offset, checked here.
#if CVR_RUST_VRIK
#include <windows.h>
#include <d3d12.h>
#include <imgui.h>
#include <imgui_internal.h>
#include <imgui_impl_dx12.h>
#include <imgui_impl_win32.h>

#include <cstddef>
#include <cstdint>

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

static_assert(offsetof(ImGuiIO, ConfigFlags) == 0 && offsetof(ImGuiIO, DisplaySize) == 8 &&
                  offsetof(ImGuiIO, IniFilename) == 24 && offsetof(ImGuiIO, FontGlobalScale) == 56 &&
                  offsetof(ImGuiIO, DisplayFramebufferScale) == 72 && offsetof(ImGuiIO, MouseDrawCursor) == 88 &&
                  offsetof(ImGuiIO, MouseWheel) == 240 && offsetof(ImGuiIO, MouseWheelH) == 244 &&
                  ImGuiConfigFlags_NavEnableKeyboard == 1,
              "the ImGuiIO fields rust/crates/cvr-render/src/imgui.rs writes");
static_assert(sizeof(ImVector<int>) == 16 && offsetof(ImDrawData, Valid) == 0 &&
                  offsetof(ImDrawData, CmdListsCount) == 4 && offsetof(ImDrawData, CmdLists) == 16 &&
                  offsetof(ImDrawData, DisplayPos) == 32 && offsetof(ImDrawData, DisplaySize) == 40 &&
                  offsetof(ImDrawData, FramebufferScale) == 48 && offsetof(ImDrawData, TotalIdxCount) == 8 &&
                  offsetof(ImDrawData, TotalVtxCount) == 12 && offsetof(ImDrawList, CmdBuffer) == 0 &&
                  offsetof(ImDrawList, IdxBuffer) == 16 && offsetof(ImDrawList, VtxBuffer) == 32 &&
                  offsetof(ImDrawList, _OwnerName) == 192 && sizeof(ImDrawCmd) == 56 &&
                  offsetof(ImDrawCmd, ClipRect) == 0 && offsetof(ImDrawCmd, TextureId) == 16 &&
                  offsetof(ImDrawCmd, VtxOffset) == 24 && offsetof(ImDrawCmd, IdxOffset) == 28 &&
                  offsetof(ImDrawCmd, ElemCount) == 32 && offsetof(ImDrawCmd, UserCallback) == 40 &&
                  sizeof(ImDrawVert) == 20 && offsetof(ImDrawVert, uv) == 8 && offsetof(ImDrawVert, col) == 16,
              "the draw data rust/crates/cvr-render/src/imgui.rs reads and edits");

static_assert(sizeof(ImGuiStyle) == 1192 && offsetof(ImGuiStyle, WindowPadding) == 8 &&
                  offsetof(ImGuiStyle, WindowRounding) == 16 && offsetof(ImGuiStyle, WindowBorderSize) == 20 &&
                  offsetof(ImGuiStyle, ChildRounding) == 44 && offsetof(ImGuiStyle, ChildBorderSize) == 48 &&
                  offsetof(ImGuiStyle, PopupRounding) == 52 && offsetof(ImGuiStyle, PopupBorderSize) == 56 &&
                  offsetof(ImGuiStyle, FramePadding) == 60 && offsetof(ImGuiStyle, FrameRounding) == 68 &&
                  offsetof(ImGuiStyle, FrameBorderSize) == 72 && offsetof(ImGuiStyle, ItemSpacing) == 76 &&
                  offsetof(ImGuiStyle, ItemInnerSpacing) == 84 && offsetof(ImGuiStyle, TouchExtraPadding) == 100 &&
                  offsetof(ImGuiStyle, IndentSpacing) == 108 && offsetof(ImGuiStyle, ScrollbarSize) == 116 &&
                  offsetof(ImGuiStyle, GrabMinSize) == 124 && offsetof(ImGuiStyle, Colors) == 244 &&
                  ImGuiCol_COUNT == 58,
              "mirrored by the style offsets in rust/crates/cvr-render/src/imgui.rs");
static_assert(ImGuiCol_Text == 0 && ImGuiCol_TextDisabled == 1 && ImGuiCol_WindowBg == 2 && ImGuiCol_ChildBg == 3 &&
                  ImGuiCol_PopupBg == 4 && ImGuiCol_Border == 5 && ImGuiCol_FrameBg == 7 &&
                  ImGuiCol_FrameBgHovered == 8 && ImGuiCol_FrameBgActive == 9 && ImGuiCol_ScrollbarBg == 14 &&
                  ImGuiCol_ScrollbarGrab == 15 && ImGuiCol_CheckMark == 18 && ImGuiCol_SliderGrab == 19 &&
                  ImGuiCol_SliderGrabActive == 20 && ImGuiCol_Button == 21 && ImGuiCol_ButtonHovered == 22 &&
                  ImGuiCol_ButtonActive == 23 && ImGuiCol_Header == 24 && ImGuiCol_HeaderHovered == 25 &&
                  ImGuiCol_HeaderActive == 26 && ImGuiCol_Separator == 27 && ImGuiCol_TableBorderStrong == 47 &&
                  ImGuiCol_TableBorderLight == 48 && ImGuiCol_TextSelectedBg == 52 &&
                  ImGuiStyleVar_FrameBorderSize == 13 && ImGuiStyleVar_ItemSpacing == 14 &&
                  ImGuiStyleVar_ButtonTextAlign == 28 && ImGuiWindowFlags_NoMove == 4 &&
                  ImGuiWindowFlags_NoSavedSettings == 256 && ImGuiWindowFlags_NoDecoration == 43 &&
                  ImDrawFlags_Closed == 1,
              "mirrored by the constants in rust/crates/cvr-render/src/imgui.rs");
static_assert(ImGuiStyleVar_FramePadding == 11 && ImGuiStyleVar_CellPadding == 17 && ImGuiStyleVar_GrabMinSize == 20 &&
                  ImGuiDataType_S32 == 4 && ImGuiDataType_Float == 8 && ImGuiSliderFlags_AlwaysClamp == 16 &&
                  ImGuiComboFlags_HeightLarge == 8 && ImGuiComboFlags_NoArrowButton == 32 &&
                  ImGuiTableFlags_BordersInnerH == 128 && ImGuiTableFlags_SizingStretchProp == 24576 &&
                  ImGuiTableColumnFlags_WidthStretch == 8 && sizeof(ImGuiID) == 4,
              "mirrored by the constants in rust/crates/cvr-render/src/imgui.rs");
static_assert(sizeof(ImVec2) == 8 && sizeof(ImDrawIdx) == 2, "ImVec2 is [f32; 2] in imgui.rs");

extern "C" {
// The style.
ImGuiStyle* cvr_imgui_style() { return &ImGui::GetStyle(); }
void cvr_imgui_style_reset() { ImGui::GetStyle() = ImGuiStyle(); }
void cvr_imgui_push_style_color(int idx, float r, float g, float b, float a) {
    ImGui::PushStyleColor(idx, ImVec4(r, g, b, a));
}
void cvr_imgui_pop_style_color(int count) { ImGui::PopStyleColor(count); }
void cvr_imgui_push_style_var_float(int idx, float value) { ImGui::PushStyleVar(idx, value); }
void cvr_imgui_push_style_var_vec2(int idx, float x, float y) { ImGui::PushStyleVar(idx, ImVec2(x, y)); }
void cvr_imgui_pop_style_var(int count) { ImGui::PopStyleVar(count); }
unsigned cvr_imgui_color_u32(float r, float g, float b, float a) {
    return ImGui::ColorConvertFloat4ToU32(ImVec4(r, g, b, a));
}
void cvr_imgui_set_window_font_scale(float scale) { ImGui::SetWindowFontScale(scale); }
void cvr_imgui_push_item_width(float width) { ImGui::PushItemWidth(width); }
void cvr_imgui_pop_item_width() { ImGui::PopItemWidth(); }

// Windows and layout.
void cvr_imgui_set_next_window_pos(float x, float y) { ImGui::SetNextWindowPos(ImVec2(x, y)); }
void cvr_imgui_set_next_window_size(float w, float h) { ImGui::SetNextWindowSize(ImVec2(w, h)); }
void cvr_imgui_set_next_window_pos_pivot(float x, float y, float pivotX, float pivotY) {
    ImGui::SetNextWindowPos(ImVec2(x, y), ImGuiCond_Always, ImVec2(pivotX, pivotY));
}
void cvr_imgui_set_next_window_bg_alpha(float alpha) { ImGui::SetNextWindowBgAlpha(alpha); }
bool cvr_imgui_begin(const char* name, int flags) { return ImGui::Begin(name, nullptr, flags); }
void cvr_imgui_end() { ImGui::End(); }
bool cvr_imgui_begin_child(const char* id, float w, float h, int childFlags, int windowFlags) {
    return ImGui::BeginChild(id, ImVec2(w, h), childFlags, windowFlags);
}
void cvr_imgui_end_child() { ImGui::EndChild(); }
void cvr_imgui_same_line(float offsetFromStartX, float spacing) { ImGui::SameLine(offsetFromStartX, spacing); }
void cvr_imgui_separator() { ImGui::Separator(); }
void cvr_imgui_push_id_int(int id) { ImGui::PushID(id); }
void cvr_imgui_pop_id() { ImGui::PopID(); }
void cvr_imgui_cursor_screen_pos(ImVec2* out) { *out = ImGui::GetCursorScreenPos(); }
void cvr_imgui_content_region_avail(ImVec2* out) { *out = ImGui::GetContentRegionAvail(); }
void cvr_imgui_calc_text_size(const char* text, ImVec2* out) { *out = ImGui::CalcTextSize(text); }
void cvr_imgui_mouse_pos(ImVec2* out) { *out = ImGui::GetIO().MousePos; }

// Items.
bool cvr_imgui_button(const char* label, float w, float h) { return ImGui::Button(label, ImVec2(w, h)); }
void cvr_imgui_text_colored(float r, float g, float b, float a, const char* text) {
    ImGui::TextColored(ImVec4(r, g, b, a), "%s", text);
}
void cvr_imgui_text_disabled(const char* text) { ImGui::TextDisabled("%s", text); }
bool cvr_imgui_is_item_hovered(int flags) { return ImGui::IsItemHovered(flags); }
bool cvr_imgui_is_item_active() { return ImGui::IsItemActive(); }
void cvr_imgui_item_rect_min(ImVec2* out) { *out = ImGui::GetItemRectMin(); }
void cvr_imgui_item_rect_max(ImVec2* out) { *out = ImGui::GetItemRectMax(); }
bool cvr_imgui_is_mouse_hovering_rect(float x0, float y0, float x1, float y1) {
    return ImGui::IsMouseHoveringRect(ImVec2(x0, y0), ImVec2(x1, y1));
}

// The VR widgets (rust/crates/cvr-render/src/vr_widgets.rs): groups, ids, text measures, and imgui_internal's
// formatting, temp input and button repeat.
void cvr_imgui_begin_group() { ImGui::BeginGroup(); }
void cvr_imgui_end_group() { ImGui::EndGroup(); }
void cvr_imgui_push_id_str(const char* id) { ImGui::PushID(id); }
unsigned cvr_imgui_get_id(const char* id) { return ImGui::GetID(id); }
const char* cvr_imgui_find_rendered_text_end(const char* text) { return ImGui::FindRenderedTextEnd(text); }
void cvr_imgui_calc_text_size_ex(const char* text, const char* end, bool hideAfterDoubleHash, float wrapWidth,
                                 ImVec2* out) {
    *out = ImGui::CalcTextSize(text, end, hideAfterDoubleHash, wrapWidth);
}
float cvr_imgui_frame_height() { return ImGui::GetFrameHeight(); }
float cvr_imgui_font_size() { return ImGui::GetFontSize(); }
ImFont* cvr_imgui_font() { return ImGui::GetFont(); }
unsigned cvr_imgui_get_color_u32(float r, float g, float b, float a) { return ImGui::GetColorU32(ImVec4(r, g, b, a)); }
void cvr_imgui_dummy(float w, float h) { ImGui::Dummy(ImVec2(w, h)); }
void cvr_imgui_spacing() { ImGui::Spacing(); }
void cvr_imgui_set_cursor_screen_pos(float x, float y) { ImGui::SetCursorScreenPos(ImVec2(x, y)); }
void cvr_imgui_set_next_item_width(float width) { ImGui::SetNextItemWidth(width); }
void cvr_imgui_begin_disabled(bool disabled) { ImGui::BeginDisabled(disabled); }
void cvr_imgui_end_disabled() { ImGui::EndDisabled(); }
void cvr_imgui_push_button_repeat(bool repeat) { ImGui::PushButtonRepeat(repeat); }
void cvr_imgui_pop_button_repeat() { ImGui::PopButtonRepeat(); }
bool cvr_imgui_key_ctrl() { return ImGui::GetIO().KeyCtrl; }
int cvr_imgui_parse_format_precision(const char* format, int defaultValue) {
    return ImParseFormatPrecision(format, defaultValue);
}
int cvr_imgui_data_type_format_string(char* buffer, int size, int type, const void* data, const char* format) {
    return ImGui::DataTypeFormatString(buffer, size, type, data, format);
}
bool cvr_imgui_temp_input_is_active(unsigned id) { return ImGui::TempInputIsActive(id); }
bool cvr_imgui_slider_scalar(const char* label, int type, void* data, const void* minimum, const void* maximum,
                             const char* format, int flags) {
    return ImGui::SliderScalar(label, type, data, minimum, maximum, format, flags);
}
bool cvr_imgui_begin_combo(const char* label, const char* preview, int flags) {
    return ImGui::BeginCombo(label, preview, flags);
}
void cvr_imgui_end_combo() { ImGui::EndCombo(); }
bool cvr_imgui_selectable(const char* label, bool selected, int flags, float w, float h) {
    return ImGui::Selectable(label, selected, flags, ImVec2(w, h));
}
void cvr_imgui_set_item_default_focus() { ImGui::SetItemDefaultFocus(); }
void cvr_imgui_text_wrapped(const char* text) { ImGui::TextWrapped("%s", text); }
bool cvr_imgui_begin_table(const char* id, int columns, int flags) { return ImGui::BeginTable(id, columns, flags); }
void cvr_imgui_table_setup_column(const char* label, int flags, float weight) {
    ImGui::TableSetupColumn(label, flags, weight);
}
void cvr_imgui_table_next_row(int flags, float minHeight) { ImGui::TableNextRow(flags, minHeight); }
bool cvr_imgui_table_next_column() { return ImGui::TableNextColumn(); }
void cvr_imgui_end_table() { ImGui::EndTable(); }

// The panels' facade (rust/crates/cvr-render/src/ui.rs) while ImGui draws: ImGui's own widgets, text, headers, tabs.
bool cvr_imgui_checkbox(const char* label, bool* value) { return ImGui::Checkbox(label, value); }
// The CET mods' tuning windows (rust/crates/cvr-ffi/src/mechanics/dev_windows.rs).
void cvr_imgui_set_next_window_pos_cond(float x, float y, int cond) { ImGui::SetNextWindowPos(ImVec2(x, y), cond); }
void cvr_imgui_set_next_window_size_cond(float w, float h, int cond) { ImGui::SetNextWindowSize(ImVec2(w, h), cond); }
bool cvr_imgui_tree_node(const char* label) { return ImGui::TreeNode(label); }
void cvr_imgui_tree_pop() { ImGui::TreePop(); }
bool cvr_imgui_drag_float(const char* label, float* v, float speed, float lo, float hi, const char* format) {
    return ImGui::DragFloat(label, v, speed, lo, hi, format);
}
bool cvr_imgui_drag_int(const char* label, int* v, float speed, int lo, int hi) {
    return ImGui::DragInt(label, v, speed, lo, hi);
}
bool cvr_imgui_slider_int_plain(const char* label, int* v, int lo, int hi) { return ImGui::SliderInt(label, v, lo, hi); }
bool cvr_imgui_radio_button(const char* label, bool active) { return ImGui::RadioButton(label, active); }
bool cvr_imgui_small_button(const char* label) { return ImGui::SmallButton(label); }
bool cvr_imgui_input_text(const char* label, char* buffer, size_t size) { return ImGui::InputText(label, buffer, size); }
bool cvr_imgui_slider_float(const char* label, float* value, float minimum, float maximum, const char* format,
                            int flags) {
    return ImGui::SliderFloat(label, value, minimum, maximum, format, flags);
}
bool cvr_imgui_input_int(const char* label, int* value, int step, int fastStep, int flags) {
    return ImGui::InputInt(label, value, step, fastStep, flags);
}
bool cvr_imgui_collapsing_header(const char* label, int flags) { return ImGui::CollapsingHeader(label, flags); }
bool cvr_imgui_begin_tab_bar(const char* id) { return ImGui::BeginTabBar(id); }
void cvr_imgui_end_tab_bar() { ImGui::EndTabBar(); }
bool cvr_imgui_begin_tab_item(const char* label) { return ImGui::BeginTabItem(label); }
void cvr_imgui_end_tab_item() { ImGui::EndTabItem(); }
void cvr_imgui_text(const char* text) { ImGui::Text("%s", text); }
void cvr_imgui_text_unformatted(const char* text) { ImGui::TextUnformatted(text); }
void cvr_imgui_set_tooltip(const char* text) { ImGui::SetTooltip("%s", text); }
bool cvr_imgui_is_any_item_active() { return ImGui::IsAnyItemActive(); }
void cvr_imgui_indent(float width) { ImGui::Indent(width); }
void cvr_imgui_unindent(float width) { ImGui::Unindent(width); }
void cvr_imgui_progress_bar(float fraction, float w, float h, const char* overlay) {
    ImGui::ProgressBar(fraction, ImVec2(w, h), overlay);
}
void cvr_imgui_plot_lines(const char* label, const float* values, int count, int offset, const char* overlay,
                          float minimum, float maximum, float w, float h, int stride) {
    ImGui::PlotLines(label, values, count, offset, overlay, minimum, maximum, ImVec2(w, h), stride);
}

// The overlay (rust/crates/cvr-render/src/imgui_overlay.rs): the context, the Win32 and DX12 backends (part of the
// library), the fonts, the IO events, the frame and its draw data.
void cvr_imgui_check_version() { IMGUI_CHECKVERSION(); }
void cvr_imgui_create_context() { ImGui::CreateContext(); }
void cvr_imgui_destroy_context() { ImGui::DestroyContext(); }
ImGuiIO* cvr_imgui_io() { return &ImGui::GetIO(); }
bool cvr_imgui_add_font_file_cyrillic(const char* path, float size) {
    ImGuiIO& io = ImGui::GetIO();
    return io.Fonts->AddFontFromFileTTF(path, size, nullptr, io.Fonts->GetGlyphRangesCyrillic()) != nullptr;
}
void cvr_imgui_add_font_default(float size) {
    ImFontConfig config;
    config.SizePixels = size;
    ImGui::GetIO().Fonts->AddFontDefault(&config);
}
// The default font's glyph `index`: false past its last.
bool cvr_imgui_font_glyph(int index, unsigned* codepoint, float* u0, float* v0) {
    const ImGuiIO& io = ImGui::GetIO();
    if (!io.Fonts || io.Fonts->Fonts.Size <= 0) return false;
    const ImFont* font = io.Fonts->Fonts[0];
    if (index < 0 || index >= font->Glyphs.Size) return false;
    const ImFontGlyph& glyph = font->Glyphs[index];
    *codepoint = glyph.Codepoint;
    *u0 = glyph.U0;
    *v0 = glyph.V0;
    return true;
}
void cvr_imgui_io_add_mouse_pos_event(float x, float y) { ImGui::GetIO().AddMousePosEvent(x, y); }
void cvr_imgui_io_add_mouse_button_event(int button, bool down) { ImGui::GetIO().AddMouseButtonEvent(button, down); }
void cvr_imgui_new_frame() { ImGui::NewFrame(); }
void cvr_imgui_render() { ImGui::Render(); }
ImDrawData* cvr_imgui_get_draw_data() { return ImGui::GetDrawData(); }
ImDrawList* cvr_imgui_background_draw_list() { return ImGui::GetBackgroundDrawList(); }
bool cvr_imgui_win32_init(HWND hwnd) { return ImGui_ImplWin32_Init(hwnd); }
void cvr_imgui_win32_shutdown() { ImGui_ImplWin32_Shutdown(); }
void cvr_imgui_win32_new_frame() { ImGui_ImplWin32_NewFrame(); }
LRESULT cvr_imgui_win32_wndproc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    return ImGui_ImplWin32_WndProcHandler(hwnd, msg, wParam, lParam);
}
bool cvr_imgui_dx12_init(ID3D12Device* device, int frames, int format, ID3D12DescriptorHeap* heap, uint64_t cpu,
                         uint64_t gpu) {
    return ImGui_ImplDX12_Init(device, frames, static_cast<DXGI_FORMAT>(format), heap,
                               D3D12_CPU_DESCRIPTOR_HANDLE{static_cast<SIZE_T>(cpu)}, D3D12_GPU_DESCRIPTOR_HANDLE{gpu});
}
void cvr_imgui_dx12_shutdown() { ImGui_ImplDX12_Shutdown(); }
void cvr_imgui_dx12_new_frame() { ImGui_ImplDX12_NewFrame(); }
void cvr_imgui_dx12_render(ImDrawData* data, ID3D12GraphicsCommandList* list) { ImGui_ImplDX12_RenderDrawData(data, list); }

// Draw lists.
void cvr_imgui_draw_convex_poly_filled(ImDrawList* list, const ImVec2* points, int count, unsigned color) {
    list->AddConvexPolyFilled(points, count, color);
}
void cvr_imgui_draw_text(ImDrawList* list, ImFont* font, float size, float x, float y, unsigned color,
                         const char* begin, const char* end, float wrapWidth) {
    list->AddText(font, size, ImVec2(x, y), color, begin, end, wrapWidth);
}
void cvr_imgui_draw_push_clip_rect(ImDrawList* list, float x0, float y0, float x1, float y1, bool intersect) {
    list->PushClipRect(ImVec2(x0, y0), ImVec2(x1, y1), intersect);
}
void cvr_imgui_draw_pop_clip_rect(ImDrawList* list) { list->PopClipRect(); }
void cvr_imgui_draw_circle_filled(ImDrawList* list, float x, float y, float radius, unsigned color, int segments) {
    list->AddCircleFilled(ImVec2(x, y), radius, color, segments);
}
void cvr_imgui_draw_circle(ImDrawList* list, float x, float y, float radius, unsigned color, int segments, float thickness) {
    list->AddCircle(ImVec2(x, y), radius, color, segments, thickness);
}
void cvr_imgui_draw_rect(ImDrawList* list, float x0, float y0, float x1, float y1, unsigned color, float rounding, int flags,
                         float thickness) {
    list->AddRect(ImVec2(x0, y0), ImVec2(x1, y1), color, rounding, flags, thickness);
}
void cvr_imgui_draw_rect_filled_rounded(ImDrawList* list, float x0, float y0, float x1, float y1, unsigned color,
                                        float rounding, int flags) {
    list->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1), color, rounding, flags);
}
void cvr_imgui_font_calc_text_size_a(ImFont* font, float size, float maxWidth, float wrapWidth, const char* begin,
                                     const char* end, ImVec2* out) {
    *out = font->CalcTextSizeA(size, maxWidth, wrapWidth, begin, end);
}
ImDrawList* cvr_imgui_window_draw_list() { return ImGui::GetWindowDrawList(); }
ImDrawList* cvr_imgui_foreground_draw_list() { return ImGui::GetForegroundDrawList(); }
void cvr_imgui_draw_line(ImDrawList* list, float x0, float y0, float x1, float y1, unsigned color, float thickness) {
    list->AddLine(ImVec2(x0, y0), ImVec2(x1, y1), color, thickness);
}
void cvr_imgui_draw_rect_filled(ImDrawList* list, float x0, float y0, float x1, float y1, unsigned color) {
    list->AddRectFilled(ImVec2(x0, y0), ImVec2(x1, y1), color);
}
void cvr_imgui_draw_rect_filled_multicolor(ImDrawList* list, float x0, float y0, float x1, float y1,
                                           unsigned upperLeft, unsigned upperRight, unsigned bottomRight,
                                           unsigned bottomLeft) {
    list->AddRectFilledMultiColor(ImVec2(x0, y0), ImVec2(x1, y1), upperLeft, upperRight, bottomRight, bottomLeft);
}
void cvr_imgui_draw_polyline(ImDrawList* list, const ImVec2* points, int count, unsigned color, int flags,
                             float thickness) {
    list->AddPolyline(points, count, color, flags, thickness);
}
void cvr_imgui_draw_triangle(ImDrawList* list, float ax, float ay, float bx, float by, float cx, float cy,
                             unsigned color, float thickness) {
    list->AddTriangle(ImVec2(ax, ay), ImVec2(bx, by), ImVec2(cx, cy), color, thickness);
}
void cvr_imgui_draw_triangle_filled(ImDrawList* list, float ax, float ay, float bx, float by, float cx, float cy,
                                    unsigned color) {
    list->AddTriangleFilled(ImVec2(ax, ay), ImVec2(bx, by), ImVec2(cx, cy), color);
}
}
#endif
