// The upload registry in its Rust build's form -- the face over rust/crates/cvr-render/src/upload_registry.rs --, the
// rest of constant_capture_test's program as the reference build compiles it: the program then checks exactly the
// ported registry.
#include <atomic>
#include <cstdint>
// The exports the Rust build defines in rust/crates/cvr-render/src/constant_registry_exports.rs (the tests' Rust library leaves
// that module out): the reference build's definitions.
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadCount{},CyberpunkVR_ConstantUploadFailures{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ConstantUploadBytes{},CyberpunkVR_ConstantUploadReads{},CyberpunkVR_ConstantUploadMisses{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadEvictions{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadTypes[6]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadNativeImports{},CyberpunkVR_ConstantUploadNativeRejects{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantRegisteredTypes[6]{},CyberpunkVR_ConstantRegisteredRejects{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadApiHits{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadFastPath{1};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ConstantUploadFastStats[6]{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantUploadFastStatsCount=6;
}
#define CVR_RUST_VRIK 1
#define CVR_RUST_TEST_FACADES 1   // the facades only the test variants call
#include "../../src/Render/ConstantUploadRegistry.cpp"
