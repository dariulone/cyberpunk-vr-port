// What the tests link from the plugin without its C++ half: the AFW / OSR launch (src/Render/Afw.cpp) -- the eyes never
// take turns here -- and the live controls' subsystem parser (rust/crates/cvr-settings calls it; no subsystem takes a
// line here, as in cvr-settings' own unit tests).
namespace cvr::afw {
bool Launch() { return false; }
}  // namespace cvr::afw
extern "C" bool cvr_cpp_settings_parse_subsystem(int, const char*, void*) { return false; }
