// rust_stubs.cpp for native_probe_test_rust, whose cvr_rust build (cvr-ffi's probe-test) defines the Rust build's
// exports itself: only the plugin's callback and the export no single-pass test reaches that it does not define.
#include <cstdint>
extern "C" bool cvr_cpp_settings_parse_subsystem(int, const char*, void*) { return false; }
extern "C" int32_t CyberpunkVR_AsymSquarePixels{};
