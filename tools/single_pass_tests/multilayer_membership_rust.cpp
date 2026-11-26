// The GPU multilayer membership check in its Rust build's form -- the face over
// rust/crates/cvr-render/src/multilayer_membership.rs --, the rest of the program as the reference build compiles it.
// The command resources stay the program's own: their reference declarations come first, so the .cpp's include of the
// header (#pragma once) adds nothing and its KeepCommandResources call reaches CommandResources.cpp.
#include "Render/CommandResources.hpp"
#define CVR_RUST_VRIK 1
#define CVR_RUST_TEST_FACADES 1   // the facades only the test variants call
#include "../../src/Render/GpuMultilayerMembership.cpp"
