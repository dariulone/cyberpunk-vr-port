#include "Render/NativeRetainedGeometry.hpp"
#include "Render/NativePreparedPair.hpp"
#include "Render/NativeDescriptorArena.hpp"
#include "Render/NativePairSubmission.hpp"
#include "Render/NativeMaterialBufferSnapshot.hpp"
#include "Render/NativeCopyWindows.hpp"
#include "Render/StereoVrsSnapshot.hpp"
#include "Render/GeometryTargetCheckpoint.hpp"
#include "Render/StereoGeometryChain.hpp"
#include "Render/StereoGeometryCommands.hpp"
#include "Render/NativeCommandRecording.hpp"
#include "Render/NativeQueueResidencyScope.hpp"
#include "Render/NativeSubmissionFrontier.hpp"
#include "Render/NativeTargetHandoff.hpp"
#include "Render/NativeTargetAccess.hpp"
#include "Render/NativeSceneAccess.hpp"
#include "Render/NativeTargetLifetime.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/NativeIndirectPolicy.hpp"
#include "Render/NativeIaBinding.hpp"
#include "Render/NativeIndirectCopy.hpp"
#include "Render/NativeShaderSnapshot.hpp"
#include "Render/NativeSourceCbvBinding.hpp"
#include "Render/NativeSourceCbvDetails.hpp"
#include "Render/NativeSourceCbvSnapshot.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/SourceTablePlan.hpp"
#include "Render/SourceSelectorPlan.hpp"
#include "Render/SourceBindingAssembly.hpp"
#include "Render/SourceResourceIntent.hpp"
#include "Render/NativeSourceSrvBinding.hpp"
#include "Render/MultilayerSelectorPlan.hpp"
#include "Render/NativeStructuredBufferSnapshot.hpp"
#include "Render/StructuredSnapshotView.hpp"
#include "Render/NativeIaSnapshot.hpp"
#include "Render/NativeInstanceObserver.hpp"
#include "Render/InstanceSnapshotView.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <algorithm>
#include <mutex>
#include <memory>
#include <map>
#include <unordered_set>
#include <unordered_map>
#include <stdexcept>
#include <bit>
#include <cmath>
#include <cstdio>
#include <tuple>
#include <set>
#if CVR_RUST_VRIK
// The capture is Rust (rust/crates/cvr-render/src/retained_geometry_native.rs: the lock, the work's start, Fail,
// the observers, the captures, seal, poll, draws, boundary and submissions; retained_work.rs: the work, its eyes and
// sources, AppendSource); this half keeps the selector catalog, loaded through jsoncpp.
extern "C" {
void cvr_nrg2_attach(const void*);
void cvr_nrg2_frame_boundary();
bool cvr_nrg2_test(uint32_t);
}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's host is rust/crates/cvr-render/src/retained_geometry_native.rs'
namespace cvr::stereo::source_selectors {void* LoadCatalogHandle(const std::filesystem::path&)noexcept;}   // SourceSelectorPlan.cpp
namespace cvr::stereo::retained_geometry {
namespace {
// The selector catalog: loaded once through jsoncpp, its rules' handle kept for the process (retained_sources.rs reads it).
void* SelectorCatalog(){
    static void* const handle=[]()->void*{
        wchar_t path[32768]{};const auto length=GetEnvironmentVariableW(L"CVR_STEREO_SOURCE_SELECTORS",path,32768);
        if(!length || length>=32768)return nullptr;
        return source_selectors::LoadCatalogHandle(path);
    }();
    return handle;
}
struct NrgHost {void*(*selectorCatalog)();};
const NrgHost kNrgHost{&SelectorCatalog};
[[maybe_unused]] const bool kNrgAttached=(cvr_nrg2_attach(&kNrgHost),true);
}
#else
namespace cvr::stereo::retained_geometry {
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void FrameBoundary(){cvr_nrg2_frame_boundary();}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool TestInfixCaptureLifetime(){return cvr_nrg2_test(0);}
bool TestFrontierFailureDisarm(){return cvr_nrg2_test(1);}
bool TestFrontierObserverModes(){return cvr_nrg2_test(2);}
#endif
}
#else

namespace cvr::detail {
using PrivateExecuteFn=void(STDMETHODCALLTYPE*)(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
PrivateExecuteFn command_queue_original_execute(ID3D12CommandQueue*);
}

extern "C" {
extern std::atomic<uint32_t> CyberpunkVR_OwnedSceneSplitEnable;
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedGeometryRequest{},CyberpunkVR_RetainedGeometryState{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedGeometryStage{7},CyberpunkVR_RetainedGeometryVerified{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedCurrentEnable{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedInfixEnable{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedTailEnable{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedVrsRefinementEnable{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedFullT5Enable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedFullT5Stats[16]{},CyberpunkVR_RetainedFullT5Rows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedFullT5Count{},CyberpunkVR_RetainedFullT5RowBytes=16*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedBorrowedTablesEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedBorrowedTablesStats[16]{},CyberpunkVR_RetainedBorrowedTablesRows{},CyberpunkVR_RetainedBorrowedRangeRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedBorrowedTablesCount{},CyberpunkVR_RetainedBorrowedTablesRowBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint32_t CyberpunkVR_RetainedBorrowedRangeCount{},CyberpunkVR_RetainedBorrowedRangeRowBytes=12*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedBorrowedInventoryRows{},CyberpunkVR_RetainedBorrowedFailure[8]{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedBorrowedInventoryCount{},CyberpunkVR_RetainedBorrowedInventoryRowBytes=24*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedPageableUseStats[16]{},CyberpunkVR_RetainedPageableUseRows{},CyberpunkVR_RetainedPageableUseFailure[24]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedPageableSearch[20]{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedPageableUseCount{},CyberpunkVR_RetainedPageableUseRowBytes=16*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedFixedAssetsEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedFixedAssetsStats[16]{},CyberpunkVR_RetainedFixedAssetsCounts[10]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedFixedAssetsRows{},CyberpunkVR_RetainedFixedAssetRows{},CyberpunkVR_RetainedFixedSlotRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedFixedAssetsCount{},CyberpunkVR_RetainedFixedAssetsRowBytes=24*sizeof(uint64_t);
__declspec(dllexport) uint32_t CyberpunkVR_RetainedFixedAssetCount{},CyberpunkVR_RetainedFixedAssetRowBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint32_t CyberpunkVR_RetainedFixedSlotCount{},CyberpunkVR_RetainedFixedSlotRowBytes=8*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedFixedDescriptorRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedFixedDescriptorCount{},CyberpunkVR_RetainedFixedDescriptorRowBytes=24*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedTail[20]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedTargetLifetime[128]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedInfix[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedCurrent[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedBaseline[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGeometryStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGeometryDifferent[12]{},CyberpunkVR_RetainedGeometryBytes[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGeometryEnds[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGeometryIndirect[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGeometryIndirectRejected[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGeometryShaderRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedGeometryShaderCount{},CyberpunkVR_RetainedGeometryShaderRowBytes=11*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedSourceStreamEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceStreamStats[16]{},CyberpunkVR_RetainedSourceStreamRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceStreamCount{},CyberpunkVR_RetainedSourceStreamRowBytes=48*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedIndirectBindingsStats[16]{},CyberpunkVR_RetainedIndirectBindingsRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedIndirectBindingsCount{},CyberpunkVR_RetainedIndirectBindingsRowBytes=56*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedIndirectSnapshotsEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedIndirectSnapshotsStats[16]{},CyberpunkVR_RetainedIndirectSnapshotsRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedIndirectSnapshotsCount{},CyberpunkVR_RetainedIndirectSnapshotsRowBytes=24*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedSourceCbvEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceCbvStats[16]{},CyberpunkVR_RetainedSourceCbvRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceCbvCount{},CyberpunkVR_RetainedSourceCbvRowBytes=24*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceCbvProducerRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceCbvProducerCount{},CyberpunkVR_RetainedSourceCbvProducerRowBytes=40*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSelectorCoverageRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSelectorCoverageCount{},CyberpunkVR_RetainedSelectorCoverageRowBytes=28*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSelectorCacheEventRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSelectorCacheEventCount{},CyberpunkVR_RetainedSelectorCacheEventRowBytes=36*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceCbvDiagnostics{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceCbvDiagnosticBytes=16*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedSourceTablesEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceTablesStats[16]{},CyberpunkVR_RetainedSourceTablesRows{},CyberpunkVR_RetainedSourceTablesCommands{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceTablesCount{},CyberpunkVR_RetainedSourceTablesCommandCount{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceTablesRowBytes=16*sizeof(uint64_t),CyberpunkVR_RetainedSourceTablesCommandBytes=24*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedSourceDetailsEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceDetailsStats[16]{},CyberpunkVR_RetainedSourceDetailsRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceDetailsCount{},CyberpunkVR_RetainedSourceDetailsRowBytes=128*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedSourceCbvSnapshotsEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceCbvSnapshotsStats[16]{},CyberpunkVR_RetainedSourceCbvSnapshotsRows{},CyberpunkVR_RetainedSourceCbvSnapshotsCommands{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceCbvSnapshotsCount{},CyberpunkVR_RetainedSourceCbvSnapshotsCommandCount{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceCbvSnapshotsRowBytes=32*sizeof(uint64_t),CyberpunkVR_RetainedSourceCbvSnapshotsCommandBytes=24*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceCbvSnapshotsBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceCbvSnapshotsBlobBytes{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceCbvCopyOwnerRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceCbvCopyOwnerCount{},CyberpunkVR_RetainedSourceCbvCopyOwnerRowBytes=8*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceSelectorsStats[16]{},CyberpunkVR_RetainedSourceSelectorsCommands{},CyberpunkVR_RetainedSourceSelectorsInputs{},CyberpunkVR_RetainedSourceSelectorsSlots{},CyberpunkVR_RetainedSourceSelectorsT5{},CyberpunkVR_RetainedSourceSelectorsBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceSelectorsCommandCount{},CyberpunkVR_RetainedSourceSelectorsInputCount{},CyberpunkVR_RetainedSourceSelectorsSlotCount{},CyberpunkVR_RetainedSourceSelectorsT5Count{},CyberpunkVR_RetainedSourceSelectorsBlobBytes{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceSelectorsCommandBytes=32*sizeof(uint64_t),CyberpunkVR_RetainedSourceSelectorsInputBytes=24*sizeof(uint64_t),CyberpunkVR_RetainedSourceSelectorsSlotBytes=16*sizeof(uint64_t),CyberpunkVR_RetainedSourceSelectorsT5Bytes=32*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedT5NativeRows{},CyberpunkVR_RetainedT5SnapshotRows{},CyberpunkVR_RetainedT5SnapshotBlob{},CyberpunkVR_RetainedT5IntentBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedT5NativeCount{},CyberpunkVR_RetainedT5SnapshotCount{},CyberpunkVR_RetainedT5SnapshotBlobBytes{},CyberpunkVR_RetainedT5IntentBlobBytes{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedT5NativeRowBytes=96*sizeof(uint64_t),CyberpunkVR_RetainedT5SnapshotRowBytes=32*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedT5CopyDiagnosticRows{},CyberpunkVR_RetainedT5MarkDiagnosticRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedT5DiagnosticCount{},CyberpunkVR_RetainedT5DiagnosticRowBytes=32*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedT5PieceRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedT5PieceCount{},CyberpunkVR_RetainedT5PieceRowBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedT5CoverageRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedT5CoverageCount{},CyberpunkVR_RetainedT5CoverageRowBytes=20*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceAssemblyStats[16]{},CyberpunkVR_RetainedSourceAssemblyCommands{},CyberpunkVR_RetainedSourceAssemblyTables{},CyberpunkVR_RetainedSourceAssemblyMissingCbvs{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceAssemblyCacheStats[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceAssemblyBankStats[6]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceAssemblyBankFailure[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedT5OverlayStats[16]{},CyberpunkVR_RetainedT5OverlayRows{},CyberpunkVR_RetainedT5OverlayBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedT5OverlayCount{},CyberpunkVR_RetainedT5OverlayRowBytes=32*sizeof(uint64_t),CyberpunkVR_RetainedT5OverlayBlobBytes{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedIaCommands{},CyberpunkVR_RetainedIaObservations{},CyberpunkVR_RetainedIaViews{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedIaCommandCount{},CyberpunkVR_RetainedIaObservationCount{},CyberpunkVR_RetainedIaViewCount{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedIaCommandBytes=16*sizeof(uint64_t),CyberpunkVR_RetainedIaObservationBytes=67*sizeof(uint64_t),CyberpunkVR_RetainedIaViewBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedInstanceRows{},CyberpunkVR_RetainedInstanceBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedInstanceCount{},CyberpunkVR_RetainedInstanceRowBytes=48*sizeof(uint64_t),CyberpunkVR_RetainedInstanceBlobBytes{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedInstanceGpuRows{},CyberpunkVR_RetainedInstanceGpuBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedInstanceGpuCount{},CyberpunkVR_RetainedInstanceGpuRowBytes=24*sizeof(uint64_t),CyberpunkVR_RetainedInstanceGpuBlobBytes{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RetainedInstanceGpuPhase{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedInstanceOverlayStats[8]{},CyberpunkVR_RetainedInstanceOverlayRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedInstanceOverlayCount{},CyberpunkVR_RetainedInstanceOverlayRowBytes=24*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedInstanceOverlayBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedInstanceOverlayBlobBytes{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedInstanceOverlayMode=2;
__declspec(dllexport) char CyberpunkVR_RetainedInstanceOverlayError[256]{};
__declspec(dllexport) char CyberpunkVR_RetainedT5OverlayError[256]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGuardedStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGuardedCacheStats[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedGuardedRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedGuardedCount{},CyberpunkVR_RetainedGuardedRowBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedStagedStats[12]{},CyberpunkVR_RetainedStagedRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedStagedCount{},CyberpunkVR_RetainedStagedRowBytes=8*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedStagedResidency[12]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedStagedResidencyDetails[12]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedStagedMembershipDetails[12]{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedIaSelections{},CyberpunkVR_RetainedPrivateGeometryRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedIaSelectionCount{},CyberpunkVR_RetainedIaSelectionBytes=5*sizeof(uint64_t);
__declspec(dllexport) uint32_t CyberpunkVR_RetainedPrivateGeometryCount{},CyberpunkVR_RetainedPrivateGeometryRowBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceAssemblyCommandCount{},CyberpunkVR_RetainedSourceAssemblyTableCount{},CyberpunkVR_RetainedSourceAssemblyMissingCbvCount{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceAssemblyCommandBytes=32*sizeof(uint64_t),CyberpunkVR_RetainedSourceAssemblyTableBytes=16*sizeof(uint64_t),CyberpunkVR_RetainedSourceAssemblyMissingCbvBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedMaterialRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedMaterialCount{},CyberpunkVR_RetainedMaterialRowBytes=24*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedMaterialSources{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedMaterialSourceCount{},CyberpunkVR_RetainedMaterialSourceBytes=5*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedMaterialBufferRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedMaterialBufferCount{},CyberpunkVR_RetainedMaterialBufferRowBytes=40*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedMaterialBufferCopyRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedMaterialBufferCopyCount{},CyberpunkVR_RetainedMaterialBufferCopyRowBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedIaReuseRows{},CyberpunkVR_RetainedIaTransferRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedIaReuseCount{},CyberpunkVR_RetainedIaTransferCount{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedNativeTableRows{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedNativeTableCount{},CyberpunkVR_RetainedNativeTableRowBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedNativeTableEntries{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedNativeTableEntryCount{},CyberpunkVR_RetainedNativeTableEntryBytes=8*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedNativeTableRings{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedNativeTableRingCount{},CyberpunkVR_RetainedNativeTableRingBytes=16*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceBoundaryRows[40]{};
__declspec(dllexport) uint32_t CyberpunkVR_RetainedSourceBoundaryCount{};
__declspec(dllexport) uint64_t CyberpunkVR_RetainedSourceQueueOrder[16]{};
}
namespace cvr::stereo::retained_geometry {
using Microsoft::WRL::ComPtr;
namespace {
std::mutex mutex;
std::atomic<bool> watching{};
struct Plane {
    std::array<D3D12_PLACED_SUBRESOURCE_FOOTPRINT,2> layouts;
    UINT rows{},stride{},component{},bytes{},slot{},plane{};
};
struct Eye {
    native_descriptors::Cache descriptorCache;
    native_recording::Lease recording;
    scene_state::GeometryTargets targets;
    std::array<bool,5> imported{};
    ComPtr<ID3D12Resource> readback;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12CommandQueue> sourceQueue;
    uint64_t signaledValue{};bool submissionCertified{};
    std::vector<Plane> planes;
    uint64_t scope{};UINT colors{};
    bool started{},sealed{},pending{},submitted{};
    std::unique_ptr<source_cbv_snapshot::SnapshotArena> cbvArena;
    ComPtr<ID3D12Resource> cbvReadback;
    bool cbvArenaAborted{};
    std::unique_ptr<structured_buffer_snapshot::SnapshotArena> t5Arena;
    ComPtr<ID3D12Resource> t5Readback;
    bool t5Aborted{};
    std::unique_ptr<structured_buffer_snapshot::SnapshotArena> fullT5Arena;
    ComPtr<ID3D12Resource> fullT5OriginalReadback,fullT5PrivateReadback;
    uint64_t fullT5ReferenceBytes{};
    std::unique_ptr<ia_snapshot::SnapshotArena> iaArena;
    bool iaAborted{},iaValidated{};
    ComPtr<ID3D12Resource> instanceReadback,instancePredicate,instancePrivateReadback;
    std::unique_ptr<instance_snapshot_view::GpuCaptureArena> instanceCapture;
    uint64_t instanceReadbackBytes{};
    std::shared_ptr<GeometryTargetCheckpoint> passDepth;
    std::shared_ptr<GeometryTargetCheckpoint> tailBaseline;
    std::array<target_lifetime::Lease,4> targetLifetimes;
    std::array<target_lifetime::BorrowedScope,4> borrowedTargets;
    uint64_t tailFirst=UINT64_MAX;UINT sealReason=UINT_MAX;
    std::unique_ptr<material_buffer_snapshot::Arena> materialArena;
};
struct Work {
    struct InputRow {char name[64]{};uint32_t index{},format{},slot{},offset{},classification{},step{};};
    static_assert(sizeof(InputRow)==88);
    struct ShaderExport {std::shared_ptr<const shader_snapshot::Snapshot> source;std::vector<InputRow> inputs;};
    StereoGeometryChain chain;
    ComPtr<ID3D12Device> device;
    std::array<Eye,2> eyes;
    std::vector<ComPtr<ID3D12PipelineState>> pipelines;
    std::vector<ShaderExport> shaders;
    std::vector<std::array<uint64_t,11>> shaderRows;
    struct Source {
        GeometrySourceCommand command;
        scene_state::Bindings bindings;
        preparation::GraphParentRecord parent;
        std::shared_ptr<const shader_snapshot::Snapshot> shader;
        ComPtr<ID3D12RootSignature> root,computeRoot;
        std::array<ComPtr<ID3D12DescriptorHeap>,2> heaps;
        geometry::IndirectBinding indirectBinding;
        IndirectCopyProof indirectProof;
        CapturedIndirectSource indirectSnapshot;
        std::vector<source_cbv::SourceCbvBacking> cbvs;
        std::vector<texture_read::ImmutableLease> materialOwners;
        std::vector<descriptors::SourceResourceBinding> finiteDependencies,sparseDependencies;
        std::vector<size_t> materialRows;
        struct MaterialBuffer {source_srv::SourceSrvBacking backing;material_buffer_snapshot::Copy copy;size_t row{};};
        std::vector<MaterialBuffer> materialBuffers;
        bool materialInputsReady{},materialTruncated{};
        std::shared_ptr<StereoVrsSnapshot> vrs;
        std::shared_ptr<GeometryTargetCheckpoint> passDepth;
        std::vector<descriptors::OwnedSourceTable> tables;
        native_descriptors::Tables nativeTables;
        descriptors::BorrowedSourceRanges borrowedRanges;
        source_cbv_snapshot::PreparedCopies cbvPrepared;
        std::vector<source_cbv_snapshot::PrivateCbv> privateCbvs;
        struct SelectorInput {source_selectors::CbvKey key;source_resource_intent::Candidate intent;size_t row{};};
        std::vector<SelectorInput> selectorInputs;
        source_selectors::Result selectorResult;
        std::vector<descriptors::OwnedSourceSlots> selectorSlots;
        std::vector<SharedBankTable> bindingTables;
        descriptors::SrvBinding multilayerT5;
        constants::ResourceCreationInfo multilayerCreation;
        source_srv::SourceSrvBacking t5Native;
        multilayer_selectors::ReadPlan t5Plan;
        source_resource_intent::Candidate t5Candidate;
        structured_buffer_snapshot::PreparedCopy t5Prepared;
        structured_buffer_snapshot::PrivateRange t5Private;
        structured_buffer_snapshot::PreparedCopy fullT5Prepared;
        structured_buffer_snapshot::PrivateRange fullT5Private;
        size_t t5Row=SIZE_MAX;
        bool t5OriginalCertified{};
        std::optional<structured_snapshot_view::PrivateView> t5View;
        ia_snapshot::PreparedCopies iaPrepared;
        std::vector<ia_snapshot::PrivateIa> iaViews;
        instance_observer::SourceCandidate instanceCandidate;
        std::optional<instance_snapshot_view::PrivateView> instanceView;
        std::optional<geometry::DrawResources> privateGeometry;
        source_binding_assembly::Plan bindingPlan;
        source_binding_assembly::GuardedPlan guardedPlan;
        source_binding_assembly::GuardedTables guardedBindings;
        std::optional<prepared_pair::Source> stagedPair;
    };
    std::map<ID3D12Resource*,texture_read::ImmutableLease> materialCache;
    std::vector<std::array<uint64_t,16>> guardedRows;
    bool guardedAttempted{};
    bool currentEnabled{},frontierObserver{},sceneAccessObserver{},currentStarted{},currentSubmitted{},infixEnabled{},infixInFlight{},tailEnabled{},allowVrsRefinement{};
    bool fullT5Enabled{};
    std::vector<std::array<uint64_t,16>> fullT5Rows;
    bool borrowedTablesEnabled{};
    queue_residency::Scope borrowedTableScope;
    std::vector<std::array<uint64_t,16>> borrowedTableRows;
    std::vector<std::array<uint64_t,12>> borrowedRangeRows;
    std::vector<std::array<uint64_t,24>> borrowedInventoryRows;
    std::map<std::tuple<uint64_t,uint64_t,uint64_t,uint64_t>,size_t> borrowedInventories;
    std::vector<queue_residency::PendingPageableUse> borrowedPageables;
    std::vector<std::array<uint64_t,16>> borrowedPageableRows;
    bool fixedAssetsEnabled{};
    std::vector<descriptors::BorrowedFixedRange> fixedAssets;
    std::vector<std::array<uint64_t,24>> fixedRangeRows;
    std::vector<std::array<uint64_t,16>> fixedAssetRows;
    std::vector<std::array<uint64_t,8>> fixedSlotRows;
    std::vector<std::array<uint64_t,24>> fixedDescriptorRows;
    std::vector<std::array<uint64_t,8>> stagedRows;
    std::vector<Source> sources;
    std::vector<std::array<uint64_t,16>> nativeTableRows;
    std::vector<std::array<uint64_t,8>> nativeTableEntries;
    std::vector<std::array<uint64_t,16>> nativeTableRings;
    std::vector<std::array<uint64_t,24>> materialRows;
    std::vector<std::array<uint64_t,40>> materialBufferRows;
    std::vector<std::array<uint64_t,16>> materialBufferCopies;
    std::vector<std::array<uint64_t,5>> materialSourceRows;
    std::vector<std::array<uint64_t,48>> instanceRows;
    std::vector<uint8_t> instanceBlob;
    std::vector<std::array<uint64_t,24>> instanceGpuRows;
    std::vector<uint8_t> instanceGpuBlob;
    std::vector<std::array<uint64_t,24>> instanceOverlayRows;
    std::vector<std::array<uint64_t,5>> iaSelections;
    std::vector<std::array<uint64_t,16>> privateGeometryRows;
    std::vector<uint8_t> instanceOverlayBlob;
    UINT instanceOverlayStage{};
    std::vector<std::array<uint64_t,48>> sourceRows;
    std::vector<std::array<uint64_t,56>> indirectRows;
    std::vector<std::array<uint64_t,24>> snapshotRows;
    std::vector<std::array<uint64_t,24>> cbvRows;
    std::vector<std::array<uint64_t,40>> cbvProducers;
    std::vector<std::array<uint64_t,16>> cbvDiagnostics;
    std::vector<std::array<uint64_t,128>> detailRows;
    std::vector<std::array<uint64_t,16>> tableRows;
    std::vector<std::array<uint64_t,24>> tableCommands;
    std::vector<std::array<uint64_t,32>> cbvSnapshotRows;
    std::vector<std::array<uint64_t,24>> cbvSnapshotCommands;
    std::vector<std::array<uint64_t,8>> cbvCopyOwners;
    std::unordered_map<ID3D12PipelineState*,source_tables::Plan> tablePlans;
    descriptors::SourceTableCache tableCache;
    StereoGeometryCommands sourceGeometry;
    bool sourceStream{},sourceFailed{},sourceCompiled{};
    bool sourceSnapshots{},snapshotFailed{};
    bool sourceCbvs{},cbvFailed{};
    bool sourceTables{},tablesFailed{};
    bool sourceDetails{},detailsFailed{};
    bool sourceCbvSnapshots{},cbvSnapshotsFailed{};
    bool sourceIaSnapshots{};
    std::vector<std::array<uint64_t,16>> iaCommands,iaViews;
    std::vector<std::array<uint64_t,8>> iaReuse;
    std::vector<std::array<uint64_t,5>> iaTransfers;
    std::vector<std::array<uint64_t,67>> iaObservations;
    bool cbvPayloadComplete{};
    std::vector<uint8_t> cbvBlob;
    bool sourceSelectors{};
    std::shared_ptr<const source_selectors::Catalog> selectorCatalog;
    std::vector<std::array<uint64_t,32>> selectorCommands,t5Rows;
    std::vector<std::array<uint64_t,24>> selectorInputs;
    std::vector<std::array<uint64_t,28>> selectorCoverage;
    std::vector<std::array<uint64_t,36>> selectorCacheEvents;
    std::vector<std::array<uint64_t,16>> selectorSlots;
    std::vector<uint8_t> selectorBlob;
    std::vector<std::array<uint64_t,96>> t5NativeRows;
    std::vector<std::array<uint64_t,32>> t5SnapshotRows;
    std::vector<uint8_t> t5Blob,t5IntentBlob;
    std::vector<std::array<uint64_t,32>> t5CopyDiagnostics,t5MarkDiagnostics;
    std::vector<std::array<uint64_t,16>> t5Pieces;
    std::vector<std::array<uint64_t,20>> t5Coverage;
    std::vector<std::array<uint64_t,32>> assemblyCommands;
    std::vector<std::array<uint64_t,16>> assemblyTables,assemblyMissingCbvs;
    uint64_t assemblyDescriptors{};
    bool assemblyComplete{},assemblyTruncated{};
    source_binding_assembly::CpuTableCache assemblyCache;
    std::unique_ptr<StereoBindingBank> assemblyBank;
    bool assemblyBankAttempted{};HRESULT assemblyBankResult{};
    std::array<uint64_t,4> assemblyBankFailure{};
    bool payloadsProcessed{};
    UINT overlayStage{};
    std::unique_ptr<structured_snapshot_view::Overlay> t5Overlay;
    std::vector<std::array<uint64_t,32>> overlayRows;
    std::vector<uint8_t> overlayBlob;
    UINT frame{},mode{},stage{};
    bool created{},failed{},complete{};
};
void LatchFrontierObserver(Work& capture){
    capture.currentEnabled=CyberpunkVR_PreparedCurrentEnable.load()!=0;
    const auto sceneMode=CyberpunkVR_OwnedSceneSplitEnable.load();
    capture.frontierObserver=capture.currentEnabled || sceneMode==1 || sceneMode==2;
}
void ArmFrontierObserver(const Work& capture){
    if(!capture.frontierObserver)return;
    CyberpunkVR_SubmissionFrontierEnable=0;
    if(submission_frontier::Install() && submission_frontier::Reset())CyberpunkVR_SubmissionFrontierEnable=1;
}
void DisarmFinishedFrontierObserver(const Work& capture){
    if(capture.frontierObserver && (capture.complete || capture.failed))CyberpunkVR_SubmissionFrontierEnable=0;
    if(capture.sceneAccessObserver && (capture.complete || capture.failed || !CyberpunkVR_SceneAccessEnable.load())){
        scene_access_trace::Finish(capture.failed || !capture.complete);CyberpunkVR_SceneChildEnable=0;
    }
}
std::unique_ptr<Work> work;
std::vector<ComPtr<ID3D12PipelineState>> verified;
std::unordered_set<ID3D12GraphicsCommandList*> renderPasses;
void Fail(UINT reason);
void ReleaseBorrowedObservers(){
    if(!work)return;
    for(auto& source:work->sources)source.borrowedRanges={};
    work->fixedAssets.clear();work->borrowedPageables.clear();work->borrowedTableScope={};
}
void PublishSources(){
    if(!work || !work->sourceStream)return;
    CyberpunkVR_RetainedFullT5Rows=reinterpret_cast<uintptr_t>(work->fullT5Rows.data());
    CyberpunkVR_RetainedFullT5Count=UINT(work->fullT5Rows.size());
    CyberpunkVR_RetainedBorrowedTablesRows=reinterpret_cast<uintptr_t>(work->borrowedTableRows.data());
    CyberpunkVR_RetainedBorrowedTablesCount=UINT(work->borrowedTableRows.size());
    CyberpunkVR_RetainedBorrowedRangeRows=reinterpret_cast<uintptr_t>(work->borrowedRangeRows.data());
    CyberpunkVR_RetainedBorrowedRangeCount=UINT(work->borrowedRangeRows.size());
    CyberpunkVR_RetainedBorrowedInventoryRows=reinterpret_cast<uintptr_t>(work->borrowedInventoryRows.data());
    CyberpunkVR_RetainedBorrowedInventoryCount=UINT(work->borrowedInventoryRows.size());
    CyberpunkVR_RetainedPageableUseRows=reinterpret_cast<uintptr_t>(work->borrowedPageableRows.data());
    CyberpunkVR_RetainedPageableUseCount=UINT(work->borrowedPageableRows.size());
    CyberpunkVR_RetainedFixedAssetsRows=reinterpret_cast<uintptr_t>(work->fixedRangeRows.data());
    CyberpunkVR_RetainedFixedAssetsCount=UINT(work->fixedRangeRows.size());
    CyberpunkVR_RetainedFixedAssetRows=reinterpret_cast<uintptr_t>(work->fixedAssetRows.data());
    CyberpunkVR_RetainedFixedAssetCount=UINT(work->fixedAssetRows.size());
    CyberpunkVR_RetainedFixedSlotRows=reinterpret_cast<uintptr_t>(work->fixedSlotRows.data());
    CyberpunkVR_RetainedFixedSlotCount=UINT(work->fixedSlotRows.size());
    CyberpunkVR_RetainedFixedDescriptorRows=reinterpret_cast<uintptr_t>(work->fixedDescriptorRows.data());
    CyberpunkVR_RetainedFixedDescriptorCount=UINT(work->fixedDescriptorRows.size());
    CyberpunkVR_RetainedNativeTableRows=reinterpret_cast<uintptr_t>(work->nativeTableRows.data());
    CyberpunkVR_RetainedNativeTableCount=UINT(work->nativeTableRows.size());
    CyberpunkVR_RetainedNativeTableEntries=reinterpret_cast<uintptr_t>(work->nativeTableEntries.data());
    CyberpunkVR_RetainedNativeTableEntryCount=UINT(work->nativeTableEntries.size());
    CyberpunkVR_RetainedNativeTableRings=reinterpret_cast<uintptr_t>(work->nativeTableRings.data());
    CyberpunkVR_RetainedNativeTableRingCount=UINT(work->nativeTableRings.size());
    auto* stats=CyberpunkVR_RetainedSourceStreamStats;
    stats[0]=work->sourceFailed?4u:work->sourceCompiled?3u:work->eyes[0].sealed && work->eyes[1].sealed?2u:1u;
    stats[1]=work->frame;stats[2]=work->sourceRows.size();
    std::fill_n(stats+3,9,0ull);
    for(const auto& row:work->sourceRows){++stats[row[2]?4:3];if(row[1]<2)++stats[5+row[1]];
        stats[7]+=(row[43]&1)!=0;stats[8]+=(row[43]&2)!=0;stats[9]+=(row[43]&4)!=0;stats[10]+=(row[43]&8)!=0;}
    stats[11]=work->sourceCompiled?work->sourceGeometry.SourceCount():0;
    // Ownership of views/state is not a frozen input or native suppression permit.
    stats[13]=stats[14]=stats[15]=0;
    CyberpunkVR_RetainedSourceStreamRows=reinterpret_cast<uintptr_t>(work->sourceRows.data());
    CyberpunkVR_RetainedSourceStreamCount=UINT(work->sourceRows.size());
    auto* indirect=CyberpunkVR_RetainedIndirectBindingsStats;
    indirect[0]=stats[0];indirect[1]=work->frame;indirect[2]=work->indirectRows.size();
    std::fill_n(indirect+3,13,0ull);
    for(const auto& row:work->indirectRows){
        ++indirect[row[3]?3:4];indirect[5]+=row[26]!=0;
        indirect[6]+=row[15]==20;indirect[7]+=row[8]==2;
    }
    CyberpunkVR_RetainedIndirectBindingsRows=reinterpret_cast<uintptr_t>(work->indirectRows.data());
    CyberpunkVR_RetainedIndirectBindingsCount=UINT(work->indirectRows.size());
    if(work->sourceCbvs){
        auto* cbvs=CyberpunkVR_RetainedSourceCbvStats;std::fill_n(cbvs,16,0ull);
        cbvs[0]=work->cbvFailed?4u:stats[0];cbvs[1]=work->frame;cbvs[2]=work->sourceRows.size();cbvs[3]=work->cbvRows.size();
        for(const auto& row:work->cbvRows){++cbvs[row[23]?4:5];++cbvs[row[2]==D3D12_SHADER_VISIBILITY_VERTEX?6:7];
            if(row[23] && row[7]>=1 && row[7]<=3)++cbvs[7+row[7]];else if(!cbvs[11])cbvs[11]=row[7];}
        CyberpunkVR_RetainedSourceCbvRows=reinterpret_cast<uintptr_t>(work->cbvRows.data());CyberpunkVR_RetainedSourceCbvCount=UINT(work->cbvRows.size());
        CyberpunkVR_RetainedSourceCbvDiagnostics=reinterpret_cast<uintptr_t>(work->cbvDiagnostics.data());
        CyberpunkVR_RetainedSourceCbvProducerRows=reinterpret_cast<uintptr_t>(work->cbvProducers.data());CyberpunkVR_RetainedSourceCbvProducerCount=UINT(work->cbvProducers.size());
        CyberpunkVR_RetainedSelectorCoverageRows=reinterpret_cast<uintptr_t>(work->selectorCoverage.data());CyberpunkVR_RetainedSelectorCoverageCount=UINT(work->selectorCoverage.size());
        CyberpunkVR_RetainedSelectorCacheEventRows=reinterpret_cast<uintptr_t>(work->selectorCacheEvents.data());CyberpunkVR_RetainedSelectorCacheEventCount=UINT(work->selectorCacheEvents.size());
    }
    if(work->sourceSnapshots){
        auto* snapshots=CyberpunkVR_RetainedIndirectSnapshotsStats;
        std::fill_n(snapshots,16,0ull);snapshots[0]=work->snapshotFailed?4u:stats[0];snapshots[1]=work->frame;
        snapshots[2]=work->indirectRows.size();snapshots[3]=work->snapshotRows.size();
        for(const auto& row:work->snapshotRows){++snapshots[row[2]?4:5];snapshots[6]+=row[13];snapshots[7]+=row[21];snapshots[8]+=row[20];
            if(!snapshots[9] && row[5])snapshots[9]=row[5];}
        snapshots[10]=work->eyes[0].sealed+UINT(work->eyes[1].sealed);snapshots[11]=stats[11];
        CyberpunkVR_RetainedIndirectSnapshotsRows=reinterpret_cast<uintptr_t>(work->snapshotRows.data());
        CyberpunkVR_RetainedIndirectSnapshotsCount=UINT(work->snapshotRows.size());
    }
    if(work->sourceTables){
        auto* tables=CyberpunkVR_RetainedSourceTablesStats;std::fill_n(tables,16,0ull);
        tables[0]=work->tablesFailed?4u:stats[0];tables[1]=work->frame;tables[2]=work->sourceRows.size();tables[3]=work->tableCommands.size();
        for(const auto& row:work->tableCommands){++tables[row[2]?4:5];tables[10]+=row[9];tables[11]+=row[10];
            if(!row[2] && !tables[12])tables[12]=row[3];}
        tables[6]=work->tableRows.size();for(const auto& row:work->tableRows)tables[7]+=row[3];
        tables[8]=work->tableCache.TableCount();tables[9]=work->tableCache.DescriptorCount();
        CyberpunkVR_RetainedSourceTablesRows=reinterpret_cast<uintptr_t>(work->tableRows.data());CyberpunkVR_RetainedSourceTablesCount=UINT(work->tableRows.size());
        CyberpunkVR_RetainedSourceTablesCommands=reinterpret_cast<uintptr_t>(work->tableCommands.data());CyberpunkVR_RetainedSourceTablesCommandCount=UINT(work->tableCommands.size());
    }
    if(work->sourceDetails){
        auto* details=CyberpunkVR_RetainedSourceDetailsStats;std::fill_n(details,16,0ull);
        details[0]=work->detailsFailed?4u:stats[0];details[1]=work->frame;details[2]=work->sourceRows.size();details[3]=work->detailRows.size();
        for(const auto& row:work->detailRows){++details[row[7]?4:5];
            if(row[7]){if(row[8]==1)++details[6];else if(row[8]==2)++details[7];else if(row[8]==5)++details[8];
                details[9]+=row[18]!=0;details[10]+=row[31]!=0;details[11]+=row[64]!=0;details[12]+=row[84]!=0;}
            else if(!details[13])details[13]=row[39];}
        CyberpunkVR_RetainedSourceDetailsRows=reinterpret_cast<uintptr_t>(work->detailRows.data());CyberpunkVR_RetainedSourceDetailsCount=UINT(work->detailRows.size());
    }
    if(work->sourceCbvSnapshots){
        auto* snapshots=CyberpunkVR_RetainedSourceCbvSnapshotsStats;std::fill_n(snapshots,16,0ull);
        snapshots[0]=work->cbvSnapshotsFailed?4u:work->cbvPayloadComplete?3u:work->eyes[0].sealed && work->eyes[1].sealed?2u:1u;
        snapshots[1]=work->frame;snapshots[2]=work->cbvSnapshotCommands.size();snapshots[3]=work->cbvSnapshotRows.size();
        for(const auto& row:work->cbvSnapshotRows){++snapshots[row[8]==1?4:row[8]==2?5:6];snapshots[7]+=row[20];snapshots[8]+=row[21];
            if(row[8]==3 && !snapshots[11])snapshots[11]=row[9];}
        for(const auto& row:work->cbvSnapshotCommands){snapshots[10]+=row[18]!=0;snapshots[12]+=row[19]!=0;
            if(row[3] && !snapshots[11])snapshots[11]=row[3];}
        for(const auto& eye:work->eyes)snapshots[9]+=eye.cbvArena && eye.cbvArena->Finalized();
        // DEFAULT-only snapshots are partial input progress, never admission.
        snapshots[13]=snapshots[14]=snapshots[15]=0;
        CyberpunkVR_RetainedSourceCbvSnapshotsRows=reinterpret_cast<uintptr_t>(work->cbvSnapshotRows.data());
        CyberpunkVR_RetainedSourceCbvSnapshotsCount=UINT(work->cbvSnapshotRows.size());
        CyberpunkVR_RetainedSourceCbvSnapshotsCommands=reinterpret_cast<uintptr_t>(work->cbvSnapshotCommands.data());
        CyberpunkVR_RetainedSourceCbvCopyOwnerRows=reinterpret_cast<uintptr_t>(work->cbvCopyOwners.data());
        CyberpunkVR_RetainedSourceCbvCopyOwnerCount=UINT(work->cbvCopyOwners.size());
        CyberpunkVR_RetainedSourceCbvSnapshotsCommandCount=UINT(work->cbvSnapshotCommands.size());
        CyberpunkVR_RetainedSourceCbvSnapshotsBlob=reinterpret_cast<uintptr_t>(work->cbvBlob.data());
        CyberpunkVR_RetainedSourceCbvSnapshotsBlobBytes=UINT(work->cbvBlob.size());
    }
    if(work->sourceSelectors){
        auto* selectors=CyberpunkVR_RetainedSourceSelectorsStats;std::fill_n(selectors,16,0ull);
        selectors[0]=work->complete?3u:work->cbvPayloadComplete?2u:1u;selectors[1]=work->frame;
        selectors[2]=work->selectorCommands.size();selectors[3]=work->selectorInputs.size();selectors[4]=work->selectorSlots.size();selectors[5]=work->t5Rows.size();
        for(const auto& row:work->selectorCommands){selectors[6]+=row[2]==1;selectors[7]+=row[2]==2;selectors[8]+=row[2]>=3;selectors[9]+=row[8];selectors[10]+=row[11]!=0;}
        for(const auto& row:work->selectorInputs){selectors[11]+=row[7]!=0;selectors[12]+=row[13]!=0;}
        selectors[13]=work->tableCache.SlotTableCount();selectors[14]=work->tableCache.SlotDescriptorCount();
        // Index evaluation and compact descriptor ownership are not execution permission.
        selectors[15]=0;
        CyberpunkVR_RetainedSourceSelectorsCommands=reinterpret_cast<uintptr_t>(work->selectorCommands.data());CyberpunkVR_RetainedSourceSelectorsCommandCount=UINT(work->selectorCommands.size());
        CyberpunkVR_RetainedSourceSelectorsInputs=reinterpret_cast<uintptr_t>(work->selectorInputs.data());CyberpunkVR_RetainedSourceSelectorsInputCount=UINT(work->selectorInputs.size());
        CyberpunkVR_RetainedSourceSelectorsSlots=reinterpret_cast<uintptr_t>(work->selectorSlots.data());CyberpunkVR_RetainedSourceSelectorsSlotCount=UINT(work->selectorSlots.size());
        CyberpunkVR_RetainedSourceSelectorsT5=reinterpret_cast<uintptr_t>(work->t5Rows.data());CyberpunkVR_RetainedSourceSelectorsT5Count=UINT(work->t5Rows.size());
        CyberpunkVR_RetainedSourceSelectorsBlob=reinterpret_cast<uintptr_t>(work->selectorBlob.data());CyberpunkVR_RetainedSourceSelectorsBlobBytes=UINT(work->selectorBlob.size());
        CyberpunkVR_RetainedT5NativeRows=reinterpret_cast<uintptr_t>(work->t5NativeRows.data());CyberpunkVR_RetainedT5NativeCount=UINT(work->t5NativeRows.size());
        CyberpunkVR_RetainedT5SnapshotRows=reinterpret_cast<uintptr_t>(work->t5SnapshotRows.data());CyberpunkVR_RetainedT5SnapshotCount=UINT(work->t5SnapshotRows.size());
        CyberpunkVR_RetainedT5SnapshotBlob=reinterpret_cast<uintptr_t>(work->t5Blob.data());CyberpunkVR_RetainedT5SnapshotBlobBytes=UINT(work->t5Blob.size());
        CyberpunkVR_RetainedT5IntentBlob=reinterpret_cast<uintptr_t>(work->t5IntentBlob.data());CyberpunkVR_RetainedT5IntentBlobBytes=UINT(work->t5IntentBlob.size());
        CyberpunkVR_RetainedT5CopyDiagnosticRows=reinterpret_cast<uintptr_t>(work->t5CopyDiagnostics.data());CyberpunkVR_RetainedT5MarkDiagnosticRows=reinterpret_cast<uintptr_t>(work->t5MarkDiagnostics.data());
        CyberpunkVR_RetainedT5DiagnosticCount=UINT(work->t5CopyDiagnostics.size());
        CyberpunkVR_RetainedT5PieceRows=reinterpret_cast<uintptr_t>(work->t5Pieces.data());CyberpunkVR_RetainedT5PieceCount=UINT(work->t5Pieces.size());
        CyberpunkVR_RetainedT5CoverageRows=reinterpret_cast<uintptr_t>(work->t5Coverage.data());CyberpunkVR_RetainedT5CoverageCount=UINT(work->t5Coverage.size());
        auto* assembly=CyberpunkVR_RetainedSourceAssemblyStats;std::fill_n(assembly,16,0ull);
        assembly[0]=work->assemblyTruncated?4u:work->assemblyComplete?3u:1u;assembly[1]=work->frame;assembly[2]=work->assemblyCommands.size();
        for(const auto& row:work->assemblyCommands){++assembly[row[2]?3:4];assembly[5]+=row[4]!=0;assembly[6]+=row[5];
            assembly[8]+=row[7];assembly[9]+=row[21]!=0;assembly[10]+=row[22]!=0;if(row[3] && !assembly[11])assembly[11]=row[3];}
        assembly[7]=work->assemblyTables.size();assembly[12]=work->assemblyTruncated;
        // Bank creation does not certify a later input epoch or execute geometry.
        assembly[13]=assembly[14]=assembly[15]=0;
        CyberpunkVR_RetainedSourceAssemblyCommands=reinterpret_cast<uintptr_t>(work->assemblyCommands.data());CyberpunkVR_RetainedSourceAssemblyCommandCount=UINT(work->assemblyCommands.size());
        CyberpunkVR_RetainedSourceAssemblyTables=reinterpret_cast<uintptr_t>(work->assemblyTables.data());CyberpunkVR_RetainedSourceAssemblyTableCount=UINT(work->assemblyTables.size());
        CyberpunkVR_RetainedSourceAssemblyMissingCbvs=reinterpret_cast<uintptr_t>(work->assemblyMissingCbvs.data());CyberpunkVR_RetainedSourceAssemblyMissingCbvCount=UINT(work->assemblyMissingCbvs.size());
        CyberpunkVR_RetainedSourceAssemblyCacheStats[0]=work->assemblyCache.UniqueDescriptorCount();
        CyberpunkVR_RetainedSourceAssemblyCacheStats[1]=work->assemblyCache.TableCount();
        CyberpunkVR_RetainedSourceAssemblyCacheStats[2]=work->assemblyCache.HitCount();
        CyberpunkVR_RetainedSourceAssemblyCacheStats[3]=work->assemblyCache.MissCount();
        auto* bank=CyberpunkVR_RetainedSourceAssemblyBankStats;
        bank[0]=work->assemblyBankAttempted;bank[1]=work->assemblyBank!=nullptr;
        bank[2]=work->assemblyBank?work->assemblyBank->DrawCount():0;
        bank[3]=work->assemblyBank?work->assemblyBank->DescriptorCount():0;
        bank[4]=uint32_t(work->assemblyBankResult);bank[5]=0;
        std::copy(work->assemblyBankFailure.begin(),work->assemblyBankFailure.end(),CyberpunkVR_RetainedSourceAssemblyBankFailure);
        auto* overlay=CyberpunkVR_RetainedT5OverlayStats;std::fill_n(overlay,16,0ull);
        overlay[0]=work->overlayStage;overlay[1]=work->frame;overlay[2]=work->overlayRows.size();
        if(work->t5Overlay){overlay[3]=work->t5Overlay->RequiredBytes();overlay[4]=work->t5Overlay->CopiedBytes();}
        overlay[11]=work->overlayRows.size();overlay[13]=work->overlayStage?1u:0u;
        for(const auto& row:work->overlayRows)overlay[12]+=row[28]!=0;
        CyberpunkVR_RetainedT5OverlayRows=reinterpret_cast<uintptr_t>(work->overlayRows.data());CyberpunkVR_RetainedT5OverlayCount=UINT(work->overlayRows.size());
        CyberpunkVR_RetainedT5OverlayBlob=reinterpret_cast<uintptr_t>(work->overlayBlob.data());CyberpunkVR_RetainedT5OverlayBlobBytes=UINT(work->overlayBlob.size());
    }
    CyberpunkVR_RetainedIaCommands=reinterpret_cast<uintptr_t>(work->iaCommands.data());CyberpunkVR_RetainedIaCommandCount=UINT(work->iaCommands.size());
    CyberpunkVR_RetainedIaObservations=reinterpret_cast<uintptr_t>(work->iaObservations.data());CyberpunkVR_RetainedIaObservationCount=UINT(work->iaObservations.size());
    CyberpunkVR_RetainedIaViews=reinterpret_cast<uintptr_t>(work->iaViews.data());CyberpunkVR_RetainedIaViewCount=UINT(work->iaViews.size());
    CyberpunkVR_RetainedIaReuseRows=reinterpret_cast<uintptr_t>(work->iaReuse.data());CyberpunkVR_RetainedIaReuseCount=UINT(work->iaReuse.size());
    CyberpunkVR_RetainedIaTransferRows=reinterpret_cast<uintptr_t>(work->iaTransfers.data());CyberpunkVR_RetainedIaTransferCount=UINT(work->iaTransfers.size());
    CyberpunkVR_RetainedInstanceRows=reinterpret_cast<uintptr_t>(work->instanceRows.data());CyberpunkVR_RetainedInstanceCount=UINT(work->instanceRows.size());
    CyberpunkVR_RetainedInstanceBlob=reinterpret_cast<uintptr_t>(work->instanceBlob.data());CyberpunkVR_RetainedInstanceBlobBytes=UINT(work->instanceBlob.size());
    CyberpunkVR_RetainedInstanceGpuRows=reinterpret_cast<uintptr_t>(work->instanceGpuRows.data());CyberpunkVR_RetainedInstanceGpuCount=UINT(work->instanceGpuRows.size());
    CyberpunkVR_RetainedInstanceGpuBlob=reinterpret_cast<uintptr_t>(work->instanceGpuBlob.data());CyberpunkVR_RetainedInstanceGpuBlobBytes=UINT(work->instanceGpuBlob.size());
    auto* instanceOverlay=CyberpunkVR_RetainedInstanceOverlayStats;
    std::fill_n(instanceOverlay,8,0ull);instanceOverlay[0]=work->instanceOverlayStage;
    for(const auto& eye:work->eyes)if(eye.instanceCapture){
        instanceOverlay[1]+=eye.instanceCapture->VersionCount();instanceOverlay[2]+=eye.instanceCapture->CopiedBytes();instanceOverlay[3]+=eye.instanceCapture->RequiredBytes();
        instanceOverlay[4]=std::max(instanceOverlay[4],eye.signaledValue);
        const auto completed=eye.fence?eye.fence->GetCompletedValue():0;
        instanceOverlay[5]=instanceOverlay[5]?std::min(instanceOverlay[5],completed):completed;
    }
    CyberpunkVR_RetainedInstanceOverlayRows=reinterpret_cast<uintptr_t>(work->instanceOverlayRows.data());CyberpunkVR_RetainedInstanceOverlayCount=UINT(work->instanceOverlayRows.size());
    CyberpunkVR_RetainedInstanceOverlayBlob=reinterpret_cast<uintptr_t>(work->instanceOverlayBlob.data());CyberpunkVR_RetainedInstanceOverlayBlobBytes=UINT(work->instanceOverlayBlob.size());
    CyberpunkVR_RetainedIaSelections=reinterpret_cast<uintptr_t>(work->iaSelections.data());CyberpunkVR_RetainedIaSelectionCount=UINT(work->iaSelections.size());
    CyberpunkVR_RetainedPrivateGeometryRows=reinterpret_cast<uintptr_t>(work->privateGeometryRows.data());CyberpunkVR_RetainedPrivateGeometryCount=UINT(work->privateGeometryRows.size());
    CyberpunkVR_RetainedMaterialRows=reinterpret_cast<uintptr_t>(work->materialRows.data());CyberpunkVR_RetainedMaterialCount=UINT(work->materialRows.size());
    CyberpunkVR_RetainedMaterialSources=reinterpret_cast<uintptr_t>(work->materialSourceRows.data());CyberpunkVR_RetainedMaterialSourceCount=UINT(work->materialSourceRows.size());
    CyberpunkVR_RetainedMaterialBufferRows=reinterpret_cast<uintptr_t>(work->materialBufferRows.data());CyberpunkVR_RetainedMaterialBufferCount=UINT(work->materialBufferRows.size());
    CyberpunkVR_RetainedMaterialBufferCopyRows=reinterpret_cast<uintptr_t>(work->materialBufferCopies.data());CyberpunkVR_RetainedMaterialBufferCopyCount=UINT(work->materialBufferCopies.size());
}
void CaptureDetails(ID3D12GraphicsCommandList* list,uint64_t root,const std::array<uint64_t,24>& cbv,
    const source_cbv::SourceCbvBacking& backing){
    std::array<uint64_t,128> row{};row[0]=work->cbvRows.size();std::copy_n(cbv.begin(),6,row.begin()+1);
    source_cbv_details::Detail detail;
    const bool captured=source_cbv_details::CaptureAtSource(list,root,UINT(cbv[4]),UINT(cbv[5]),backing,detail);
    row[7]=captured;row[8]=UINT(detail.resource.heap.Type);row[9]=UINT(detail.resource.heapFlags);row[10]=UINT(detail.resource.flags);
    row[11]=detail.resource.width;row[12]=detail.resource.gpu;row[13]=detail.resource.comOffset;
    row[14]=backing.recording.generation;row[15]=backing.recording.context;row[16]=root;row[17]=reinterpret_cast<uintptr_t>(list);
    constants::ResourceCreationInfo creation;
    if(captured && constants::QueryResourceCreation(backing.resource,creation)){
        row[18]=creation.kind;row[19]=creation.initialStateKnown;row[20]=UINT(creation.initialState);
        row[21]=reinterpret_cast<uintptr_t>(creation.heap.Get());row[22]=creation.heapOffset;row[23]=creation.allocationBytes;
    }
    using Observation=source_cbv_details::Observation;
    row[24]=detail.context.observation==Observation::Known;row[25]=detail.context.kind;row[26]=detail.context.pending;row[27]=detail.context.submitted;
    row[28]=detail.state.status;row[29]=detail.state.value;row[30]=detail.state.kind;
    if(detail.record.observation==Observation::Known){row[31]=1;row[32]=detail.record.address;std::memcpy(row.data()+40,detail.record.bytes.data(),detail.record.bytes.size());}
    row[33]=backing.native.kind;row[34]=backing.native.handle;row[35]=reinterpret_cast<uintptr_t>(backing.resource.Get());
    row[36]=backing.stamp.value[4];row[37]=backing.descriptorBytes;row[38]=cbv[6];row[39]=UINT(detail.status);
    const auto readable=[](Observation value){return value==Observation::Known || value==Observation::UnknownAbi || value==Observation::Unsupported;};
    if(readable(detail.allocation.observation)){row[64]=1;row[65]=detail.allocation.address;std::memcpy(row.data()+66,detail.allocation.raw.data(),detail.allocation.raw.size());}
    if(readable(detail.allocator.observation)){row[79]=1;row[80]=detail.allocator.address;std::memcpy(row.data()+81,detail.allocator.raw.data(),detail.allocator.raw.size());}
    if(detail.pool.observation==Observation::Known){row[84]=1;row[85]=detail.pool.entry;row[86]=detail.pool.owner;row[87]=detail.pool.packed;
        row[88]=detail.pool.slot;row[89]=detail.pool.unit;row[90]=detail.pool.occupiedWord;}
    if(readable(detail.alias.observation)){row[96]=1;row[97]=detail.alias.payload;row[98]=detail.alias.control;std::memcpy(row.data()+99,detail.alias.raw.data(),detail.alias.raw.size());}
    row[106]=UINT(detail.allocation.observation);row[107]=UINT(detail.allocator.observation);row[108]=UINT(detail.pool.observation);row[109]=UINT(detail.alias.observation);
    row[110]=UINT(detail.allocationPath);row[111]=detail.allocation.associationKnown;row[112]=detail.allocation.resourceMatches;row[113]=detail.allocation.extentCovers;
    row[114]=detail.allocation.aliasShapeKnown;row[115]=detail.allocation.aliasShapeMatches;row[116]=detail.allocator.associationKnown;row[117]=detail.allocator.pimplMatches;
    row[118]=detail.resource.heap.CreationNodeMask;row[119]=detail.resource.heap.VisibleNodeMask;
    row[120]=UINT(detail.resource.heap.CPUPageProperty);row[121]=UINT(detail.resource.heap.MemoryPoolPreference);
    row[122]=backing.native.registry;row[123]=detail.context.state;row[124]=detail.allocation.heap;row[125]=detail.allocation.block;row[126]=detail.allocation.handle;
    if(!captured)work->detailsFailed=true;work->detailRows.push_back(row);
}
void CaptureTables(ID3D12GraphicsCommandList* list,Work::Source& source,const std::array<uint64_t,48>& row){
    std::array<uint64_t,24> receipt{};receipt[0]=row[0];receipt[1]=row[1];receipt[4]=receipt[5]=UINT_MAX;
    receipt[11]=row[11];receipt[12]=row[6];receipt[13]=row[7];receipt[14]=row[8];receipt[15]=source.cbvs.size();
    const auto reject=[&](UINT reason){receipt[3]=reason;work->tablesFailed=true;};
    const auto pipeline=reinterpret_cast<ID3D12PipelineState*>(row[10]);const auto family=families::Lookup(pipeline);
    if(!family || family->sourceRoot.Get()!=source.root.Get() || !source.shader){reject(101);}
    else{
        auto found=work->tablePlans.find(pipeline);
        if(found==work->tablePlans.end()){
            ComPtr<ID3D12VersionedRootSignatureDeserializer> reader;const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* desc{};source_tables::Plan plan;
            if(FAILED(D3D12CreateVersionedRootSignatureDeserializer(source.shader->root.data(),source.shader->root.size(),IID_PPV_ARGS(&reader))) ||
                FAILED(reader->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&desc)) ||
                !source_tables::Build(desc->Desc_1_1,family->uses,plan))reject(102);
            else found=work->tablePlans.emplace(pipeline,std::move(plan)).first;
        }
        if(!receipt[3]){
            const auto& plan=found->second;std::vector<descriptors::SourceTableRequest> requests;requests.reserve(plan.finite.size());
            receipt[6]=plan.finite.size();receipt[10]=plan.deferred.size();
            for(const auto& table:plan.finite){requests.push_back({table.parameter,table.count,table.uses});
                for(const auto& use:table.uses)receipt[9]+=use.count;}
            const auto provider=[](void* user,ID3D12GraphicsCommandList*,UINT,UINT,const descriptors::DescriptorStamp& stamp,
                const ComPtr<IUnknown>& origin,descriptors::SourceCbvBacking& backing){
                auto& source=*static_cast<Work::Source*>(user);
                for(const auto& cbv:source.cbvs)if(cbv.stamp.gpu==stamp.gpu && cbv.stamp.heap==stamp.heap && cbv.stamp.value==stamp.value &&
                    cbv.originOwner.Get()==origin.Get() && cbv.resource){backing={cbv.resource,cbv.originOwner};return true;}
                return false;
            };
            if(!descriptors::CaptureSourceTablesAtDraw(work->device.Get(),list,source.bindings,requests,provider,&source,work->tableCache,source.tables,&source.finiteDependencies)){
                const auto failure=descriptors::LastSourceTableFailure();receipt[4]=failure.parameter;receipt[5]=failure.offset;reject(failure.reason);
            }else{
                receipt[2]=1;receipt[7]=source.tables.size();
                for(const auto& table:source.tables){
                    std::array<uint64_t,16> value{row[0],row[1],table.parameter,table.count,reinterpret_cast<uintptr_t>(table.cpuHeap.Get()),
                        reinterpret_cast<uintptr_t>(table.owner.Get()),table.identity,table.version,table.sourceGpu,table.sourceHeap};
                    const auto& planned=*std::find_if(plan.finite.begin(),plan.finite.end(),[&](const auto& item){return item.parameter==table.parameter;});
                    for(const auto& use:planned.uses){value[10]+=use.count;
                        value[11+(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV?0:use.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV?1:
                            use.type==D3D12_DESCRIPTOR_RANGE_TYPE_UAV?2:3)]+=use.count;}
                    receipt[8]+=table.count;work->tableRows.push_back(value);
                }
            }
        }
    }
    work->tableCommands.push_back(receipt);
}
void CaptureSelectorInput(ID3D12GraphicsCommandList* list,Work::Source& source,const std::array<uint64_t,48>& command,
    uint64_t position,const source_selectors::CbvRequirement& requirement){
    const auto& key=requirement.key;std::array<uint64_t,24> row{command[0],command[1],key.visibility,key.shaderRegister,key.parameter,key.tableOffset,requirement.reflectedBytes};
    row[11]=position;row[21]=command[7];row[22]=command[8];
    Work::Source::SelectorInput input;input.key=key;input.row=work->selectorInputs.size();
    std::array<uint64_t,28> coverage{input.row,command[0],command[1],0};
    std::array<uint64_t,36> events{input.row,command[0],command[1],0};
    UINT parameter{},offset{};
    const auto stage=key.visibility==D3D12_SHADER_VISIBILITY_VERTEX?0u:1u;
    const auto found=std::find_if(source.cbvs.begin(),source.cbvs.end(),[&](const auto& cbv){return cbv.stage==stage && cbv.shaderRegister==key.shaderRegister;});
    if(found!=source.cbvs.end() && descriptors::FindBinding(command[11],D3D12_DESCRIPTOR_RANGE_TYPE_CBV,key.space,key.shaderRegister,
        D3D12_SHADER_VISIBILITY(key.visibility),parameter,offset) && parameter==key.parameter && offset==key.tableOffset){
        const auto& cbv=*found;row[17]=cbv.stamp.gpu;row[18]=cbv.stamp.heap;row[19]=reinterpret_cast<uintptr_t>(cbv.resource.Get());row[20]=cbv.stamp.value[4];
        source_resource_intent::GenericCoverageDiagnostic current;
        if(source_resource_intent::QueryGenericCoverage(cbv.resource,cbv.sourceOffset,requirement.reflectedBytes,current)){
            coverage[3]=1;static_assert(sizeof(current)==24*sizeof(uint64_t));std::memcpy(coverage.data()+4,&current,sizeof(current));
        }
        source_resource_intent::CacheEventDiagnostic history;
        if(source_resource_intent::QueryCacheEvents(cbv.resource,cbv.sourceOffset,requirement.reflectedBytes,history)){
            events[3]=1;static_assert(sizeof(history)==32*sizeof(uint64_t));std::memcpy(events.data()+4,&history,sizeof(history));
        }
        if(position && descriptors::MatchesDescriptor(cbv.stamp) &&
            source_resource_intent::CaptureAtDraw(cbv.resource,cbv.stamp.value[4],requirement.reflectedBytes,input.intent)){
            // Producer list/sequence are observation labels, not queue/epoch proof.
            row[7]=1;row[8]=input.intent.CopySequence();row[9]=input.intent.ProducerList();row[10]=input.intent.CopySequence();
            row[12]=input.intent.ViewOffset();row[15]=work->selectorBlob.size()+1;row[16]=input.intent.Bytes().size();
            work->selectorBlob.insert(work->selectorBlob.end(),input.intent.Bytes().begin(),input.intent.Bytes().end());
        }
    }
    source.selectorInputs.push_back(std::move(input));work->selectorInputs.push_back(row);
    work->selectorCoverage.push_back(coverage);
    work->selectorCacheEvents.push_back(events);
}
void CaptureMultilayerT5(ID3D12GraphicsCommandList* list,Work::Source& source,const std::array<uint64_t,48>& command){
    std::array<uint64_t,32> row{};row[0]=command[0];row[1]=command[1];row[27]=command[7];
    UINT parameter{},offset{};auto& binding=source.multilayerT5;
    if(!descriptors::FindBinding(command[11],D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,5,D3D12_SHADER_VISIBILITY_PIXEL,parameter,offset) ||
        parameter!=23 || offset!=5 || !descriptors::ResolveSrvAtDraw(list,command[11],parameter,offset,binding)){row[3]=1;}
    else{
        const auto desc=binding.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
        row[4]=parameter;row[5]=offset;row[6]=binding.stamp.gpu;row[7]=binding.stamp.heap;row[8]=binding.description[0];row[9]=binding.description[2];
        row[10]=reinterpret_cast<uintptr_t>(binding.resource.Get());row[11]=binding.view.Buffer.FirstElement;row[12]=binding.view.Buffer.NumElements;row[13]=binding.view.Buffer.StructureByteStride;
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || binding.view.ViewDimension!=D3D12_SRV_DIMENSION_BUFFER ||
            binding.view.Format!=DXGI_FORMAT_UNKNOWN || binding.view.Buffer.Flags || row[13]!=128 ||
            FAILED(binding.resource->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_DEFAULT ||
            !descriptors::MatchesDescriptor(binding.stamp)){row[3]=2;}
        else{
            row[2]=1;
            if(constants::QueryResourceCreation(binding.resource,source.multilayerCreation)){
                const auto& creation=source.multilayerCreation;row[21]=creation.kind;row[22]=creation.initialStateKnown;row[23]=UINT(creation.initialState);
                row[24]=reinterpret_cast<uintptr_t>(creation.heap.Get());row[25]=creation.heapOffset;row[26]=creation.allocationBytes;
            }
            if(!source.selectorInputs.empty() && source.selectorInputs[0].intent){
                const auto bytes=source.selectorInputs[0].intent.Bytes();float base{};uint32_t mask{};
                if(bytes.size()>=228){std::memcpy(&base,bytes.data()+128,4);std::memcpy(&mask,bytes.data()+224,4);
                    if(std::isfinite(base) && base>=0 && double(base)<4294967296.0 && mask){
                        row[16]=uint32_t(base);row[17]=mask;row[18]=std::bit_width(mask);
                        if(row[16]<=row[12] && row[18]<=row[12]-row[16] && row[11]<=UINT64_MAX-row[16] &&
                            row[11]+row[16]<=UINT64_MAX/128){
                            row[14]=(row[11]+row[16])*128;row[15]=row[18]*128;
                            row[19]=row[14]<=desc.Width && row[15]<=desc.Width-row[14];
                        }
                    }
                }
            }
        }
    }
    // Current view ownership only: no native state/allocation/copy permission.
    if(row[2]){
        const auto captured=std::find_if(source.tables.begin(),source.tables.end(),[](const auto& table){return table.parameter==23;});
        source.t5OriginalCertified=captured!=source.tables.end() && descriptors::MatchesCapturedSourceSrv(*captured,5,binding);
    }
    work->t5Rows.push_back(row);
    std::array<uint64_t,96> native{};native[0]=command[0];native[1]=command[1];native[15]=command[6];native[16]=command[7];
    if(row[2]){
        native[2]=UINT(source_srv::ReadAtSource(list,command[11],23,5,binding,work->eyes[size_t(command[1])].recording,source.t5Native));
        const auto& current=source.t5Native;native[3]=current.stage;native[4]=current.shaderRegister;native[5]=current.route;native[6]=current.lowType;
        native[7]=current.native.handle;native[8]=current.native.kind;native[9]=current.native.gpu;native[10]=current.native.bytes;native[11]=current.native.resource;
        native[12]=current.native.registry;native[13]=current.context.context;native[14]=current.context.state;native[17]=current.recordAddress;
        native[18]=current.cpuDescriptor;native[19]=current.serial;native[20]=current.state.status;native[21]=current.state.value;native[22]=current.state.kind;
        native[23]=native[2]==UINT(source_srv::Status::Captured);native[24]=UINT(current.allocation.observation);native[25]=current.allocation.address;
        native[26]=current.allocation.kind;native[27]=current.allocation.resource;native[28]=current.allocation.bytes;native[29]=current.allocation.heap;
        native[30]=current.allocation.pimpl;native[31]=current.allocator.address;native[32]=current.allocator.pimpl;native[33]=UINT(current.allocator.observation);
        native[34]=current.gpu;native[35]=current.backingBytes;native[36]=current.sourceOffset;native[37]=current.viewBytes;
        native[38]=current.slot.group;native[39]=current.slot.first;native[40]=current.slot.count;native[41]=current.slot.cachedIndex;
        native[42]=source_srv::Compatible();native[43]=current.heapType;native[44]=current.flags;
        std::memcpy(native.data()+48,current.rawRecord.data(),current.rawRecord.size());std::memcpy(native.data()+72,current.slot.raw.data(),current.slot.raw.size());
    }
    work->t5NativeRows.push_back(native);
    std::array<uint64_t,32> copied{},marked{};copied[0]=marked[0]=command[0];copied[1]=marked[1]=command[1];
    source_resource_intent::CopyDiagnostic copy;source_resource_intent::MarkDiagnostic mark;
    static_assert(sizeof(copy)==24*sizeof(uint64_t) && sizeof(mark)==24*sizeof(uint64_t));
    if(binding.resource && source_resource_intent::QueryCopyDiagnostic(binding.resource,copy)){copied[2]=1;std::memcpy(copied.data()+4,&copy,sizeof(copy));}
    if(binding.resource && source_resource_intent::QueryMarkDiagnostic(binding.resource,mark)){marked[2]=1;std::memcpy(marked.data()+4,&mark,sizeof(mark));}
    work->t5CopyDiagnostics.push_back(copied);work->t5MarkDiagnostics.push_back(marked);
}
bool CaptureSelectedSlots(ID3D12GraphicsCommandList* list,Work::Source& source,const std::array<uint64_t,48>& sourceRow,std::array<uint64_t,32>& command){
    command[8]=source.selectorResult.evaluatedSites;
    if(source.selectorResult.tables.empty()){command[2]=1;return true;}
    std::vector<std::vector<descriptors::SourceSlotUse>> uses;uses.reserve(source.selectorResult.tables.size());
    std::vector<descriptors::SourceTableRequest> requests;requests.reserve(source.selectorResult.tables.size());bool current=true;
    for(const auto& table:source.selectorResult.tables){UINT parameter{},offset{};
        current&=descriptors::FindBinding(sourceRow[11],D3D12_DESCRIPTOR_RANGE_TYPE_SRV,table.key.space,table.key.shaderRegisterBase,
            D3D12_SHADER_VISIBILITY(table.key.visibility),parameter,offset) && parameter==table.key.parameter && offset==table.key.originalTableOffset;
        uses.emplace_back();for(const auto& use:table.uses)uses.back().push_back({use.offset,use.count,D3D12_DESCRIPTOR_RANGE_TYPE_SRV});
        requests.push_back({table.key.parameter,table.count,uses.back()});
    }
    if(!current || !descriptors::CaptureSourceSlotsAtDraw(work->device.Get(),list,source.bindings,requests,nullptr,nullptr,work->tableCache,source.selectorSlots,&source.sparseDependencies)){
        command[2]=5;command[3]=descriptors::LastSourceTableFailure().reason;return false;
    }
    command[2]=1;command[9]=source.selectorSlots.size();
    for(const auto& table:source.selectorSlots)for(const auto& slot:table.slots){
        work->selectorSlots.push_back({sourceRow[0],sourceRow[1],table.parameter,slot.offset,slot.compactIndex,UINT(slot.type),slot.stamp.gpu,slot.stamp.heap,
            slot.stamp.value[0],slot.stamp.value[2],reinterpret_cast<uintptr_t>(table.cpuHeap.Get()),reinterpret_cast<uintptr_t>(table.owner.Get()),table.identity,table.version,table.count});
        ++command[10];
    }
    return true;
}
void CaptureMultilayerSelectors(ID3D12GraphicsCommandList* list,Work::Source& source,const std::array<uint64_t,48>& sourceRow,
    const families::Family& family,std::array<uint64_t,32>& command){
    command[25]=1;const auto& binding=source.multilayerT5;
    if(!binding.resource || source.selectorInputs.empty() || !source.selectorInputs[0].intent){command[2]=3;return;}
    const auto& view=binding.view;multilayer_selectors::StructuredView input;
    input.key={family.rootSha256,5,5,0,23,5};input.firstElement=view.Buffer.FirstElement;input.numElements=view.Buffer.NumElements;
    input.structureByteStride=view.Buffer.StructureByteStride;input.resourceWidth=binding.resource->GetDesc().Width;
    input.format=UINT(view.Format);input.dimension=UINT(view.ViewDimension);input.componentMapping=view.Shader4ComponentMapping;input.flags=UINT(view.Buffer.Flags);input.explicitDescription=true;
    const auto& b4=source.selectorInputs[0];std::string error;
    if(!multilayer_selectors::PlanRead({family.rootSha256,family.vertexSha256,family.pixelSha256},{b4.key,b4.intent.Bytes()},input,source.t5Plan,error)){command[2]=4;command[3]=1;return;}
    std::span<const uint8_t> bytes;
    if(source.t5Plan.byteExtent){
        const auto address=binding.resource->GetGPUVirtualAddress();
        if(address>UINT64_MAX-source.t5Plan.resourceOffset || !source_resource_intent::CaptureStructuredAtDraw(binding.resource,address+source.t5Plan.resourceOffset,
            source.t5Plan.byteExtent,source.t5Candidate)){command[2]=3;command[3]=2;return;}
        bytes=source.t5Candidate.Bytes();
        if(source.t5Candidate.Pieces().size()>65536-work->t5Pieces.size()){
            source.t5Candidate={};command[2]=3;command[3]=4;return;
        }
    }
    if(!multilayer_selectors::Enumerate(source.t5Plan,{input,source.t5Plan.resourceOffset,bytes},source.selectorResult,error)){command[2]=4;command[3]=3;return;}
    CaptureSelectedSlots(list,source,sourceRow,command);
}
void CaptureSelectors(ID3D12GraphicsCommandList* list,Work::Source& source,const std::array<uint64_t,48>& sourceRow){
    std::array<uint64_t,32> command{};command[0]=sourceRow[0];command[1]=sourceRow[1];command[13]=sourceRow[6];command[14]=sourceRow[7];command[15]=sourceRow[8];
    command[16]=sourceRow[11];command[17]=sourceRow[12];command[20]=CyberpunkVR_RegisteredCopyCount.load();command[21]=CyberpunkVR_RegisteredCopyRejected.load();
    command[22]=CyberpunkVR_RegisteredCopyChanged.load();
    const auto family=families::Lookup(reinterpret_cast<ID3D12PipelineState*>(sourceRow[10]));
    const auto* plan=family && work->selectorCatalog?work->selectorCatalog->Find({family->rootSha256,family->vertexSha256,family->pixelSha256}):nullptr;
    if(!plan || sourceRow[44])command[2]=6;
    else{
        command[4]=family->id;command[18]=plan->number;command[5]=sourceRow[0]+1;
        if(!plan->supported){
            command[2]=2;
            if(family->pixelSha256=="9a4947a3ec0eb3d4605d88b663bb27a0bc809fe8a3d91c4ae67886bd77148db8"){
                source_selectors::CbvRequirement input;input.key={family->rootSha256,5,4,0,19,0};input.reflectedBytes=228;
                CaptureSelectorInput(list,source,sourceRow,command[5],input);CaptureMultilayerT5(list,source,sourceRow);command[6]=1;
                CaptureMultilayerSelectors(list,source,sourceRow,*family,command);
            }
        }else{
            command[6]=plan->cbvs.size();for(const auto& requirement:plan->cbvs)CaptureSelectorInput(list,source,sourceRow,command[5],requirement);
            std::vector<source_selectors::OwnedCbvBytes> bytes;
            for(const auto& input:source.selectorInputs)if(input.intent)bytes.push_back({input.key,input.intent.Bytes()});
            command[7]=bytes.size();std::string error;
            if(bytes.size()!=plan->cbvs.size())command[2]=3;
            else if(!source_selectors::Evaluate(*plan,bytes,source.selectorResult,error))command[2]=4;
            else{
                CaptureSelectedSlots(list,source,sourceRow,command);
            }
        }
        if(!plan->supported)for(const auto& input:source.selectorInputs)command[7]+=bool(input.intent);
    }
    work->selectorCommands.push_back(command);
}
void CaptureCbvSnapshots(ID3D12GraphicsCommandList* list,UINT side,Work::Source& source,const std::array<uint64_t,48>& sourceRow){
    auto& eye=work->eyes[side];std::array<uint64_t,24> command{};
    command[0]=sourceRow[0];command[1]=side;command[11]=reinterpret_cast<uintptr_t>(list);command[12]=eye.recording.generation;
    command[13]=eye.recording.context;command[14]=sourceRow[0]+1;
    std::vector<std::array<uint64_t,32>> rows;std::vector<source_cbv_snapshot::Request> requests;
    const auto reject=[&](UINT code){command[2]=3;command[3]=code;work->cbvSnapshotsFailed=true;
        eye.cbvArenaAborted=true;eye.cbvArena.reset();Fail(950+code);
        for(auto& row:rows)if(!row[8]){row[8]=3;row[9]=code;}};
    const auto family=families::Lookup(reinterpret_cast<ID3D12PipelineState*>(sourceRow[10]));
    if(eye.cbvArenaAborted || work->mode!=1 || sourceRow[44] || !family || family->sourceRoot.Get()!=source.root.Get() ||
        !source.shader || work->cbvFailed || work->tablesFailed || !work->sourceTables || work->tableCommands.empty() ||
        work->tableCommands.back()[0]!=sourceRow[0] || !work->tableCommands.back()[2])reject(1);
    else{
        rows.reserve(family->uses.size());requests.reserve(source_cbv_snapshot::detail::MaxRequests);
        bool current=true;
        for(const auto& use:family->uses)if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV){
            std::array<uint64_t,32> row{};row[0]=sourceRow[0];row[1]=side;row[2]=use.visibility;row[3]=use.shaderRegister;row[6]=use.bytes;
            ++command[6];UINT parameter{},offset{};
            const auto stage=use.visibility==D3D12_SHADER_VISIBILITY_VERTEX?0u:use.visibility==D3D12_SHADER_VISIBILITY_PIXEL?1u:UINT_MAX;
            const auto found=std::find_if(source.cbvs.begin(),source.cbvs.end(),[&](const auto& cbv){return cbv.stage==stage && cbv.shaderRegister==use.shaderRegister;});
            if(use.count!=1 || found==source.cbvs.end() || !descriptors::FindBinding(sourceRow[11],D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
                use.space,use.shaderRegister,D3D12_SHADER_VISIBILITY(use.visibility),parameter,offset)){
                row[8]=3;row[9]=2;current=false;rows.push_back(row);continue;
            }
            const auto& cbv=*found;row[4]=parameter;row[5]=offset;row[7]=cbv.descriptorBytes;
            row[12]=reinterpret_cast<uintptr_t>(cbv.resource.Get());row[13]=cbv.stamp.value[4];row[14]=cbv.sourceOffset;
            row[15]=cbv.native.kind;row[16]=cbv.native.handle;row[23]=cbv.stamp.gpu;row[24]=cbv.stamp.heap;
            row[25]=cbv.recording.generation;row[26]=cbv.recording.context;row[27]=cbv.cookie;row[28]=cbv.origin;
            D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};constants::ResourceCreationInfo creation;
            if(!cbv.resource || FAILED(cbv.resource->GetHeapProperties(&heap,&flags))){
                row[8]=3;row[9]=3;current=false;rows.push_back(row);continue;
            }
            const bool creationKnown=constants::QueryResourceCreation(cbv.resource,creation);row[10]=UINT(heap.Type);row[11]=creation.kind;
            const source_cbv_snapshot::ReadEpochCertificate originalEpoch{reinterpret_cast<uintptr_t>(list),eye.recording.generation,command[14],true};
            const bool inlineUpload=heap.Type==D3D12_HEAP_TYPE_UPLOAD && CyberpunkVR_SourceCbvSnapshotsUploadEnable.load(std::memory_order_relaxed) &&
                source_cbv_snapshot::UploadCompatible() && source_cbv_snapshot::detail::InlineUpload(cbv,originalEpoch,use.bytes);
            if((heap.Type==D3D12_HEAP_TYPE_UPLOAD || heap.Type==D3D12_HEAP_TYPE_GPU_UPLOAD) && !inlineUpload){
                row[8]=2;++command[5];rows.push_back(row);continue;
            }
            source_cbv_details::Detail detail;
            const bool observed=creationKnown && (inlineUpload || (heap.Type==D3D12_HEAP_TYPE_DEFAULT && heap.CPUPageProperty!=D3D12_CPU_PAGE_PROPERTY_WRITE_BACK &&
                heap.CPUPageProperty!=D3D12_CPU_PAGE_PROPERTY_WRITE_COMBINE)) && source_cbv_details::CaptureAtSource(list,sourceRow[11],parameter,offset,cbv,detail);
            const bool readOnly=observed && detail.context.observation==source_cbv_details::Observation::Known && !detail.context.pending &&
                !detail.context.submitted && detail.stateQueried && detail.stateValueKnown &&
                ((inlineUpload && detail.state.status==1 && detail.state.value==D3D12_RESOURCE_STATE_GENERIC_READ) ||
                 (cbv.native.kind==1 && detail.state.kind==1 && (detail.state.status==8 || detail.state.status==9) &&
                    detail.state.value==D3D12_RESOURCE_STATE_GENERIC_READ) || source_cbv_snapshot::detail::ReadMask(detail.state));
            const camera_upload::Context context{cbv.recording.context,detail.context.state,cbv.recording.list};
            if(!readOnly || cbv.recording.list!=reinterpret_cast<uintptr_t>(list) || cbv.recording.generation!=eye.recording.generation ||
                !native_recording::ValidateRecording(cbv.recording,list,context)){row[8]=3;row[9]=4;current=false;rows.push_back(row);continue;}
            requests.push_back({cbv,parameter,offset,use.bytes});++command[4];rows.push_back(row);
        }
        if(!current)reject(5);
        else if(requests.empty())command[2]=2;
        else if(!preparation::RevalidateGraphParent(source.parent))reject(6);
        else{
            if(!eye.cbvArena){eye.cbvArena=std::make_unique<source_cbv_snapshot::SnapshotArena>();
                if(FAILED(eye.cbvArena->Create(work->device.Get(),list,eye.recording.generation,8*1024*1024)))reject(7);}
            if(!command[3]){
                // The inline direct route stays in the same valid original
                // queued epoch, after its bounded producer and before its draw.
                source_cbv_snapshot::ActualCallScope scope(list,eye.recording.generation,command[14]);
                const source_cbv_snapshot::ReadEpochCertificate epoch{reinterpret_cast<uintptr_t>(list),eye.recording.generation,command[14],true};
                if(!scope.Active() || !source_cbv_snapshot::PrepareAtActualCall(list,sourceRow[11],requests,source.parent,epoch,*eye.cbvArena,source.cbvPrepared))
                    reject(100+UINT(source_cbv_snapshot::LastFailure()));
                else{
                    command[15]=1;command[19]=1;
                    if(!source_cbv_snapshot::RecordAtActualCall(list,source.cbvPrepared,source.privateCbvs))reject(100+UINT(source_cbv_snapshot::LastFailure()));
                    else{
                        command[2]=1;command[16]=1;command[7]=source.privateCbvs.size();
                        for(const auto& captured:source.privateCbvs){
                            auto row=std::find_if(rows.begin(),rows.end(),[&](const auto& value){return value[4]==captured.parameter && value[5]==captured.offset && !value[8];});
                            if(row==rows.end())throw std::logic_error("Private CBV source row is missing");
                            (*row)[8]=1;(*row)[17]=reinterpret_cast<uintptr_t>(captured.resource.Get());(*row)[18]=captured.view.BufferLocation;
                            (*row)[19]=captured.privateOffset;(*row)[20]=captured.copiedBytes;(*row)[21]=captured.paddedBytes;(*row)[22]=reinterpret_cast<uintptr_t>(captured.owner.Get());
                            command[8]+=captured.copiedBytes;command[9]+=captured.paddedBytes;command[10]=(*row)[17];
                        }
                    }
                }
            }
        }
    }
    work->cbvSnapshotRows.insert(work->cbvSnapshotRows.end(),rows.begin(),rows.end());work->cbvSnapshotCommands.push_back(command);
    work->cbvCopyOwners.push_back({command[0],command[1],command[11],command[12],command[14],
        command[2]==1?reinterpret_cast<uintptr_t>(source.cbvPrepared.owner.Get()):0,command[7],command[8]});
}
void CaptureT5Snapshot(ID3D12GraphicsCommandList* list,UINT side,Work::Source& source,const std::array<uint64_t,48>& sourceRow){
    if(!work->sourceSelectors || work->selectorCommands.empty() || !work->selectorCommands.back()[25])return;
    auto& command=work->selectorCommands.back();auto& eye=work->eyes[side];
    std::array<uint64_t,32> row{};row[0]=sourceRow[0];row[1]=side;row[4]=reinterpret_cast<uintptr_t>(source.multilayerT5.resource.Get());
    row[12]=source.multilayerT5.stamp.gpu;row[13]=source.multilayerT5.stamp.heap;row[14]=eye.recording.context;row[15]=eye.recording.generation;row[20]=sourceRow[0]+1;
    row[6]=source.t5Native.native.handle;row[7]=source.t5Native.native.kind;row[8]=source.t5Native.state.status;row[9]=source.t5Native.state.value;
    if(source.multilayerT5.resource)row[5]=source.multilayerT5.resource->GetGPUVirtualAddress();
    command[29]=source.t5Native.recording.token && source.t5Native.native.resource==row[4];
    row[10]=source.t5Plan.resourceOffset;row[11]=source.t5Plan.byteExtent;
    std::array<uint64_t,20> coverage{sourceRow[0],side};source_resource_intent::StructuredCoverageDiagnostic diagnostic;
    static_assert(sizeof(diagnostic)==16*sizeof(uint64_t));
    if(source.multilayerT5.resource && source.t5Plan.originalSiteCoverageComplete && source.t5Plan.byteExtent){
        coverage[2]=source_resource_intent::QueryStructuredCoverage(source.multilayerT5.resource,source.t5Plan.resourceOffset,source.t5Plan.byteExtent,diagnostic);
        std::memcpy(coverage.data()+4,&diagnostic,sizeof(diagnostic));
    }
    if(work->t5Coverage.size()<SourceAssemblyCommandLimit)work->t5Coverage.push_back(coverage);
    source.t5Row=work->t5SnapshotRows.size();command[28]=source.t5Row+1;
    if(source.t5Candidate){
        row[30]=source.t5Candidate.IsComposed();row[25]=row[30]?sourceRow[0]+1:source.t5Candidate.CopySequence();row[26]=work->t5IntentBlob.size()+1;
        work->t5IntentBlob.insert(work->t5IntentBlob.end(),source.t5Candidate.Bytes().begin(),source.t5Candidate.Bytes().end());}
    for(const auto& piece:source.t5Candidate.Pieces()){
        std::array<uint64_t,16> value{};value[0]=sourceRow[0];value[1]=side;value[2]=work->t5Pieces.size();value[3]=row[25];
        static_assert(sizeof(piece)==9*sizeof(uint64_t));std::memcpy(value.data()+4,&piece,sizeof(piece));work->t5Pieces.push_back(value);
    }
    const bool planned=command[2]==1 || (command[2]==3 && command[3]==2 && source.t5Plan.byteExtent);
    if(!planned || eye.t5Aborted || sourceRow[44] || !command[29]){row[2]=3;row[3]=UINT(structured_buffer_snapshot::Failure::Source);}
    else if(!row[11])row[2]=2;
    else{
        const auto reject=[&](UINT error,bool fatal=false){row[2]=3;row[3]=error;
            if(fatal){eye.t5Aborted=true;if(eye.t5Arena)eye.t5Arena->Abort();Fail(1350+error);}};
        if(!eye.t5Arena){eye.t5Arena=std::make_unique<structured_buffer_snapshot::SnapshotArena>();
            if(FAILED(eye.t5Arena->Create(work->device.Get(),list,eye.recording.generation,8*1024*1024,structured_buffer_snapshot::Layout::OriginalIndices)))
                reject(UINT(structured_buffer_snapshot::Failure::Capacity));}
        if(!row[3]){
            structured_buffer_snapshot::ActualCallScope scope(list,eye.recording.generation,row[20]);
            const structured_buffer_snapshot::ReadEpochCertificate epoch{sourceRow[6],eye.recording.generation,row[20],true};
            const structured_buffer_snapshot::Request request{source.t5Native,23,5,row[10],UINT(row[11])};
            if(!scope.Active() || !structured_buffer_snapshot::PrepareAtActualCall(list,sourceRow[11],request,source.parent,epoch,*eye.t5Arena,source.t5Prepared))
                reject(UINT(structured_buffer_snapshot::LastFailure()));
            else{row[21]=1;row[29]=1;
                if(!structured_buffer_snapshot::RecordAtActualCall(list,source.t5Prepared,source.t5Private)){
                    const auto failure=structured_buffer_snapshot::LastFailure();
                    eye.t5Aborted=true;eye.t5Arena->Abort();
                    reject(UINT(failure),failure==structured_buffer_snapshot::Failure::RecoveryFatal);
                }else{
                    row[2]=1;row[22]=1;row[16]=reinterpret_cast<uintptr_t>(source.t5Private.resource.Get());row[17]=source.t5Private.privateOffset;
                    row[18]=source.t5Private.copiedBytes;row[19]=reinterpret_cast<uintptr_t>(source.t5Private.owner.Get());command[26]=1;
                }
            }
        }
    }
    work->t5SnapshotRows.push_back(row);
}
bool FullT5Readback(ID3D12GraphicsCommandList* list,uint64_t bytes,ComPtr<ID3D12Resource>& output){
    if(!bytes || bytes>128ull*1024*1024)return false;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;probe::Scope internal;
    return SUCCEEDED(work->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,
        IID_PPV_ARGS(&output))) && gpu::KeepCommandResources(list,{output.Get()});
}
void CaptureFullT5(ID3D12GraphicsCommandList* list,UINT side,Work::Source& source,const std::array<uint64_t,48>& saved){
    if(!work->fullT5Enabled || saved[4]!=9 || work->failed)return;
    auto& eye=work->eyes[side];auto* stats=CyberpunkVR_RetainedFullT5Stats;
    constexpr UINT bytes=UINT(structured_buffer_snapshot::detail::SourceBytes);
    const auto reject=[&](UINT why){if(!stats[11])stats[11]=why;if(eye.fullT5Arena)eye.fullT5Arena->Abort();Fail(2700+why);};
    if(!source.t5OriginalCertified || !source.t5Native.recording.token || saved[44] || saved[2] ||
        eye.fullT5ReferenceBytes>128ull*1024*1024-bytes){reject(1);return;}
    if(!eye.fullT5Arena){
        eye.fullT5Arena=std::make_unique<structured_buffer_snapshot::SnapshotArena>();
        if(FAILED(eye.fullT5Arena->Create(work->device.Get(),list,eye.recording.generation,8*1024*1024,
            structured_buffer_snapshot::Layout::OriginalIndices)) || !FullT5Readback(list,128ull*1024*1024,eye.fullT5OriginalReadback)){
            reject(2);return;
        }
    }
    structured_buffer_snapshot::ActualCallScope scope(list,eye.recording.generation,saved[0]+1);
    const structured_buffer_snapshot::ReadEpochCertificate epoch{saved[6],eye.recording.generation,saved[0]+1,true};
    const structured_buffer_snapshot::Request request{source.t5Native,23,5,0,bytes,structured_buffer_snapshot::Extent::FullOriginalView};
    if(!scope.Active() || !structured_buffer_snapshot::PrepareAtActualCall(list,saved[11],request,source.parent,epoch,*eye.fullT5Arena,source.fullT5Prepared) ||
        !structured_buffer_snapshot::RecordAtActualCall(list,source.fullT5Prepared,source.fullT5Private) ||
        !structured_buffer_snapshot::ValidateAtActualCall(list,source.fullT5Prepared)){
        reject(100+UINT(structured_buffer_snapshot::LastFailure()));return;
    }
    D3D12_SHADER_RESOURCE_VIEW_DESC view{};
    if(!structured_buffer_snapshot::RecordedView(source.fullT5Prepared,source.fullT5Private,view)){
        reject(3);return;
    }
    // Independent full source read at every occurrence verifies reuse as well
    // as first copies. No predicted selector bytes enter this comparison.
    {probe::Scope internal;list->CopyBufferRegion(eye.fullT5OriginalReadback.Get(),eye.fullT5ReferenceBytes,
        source.multilayerT5.resource.Get(),0,bytes);}
    const auto& proof=source.fullT5Prepared;const auto& copy=source.fullT5Private;
    work->fullT5Rows.push_back({saved[0],side,work->frame,saved[6],eye.recording.generation,saved[0]+1,
        reinterpret_cast<uintptr_t>(source.multilayerT5.resource.Get()),reinterpret_cast<uintptr_t>(copy.resource.Get()),
        copy.privateOffset,copy.copiedBytes,proof.reused,proof.gpuCopiedBytes,eye.fullT5ReferenceBytes,
        view.Buffer.FirstElement,0,UINT64_MAX});
    eye.fullT5ReferenceBytes+=bytes;stats[2]=work->frame;++stats[3];++stats[proof.reused?5:4];
    stats[6]+=proof.gpuCopiedBytes;stats[12+side]=eye.fullT5Arena->Bytes();stats[14]+=bytes;
}
void BorrowedTableFailure(std::array<uint64_t,16>& row,UINT code){
    if(!row[14])row[14]=code;auto* stats=CyberpunkVR_RetainedBorrowedTablesStats;
    if(!stats[13]){stats[12]=row[0]+1;stats[13]=code;
        auto* detail=CyberpunkVR_RetainedBorrowedFailure;detail[0]=row[0];detail[1]=code;
        if(code>=100 && code<200){detail[2]=descriptors::BorrowedRangeLastFailure();
            const auto observed=descriptors::BorrowedRangeLastDetails();std::copy(observed.begin(),observed.end(),detail+3);}
    }
}
void CaptureFixedAssets(ID3D12GraphicsCommandList* list,UINT side,Work::Source& source,const std::array<uint64_t,48>& saved,
                        UINT rangeIndex,uint64_t rangeRow,const descriptors::BorrowedRangeSummary::Range& range)try{
    if(!work->fixedAssetsEnabled)return;auto* stats=CyberpunkVR_RetainedFixedAssetsStats;
    stats[2]=work->frame;++stats[3];
    auto& row=work->fixedRangeRows.emplace_back(std::array<uint64_t,24>{saved[0],side,work->frame,rangeRow,
        range.heap,range.gpu,range.count,range.version});
    const auto refused=[&](UINT code){row[17]=code;++stats[11];};
    if(work->fixedRangeRows.size()>4 || range.count>65536 || range.count>262144-work->fixedSlotRows.size()){
        refused(1);return;
    }
    descriptors::BorrowedFixedRange captured;descriptors::BorrowedFixedRangeSummary summary;
    if(!descriptors::CaptureBorrowedFixedRange(list,work->eyes[side].recording,source.borrowedRanges,rangeIndex,captured) ||
        !descriptors::DescribeBorrowedFixedRange(captured,summary)){
        refused(100+descriptors::BorrowedRangeLastFailure());return;
    }
    if(summary.list!=saved[6] || summary.generation!=saved[7] || summary.root!=saved[11] || summary.pipeline!=saved[10] ||
        summary.range.heap!=range.heap || summary.range.gpu!=range.gpu || summary.range.count!=range.count || summary.range.version!=range.version ||
        summary.slots.size()!=range.count || summary.assets.size()>65536-work->fixedAssetRows.size()){
        refused(2);return;
    }
    const auto captureIndex=work->fixedRangeRows.size()-1;
    row[10]=work->fixedAssetRows.size();row[11]=summary.assets.size();row[12]=work->fixedSlotRows.size();row[13]=summary.slots.size();
    row[14]=summary.root;row[15]=summary.pipeline;row[16]=summary.family;
    for(size_t index=0;index<summary.assets.size();++index){const auto& asset=summary.assets[index];
        auto& value=work->fixedAssetRows.emplace_back(std::array<uint64_t,16>{captureIndex,index,asset.cookie,asset.candidate,
            asset.actualCookie,UINT(asset.status),asset.stage,asset.failure});
        std::copy(asset.pinDetails.begin(),asset.pinDetails.end(),value.begin()+8);
        if(asset.status==descriptors::BorrowedFixedStatus::Immutable)++stats[9];else ++stats[10];
    }
    for(const auto& slot:summary.slots)work->fixedSlotRows.push_back({captureIndex,slot.offset,slot.asset,slot.cookie,slot.candidate,UINT(slot.status),0,0});
    for(const auto& sample:summary.samples){auto& diagnostic=work->fixedDescriptorRows.emplace_back(
        std::array<uint64_t,24>{captureIndex,sample.offset,UINT(sample.status),sample.asset});
        std::copy(sample.canonical.begin(),sample.canonical.end(),diagnostic.begin()+4);
    }
    for(size_t index=0;index<summary.counts.size();++index)CyberpunkVR_RetainedFixedAssetsCounts[index]+=summary.counts[index];
    row[8]=1;row[9]=summary.allSlotsQualified;++stats[4];stats[5]+=summary.allSlotsQualified;
    stats[6]+=row[13];stats[7]+=row[11];stats[8]+=summary.counts[UINT(descriptors::BorrowedFixedStatus::Immutable)];
    work->fixedAssets.push_back(std::move(captured));
}catch(...){auto* stats=CyberpunkVR_RetainedFixedAssetsStats;++stats[11];if(!work->fixedRangeRows.empty())work->fixedRangeRows.back()[17]=3;}
void CaptureBorrowedTables(ID3D12GraphicsCommandList* list,UINT side,Work::Source& source,const std::array<uint64_t,48>& saved)try{
    if(!work->borrowedTablesEnabled || saved[4]!=9 || work->failed)return;
    auto* stats=CyberpunkVR_RetainedBorrowedTablesStats;stats[2]=work->frame;++stats[3];
    auto& row=work->borrowedTableRows.emplace_back(std::array<uint64_t,16>{saved[0],side,work->frame,saved[6],saved[7],saved[11],saved[10]});
    if(saved[44] || saved[2]){++stats[9];BorrowedTableFailure(row,7);return;}
    if(!descriptors::CaptureBorrowedSourceRanges(list,work->eyes[side].recording,source.bindings,source.borrowedRanges)){
        ++stats[9];BorrowedTableFailure(row,100+descriptors::BorrowedRangeLastFailure());return;
    }
    descriptors::BorrowedRangeSummary summary;
    if(!descriptors::DescribeBorrowedSourceRanges(source.borrowedRanges,summary) || summary.list!=saved[6] || summary.generation!=saved[7] ||
        summary.root!=saved[11] || summary.pipeline!=saved[10] || !summary.family || summary.ranges.empty() ||
        summary.ranges.size()>16384-work->borrowedRangeRows.size()){
        ++stats[9];BorrowedTableFailure(row,2);return;
    }
    row[7]=summary.family;row[8]=work->borrowedRangeRows.size();row[9]=summary.ranges.size();
    for(const auto& range:summary.ranges){
        work->borrowedRangeRows.push_back({saved[0],range.parameter,range.type,range.firstOffset,range.count,range.stride,
            range.shaderDeclaredCount,range.table,range.gpu,range.heap,range.version,range.physicalRemainder});
        row[10]+=range.count;
    }
    for(UINT index=0;index<summary.ranges.size();++index){const auto& range=summary.ranges[index];if(!range.physicalRemainder)continue;
        const auto key=std::make_tuple(range.heap,range.gpu,uint64_t(range.count),range.version);
        if(work->borrowedInventories.contains(key))continue;
        descriptors::BorrowedRangeInventory inventory;
        if(work->borrowedInventoryRows.size()>=256 || !descriptors::InventoryBorrowedSourceRange(source.borrowedRanges,index,inventory) ||
            inventory.list!=saved[6] || inventory.generation!=saved[7] || inventory.root!=saved[11] || inventory.pipeline!=saved[10] ||
            inventory.range.heap!=range.heap || inventory.range.gpu!=range.gpu || inventory.range.count!=range.count || inventory.range.version!=range.version){
            ++stats[9];BorrowedTableFailure(row,100+descriptors::BorrowedRangeLastFailure());return;
        }
        const auto& counts=inventory.counts;
        work->borrowedInventories.emplace(key,work->borrowedInventoryRows.size());
        // One metadata observation per unchanged range. Weak tracked-cookie
        // counts describe this occurrence only; they are not retained resources.
        work->borrowedInventoryRows.push_back({saved[0],side,work->frame,row[8]+index,range.heap,range.gpu,range.count,range.version,
            counts.slots,counts.unknown,counts.knownNull,counts.resourceSRV,counts.otherKinds,counts.uniqueCookies,
            counts.trackedCookies,counts.untrackedCookies,counts.texture2D,counts.texture2DArray,counts.flagsRTDSUAV,
            counts.shapeUnknown,0,0,0,0});
        CaptureFixedAssets(list,side,source,saved,index,row[8]+index,range);
    }
    row[11]=1;++stats[4];stats[5]+=row[9];stats[6]+=row[10];
}catch(...){auto* stats=CyberpunkVR_RetainedBorrowedTablesStats;++stats[9];if(!stats[13]){stats[12]=saved[0]+1;stats[13]=3;}}
void ValidateBorrowedTables(bool afterPrefix){
    if(!work->borrowedTablesEnabled)return;auto* stats=CyberpunkVR_RetainedBorrowedTablesStats;
    const bool scopeValid=queue_residency::Validate(work->borrowedTableScope);
    for(auto& row:work->borrowedTableRows){
        const bool joined=scopeValid && row[11] && row[0]<work->sources.size() &&
            (afterPrefix?row[12]!=0:queue_residency::ContainsRecording(work->borrowedTableScope,work->eyes[size_t(row[1])].recording));
        const bool valid=joined && descriptors::ValidateBorrowedSourceRanges(work->sources[size_t(row[0])].borrowedRanges);
        row[afterPrefix?13:12]=valid;++stats[valid?(afterPrefix?8:7):(afterPrefix?11:10)];
        if(!valid)BorrowedTableFailure(row,joined?100+descriptors::BorrowedRangeLastFailure():(afterPrefix?5:4));
    }
}
void ObserveBorrowedPageables(bool afterPrefix){
    if(!work->borrowedTablesEnabled)return;auto* stats=CyberpunkVR_RetainedPageableUseStats;stats[2]=work->frame;
    const auto failure=[&](std::array<uint64_t,16>& row){
        row[13]=queue_residency::LastFailure();const auto details=queue_residency::LastDetails();row[14]=details[8];
        if(!stats[11]){stats[11]=row[13];stats[12]=row[14];stats[13]=row[0]+1;
            std::copy(details.begin(),details.end(),CyberpunkVR_RetainedPageableUseFailure);
            const auto member=queue_residency::MembershipDetails();std::copy(member.begin(),member.end(),CyberpunkVR_RetainedPageableUseFailure+12);
            const auto search=queue_residency::PageableSearchDetails();std::copy(search.begin(),search.end(),CyberpunkVR_RetainedPageableSearch);}
    };
    if(afterPrefix){
        for(size_t i=0;i<work->borrowedPageableRows.size();++i){auto& row=work->borrowedPageableRows[i];if(!row[11])continue;
            row[12]=queue_residency::ValidatePageableUse(work->borrowedPageables[i]);++stats[row[12]?5:7];
            if(!row[12])failure(row);
        }
        return;
    }
    std::set<std::tuple<UINT,uint64_t,uint32_t,uint64_t>> seen;
    for(const auto& borrowed:work->borrowedTableRows){const auto ordinal=size_t(borrowed[0]);if(ordinal>=work->sources.size())continue;
        const auto& source=work->sources[ordinal];if(!source.stagedPair)continue;const UINT side=UINT(borrowed[1]);
        for(const auto& material:source.stagedPair->materials)if(material.immutable.owner){const auto& native=material.immutable.source.native;
            if(!seen.emplace(side,native.registry,native.handle,native.resource).second)continue;
            if(work->borrowedPageableRows.size()>=2048){stats[10]=1;return;}
            queue_residency::PendingPageableUse receipt;
            std::array<uint64_t,16> row{ordinal,side,work->frame,native.handle,native.resource,work->eyes[side].recording.list};
            row[11]=queue_residency::BindPageableUse(work->borrowedTableScope,work->eyes[side].recording,material.immutable,receipt);
            ++stats[3];++stats[row[11]?4:6];
            if(row[11]){queue_residency::PageableUseDescription description;
                if(queue_residency::DescribePageableUse(receipt,description)){
                    row[6]=description.pageable;row[7]=description.managedEntry;row[8]=description.memberIndex;
                    row[9]=description.sourceSet;row[10]=description.pageableKind;++stats[description.pageableKind==2?9:8];
                }
            }else failure(row);
            work->borrowedPageables.push_back(std::move(receipt));work->borrowedPageableRows.push_back(row);
        }
    }
}
void CaptureInstanceWriter(ID3D12GraphicsCommandList* list,UINT side,Work::Source& source,
    const std::array<uint64_t,48>& sourceRow){
    if(!work->sourceIaSnapshots)return;
    const auto& geometry=source.command.geometry;const auto& view=geometry.draw.vertices[7];
    std::array<uint64_t,48> row{sourceRow[0],side,sourceRow[2]};
    row[8]=sourceRow[10];row[9]=sourceRow[12];row[10]=reinterpret_cast<uintptr_t>(geometry.vertices[7].resource.Get());
    row[11]=geometry.vertices[7].gpu;row[12]=view.BufferLocation;row[13]=view.SizeInBytes;row[14]=view.StrideInBytes;
    // Indirect firstInstance/count remain unknown until their private arguments
    // are decoded. A live binding or historical numeric draw is not that proof.
    if(!sourceRow[2] && !sourceRow[44] && (geometry.draw.inputSlotMask&128) && source.shader &&
        instance_snapshot_view::detail::Layout(source.shader->input,view.StrideInBytes)){
        row[3]=1;
        const bool captured=instance_observer::QueryAtSource(list,geometry,true,work->eyes[side].recording,source.parent,
            sourceRow[0]+1,source.instanceCandidate);
        const auto observed=instance_observer::LastQuery();std::copy(observed.values.begin(),observed.values.end(),row.begin()+16);
        if(captured && source.instanceCandidate.owner && source.instanceCandidate.cpu){
            const auto bytes=source.instanceCandidate.cpu.Bytes();
            if(bytes.empty() || bytes.size()>16*1024*1024-work->instanceBlob.size())throw std::logic_error("Instance CPU payload exceeds its bounded capture");
            row[4]=1;row[5]=reinterpret_cast<uintptr_t>(source.instanceCandidate.owner.Get());row[6]=work->instanceBlob.size()+1;row[7]=bytes.size();
            work->instanceBlob.insert(work->instanceBlob.end(),bytes.begin(),bytes.end());
        }
    }
    std::array<uint64_t,24> probeRow{sourceRow[0],side};
    if(row[4]){auto& eye=work->eyes[side];
        CyberpunkVR_RetainedInstanceGpuPhase=1;
        if(!eye.instanceReadback){
            D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=16*1024*1024;
            desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
            D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;probe::Scope internal;
            if(FAILED(work->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,
                IID_PPV_ARGS(&eye.instanceReadback))))throw std::runtime_error("Instance readback allocation failed");
            CyberpunkVR_RetainedInstanceGpuPhase=2;heap.Type=D3D12_HEAP_TYPE_UPLOAD;desc.Width=2048*8;
            if(FAILED(work->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,
                IID_PPV_ARGS(&eye.instancePredicate))))throw std::runtime_error("Instance predicate allocation failed");
            CyberpunkVR_RetainedInstanceGpuPhase=3;void* mapped{};const D3D12_RANGE none{};
            if(FAILED(eye.instancePredicate->Map(0,&none,&mapped)) || !mapped)throw std::runtime_error("Instance predicate initialization failed");
            std::memset(mapped,0,2048*8);const D3D12_RANGE written{0,2048*8};eye.instancePredicate->Unmap(0,&written);
        }
        const auto offset=eye.instanceReadbackBytes;const auto predicateOffset=sourceRow[0]*8;
        CyberpunkVR_RetainedInstanceGpuPhase=4;
        if(row[7]>16*1024*1024-offset)throw std::length_error("Instance GPU capture budget exceeded");
        if(!eye.instanceCapture){eye.instanceCapture=std::make_unique<instance_snapshot_view::GpuCaptureArena>();
            if(FAILED(eye.instanceCapture->Create(work->device.Get(),eye.recording,side)))throw std::runtime_error("Instance GPU capture allocation failed");}
        instance_snapshot_view::Input input;input.scope={sourceRow[11],sourceRow[6],sourceRow[7],sourceRow[0]+1,side};
        input.pipeline=geometry.state.original;input.shader=source.shader;input.original=view;
        input.draw={geometry.draw.indices,geometry.draw.instances,geometry.draw.firstIndex,geometry.draw.baseVertex,geometry.draw.firstInstance};
        struct CopyContext {const instance_observer::SourceCandidate& source;ID3D12Resource* readback;uint64_t offset;ID3D12Resource* predicate;uint64_t predicateOffset;};
        CopyContext context{source.instanceCandidate,eye.instanceReadback.Get(),offset,eye.instancePredicate.Get(),predicateOffset};
        const auto record=[](void* opaque,ID3D12GraphicsCommandList* commands,const instance_snapshot_view::Input& actual,ID3D12Resource* target,uint64_t at){
            const auto& args=*static_cast<CopyContext*>(opaque);
            return instance_observer::RecordGpuCapture(commands,args.source,actual,target,at,args.readback,args.offset,args.predicate,args.predicateOffset);};
        instance_snapshot_view::PrivateView captured;
        if(eye.instanceCapture->Capture(list,input,record,&context,captured)){
            source.instanceView=std::move(captured);
            probeRow[2]=1;probeRow[6]=reinterpret_cast<uintptr_t>(eye.instanceReadback.Get());probeRow[7]=reinterpret_cast<uintptr_t>(eye.instancePredicate.Get());
            probeRow[8]=row[10];probeRow[9]=row[12]-row[11]+sourceRow[24]*row[14];probeRow[10]=offset;probeRow[11]=row[7];probeRow[12]=predicateOffset;
            probeRow[13]=sourceRow[6];probeRow[14]=sourceRow[7];probeRow[15]=sourceRow[0]+1;probeRow[16]=row[5];probeRow[17]=row[6];
            eye.instanceReadbackBytes+=row[7];
        }else throw std::runtime_error("Original-position instance GPU capture failed");
        CyberpunkVR_RetainedInstanceGpuPhase=5;
    }
    work->instanceRows.push_back(row);work->instanceGpuRows.push_back(probeRow);
}
void CaptureIaSnapshots(ID3D12GraphicsCommandList* list,UINT side,Work::Source& source,
    const std::array<uint64_t,48>& sourceRow,bool indexed){
    if(!work->sourceIaSnapshots)return;
    auto& eye=work->eyes[side];std::array<uint64_t,16> command{sourceRow[0],side};
    const UINT separate=sourceRow[0]<work->instanceGpuRows.size() && work->instanceGpuRows[size_t(sourceRow[0])][2]?128u:0u;
    const UINT copiedMask=source.command.geometry.draw.inputSlotMask&~separate;
    command[10]=sourceRow[6];command[11]=sourceRow[7];command[12]=sourceRow[0]+1;
    ia_snapshot::Observations observations;ia_snapshot::Failure error=ia_snapshot::Failure::None;
    try{
    work->iaCommands.reserve(work->iaCommands.size()+1);
    work->iaObservations.reserve(work->iaObservations.size()+ia_snapshot::Observations::MaxViews);
    work->iaViews.reserve(work->iaViews.size()+ia_snapshot::Observations::MaxViews);
    work->iaReuse.reserve(work->iaReuse.size()+ia_snapshot::Observations::MaxViews);work->iaTransfers.reserve(work->iaTransfers.size()+1);
    ia_snapshot::ActualCallScope scope(list,eye.recording.generation,command[12]);
    const ia_snapshot::ReadEpochCertificate epoch{sourceRow[6],sourceRow[7],command[12],true,separate};
    command[2]=ia_snapshot::ObserveAtActualCall(list,source.command.geometry,indexed,eye.recording,source.parent,epoch,observations);
    error=ia_snapshot::LastFailure();
    if(observations.count>ia_snapshot::Observations::MaxViews)throw std::logic_error("IA observation count exceeds its bounded set");
    if(!command[2]){if(error==ia_snapshot::Failure::None)error=ia_snapshot::Failure::Arguments;}
    else if(sourceRow[44] || work->mode!=1)error=ia_snapshot::Failure::CurrentIa;
    else if(eye.iaAborted)error=ia_snapshot::Failure::Changed;
    else if(!observations.count)error=ia_snapshot::Failure::Empty;
    else if(observations.admitted==observations.count && observations.first==ia_snapshot::Failure::None){
        if(!eye.iaArena){auto arena=std::make_unique<ia_snapshot::SnapshotArena>();
            if(FAILED(arena->Create(work->device.Get(),list,eye.recording.generation,ia_snapshot::detail::MaxArenaBytes)))error=ia_snapshot::Failure::Capacity;
            else eye.iaArena=std::move(arena);}
        if(error==ia_snapshot::Failure::None){
            const auto initial=observations;
            command[5]=ia_snapshot::PrepareAtActualCall(list,source.command.geometry,indexed,eye.recording,source.parent,epoch,
                *eye.iaArena,source.iaPrepared,observations);
            error=ia_snapshot::LastFailure();
            if(!command[5] && !observations.count)observations=initial;
            if(observations.count>ia_snapshot::Observations::MaxViews)throw std::logic_error("Prepared IA observation count exceeds its bounded set");
            if(command[5]){
                command[13]=reinterpret_cast<uintptr_t>(source.iaPrepared.owner.Get());
                // Allocate the replacement before any inserted GPU copy. These
                // original draw arguments and full view extents stay unchanged.
                auto replacement=source.command.geometry;
                if(!ia_snapshot::RecordAtActualCall(list,source.iaPrepared,source.iaViews)){
                    error=ia_snapshot::LastFailure();eye.iaAborted=true;eye.iaArena->Abort();Fail(1700+UINT(error));
                }else{
                    command[6]=1;command[8]=source.iaViews.size();command[9]=source.iaPrepared.copiedBytes;
                    if(source.iaViews.size()!=observations.count || source.iaViews.size()!=source.iaPrepared.count)
                        throw std::logic_error("Recorded IA set differs from its complete proof");
                    std::array<bool,17> seen{};
                    for(const auto& view:source.iaViews){
                        const bool index=view.role==ia_snapshot::Role::Index;
                        if(!view.resource || !view.owner || (index?(!indexed || view.slot!=UINT_MAX):view.role!=ia_snapshot::Role::Vertex || view.slot>=16 ||
                            !(replacement.draw.inputSlotMask&(1u<<view.slot))))throw std::logic_error("Private IA role is not consumed by this draw");
                        const UINT key=index?0:view.slot+1;if(seen[key])throw std::logic_error("Private IA role was copied twice");seen[key]=true;
                        const auto desc=view.resource->GetDesc();const auto gpu=view.resource->GetGPUVirtualAddress();
                        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || view.privateOffset>desc.Width ||
                            view.reservedBytes>desc.Width-view.privateOffset || !view.copiedBytes || view.copiedBytes>view.reservedBytes || gpu>UINT64_MAX-view.privateOffset)
                            throw std::logic_error("Private IA range differs from its owned buffer");
                        const geometry::BufferLease owned{view.resource,gpu,desc.Width,D3D12_HEAP_TYPE_DEFAULT,UINT(desc.Flags),{}};
                        if(index){const auto& original=replacement.draw.index;
                            if(view.index.SizeInBytes!=original.SizeInBytes || view.index.Format!=original.Format || view.copiedBytes!=original.SizeInBytes ||
                                view.index.BufferLocation!=gpu+view.privateOffset)throw std::logic_error("Private IB changed original coordinates");
                            replacement.index=owned;replacement.draw.index=view.index;
                        }else{const auto& original=replacement.draw.vertices[view.slot];
                            if(view.vertex.SizeInBytes!=original.SizeInBytes || view.vertex.StrideInBytes!=original.StrideInBytes || view.copiedBytes!=original.SizeInBytes ||
                                view.vertex.BufferLocation!=gpu+view.privateOffset)throw std::logic_error("Private VB changed original coordinates");
                            replacement.vertices[view.slot]=owned;replacement.draw.vertices[view.slot]=view.vertex;
                        }
                    }
                    if(seen[0]!=indexed)throw std::logic_error("Consumed IB was omitted");
                    for(UINT slot=0;slot<16;++slot)if(seen[slot+1]!=bool(copiedMask&(1u<<slot)))
                        throw std::logic_error("Consumed VB was omitted");
                    for(const auto& view:source.iaViews){const bool index=view.role==ia_snapshot::Role::Index;
                        work->iaViews.push_back({sourceRow[0],side,uint64_t(view.role),view.slot,reinterpret_cast<uintptr_t>(view.resource.Get()),
                            reinterpret_cast<uintptr_t>(view.owner.Get()),view.sourceOffset,view.privateOffset,view.copiedBytes,view.reservedBytes,
                            index?view.index.BufferLocation:view.vertex.BufferLocation,index?view.index.SizeInBytes:view.vertex.SizeInBytes,
                            index?UINT(view.index.Format):view.vertex.StrideInBytes,0,0,0});
                        work->iaReuse.push_back({sourceRow[0],side,UINT(view.role),view.slot,view.originOccurrence,view.reused,
                            view.privateOffset,view.copiedBytes});}
                    source.command.geometry=std::move(replacement);
                }
            }
        }
    }
    }catch(...){eye.iaAborted=true;if(eye.iaArena)eye.iaArena->Abort();Fail(1731);error=ia_snapshot::Failure::Exception;
        command[5]=source.iaPrepared.owner?1u:0u;command[6]=source.iaViews.empty()?0u:1u;command[8]=source.iaViews.size();
        command[9]=source.iaPrepared.copiedBytes;command[13]=reinterpret_cast<uintptr_t>(source.iaPrepared.owner.Get());}
    command[3]=observations.count;command[4]=observations.admitted;command[7]=UINT(error);
    static_assert(sizeof(ia_snapshot::Observation)==64*sizeof(uint64_t));
    try{for(UINT index=0;index<std::min(observations.count,ia_snapshot::Observations::MaxViews);++index){std::array<uint64_t,67> row{sourceRow[0],side,index};
            std::memcpy(row.data()+3,&observations.rows[index],sizeof(ia_snapshot::Observation));work->iaObservations.push_back(row);}
        work->iaCommands.push_back(command);
        work->iaTransfers.push_back({sourceRow[0],side,command[6]?source.iaPrepared.transferredBytes:0,
            command[6]?source.iaPrepared.reusedBytes:0,eye.iaArena?eye.iaArena->Bytes():0});
        work->iaSelections.push_back({sourceRow[0],side,separate,source.command.geometry.draw.inputSlotMask,copiedMask});
    }catch(...){eye.iaAborted=true;if(eye.iaArena)eye.iaArena->Abort();Fail(1731);}
}
void CaptureMaterialBuffer(ID3D12GraphicsCommandList* list,Work::Source& source,const std::array<uint64_t,48>& saved,
    const descriptors::SourceResourceBinding& binding){
    if(binding.parameter!=23 || (binding.offset!=4 && binding.offset!=6) || work->materialBufferRows.size()>=SourceAssemblyDetailLimit)return;
    std::array<uint64_t,40> row{saved[0],saved[1],binding.parameter,binding.offset,UINT(source_srv::Status::InvalidBinding),reinterpret_cast<uintptr_t>(binding.resource.Get())};
    row[6]=binding.resource->GetGPUVirtualAddress();row[7]=binding.resource->GetDesc().Width;row[11]=UINT(binding.resource->GetDesc().Flags);
    row[33]=saved[6];row[34]=saved[7];row[35]=saved[0]+1;
    D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};if(SUCCEEDED(binding.resource->GetHeapProperties(&heap,&flags)))row[10]=UINT(heap.Type);
    constants::ResourceCreationInfo creation;
    if(constants::QueryResourceCreation(binding.resource,creation)){row[24]=creation.kind;row[25]=reinterpret_cast<uintptr_t>(creation.heap.Get());row[26]=creation.heapOffset;row[27]=creation.allocationBytes;}
    descriptors::SrvBinding held;source_srv::SourceSrvBacking backing;
    if(descriptors::ResolveSrvAtDraw(list,saved[11],binding.parameter,binding.offset,held) && held.resource==binding.resource && held.stamp.value==binding.stamp.value){
        const auto& view=held.view;row[28]=UINT(view.Format);row[29]=view.Buffer.FirstElement;row[30]=view.Buffer.NumElements;
        row[31]=view.Buffer.StructureByteStride;row[32]=UINT(view.Buffer.Flags);
        row[4]=UINT(source_srv::ReadAtSource(list,saved[11],binding.parameter,binding.offset,held,work->eyes[size_t(saved[1])].recording,backing));
        if(row[4]==UINT(source_srv::Status::Captured)){
            row[8]=backing.sourceOffset;row[9]=backing.viewBytes;row[12]=backing.native.handle;row[13]=backing.native.kind;row[14]=backing.native.bytes;row[15]=backing.lowType;
            row[16]=backing.state.status;row[17]=backing.state.value;row[18]=UINT(backing.allocation.observation);row[19]=backing.allocation.address;
            row[20]=backing.allocation.kind;row[21]=backing.allocation.bytes;row[22]=UINT(backing.allocator.observation);row[23]=backing.allocator.address;
            row[36]=backing.recordAddress;row[37]=backing.cpuDescriptor;
        }
    }
    work->materialBufferRows.push_back(row);
    std::array<uint64_t,16> copied{saved[0],saved[1],binding.offset,row[4],UINT(material_buffer_snapshot::Failure::Native)};
    copied[13]=saved[6];copied[14]=saved[7];copied[15]=saved[0]+1;
    if(row[4]==UINT(source_srv::Status::Captured) && work->sourceSelectors){
        Work::Source::MaterialBuffer item{std::move(backing),{},work->materialBufferCopies.size()};auto& eye=work->eyes[size_t(saved[1])];
        if(!eye.materialArena){auto arena=std::make_unique<material_buffer_snapshot::Arena>();
            if(SUCCEEDED(arena->Create(work->device.Get(),list,saved[7])))eye.materialArena=std::move(arena);}
        if(eye.materialArena){material_buffer_snapshot::ActualCallScope scope(list,saved[7],saved[0]+1);
            const material_buffer_snapshot::Epoch epoch{saved[6],saved[7],saved[0]+1,true};
            if(scope.Active() && material_buffer_snapshot::Capture(list,saved[11],item.backing,source.parent,epoch,*eye.materialArena,item.copy)){
                copied[4]=0;copied[5]=reinterpret_cast<uintptr_t>(item.copy.owner.Get());copied[6]=reinterpret_cast<uintptr_t>(item.copy.resource.Get());
                copied[7]=item.copy.sourceOffset;copied[8]=item.copy.offset;copied[9]=item.copy.bytes;copied[10]=1;
            }else copied[4]=UINT(material_buffer_snapshot::LastFailure());
        }else copied[4]=UINT(material_buffer_snapshot::Failure::Capacity);
        source.materialBuffers.push_back(std::move(item));
    }
    work->materialBufferCopies.push_back(copied);
}
void CaptureMaterialResources(ID3D12GraphicsCommandList* list,Work::Source& source,const std::array<uint64_t,48>& saved){
    for(const auto* group:{&source.finiteDependencies,&source.sparseDependencies})for(const auto& binding:*group){
        if(work->materialRows.size()>=SourceAssemblyDetailLimit){source.materialTruncated=true;return;}
        std::array<uint64_t,24> row{saved[0],saved[1],binding.parameter,binding.offset,UINT(binding.type),
            reinterpret_cast<uintptr_t>(binding.resource.Get()),reinterpret_cast<uintptr_t>(binding.counter.Get()),binding.stamp.value[2]};
        row[14]=saved[6];row[15]=saved[7];row[16]=saved[0]+1;texture_read::ImmutableLease owned;
        if(binding.type!=D3D12_DESCRIPTOR_RANGE_TYPE_SRV)row[8]=3;
        else if(!binding.resource && !binding.counter && !binding.stamp.value[2])row[8]=1;
        else if(binding.resource){const auto desc=binding.resource->GetDesc();row[19]=UINT(desc.Dimension);row[20]=UINT(desc.Format);
            if(desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER){row[8]=2;CaptureMaterialBuffer(list,source,saved,binding);}
            else{row[8]=4;const auto found=work->materialCache.find(binding.resource.Get());
                if(found!=work->materialCache.end() && texture_read::ValidateImmutable(found->second))owned=found->second;
                else if(texture_read::AcquireImmutable(list,binding.resource.Get(),owned))work->materialCache[binding.resource.Get()]=owned;
                else row[9]=texture_read::LastFailure();
                if(owned.owner){row[8]=5;row[10]=reinterpret_cast<uintptr_t>(owned.owner.Get());row[11]=owned.source.native.handle;
                    row[12]=owned.source.receipt;row[13]=owned.source.owner;}
            }
        }else{row[8]=4;row[9]=95;}
        source.materialRows.push_back(work->materialRows.size());source.materialOwners.push_back(std::move(owned));
        work->materialRows.push_back(row);
    }
}
void FinalizeMaterialResources(){
    work->materialSourceRows.clear();
    for(size_t ordinal=0;ordinal<work->sources.size();++ordinal){auto& source=work->sources[ordinal];bool ready=source.bindingPlan.Ready() && !source.materialTruncated;
        for(size_t index=0;index<source.materialRows.size();++index){auto& row=work->materialRows[source.materialRows[index]];
            if(row[8]==1)row[18]=row[17];
            else if(row[8]==5)row[18]=row[17] && texture_read::ValidateImmutable(source.materialOwners[index],work->eyes[size_t(row[1])].sourceQueue.Get());
            else if(row[8]==2 && row[2]==23 && row[3]==5 && source.t5View && source.t5OriginalCertified &&
                work->overlayStage==2 && reinterpret_cast<uintptr_t>(source.multilayerT5.resource.Get())==row[5] && source.bindingPlan.Ready()){
                row[8]=6;row[10]=reinterpret_cast<uintptr_t>(source.t5View->owner.Get());row[18]=1;
            }
            else if(row[8]==2 && row[2]==23 && (row[3]==4 || row[3]==6)){
                const auto found=std::find_if(source.materialBuffers.begin(),source.materialBuffers.end(),[&](const auto& item){return item.backing.shaderRegister==row[3];});
                const auto* binding=index<source.finiteDependencies.size()?&source.finiteDependencies[index]:
                    index-source.finiteDependencies.size()<source.sparseDependencies.size()?&source.sparseDependencies[index-source.finiteDependencies.size()]:nullptr;
                if(found!=source.materialBuffers.end() && work->materialBufferCopies[found->row][12] && binding && material_buffer_snapshot::MatchesPrivate(found->copy,*binding)){
                    row[8]=7;row[10]=reinterpret_cast<uintptr_t>(found->copy.owner.Get());row[17]=row[18]=1;
                }
            }
            ready&=row[18]!=0;
        }
        source.materialInputsReady=ready;
        work->materialSourceRows.push_back({ordinal,work->sourceRows[ordinal][1],source.materialRows.size(),ready,source.materialTruncated});
    }
}
void FinalizeMaterialBufferCopies(){
    for(auto& source:work->sources)for(auto& item:source.materialBuffers)if(item.copy.owner){
        auto& row=work->materialBufferCopies[item.row];const auto& eye=work->eyes[size_t(row[1])];
        row[12]=row[11] && material_buffer_snapshot::Ready(item.copy,eye.recording,eye.sourceQueue.Get(),eye.fence.Get(),eye.signaledValue,eye.submitted && eye.submissionCertified);
    }
}
void AppendSource(ID3D12GraphicsCommandList* list,UINT side,const native_probe::DrawRecord& draw,const PureIndirectSource* indirect=nullptr,
    PureIndirectSource* replay=nullptr)try{
    if(!work->sourceStream)return;
    if(work->sources.size()>=StereoGeometryCommands::MaxSourceCommands){work->sourceFailed=true;CyberpunkVR_RetainedSourceStreamStats[12]=901;PublishSources();return;}
    Work::Source source;std::array<uint64_t,48> row{};const auto& lease=work->eyes[side].recording;
    const auto kind=indirect?indirect_policy::DrawKind(indirect->signature.Get()):0;
    row[0]=work->sourceRows.size();row[1]=side;row[2]=kind;row[3]=work->frame;
    const auto tag=preparation::CurrentDrawTag();row[4]=tag.stage;row[5]=tag.scope;
    row[6]=reinterpret_cast<uintptr_t>(list);row[7]=lease.generation;row[8]=lease.context;row[9]=lease.registry;
    row[10]=draw.pipeline;row[11]=draw.root;row[19]=draw.inputSlotMask;
    row[20]=draw.indices;row[21]=draw.instances;row[22]=draw.firstIndex;row[23]=uint32_t(draw.baseVertex);row[24]=draw.firstInstance;
    row[25]=draw.index.BufferLocation;row[26]=draw.index.SizeInBytes;row[27]=draw.index.Format;row[46]=draw.rtCount;
    const auto reject=[&](UINT reason){if(!row[44])row[44]=reason;};
    source.command.geometry.draw=draw;
    if(geometry::CopyCurrentBindings(list,draw,!indirect || kind==1,source.command.geometry)){
        row[43]|=1;row[28]=reinterpret_cast<uintptr_t>(source.command.geometry.index.resource.Get());
    }else reject(902);
    if(indirect){source.command.indirect=*indirect;row[29]=reinterpret_cast<uintptr_t>(indirect->signature.Get());
        row[30]=indirect_policy::PureDrawStride(indirect->signature.Get());row[31]=indirect->maxCommandCount;
        row[32]=reinterpret_cast<uintptr_t>(indirect->arguments.Get());row[33]=indirect->argumentOffset;
        row[34]=reinterpret_cast<uintptr_t>(indirect->countBuffer.Get());row[35]=indirect->countOffset;}
    if(indirect){void* addresses[32]{};const auto count=CaptureStackBackTrace(0,32,addresses,nullptr);
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        for(USHORT i=0;i<count;++i){const auto at=reinterpret_cast<uintptr_t>(addresses[i]);
            if(at>=base && at-base<0x3000000){row[45]=at-base;break;}}
    }
    if(indirect){
        std::array<uint64_t,56> evidence{};evidence[0]=row[0];evidence[1]=side;evidence[2]=row[45];
        evidence[14]=indirect->argumentOffset;evidence[20]=indirect->maxCommandCount;
        evidence[21]=lease.generation;evidence[22]=lease.context;evidence[23]=reinterpret_cast<uintptr_t>(list);
        evidence[25]=reinterpret_cast<uintptr_t>(indirect->arguments.Get());
        auto& bound=source.indirectBinding;
        if(geometry::BoundIndirect(list,indirect->signature.Get(),indirect->maxCommandCount,indirect->arguments.Get(),
            indirect->argumentOffset,indirect->countBuffer.Get(),indirect->countOffset,lease,bound)){
            const auto& buffer=bound.arguments;const auto& native=buffer.native;const auto desc=buffer.resource->GetDesc();
            evidence[2]=bound.callerRva;evidence[3]=1;evidence[4]=bound.recordAddress;evidence[5]=native.handle;evidence[6]=native.kind;
            evidence[7]=bound.state.value;evidence[8]=bound.state.status;evidence[9]=bound.state.kind;
            evidence[10]=native.gpu;evidence[11]=native.bytes;evidence[12]=buffer.resource->GetGPUVirtualAddress();evidence[13]=desc.Width;
            evidence[15]=bound.argumentBytes;evidence[16]=buffer.heapType;evidence[17]=buffer.flags;
            evidence[18]=bound.signatureKind;evidence[19]=bound.signatureStride;evidence[24]=native.registry;evidence[26]=bound.fullBacking;
            evidence[27]=geometry::RevalidateIndirect(list,lease,bound);
            std::memcpy(evidence.data()+32,bound.rawRecord.data(),bound.rawRecord.size());
        }
        if(!evidence[3] || !evidence[27]){
            evidence[28]=UINT(geometry::LastIndirectBindingFailure());
            if(evidence[28]==UINT(geometry::IndirectBindingFailure::Recording))evidence[29]=native_recording::LastFailure();
        }
        work->indirectRows.push_back(evidence);
    }
    const auto pipeline=reinterpret_cast<ID3D12PipelineState*>(draw.pipeline);
    if(scene_state::CaptureSourceBindings(list,source.bindings) && source.bindings.root==reinterpret_cast<ID3D12RootSignature*>(draw.root) &&
        scene_state::CaptureDrawState(list,pipeline,source.command.geometry.state)){
        source.root=source.bindings.root;source.computeRoot=source.bindings.computeRoot;
        for(UINT i=0;i<source.bindings.heapCount;++i)source.heaps[i]=source.bindings.heaps[i];
        row[43]|=2|16;row[13]=source.bindings.tableMask;row[14]=source.bindings.computeTableMask;
        row[15]=source.bindings.heapCount;row[16]=source.bindings.topology;row[17]=source.bindings.stencil;row[18]=source.bindings.rate.value;
    }else reject(903);
    if(preparation::CurrentGraphParent(int(side),work->frame,source.parent)){
        const auto& parent=source.parent;row[43]|=4;row[36]=parent.manager;row[37]=parent.fingerprint;
        row[38]=parent.handle;row[39]=parent.ordinal;row[40]=parent.viewIndex;row[41]=parent.slot;row[42]=parent.name;
    }else reject(904);
    source.shader=shader_snapshot::Capture(pipeline);
    if(source.shader && !source.shader->root.empty()){row[43]|=8;row[12]=source.shader->id;}else reject(905);
    if(work->sourceCbvs){
        const auto family=families::Lookup(pipeline);
        if(!family || family->sourceRoot.Get()!=source.root.Get()){work->cbvFailed=true;CyberpunkVR_RetainedSourceCbvStats[11]=UINT(source_cbv::Status::MissingObservation);}
        else for(const auto& use:family->uses)if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV){
            std::array<uint64_t,24> value{};value[0]=row[0];value[1]=side;value[2]=use.visibility;value[3]=use.shaderRegister;value[6]=use.bytes;
            std::array<uint64_t,40> producer{};std::copy_n(value.begin(),6,producer.begin());
            UINT parameter{},offset{};descriptors::DescriptorStamp stamp;ComPtr<IUnknown> origin;source_cbv::SourceCbvBacking backing;
            std::array<uint64_t,16> diagnostic{};
            auto status=source_cbv::Status::InvalidBinding;
            if(use.count==1 && descriptors::FindBinding(draw.root,D3D12_DESCRIPTOR_RANGE_TYPE_CBV,use.space,use.shaderRegister,
                D3D12_SHADER_VISIBILITY(use.visibility),parameter,offset)){
                value[4]=parameter;value[5]=offset;
                producer[4]=parameter;producer[5]=offset;
                if(descriptors::CaptureCbvSource(list,draw.root,parameter,offset,stamp,origin)){
                    status=source_cbv::ReadAtSource(list,draw.root,parameter,offset,stamp,origin.Get(),backing);
                    diagnostic=source_cbv::LastDiagnostic().values;
                }
            }
            value[7]=UINT(status);value[8]=stamp.gpu;value[9]=stamp.heap;value[10]=stamp.value[4];value[11]=stamp.value[5];
            if(backing.resource && (status==source_cbv::Status::Registered || status==source_cbv::Status::Transient || status==source_cbv::Status::Created)){
                value[12]=reinterpret_cast<uintptr_t>(backing.resource.Get());value[13]=backing.gpu;value[14]=backing.backingBytes;
                value[15]=backing.recording.context;value[16]=backing.recording.generation;value[17]=backing.cookie;value[18]=backing.origin;
                value[19]=backing.creatorRva;value[20]=backing.cpuDescriptor;value[21]=backing.native.handle;value[22]=backing.logicalBytes;value[23]=1;
                producer[6]=backing.logicalBytes;producer[7]=backing.reservedBytes;producer[8]=backing.completedProducerRva;producer[9]=backing.creatorRva;
                producer[10]=reinterpret_cast<uintptr_t>(backing.resource.Get());producer[11]=backing.stamp.value[4];
                const auto putLease=[&](UINT at,const native_recording::Lease& lease){producer[at]=reinterpret_cast<uintptr_t>(lease.token.Get());
                    producer[at+1]=lease.list;producer[at+2]=lease.generation;producer[at+3]=lease.context;producer[at+4]=lease.registry;};
                putLease(12,backing.recording);putLease(17,backing.creationRecording);
                const source_cbv_snapshot::ReadEpochCertificate epoch{row[6],row[7],row[0]+1,true};
                producer[22]=source_cbv_snapshot::detail::InlineDirectUpload(backing,epoch,use.bytes);
                producer[24]=backing.writerNonce;producer[25]=backing.writerBytes;producer[26]=backing.writerDestination;
                putLease(27,backing.writerRecording);
                producer[32]=source_cbv_snapshot::detail::InlineTransientUpload(backing,epoch,use.bytes);
                source.cbvs.push_back(std::move(backing));
            }else work->cbvFailed=true;
            if(work->sourceDetails){
                if(value[23])CaptureDetails(list,draw.root,value,source.cbvs.back());
                else{std::array<uint64_t,128> detail{};detail[0]=work->cbvRows.size();std::copy_n(value.begin(),6,detail.begin()+1);
                    detail[39]=UINT(source_cbv_details::Status::SourceBefore);work->detailRows.push_back(detail);work->detailsFailed=true;}
            }
            work->cbvRows.push_back(value);
            work->cbvProducers.push_back(producer);
            work->cbvDiagnostics.push_back(diagnostic);
        }
    }
    if(work->sourceTables)CaptureTables(list,source,row);
    if(work->sourceSelectors)CaptureSelectors(list,source,row);
    if(work->sourceTables)CaptureMaterialResources(list,source,row);
    if(work->sourceIaSnapshots && source.command.geometry.state.rate.image){
        source.vrs=std::make_shared<StereoVrsSnapshot>();probe::Scope internal;
        if(FAILED(source.vrs->Initialize(work->device.Get(),source.command.geometry.state.rate.image)) ||
            FAILED(source.vrs->Capture(list,source.command.geometry.state.rate.image,0)))source.vrs.reset();
    }
    if(work->sourceCbvSnapshots)CaptureCbvSnapshots(list,side,source,row);
    CaptureT5Snapshot(list,side,source,row);
    CaptureFullT5(list,side,source,row);
    CaptureInstanceWriter(list,side,source,row);
    CaptureIaSnapshots(list,side,source,row,!indirect || kind==1);
    CaptureBorrowedTables(list,side,source,row);
    source.passDepth=work->eyes[side].passDepth;
    if(!work->failed && indirect && work->sourceSnapshots){
        const uint64_t position=row[0]+1;std::array<uint64_t,24> snapshot{};
        snapshot[0]=row[0];snapshot[1]=side;snapshot[6]=row[6];snapshot[7]=lease.generation;snapshot[8]=position;
        snapshot[9]=row[32];snapshot[10]=row[33];snapshot[19]=row[29];snapshot[22]=lease.context;
        HRESULT result=E_ACCESSDENIED;
        indirect_copy::ActualCallScope scope(list,lease.generation,position);
        if(scope.Active() && !row[44] && source.indirectBinding.arguments.resource &&
            indirect_copy::PrepareAtActualCall(list,source.indirectBinding,source.parent,position,source.indirectProof)){
            snapshot[3]=1;snapshot[14]=UINT(*source.indirectProof.state);
            snapshot[15]=reinterpret_cast<uintptr_t>(source.indirectProof.resource.Get());
            snapshot[16]=reinterpret_cast<uintptr_t>(source.indirectProof.allocationOwner.Get());
            result=StereoGeometryCommands::CaptureIndirectSource(list,*indirect,source.indirectProof,nullptr,indirect_copy::Validate,source.indirectSnapshot);
        }
        snapshot[4]=UINT(result);
        if(SUCCEEDED(result)){
            source.command.indirect=source.indirectSnapshot.replay;if(replay)*replay=source.indirectSnapshot.replay;
            snapshot[2]=1;snapshot[11]=reinterpret_cast<uintptr_t>(source.indirectSnapshot.replay.arguments.Get());
            snapshot[12]=source.indirectSnapshot.replay.argumentOffset;snapshot[13]=source.indirectSnapshot.receipt.copiedBytes[0];
            snapshot[17]=reinterpret_cast<uintptr_t>(source.indirectSnapshot.replay.countBuffer.Get());snapshot[18]=source.indirectSnapshot.receipt.copiedBytes[1];
        }else{work->snapshotFailed=true;snapshot[5]=indirect_copy::LastFailure();if(!snapshot[5])snapshot[5]=UINT(indirect_copy::Failure::Ownership);}
        work->snapshotRows.push_back(snapshot);
    }
    if(row[44]){work->sourceFailed=true;if(!CyberpunkVR_RetainedSourceStreamStats[12])CyberpunkVR_RetainedSourceStreamStats[12]=row[44];}
    work->sources.push_back(std::move(source));work->sourceRows.push_back(row);PublishSources();
}catch(...){work->sourceFailed=true;if(work->sourceCbvSnapshots){work->cbvSnapshotsFailed=true;work->eyes[side].cbvArenaAborted=true;work->eyes[side].cbvArena.reset();Fail(960);}
    if(!CyberpunkVR_RetainedSourceStreamStats[12])CyberpunkVR_RetainedSourceStreamStats[12]=906;PublishSources();}
void CompileSources()try{
    if(!work->sourceStream || work->sourceFailed || work->sourceCompiled || !work->eyes[0].sealed || !work->eyes[1].sealed)return;
    std::vector<GeometrySourceCommand> commands;commands.reserve(work->sources.size());
    for(const auto& source:work->sources)commands.push_back(source.command);
    if(FAILED(work->sourceGeometry.CreateMixedSources(work->device.Get(),commands))){
        work->sourceFailed=true;CyberpunkVR_RetainedSourceStreamStats[12]=907;
    }else work->sourceCompiled=work->sourceGeometry.SourceCount()==work->sources.size();
    PublishSources();
}catch(...){work->sourceFailed=true;if(!CyberpunkVR_RetainedSourceStreamStats[12])CyberpunkVR_RetainedSourceStreamStats[12]=906;PublishSources();}
void Fail(UINT reason){if(work){work->failed=true;DisarmFinishedFrontierObserver(*work);}
    target_handoff::FinishObservation();
    target_access::Finish();
    if(!CyberpunkVR_RetainedGeometryStats[14])CyberpunkVR_RetainedGeometryStats[14]=reason;CyberpunkVR_RetainedGeometryState=4;}
bool BindNativeSourceTables(ID3D12GraphicsCommandList* list){
    if(!work->sourceStream || !work->sourceTables || work->mode!=1 || work->sources.empty())return false;
    const auto ordinal=work->sources.size()-1;auto& source=work->sources.back();const auto& saved=work->sourceRows[ordinal];
    std::array<uint64_t,16> row{ordinal,saved[1]};row[11]=saved[6];row[12]=saved[7];
    bool bound{};
    if(native_descriptors::CopyTables(list,source.bindings,source.tables,ordinal,work->eyes[size_t(saved[1])].descriptorCache,source.nativeTables)){
        const auto& native=source.nativeTables;row[2]=native.ring[2]?2:1;row[4]=native.descriptors;row[5]=native.bindings.size();row[6]=native.heap;
        row[7]=native.base;row[8]=native.capacity;row[9]=native.range.before;row[10]=native.range.after;row[13]=native.borrowedMask;
        bound=native_descriptors::Bind(list,native);row[14]=bound;if(!bound)Fail(2300);
        work->nativeTableEntries.insert(work->nativeTableEntries.end(),native.entries.begin(),native.entries.end());
        if(native.ring[2])work->nativeTableRings.push_back(native.ring);
    }else{row[3]=UINT(native_descriptors::LastFailure());
        if(row[3]!=UINT(native_descriptors::Failure::Capacity) && row[3]!=UINT(native_descriptors::Failure::State))Fail(2300+UINT(row[3]));}
    work->nativeTableRows.push_back(row);PublishSources();return bound;
}
UINT EyeIndex(UINT side){return 1-side;}
void Restore(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw){
    const D3D12_CPU_DESCRIPTOR_HANDLE depth{draw.depth};std::array<D3D12_CPU_DESCRIPTOR_HANDLE,8> handles{};
    for(UINT i=0;i<8;++i)handles[i]={draw.targets[i]};probe::Scope internal;
    list->OMSetRenderTargets(draw.rtCount,draw.rtCount?handles.data():nullptr,BOOL(draw.rtContiguous),draw.depth?&depth:nullptr);
}
void Publish(){
    if(!work || !work->created)return;const auto counts=work->chain.Counts();
    for(UINT side=0;side<2;++side){CyberpunkVR_RetainedGeometryStats[10+side]=counts[EyeIndex(side)];CyberpunkVR_RetainedGeometryStats[12+side]=counts[5+EyeIndex(side)];}
}
void PublishShaders(){
    if(!work)return;work->shaders.clear();work->shaderRows.clear();
    work->shaders.reserve(work->pipelines.size());work->shaderRows.reserve(work->pipelines.size());
    for(const auto& pipeline:work->pipelines){const auto source=shader_snapshot::Capture(pipeline.Get());if(!source)continue;
        Work::ShaderExport exported;exported.source=source;exported.inputs.reserve(source->input.size());
        bool valid=true;for(const auto& input:source->input){if(input.semantic.size()>=64){valid=false;break;}
            Work::InputRow row;std::copy(input.semantic.begin(),input.semantic.end(),row.name);row.index=input.index;row.format=input.format;
            row.slot=input.slot;row.offset=input.offset;row.classification=input.classification;row.step=input.step;exported.inputs.push_back(row);}
        if(!valid)continue;
        work->shaders.push_back(std::move(exported));const auto& saved=work->shaders.back();
        work->shaderRows.push_back({source->id,reinterpret_cast<uintptr_t>(pipeline.Get()),reinterpret_cast<uintptr_t>(source->vs.data()),source->vs.size(),
            reinterpret_cast<uintptr_t>(source->ps.data()),source->ps.size(),reinterpret_cast<uintptr_t>(source->root.data()),source->root.size(),
            reinterpret_cast<uintptr_t>(saved.inputs.data()),saved.inputs.size(),sizeof(Work::InputRow)});
    }
    CyberpunkVR_RetainedGeometryShaderRows=reinterpret_cast<uintptr_t>(work->shaderRows.data());CyberpunkVR_RetainedGeometryShaderCount=UINT(work->shaderRows.size());
}
bool Proof(ID3D12GraphicsCommandList* list,UINT side)try{
    auto& value=work->eyes[side];std::vector<Plane> planes;UINT64 size{};
    for(UINT slot=0;slot<5;++slot)if(value.imported[slot]){
        auto* native=value.targets.resources[slot].Get();auto* array=work->chain.Resource(slot);
        const auto nativeDesc=native->GetDesc(),arrayDesc=array->GetDesc();
        for(UINT plane=0;plane<(slot==4?2u:1u);++plane){Plane row;row.slot=slot;row.plane=plane;
            UINT rows[2]{};UINT64 bytes[2]{},total{};
            work->device->GetCopyableFootprints(&nativeDesc,plane,1,size,&row.layouts[0],&rows[0],&bytes[0],&total);
            if(total==UINT64_MAX || size>UINT64_MAX-total)return false;size+=total;size=(size+511)&~UINT64(511);
            work->device->GetCopyableFootprints(&arrayDesc,EyeIndex(side)+plane*2,1,size,&row.layouts[1],&rows[1],&bytes[1],&total);
            if(total==UINT64_MAX || size>UINT64_MAX-total || rows[0]!=rows[1] || rows[0]!=nativeDesc.Height || bytes[0]!=bytes[1] ||
                bytes[0]%nativeDesc.Width || bytes[0]/nativeDesc.Width>UINT_MAX)return false;
            size+=total;size=(size+511)&~UINT64(511);row.rows=rows[0];row.stride=UINT(bytes[0]/nativeDesc.Width);
            row.bytes=slot==4?(plane?1u:4u):4u;row.component=slot==4 && plane && row.stride==8?4u:0u;
            if((slot==4 && row.stride!=4 && row.stride!=8 && !(plane && row.stride==1)) || (slot<4 && row.stride!=4))return false;
            planes.push_back(row);
        }
    }
    if(!size || size>512*1024*1024)return false;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=size;desc.Height=1;
    desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    probe::Scope internal;
    if(FAILED(work->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&value.readback))) ||
        !gpu::KeepCommandResources(list,{value.readback.Get()}))return false;
    for(UINT slot=0;slot<5;++slot)if(value.imported[slot]){
        const auto state=slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET;
        auto* native=value.targets.resources[slot].Get();auto* array=work->chain.Resource(slot);
        if(!gpu::KeepCommandResources(list,{native,array}))return false;
        D3D12_RESOURCE_BARRIER barriers[3]{};UINT count{};
        auto add=[&](ID3D12Resource* resource,UINT sub){auto& b=barriers[count++];b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={resource,sub,state,D3D12_RESOURCE_STATE_COPY_SOURCE};};
        add(native,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES);
        for(UINT plane=0;plane<(slot==4?2u:1u);++plane)add(array,EyeIndex(side)+plane*2);
        list->ResourceBarrier(count,barriers);
        for(const auto& plane:planes)if(plane.slot==slot)for(UINT version=0;version<2;++version){D3D12_TEXTURE_COPY_LOCATION from{},to{};
            from.pResource=version?array:native;from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
            from.SubresourceIndex=version?EyeIndex(side)+plane.plane*2:plane.plane;
            to.pResource=value.readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=plane.layouts[version];
            list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
        }
        for(UINT i=0;i<count;++i)std::swap(barriers[i].Transition.StateBefore,barriers[i].Transition.StateAfter);
        list->ResourceBarrier(count,barriers);
    }
    value.planes=std::move(planes);return true;
}catch(...){return false;}
void Seal(ID3D12GraphicsCommandList* list,UINT side,UINT reason,target_handoff::ExitKind kind=target_handoff::ExitKind::Unknown){
    auto& value=work->eyes[side];if(!value.started || value.sealed || value.recording.list!=reinterpret_cast<uintptr_t>(list))return;
    value.sealReason=reason;
    if(work->tailEnabled)target_handoff::MarkExit(list,reason,kind);
    if(work->tailEnabled)target_access::MarkExit(list);
    native_probe::DrawRecord actual;const bool restore=native_probe::CurrentState(list,actual);
    if(work->mode==1){
        const std::array<ComPtr<ID3D12Resource>,4> proofTargets{
            value.targets.resources[0],value.targets.resources[1],value.targets.resources[2],value.targets.resources[4]};
        target_access::DiagnosticProofScope proofScope(list,proofTargets);
        if(!Proof(list,side))Fail(40+reason);else proofScope.Complete();
    }
    else if(!work->chain.ExportEye(list,EyeIndex(side)))Fail(60+reason);
    if(restore)Restore(list,actual);
    if(work->sourceCbvSnapshots && value.cbvArena && !value.cbvArenaAborted){
        if(!value.cbvArena->Finalize(list)){work->cbvSnapshotsFailed=true;value.cbvArenaAborted=true;value.cbvArena.reset();
            Fail(1100+UINT(source_cbv_snapshot::LastFailure()));}
        else{
            for(auto& row:work->cbvSnapshotCommands)if(row[1]==side && row[2]==1)row[17]=1;
            for(auto& row:work->cbvSnapshotRows)if(row[1]==side && row[8]==1)row[29]=1;
            ComPtr<ID3D12Resource> source;uint64_t bytes{};
            for(const auto& captured:work->sources)for(const auto& cbv:captured.privateCbvs)
                if(captured.command.geometry.draw.side==side){source=cbv.resource;bytes=std::max(bytes,cbv.privateOffset+cbv.paddedBytes);}
            if(source && bytes){
                auto desc=source->GetDesc();desc.Width=bytes;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
                probe::Scope internal;
                if(FAILED(work->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&value.cbvReadback))) ||
                    !gpu::KeepCommandResources(list,{source.Get(),value.cbvReadback.Get()})){work->cbvSnapshotsFailed=true;Fail(1120);}
                else list->CopyBufferRegion(value.cbvReadback.Get(),0,source.Get(),0,bytes);
            }
        }
    }
    if(work->sourceSelectors && value.t5Arena && value.t5Arena->Bytes() && !value.t5Aborted){
        if(!value.t5Arena->Finalize(list)){value.t5Aborted=true;value.t5Arena->Abort();Fail(1400+UINT(structured_buffer_snapshot::LastFailure()));}
        else{
            const auto source=value.t5Arena->Resource();const auto bytes=value.t5Arena->Bytes();
            auto desc=source->GetDesc();desc.Width=bytes;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
            probe::Scope internal;
            if(FAILED(work->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&value.t5Readback))) ||
                !gpu::KeepCommandResources(list,{source.Get(),value.t5Readback.Get()})){value.t5Aborted=true;Fail(1450);}
            else{list->CopyBufferRegion(value.t5Readback.Get(),0,source.Get(),0,bytes);
                for(auto& row:work->t5SnapshotRows)if(row[1]==side && row[2]==1)row[23]=1;}
        }
    }
    if(work->fullT5Enabled && value.fullT5Arena){
        auto* stats=CyberpunkVR_RetainedFullT5Stats;
        if(!value.fullT5Arena->Finalize(list) || !FullT5Readback(list,value.fullT5Arena->Bytes(),value.fullT5PrivateReadback)){
            if(!stats[11])stats[11]=4;value.fullT5Arena->Abort();Fail(2704);
        }else{probe::Scope internal;
            list->CopyBufferRegion(value.fullT5PrivateReadback.Get(),0,value.fullT5Arena->Resource().Get(),0,value.fullT5Arena->Bytes());
            stats[7]|=1ull<<side;
        }
    }
    if(work->sourceIaSnapshots && value.iaArena && value.iaArena->Bytes() && !value.iaAborted){
        if(!value.iaArena->Finalize(list)){const auto error=ia_snapshot::LastFailure();value.iaAborted=true;value.iaArena->Abort();Fail(1740+UINT(error));
            for(auto& command:work->iaCommands)if(command[1]==side && command[6] && !command[7])command[7]=UINT(error);}
        else{
            for(auto& command:work->iaCommands)if(command[1]==side && command[6] && !command[7])command[14]=1;
            for(auto& row:work->iaViews)if(row[1]==side)row[13]=1;
        }
    }
    if(value.materialArena && value.materialArena->Bytes() && !value.materialArena->Finalize(list)){
        value.materialArena->Abort();Fail(2201);
    }
    if(value.instanceCapture && value.instanceCapture->VersionCount()){
        if(!value.instanceCapture->Finalize(list) || !value.instanceReadbackBytes)Fail(1811);
        else{
            auto desc=value.instanceCapture->Resource()->GetDesc();desc.Width=value.instanceReadbackBytes;
            D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
            HRESULT created{};{probe::Scope internal;created=work->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,
                IID_PPV_ARGS(&value.instancePrivateReadback));}
            if(FAILED(created) || !gpu::KeepCommandResources(list,{value.instancePrivateReadback.Get()}))Fail(1812);
            else for(size_t index=0;index<work->sources.size();++index){const auto& source=work->sources[index];
                if(!source.instanceView || source.command.geometry.draw.side!=side)continue;const auto& view=*source.instanceView;
                if(index>=work->instanceGpuRows.size() || !instance_snapshot_view::ValidateNativeViewForRecording(view,list,value.recording)){Fail(1813);break;}
                const auto& row=work->instanceGpuRows[index];
                if(!row[2] || row[10]>value.instanceReadbackBytes || row[11]>value.instanceReadbackBytes-row[10] || row[11]!=view.mapping.copyBytes){Fail(1814);break;}
                probe::Scope internal;list->CopyBufferRegion(value.instancePrivateReadback.Get(),row[10],view.resource.Get(),view.mapping.targetOffset,view.mapping.copyBytes);
            }
        }
    }
    value.sealed=true;CyberpunkVR_RetainedGeometryStats[4+side]=1;Publish();
    if(work->sourceStream && CyberpunkVR_RetainedSourceBoundaryCount<2){
        const auto& peer=work->eyes[1-side];auto* row=CyberpunkVR_RetainedSourceBoundaryRows+CyberpunkVR_RetainedSourceBoundaryCount*20;
        row[0]=work->frame;row[1]=side;row[2]=CyberpunkVR_RetainedSourceBoundaryCount;row[3]=value.recording.list;row[4]=value.recording.generation;
        row[5]=std::count_if(work->sourceRows.begin(),work->sourceRows.end(),[&](const auto& source){return source[1]==side;});row[6]=work->sourceRows.size();
        row[7]=peer.sealed;row[8]=peer.submitted;row[9]=peer.submissionCertified;row[10]=reinterpret_cast<uintptr_t>(peer.sourceQueue.Get());
        row[11]=reinterpret_cast<uintptr_t>(peer.fence.Get());row[12]=peer.signaledValue;row[13]=peer.fence?peer.fence->GetCompletedValue():0;
        row[14]=std::count_if(work->iaCommands.begin(),work->iaCommands.end(),[&](const auto& command){return command[1]==side && command[14];});
        row[15]=std::count_if(work->sources.begin(),work->sources.end(),[&](const auto& source){return source.command.geometry.draw.side==side && !source.tables.empty();});
        SIZE_T copied{};if(value.recording.registry && value.recording.registry<=UINT64_MAX-0x14104D8)
            ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(value.recording.registry+0x14104D0),row+16,8,&copied);
        if(copied!=8)row[16]=0;
        row[17]=value.cbvArena && value.cbvArena->Finalized();
        row[18]=std::count_if(work->sources.begin(),work->sources.end(),[&](const auto& source){return source.command.geometry.draw.side==side && source.instanceView &&
            instance_snapshot_view::ValidateNativeViewForRecording(*source.instanceView,list,value.recording);});
        row[19]=work->payloadsProcessed;++CyberpunkVR_RetainedSourceBoundaryCount;
    }
    CompileSources();PublishSources();
    CyberpunkVR_RetainedGeometryEnds[side]=reason;
    if(work->eyes[0].sealed && work->eyes[1].sealed && !work->failed)CyberpunkVR_RetainedGeometryState=2;
}
Eye* Owned(ID3D12GraphicsCommandList* list,UINT& side){
    if(!work)return nullptr;
    for(side=0;side<2;++side){auto& value=work->eyes[side];
        if(value.started && !value.sealed && value.recording.list==reinterpret_cast<uintptr_t>(list))return &value;
    }
    return nullptr;
}
bool Touches(const Eye& value,ID3D12Resource* resource){
    if(!resource)return false;
    for(UINT slot=0;slot<5;++slot)if(value.imported[slot] && value.targets.resources[slot].Get()==resource)return true;
    return false;
}
bool OverlayB4(size_t index,const source_cbv_snapshot::PrivateCbv*& value,std::span<const uint8_t>& bytes,size_t& snapshotIndex){
    value=nullptr;bytes={};snapshotIndex=SIZE_MAX;
    if(index>=work->sources.size() || index>=work->sourceRows.size())return false;
    const auto& source=work->sources[index];const auto& saved=work->sourceRows[index];
    const auto input=std::find_if(source.selectorInputs.begin(),source.selectorInputs.end(),[](const auto& item){
        return item.key.visibility==5 && item.key.shaderRegister==4 && !item.key.space && item.key.parameter==19 && !item.key.tableOffset;
    });
    const auto b4=std::find_if(source.privateCbvs.begin(),source.privateCbvs.end(),[](const auto& item){return item.parameter==19 && !item.offset;});
    if(input==source.selectorInputs.end() || b4==source.privateCbvs.end() || input->row>=work->selectorInputs.size())return false;
    const auto& matched=work->selectorInputs[input->row];
    if(matched[0]!=index || matched[1]!=saved[1] || matched[13]!=1 || !matched[14] || matched[14]>work->cbvSnapshotRows.size())return false;
    snapshotIndex=size_t(matched[14]-1);const auto& row=work->cbvSnapshotRows[snapshotIndex];
    if(row[0]!=index || row[1]!=saved[1] || row[2]!=5 || row[3]!=4 || row[4]!=19 || row[5] || row[8]!=1 ||
        !row[29] || !row[30] || !row[31] || row[20]!=multilayer_selectors::B4Bytes ||
        row[17]!=reinterpret_cast<uintptr_t>(b4->resource.Get()) || row[18]!=b4->view.BufferLocation || row[19]!=b4->privateOffset ||
        row[20]!=b4->copiedBytes || row[21]!=b4->paddedBytes || row[22]!=reinterpret_cast<uintptr_t>(b4->owner.Get()) ||
        !b4->resource || !b4->owner || b4->reflectedBytes!=multilayer_selectors::B4Bytes ||
        !source.cbvPrepared.owner || source.cbvPrepared.list!=saved[6] || source.cbvPrepared.generation!=saved[7] || source.cbvPrepared.occurrence!=index+1 ||
        row[31]-1>work->cbvBlob.size() || row[20]>work->cbvBlob.size()-(row[31]-1))return false;
    bytes={work->cbvBlob.data()+row[31]-1,size_t(row[20])};
    if(!std::equal(bytes.begin(),bytes.end(),source.t5Plan.b4Bytes.begin()))return false;
    value=&*b4;return true;
}
bool PrepareT5Overlay()try{
    if(!work->sourceSelectors || work->overlayStage==2)return true;
    if(work->overlayStage==3)return false;
    std::vector<structured_snapshot_view::Input> inputs;std::vector<size_t> indices;
    for(size_t index=0;index<work->sources.size();++index){const auto& source=work->sources[index];
        if(!source.t5Plan.byteExtent)continue;
        if(source.t5Row>=work->t5SnapshotRows.size())throw std::logic_error("Nonempty t5 has no copy receipt");
        const auto& row=work->t5SnapshotRows[source.t5Row];const auto& saved=work->sourceRows[index];
        const auto& eye=work->eyes[size_t(saved[1])];
        const source_cbv_snapshot::PrivateCbv* b4{};std::span<const uint8_t> bytes;size_t b4Row{};
        D3D12_SHADER_RESOURCE_VIEW_DESC recorded{};
        if(row[2]!=1 || !row[23] || !row[24] || row[28]!=1 || !source.t5OriginalCertified ||
            !eye.submissionCertified || !eye.sourceQueue || !eye.signaledValue || eye.t5Aborted || !eye.t5Arena || !eye.t5Arena->Finalized())
            throw std::logic_error("t5 source ownership/completion differs at "+std::to_string(index));
        if(!structured_buffer_snapshot::RecordedView(source.t5Prepared,source.t5Private,recorded))
            throw std::logic_error("t5 original-position receipt differs at "+std::to_string(index));
        if(!OverlayB4(index,b4,bytes,b4Row))throw std::logic_error("t5 B4 source association differs at "+std::to_string(index));
        structured_snapshot_view::Input input;
        input.scope={saved[11],saved[6],saved[7],index+1,UINT(saved[1])};input.privateB4Scope=input.scope;
        input.dense=source.t5Private;input.copy=source.t5Prepared;input.originalView=source.t5Plan.view;input.readPlan=source.t5Plan;
        input.privateB4Bytes=bytes;input.privateB4GpuMatchVerified=true;input.allRecordPathsBounded=source.t5Plan.originalSiteCoverageComplete;
        input.completion={input.scope,eye.sourceQueue,eye.fence,eye.signaledValue,true,true};
        inputs.push_back(std::move(input));indices.push_back(index);
    }
    if(inputs.empty())return true;
    structured_snapshot_view::Failure failure;work->t5Overlay=std::make_unique<structured_snapshot_view::Overlay>();
    if(!work->t5Overlay->AdoptSources(work->device.Get(),inputs,failure))
        throw std::logic_error("Native structured view adoption rejected: "+std::to_string(UINT(failure)));
    work->overlayRows.reserve(indices.size());
    for(UINT version=0;version<indices.size();++version){const auto index=indices[version];auto& source=work->sources[index];
        structured_snapshot_view::PrivateView view;D3D12_SHADER_RESOURCE_VIEW_DESC recorded{};
        const source_cbv_snapshot::PrivateCbv* b4{};std::span<const uint8_t> bytes;size_t b4Row{};
        if(!work->t5Overlay->GetView(version,view) || !structured_snapshot_view::ValidatePrivateView(view))
            throw std::logic_error("Adopted structured view owner differs at "+std::to_string(index));
        if(!structured_buffer_snapshot::RecordedView(source.t5Prepared,source.t5Private,recorded))
            throw std::logic_error("Adopted structured source receipt differs at "+std::to_string(index));
        if(!structured_snapshot_view::detail::SameBufferView(view.srv,recorded))
            throw std::logic_error("Adopted structured SRV fields differ at "+std::to_string(index));
        if(!OverlayB4(index,b4,bytes,b4Row))throw std::logic_error("Adopted structured B4 association differs at "+std::to_string(index));
        const auto& saved=work->sourceRows[index];const auto& snapshot=work->t5SnapshotRows[source.t5Row];
        if(!snapshot[27] || snapshot[27]-1>work->t5Blob.size() || view.copiedBytes>work->t5Blob.size()-(snapshot[27]-1))
            throw std::logic_error("Native structured view has no original GPU payload");
        const auto* data=work->t5Blob.data()+snapshot[27]-1;
        work->overlayBlob.insert(work->overlayBlob.end(),data,data+view.copiedBytes);
        work->overlayRows.push_back({index,saved[1],1,0,saved[11],saved[6],saved[7],index+1,
            reinterpret_cast<uintptr_t>(view.resource.Get()),reinterpret_cast<uintptr_t>(view.owner.Get()),
            reinterpret_cast<uintptr_t>(source.t5Private.resource.Get()),reinterpret_cast<uintptr_t>(source.t5Private.owner.Get()),
            source.t5Private.privateOffset,view.sourceOffset,view.copiedBytes,view.targetOffset,view.srv.Buffer.FirstElement,
            view.srv.Buffer.NumElements,view.srv.Buffer.StructureByteStride,reinterpret_cast<uintptr_t>(b4->resource.Get()),
            reinterpret_cast<uintptr_t>(b4->owner.Get()),reinterpret_cast<uintptr_t>(source.cbvPrepared.owner.Get()),b4->view.BufferLocation,
            b4->privateOffset,b4->copiedBytes,b4Row+1,1,1,0,view.copyList,view.copyGeneration,1});
        source.t5View=std::move(view);
    }
    work->overlayStage=2;return true;
}catch(const std::exception& error){std::snprintf(CyberpunkVR_RetainedT5OverlayError,sizeof(CyberpunkVR_RetainedT5OverlayError),"%s",error.what());
    work->overlayStage=3;Fail(1601);PublishSources();return false;
}catch(...){std::snprintf(CyberpunkVR_RetainedT5OverlayError,sizeof(CyberpunkVR_RetainedT5OverlayError),"Unknown structured view exception");
    work->overlayStage=3;Fail(1601);PublishSources();return false;}
bool PrepareInstanceOverlay()try{
    if(!work->sourceIaSnapshots || work->instanceOverlayStage==2)return true;
    if(work->instanceOverlayStage==3)return false;
    std::array<void*,2> mapped{};
    struct Unmap {std::array<Eye,2>& eyes;std::array<void*,2>& data;~Unmap(){const D3D12_RANGE none{};
        for(UINT side=0;side<2;++side)if(data[side])eyes[side].instancePrivateReadback->Unmap(0,&none);}} unmap{work->eyes,mapped};
    for(UINT side=0;side<2;++side){const auto& eye=work->eyes[side];if(!eye.instanceCapture)continue;
        const D3D12_RANGE range{0,SIZE_T(eye.instanceReadbackBytes)};
        if(!eye.instancePrivateReadback || FAILED(eye.instancePrivateReadback->Map(0,&range,&mapped[side])) || !mapped[side])
            throw std::runtime_error("Native instance copy diagnostic readback failed");
    }
    for(size_t index=0;index<work->instanceGpuRows.size();++index){const auto& original=work->instanceGpuRows[index];if(original[5]!=1)continue;
        const auto& source=work->sources[index];const auto& eye=work->eyes[size_t(original[1])];
        const instance_snapshot_view::NativeCompletion completion{eye.recording,eye.sourceQueue,eye.fence,eye.signaledValue,eye.submissionCertified};
        if(!source.instanceView || !instance_snapshot_view::ValidateNativeViewAfterCompletion(*source.instanceView,completion) ||
            !original[2] || !original[3] || !original[4] || !original[21] || !mapped[original[1]] ||
            original[10]>eye.instanceReadbackBytes || original[11]>eye.instanceReadbackBytes-original[10] || !original[18] ||
            original[18]-1>work->instanceGpuBlob.size() || original[11]>work->instanceGpuBlob.size()-(original[18]-1))
            throw std::logic_error("Native instance copy lacks original submission evidence");
        const auto& view=*source.instanceView;const auto* bytes=static_cast<const uint8_t*>(mapped[original[1]])+original[10];
        if(view.mapping.copyBytes!=original[11] || std::memcmp(bytes,work->instanceGpuBlob.data()+original[18]-1,size_t(original[11])))
            throw std::logic_error("Native private instance copy differs from original GPU bytes");
        work->instanceOverlayRows.push_back({index,view.scope.eye,view.version,view.scope.root,view.scope.list,view.scope.generation,view.scope.occurrence,
            reinterpret_cast<uintptr_t>(view.pipeline.Get()),view.shader->id,view.original.BufferLocation,view.original.SizeInBytes,view.original.StrideInBytes,
            reinterpret_cast<uintptr_t>(view.resource.Get()),reinterpret_cast<uintptr_t>(view.owner.Get()),view.vertex.BufferLocation,view.mapping.targetOffset,
            view.mapping.copyBytes,eye.instanceCapture->RequiredBytes(),1,1,1,original[18],view.copyList,view.copyGeneration});
        work->instanceOverlayBlob.insert(work->instanceOverlayBlob.end(),bytes,bytes+original[11]);
    }
    work->instanceOverlayStage=2;return true;
}catch(const std::exception& error){std::snprintf(CyberpunkVR_RetainedInstanceOverlayError,sizeof(CyberpunkVR_RetainedInstanceOverlayError),"%s",error.what());
    work->instanceOverlayStage=3;Fail(1810);PublishSources();return false;
}catch(...){std::snprintf(CyberpunkVR_RetainedInstanceOverlayError,sizeof(CyberpunkVR_RetainedInstanceOverlayError),"Unknown instance overlay exception");
    work->instanceOverlayStage=3;Fail(1810);PublishSources();return false;}
void BuildPrivateGeometry() {
    if(!work->sourceIaSnapshots || !work->privateGeometryRows.empty())return;
    for(size_t index=0;index<work->sources.size();++index){auto& source=work->sources[index];const auto& saved=work->sourceRows[index];
        const auto& eye=work->eyes[size_t(saved[1])];
        const instance_snapshot_view::NativeCompletion completion{eye.recording,eye.sourceQueue,eye.fence,eye.signaledValue,eye.submissionCertified};
        std::array<uint64_t,16> row{index,saved[1]};row[3]=saved[19];row[4]=saved[6];row[5]=saved[7];row[6]=index+1;
        if(index>=work->iaCommands.size() || index>=work->iaSelections.size())throw std::logic_error("Private geometry has no IA selection");
        const auto& copied=work->iaCommands[index];const auto& selection=work->iaSelections[index];
        if(copied[6] && copied[14] && copied[15] && !copied[7]){
            auto assembled=source.command.geometry;bool ready=selection[2]==0;
            if(selection[2]==128 && source.instanceView && work->instanceGpuRows[index][5]==1 &&
                instance_snapshot_view::ValidateNativeViewAfterCompletion(*source.instanceView,completion)){
                const auto& view=*source.instanceView;const auto desc=view.resource->GetDesc();const auto gpu=view.resource->GetGPUVirtualAddress();
                if(view.scope.list!=saved[6] || view.scope.generation!=saved[7] || view.scope.occurrence!=index+1 || view.scope.eye!=saved[1] ||
                    view.draw.IndexCountPerInstance!=saved[20] || view.draw.InstanceCount!=saved[21] || view.draw.StartIndexLocation!=saved[22] ||
                    uint32_t(view.draw.BaseVertexLocation)!=saved[23] || view.draw.StartInstanceLocation!=saved[24])throw std::logic_error("Private instance draw coordinates changed");
                assembled.vertices[7]={view.resource,gpu,desc.Width,D3D12_HEAP_TYPE_DEFAULT,UINT(desc.Flags),{}};
                assembled.draw.vertices[7]=view.vertex;
                const auto& match=work->instanceGpuRows[index];const auto begin=work->instanceGpuBlob.begin()+ptrdiff_t(match[18]-1);
                assembled.instances.assign(begin,begin+ptrdiff_t(match[11]));ready=true;
                row[9]=reinterpret_cast<uintptr_t>(view.resource.Get());row[10]=reinterpret_cast<uintptr_t>(view.owner.Get());row[11]=view.vertex.BufferLocation;
            }
            if(ready){row[2]=1;row[7]=reinterpret_cast<uintptr_t>(source.iaPrepared.owner.Get());row[8]=source.iaViews.size();
                row[12]=assembled.draw.firstInstance;row[13]=assembled.draw.instances;source.privateGeometry=std::move(assembled);}
        }
        work->privateGeometryRows.push_back(row);
    }
}
void BuildGuardedSources()try{
    if(!work || work->guardedAttempted || work->failed || !work->sourceSelectors || !work->sourceCbvSnapshots || !work->sourceTables ||
        !work->eyes[0].sealed || !work->eyes[1].sealed || !work->eyes[0].submissionCertified || !work->eyes[1].submissionCertified)return;
    using namespace source_binding_assembly;work->guardedAttempted=true;auto* stats=CyberpunkVR_RetainedGuardedStats;
    stats[0]=1;stats[1]=work->frame;stats[2]=work->sources.size();stats[7]=work->eyes[0].sealed;stats[8]=work->eyes[1].sealed;
    stats[9]=work->eyes[0].submissionCertified;stats[10]=work->eyes[1].submissionCertified;
    stats[11]=work->payloadsProcessed;stats[12]=work->cbvPayloadComplete;stats[13]=work->overlayStage;stats[14]=UINT64_MAX;
    const auto publish=[](){CyberpunkVR_RetainedGuardedRows=reinterpret_cast<uintptr_t>(work->guardedRows.data());
        CyberpunkVR_RetainedGuardedCount=UINT(work->guardedRows.size());};
    const auto reject=[&](Failure reason,size_t index){stats[0]=3;stats[14]=index;stats[15]=UINT(reason);Fail(2400+UINT(reason));publish();};
    if(stats[11] || stats[12] || stats[13] || work->sources.empty() || work->sources.size()!=work->sourceRows.size()){
        reject(Failure::Scope,UINT64_MAX);return;}
    work->guardedRows.reserve(work->sources.size());
    for(size_t index=0;index<work->sources.size();++index){auto& source=work->sources[index];const auto& saved=work->sourceRows[index];
        if(saved[0]!=index || saved[1]>1){reject(Failure::Scope,index);return;}
        const Scope scope{saved[11],saved[6],saved[7],index+1,index+1,UINT(saved[1])};auto& eye=work->eyes[scope.eye];
        auto& row=work->guardedRows.emplace_back(std::array<uint64_t,16>{index,scope.eye,scope.list,scope.generation,scope.occurrence,scope.root});
        row[11]=source.privateCbvs.size();row[12]=source.t5Plan.byteExtent;row[13]=eye.sealed;row[14]=eye.submissionCertified;row[15]=work->payloadsProcessed;
        Failure failure=Failure::Root;const auto family=families::Lookup(source.command.geometry.state.original.Get());
        ComPtr<ID3D12VersionedRootSignatureDeserializer> reader;const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* root{};
        if(!family || family->sourceRoot!=source.root || !source.shader || source.shader->root.empty() ||
            FAILED(D3D12CreateVersionedRootSignatureDeserializer(source.shader->root.data(),source.shader->root.size(),IID_PPV_ARGS(&reader))) ||
            FAILED(reader->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&root))){row[7]=UINT(failure);reject(failure,index);return;}
        GuardedInput input;input.source.scope=scope;input.source.selectorScope=scope;input.source.root=&root->Desc_1_1;input.source.shaderUses=family->uses;
        input.source.family={family->rootSha256,family->vertexSha256,family->pixelSha256};
        input.recipe=work->selectorCatalog?work->selectorCatalog->Find(input.source.family):nullptr;
        std::vector<FiniteTable> finite;std::vector<SparseTable> sparse;std::vector<PrivateCbv> cbvs;std::vector<PredictedCbv> predicted;
        for(const auto& table:source.tables)finite.push_back({scope,&table});
        for(const auto& table:source.selectorSlots)sparse.push_back({scope,&table});
        for(const auto& value:source.privateCbvs)cbvs.push_back({scope,&value,&source.cbvPrepared,eye.cbvArena && !eye.cbvArenaAborted && eye.cbvArena->Finalized()});
        for(const auto& value:source.selectorInputs)if(value.intent)predicted.push_back({value.key,value.intent.Bytes()});
        input.source.finite=finite;input.source.sparse=sparse;input.source.privateCbvs=cbvs;input.cbvs=predicted;
        PredictedT5 t5;
        if(input.recipe && !input.recipe->supported){
            const auto original=std::find_if(source.tables.begin(),source.tables.end(),[](const auto& table){return table.parameter==23;});
            if(original==source.tables.end() || !source.t5OriginalCertified){row[7]=UINT(Failure::SrvOwner);reject(Failure::SrvOwner,index);return;}
            t5.original.scope=scope;t5.original.parameter=23;t5.original.offset=5;t5.original.original=source.multilayerT5;
            t5.original.originalOwner=original->owner;t5.original.originalIdentity=original->identity;t5.original.originalVersion=original->version;
            t5.original.originalGpu=original->sourceGpu;t5.original.originalHeap=original->sourceHeap;t5.original.originalCount=original->count;
            t5.original.resourceCookie=source.multilayerT5.description[2];t5.original.originalSlotCertified=source.t5OriginalCertified;
            t5.copy=source.t5Private;t5.receipt=source.t5Prepared;if(source.t5Candidate)t5.expected=source.t5Candidate.Bytes();
            if(source.t5Plan.byteExtent)t5.recordedViewCertified=eye.t5Arena && eye.t5Arena->Finalized() && !eye.t5Aborted &&
                structured_buffer_snapshot::RecordedView(source.t5Prepared,source.t5Private,t5.view);
            input.t5=&t5;
        }
        if(!BuildGuarded(input,source.guardedPlan,failure)){row[7]=UINT(failure);reject(failure,index);return;}
        row[6]=1;++stats[3];
    }
    std::vector<const GuardedPlan*> plans;plans.reserve(work->sources.size());for(const auto& source:work->sources)plans.push_back(&source.guardedPlan);
    std::vector<GuardedTables> tables;Failure failure{};
    if(FAILED(AssembleGuardedPass(work->device.Get(),plans,work->assemblyCache,tables,failure)) || tables.size()!=work->sources.size()){
        reject(failure==Failure::None?Failure::Scope:failure,UINT64_MAX);return;}
    for(size_t index=0;index<tables.size();++index){auto& source=work->sources[index];auto& row=work->guardedRows[index];
        source.guardedBindings=std::move(tables[index]);row[8]=source.guardedBindings.tables.size();row[9]=source.guardedBindings.checks.size();
        for(const auto& check:source.guardedBindings.checks)row[10]+=check.expected.size();
        ++stats[4];stats[5]+=row[9];stats[6]+=row[10];
    }
    CyberpunkVR_RetainedGuardedCacheStats[0]=work->assemblyCache.UniqueDescriptorCount();
    CyberpunkVR_RetainedGuardedCacheStats[1]=work->assemblyCache.TableCount();
    CyberpunkVR_RetainedGuardedCacheStats[2]=work->assemblyCache.HitCount();
    CyberpunkVR_RetainedGuardedCacheStats[3]=work->assemblyCache.MissCount();
    stats[0]=2;publish();
}catch(...){CyberpunkVR_RetainedGuardedStats[0]=3;CyberpunkVR_RetainedGuardedStats[15]=UINT(source_binding_assembly::Failure::Exception);
    if(work){CyberpunkVR_RetainedGuardedRows=reinterpret_cast<uintptr_t>(work->guardedRows.data());CyberpunkVR_RetainedGuardedCount=UINT(work->guardedRows.size());}
    Fail(2415);}
UINT StagePairSource(size_t ordinal,prepared_pair::Source& value){
    auto& source=work->sources[ordinal];const auto& saved=work->sourceRows[ordinal];
    if(saved[1]>1 || source.command.indirect || !source.shader || !source.passDepth || source.materialTruncated ||
        source.guardedBindings.tables.empty() || source.guardedBindings.checks.empty())return 1;
    const auto& eye=work->eyes[size_t(saved[1])];
    if(!eye.sealed || !eye.submissionCertified || !eye.sourceQueue || !eye.iaValidated || ordinal>=work->iaCommands.size() ||
        ordinal>=work->iaSelections.size())return 2;
    const auto& copied=work->iaCommands[ordinal];const auto& selection=work->iaSelections[ordinal];
    if(!copied[6] || !copied[14] || copied[7] || !source.iaPrepared.owner)return 2;
    value.ordinal=ordinal;value.owned=source.command.geometry;
    value.sourceRecording=eye.recording;value.sourceQueue=eye.sourceQueue;
    if(!ia_snapshot::CopiedSourceShape(source.iaPrepared,value.original))return 5;
    if(selection[2]==128){
        if(!source.instanceView || ordinal>=work->instanceGpuRows.size() || !work->instanceGpuRows[ordinal][3])return 3;
        const instance_snapshot_view::NativeCompletion completion{eye.recording,eye.sourceQueue,eye.fence,eye.signaledValue,
            eye.submitted && eye.submissionCertified};
        const bool viewReady=eye.submitted?instance_snapshot_view::ValidateNativeViewAfterSubmission(*source.instanceView,completion):
            instance_snapshot_view::ValidateNativeViewForRecording(*source.instanceView,
                reinterpret_cast<ID3D12GraphicsCommandList*>(saved[6]),eye.recording);
        if(!viewReady)return 3;
        const auto& view=*source.instanceView;
        if(view.scope.list!=saved[6] || view.scope.generation!=saved[7] || view.scope.occurrence!=ordinal+1 || view.scope.eye!=saved[1] ||
            view.draw.IndexCountPerInstance!=saved[20] || view.draw.InstanceCount!=saved[21] || view.draw.StartIndexLocation!=saved[22] ||
            uint32_t(view.draw.BaseVertexLocation)!=saved[23] || view.draw.StartInstanceLocation!=saved[24])return 5;
        const auto desc=view.resource->GetDesc();const auto gpu=view.resource->GetGPUVirtualAddress();
        value.owned.vertices[7]={view.resource,gpu,desc.Width,D3D12_HEAP_TYPE_DEFAULT,UINT(desc.Flags),{}};
        value.owned.draw.vertices[7]=view.vertex;value.owners.push_back(view.owner);
    }else if(selection[2])return 3;
    // GPU geometry comparisons consume these private views directly. Neither
    // a predicted instance payload nor a later CPU readback supplies draw data.
    value.owned.instances.clear();value.original.instances.clear();
    value.root=source.root;value.shader=source.shader;value.cbvs=source.privateCbvs;
    value.cbvReceipt=source.cbvPrepared;
    value.tables=source.guardedBindings.tables;value.t5=source.guardedBindings.t5;
    if(work->fullT5Enabled && saved[4]==9){
        prepared_pair::Source::FullT5 full{source.fullT5Prepared,source.fullT5Private,{}};
        if(!eye.fullT5Arena || !eye.fullT5Arena->Finalized() || !full.copy.owner ||
            !structured_buffer_snapshot::RecordedView(full.copy,full.range,full.view))return 7;
        value.owners.push_back(full.copy.owner);value.owners.push_back(full.range.owner);value.fullT5=std::move(full);
    }
    value.inputChecks=source.guardedBindings.checks;
    value.owners.push_back(source.iaPrepared.owner);value.owners.push_back(source.cbvPrepared.owner);
    if(value.t5)value.owners.push_back(value.t5->owner);
    if(source.vrs){value.rateImage=source.vrs->SourceImage();value.rateCapture=source.vrs;}
    value.passDepth=source.passDepth->Targets().resources[4];
    value.passBaseline=source.passDepth;
    if(!value.passDepth || bool(value.owned.state.rate.image)!=bool(value.rateImage))return 6;
    size_t dependency{};
    for(const auto* group:{&source.finiteDependencies,&source.sparseDependencies})for(const auto& item:*group){
        if(dependency>=source.materialOwners.size() || dependency>=source.materialRows.size())return 4;
        const auto& row=work->materialRows[source.materialRows[dependency]];
        prepared_pair::Material material{item,source.materialOwners[dependency++],{}};
        if(row[8]==1){if(!row[17] || item.resource || item.counter)return 4;}
        else if(row[8]==5){if(!row[17] || !material.immutable.owner || !texture_read::ValidateImmutable(material.immutable,eye.sourceQueue.Get()))return 4;}
        else if(row[8]==2 && item.parameter==23 && item.offset==5){
            if(!value.t5 || !source.t5OriginalCertified || item.resource!=source.multilayerT5.resource)return 4;
        }else if(row[8]==2 && item.parameter==23 && (item.offset==4 || item.offset==6)){
            const auto found=std::find_if(source.materialBuffers.begin(),source.materialBuffers.end(),[&](const auto& buffer){return buffer.backing.shaderRegister==item.offset;});
            if(found==source.materialBuffers.end() || !material_buffer_snapshot::MatchesStagedPrivate(found->copy,item,eye.recording,eye.sourceQueue.Get()))return 4;
            material.buffer=found->copy;
        }else return 4;
        value.materials.push_back(std::move(material));
    }
    return value.materials.size()==source.materialRows.size()?0:4;
}
void BuildStagedSources()try{
    auto* stats=CyberpunkVR_RetainedStagedStats;
    if(!work || work->failed || stats[0] || CyberpunkVR_RetainedGuardedStats[0]!=2)return;
    stats[0]=1;stats[1]=work->frame;stats[2]=work->sources.size();stats[7]=work->payloadsProcessed;
    stats[8]=work->cbvPayloadComplete;stats[9]=work->overlayStage;
    if(stats[7] || stats[8] || stats[9])throw std::logic_error("Staged packets were built after CPU readback");
    work->stagedRows.reserve(work->sources.size());
    for(size_t ordinal=0;ordinal<work->sources.size();++ordinal){const auto& saved=work->sourceRows[ordinal];prepared_pair::Source value;
        const auto reason=StagePairSource(ordinal,value);
        work->stagedRows.push_back({ordinal,saved[1],reason==0,reason,saved[6],saved[7],ordinal+1,reason?0:value.materials.size()});
        if(reason){++stats[4];continue;}
        ++stats[3];stats[5]+=value.materials.size();stats[6]+=value.inputChecks.size();work->sources[ordinal].stagedPair=std::move(value);
    }
    stats[0]=2;CyberpunkVR_RetainedStagedRows=reinterpret_cast<uintptr_t>(work->stagedRows.data());
    CyberpunkVR_RetainedStagedCount=UINT(work->stagedRows.size());
}catch(...){CyberpunkVR_RetainedStagedStats[0]=3;Fail(2420);}
void ValidateStagedSources(){
    for(const auto& source:work->sources){
        const bool completedCandidate=source.privateGeometry && source.materialInputsReady && !source.command.indirect && source.shader &&
            !source.bindingTables.empty() && source.passDepth && !source.guardedBindings.tables.empty() && !source.guardedBindings.checks.empty();
        if(completedCandidate && !source.stagedPair)throw std::logic_error("Early packet construction lost a completed source candidate");
        if(!source.stagedPair)continue;
        ++CyberpunkVR_RetainedStagedStats[10];
        if(!source.privateGeometry || !source.materialInputsReady || source.bindingTables.empty())throw std::logic_error("Staged packet failed independent completion validation");
        const auto& staged=source.stagedPair->owned;const auto& ready=*source.privateGeometry;
        const auto& a=staged.draw;const auto& b=ready.draw;
        if(a.frame!=b.frame || a.side!=b.side || a.pipeline!=b.pipeline || a.root!=b.root || a.flags!=b.flags ||
            a.inputSlotMask!=b.inputSlotMask || a.indices!=b.indices || a.instances!=b.instances || a.firstIndex!=b.firstIndex ||
            a.baseVertex!=b.baseVertex || a.firstInstance!=b.firstInstance ||
            std::memcmp(&a.index,&b.index,sizeof(a.index)) || std::memcmp(a.vertices.data(),b.vertices.data(),sizeof(a.vertices)) ||
            std::memcmp(&a.viewport,&b.viewport,sizeof(a.viewport)) || std::memcmp(&a.scissor,&b.scissor,sizeof(a.scissor)) ||
            staged.index.resource!=ready.index.resource || staged.index.gpu!=ready.index.gpu || staged.index.bytes!=ready.index.bytes)
            throw std::logic_error("Staged geometry differs from independently completed geometry");
        for(UINT slot=0;slot<16;++slot)if(a.inputSlotMask&(1u<<slot)){
            const auto& x=staged.vertices[slot];const auto& y=ready.vertices[slot];
            if(x.resource!=y.resource || x.gpu!=y.gpu || x.bytes!=y.bytes)throw std::logic_error("Staged vertex ownership changed");
        }
        for(const auto& material:source.stagedPair->materials)if(material.buffer && !material_buffer_snapshot::MatchesPrivate(*material.buffer,material.binding))
            throw std::logic_error("Staged material did not complete");
        ++CyberpunkVR_RetainedStagedStats[11];
    }
}
void StartCurrentSources(ID3D12CommandQueue* queue)try{
    if(!work || !work->currentEnabled || work->currentStarted || work->failed || CyberpunkVR_RetainedStagedStats[0]!=2)return;
    auto* stats=CyberpunkVR_RetainedCurrent;stats[1]=work->frame;stats[2]=CyberpunkVR_RetainedStagedStats[3];
    stats[3]=!work->eyes[0].submitted || !work->eyes[1].submitted;
    stats[4]=work->eyes[0].submitted;stats[5]=work->eyes[1].submitted;
    stats[6]=work->payloadsProcessed;stats[7]=work->cbvPayloadComplete;stats[8]=work->overlayStage;
    stats[12]=reinterpret_cast<uintptr_t>(queue);
    if(!queue || !stats[3] || stats[6] || stats[7] || stats[8])throw std::logic_error("Current packet missed its original source submission");
    for(const auto& eye:work->eyes)if(!eye.sealed || !eye.submissionCertified || eye.sourceQueue.Get()!=queue)
        throw std::logic_error("Current source queues or submission certificates differ");
    std::array<std::shared_ptr<prepared_pair::NativeExit>,2> exits;
    if(work->tailEnabled){stats[2]=0;
        if(work->sourceFailed || work->sources.size()!=work->sourceRows.size())throw std::logic_error("Native tail source stream is incomplete");
        for(UINT side=0;side<2;++side){const auto& eye=work->eyes[side];auto* tail=CyberpunkVR_RetainedTail+side*10;tail[9]=1;
            if(!eye.tailBaseline || !eye.tailBaseline->Captured() || eye.tailFirst==UINT64_MAX || !eye.readback ||
                eye.targets.colors!=3 || (eye.sealReason!=0 && eye.sealReason!=11))throw std::logic_error("Tail has no exact baseline or native exit");
            tail[4]=0;
            for(size_t index=0;index<work->sources.size();++index)if(work->sourceRows[index][1]==side && index>=eye.tailFirst){
                const auto& saved=work->sourceRows[index];const auto& source=work->sources[index];
                if(saved[2] || saved[4]!=9 || saved[44] || !source.stagedPair || source.command.indirect)
                    throw std::logic_error("Native tail contains an excluded or non-stage9 draw");
                if(!tail[4])tail[2]=index;tail[3]=index;++tail[4];
            }
            if(!tail[4] || tail[2]!=eye.tailFirst)throw std::logic_error("Native tail is empty or starts after its baseline");
            auto output=std::make_shared<prepared_pair::NativeExit>();output->readback=eye.readback;
            const auto desc=eye.targets.resources[4]->GetDesc();output->width=UINT(desc.Width);output->height=desc.Height;
            output->eye=EyeIndex(side);output->sourceSide=side;output->frame=work->frame;
            output->list=eye.recording.list;output->generation=eye.recording.generation;
            for(const auto& plane:eye.planes)output->planes.push_back({plane.slot,plane.plane,plane.rows,plane.stride,plane.component,plane.bytes,plane.layouts[0]});
            exits[side]=std::move(output);tail[8]=1;tail[9]=0;stats[2]+=tail[4];
        }
    }
    std::vector<prepared_pair::Source> sources;sources.reserve(size_t(stats[2]));
    for(const auto& source:work->sources)if(source.stagedPair){auto value=*source.stagedPair;
        if(work->tailEnabled){const UINT side=value.owned.draw.side;const auto& eye=work->eyes[side];
            if(value.ordinal<eye.tailFirst)continue;
            value.passBaseline=eye.tailBaseline;value.passDepth=eye.tailBaseline->Targets().resources[4];value.nativeExit=exits[side];
            value.allowVrsRefinement=work->allowVrsRefinement;
        }
        value.referenceTables=value.tables;sources.push_back(std::move(value));}
    if(sources.size()!=stats[2] || !prepared_pair::StartCurrentSequence(work->device.Get(),std::move(sources),queue,work->frame))
        throw std::logic_error("Current prepared sequence could not be recorded");
    work->currentStarted=true;
}catch(...){CyberpunkVR_RetainedCurrent[15]=2430;Fail(2430);if(work)prepared_pair::CancelCurrent(work->frame);}
void ObserveStagedResidency(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists)try{
    auto* stats=CyberpunkVR_RetainedStagedResidency;
    if(!work || work->failed || stats[0] || CyberpunkVR_RetainedStagedStats[0]!=2)return;
    stats[0]=1;stats[1]=work->frame;stats[2]=CyberpunkVR_RetainedStagedStats[3];stats[8]=count;
    for(UINT side=0;side<2;++side)for(UINT i=0;i<count;++i)
        if(reinterpret_cast<uintptr_t>(lists[i])==work->eyes[side].recording.list)stats[7]|=1u<<side;
    // Earlier subsets have a different native use fence. They are deliberately
    // excluded from this same-invocation observation, even on the same queue.
    if(stats[7]!=3){stats[0]=4;return;}
    queue_residency::Scope scope;
    if(!queue_residency::Capture(queue,count,lists,scope)){stats[0]=3;stats[6]=queue_residency::LastFailure();return;}
    std::map<std::pair<UINT,uint32_t>,uint64_t> seen;
    for(const auto& source:work->sources)if(source.stagedPair){const auto& packet=*source.stagedPair;const UINT side=packet.owned.draw.side;
        ++stats[3];
        for(const auto& material:packet.materials)if(material.immutable.owner){const auto& texture=material.immutable.source.native;
            const auto [where,inserted]=seen.emplace(std::make_pair(side,texture.handle),texture.resource);
            if(!inserted){if(where->second!=texture.resource){stats[0]=3;stats[6]=10;return;}continue;}
            ++stats[4];
            if(!texture_read::ValidateImmutable(material.immutable,queue)){
                stats[0]=3;stats[6]=100+texture_read::LastFailure();return;}
            if(!queue_residency::Contains(scope,work->eyes[side].recording,texture.registry,texture.handle,texture.resource)){
                stats[0]=3;stats[6]=queue_residency::LastFailure();return;}
            ++stats[5];
        }
    }
    stats[9]=queue_residency::Validate(scope);stats[10]=work->frame;
    if(!stats[9]){stats[0]=3;stats[6]=queue_residency::LastFailure();return;}
    stats[0]=2;
}catch(...){CyberpunkVR_RetainedStagedResidency[0]=3;CyberpunkVR_RetainedStagedResidency[6]=9;}
void BuildSourceAssemblies()try{
    if(!work->sourceSelectors || work->assemblyComplete)return;
    using Failure=source_binding_assembly::Failure;
    work->assemblyCommands.reserve(std::min<size_t>(work->sources.size(),SourceAssemblyCommandLimit));
    work->assemblyTables.reserve(std::min<size_t>(work->sources.size()*64,SourceAssemblyDetailLimit));
    work->assemblyMissingCbvs.reserve(std::min<size_t>(work->sources.size()*16,SourceAssemblyDetailLimit));
    for(size_t sourceIndex=0;sourceIndex<work->sources.size();++sourceIndex){
        if(work->assemblyCommands.size()>=SourceAssemblyCommandLimit || sourceIndex>=work->sourceRows.size()){
            work->assemblyTruncated=true;break;
        }
        auto& source=work->sources[sourceIndex];const auto& saved=work->sourceRows[sourceIndex];
        std::array<uint64_t,32> row{};row[0]=sourceIndex;row[1]=saved[1];row[8]=source.privateCbvs.size();
        row[11]=saved[6];row[12]=saved[7];row[13]=saved[0]+1;row[14]=saved[11];
        row[18]=source.tables.size();row[19]=source.selectorSlots.size();row[6]=row[18]+row[19];
        row[27]=work->assemblyTables.size();row[29]=work->assemblyMissingCbvs.size();
        Failure failure=Failure::None;
        try{
            const source_binding_assembly::Scope scope{saved[11],saved[6],saved[7],saved[0]+1,saved[0]+1,UINT(saved[1])};
            const auto tableReceipt=[&](const auto& table,UINT kind,UINT descriptors){
                row[7]+=table.count;
                if(work->assemblyTables.size()>=SourceAssemblyDetailLimit){work->assemblyTruncated=true;failure=Failure::Capacity;return;}
                work->assemblyTables.push_back({sourceIndex,saved[1],table.parameter,table.count,kind,descriptors,
                    reinterpret_cast<uintptr_t>(table.cpuHeap.Get()),reinterpret_cast<uintptr_t>(table.owner.Get()),
                    table.identity,table.version,table.sourceGpu,table.sourceHeap});
            };
            for(const auto& table:source.tables)tableReceipt(table,1,table.count);
            for(const auto& table:source.selectorSlots){row[20]+=table.slots.size();tableReceipt(table,2,UINT(table.slots.size()));}
            const auto family=families::Lookup(reinterpret_cast<ID3D12PipelineState*>(saved[10]));
            ComPtr<ID3D12VersionedRootSignatureDeserializer> reader;const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* decoded{};
            source_tables::Plan domains;
            if(saved[0]!=sourceIndex || saved[1]>=2 || !saved[6] || !saved[7] || !saved[11] || saved[44] ||
                source.root.Get()!=reinterpret_cast<ID3D12RootSignature*>(saved[11]))failure=Failure::Scope;
            else if(!family || family->sourceRoot.Get()!=source.root.Get() || !source.shader || source.shader->root.empty() ||
                FAILED(D3D12CreateVersionedRootSignatureDeserializer(source.shader->root.data(),source.shader->root.size(),IID_PPV_ARGS(&reader))) ||
                FAILED(reader->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&decoded)) ||
                !source_tables::Build(decoded->Desc_1_1,family->uses,domains))failure=Failure::Root;
            if(family && decoded){
                row[15]=family->id;
                // Report missing original CBV domains even when dynamic accesses
                // cannot be admitted. No original-safe certificate is invented.
                std::vector<std::array<UINT,6>> requirements;requirements.reserve(128);
                for(const auto& use:family->uses)if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV){
                    source_tables::Plan cbvDomain;
                    if(!use.count || use.count>128 || !source_tables::Build(decoded->Desc_1_1,{&use,1},cbvDomain)){
                        failure=Failure::Root;continue;
                    }
                    const auto require=[&](UINT parameter,UINT offset,UINT count){
                        for(UINT n=0;n<count;++n){const auto existing=std::find_if(requirements.begin(),requirements.end(),[&](const auto& value){
                            return value[0]==parameter && value[1]==offset+n;});
                            if(existing!=requirements.end()){(*existing)[5]=std::max((*existing)[5],use.bytes);continue;}
                            if(requirements.size()>=128){failure=Failure::Capacity;break;}
                            requirements.push_back({parameter,offset+n,use.visibility,use.shaderRegister+n,use.space,use.bytes});
                        }
                    };
                    for(const auto& table:cbvDomain.finite)for(const auto& slot:table.uses)require(table.parameter,slot.offset,slot.count);
                    for(const auto& deferred:cbvDomain.deferred)require(deferred.parameter,deferred.offset,deferred.use.count);
                }
                row[31]=requirements.size();
                const UINT stride=work->device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
                const bool ownsCopies=source.cbvPrepared.owner && source.cbvPrepared.list==scope.list &&
                    source.cbvPrepared.generation==scope.generation && source.cbvPrepared.occurrence==scope.occurrence;
                for(const auto& required:requirements){
                    const auto privateCbv=std::find_if(source.privateCbvs.begin(),source.privateCbvs.end(),[&](const auto& value){
                        return value.parameter==required[0] && value.offset==required[1];});
                    const bool owned=ownsCopies && privateCbv!=source.privateCbvs.end() && privateCbv->resource && privateCbv->owner;
                    if(owned && privateCbv->reflectedBytes>=required[5] && privateCbv->copiedBytes>=required[5])continue;
                    ++row[5];
                    if(work->assemblyMissingCbvs.size()>=SourceAssemblyDetailLimit){work->assemblyTruncated=true;failure=Failure::Capacity;continue;}
                    std::array<uint64_t,16> missing{sourceIndex,saved[1],required[2],required[3],required[4],required[0],required[1],required[5]};
                    missing[10]=owned?2u:1u;
                    const auto original=std::find_if(source.cbvs.begin(),source.cbvs.end(),[&](const auto& value){
                        if(required[0]>=source.bindings.tables.size() || !stride)return false;
                        const auto base=source.bindings.tables[required[0]].ptr;
                        return base && required[1]<=(UINT64_MAX-base)/stride && value.stamp.gpu==base+uint64_t(required[1])*stride &&
                            value.recording.list==scope.list && value.recording.generation==scope.generation;
                    });
                    if(original!=source.cbvs.end()){
                        missing[8]=original->descriptorBytes;missing[11]=reinterpret_cast<uintptr_t>(original->resource.Get());
                        missing[12]=original->stamp.gpu;missing[13]=original->stamp.heap;
                        D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
                        if(original->resource && SUCCEEDED(original->resource->GetHeapProperties(&heap,&flags))){
                            missing[9]=UINT(heap.Type);missing[15]=heap.Type==D3D12_HEAP_TYPE_UPLOAD || heap.Type==D3D12_HEAP_TYPE_GPU_UPLOAD;
                        }
                    }
                    work->assemblyMissingCbvs.push_back(missing);
                }
                auto& eye=work->eyes[UINT(saved[1])];
                row[10]=eye.cbvArena && !eye.cbvArenaAborted && eye.cbvArena->Finalized();
                const bool dynamic=std::any_of(domains.deferred.begin(),domains.deferred.end(),[](const auto& use){return use.use.count>1;});
                const auto selector=std::find_if(work->selectorCommands.begin(),work->selectorCommands.end(),[&](const auto& command){
                    return command[0]==sourceIndex && command[1]==saved[1];});
                source_binding_assembly::Input input;input.scope=scope;input.root=&decoded->Desc_1_1;input.shaderUses=family->uses;
                input.family={family->rootSha256,family->vertexSha256,family->pixelSha256};input.selectorScope=scope;
                if(selector!=work->selectorCommands.end()){
                    row[16]=(*selector)[2];row[17]=(*selector)[11];
                    const auto* catalog=work->selectorCatalog?work->selectorCatalog->Find(input.family):nullptr;
                    const bool multilayer=(*selector)[25] && input.family.rootSha256==multilayer_selectors::RootSha256 &&
                        input.family.vertexSha256==multilayer_selectors::VertexSha256 && input.family.pixelSha256==multilayer_selectors::PixelSha256 &&
                        source.t5Plan.family==input.family && source.t5Plan.originalSiteCoverageComplete && (*selector)[27];
                    const bool supported=catalog && (catalog->supported || multilayer);
                    row[9]=supported && (*selector)[2]==1 && (*selector)[11] && !(*selector)[23] && (*selector)[24] &&
                        (*selector)[5]==scope.occurrence && (*selector)[13]==scope.list && (*selector)[14]==scope.generation &&
                        (*selector)[16]==scope.root && source.selectorResult.selectorEvaluationComplete && source.selectorResult.family==input.family;
                }
                if(dynamic){input.selectors=&source.selectorResult;input.dynamicCoverageCertified=row[9]!=0;}
                std::vector<source_binding_assembly::FiniteTable> finite;finite.reserve(source.tables.size());
                std::vector<source_binding_assembly::SparseTable> sparse;sparse.reserve(source.selectorSlots.size());
                std::vector<source_binding_assembly::PrivateCbv> cbvs;cbvs.reserve(source.privateCbvs.size());
                for(const auto& table:source.tables)finite.push_back({scope,&table});
                for(const auto& table:source.selectorSlots)sparse.push_back({scope,&table});
                for(const auto& cbv:source.privateCbvs)cbvs.push_back({scope,&cbv,&source.cbvPrepared,row[10]!=0});
                input.finite=finite;input.sparse=sparse;input.privateCbvs=cbvs;
                source_binding_assembly::PrivateSrv privateT5;
                if(source.t5Plan.byteExtent){
                    const source_cbv_snapshot::PrivateCbv* b4{};std::span<const uint8_t> bytes;size_t b4Row{};
                    const auto original=std::find_if(source.tables.begin(),source.tables.end(),[](const auto& table){return table.parameter==23;});
                    if(work->overlayStage!=2 || !source.t5View || !source.t5OriginalCertified || original==source.tables.end() ||
                        !OverlayB4(sourceIndex,b4,bytes,b4Row))failure=Failure::SrvOwner;
                    else{
                        privateT5.scope=scope;privateT5.parameter=23;privateT5.offset=5;privateT5.value=&*source.t5View;
                        privateT5.original=source.multilayerT5;privateT5.originalOwner=original->owner;
                        privateT5.originalIdentity=original->identity;privateT5.originalVersion=original->version;privateT5.originalCount=original->count;
                        privateT5.originalGpu=original->sourceGpu;privateT5.originalHeap=original->sourceHeap;privateT5.resourceCookie=source.multilayerT5.description[2];
                        privateT5.originalSlotCertified=true;privateT5.privateB4=b4;privateT5.privateB4Copies=&source.cbvPrepared;
                        privateT5.privateB4Bytes=bytes;privateT5.privateB4BytesCertified=true;input.privateSrvs={&privateT5,1};
                    }
                }
                if(failure==Failure::None && (!work->cbvPayloadComplete || work->cbvSnapshotsFailed ||
                    (!source.privateCbvs.empty() && !row[10])))failure=Failure::NotReady;
                if(failure==Failure::None){
                    auto& plan=source.bindingPlan;
                    row[2]=source_binding_assembly::Build(input,plan,failure);row[4]=row[2] && plan.Ready();
                    if(row[2] && plan.MissingCbvs().size()!=row[5]){failure=Failure::CbvOwner;row[4]=0;}
                }
            }
        }catch(...){failure=Failure::Exception;work->assemblyTruncated=true;source.bindingTables.clear();row[2]=row[4]=row[21]=0;}
        row[3]=UINT(failure);row[28]=work->assemblyTables.size()-row[27];row[30]=work->assemblyMissingCbvs.size()-row[29];
        // These CPU tables are deliberately not consumed by a bank or draw.
        row[22]=row[23]=row[24]=row[25]=0;work->assemblyCommands.push_back(row);
    }
    std::vector<const source_binding_assembly::Plan*> plans;std::vector<size_t> indices;
    for(size_t n=0;n<work->assemblyCommands.size();++n)if(work->assemblyCommands[n][4]){
        indices.push_back(n);plans.push_back(&work->sources[n].bindingPlan);
    }
    if(!plans.empty()){
        std::vector<std::vector<SharedBankTable>> tables;Failure failure{};
        const auto result=source_binding_assembly::AssemblePass(work->device.Get(),plans,work->assemblyCache,tables,failure);
        if(SUCCEEDED(result) && tables.size()!=indices.size()){work->assemblyTruncated=true;throw std::logic_error("Pass assembly source count changed");}
        for(size_t n=0;n<indices.size();++n){auto& row=work->assemblyCommands[indices[n]];row[26]=uint32_t(result);row[3]=UINT(failure);
            if(SUCCEEDED(result)){
                auto& source=work->sources[indices[n]];source.bindingTables=std::move(tables[n]);row[21]=1;
                for(auto& overlay:work->overlayRows)if(overlay[0]==indices[n])overlay[28]=1;
                for(size_t at=size_t(row[27]);at<size_t(row[27]+row[28]);++at){auto& table=work->assemblyTables[at];
                    const auto assembled=std::find_if(source.bindingTables.begin(),source.bindingTables.end(),[&](const auto& value){return value.parameter==table[2];});
                    if(assembled!=source.bindingTables.end())table[12]=reinterpret_cast<uintptr_t>(assembled->cpuHeap.Get());
                }
            }
        }
        work->assemblyDescriptors=work->assemblyCache.UniqueDescriptorCount();
        if(SUCCEEDED(result)){
            std::vector<SourceBankDraw> banks;banks.reserve(indices.size());
            for(const auto index:indices){const auto& source=work->sources[index];
                banks.push_back({source.root,source.shader->root,source.bindingTables});}
            work->assemblyBankAttempted=true;
            auto bank=std::make_unique<StereoBindingBank>();
            work->assemblyBankResult=bank->CreateSourcePass(work->device.Get(),banks);
            const auto rejected=bank->LastSourcePassFailure();
            work->assemblyBankFailure={rejected.stage,rejected.input,rejected.parameter,rejected.bits};
            if(SUCCEEDED(work->assemblyBankResult)){
                work->assemblyBank=std::move(bank);
                for(const auto index:indices){auto& row=work->assemblyCommands[index];row[22]=1;
                    for(size_t at=size_t(row[27]);at<size_t(row[27]+row[28]);++at)work->assemblyTables[at][13]=1;}
            }
        }
    }
    work->assemblyComplete=true;
}catch(...){work->assemblyTruncated=true;work->assemblyComplete=true;}
void Poll(){
    if(!work || work->complete)return;
    for(const auto& value:work->eyes)if(!value.submitted || !value.submissionCertified || !value.sourceQueue || !value.fence ||
        !value.signaledValue || value.fence->GetCompletedValue()<value.signaledValue)return;
    if(!work->payloadsProcessed){
    for(UINT side=0;side<2;++side){auto& eye=work->eyes[side];if(!eye.instanceReadback || !eye.instanceReadbackBytes)continue;
        void* mapped{};const D3D12_RANGE range{0,SIZE_T(eye.instanceReadbackBytes)};
        if(FAILED(eye.instanceReadback->Map(0,&range,&mapped)) || !mapped){Fail(1801);continue;}
        for(auto& row:work->instanceGpuRows)if(row[1]==side && row[2] && row[3] && row[4]){
            if(row[10]>eye.instanceReadbackBytes || row[11]>eye.instanceReadbackBytes-row[10] || !row[17] ||
                row[17]-1>work->instanceBlob.size() || row[11]>work->instanceBlob.size()-(row[17]-1)){Fail(1802);continue;}
            const auto* bytes=static_cast<const uint8_t*>(mapped)+row[10];row[18]=work->instanceGpuBlob.size()+1;
            work->instanceGpuBlob.insert(work->instanceGpuBlob.end(),bytes,bytes+row[11]);
            row[5]=std::memcmp(bytes,work->instanceBlob.data()+row[17]-1,size_t(row[11]))?2u:1u;
            row[19]=reinterpret_cast<uintptr_t>(eye.fence.Get());row[20]=eye.signaledValue;row[21]=1;
        }
        const D3D12_RANGE none{};eye.instanceReadback->Unmap(0,&none);
    }
    for(UINT side=0;side<2;++side){auto& value=work->eyes[side];
        if(value.fence->GetCompletedValue()==UINT64_MAX){Fail(90);work->complete=true;watching=false;return;}
        if(work->mode==1 && value.readback){void* mapped{};const auto size=value.readback->GetDesc().Width;const D3D12_RANGE range{0,SIZE_T(size)};
            if(FAILED(value.readback->Map(0,&range,&mapped)) || !mapped){Fail(91);continue;}
            for(const auto& plane:value.planes){const auto width=value.targets.resources[plane.slot]->GetDesc().Width;
                const auto output=side*6+(plane.slot==4?4+plane.plane:plane.slot);
                for(UINT row=0;row<plane.rows;++row){const auto* a=static_cast<const uint8_t*>(mapped)+plane.layouts[0].Offset+UINT64(row)*plane.layouts[0].Footprint.RowPitch+plane.component;
                    const auto* b=static_cast<const uint8_t*>(mapped)+plane.layouts[1].Offset+UINT64(row)*plane.layouts[1].Footprint.RowPitch+plane.component;
                    for(UINT64 pixel=0;pixel<width;++pixel){CyberpunkVR_RetainedGeometryDifferent[output]+=std::memcmp(a+pixel*plane.stride,b+pixel*plane.stride,plane.bytes)!=0;
                        CyberpunkVR_RetainedGeometryBytes[side]+=plane.bytes;}
                }
            }
            const D3D12_RANGE none{};value.readback->Unmap(0,&none);
        }
    }
    if(work->sourceCbvSnapshots && !work->cbvSnapshotsFailed){
        std::array<void*,2> mapped{};bool valid=true;
        for(UINT side=0;side<2;++side)if(work->eyes[side].cbvReadback){const D3D12_RANGE range{0,SIZE_T(work->eyes[side].cbvReadback->GetDesc().Width)};
            valid&=SUCCEEDED(work->eyes[side].cbvReadback->Map(0,&range,&mapped[side])) && mapped[side];}
        try{
            if(valid)for(auto& row:work->cbvSnapshotRows)if(row[8]==1){
                const auto side=UINT(row[1]);const auto& readback=work->eyes[side].cbvReadback;
                if(!mapped[side] || !readback || !row[29] || !row[30] || row[19]>readback->GetDesc().Width || row[20]>readback->GetDesc().Width-row[19]){valid=false;break;}
                row[31]=work->cbvBlob.size()+1;const auto* start=static_cast<const uint8_t*>(mapped[side])+row[19];
                work->cbvBlob.insert(work->cbvBlob.end(),start,start+row[20]);
            }
        }catch(...){valid=false;}
        for(UINT side=0;side<2;++side)if(mapped[side]){const D3D12_RANGE none{};work->eyes[side].cbvReadback->Unmap(0,&none);}
        if(!valid){work->cbvBlob.clear();work->cbvSnapshotsFailed=true;Fail(1121);}else work->cbvPayloadComplete=true;
        PublishSources();
    }
    if(work->sourceSelectors && work->cbvPayloadComplete){
        std::array<void*,2> mapped{};
        for(UINT side=0;side<2;++side)if(work->eyes[side].t5Readback && !work->eyes[side].t5Aborted){const auto& readback=work->eyes[side].t5Readback;
            const D3D12_RANGE range{0,SIZE_T(readback->GetDesc().Width)};
            if(FAILED(readback->Map(0,&range,&mapped[side])) || !mapped[side])Fail(1451);}
        for(auto& row:work->t5SnapshotRows)if(row[2]==1 && row[23] && row[24]){
            const auto side=UINT(row[1]);const auto& readback=work->eyes[side].t5Readback;
            if(!mapped[side] || !readback || row[17]>readback->GetDesc().Width || row[18]>readback->GetDesc().Width-row[17]){Fail(1452);continue;}
            row[27]=work->t5Blob.size()+1;const auto* bytes=static_cast<const uint8_t*>(mapped[side])+row[17];
            work->t5Blob.insert(work->t5Blob.end(),bytes,bytes+row[18]);
            const auto& candidate=work->sources[size_t(row[0])].t5Candidate;
            if(candidate && candidate.Bytes().size()==row[18])row[28]=std::memcmp(candidate.Bytes().data(),bytes,size_t(row[18]))?2u:1u;
        }
        for(UINT side=0;side<2;++side)if(mapped[side]){const D3D12_RANGE none{};work->eyes[side].t5Readback->Unmap(0,&none);}
        for(const auto& source:work->sources)for(const auto& input:source.selectorInputs){
            auto& row=work->selectorInputs[input.row];if(!input.intent)continue;
            for(size_t n=0;n<work->cbvSnapshotRows.size();++n){const auto& snapshot=work->cbvSnapshotRows[n];
                if(snapshot[0]!=row[0] || snapshot[1]!=row[1] || snapshot[2]!=row[2] || snapshot[3]!=row[3] || snapshot[4]!=row[4] || snapshot[5]!=row[5])continue;
                if(snapshot[8]==1 && snapshot[12]==row[19] && snapshot[13]==row[20] && snapshot[23]==row[17] && snapshot[24]==row[18] &&
                    snapshot[25]==row[21] && snapshot[26]==row[22] && snapshot[20]==row[6] && snapshot[29] && snapshot[30] && snapshot[31] &&
                    snapshot[31]-1<=work->cbvBlob.size() && snapshot[20]<=work->cbvBlob.size()-(snapshot[31]-1)){
                    row[14]=n+1;row[13]=std::memcmp(input.intent.Bytes().data(),work->cbvBlob.data()+snapshot[31]-1,size_t(snapshot[20]))?2u:1u;
                }
                break;
            }
        }
        for(auto& command:work->selectorCommands)if(command[2]==1){
            const auto& source=work->sources[size_t(command[0])];
            command[11]=!source.selectorInputs.empty() && !command[23] && std::all_of(source.selectorInputs.begin(),source.selectorInputs.end(),
                [&](const auto& input){return work->selectorInputs[input.row][13]==1;});
            if(command[25]){
                const auto view=std::find_if(work->t5Rows.begin(),work->t5Rows.end(),[&](const auto& row){return row[0]==command[0];});
                command[27]=source.t5Row!=SIZE_MAX && source.t5Row<work->t5SnapshotRows.size() &&
                    view!=work->t5Rows.end() && (*view)[29] &&
                    ((!source.t5Plan.byteExtent && work->t5SnapshotRows[source.t5Row][2]==2) || work->t5SnapshotRows[source.t5Row][28]==1);
                command[11]&=command[27];
            }
        }
        for(auto& row:work->t5Rows){const auto& source=work->sources[size_t(row[0])];
            row[28]=!source.selectorInputs.empty() && work->selectorInputs[source.selectorInputs[0].row][13]==1 && row[19];}
        // A byte match is separate from frozen producer/order certification.
        // Do not freeze the unrelated startup ledger or permit a native route.
    }
    if(work->fullT5Enabled){
        auto* stats=CyberpunkVR_RetainedFullT5Stats;
        for(UINT side=0;side<2;++side){auto& eye=work->eyes[side];void* original{};void* copied{};
            bool valid=eye.fullT5Arena && eye.fullT5Arena->Finalized() && eye.fullT5OriginalReadback && eye.fullT5PrivateReadback;
            const D3D12_RANGE originalRange{0,SIZE_T(eye.fullT5ReferenceBytes)},privateRange{0,SIZE_T(eye.fullT5Arena?eye.fullT5Arena->Bytes():0)};
            if(valid)valid=SUCCEEDED(eye.fullT5OriginalReadback->Map(0,&originalRange,&original)) && original;
            if(valid)valid=SUCCEEDED(eye.fullT5PrivateReadback->Map(0,&privateRange,&copied)) && copied;
            for(auto& row:work->fullT5Rows)if(row[1]==side){
                if(!valid || row[8]>privateRange.End || row[9]>privateRange.End-row[8] || row[12]>originalRange.End ||
                    row[9]>originalRange.End-row[12] || row[14]!=1){valid=false;break;}
                const auto* a=static_cast<const uint8_t*>(original)+row[12];const auto* b=static_cast<const uint8_t*>(copied)+row[8];
                uint64_t differences{};
                if(std::memcmp(a,b,SIZE_T(row[9])))for(uint64_t i=0;i<row[9];++i)if(a[i]!=b[i]){
                    if(!differences)row[15]=i;++differences;
                }
                row[14]=differences?3:2;stats[10]+=differences;++stats[9];
            }
            const D3D12_RANGE none{};if(original)eye.fullT5OriginalReadback->Unmap(0,&none);if(copied)eye.fullT5PrivateReadback->Unmap(0,&none);
            if(!valid){if(!stats[11])stats[11]=6;Fail(2706);}
        }
        if(stats[10] || !stats[3] || stats[9]!=stats[3]){if(!stats[11])stats[11]=7;Fail(2707);}
    }
    work->payloadsProcessed=true;
    }
    if(work->failed){work->complete=true;watching=false;PublishSources();return;}
    if(!PrepareT5Overlay()){
        if(work->overlayStage==3){work->complete=true;watching=false;}
        PublishSources();return;
    }
    if(!PrepareInstanceOverlay()){
        if(work->instanceOverlayStage==3){work->complete=true;watching=false;}
        PublishSources();return;
    }
    try{BuildPrivateGeometry();}catch(...){Fail(1820);}
    FinalizeMaterialBufferCopies();
    BuildSourceAssemblies();
    FinalizeMaterialResources();
    try{ValidateStagedSources();}catch(...){Fail(2421);}
    if(std::any_of(std::begin(CyberpunkVR_RetainedGeometryDifferent),std::end(CyberpunkVR_RetainedGeometryDifferent),[](auto v){return v!=0;}))Fail(92);
    if(!work->failed){PublishShaders();if(work->mode==1){verified=work->pipelines;CyberpunkVR_RetainedGeometryVerified=1;}CyberpunkVR_RetainedGeometryState=3;}
    work->complete=true;watching=false;PublishSources();
}
}
bool Active(){return watching.load(std::memory_order_relaxed) || CyberpunkVR_RetainedGeometryRequest.load(std::memory_order_relaxed)!=0 ||
    CyberpunkVR_PreparedPairRequest.load(std::memory_order_relaxed)!=0 || prepared_pair::Active();}
bool InRenderPass(ID3D12GraphicsCommandList* list){std::lock_guard lock(mutex);return renderPasses.contains(list);}
void FrameBoundary(){
    prepared_pair::Poll();
    if(!Active() && CyberpunkVR_RetainedGeometryState.load()!=2)return;
    std::lock_guard lock(mutex);
    // The owned fence can finish while DeviceHooks is still executing the
    // native suffix. Keep this capture identity until that whole batch returns.
    if(work && work->infixInFlight)return;
    if(work && watching && !CyberpunkVR_RetainedGeometryState.load() &&
        std::none_of(work->eyes.begin(),work->eyes.end(),[](const auto& eye){return eye.started && !eye.sealed;})){
        work->complete=true;watching=false;target_handoff::FinishObservation();target_access::Finish();
    }
    Poll();
    if(work && work->complete)ReleaseBorrowedObservers();
    if(work)DisarmFinishedFrontierObserver(*work);
    const auto pairRequest=CyberpunkVR_PreparedPairRequest.exchange(0);
    if(pairRequest && !prepared_pair::Active()){
        bool started{},selected{};UINT selection{};
        CyberpunkVR_PreparedPairSelectionCount=0;
        std::fill_n(CyberpunkVR_PreparedPairCandidates,16,0ull);
        std::fill_n(CyberpunkVR_PreparedPairStateMismatch,24,0ull);
        std::fill_n(CyberpunkVR_PreparedPairGpu,36,0ull);
        std::fill_n(CyberpunkVR_PreparedPairColor,12,0ull);
        std::fill_n(CyberpunkVR_PreparedPairDepthState,16,0ull);
        std::fill_n(CyberpunkVR_PreparedPairMeshStats,12,0ull);std::fill_n(CyberpunkVR_PreparedPairMeshResults,512,0ull);
        std::fill_n(CyberpunkVR_PreparedPairVrs,8,0ull);
        std::fill_n(CyberpunkVR_PreparedBatchStats,16,0ull);
        CyberpunkVR_PreparedBatchSourceCount=CyberpunkVR_PreparedBatchPlanCount=0;
        if(work && work->complete && !work->failed){
            CyberpunkVR_PreparedPairCandidates[0]=work->sources.size();
            std::vector<prepared_pair::Source> candidates;
            for(size_t ordinal=0;ordinal<work->sources.size();++ordinal){const auto& source=work->sources[ordinal];
                if(!source.stagedPair || !source.privateGeometry || !source.materialInputsReady || source.bindingTables.empty())continue;
                ++CyberpunkVR_PreparedPairCandidates[1];
                auto value=*source.stagedPair;
                ++CyberpunkVR_PreparedPairCandidates[2];
                value.referenceTables=source.bindingTables;
                candidates.push_back(std::move(value));
            }
            if(pairRequest==2){selected=true;started=prepared_pair::StartSequence(work->device.Get(),std::move(candidates));selection=1;}
            else{const auto requested=CyberpunkVR_PreparedPairSelection.load(std::memory_order_relaxed);
            for(const auto& left:candidates){if(left.owned.draw.side!=1)continue;
                for(const auto& right:candidates){if(right.owned.draw.side!=0)continue;
                    const auto why=prepared_pair::CompatibilityFailure(left,right);
                    if(why){++CyberpunkVR_PreparedPairCandidates[why>=10?4:3+why];continue;}
                    ++CyberpunkVR_PreparedPairCandidates[3];
                    const auto family=families::Lookup(left.owned.state.original.Get());
                    if(!family || !family->pipeline || !family->pairedRoot){++CyberpunkVR_PreparedPairCandidates[13];continue;}
                    if(!family->instanceRebaseSafe){++CyberpunkVR_PreparedPairCandidates[14];continue;}
                    if(!family->pass.depthStateKnown){++CyberpunkVR_PreparedPairCandidates[15];continue;}
                    if(selection++!=requested)continue;
                    selected=true;
                    if(prepared_pair::Start(work->device.Get(),{left,right},family)){started=true;break;}}
                if(started)break;}}
        }
        CyberpunkVR_PreparedPairSelectionCount=selection;
        if(!started){std::fill_n(CyberpunkVR_PreparedPairStats,16,0ull);CyberpunkVR_PreparedPairStats[10]=selected?27:24;CyberpunkVR_PreparedPairState=6;}
    }
    if(prepared_pair::Active())return;
    const auto request=CyberpunkVR_RetainedGeometryRequest.exchange(0);if(!request)return;
    if(work && !work->complete && (work->eyes[0].started || work->eyes[1].started)){Fail(1);return;}
    std::unique_ptr<Work> pending;
    try{pending=std::make_unique<Work>();pending->mode=request;pending->stage=CyberpunkVR_RetainedGeometryStage.load();
        pending->sourceStream=CyberpunkVR_RetainedSourceStreamEnable.load()!=0;
        pending->sourceSnapshots=CyberpunkVR_RetainedIndirectSnapshotsEnable.load()!=0;
        pending->sourceCbvs=CyberpunkVR_RetainedSourceCbvEnable.load()!=0;
        pending->sourceTables=CyberpunkVR_RetainedSourceTablesEnable.load()!=0;
        pending->sourceDetails=CyberpunkVR_RetainedSourceDetailsEnable.load()!=0;
        pending->sourceCbvSnapshots=CyberpunkVR_RetainedSourceCbvSnapshotsEnable.load()!=0;
        pending->sourceIaSnapshots=CyberpunkVR_NativeIaSnapshotsEnable.load()!=0;
        LatchFrontierObserver(*pending);
        pending->sceneAccessObserver=CyberpunkVR_SceneAccessEnable.load()==1;
        if(CyberpunkVR_SceneAccessEnable.load()>1 || (pending->sceneAccessObserver && CyberpunkVR_OwnedSceneSplitEnable.load()!=2))
            throw std::invalid_argument("Scene access capture requires both native scene sections");
        pending->infixEnabled=CyberpunkVR_PreparedInfixEnable.load()!=0;
        if(pending->infixEnabled && !pending->currentEnabled)throw std::invalid_argument("Infix consumer requires the owned current path");
        pending->tailEnabled=CyberpunkVR_PreparedTailEnable.load()!=0;
        if(pending->tailEnabled && !pending->infixEnabled)throw std::invalid_argument("Native tail requires certified prefix insertion");
        pending->fullT5Enabled=CyberpunkVR_RetainedFullT5Enable.load()!=0;
        if(pending->fullT5Enabled && !pending->tailEnabled)throw std::invalid_argument("Full T5 requires the complete native tail");
        pending->borrowedTablesEnabled=CyberpunkVR_RetainedBorrowedTablesEnable.load()!=0;
        if(pending->borrowedTablesEnabled && !pending->fullT5Enabled)throw std::invalid_argument("Borrowed tables require complete T5 and native tail capture");
        pending->fixedAssetsEnabled=CyberpunkVR_RetainedFixedAssetsEnable.load()!=0;
        if(pending->fixedAssetsEnabled && !pending->borrowedTablesEnabled)throw std::invalid_argument("Fixed asset observation requires borrowed tables");
        pending->allowVrsRefinement=CyberpunkVR_PreparedVrsRefinementEnable.load()!=0;
        if(pending->allowVrsRefinement && !pending->tailEnabled)throw std::invalid_argument("VRS refinement policy requires native tail comparison");
        pending->sourceSelectors=pending->sourceCbvSnapshots && constants::RegisteredIntentTracking();
        if(pending->currentEnabled && (request!=1 || pending->stage!=7 || !pending->sourceSelectors ||
            !pending->sourceIaSnapshots || !pending->sourceTables || !pending->sourceStream))
            throw std::invalid_argument("Current prepared sequence requires complete source-copy capture");
        if(pending->sourceSelectors){
            static const auto catalog=[]()->std::shared_ptr<const source_selectors::Catalog>{
                wchar_t path[32768]{};const auto length=GetEnvironmentVariableW(L"CVR_STEREO_SOURCE_SELECTORS",path,32768);
                if(!length || length>=32768)return {};
                auto result=std::make_shared<source_selectors::Catalog>();std::string error;
                return source_selectors::LoadCatalogFile(path,*result,error)?result:nullptr;
            }();
            if(!catalog)throw std::invalid_argument("Source selector catalog is invalid");pending->selectorCatalog=catalog;
        }
        if(pending->sourceCbvSnapshots && (!pending->sourceStream || !pending->sourceCbvs || !pending->sourceTables || request!=1))
            throw std::invalid_argument("DEFAULT CBV snapshots require owned source CBVs/tables and original reference stream");
        if(pending->sourceDetails && (!pending->sourceStream || !pending->sourceCbvs || request!=1))throw std::invalid_argument("Source details require current source CBVs and original reference stream");
        if(pending->sourceTables && (!pending->sourceStream || !pending->sourceCbvs || request!=1))throw std::invalid_argument("Source tables require owned source CBVs and original reference stream");
        if(pending->sourceSnapshots && (!pending->sourceStream || request!=1))throw std::invalid_argument("Indirect snapshots require the original source reference stream");
        if(pending->sourceIaSnapshots && (!pending->sourceStream || request!=1))throw std::invalid_argument("IA snapshots require the original source reference stream");
        if(pending->sourceStream){pending->sources.reserve(StereoGeometryCommands::MaxSourceCommands);pending->sourceRows.reserve(StereoGeometryCommands::MaxSourceCommands);
            pending->indirectRows.reserve(StereoGeometryCommands::MaxSourceCommands);pending->snapshotRows.reserve(StereoGeometryCommands::MaxSourceCommands);
            if(pending->sourceCbvs){pending->cbvRows.reserve(StereoGeometryCommands::MaxSourceCommands*16);pending->cbvDiagnostics.reserve(StereoGeometryCommands::MaxSourceCommands*16);}}
        if(pending->sourceTables){pending->tableCommands.reserve(StereoGeometryCommands::MaxSourceCommands);pending->tableRows.reserve(StereoGeometryCommands::MaxSourceCommands*32);}
        if(pending->sourceDetails)pending->detailRows.reserve(StereoGeometryCommands::MaxSourceCommands*16);
        if(pending->sourceCbvSnapshots){pending->cbvSnapshotCommands.reserve(StereoGeometryCommands::MaxSourceCommands);
            pending->cbvSnapshotRows.reserve(StereoGeometryCommands::MaxSourceCommands*16);}
        if(pending->sourceIaSnapshots){pending->iaCommands.reserve(StereoGeometryCommands::MaxSourceCommands);
            pending->iaObservations.reserve(StereoGeometryCommands::MaxSourceCommands*ia_snapshot::Observations::MaxViews);
            pending->iaViews.reserve(StereoGeometryCommands::MaxSourceCommands*ia_snapshot::Observations::MaxViews);}
    }catch(...){CyberpunkVR_SubmissionFrontierEnable=0;CyberpunkVR_RetainedGeometryState=4;
        CyberpunkVR_RetainedSourceStreamStats[0]=4;CyberpunkVR_RetainedSourceStreamStats[12]=906;watching=false;return;}
    std::fill_n(CyberpunkVR_RetainedSourceStreamStats,16,0ull);CyberpunkVR_RetainedSourceStreamRows=0;CyberpunkVR_RetainedSourceStreamCount=0;
    std::fill_n(CyberpunkVR_RetainedFullT5Stats,16,0ull);CyberpunkVR_RetainedFullT5Stats[0]=1;
    CyberpunkVR_RetainedFullT5Stats[1]=pending->fullT5Enabled;CyberpunkVR_RetainedFullT5Rows=0;CyberpunkVR_RetainedFullT5Count=0;
    std::fill_n(CyberpunkVR_RetainedBorrowedTablesStats,16,0ull);CyberpunkVR_RetainedBorrowedTablesStats[0]=1;
    CyberpunkVR_RetainedBorrowedTablesStats[1]=pending->borrowedTablesEnabled;
    CyberpunkVR_RetainedBorrowedTablesRows=CyberpunkVR_RetainedBorrowedRangeRows=0;
    CyberpunkVR_RetainedBorrowedTablesCount=CyberpunkVR_RetainedBorrowedRangeCount=0;
    CyberpunkVR_RetainedBorrowedInventoryRows=0;CyberpunkVR_RetainedBorrowedInventoryCount=0;
    std::fill_n(CyberpunkVR_RetainedBorrowedFailure,8,0ull);
    std::fill_n(CyberpunkVR_RetainedPageableUseStats,16,0ull);CyberpunkVR_RetainedPageableUseStats[0]=1;
    CyberpunkVR_RetainedPageableUseStats[1]=pending->borrowedTablesEnabled;
    std::fill_n(CyberpunkVR_RetainedPageableUseFailure,24,0ull);CyberpunkVR_RetainedPageableUseRows=0;CyberpunkVR_RetainedPageableUseCount=0;
    std::fill_n(CyberpunkVR_RetainedPageableSearch,20,0ull);
    std::fill_n(CyberpunkVR_RetainedFixedAssetsStats,16,0ull);CyberpunkVR_RetainedFixedAssetsStats[0]=1;
    CyberpunkVR_RetainedFixedAssetsStats[1]=pending->fixedAssetsEnabled;std::fill_n(CyberpunkVR_RetainedFixedAssetsCounts,10,0ull);
    CyberpunkVR_RetainedFixedAssetsRows=CyberpunkVR_RetainedFixedAssetRows=CyberpunkVR_RetainedFixedSlotRows=0;
    CyberpunkVR_RetainedFixedAssetsCount=CyberpunkVR_RetainedFixedAssetCount=CyberpunkVR_RetainedFixedSlotCount=0;
    CyberpunkVR_RetainedFixedDescriptorRows=0;CyberpunkVR_RetainedFixedDescriptorCount=0;
    CyberpunkVR_RetainedNativeTableRows=0;CyberpunkVR_RetainedNativeTableCount=0;
    CyberpunkVR_RetainedNativeTableEntries=0;CyberpunkVR_RetainedNativeTableEntryCount=0;
    CyberpunkVR_RetainedNativeTableRings=0;CyberpunkVR_RetainedNativeTableRingCount=0;
    std::fill_n(CyberpunkVR_RetainedSourceBoundaryRows,40,0ull);CyberpunkVR_RetainedSourceBoundaryCount=0;
    std::fill_n(CyberpunkVR_RetainedSourceQueueOrder,16,0ull);
    std::fill_n(CyberpunkVR_RetainedIndirectBindingsStats,16,0ull);CyberpunkVR_RetainedIndirectBindingsRows=0;CyberpunkVR_RetainedIndirectBindingsCount=0;
    std::fill_n(CyberpunkVR_RetainedIndirectSnapshotsStats,16,0ull);CyberpunkVR_RetainedIndirectSnapshotsRows=0;CyberpunkVR_RetainedIndirectSnapshotsCount=0;
    std::fill_n(CyberpunkVR_RetainedSourceCbvStats,16,0ull);CyberpunkVR_RetainedSourceCbvRows=0;CyberpunkVR_RetainedSourceCbvCount=0;
    CyberpunkVR_RetainedSourceCbvProducerRows=0;CyberpunkVR_RetainedSourceCbvProducerCount=0;
    CyberpunkVR_RetainedSelectorCoverageRows=0;CyberpunkVR_RetainedSelectorCoverageCount=0;
    CyberpunkVR_RetainedSelectorCacheEventRows=0;CyberpunkVR_RetainedSelectorCacheEventCount=0;
    CyberpunkVR_RetainedSourceCbvDiagnostics=0;
    std::fill_n(CyberpunkVR_RetainedSourceTablesStats,16,0ull);CyberpunkVR_RetainedSourceTablesRows=0;CyberpunkVR_RetainedSourceTablesCount=0;
    CyberpunkVR_RetainedSourceTablesCommands=0;CyberpunkVR_RetainedSourceTablesCommandCount=0;
    std::fill_n(CyberpunkVR_RetainedSourceDetailsStats,16,0ull);CyberpunkVR_RetainedSourceDetailsRows=0;CyberpunkVR_RetainedSourceDetailsCount=0;
    std::fill_n(CyberpunkVR_RetainedSourceCbvSnapshotsStats,16,0ull);CyberpunkVR_RetainedSourceCbvSnapshotsRows=0;CyberpunkVR_RetainedSourceCbvSnapshotsCount=0;
    CyberpunkVR_RetainedSourceCbvSnapshotsCommands=0;CyberpunkVR_RetainedSourceCbvSnapshotsCommandCount=0;
    CyberpunkVR_RetainedSourceCbvCopyOwnerRows=0;CyberpunkVR_RetainedSourceCbvCopyOwnerCount=0;
    CyberpunkVR_RetainedSourceCbvSnapshotsBlob=0;CyberpunkVR_RetainedSourceCbvSnapshotsBlobBytes=0;
    std::fill_n(CyberpunkVR_RetainedSourceSelectorsStats,16,0ull);
    CyberpunkVR_RetainedSourceSelectorsCommands=CyberpunkVR_RetainedSourceSelectorsInputs=CyberpunkVR_RetainedSourceSelectorsSlots=CyberpunkVR_RetainedSourceSelectorsT5=CyberpunkVR_RetainedSourceSelectorsBlob=0;
    CyberpunkVR_RetainedSourceSelectorsCommandCount=CyberpunkVR_RetainedSourceSelectorsInputCount=CyberpunkVR_RetainedSourceSelectorsSlotCount=CyberpunkVR_RetainedSourceSelectorsT5Count=CyberpunkVR_RetainedSourceSelectorsBlobBytes=0;
    CyberpunkVR_RetainedT5NativeRows=CyberpunkVR_RetainedT5SnapshotRows=CyberpunkVR_RetainedT5SnapshotBlob=CyberpunkVR_RetainedT5IntentBlob=0;
    CyberpunkVR_RetainedT5NativeCount=CyberpunkVR_RetainedT5SnapshotCount=CyberpunkVR_RetainedT5SnapshotBlobBytes=CyberpunkVR_RetainedT5IntentBlobBytes=0;
    CyberpunkVR_RetainedT5CopyDiagnosticRows=CyberpunkVR_RetainedT5MarkDiagnosticRows=0;CyberpunkVR_RetainedT5DiagnosticCount=0;
    CyberpunkVR_RetainedT5PieceRows=0;CyberpunkVR_RetainedT5PieceCount=0;
    CyberpunkVR_RetainedT5CoverageRows=0;CyberpunkVR_RetainedT5CoverageCount=0;
    std::fill_n(CyberpunkVR_RetainedSourceAssemblyStats,16,0ull);
    std::fill_n(CyberpunkVR_RetainedSourceAssemblyCacheStats,4,0ull);
    std::fill_n(CyberpunkVR_RetainedSourceAssemblyBankStats,6,0ull);
    std::fill_n(CyberpunkVR_RetainedSourceAssemblyBankFailure,4,0ull);
    std::fill_n(CyberpunkVR_RetainedT5OverlayStats,16,0ull);
    CyberpunkVR_RetainedT5OverlayRows=0;CyberpunkVR_RetainedT5OverlayCount=0;
    CyberpunkVR_RetainedT5OverlayBlob=0;CyberpunkVR_RetainedT5OverlayBlobBytes=0;
    CyberpunkVR_RetainedSourceAssemblyCommands=CyberpunkVR_RetainedSourceAssemblyTables=CyberpunkVR_RetainedSourceAssemblyMissingCbvs=0;
    CyberpunkVR_RetainedSourceAssemblyCommandCount=CyberpunkVR_RetainedSourceAssemblyTableCount=CyberpunkVR_RetainedSourceAssemblyMissingCbvCount=0;
    CyberpunkVR_RetainedIaCommands=CyberpunkVR_RetainedIaObservations=CyberpunkVR_RetainedIaViews=0;
    CyberpunkVR_RetainedIaCommandCount=CyberpunkVR_RetainedIaObservationCount=CyberpunkVR_RetainedIaViewCount=0;
    CyberpunkVR_RetainedInstanceRows=CyberpunkVR_RetainedInstanceBlob=0;
    CyberpunkVR_RetainedInstanceCount=CyberpunkVR_RetainedInstanceBlobBytes=0;
    CyberpunkVR_RetainedInstanceGpuRows=CyberpunkVR_RetainedInstanceGpuBlob=0;
    CyberpunkVR_RetainedInstanceGpuCount=CyberpunkVR_RetainedInstanceGpuBlobBytes=0;
    std::fill_n(CyberpunkVR_RetainedInstanceOverlayStats,8,0ull);CyberpunkVR_RetainedInstanceOverlayRows=0;CyberpunkVR_RetainedInstanceOverlayCount=0;
    CyberpunkVR_RetainedInstanceOverlayBlob=0;CyberpunkVR_RetainedInstanceOverlayBlobBytes=0;
    std::fill_n(CyberpunkVR_RetainedInstanceOverlayError,sizeof(CyberpunkVR_RetainedInstanceOverlayError),0);
    std::fill_n(CyberpunkVR_RetainedT5OverlayError,sizeof(CyberpunkVR_RetainedT5OverlayError),0);
    std::fill_n(CyberpunkVR_RetainedGuardedStats,16,0ull);CyberpunkVR_RetainedGuardedRows=0;CyberpunkVR_RetainedGuardedCount=0;
    std::fill_n(CyberpunkVR_RetainedGuardedCacheStats,4,0ull);
    std::fill_n(CyberpunkVR_RetainedStagedStats,12,0ull);CyberpunkVR_RetainedStagedRows=0;CyberpunkVR_RetainedStagedCount=0;
    std::fill_n(CyberpunkVR_RetainedStagedResidency,12,0ull);
    std::fill_n(CyberpunkVR_RetainedStagedResidencyDetails,12,0ull);
    std::fill_n(CyberpunkVR_RetainedStagedMembershipDetails,12,0ull);
    std::fill_n(CyberpunkVR_RetainedCurrent,16,0ull);CyberpunkVR_RetainedCurrent[0]=pending->currentEnabled;
    std::fill_n(CyberpunkVR_RetainedBaseline,16,0ull);
    std::fill_n(CyberpunkVR_RetainedInfix,16,0ull);CyberpunkVR_RetainedInfix[0]=pending->infixEnabled;
    std::fill_n(CyberpunkVR_RetainedTail,20,0ull);CyberpunkVR_RetainedTail[0]=CyberpunkVR_RetainedTail[10]=pending->tailEnabled;
    std::fill_n(CyberpunkVR_RetainedTargetLifetime,128,0ull);
    CyberpunkVR_RetainedIaSelections=CyberpunkVR_RetainedPrivateGeometryRows=0;
    CyberpunkVR_RetainedMaterialRows=0;CyberpunkVR_RetainedMaterialCount=0;
    CyberpunkVR_RetainedMaterialSources=0;CyberpunkVR_RetainedMaterialSourceCount=0;
    CyberpunkVR_RetainedIaSelectionCount=CyberpunkVR_RetainedPrivateGeometryCount=0;
    CyberpunkVR_RetainedGeometryShaderRows=0;CyberpunkVR_RetainedGeometryShaderCount=0;
    work=std::move(pending);
    std::fill_n(CyberpunkVR_RetainedGeometryStats,16,0ull);std::fill_n(CyberpunkVR_RetainedGeometryDifferent,12,0ull);std::fill_n(CyberpunkVR_RetainedGeometryBytes,2,0ull);
    std::fill_n(CyberpunkVR_RetainedGeometryEnds,4,0ull);
    std::fill_n(CyberpunkVR_RetainedGeometryIndirect,2,0ull);
    std::fill_n(CyberpunkVR_RetainedGeometryIndirectRejected,8,0ull);
    CyberpunkVR_RetainedGeometryShaderRows=0;CyberpunkVR_RetainedGeometryShaderCount=0;
    CyberpunkVR_RetainedGeometryStats[15]=request;
    if((request!=1 && request!=2) || (work->stage!=6 && work->stage!=7) || !native_probe::ObserveBindings() || !native_recording::Compatible() ||
        (request==2 && (!CyberpunkVR_RetainedGeometryVerified.load() || verified.empty()))){Fail(2);work->complete=true;return;}
    if(request==1){verified.clear();CyberpunkVR_RetainedGeometryVerified=0;}
    target_handoff::Clear();
    target_access::Clear();
    if(work->tailEnabled)target_access::Arm();
    ArmFrontierObserver(*work);
    if(work->sceneAccessObserver){scene_access_trace::Arm();CyberpunkVR_SceneChildEnable=1;}
    watching=true;CyberpunkVR_RetainedGeometryState=1;
}
bool Draw(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw){
    if(!probe::internalCommands && prepared_pair::Active() && !InRenderPass(list))prepared_pair::Draw(list,draw);
    if(probe::internalCommands || !Active())return false;
    const auto tag=preparation::CurrentDrawTag();std::lock_guard lock(mutex);
    if(!work || work->complete)return false;
    if(work->failed){UINT side{};if(Owned(list,side))Seal(list,side,18);return false;}
    if(draw.side>1 || !tag.scope || tag.side!=int(draw.side) || !tag.frame){UINT side{};if(Owned(list,side))Seal(list,side,1);return false;}
    const auto side=draw.side;auto& value=work->eyes[side];
    if(renderPasses.contains(list)){if(value.started)Fail(9);return false;}
    if(value.started && (value.sealed || value.recording.list!=reinterpret_cast<uintptr_t>(list)))return false;
    if(!value.started && tag.stage!=work->stage)return false;
    if(work->frame && work->frame!=tag.frame)return false;
    const auto policy=families::NativeGeometryStatus(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));scene_state::GeometryTargets targets;
    if(policy!=1 || draw.flags&64 || !scene_state::CaptureGeometryTargets(draw,targets) || targets.views[4].viewFlags){
        if(value.started)Seal(list,side,2);else Fail(policy!=1?400+policy:3);return false;
    }
    if(work->mode==2 && std::none_of(verified.begin(),verified.end(),[&](const auto& p){return p.Get()==reinterpret_cast<ID3D12PipelineState*>(draw.pipeline);})){Fail(4);if(value.started)Seal(list,side,3);return false;}
    if(value.started)for(UINT slot=0;slot<5;++slot)if((slot==4 || slot<targets.colors) && value.imported[slot] &&
        (value.targets.resources[slot]!=targets.resources[slot] || value.targets.views[slot].identity!=targets.views[slot].identity)){
        Seal(list,side,19);return false;
    }
    if(!value.started){
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        if(!native_recording::Capture(list,camera_upload::CurrentContext(base),value.recording)){Fail(100+native_recording::LastFailure());return false;}
        if(!work->created){if(FAILED(list->GetDevice(IID_PPV_ARGS(&work->device))) || FAILED(work->chain.Create(work->device.Get(),UINT(draw.viewport.Width),UINT(draw.viewport.Height)))){Fail(5);return false;}work->created=true;}
        if(FAILED(work->device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&value.fence)))){Fail(6);return false;}
        work->frame=tag.frame;value.started=true;
    }
    for(UINT slot=0;slot<5;++slot)if((slot==4 || slot<targets.colors) && !value.imported[slot]){
        if(!texture_read::ValidateWritableTarget(list,targets.resources[slot].Get(),slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET)){
            Fail(200+texture_read::LastFailure());if(CyberpunkVR_RetainedGeometryStats[side])Seal(list,side,4);else value.sealed=true;return false;}
    }
    if(work->sourceIaSnapshots && !value.imported[4]){
        // Preserve the complete pre-interval contents. Depth-only seeding cannot
        // represent a native G-buffer baseline when earlier color already exists.
        auto* baseline=CyberpunkVR_RetainedBaseline+side*8;baseline[0]=work->frame;baseline[1]=targets.colors;baseline[5]=1;
        constexpr uint64_t budget=64ull*1024*1024;
        for(UINT slot=0;slot<5;++slot)if(slot==4 || slot<targets.colors){
            auto desc=targets.resources[slot]->GetDesc();desc.Alignment=0;
            desc.Flags=slot==4?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
            const auto allocation=work->device->GetResourceAllocationInfo(0,1,&desc);
            if(!allocation.SizeInBytes || allocation.SizeInBytes>budget-baseline[2]){baseline[4]=1;break;}
            baseline[2]+=allocation.SizeInBytes;
        }
        if(!baseline[4]){
            auto checkpoint=std::make_shared<GeometryTargetCheckpoint>();
            if(FAILED(checkpoint->Create(work->device.Get(),targets)))baseline[4]=2;
            else if(!checkpoint->Capture(list,targets))baseline[4]=3;
            else{value.passDepth=std::move(checkpoint);baseline[3]=1;}
        }
        if(!baseline[3]){Fail(2440+UINT(baseline[4]));value.sealed=true;return false;}
    }
    if(work->tailEnabled && tag.stage==9 && !value.tailBaseline){
        auto* tail=CyberpunkVR_RetainedTail+side*10;tail[1]=work->frame;tail[2]=work->sources.size();tail[7]=9;
        constexpr uint64_t budget=64ull*1024*1024;
        if(targets.colors!=3){tail[9]=2;Fail(2462);value.sealed=true;return false;}
        for(UINT slot:{0u,1u,2u,4u}){
            if(!texture_read::ValidateWritableTarget(list,targets.resources[slot].Get(),slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET)){
                tail[9]=3;Fail(2463);value.sealed=true;return false;}
            auto desc=targets.resources[slot]->GetDesc();desc.Alignment=0;
            desc.Flags=slot==4?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
            const auto allocation=work->device->GetResourceAllocationInfo(0,1,&desc);
            if(!allocation.SizeInBytes || allocation.SizeInBytes>budget-tail[6]){tail[9]=4;Fail(2464);value.sealed=true;return false;}
            tail[6]+=allocation.SizeInBytes;
        }
        target_handoff::Begin(list,value.recording,work->frame,side,targets);
        target_access::MarkSource(list,value.recording,work->frame,side);
        UINT targetIndex{};
        for(UINT slot:{0u,1u,2u,4u}){
            auto* row=CyberpunkVR_RetainedTargetLifetime+(side*4+targetIndex)*16;row[0]=side;row[1]=slot;
            texture_read::WritableTargetIdentity identity;
            if(texture_read::CaptureWritableIdentity(list,targets.resources[slot].Get(),
                slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,identity)){
                row[2]=target_lifetime::Capture(identity,value.targetLifetimes[targetIndex]);
                if(!row[2])row[5]=target_lifetime::LastFailure();
                const auto details=target_lifetime::LastDetails();row[6]=details[0];row[7]=details[1];
                row[8]=details[6];row[9]=details[7];row[10]=details[3];row[11]=details[4];row[12]=details[5];
                row[13]=details[8];row[14]=details[9];row[15]=details[11];
            }else row[5]=100+texture_read::LastFailure();
            ++targetIndex;
        }
        auto checkpoint=std::make_shared<GeometryTargetCheckpoint>();
        if(FAILED(checkpoint->Create(work->device.Get(),targets)) || !checkpoint->Capture(list,targets)){
            tail[9]=5;Fail(2465);value.sealed=true;return false;}
        value.tailFirst=work->sources.size();value.tailBaseline=std::move(checkpoint);tail[5]=1;
    }
    if(FAILED(work->chain.PrepareEye(list,EyeIndex(side),targets)) || !work->chain.Bind(list,1u<<EyeIndex(side),targets.colors)){
        Fail(7);if(CyberpunkVR_RetainedGeometryStats[side])Seal(list,side,5);return false;}
    for(UINT slot=0;slot<5;++slot)if(slot==4 || slot<targets.colors){value.targets.resources[slot]=targets.resources[slot];value.targets.views[slot]=targets.views[slot];value.targets.handles[slot]=targets.handles[slot];value.imported[slot]=true;}
    value.targets.colors=targets.colors;value.colors=targets.colors;
    if(value.scope!=tag.scope){value.scope=tag.scope;++CyberpunkVR_RetainedGeometryStats[2+side];}
    if(tag.stage<64)CyberpunkVR_RetainedGeometryEnds[2+side]|=UINT64(1)<<tag.stage;
    auto* pipeline=reinterpret_cast<ID3D12PipelineState*>(draw.pipeline);
    AppendSource(list,side,draw);
    if(work->failed){if(!work->chain.RestoreNative(list,EyeIndex(side),targets.colors))Fail(8);PublishSources();return false;}
    if(std::none_of(work->pipelines.begin(),work->pipelines.end(),[&](const auto& p){return p.Get()==pipeline;}))work->pipelines.emplace_back(pipeline);
    const bool nativeTables=BindNativeSourceTables(list);
    if(work->failed){work->chain.RestoreNative(list,EyeIndex(side),targets.colors);return false;}
    {copy_windows::ReadOnlyScope readOnly;probe::Scope internal;list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);}
    if(nativeTables){probe::Scope internal;work->sources.back().bindings.Restore(list);}
    if(!work->chain.RestoreNative(list,EyeIndex(side),targets.colors))Fail(8);
    ++CyberpunkVR_RetainedGeometryStats[side];Publish();return work->mode==2;
}
void Flush(ID3D12GraphicsCommandList* list,UINT reason){
    if(probe::internalCommands || !Active())return;std::lock_guard lock(mutex);UINT side{};
    if(Owned(list,side))Seal(list,side,reason,reason==0?target_handoff::ExitKind::Close:target_handoff::ExitKind::Other);
}
bool Indirect(ID3D12GraphicsCommandList* list,ID3D12CommandSignature* signature,UINT count,ID3D12Resource* arguments,UINT64 offset,
              ID3D12Resource* countBuffer,UINT64 countOffset){
    if(probe::internalCommands || !Active())return false;
    const auto tag=preparation::CurrentDrawTag();std::lock_guard lock(mutex);UINT side{};auto* value=Owned(list,side);
    if(!value || !work || work->complete)return false;
    native_probe::DrawRecord current;scene_state::GeometryTargets targets;
    const auto stride=indirect_policy::PureDrawStride(signature);
    const bool state=native_probe::CurrentState(list,current);
    const auto policy=state?families::NativeGeometryStatus(reinterpret_cast<ID3D12PipelineState*>(current.pipeline)):0;
    const bool capture=state && scene_state::CaptureGeometryTargets(current,targets);
    // Native indirect epilogues can run outside the CPU DrawScope. The retained
    // generation, current shader and exact live target identities still apply.
    bool valid=!work->failed && stride && arguments && offset%4==0 && countOffset%4==0 &&
        (!tag.scope || (tag.frame==work->frame && tag.side==int(side))) && state && !(current.flags&64) && policy==1 && capture && !targets.views[4].viewFlags;
    auto reject=[&]{const auto receipt=indirect_policy::Describe(signature);
        const uint64_t details[]{stride,receipt.kind,receipt.stride,tag.scope,tag.stage,policy,capture,current.flags};
        std::copy_n(details,8,CyberpunkVR_RetainedGeometryIndirectRejected);Seal(list,side,9);return false;};
    if(valid){const auto desc=arguments->GetDesc();ComPtr<ID3D12Device> device;
        valid=desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && offset<=desc.Width && UINT64(count)*stride<=desc.Width-offset &&
            SUCCEEDED(arguments->GetDevice(IID_PPV_ARGS(&device))) && device==work->device;
        if(valid && countBuffer){const auto d=countBuffer->GetDesc();
            valid=d.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && countOffset<=d.Width && 4<=d.Width-countOffset &&
                SUCCEEDED(countBuffer->GetDevice(IID_PPV_ARGS(&device))) && device==work->device;}
    }
    if(valid)for(UINT slot=0;slot<5;++slot)if((slot==4 || slot<targets.colors) && (!value->imported[slot] ||
        value->targets.resources[slot]!=targets.resources[slot] || value->targets.views[slot].identity!=targets.views[slot].identity)){valid=false;break;}
    auto* pipeline=reinterpret_cast<ID3D12PipelineState*>(current.pipeline);
    if(valid && work->mode==2 && std::none_of(verified.begin(),verified.end(),[&](const auto& p){return p.Get()==pipeline;}))valid=false;
    if(!valid)return reject();
    current.side=side;current.frame=work->frame;current.list=reinterpret_cast<uintptr_t>(list);
    const PureIndirectSource source{signature,count,arguments,offset,countBuffer,countOffset};auto replay=source;AppendSource(list,side,current,&source,&replay);
    if(work->failed){if(!work->chain.RestoreNative(list,EyeIndex(side),targets.colors))Fail(23);PublishSources();return false;}
    if(!gpu::KeepCommandResources(list,{signature,arguments,pipeline}) || (countBuffer && !gpu::KeepCommandResources(list,{countBuffer}))){Fail(24);Seal(list,side,21);return false;}
    if(!work->chain.Bind(list,1u<<EyeIndex(side),targets.colors)){Fail(22);Seal(list,side,20);return false;}
    if(std::none_of(work->pipelines.begin(),work->pipelines.end(),[&](const auto& p){return p.Get()==pipeline;}))work->pipelines.emplace_back(pipeline);
    const bool nativeTables=BindNativeSourceTables(list);
    if(work->failed){work->chain.RestoreNative(list,EyeIndex(side),targets.colors);return false;}
    {copy_windows::ReadOnlyScope readOnly;probe::Scope internal;list->ExecuteIndirect(replay.signature.Get(),replay.maxCommandCount,replay.arguments.Get(),replay.argumentOffset,replay.countBuffer.Get(),replay.countOffset);}
    if(nativeTables){probe::Scope internal;work->sources.back().bindings.Restore(list);}
    if(work->sourceSnapshots && !work->snapshotRows.empty() && work->snapshotRows.back()[11]==reinterpret_cast<uintptr_t>(replay.arguments.Get())){
        work->snapshotRows.back()[21]=1;PublishSources();
    }
    if(!work->chain.RestoreNative(list,EyeIndex(side),targets.colors))Fail(23);
    if(tag.scope && value->scope!=tag.scope){value->scope=tag.scope;++CyberpunkVR_RetainedGeometryStats[2+side];}
    if(tag.scope && tag.stage<64)CyberpunkVR_RetainedGeometryEnds[2+side]|=UINT64(1)<<tag.stage;
    ++CyberpunkVR_RetainedGeometryStats[side];++CyberpunkVR_RetainedGeometryIndirect[side];return work->mode==2;
}
void BeforeBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers){
    if(probe::internalCommands || !Active() || !count)return;std::lock_guard lock(mutex);UINT side{};auto* value=Owned(list,side);if(!value)return;
    if(!barriers){Seal(list,side,10);return;}
    for(UINT i=0;i<count;++i){const auto& b=barriers[i];
        if(b.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING || (b.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION && Touches(*value,b.Transition.pResource)) ||
            (b.Type==D3D12_RESOURCE_BARRIER_TYPE_UAV && (!b.UAV.pResource || Touches(*value,b.UAV.pResource)))){
            const auto kind=b.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION?target_handoff::ExitKind::Transition:
                b.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING?target_handoff::ExitKind::Aliasing:target_handoff::ExitKind::Uav;
            Seal(list,side,11,kind);return;}
    }
}
void BeforeTargets(ID3D12GraphicsCommandList* list,UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* handles,BOOL contiguous,const D3D12_CPU_DESCRIPTOR_HANDLE* depth){
    // OM binding does not access image contents or transition resource states.
    // Draw validates the new live views; barriers, clears and copies seal before
    // their actual memory access. Retain across a scope's temporary OM unbind.
    (void)list;(void)count;(void)handles;(void)contiguous;(void)depth;
}
void BeforeResources(ID3D12GraphicsCommandList* list,ID3D12Resource* source,ID3D12Resource* destination,UINT reason){
    if(probe::internalCommands || !Active())return;std::lock_guard lock(mutex);UINT side{};auto* value=Owned(list,side);
    if(value && (Touches(*value,source) || Touches(*value,destination)))Seal(list,side,reason,
        reason==11?target_handoff::ExitKind::Resolve:reason==3?target_handoff::ExitKind::Discard:target_handoff::ExitKind::Copy);
}
void BeforeClose(ID3D12GraphicsCommandList* list){Flush(list,0);}
void Reset(ID3D12GraphicsCommandList* list){
    target_handoff::Reset(list);
    prepared_pair::Reset(list);
    if(probe::internalCommands || !native_probe::ObserveBindings())return;std::lock_guard lock(mutex);renderPasses.erase(list);UINT side{};
    if(auto* value=Owned(list,side)){if(work->sourceCbvSnapshots){work->cbvSnapshotsFailed=true;value->cbvArenaAborted=true;value->cbvArena.reset();}
        value->sealed=true;Fail(20);}
}
void RenderPass(ID3D12GraphicsCommandList* list,bool begin){
    if(probe::internalCommands || !native_probe::ObserveBindings())return;
    std::lock_guard lock(mutex);
    if(begin){UINT side{};if(Owned(list,side))Seal(list,side,12);renderPasses.insert(list);}
    else renderPasses.erase(list);
}
bool BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    prepared_pair::BeforeSubmit(queue,count,lists);
    if(probe::internalCommands || !Active() || !queue || !lists)return false;
    std::lock_guard lock(mutex);if(!work || work->complete)return false;bool serialize{};
    for(UINT side=0;side<2;++side){auto& value=work->eyes[side];if(!value.started || value.submitted)continue;
        UINT occurrences{};for(UINT i=0;i<count;++i)occurrences+=reinterpret_cast<uintptr_t>(lists[i])==value.recording.list;
        if(!occurrences)continue;serialize=true;value.pending=true;
        if(!value.sealed || occurrences!=1 || !native_recording::ValidateSubmission(value.recording,queue))Fail(300+native_recording::LastFailure());
        for(auto& source:work->sources)for(size_t n=0;n<source.materialRows.size();++n){auto& row=work->materialRows[source.materialRows[n]];
            if(row[1]!=side)continue;
            if(row[8]==1)row[17]=!work->failed;
            else if(row[8]==5)row[17]=!work->failed && texture_read::ValidateImmutable(source.materialOwners[n],queue);
        }
        if(work->sourceSnapshots)for(auto& snapshot:work->snapshotRows)if(snapshot[1]==side && snapshot[2]){
            auto& source=work->sources[size_t(snapshot[0])];
            if(indirect_copy::ValidateSubmission(source.indirectProof,queue))snapshot[20]=1;
            else{work->snapshotFailed=true;snapshot[5]=indirect_copy::LastFailure();if(!snapshot[5])snapshot[5]=UINT(indirect_copy::Failure::Ownership);}
        }
        if(work->sourceIaSnapshots){
            for(auto& instance:work->instanceRows)if(instance[1]==side && instance[4] && instance[0]<work->sources.size())
                instance[15]=instance_observer::ValidateSource(work->sources[size_t(instance[0])].instanceCandidate,queue);
            auto error=value.iaAborted?ia_snapshot::Failure::Submission:ia_snapshot::Failure::None;
            for(const auto& command:work->iaCommands)if(command[1]==side && command[6]){
                if(command[7] || !command[14] || command[0]>=work->sources.size())error=ia_snapshot::Failure::Submission;
                else if(error==ia_snapshot::Failure::None){const auto& proof=work->sources[size_t(command[0])].iaPrepared;
                    if(proof.list!=command[10] || proof.generation!=command[11] || proof.occurrence!=command[12] ||
                        reinterpret_cast<uintptr_t>(proof.owner.Get())!=command[13] || proof.count!=command[8] || proof.copiedBytes!=command[9])error=ia_snapshot::Failure::Changed;
                    else if(!ia_snapshot::ValidateSourceSubmission(proof,queue))error=ia_snapshot::LastFailure();
                }
            }
            if(error!=ia_snapshot::Failure::None){value.iaAborted=true;if(value.iaArena)value.iaArena->Abort();Fail(1780+UINT(error));
                for(auto& command:work->iaCommands)if(command[1]==side && command[6] && !command[7])command[7]=UINT(error);}
            value.iaValidated=error==ia_snapshot::Failure::None && !work->failed;
        }
        if(work->sourceCbvSnapshots){
            UINT error=value.cbvArenaAborted?8u:0u;
            for(auto& snapshot:work->cbvSnapshotCommands)if(snapshot[1]==side && snapshot[2]==1){
                if(snapshot[0]>=work->sources.size())error=9;
                else if(!error && !source_cbv_snapshot::ValidateSourceSubmission(work->sources[size_t(snapshot[0])].cbvPrepared,queue))error=100+UINT(source_cbv_snapshot::LastFailure());
            }
            if(error){work->cbvSnapshotsFailed=true;value.cbvArenaAborted=true;Fail(1200+error);}
            for(auto& snapshot:work->cbvSnapshotCommands)if(snapshot[1]==side && snapshot[2]==1){snapshot[18]=!error;if(error)snapshot[3]=error;}
            for(auto& row:work->cbvSnapshotRows)if(row[1]==side && row[8]==1)row[30]=!error;
        }
        if(work->sourceSelectors){
            UINT snapshotFailure{};
            for(auto& row:work->t5SnapshotRows)if(row[1]==side && row[2]==1){
                if(value.t5Aborted || row[0]>=work->sources.size())snapshotFailure=1;
                else if(!snapshotFailure && !structured_buffer_snapshot::ValidateSourceSubmission(work->sources[size_t(row[0])].t5Prepared,queue))
                    snapshotFailure=UINT(structured_buffer_snapshot::LastFailure());
            }
            if(snapshotFailure){value.t5Aborted=true;if(value.t5Arena)value.t5Arena->Abort();Fail(1500+snapshotFailure);}
            for(auto& row:work->t5SnapshotRows)if(row[1]==side && row[2]==1)row[24]=!snapshotFailure;
            for(auto& command:work->selectorCommands)if(command[1]==side && command[0]<work->sources.size()){
                const auto& source=work->sources[size_t(command[0])];
                for(const auto& table:source.selectorSlots)for(const auto& slot:table.slots)command[23]+=!descriptors::MatchesSourceSlot(slot);
                command[24]=!work->failed;
            }
            for(auto& row:work->t5Rows)if(row[1]==side && row[2])row[29]=descriptors::MatchesDescriptor(work->sources[size_t(row[0])].multilayerT5.stamp);
        }
        if(work->fullT5Enabled){auto* stats=CyberpunkVR_RetainedFullT5Stats;
            for(auto& row:work->fullT5Rows)if(row[1]==side){
                if(row[0]>=work->sources.size() || !value.fullT5Arena || !value.fullT5Arena->Finalized() ||
                    !structured_buffer_snapshot::ValidateSourceSubmission(work->sources[size_t(row[0])].fullT5Prepared,queue)){
                    if(!stats[11])stats[11]=5;Fail(2705);
                }else{row[14]=1;++stats[8];}
            }
        }
        for(auto& source:work->sources)for(auto& item:source.materialBuffers)if(item.copy.owner){auto& row=work->materialBufferCopies[item.row];
            if(row[1]!=side)continue;row[11]=material_buffer_snapshot::ValidateSubmission(item.copy,queue);
            if(!row[11]){if(value.materialArena)value.materialArena->Abort();Fail(2210);}
        }
        value.submissionCertified=!work->failed;
        for(auto& row:work->nativeTableRows)if(row[1]==side && row[2]){
            row[15]=native_descriptors::ValidateSubmission(work->sources[size_t(row[0])].nativeTables,queue);
            if(!row[15]){Fail(2312);value.submissionCertified=false;}}
        if(value.submissionCertified)value.sourceQueue=queue;
        if(value.instancePredicate){void* mapped{};const D3D12_RANGE none{};
            if(FAILED(value.instancePredicate->Map(0,&none,&mapped)) || !mapped){Fail(1803);value.submissionCertified=false;}
            else{auto* permits=static_cast<uint64_t*>(mapped);
                for(auto& row:work->instanceGpuRows)if(row[1]==side && row[2]){
                    row[3]=!work->failed && row[0]<work->instanceRows.size() && work->instanceRows[size_t(row[0])][15];
                    permits[row[12]/8]=row[3];}
                const D3D12_RANGE written{0,2048*8};value.instancePredicate->Unmap(0,&written);
            }
        }
    }
    PublishSources();
    if(serialize){BuildGuardedSources();BuildStagedSources();StartCurrentSources(queue);ObserveStagedResidency(queue,count,lists);
        if(CyberpunkVR_RetainedStagedResidency[0]==3){const auto details=queue_residency::LastDetails();
            std::copy(details.begin(),details.end(),CyberpunkVR_RetainedStagedResidencyDetails);
            const auto membership=queue_residency::MembershipDetails();
            std::copy(membership.begin(),membership.end(),CyberpunkVR_RetainedStagedMembershipDetails);}
        ++CyberpunkVR_RetainedGeometryStats[8];CyberpunkVR_RetainedGeometryStats[9]+=count;}return serialize;
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    prepared_pair::Submitted(queue,count,lists);
    if(probe::internalCommands || !Active() || !queue || !lists)return;std::lock_guard lock(mutex);if(!work)return;
    if(CyberpunkVR_RetainedSourceQueueOrder[14] && CyberpunkVR_RetainedSourceQueueOrder[1]==reinterpret_cast<uintptr_t>(queue))
        for(UINT i=0;i<count;++i)if(reinterpret_cast<uintptr_t>(lists[i])==CyberpunkVR_RetainedSourceQueueOrder[4])CyberpunkVR_RetainedSourceQueueOrder[15]=1;
    for(UINT side=0;side<2;++side){auto& value=work->eyes[side];if(!value.pending || value.submitted)continue;
        if(std::none_of(lists,lists+count,[&](const auto* l){return reinterpret_cast<uintptr_t>(l)==value.recording.list;}))continue;
        value.pending=false;
        if(!value.submissionCertified || value.sourceQueue.Get()!=queue || !value.fence || FAILED(queue->Signal(value.fence.Get(),1))){
            value.submissionCertified=false;value.sourceQueue.Reset();Fail(21);work->complete=true;watching=false;PublishSources();return;
        }
        value.signaledValue=1;value.submitted=true;CyberpunkVR_RetainedGeometryStats[6+side]=1;
        for(auto& row:work->instanceGpuRows)if(row[1]==side && row[2] && row[3])row[4]=1;
        if(work->sourceIaSnapshots && value.iaValidated){
            for(auto& command:work->iaCommands)if(command[1]==side && command[6] && command[14] && !command[7])command[15]=1;
            for(auto& row:work->iaViews)if(row[1]==side && row[13])row[14]=1;
        }
    }
}
void SubmitCurrent(ID3D12CommandQueue* queue)try{
    if(probe::internalCommands || !queue || !Active())return;
    std::lock_guard lock(mutex);
    if(!work || !work->currentEnabled || !work->currentStarted || work->currentSubmitted)return;
    if(work->failed){prepared_pair::CancelCurrent(work->frame);work->currentStarted=false;return;}
    if(std::any_of(work->eyes.begin(),work->eyes.end(),[](const auto& eye){return !eye.submitted;}))return;
    if(work->infixEnabled && (!CyberpunkVR_RetainedInfix[3] || CyberpunkVR_RetainedInfix[12]!=CyberpunkVR_RetainedInfix[3] ||
        CyberpunkVR_RetainedInfix[14]))throw std::logic_error("Infix consumer is outside the certified prefix boundary");
    auto* stats=CyberpunkVR_RetainedCurrent;stats[10]=work->eyes[0].submitted;stats[11]=work->eyes[1].submitted;
    ValidateBorrowedTables(true);
    ObserveBorrowedPageables(true);
    stats[13]=work->eyes[0].signaledValue;stats[14]=work->eyes[1].signaledValue;
    for(const auto& eye:work->eyes)if(!eye.sealed || !eye.submissionCertified || eye.sourceQueue.Get()!=queue ||
        !eye.fence || !eye.signaledValue)throw std::logic_error("Current source execution lacks its own queued fence");
    for(UINT side=0;side<2;++side)for(UINT i=0;i<4;++i){auto* row=CyberpunkVR_RetainedTargetLifetime+(side*4+i)*16;
        if(row[3]){row[4]=target_lifetime::Validate(work->eyes[side].borrowedTargets[i]);
            if(!row[4]){row[5]=300+target_lifetime::LastFailure();const auto details=target_lifetime::LastDetails();row[6]=details[0];row[7]=details[1];}}
    }
    if(stats[12]!=reinterpret_cast<uintptr_t>(queue) || !prepared_pair::SubmitCurrent(queue,work->frame))
        throw std::logic_error("Current consumer submission failed");
    work->currentSubmitted=true;stats[9]=1;
    if(work->infixEnabled)CyberpunkVR_RetainedInfix[13]=1;
}catch(...){std::lock_guard lock(mutex);CyberpunkVR_RetainedCurrent[15]=2431;Fail(2431);
    if(work){prepared_pair::CancelCurrent(work->frame);work->currentStarted=false;}}
UINT CurrentBoundary(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists)try{
    std::lock_guard lock(mutex);
    if(!work || !work->infixEnabled || !work->currentStarted || work->currentSubmitted || work->failed ||
        CyberpunkVR_RetainedInfix[2])return 0;
    auto* stats=CyberpunkVR_RetainedInfix;stats[1]=work->frame;++stats[2];stats[6]=count;
    std::array<submission_frontier::Source,2> sources;
    for(UINT side=0;side<2;++side){const auto& eye=work->eyes[side];
        if(eye.submitted || !eye.sealed || !eye.submissionCertified || eye.sourceQueue.Get()!=queue)
            throw std::logic_error("Infix producers are not both pending on this queue");
        sources[side].recording=eye.recording;bool first=true;
        for(const auto& source:work->sources)if(source.command.geometry.draw.side==side){const auto& parent=source.parent;
            if(parent.status)throw std::logic_error("Infix source has no qualified graph parent");
            if(first){sources[side].parent=parent;first=false;}
            else if(parent.fingerprint!=sources[side].parent.fingerprint || parent.handle!=sources[side].parent.handle ||
                parent.viewIndex!=side || parent.slot!=sources[side].parent.slot || parent.object!=sources[side].parent.object)
                throw std::logic_error("Infix source spans multiple graph producers");
        }
        if(first)throw std::logic_error("Infix source eye is empty");
    }
    submission_frontier::Boundary boundary;
    if(!submission_frontier::FindBoundary(queue,count,lists,sources,boundary)){
        stats[15]=2500+submission_frontier::LastBoundaryFailure();throw std::logic_error("Native suffix boundary is unqualified");}
    std::array<target_handoff::SourceSnapshot,2> targetSnapshots;
    const bool targetSnapshotsValid=target_handoff::SnapshotSources(queue,work->frame,targetSnapshots);
    queue_residency::Scope targetScope;
    const bool targetScopeValid=queue_residency::Capture(queue,count,lists,targetScope);
    if(work->borrowedTablesEnabled){work->borrowedTableScope=targetScope;ValidateBorrowedTables(false);ObserveBorrowedPageables(false);}
    target_access::Receipt targetReceipt;
    if(targetSnapshotsValid){
        if(targetScopeValid)target_access::Query(queue,targetSnapshots,boundary,targetScope,targetReceipt);
        else target_access::Query(queue,targetSnapshots,boundary);
    }
    for(UINT side=0;side<2;++side)for(UINT i=0;i<4;++i){auto* row=CyberpunkVR_RetainedTargetLifetime+(side*4+i)*16;
        if(!row[2])continue;
        if(!targetScopeValid){row[5]=400+queue_residency::LastFailure();continue;}
        row[3]=target_lifetime::Bind(work->eyes[side].targetLifetimes[i],queue,targetScope,work->eyes[side].borrowedTargets[i]);
        if(!row[3]){row[5]=200+target_lifetime::LastFailure();const auto details=target_lifetime::LastDetails();row[6]=details[0];row[7]=details[1];}
    }
    if(prepared_pair::WantsNativeExport(work->frame)){
        std::array<target_lifetime::BorrowedScope,8> lifetimes;
        for(UINT side=0;side<2;++side)for(UINT i=0;i<4;++i)lifetimes[side*4+i]=work->eyes[side].borrowedTargets[i];
        prepared_pair::PrepareNativeExport(queue,work->frame,targetReceipt,lifetimes);
    }
    stats[3]=boundary.afterIndex;stats[4]=boundary.sourceIndices[0];stats[5]=boundary.sourceIndices[1];stats[7]=count-boundary.afterIndex;
    stats[8]=boundary.batch;stats[9]=boundary.consumer;stats[10]=boundary.graphEpoch;stats[11]=boundary.fingerprint;
    work->infixInFlight=true;
    return boundary.afterIndex;
}catch(...){std::lock_guard lock(mutex);if(!CyberpunkVR_RetainedInfix[15])CyberpunkVR_RetainedInfix[15]=2450;Fail(UINT(CyberpunkVR_RetainedInfix[15]));
    target_handoff::FinishObservation();
    target_access::Finish();
    if(work){prepared_pair::CancelCurrent(work->frame);work->currentStarted=false;}return 0;}
void CurrentPrefixExecuted(UINT count){std::lock_guard lock(mutex);
    if(work && work->infixEnabled && work->infixInFlight && !work->currentSubmitted)CyberpunkVR_RetainedInfix[12]=count;}
void CurrentSuffixExecuted(UINT count){std::lock_guard lock(mutex);
    if(work && work->infixEnabled && work->infixInFlight){
        CyberpunkVR_RetainedInfix[14]=count;work->infixInFlight=false;
        target_handoff::FinishObservation();
        target_access::Finish();
        ReleaseBorrowedObservers();
    }}
void OrderedExecution(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists,bool separateCalls){
    if(probe::internalCommands || !Active() || !queue || !lists || !count || count>256)return;
    std::lock_guard lock(mutex);if(!work || work->complete || !work->sourceStream || work->eyes[0].submitted)return;
    const auto& main=work->eyes[0];const auto& peer=work->eyes[1];std::array<uint64_t,256> addresses{};
    for(UINT i=0;i<count;++i)addresses[i]=reinterpret_cast<uintptr_t>(lists[i]);
    if(std::find(addresses.begin(),addresses.begin()+count,main.recording.list)==addresses.begin()+count)return;
    const auto input=[](const Eye& eye){return pair_submission::Source{eye.recording.list,eye.recording.generation,
        reinterpret_cast<uintptr_t>(eye.sourceQueue.Get()),eye.sealed,eye.submissionCertified,eye.submitted};};
    const auto result=pair_submission::Check(input(peer),input(main),reinterpret_cast<uintptr_t>(queue),{addresses.data(),count},separateCalls);
    const bool finalized=!work->failed && main.sealed && peer.sealed &&
        (!work->sourceCbvSnapshots || (main.cbvArena && peer.cbvArena && main.cbvArena->Finalized() && peer.cbvArena->Finalized())) &&
        (!work->sourceIaSnapshots || (main.iaValidated && peer.iaValidated));
    const uint64_t row[]{work->frame,reinterpret_cast<uintptr_t>(queue),peer.recording.list,peer.recording.generation,
        main.recording.list,main.recording.generation,result.producerIndex,result.consumerIndex,count,separateCalls,peer.submitted,
        peer.submissionCertified,main.submissionCertified,finalized,finalized?UINT(result.order):0,0};
    std::copy(std::begin(row),std::end(row),CyberpunkVR_RetainedSourceQueueOrder);
}
#ifdef CVR_PREPARED_PAIR_TESTING
bool TestInfixCaptureLifetime(){
    std::unique_lock lock(mutex);if(work || watching || prepared_pair::Active())return false;
    const auto state=CyberpunkVR_RetainedGeometryState.load(),request=CyberpunkVR_RetainedGeometryRequest.load();
    std::array<uint64_t,16> previous{};std::copy_n(CyberpunkVR_RetainedInfix,16,previous.begin());
    work=std::make_unique<Work>();work->infixEnabled=work->infixInFlight=work->complete=true;
    const auto* original=work.get();watching=true;CyberpunkVR_RetainedGeometryState=3;CyberpunkVR_RetainedGeometryRequest=1;
    lock.unlock();FrameBoundary();lock.lock();
    bool result=work.get()==original && work->infixInFlight && CyberpunkVR_RetainedGeometryRequest.load()==1;
    lock.unlock();CurrentSuffixExecuted(5);lock.lock();
    result=result && work.get()==original && !work->infixInFlight && CyberpunkVR_RetainedInfix[14]==5;
    work.reset();watching=false;CyberpunkVR_RetainedGeometryState=state;CyberpunkVR_RetainedGeometryRequest=request;
    std::copy(previous.begin(),previous.end(),CyberpunkVR_RetainedInfix);return result;
}
bool TestFrontierFailureDisarm(){
    std::lock_guard lock(mutex);if(work || watching)return false;
    const auto state=CyberpunkVR_RetainedGeometryState.load();const auto error=CyberpunkVR_RetainedGeometryStats[14];
    const auto enabled=CyberpunkVR_SubmissionFrontierEnable.load();
    const auto current=CyberpunkVR_PreparedCurrentEnable.load(),split=CyberpunkVR_OwnedSceneSplitEnable.load();bool result=true;
    for(const bool prepared:{false,true}){
        CyberpunkVR_PreparedCurrentEnable=prepared?1:0;CyberpunkVR_OwnedSceneSplitEnable=prepared?0:1;
        work=std::make_unique<Work>();LatchFrontierObserver(*work);CyberpunkVR_SubmissionFrontierEnable=1;
        CyberpunkVR_PreparedCurrentEnable=0;CyberpunkVR_OwnedSceneSplitEnable=0;
        Fail(2449);result=result && work->currentEnabled==prepared && work->frontierObserver && work->failed &&
            CyberpunkVR_SubmissionFrontierEnable.load()==0 && CyberpunkVR_RetainedGeometryState.load()==4;
    }
    work.reset();CyberpunkVR_RetainedGeometryState=state;CyberpunkVR_RetainedGeometryStats[14]=error;CyberpunkVR_SubmissionFrontierEnable=enabled;
    CyberpunkVR_PreparedCurrentEnable=current;CyberpunkVR_OwnedSceneSplitEnable=split;
    return result;
}
bool TestFrontierObserverModes(){
    std::lock_guard lock(mutex);if(work || watching)return false;
    const auto enabled=CyberpunkVR_SubmissionFrontierEnable.load();
    const auto current=CyberpunkVR_PreparedCurrentEnable.load(),split=CyberpunkVR_OwnedSceneSplitEnable.load();bool result=true;
    for(const uint32_t prepared:{0u,1u})for(const uint32_t scene:{0u,1u,2u,3u}){
        CyberpunkVR_PreparedCurrentEnable=prepared;CyberpunkVR_OwnedSceneSplitEnable=scene;
        Work capture;LatchFrontierObserver(capture);const bool requested=prepared!=0 || scene==1 || scene==2;
        result=result && capture.currentEnabled==(prepared!=0) && capture.frontierObserver==requested &&
            !capture.currentStarted && !capture.currentSubmitted && !capture.infixEnabled;
        CyberpunkVR_PreparedCurrentEnable=0;CyberpunkVR_OwnedSceneSplitEnable=0;
        // The CPU fixture's Install stub rejects. Rejection leaves an armed
        // capture disabled; an unrelated unrequested observer is untouched.
        CyberpunkVR_SubmissionFrontierEnable=1;ArmFrontierObserver(capture);
        result=result && CyberpunkVR_SubmissionFrontierEnable.load()==(requested?0u:1u);
        CyberpunkVR_SubmissionFrontierEnable=1;DisarmFinishedFrontierObserver(capture);
        result=result && CyberpunkVR_SubmissionFrontierEnable.load()==1;
        capture.complete=true;DisarmFinishedFrontierObserver(capture);
        result=result && CyberpunkVR_SubmissionFrontierEnable.load()==(requested?0u:1u);
    }
    CyberpunkVR_SubmissionFrontierEnable=enabled;CyberpunkVR_PreparedCurrentEnable=current;CyberpunkVR_OwnedSceneSplitEnable=split;
    return result;
}
#endif
}
#endif
