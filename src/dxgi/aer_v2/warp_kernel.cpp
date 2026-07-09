#include "warp_kernel.h"

#include <windows.h>

extern void Log(const char* fmt, ...);

#if defined(AER_V2_NVOF_ENABLED)
#include <cstdio>
#include <cuda_runtime_api.h>

// Forward declaration of the nvcc-compiled kernel body.
extern "C" __global__ void aer_v2_warp_1x_depth(
    cudaTextureObject_t prevColorTex,
    cudaTextureObject_t currColorTex,
    const short2* flowDevPtr,
    uint32_t flowStride,
    uint32_t flowGridSize,
    cudaTextureObject_t engineMvTex,
    float engineMvScaleX,
    float engineMvScaleY,
    uint32_t engineMvW,
    uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex,
    cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr,
    uint32_t costStride,
    cudaSurfaceObject_t outSurf,
    uint32_t outW,
    uint32_t outH,
    uint32_t outputFormat,
    aer_v2::WarpPoseParams pose,
    float blend);
extern "C" __global__ void aer_v2_warp_2x_depth(
    cudaTextureObject_t, cudaTextureObject_t, const short2*, uint32_t, uint32_t,
    cudaTextureObject_t, float, float, uint32_t, uint32_t,
    cudaTextureObject_t, cudaTextureObject_t,
    const void*, uint32_t,
    cudaSurfaceObject_t, uint32_t, uint32_t, uint32_t,
    aer_v2::WarpPoseParams, float);
extern "C" __global__ void aer_v2_warp_4x_depth(
    cudaTextureObject_t, cudaTextureObject_t, const short2*, uint32_t, uint32_t,
    cudaTextureObject_t, float, float, uint32_t, uint32_t,
    cudaTextureObject_t, cudaTextureObject_t,
    const void*, uint32_t,
    cudaSurfaceObject_t, uint32_t, uint32_t, uint32_t,
    aer_v2::WarpPoseParams, float);
extern "C" __global__ void aer_v2_warp_8x_depth(
    cudaTextureObject_t, cudaTextureObject_t, const short2*, uint32_t, uint32_t,
    cudaTextureObject_t, float, float, uint32_t, uint32_t,
    cudaTextureObject_t, cudaTextureObject_t,
    const void*, uint32_t,
    cudaSurfaceObject_t, uint32_t, uint32_t, uint32_t,
    aer_v2::WarpPoseParams, float);
extern "C" __global__ void aer_v2_warp_16x_depth(
    cudaTextureObject_t, cudaTextureObject_t, const short2*, uint32_t, uint32_t,
    cudaTextureObject_t, float, float, uint32_t, uint32_t,
    cudaTextureObject_t, cudaTextureObject_t,
    const void*, uint32_t,
    cudaSurfaceObject_t, uint32_t, uint32_t, uint32_t,
    aer_v2::WarpPoseParams, float);
extern "C" __global__ void aer_v2_warp_32x_depth(
    cudaTextureObject_t, cudaTextureObject_t, const short2*, uint32_t, uint32_t,
    cudaTextureObject_t, float, float, uint32_t, uint32_t,
    cudaTextureObject_t, cudaTextureObject_t,
    const void*, uint32_t,
    cudaSurfaceObject_t, uint32_t, uint32_t, uint32_t,
    aer_v2::WarpPoseParams, float);
extern "C" __global__ void aer_v2_warp_64x_depth(
    cudaTextureObject_t, cudaTextureObject_t, const short2*, uint32_t, uint32_t,
    cudaTextureObject_t, float, float, uint32_t, uint32_t,
    cudaTextureObject_t, cudaTextureObject_t,
    const void*, uint32_t,
    cudaSurfaceObject_t, uint32_t, uint32_t, uint32_t,
    aer_v2::WarpPoseParams, float);

