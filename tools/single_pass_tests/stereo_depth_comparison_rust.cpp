// The stereo depth comparison in its Rust build's form -- the targets, planes, recording, copies and read in
// rust/crates/cvr-render/src/depth_comparison.rs --, the rest of the program as the reference build compiles it. Its
// headers come first, so the .cpp's includes of them (#pragma once) add nothing and its calls reach the program's
// definitions (the binding bank, the geometry commands, the paired geometry pass, the command resources).
#include "Render/StereoDepthComparison.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#define CVR_RUST_VRIK 1
#define CVR_RUST_TEST_FACADES 1   // the facades only the test variants call
#include "../../src/Render/StereoDepthComparison.cpp"
