// AER V2 top-level pipeline.
//
// Composes CudaInterop + NvOFInstance + WarpKernel + quaternion_math on the
// project's D3D12 / OpenXR backend. Replaces the older D3D12-compute
// MotionVectorWarp path when AER V2 (NvOF) is selected in the overlay.
#pragma once

#include <cstdint>
#include <memory>
#include <vector>
#include <unordered_map>

#include <d3d12.h>
#include <wrl.h>
#include <openxr/openxr.h>

#include "cuda_interop.h"
#include "nvof_instance.h"
#include "warp_kernel.h"
#include "quaternion_math.h"

namespace aer_v2 {

// Top-level AER V2 manager. One instance, owned by OpenXRManager.
// Thread affinity: all public methods must be called from the same thread.
class AerV2Pipeline {
public:
    AerV2Pipeline() = default;
    ~AerV2Pipeline();

    AerV2Pipeline(const AerV2Pipeline&) = delete;
    AerV2Pipeline& operator=(const AerV2Pipeline&) = delete;

    enum class Mode : int {
        Off            = 0,   // disable AER V2 — fall back to AER legacy
        LegacyAER      = 1,   // alternate eye, no interpolation (some ghosting)
        AERv2          = 2,   // NvOF + warp interpolation (low ghosting)
        AERv2HighQ     = 3,   // AERv2 + ForceMatchedEyePoses
    };

    // Lazily bind CUDA + NvOF on the supplied D3D12 device. Safe to call every
    // frame; cached. Resolution changes trigger NvOF re-init. Returns false if
    // CUDA/NvOF is unavailable.
    bool EnsureInitialized(ID3D12Device* device,
                           ID3D12CommandQueue* queue,
                           uint32_t width,
                           uint32_t height);
    void Shutdown();
    bool IsReady() const { return m_ready; }

    // Mode + ForceMatchedEyePoses toggles driven from the F10 overlay.
    void SetMode(Mode m) { m_mode = m; }
    Mode GetMode() const { return m_mode; }
    void SetForceMatchedEyePoses(bool v) { m_forceMatched = v; }
    bool GetForceMatchedEyePoses() const { return m_forceMatched; }
    // Overlay warp tuning (0..1). Applied to the kernel each warp.
    void SetWarpTuning(float refineStrength, float occlusionSharp, float foveation, float flowSmooth) {
        m_refineStrength = refineStrength; m_occlusionSharp = occlusionSharp; m_foveation = foveation; m_flowSmooth = flowSmooth;
    }

    // Per-frame: synthesize the midpoint frame for ONE eye from its previous
    // and current converted color textures (`flowPrev`/`flowCurr` in
    // OpenXRManager, already BGRA8 and COPY_SOURCE). This keeps the existing
    // worker's per-eye flow model and the same-eye frame generation
    // behavior: each eye gets an interpolated in-between frame between its
    // previous and current real renders.
    // On success the synthesized color is written into `outSynthColor`, which
    // must be a pre-allocated D3D12 texture created with a shareable heap.
    bool ProcessTemporalFrame(int eye,
                              ID3D12Resource* prevFlowInput,
                              ID3D12Resource* currFlowInput,
                              ID3D12Resource* prevColor,
                              ID3D12Resource* currColor,
                              ID3D12Resource* engineMv,
                              ID3D12Resource* prevDepth,
                              ID3D12Resource* currDepth,
                              float mvScaleX,
                              float mvScaleY,
                              const XrFovf& fov,
                              const XrPosef&  prevPose,
                              const XrPosef&  currPose,
                              const XrPosef&  predictedPose,
                              float blend,
                              ID3D12Resource* outSynthColor);

private:
    bool ImportTexture(ID3D12Resource* color, bool writable, CudaExternalMemory* out);
    bool RunNvOF(int eye, const CudaExternalMemory& prev, const CudaExternalMemory& curr, CUdeviceptr* outFlow);
    bool RunWarp(int eye, const CudaExternalMemory& prev, const CudaExternalMemory& curr, const CudaExternalMemory& dst,
                 const CudaExternalMemory* engineMv,
                 const CudaExternalMemory* prevDepth,
                 const CudaExternalMemory* currDepth,
                 float mvScaleX,
                 float mvScaleY,
                 const Quat& prevQ, const Vec3& prevPos,
                 const Quat& currQ, const Vec3& currPos,
                 const Quat& predQ, const Vec3& predPos,
                 float tanLeft,
                 float tanRight,
                 float tanDown,
                 float tanUp,
                 uint32_t outputFormat,
                 float blend);

