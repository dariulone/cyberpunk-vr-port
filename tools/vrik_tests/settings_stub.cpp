// The chain and room programs against the Rust solve link cvr_rust.lib's settings parser (rust/crates/cvr-settings), whose
// subsystem keys go to src/Core/LiveControlsParse.cpp's adapter. Those programs read no ini, so no line is a subsystem's;
// the water program compiles the real adapter instead.
struct LiveControlsSubsystems;
extern "C" bool cvr_cpp_settings_parse_subsystem(int, const char*, LiveControlsSubsystems*) { return false; }
