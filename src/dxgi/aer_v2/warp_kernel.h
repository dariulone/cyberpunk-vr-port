// CUDA warp kernel launcher for AER V2.
//
// Algorithm:
//   For each output pixel p:
//     1. Sample NvOF flow at p's grid cell → motion vector (dx, dy).
//     2. Combine with engine DLSS MV (already in screen space).
//     3. Sample prevColor at (uv - mv * blend) and currColor at (uv + mv * (1-blend)).
//     4. Depth-aware blend: compare prevDepth/curDepth at sample site; pick the
//        nearer (occlusion-aware); weigh by NvOF cost-map confidence.
//     5. Write interpolated color to outSurf.
//
// The CUDA source (warp_kernel.cu) requires nvcc + the CUDA Toolkit to build.
// When CUDA is not available, this dispatch reports unavailable and the AER V2
// pipeline falls back to the existing D3D12 MotionVectorWarp pass.
//
#pragma once

#include <cstdint>

namespace aer_v2 {

// Re-export the CUDA handle aliases (kept here so callers don't need cuda.h).
using CKtex   = uint64_t;   // cudaTextureObject_t
using CKsurf  = uint64_t;   // cudaSurfaceObject_t
using CKptr   = void*;      // CUdeviceptr
using CKstream= void*;      // CUstream

// Host-packed by-value reprojection parameters. The CUDA kernel uses these to
// convert UV+depth into a view-space point, apply render→predicted camera
// deltas, and derive a pose-driven screen-space offset in addition to NvOF/DLSS
// motion.
struct WarpPoseParams {
    float prevToPred[16]{};   // row-major render(prev) → predicted transform
    float currToPred[16]{};   // row-major render(curr) → predicted transform
    float tanLeft = -1.0f;
    float tanRight = 1.0f;
    float tanDown = -1.0f;
    float tanUp = 1.0f;
    float nearZ = 0.02f;
    // Overlay warp tuning (0..1), carried in the by-value pose struct so the
    // kernel signature doesn't change. 1 = built-in default, 0 = disable.
    float refineStrength = 1.0f;   // MV + pose flow refinement scale
    float occlusionSharp = 1.0f;   // occlusion edge-sharpen scale
    float foveation = 0.0f;        // fixed-foveation periphery fraction (0 = off)
    float flowSmooth = 0.0f;       // 0..0.9 NvOF flow 3x3 spatial low-pass (stale-eye shimmer reduction)
};

struct WarpKernelArgs {
    // Color frames (CUDA texture objects from CudaExternalMemory::texObj).
    CKtex   prevColorTex  = 0;
    CKtex   currColorTex  = 0;
    CKsurf  prevColorSurf = 0;  // DIAGNOSTIC: surface object for surf2Dread bypass
    // NvOF optical-flow output (short2 per grid cell, CUdeviceptr).
    CKptr   flowDevPtr    = nullptr;
    uint32_t flowStride   = 0;   // bytes per row
    uint32_t flowGridSize = 0;   // pixels per cell (1/2/4/8)
    // Optional engine DLSS motion vectors (R16G16_FLOAT or R32G32_FLOAT at
    // render resolution), captured from Streamline slSetTag. Sampled as a CUDA
    // texture object and scaled by the NGX-reported mvScaleX/Y.
    CKtex   engineMvTex = 0;
    float   engineMvScaleX = 1.0f;
    float   engineMvScaleY = 1.0f;
    uint32_t engineMvW = 0;
    uint32_t engineMvH = 0;
    // Optional depth (R32_FLOAT / typeless depth plane imported as texture) for
    // occlusion-aware blend.
    CKtex   prevDepthTex = 0;
    CKtex   currDepthTex = 0;
    // Cost map from NvOF (per-cell confidence, R8).
    CKptr   costMapDevPtr = nullptr;
    uint32_t costStride = 0;     // bytes per row
    // Output surface (CUDA surface object from the destination eye RT import).
    CKsurf  outSurf       = 0;
    // Output dimensions.
    uint32_t outW         = 0;
    uint32_t outH         = 0;
    uint32_t outputFormat = 0;   // DXGI_FORMAT enum of outSurf resource
    WarpPoseParams pose{};
    // Interpolation position between prev (0.0) and curr (1.0).
    float    blend        = 0.5f;
};

class WarpKernel {
public:
    // Compile + cache the per-scale kernel variants. Idempotent. Returns false
    // if the CUDA Toolkit wasn't available at build time (warp disabled).
    bool Init();
    void Shutdown();
    bool IsReady() const { return m_ready; }

    // Dispatch the warp kernel on `stream`. Caller ensures the source/dest
    // CudaExternalMemory imports are alive and the D3D12 fence semaphore has
    // been waited on (so D3D12 writes are visible to CUDA before the kernel
    // reads them). Picks the kernel variant matching (scale, depth-aware, HDR)
    // — scale is derived from outW/outH vs the source RT resolution.
    bool Launch(const WarpKernelArgs& args, CKstream stream);

private:
    bool m_ready = false;
    // Opaque handles to the compiled CUDA functions (one per scale variant).
    void*  m_fn[7] = {};   // index by scaleIndex ∈ {0,1,3,7,15,31,63}
};

} // namespace aer_v2