namespace aer_v2 {

namespace {
void LogWarp(const char* msg, int err = -1) {
    char buf[256];
    if (err >= 0) {
        std::snprintf(buf, sizeof(buf), "[aer_v2::warp] %s err=%d\n", msg, err);
    } else {
        std::snprintf(buf, sizeof(buf), "[aer_v2::warp] %s\n", msg);
    }
    OutputDebugStringA(buf);
    Log("%s", buf);
}
}

bool WarpKernel::Init() {
    m_fn[0] = reinterpret_cast<void*>(&aer_v2_warp_1x_depth);
    m_fn[1] = reinterpret_cast<void*>(&aer_v2_warp_2x_depth);
    m_fn[2] = reinterpret_cast<void*>(&aer_v2_warp_4x_depth);
    m_fn[3] = reinterpret_cast<void*>(&aer_v2_warp_8x_depth);
    m_fn[4] = reinterpret_cast<void*>(&aer_v2_warp_16x_depth);
    m_fn[5] = reinterpret_cast<void*>(&aer_v2_warp_32x_depth);
    m_fn[6] = reinterpret_cast<void*>(&aer_v2_warp_64x_depth);
    m_ready = true;
    LogWarp("Init OK (kernels 1/2/4/8/16/32/64x)");
    return true;
}

void WarpKernel::Shutdown() {
    for (void*& fn : m_fn) fn = nullptr;
    m_ready = false;
}

bool WarpKernel::Launch(const WarpKernelArgs& args, CKstream stream) {
    if (!m_ready || !m_fn[0] || !args.prevColorTex || !args.currColorTex || !args.flowDevPtr || !args.outSurf ||
        args.outW == 0 || args.outH == 0 || args.flowGridSize == 0) {
        return false;
    }

    dim3 block(16, 16, 1);
    dim3 grid((args.outW + block.x - 1) / block.x,
              (args.outH + block.y - 1) / block.y,
              1);

    cudaTextureObject_t prevTex = static_cast<cudaTextureObject_t>(args.prevColorTex);
    cudaTextureObject_t currTex = static_cast<cudaTextureObject_t>(args.currColorTex);
    auto* flowPtr = reinterpret_cast<const short2*>(args.flowDevPtr);
    uint32_t flowStride = args.flowStride;
    uint32_t flowGridSize = args.flowGridSize;
    auto outSurf = static_cast<cudaSurfaceObject_t>(args.outSurf);
    cudaTextureObject_t mvTex = static_cast<cudaTextureObject_t>(args.engineMvTex);
    float engineMvScaleX = args.engineMvScaleX;
    float engineMvScaleY = args.engineMvScaleY;
    uint32_t engineMvW = args.engineMvW;
    uint32_t engineMvH = args.engineMvH;
    cudaTextureObject_t prevDepthTex = static_cast<cudaTextureObject_t>(args.prevDepthTex);
    cudaTextureObject_t currDepthTex = static_cast<cudaTextureObject_t>(args.currDepthTex);
    auto costMapDevPtr = args.costMapDevPtr;
    uint32_t costStride = args.costStride;
    uint32_t outW = args.outW;
    uint32_t outH = args.outH;
    uint32_t outputFormat = args.outputFormat;
    WarpPoseParams pose = args.pose;
    float blend = args.blend;
    void* params[] = {
        &prevTex,
        &currTex,
        &flowPtr,
        &flowStride,
        &flowGridSize,
        &mvTex,
        &engineMvScaleX,
        &engineMvScaleY,
        &engineMvW,
        &engineMvH,
        &prevDepthTex,
        &currDepthTex,
        &costMapDevPtr,
        &costStride,
        &outSurf,
        &outW,
        &outH,
        &outputFormat,
        &pose,
        &blend,
    };

    void* fn = m_fn[0];
    switch (args.flowGridSize) {
    case 1:  fn = m_fn[0]; break;
    case 2:  fn = m_fn[1]; break;
    case 4:  fn = m_fn[2]; break;
    case 8:  fn = m_fn[3]; break;
    case 16: fn = m_fn[4]; break;
    case 32: fn = m_fn[5]; break;
    case 64: fn = m_fn[6]; break;
    default: fn = m_fn[0]; break;
    }

    cudaError_t err = cudaLaunchKernel(fn, grid, block, params, 0, reinterpret_cast<cudaStream_t>(stream));
    if (err != cudaSuccess) {
        LogWarp("cudaLaunchKernel failed", static_cast<int>(err));
        return false;
    }
    return true;
}

} // namespace aer_v2

#else

namespace aer_v2 {

bool WarpKernel::Init() {
    OutputDebugStringA("[aer_v2::warp] Init stub — CUDA kernel not built\n");
    return false;
}

void WarpKernel::Shutdown() { m_ready = false; }

bool WarpKernel::Launch(const WarpKernelArgs& args, CKstream stream) {
    (void)args;
    (void)stream;
    return false;
}

} // namespace aer_v2

#endif
