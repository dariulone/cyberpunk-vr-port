// The texture read module in its Rust build's form -- its registry rust/crates/cvr-render/src/texture_receipts.rs, its
// rules rust/crates/cvr-render/src/texture_read.rs --, the rest of the program as the reference build compiles it.
// resource_creation_info_test defines its own KeepCommandResources (it must not be reached); the Rust build's header
// defines it inline, so this translation unit's copy is renamed instead of clashing with the test's.
#define CVR_RUST_VRIK 1
#define CVR_RUST_TEST_FACADES 1   // the facades only the test variants call
#define KeepCommandResources KeepCommandResourcesRustFace
#include "../../src/Render/NativeTextureRead.cpp"
