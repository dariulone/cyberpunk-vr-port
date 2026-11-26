#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The build gate, owned by Rust since stage 2 (rust/crates/cvr-engine/src/game_build.rs): the PE header match, the
// CVR_ALLOW_UNKNOWN_BUILD / CVR_PRETEND_UNKNOWN_BUILD overrides and the unmapped list, decided once for the whole
// DLL. The constants in Core/GameBuild.hpp stay the source both languages read (cvr-tool engine-addresses).
#include "Core/GameBuild.hpp"

#include "Rust/CvrRust.h"

#if !CVR_RUST_VRIK
namespace cvr::game_build {
#endif

#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs (cpp_forwards)
bool Supported() { return cvr_game_build_supported(); }
#endif
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
bool Overridden() { return cvr_game_build_overridden(); }
#endif
#if !CVR_RUST_VRIK
bool Unmapped(uintptr_t rva) { return cvr_game_build_unmapped(rva); }
#endif
#if !CVR_RUST_VRIK
const char* Expected() { return cvr_game_build_expected(); }
#endif
#if !CVR_RUST_VRIK
const char* Running() { return cvr_game_build_running(); }
#endif

#if !CVR_RUST_VRIK
}  // namespace cvr::game_build
#endif
#endif
