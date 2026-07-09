// NvOF CUDA instance for AER V2.
//
// Thin wrapper over the NVIDIA Optical Flow SDK plus the stream/caps/grid-size
// orchestration this project needs around it.
#pragma once

#include <cstdint>
#include <memory>

#include "cuda_interop.h"

namespace aer_v2 {

// Wraps an NvOF CUDA session bound to one CUcontext + one (width, height).
// Re-created when resolution changes.
// Input = two CUDA arrays (prev + curr eye color, from CudaInterop::ImportD3D12Resource).
// Output = a CUDA device-pointer buffer of NV_OF flow vectors (short2 per grid cell).
class NvOFInstance {
public:
    NvOFInstance();
    ~NvOFInstance();

    NvOFInstance(const NvOFInstance&) = delete;
    NvOFInstance& operator=(const NvOFInstance&) = delete;

    // Lazily create the NvOF engine for the given resolution on the calling
    // thread's CUcontext (fetched via CudaInterop::GetCurrentContext). Picks the
    // smallest supported output grid size for the target eye resolution.
    // Returns false (and disables AER V2) if nvofapi64.dll is missing, the GPU
    // is pre-Turing, or any NvOF call fails — caller falls back to AER legacy.
    bool Init(CudaInterop* cuda, uint32_t width, uint32_t height);
    void Shutdown();
    bool IsReady() const { return m_ready; }

    // Run optical-flow estimation between prev and curr color frames.
    // `prevArray` / `currArray` are CUarray handles obtained from
    // CudaExternalMemory::level0 (post ImportD3D12Resource). On success
    // `*outFlowDevPtr` receives a CUdeviceptr (opaque void*) the caller can
    // pass to the warp kernel. The buffer is owned by this instance and reused
    // across calls (single-buffered; caller must consume before next Execute).
    bool Execute(CUarray prevArray, CUarray currArray, CUdeviceptr* outFlowDevPtr);

    // Optional cost-map output (per-cell confidence). Used by the depth-aware
    // warp kernel to mask low-confidence regions.
    CUdeviceptr GetCostMapDevPtr() const;

    uint32_t GetWidth()       const { return m_width; }
    uint32_t GetHeight()      const { return m_height; }
    uint32_t GetGridSize()    const { return m_gridSize; }   // pixels per flow cell (1/2/4/8)
    uint32_t GetFlowStrideBytes() const;
    uint32_t GetCostStrideBytes() const;
    CUstream GetInputStream() const;                          // priority non-blocking stream

private:
    struct Impl;                            // pimpl hides NvOFCuda + cuda.h
    std::unique_ptr<Impl> m_impl;
    CudaInterop* m_cuda    = nullptr;
    uint32_t m_width       = 0;
    uint32_t m_height      = 0;
    uint32_t m_gridSize    = 0;
    bool     m_ready       = false;
};

} // namespace aer_v2
