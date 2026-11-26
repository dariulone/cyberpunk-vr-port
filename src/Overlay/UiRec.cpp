// UiRec -- see include/Overlay/UiRec.hpp. The recorder, the record pass, and the UI:: facade.

#include "Overlay/UiRec.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Overlay/OverlayInternal.hpp"

#include <algorithm>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <unordered_map>
#include <vector>

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// THE RECORDER AND THE UI:: FACADE ARE RUST'S (rust/crates/cvr-render/src/ui_rec.rs, ui.rs). These forward for the C++
// half's own callers: the record pass (Overlay/ImGuiOverlay.cpp), the CET natives (Natives/VrMenu.cpp) and the
// full-body tracking section (Fbt/Game/Menu.cpp). Text is formatted here and handed over finished.
extern "C" {
size_t cvr_uirec_latest(int page, char* out, size_t capacity);
}
namespace cvr::uirec {
std::string Latest(int page) {
    std::string text(8192, '\0');
    for (;;) {
        const size_t n = cvr_uirec_latest(page, text.data(), text.size());
        if (n <= text.size()) {
            text.resize(n);
            return text;
        }
        text.assign(n, '\0');
    }
}
}  // namespace cvr::uirec

namespace UI {
}  // namespace UI
#endif
#else
namespace cvr::uirec {

const char* const kPageNames[kPageCount] = {"GENERAL", "FRAMEGEN", "HUD", "CONTROLS", "STEREO", "AVATAR", "OVERLAY"};

namespace {

struct Edit {
    int page = -1;
    std::string id;
    double value = 0.0;
};

struct Item {
    char kind = 'T';
    std::string id, label, text, tooltip;
    double value = 0.0, minimum = 0.0, maximum = 0.0;
    int flags = 0, indent = 0;
};

struct Recorder {
    int page = 0;
    std::vector<Edit> edits;
    std::vector<Item> items;
    std::vector<std::string> ids;
    std::vector<bool> disabled;
    std::unordered_map<std::string, int> seen;
    int indent = 0;

    bool Disabled() const { return std::find(disabled.begin(), disabled.end(), true) != disabled.end(); }

    // The id is the ImGui one -- the PushID scope and the whole label, "##" part included -- so two
    // "Center width" sliders under different scopes stay apart; a repeat in one scope gets "#n".
    std::string MakeId(const char* label) {
        std::string id;
        for (const auto& s : ids) id += s + "/";
        id += label ? label : "";
        const int n = seen[id]++;
        if (n) id += "#" + std::to_string(n);
        return id;
    }

    Item& Add(char kind, const char* label) {
        Item it;
        it.kind = kind;
        it.id = MakeId(label);
        if (label) {
            const char* hash = std::strstr(label, "##");
            it.label = hash ? std::string(label, hash) : std::string(label);
        }
        it.indent = indent;
        if (Disabled()) it.flags |= 16;
        items.push_back(std::move(it));
        return items.back();
    }

