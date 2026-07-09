#include "aer_v2_pipeline.h"

#include <cmath>
#include <cstdio>
#include <windows.h>

extern void Log(const char* fmt, ...);

namespace aer_v2 {

namespace {
void LogAER(const char* msg) {
    char buf[512];
    std::snprintf(buf, sizeof(buf), "[aer_v2::pipeline] %s\n", msg ? msg : "");
    OutputDebugStringA(buf);
    Log("%s", buf);
}

Quat ToQuat(const XrQuaternionf& q) { return Quat{ q.x, q.y, q.z, q.w }; }
Vec3 ToVec3(const XrVector3f& v) { return Vec3{ v.x, v.y, v.z }; }
}

AerV2Pipeline::~AerV2Pipeline() { Shutdown(); }

bool AerV2Pipeline::EnsureInitialized(ID3D12Device* device,
                                      ID3D12CommandQueue* queue,
                                      uint32_t width,
                                      uint32_t height) {
    if (!device || !queue || width == 0 || height == 0) {
        return false;
    }

    if (!m_cuda.Load()) {
        LogAER("CUDA runtime/driver unavailable — AER V2 disabled");
        return false;
    }

    m_device = device;
    m_queue = queue;

    if (!m_d3dFence) {
        Microsoft::WRL::ComPtr<ID3D12Fence> fence;
        if (FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_SHARED, IID_PPV_ARGS(&fence)))) {
            LogAER("CreateFence(D3D12_FENCE_FLAG_SHARED) failed");
            return false;
        }
        m_d3dFence = std::move(fence);
        if (!m_cuda.ImportD3D12Fence(m_d3dFence.Get(), &m_cudaSem)) {
            LogAER("ImportD3D12Fence failed");
            return false;
        }
    }

    if (!m_nvof[0].IsReady() || !m_nvof[1].IsReady() || m_width != width || m_height != height) {
        // Resolution changed: the captured/synth textures get recreated, so the
        // import cache (keyed by ID3D12Resource*) would dangle. Drop it + the flow
        // cache keys (the device flow buffers are re-created too).
        ClearImportCache();
        for (int eye = 0; eye < 2; ++eye) {
            m_nvof[eye].Shutdown();
            if (!m_nvof[eye].Init(&m_cuda, width, height)) {
                LogAER("NvOF init failed");
                return false;
            }
            m_stream[eye] = m_nvof[eye].GetInputStream();
            m_flowKeyPrev[eye] = nullptr;
            m_flowKeyCurr[eye] = nullptr;
            m_cachedFlow[eye] = nullptr;
        }
        m_width = width;
        m_height = height;
    }

    if (!m_warp.IsReady()) {
        if (!m_warp.Init()) {
            LogAER("warp kernel init failed");
            return false;
        }
    }

    m_ready = true;
    return true;
}

void AerV2Pipeline::Shutdown() {
    // Drain in-flight async warps before freeing CUDA. Wait for the GPU to reach
    // the last signaled fence value so we never release imports still in use, then
    // free the import cache.
    if (m_d3dFence && m_fenceValue != 0 && m_d3dFence->GetCompletedValue() < m_fenceValue) {
        if (!m_fenceEvent) m_fenceEvent = CreateEvent(nullptr, FALSE, FALSE, nullptr);
        if (m_fenceEvent && SUCCEEDED(m_d3dFence->SetEventOnCompletion(m_fenceValue, m_fenceEvent))) {
            WaitForSingleObject(m_fenceEvent, 200);
        }
    }
    ClearImportCache();

    m_warp.Shutdown();
    m_nvof[0].Shutdown();
    m_nvof[1].Shutdown();
    m_cuda.ReleaseExternalSemaphore(&m_cudaSem);
    m_d3dFence.Reset();
    if (m_fenceEvent) {
        CloseHandle(m_fenceEvent);
        m_fenceEvent = nullptr;
    }
    m_stream[0] = nullptr;
    m_stream[1] = nullptr;
    for (int e = 0; e < 2; ++e) { m_flowKeyPrev[e] = nullptr; m_flowKeyCurr[e] = nullptr; m_cachedFlow[e] = nullptr; }
    m_cuda.Unload();
    m_device = nullptr;
    m_queue = nullptr;
    m_ready = false;
    m_width = 0;
    m_height = 0;
    m_fenceValue = 0;
}

