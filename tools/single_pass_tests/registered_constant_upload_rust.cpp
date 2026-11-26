// The registered constant uploads in their Rust build's form -- the creation receipts
// rust/crates/cvr-render/src/creation_receipts.rs and the copy ledger rust/crates/cvr-render/src/registered_copies.rs --,
// the rest of the program as the reference build compiles it. The command resources stay the program's own: their
// reference declarations come first, so the .cpp's include of the header (#pragma once) adds nothing and its
// KeepCommandResources calls reach the program's definition (CommandResources.cpp, or the test's own).
#include "Render/CommandResources.hpp"
#include <atomic>
#include <cstdint>
// The exports the Rust build defines in rust/crates/cvr-render/src/native_state_exports.rs (the tests' Rust library leaves
// that module out): the reference build's definitions.
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredIntentEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RegisteredIntentCacheStats[10]{};
__declspec(dllexport) uint64_t CyberpunkVR_RegisteredStructuredIntentStats[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyCount{},CyberpunkVR_RegisteredCopyRejected{},CyberpunkVR_RegisteredCopyChanged{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyUnscoped{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyPendingReads{},CyberpunkVR_RegisteredCopyOrderRejected{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_RegisteredCopyReads{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyTimelineResults[8]{},CyberpunkVR_RegisteredCopyTimelineStats[5]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopySignals{},CyberpunkVR_RegisteredCopyWaits{},CyberpunkVR_RegisteredCopyCpuSignals{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyTraceSeq{},CyberpunkVR_RegisteredCopyTraceCount{},CyberpunkVR_RegisteredCopyTraceOverflow{};
__declspec(dllexport) uint64_t CyberpunkVR_RegisteredCopyTraceData{};
}
#define CVR_RUST_VRIK 1
#define CVR_RUST_TEST_FACADES 1   // the facades only the test variants call
#include "../../src/Render/RegisteredConstantUpload.cpp"
