// The exports src/Render/StereoGpuProbe.cpp defines in the reference build and the Rust build defines in Rust (its
// rust-build modules: native_state_exports.rs), with the probe's start resets (cvr_sgp_reset_start): their reference
// definitions, for scene_probe_test built whole in the Rust build's form, whose Rust library leaves those modules out.
#include "Render/StereoGpuProbe.hpp"
#include <atomic>
#include <climits>
#include <cstdint>
#include <cstring>
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeRequest{0},CyberpunkVR_StereoGpuProbeState{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeNativeReference{0},CyberpunkVR_StereoGpuProbeReferenceSkipped{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeQueueWaits{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeGroup{0},CyberpunkVR_StereoGpuProbeSourceDraws{0},CyberpunkVR_StereoGpuProbeMainDraws{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeMainViewMask{UINT_MAX},CyberpunkVR_StereoGpuProbeMainReferenceMask{UINT_MAX};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeEyeDifferences[2]{},CyberpunkVR_StereoGpuProbeTargetDifferences[4]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeDifferenceCount{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbePrepareGateMs{0},CyberpunkVR_StereoGpuProbePrepareGateReason{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeLateVisibility{0},CyberpunkVR_StereoGpuProbeMatchedDraws{0},CyberpunkVR_StereoGpuProbeFallbackDraws{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoGpuProbePrepareGateStart{0},CyberpunkVR_StereoGpuProbePrepareGateMain{0},CyberpunkVR_StereoGpuProbePrepareGateRelease{0};
__declspec(dllexport) int32_t CyberpunkVR_StereoGpuProbeResult{};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeDifferentBytes{},CyberpunkVR_StereoGpuProbeComparedBytes{},CyberpunkVR_StereoGpuProbeCoverage[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeSceneDepth{0},CyberpunkVR_StereoGpuProbeSceneReject{0};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeSceneSize[2]{},CyberpunkVR_StereoGpuProbeScenePhase[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeSceneRoute{0},CyberpunkVR_StereoGpuProbeSceneCommitted{0};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeSceneRouted[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeSceneSamples[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeTiming{0},CyberpunkVR_StereoGpuProbeTimingDrops{0};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeTicks[7][2]{},CyberpunkVR_StereoGpuProbeFrequency[2]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeTimedCount[7][2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeMixedMaterials{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeFineRate{0};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeVrs[2][4]{},CyberpunkVR_StereoGpuProbeVrsChanges[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeVrsImage[2]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeVrsSize[2]{},CyberpunkVR_StereoGpuProbeVrsCaptured[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeVrsDifference{},CyberpunkVR_StereoGpuProbeVrsHistogram[2][16]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeDepthPrepass{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeStartIndices{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeStartRun{0};
// The reference build's start resets (StereoGpuProbe.cpp, Draw at the probe's start).
void cvr_sgp_reset_start(){
    std::memset(CyberpunkVR_StereoGpuProbeVrs,0,sizeof(CyberpunkVR_StereoGpuProbeVrs));
    std::memset(CyberpunkVR_StereoGpuProbeVrsChanges,0,sizeof(CyberpunkVR_StereoGpuProbeVrsChanges));
    std::memset(CyberpunkVR_StereoGpuProbeVrsImage,0,sizeof(CyberpunkVR_StereoGpuProbeVrsImage));
    std::memset(CyberpunkVR_StereoGpuProbeVrsSize,0,sizeof(CyberpunkVR_StereoGpuProbeVrsSize));std::memset(CyberpunkVR_StereoGpuProbeVrsCaptured,0,sizeof(CyberpunkVR_StereoGpuProbeVrsCaptured));
    CyberpunkVR_StereoGpuProbeVrsDifference=0;std::memset(CyberpunkVR_StereoGpuProbeVrsHistogram,0,sizeof(CyberpunkVR_StereoGpuProbeVrsHistogram));
    CyberpunkVR_StereoGpuProbeSceneCommitted=0;for(auto& x:CyberpunkVR_StereoGpuProbeSceneRouted)x=0;for(auto& x:CyberpunkVR_StereoGpuProbeSceneSamples)x=0;
    CyberpunkVR_StereoGpuProbeTimingDrops=0;for(auto& stage:CyberpunkVR_StereoGpuProbeTicks)for(auto& x:stage)x=0;
    for(auto& stage:CyberpunkVR_StereoGpuProbeTimedCount)for(auto& x:stage)x=0;for(auto& x:CyberpunkVR_StereoGpuProbeFrequency)x=0;
    CyberpunkVR_StereoGpuProbeSceneReject=0;for(auto& p:CyberpunkVR_StereoGpuProbeScenePhase)p=0;for(auto& s:CyberpunkVR_StereoGpuProbeSceneSize)s=0;
    CyberpunkVR_StereoGpuProbeReferenceSkipped=0;CyberpunkVR_StereoGpuProbeQueueWaits=0;CyberpunkVR_StereoGpuProbeResult=0;
    CyberpunkVR_StereoGpuProbeDifferentBytes=CyberpunkVR_StereoGpuProbeComparedBytes=0;
    CyberpunkVR_StereoGpuProbeCoverage[0]=CyberpunkVR_StereoGpuProbeCoverage[1]=0;
    for(auto& n:CyberpunkVR_StereoGpuProbeEyeDifferences)n=0;for(auto& n:CyberpunkVR_StereoGpuProbeTargetDifferences)n=0;CyberpunkVR_StereoGpuProbeDifferenceCount=0;
    CyberpunkVR_StereoGpuProbeSourceDraws=1;CyberpunkVR_StereoGpuProbeMainDraws=0;
    CyberpunkVR_StereoGpuProbeMatchedDraws=0;CyberpunkVR_StereoGpuProbeFallbackDraws=0;
    CyberpunkVR_StereoGpuProbePrepareGateReason=0;CyberpunkVR_StereoGpuProbePrepareGateStart=0;
    CyberpunkVR_StereoGpuProbePrepareGateMain=0;CyberpunkVR_StereoGpuProbePrepareGateRelease=0;
}
}
