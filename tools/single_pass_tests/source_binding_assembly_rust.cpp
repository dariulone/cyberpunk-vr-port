// The source binding assembly in its Rust build's form -- its root and view rules
// rust/crates/cvr-render/src/assembly_layout.rs --, the rest of the program as the reference build compiles it. The
// command resources and the paired root stay the program's own: their reference declarations come first, so the .cpp's
// includes of those headers (#pragma once) add nothing and its calls reach the program's definitions.
#include "Render/CommandResources.hpp"
#include "Render/StereoPairedRoot.hpp"
#define CVR_RUST_VRIK 1
#define CVR_RUST_TEST_FACADES 1   // the facades only the test variants call
#include "../../src/Render/SourceBindingAssembly.cpp"