    // The edit the VR menu sent for this item, taken (once); nullptr when there is none, and never for a
    // disabled item -- the panel would not have let it change either.
    const Edit* Take(const Item& it) {
        if (it.flags & 16) return nullptr;
        for (auto e = edits.begin(); e != edits.end(); ++e) {
            if (e->id == it.id) {
                taken = *e;
                edits.erase(e);
                return &taken;
            }
        }
        return nullptr;
    }
    Edit taken;
};

Recorder* g_rec = nullptr;

std::mutex g_lock;
int g_requested = -1;
std::vector<Edit> g_edits;
std::string g_latest[kPageCount];
unsigned g_sequence = 0;

std::string Format(const char* format, double value, bool integer) {
    char buffer[128]{};
    if (!format || !*format) format = integer ? "%d" : "%.3f";
    if (integer) std::snprintf(buffer, sizeof(buffer), format, static_cast<int>(value));
    else std::snprintf(buffer, sizeof(buffer), format, value);
    return buffer;
}

void Clean(std::string& s) {
    for (char& c : s) {
        if (c == '\x1e' || c == '\x1f' || c == '\x1d') c = ' ';
    }
}

std::string Serialize(int page, unsigned sequence, std::vector<Item>& items) {
    std::string out;
    char head[64];
    std::snprintf(head, sizeof(head), "#\x1f\x1f%s\x1f%d\x1f%u\x1f\x1f\x1f\x1f\x1f", kPageNames[page], page, sequence);
    out += head;
    for (auto& it : items) {
        Clean(it.label);
        Clean(it.tooltip);
        if (it.kind != 'O' && it.kind != 'P') Clean(it.text);
        char num[96];
        out += '\x1e';
        out += it.kind;
        out += '\x1f';
        out += it.id;
        out += '\x1f';
        out += it.label;
        std::snprintf(num, sizeof(num), "\x1f%.9g\x1f%.9g\x1f%.9g\x1f", it.value, it.minimum, it.maximum);
        out += num;
        out += it.text;
        std::snprintf(num, sizeof(num), "\x1f%d\x1f", it.flags);
        out += num;
        out += it.tooltip;
        std::snprintf(num, sizeof(num), "\x1f%d", it.indent);
        out += num;
    }
    return out;
}

// One run of the page's panel code, recording; true when an edit changed the live controls.
bool Record(int page, std::vector<Edit> edits, LiveControlsUiState& state, std::vector<Item>& out) {
    Recorder rec;
    rec.page = page;
    rec.edits = std::move(edits);
    rec.ids.push_back(std::to_string(page));
    g_rec = &rec;
    bool changed = false;
    if (page >= 0 && page <= 5) changed = overlay::DrawLiveControls(state, page, nullptr);
    else if (page == 6) changed = overlay::DrawOverlayPage(state);
    g_rec = nullptr;
    out = std::move(rec.items);
    return changed;
}

}  // namespace

bool Recording() { return g_rec != nullptr; }

void Request(int page) {
    if (page < 0 || page >= kPageCount) return;
    std::lock_guard lock(g_lock);
    g_requested = page;
}

void QueueEdit(int page, const char* id, double value) {
    if (page < 0 || page >= kPageCount || !id) return;
    std::lock_guard lock(g_lock);
    // A slider dragged from the VR menu sends a stream of values; only the newest one matters.
    for (auto& e : g_edits) {
        if (e.page == page && e.id == id) {
            e.value = value;
            return;
        }
    }
    g_edits.push_back({page, id, value});
}

std::string Latest(int page) {
    if (page < 0 || page >= kPageCount) return {};
    std::lock_guard lock(g_lock);
    return g_latest[page];
}

void RunPendingPass() {
    int page = -1;
    std::vector<Edit> edits;
    {
        std::lock_guard lock(g_lock);
        if (g_requested < 0 && g_edits.empty()) return;
        page = g_edits.empty() ? g_requested : g_edits.front().page;
        for (auto e = g_edits.begin(); e != g_edits.end();) {
            if (e->page == page) {
                edits.push_back(std::move(*e));
                e = g_edits.erase(e);
            } else {
                ++e;
            }
        }
        if (g_requested == page) g_requested = -1;
    }
    LiveControlsUiState state{};
    GetLiveControlsUiState(&state);
    std::vector<Item> items;
    const bool hadEdits = !edits.empty();
    if (Record(page, std::move(edits), state, items)) SetLiveControlsUiState(&state, 1);
    // After an edit the page is read once more, so what it publishes is what the edit made of the
    // controls -- items above the edited one that depend on it included.
    if (hadEdits) {
        LiveControlsUiState fresh{};
        GetLiveControlsUiState(&fresh);
        Record(page, {}, fresh, items);
    }
    std::lock_guard lock(g_lock);
    g_latest[page] = Serialize(page, ++g_sequence, items);
}

bool Checkbox(const char* label, bool* value) {
    auto& it = g_rec->Add('C', label);
    if (const Edit* e = g_rec->Take(it)) {
        const bool next = e->value != 0.0;
        const bool changed = next != *value;
        *value = next;
        it.value = *value ? 1.0 : 0.0;
        return changed;
    }
    it.value = *value ? 1.0 : 0.0;
    return false;
}

bool SliderFloat(const char* label, float* value, float minimum, float maximum, const char* format) {
    auto& it = g_rec->Add('S', label);
    bool changed = false;
    if (const Edit* e = g_rec->Take(it)) {
        const float next = std::clamp(static_cast<float>(e->value), std::min(minimum, maximum), std::max(minimum, maximum));
        changed = next != *value;
        *value = next;
    }
    it.value = *value;
    it.minimum = minimum;
    it.maximum = maximum;
    it.text = Format(format, *value, false);
    return changed;
}

bool SliderInt(const char* label, int* value, int minimum, int maximum, const char* format) {
    auto& it = g_rec->Add('I', label);
    bool changed = false;
    if (const Edit* e = g_rec->Take(it)) {
        const int next = std::clamp(static_cast<int>(std::lround(e->value)), std::min(minimum, maximum), std::max(minimum, maximum));
        changed = next != *value;
        *value = next;
    }
    it.value = *value;
    it.minimum = minimum;
    it.maximum = maximum;
    it.text = Format(format, *value, true);
    return changed;
}

bool Combo(const char* label, int* selected, const char* const* items, int count) {
    auto& it = g_rec->Add('O', label);
    bool changed = false;
    if (const Edit* e = g_rec->Take(it)) {
        const int next = std::clamp(static_cast<int>(std::lround(e->value)), 0, std::max(0, count - 1));
        changed = next != *selected;
        *selected = next;
    }
    it.value = *selected;
    it.minimum = 0;
    it.maximum = count - 1;
    for (int i = 0; i < count; ++i) {
        if (i) it.text += '\x1d';
        std::string option = items[i] ? items[i] : "";
        Clean(option);
        it.text += option;
    }
    return changed;
}

bool Tabs(const char* id, int& selected, const char* const* labels, int count) {
    auto& it = g_rec->Add('P', id);
    bool changed = false;
    if (const Edit* e = g_rec->Take(it)) {
        const int next = std::clamp(static_cast<int>(std::lround(e->value)), 0, std::max(0, count - 1));
        changed = next != selected;
        selected = next;
    }
    it.value = selected;
    it.maximum = count - 1;
    for (int i = 0; i < count; ++i) {
        if (i) it.text += '\x1d';
        it.text += labels[i] ? labels[i] : "";
    }
    return changed;
}

bool Button(const char* label) {
    auto& it = g_rec->Add('B', label);
    return g_rec->Take(it) != nullptr;
}

bool InputInt(const char* label, int* value, int step) {
    auto& it = g_rec->Add('N', label);
    bool changed = false;
    if (const Edit* e = g_rec->Take(it)) {
        const int next = static_cast<int>(std::lround(e->value));
        changed = next != *value;
        *value = next;
    }
    it.value = *value;
    it.minimum = step;
    it.text = std::to_string(*value);
    return changed;
}

bool Header(const char* label, bool defaultOpen) {
    auto& it = g_rec->Add('H', label);
    if (defaultOpen) it.flags |= 8;
    return true;
}

void Text(int flags, unsigned color, const char* text) {
    auto& it = g_rec->Add('T', nullptr);
    it.flags |= flags;
    it.value = color;
    it.text = text ? text : "";
}

void Binding(const char* input, const char* action) {
    auto& it = g_rec->Add('K', input);
    it.label = input ? input : "";
    it.text = action ? action : "";
}

void Progress(float fraction) {
    auto& it = g_rec->Add('G', nullptr);
    it.value = fraction;
}

void Separator() { g_rec->Add('X', nullptr); }

void Tooltip(const char* text) {
    if (!g_rec->items.empty() && text) g_rec->items.back().tooltip = text;
}

void BeginDisabled(bool disabled) { g_rec->disabled.push_back(disabled); }
void EndDisabled() {
    if (!g_rec->disabled.empty()) g_rec->disabled.pop_back();
}
void PushID(const char* id) { g_rec->ids.push_back(id ? id : ""); }
void PushID(int id) { g_rec->ids.push_back(std::to_string(id)); }
void PopID() {
    if (g_rec->ids.size() > 1) g_rec->ids.pop_back();
}
void Indent(int delta) { g_rec->indent = std::max(0, g_rec->indent + delta); }

}  // namespace cvr::uirec