    // Sub-components. NvOF is PER-EYE so each eye's flow can be cached across
    // display frames without the other eye overwriting the shared flow buffer.
    // NvOF Execute is the expensive part (it CPU-syncs on its output stream), so
    // we only re-run it when an eye gets a NEW capture — flow between the same
    // prev/curr pair is identical, the per-frame warp just reuses it at a new
    // blend. This is the "flow once per capture, warp every frame" model and
    // is what gets the HMD to full refresh (re-importing/re-flowing every frame
    // at 3072^2 x2 eyes blew the 11ms budget -> 56fps).
    CudaInterop   m_cuda;
    NvOFInstance  m_nvof[2];
    WarpKernel    m_warp;

    // Per-eye flow cache key: the (prevColor,currColor) resources the cached flow
    // was computed from. Same pair -> skip Execute.
    ID3D12Resource* m_flowKeyPrev[2] = {nullptr, nullptr};
    ID3D12Resource* m_flowKeyCurr[2] = {nullptr, nullptr};
    void*           m_cachedFlow[2]  = {nullptr, nullptr};   // CUdeviceptr of last flow per eye

    // Cross-API fence (one shared between D3D12 queue and CUDA).
    Microsoft::WRL::ComPtr<ID3D12Fence> m_d3dFence;
    CudaExternalSemaphore               m_cudaSem{};
    HANDLE                              m_fenceEvent = nullptr;
    uint64_t                            m_fenceValue = 0;

    // NON-BLOCKING warp: ProcessTemporalFrame queues the CUDA
    // warp + signals the fence and returns WITHOUT a CPU WaitForSingleObject (the
    // old block stalled the game present thread -> jitter). GPU ordering for the
    // consumer's CopyResource is guaranteed because the submit runs on the SAME
    // m_d3dQueue (FIFO after the in-pipeline m_queue->Wait).
    //
    // IMPORT CACHE. The captured/synth textures
    // are reused every frame (ring-buffered), so re-importing them into CUDA each
    // call — CreateSharedHandle + cudaImportExternalMemory + map + create tex/surf
    // object, x5-6 textures x2 eyes — cost ~25ms/frame (37 fps). Instead import
    // ONCE per ID3D12Resource* and reuse. Entries live until Shutdown/resize, so
    // the GPU can always reference them (no per-frame release, no use-after-free).
    std::unordered_map<ID3D12Resource*, CudaExternalMemory> m_importCache;
    bool GetOrImport(ID3D12Resource* res, bool writable, CudaExternalMemory* out);
    void ClearImportCache();

    // High-priority non-blocking CUDA stream, per eye (the eye's NvOF input stream).
    CUstream m_stream[2] = {nullptr, nullptr};

    ID3D12Device*       m_device = nullptr;
    ID3D12CommandQueue* m_queue  = nullptr;

    Mode m_mode           = Mode::Off;
    bool m_forceMatched   = true;
    float m_refineStrength = 1.0f;    // overlay warp tuning
    float m_occlusionSharp = 1.0f;
    float m_foveation = 0.0f;         // fixed-foveation periphery fraction (0 = off)
    float m_flowSmooth = 0.0f;        // NvOF flow 3x3 spatial low-pass (0 = off)
    bool m_ready          = false;
    uint32_t m_width      = 0;
    uint32_t m_height     = 0;
};

} // namespace aer_v2