bool AerV2Pipeline::GetOrImport(ID3D12Resource* res, bool writable, CudaExternalMemory* out) {
    if (!res || !out) return false;
    auto it = m_importCache.find(res);
    if (it != m_importCache.end()) {
        *out = it->second;   // shallow copy of cached handle (cache keeps ownership)
        return true;
    }
    CudaExternalMemory mem{};
    if (!m_cuda.ImportD3D12Resource(res, 1, writable, &mem)) {
        return false;
    }
    m_importCache.emplace(res, mem);
    *out = mem;
    return true;
}

void AerV2Pipeline::ClearImportCache() {
    for (auto& kv : m_importCache) {
        m_cuda.ReleaseExternalMemory(&kv.second);
    }
    m_importCache.clear();
}

bool AerV2Pipeline::ImportTexture(ID3D12Resource* color, bool writable, CudaExternalMemory* out) {
    // Cached: import once per resource, reuse every frame (see m_importCache).
    return GetOrImport(color, writable, out);
}

bool AerV2Pipeline::RunNvOF(int eye, const CudaExternalMemory& prev, const CudaExternalMemory& curr, CUdeviceptr* outFlow) {
    // Flow between the same (prev,curr) pair is identical, so skip the expensive,
    // CPU-blocking NvOF Execute when the pair is unchanged across display frames.
    // Keyed by the imported D3D12 resource ptr.
    if (prev.source && curr.source &&
        prev.source == m_flowKeyPrev[eye] && curr.source == m_flowKeyCurr[eye] && m_cachedFlow[eye]) {
        *outFlow = m_cachedFlow[eye];
        return true;
    }
    if (!m_nvof[eye].Execute(prev.level0, curr.level0, outFlow)) {
        return false;
    }
    m_flowKeyPrev[eye] = prev.source;
    m_flowKeyCurr[eye] = curr.source;
    m_cachedFlow[eye] = *outFlow;
    return true;
}

bool AerV2Pipeline::RunWarp(int eye,
                            const CudaExternalMemory& prev,
                            const CudaExternalMemory& curr,
                            const CudaExternalMemory& dst,
                            const CudaExternalMemory* engineMv,
                            const CudaExternalMemory* prevDepth,
                            const CudaExternalMemory* currDepth,
                            float mvScaleX,
                            float mvScaleY,
                            const Quat& prevQ,
                            const Vec3& prevPos,
                            const Quat& currQ,
                            const Vec3& currPos,
                            const Quat& predQ,
                            const Vec3& predPos,
                            float tanLeft,
                            float tanRight,
                            float tanDown,
                            float tanUp,
                            uint32_t outputFormat,
                            float blend) {
    (void)prevQ;
    (void)prevPos;

    // Build the quaternion delta transform and feed it into the warp kernel.
    float prevToPred[16]{};
    float currToPred[16]{};
    buildWarpTransform(prevQ, prevPos, predQ, predPos, prevToPred);
    buildWarpTransform(currQ, currPos, predQ, predPos, currToPred);

    WarpKernelArgs args{};
    args.prevColorTex = prev.surfObj;
    args.prevColorSurf = prev.surfObj;
    args.currColorTex = curr.surfObj;
    if (engineMv) {
        args.engineMvTex = engineMv->surfObj;
        args.engineMvScaleX = mvScaleX;
        args.engineMvScaleY = mvScaleY;
        if (engineMv->source) {
            const auto desc = engineMv->source->GetDesc();
            args.engineMvW = static_cast<uint32_t>(desc.Width);
            args.engineMvH = desc.Height;
        }
    }
    args.prevDepthTex = prevDepth ? prevDepth->surfObj : 0;
    args.currDepthTex = currDepth ? currDepth->surfObj : 0;
    args.flowDevPtr = nullptr;
    if (!RunNvOF(eye, prev, curr, &args.flowDevPtr)) {
        return false;
    }
    args.flowStride = m_nvof[eye].GetFlowStrideBytes();
    args.flowGridSize = m_nvof[eye].GetGridSize();
    args.costMapDevPtr = m_nvof[eye].GetCostMapDevPtr();
    args.costStride = m_nvof[eye].GetCostStrideBytes();
    args.outSurf = dst.surfObj;
    args.outW = m_width;
    args.outH = m_height;
    args.outputFormat = outputFormat;
    for (int i = 0; i < 16; ++i) {
        args.pose.prevToPred[i] = prevToPred[i];
        args.pose.currToPred[i] = currToPred[i];
    }
    args.pose.tanLeft = tanLeft;
    args.pose.tanRight = tanRight;
    args.pose.tanDown = tanDown;
    args.pose.tanUp = tanUp;
    args.pose.nearZ = 0.02f;
    args.pose.refineStrength = m_refineStrength;
    args.pose.occlusionSharp = m_occlusionSharp;
    args.pose.foveation = m_foveation;
    args.pose.flowSmooth = m_flowSmooth;
    args.blend = blend;
    return m_warp.Launch(args, m_stream[eye]);
}