namespace UI {
namespace rec = cvr::uirec;

namespace {
unsigned Pack(const ImVec4& c) {
    auto b = [](float v) { return static_cast<unsigned>(std::clamp(v, 0.0f, 1.0f) * 255.0f + 0.5f); };
    return (b(c.x) << 16) | (b(c.y) << 8) | b(c.z);
}
void RecordText(int flags, unsigned color, const char* format, va_list args) {
    char buffer[4096];
    std::vsnprintf(buffer, sizeof(buffer), format, args);
    rec::Text(flags, color, buffer);
}
}  // namespace

bool Checkbox(const char* label, bool* value) {
    return rec::Recording() ? rec::Checkbox(label, value) : ImGui::Checkbox(label, value);
}
bool Button(const char* label, const ImVec2& size) {
    return rec::Recording() ? rec::Button(label) : ImGui::Button(label, size);
}
bool SliderFloat(const char* label, float* value, float minimum, float maximum, const char* format, ImGuiSliderFlags flags) {
    return rec::Recording() ? rec::SliderFloat(label, value, minimum, maximum, format)
                            : ImGui::SliderFloat(label, value, minimum, maximum, format, flags);
}
bool InputInt(const char* label, int* value, int step, int fastStep, ImGuiInputTextFlags flags) {
    return rec::Recording() ? rec::InputInt(label, value, step) : ImGui::InputInt(label, value, step, fastStep, flags);
}
bool CollapsingHeader(const char* label, ImGuiTreeNodeFlags flags) {
    return rec::Recording() ? rec::Header(label, (flags & ImGuiTreeNodeFlags_DefaultOpen) != 0)
                            : ImGui::CollapsingHeader(label, flags);
}
bool BeginTabBar(const char* id, ImGuiTabBarFlags flags) { return rec::Recording() ? false : ImGui::BeginTabBar(id, flags); }
void EndTabBar() {
    if (!rec::Recording()) ImGui::EndTabBar();
}
bool BeginTabItem(const char* label, bool* open, ImGuiTabItemFlags flags) {
    return rec::Recording() ? false : ImGui::BeginTabItem(label, open, flags);
}
void EndTabItem() {
    if (!rec::Recording()) ImGui::EndTabItem();
}
void Text(const char* format, ...) {
    va_list args;
    va_start(args, format);
    if (rec::Recording()) RecordText(0, 0, format, args);
    else ImGui::TextV(format, args);
    va_end(args);
}
void TextDisabled(const char* format, ...) {
    va_list args;
    va_start(args, format);
    if (rec::Recording()) RecordText(1, 0, format, args);
    else ImGui::TextDisabledV(format, args);
    va_end(args);
}
void TextWrapped(const char* format, ...) {
    va_list args;
    va_start(args, format);
    if (rec::Recording()) RecordText(2, 0, format, args);
    else ImGui::TextWrappedV(format, args);
    va_end(args);
}
void TextColored(const ImVec4& color, const char* format, ...) {
    va_list args;
    va_start(args, format);
    if (rec::Recording()) RecordText(4, Pack(color), format, args);
    else ImGui::TextColoredV(color, format, args);
    va_end(args);
}
void TextUnformatted(const char* text, const char* end) {
    if (rec::Recording()) rec::Text(0, 0, end ? std::string(text, end).c_str() : text);
    else ImGui::TextUnformatted(text, end);
}
void SetTooltip(const char* format, ...) {
    va_list args;
    va_start(args, format);
    if (rec::Recording()) {
        char buffer[4096];
        std::vsnprintf(buffer, sizeof(buffer), format, args);
        rec::Tooltip(buffer);
    } else {
        ImGui::SetTooltipV(format, args);
    }
    va_end(args);
}
// Recording, every item counts as pointed at -- that is what makes each SetTooltip in the panels land
// on its item -- and none as being dragged.
bool IsItemHovered(ImGuiHoveredFlags flags) { return rec::Recording() ? true : ImGui::IsItemHovered(flags); }
bool IsItemActive() { return rec::Recording() ? false : ImGui::IsItemActive(); }
bool IsAnyItemActive() { return rec::Recording() ? false : ImGui::IsAnyItemActive(); }
void BeginDisabled(bool disabled) {
    if (rec::Recording()) rec::BeginDisabled(disabled);
    else ImGui::BeginDisabled(disabled);
}
void EndDisabled() {
    if (rec::Recording()) rec::EndDisabled();
    else ImGui::EndDisabled();
}
void PushID(const char* id) {
    if (rec::Recording()) rec::PushID(id);
    else ImGui::PushID(id);
}
void PushID(int id) {
    if (rec::Recording()) rec::PushID(id);
    else ImGui::PushID(id);
}
void PopID() {
    if (rec::Recording()) rec::PopID();
    else ImGui::PopID();
}
void Separator() {
    if (rec::Recording()) rec::Separator();
    else ImGui::Separator();
}
void SameLine(float offset, float spacing) {
    if (!rec::Recording()) ImGui::SameLine(offset, spacing);
}
void Spacing() {
    if (!rec::Recording()) ImGui::Spacing();
}
void Indent(float width) {
    if (rec::Recording()) rec::Indent(1);
    else ImGui::Indent(width);
}
void Unindent(float width) {
    if (rec::Recording()) rec::Indent(-1);
    else ImGui::Unindent(width);
}
void SetNextItemWidth(float width) {
    if (!rec::Recording()) ImGui::SetNextItemWidth(width);
}
void ProgressBar(float fraction, const ImVec2& size, const char* overlay) {
    if (rec::Recording()) rec::Progress(fraction);
    else ImGui::ProgressBar(fraction, size, overlay);
}
void PlotLines(const char* label, const float* values, int count, int offset, const char* overlay, float minimum,
               float maximum, ImVec2 size, int stride) {
    if (!rec::Recording()) ImGui::PlotLines(label, values, count, offset, overlay, minimum, maximum, size, stride);
}
ImGuiIO& GetIO() { return ImGui::GetIO(); }

}  // namespace UI
#endif
