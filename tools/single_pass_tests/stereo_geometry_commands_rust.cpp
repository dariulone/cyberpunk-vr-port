// The stereo geometry commands in their Rust build's form -- the face over rust/crates/cvr-render/src/geometry_commands.rs
// --, the rest of the program as the reference build compiles it. The command resources and the indirect policy stay the
// program's own: their reference declarations come first, so the .cpp's includes of those headers (#pragma once) add
// nothing and its calls reach the program's definitions (CommandResources.cpp, NativeIndirectPolicy.cpp).
#include "Render/CommandResources.hpp"
#include "Render/NativeIndirectPolicy.hpp"
#define CVR_RUST_VRIK 1
#define CVR_RUST_TEST_FACADES 1   // the facades only the test variants call
#include "../../src/Render/StereoGeometryCommands.cpp"