bool AerV2Pipeline::ProcessTemporalFrame(int eye,
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
                                         const XrPosef& prevPose,
                                         const XrPosef& currPose,
                                         const XrPosef& predictedPose,
                                         float blend,
                                         ID3D12Resource* outSynthColor) {
    if (eye < 0 || eye > 1 || !m_ready || m_mode == Mode::Off || !prevFlowInput || !currFlowInput || !prevColor || !currColor || !outSynthColor || !m_queue || !m_d3dFence) {
        return false;
    }

    CudaExternalMemory prevCUDA{};
    CudaExternalMemory currCUDA{};
    CudaExternalMemory prevColorCUDA{};
    CudaExternalMemory currColorCUDA{};
    CudaExternalMemory dstCUDA{};
    CudaExternalMemory mvCUDA{};
    CudaExternalMemory prevDepthCUDA{};
    CudaExternalMemory currDepthCUDA{};
    bool ok = false;
    uint64_t outputReadyValue = 0;  // fence value CUDA signals when this warp's output is done

    do {
        if (!ImportTexture(prevFlowInput, false, &prevCUDA) ||
            !ImportTexture(currFlowInput, false, &currCUDA) ||
            !ImportTexture(outSynthColor, true, &dstCUDA)) {
            LogAER("ImportTexture failed");
            break;
        }
        if (prevColor == prevFlowInput) {
            prevColorCUDA = prevCUDA;
            prevColorCUDA.extMem = nullptr;
            prevColorCUDA.source = nullptr;
        } else if (!ImportTexture(prevColor, false, &prevColorCUDA)) {
            LogAER("ImportTexture(prevColor) failed");
            break;
        }
        if (currColor == currFlowInput) {
            currColorCUDA = currCUDA;
            currColorCUDA.extMem = nullptr;
            currColorCUDA.source = nullptr;
        } else if (!ImportTexture(currColor, false, &currColorCUDA)) {
            LogAER("ImportTexture(currColor) failed");
            break;
        }
        if (engineMv && !ImportTexture(engineMv, false, &mvCUDA)) {
            LogAER("ImportTexture(engineMv) failed");
        }
        if (prevDepth && !ImportTexture(prevDepth, false, &prevDepthCUDA)) {
            LogAER("ImportTexture(prevDepth) failed");
        }
        if (currDepth && currDepth != prevDepth && !ImportTexture(currDepth, false, &currDepthCUDA)) {
            LogAER("ImportTexture(currDepth) failed");
        }

        // D3D12 -> CUDA visibility: signal the shared fence on the D3D queue,
        // then queue a CUDA wait on the same value before reading imported
        // textures.
        ++m_fenceValue;
        if (FAILED(m_queue->Signal(m_d3dFence.Get(), m_fenceValue))) {
            LogAER("ID3D12CommandQueue::Signal failed");
            break;
        }
        if (!m_cuda.WaitSemaphore(&m_cudaSem, m_fenceValue, m_stream[eye])) {
            LogAER("cudaWaitExternalSemaphoresAsync_v2 failed");
            break;
        }

        const Quat prevQ = ToQuat(prevPose.orientation);
        const Vec3 prevPos = ToVec3(prevPose.position);
        const Quat currQ = ToQuat(currPose.orientation);
        const Vec3 currPos = ToVec3(currPose.position);
        const Quat predQ = ToQuat(predictedPose.orientation);
        const Vec3 predPos = ToVec3(predictedPose.position);
        const float tanLeft = std::tanf(fov.angleLeft);
        const float tanRight = std::tanf(fov.angleRight);
        const float tanDown = std::tanf(fov.angleDown);
        const float tanUp = std::tanf(fov.angleUp);

        const uint32_t outFmt = static_cast<uint32_t>(outSynthColor->GetDesc().Format);
        const CudaExternalMemory* prevDepthPtr = prevDepthCUDA.extMem ? &prevDepthCUDA : nullptr;
        const CudaExternalMemory* currDepthPtr = nullptr;
        if (currDepth && currDepth == prevDepth) {
            currDepthPtr = prevDepthPtr;
        } else if (currDepthCUDA.extMem) {
            currDepthPtr = &currDepthCUDA;
        }
        const CudaExternalMemory& warpPrev = prevColor == prevFlowInput ? prevCUDA : prevColorCUDA;
        const CudaExternalMemory& warpCurr = currColor == currFlowInput ? currCUDA : currColorCUDA;
        ok = RunWarp(eye,
                     warpPrev,
                     warpCurr,
                     dstCUDA,
                     mvCUDA.extMem ? &mvCUDA : nullptr,
                     prevDepthPtr,
                     currDepthPtr,
                     mvScaleX,
                     mvScaleY,
                     prevQ,
                     prevPos,
                     currQ,
                     currPos,
                     predQ,
                     predPos,
                     tanLeft,
                     tanRight,
                     tanDown,
                     tanUp,
                     outFmt,
                     blend);
        if (!ok) {
            LogAER("RunWarp failed");
            break;
        }

        // CUDA → D3D12 visibility: signal the shared fence on the CUDA stream
        // when the warp finishes. Then queue a wait on the SAME m_d3dQueue. The
        // consumer's submit CopyResource also runs on m_d3dQueue, so FIFO queue
        // ordering guarantees it cannot read the synth texture before the CUDA
        // write completes, GPU-side.
        // NO CpuWaitForSingleObject here: blocking the present thread per warp
        // was the jitter cause. The function returns immediately after queueing.
        ++m_fenceValue;
        outputReadyValue = m_fenceValue;
        if (!m_cuda.SignalSemaphore(&m_cudaSem, m_fenceValue, m_stream[eye])) {
            LogAER("cudaSignalExternalSemaphoresAsync_v2 failed");
            ok = false;
            break;
        }
        if (FAILED(m_queue->Wait(m_d3dFence.Get(), m_fenceValue))) {
            LogAER("ID3D12CommandQueue::Wait failed");
            ok = false;
            break;
        }
    } while (false);

    // No per-frame release: all imports live in m_importCache (owned there) and
    // are reused next frame. The locals above are shallow copies of cached
    // handles, so they must NOT be freed here. The cache is released on
    // Shutdown/resize once the GPU is idle.
    (void)outputReadyValue;
    return ok;
}

} // namespace aer_v2
