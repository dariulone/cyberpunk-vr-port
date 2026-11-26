// The native constant capture in its Rust build's form -- the ledger rust/crates/cvr-render/src/constant_ledger.rs and
// the checks constant_capture_checks.rs --, the rest of the program as the reference build compiles it. Its headers come
// first, so the .cpp's includes of them (#pragma once) add nothing and its calls reach the program's definitions
// (StereoDescriptorAudit.cpp, the registered uploads, the binding bank, the geometry pass and the depth comparison).
#include "Render/NativeConstantCapture.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/StereoGeometryCommands.hpp"
#include "Render/StereoGeometryPass.hpp"
#include "Render/StereoTargetArray.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/NativeGeometrySnapshots.hpp"
#include <atomic>
// The exports the Rust build defines in rust/crates/cvr-render/src/native_state_exports.rs (the tests' Rust library
// leaves that module out): the reference build's definitions.
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantCaptureEnable{},CyberpunkVR_ConstantCaptureSeq{},CyberpunkVR_ConstantCaptureFailure{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantDrawCount{},CyberpunkVR_ConstantBindingCount{},CyberpunkVR_ConstantPayloadCount{},CyberpunkVR_ConstantBlobBytes{},CyberpunkVR_ConstantPendingLists{};
__declspec(dllexport) uint64_t CyberpunkVR_ConstantDraws{},CyberpunkVR_ConstantBindings{},CyberpunkVR_ConstantPayloads{},CyberpunkVR_ConstantBlob{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantBankRequest{},CyberpunkVR_ConstantBankDone{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankSequences[2*cvr::stereo::constants::BankPairCapacity]{},CyberpunkVR_ConstantBankDescriptors{},CyberpunkVR_ConstantBankDraws{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantBankPairCount{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantBankPrepareGeometry{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankPreparedDraws{};
__declspec(dllexport) uint64_t CyberpunkVR_ConstantBankInstanceBytes{};
__declspec(dllexport) int32_t CyberpunkVR_ConstantBankGeometryResult{};
__declspec(dllexport) int32_t CyberpunkVR_ConstantBankPassResult{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankPassDraws{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankPassStage{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankPassValidation[3]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantBankCompareDepth{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantCompareState{},CyberpunkVR_ConstantCompareToken{},CyberpunkVR_ConstantCompareSeq{};
__declspec(dllexport) int32_t CyberpunkVR_ConstantCompareResult{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantCompareStage{};
__declspec(dllexport) uint64_t CyberpunkVR_ConstantBankBytes{};
__declspec(dllexport) int32_t CyberpunkVR_ConstantBankResult{};
}
#define CVR_RUST_VRIK 1
#define CVR_RUST_TEST_FACADES 1   // the facades only the test variants call
#include "../../src/Render/NativeConstantCapture.cpp"
