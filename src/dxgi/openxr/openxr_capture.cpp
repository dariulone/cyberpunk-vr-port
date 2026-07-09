// openxr_capture.cpp - mono/AER frame + depth capture and submit-resource setup.
// Split verbatim from openxr_manager.cpp (OpenXRManager methods). Shared module
// state/helpers via openxr_internal.h (inline).
#include "openxr_manager.h"
#include "openxr_internal.h"
#include "openxr_math.h"
#include "shared_slots.h"
#include "runtime_fov_correction.h"
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <vector>
#include <utility>
#include <chrono>
#include <thread>
#include <memory>
#include <algorithm>
#include <dxgi1_4.h>

bool OpenXRManager::EnsureMonoCaptureResource(const D3D12_RESOURCE_DESC& sourceDesc) {
    if (!m_d3dDevice || !m_d3dQueue) {
        return false;
    }

    const uint32_t width = static_cast<uint32_t>(sourceDesc.Width);
    const uint32_t height = sourceDesc.Height;
    const uint32_t format = static_cast<uint32_t>(sourceDesc.Format);
    if (width == 0 || height == 0 || format == 0) {
        return false;
    }

    if (!m_captureCmdAllocators[0]) {
        for (int i = 0; i < 3; ++i) {
            if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_captureCmdAllocators[i])))) {
                Log("OpenXRManager: Failed to create mono capture command allocator %d\n", i);
                return false;
            }
            SetD3DName(m_captureCmdAllocators[i], L"OpenXR_capture_allocator");
        }
    }
    if (!m_captureCmdLists[0]) {
        for (int i = 0; i < 3; ++i) {
            if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_captureCmdAllocators[i], nullptr, IID_PPV_ARGS(&m_captureCmdLists[i])))) {
                Log("OpenXRManager: Failed to create mono capture command list %d\n", i);
                return false;
            }
            SetD3DName(m_captureCmdLists[i], L"OpenXR_capture_command_list");
            m_captureCmdLists[i]->Close();
        }
    }
    if (!m_captureFence) {
        if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_captureFence)))) {
            Log("OpenXRManager: Failed to create mono capture fence\n");
            return false;
        }
        SetD3DName(m_captureFence, L"OpenXR_capture_fence");
    }
    if (!m_captureFenceEvent) {
        m_captureFenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!m_captureFenceEvent) {
            Log("OpenXRManager: Failed to create mono capture fence event\n");
            return false;
        }
    }

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        if (m_monoCapturedFrame.texture &&
            m_monoCapturedFrame.width == width &&
            m_monoCapturedFrame.height == height &&
            m_monoCapturedFrame.format == format) {
            return true;
        }

        if (m_monoCapturedFrame.texture) {
            m_monoCapturedFrame.texture->Release();
            m_monoCapturedFrame.texture = nullptr;
        }
        m_monoCapturedFrame.width = 0;
        m_monoCapturedFrame.height = 0;
        m_monoCapturedFrame.format = 0;
        m_monoCapturedFrame.serial = 0;
        m_monoCapturedFrame.hasView[0] = false;
        m_monoCapturedFrame.hasView[1] = false;
    }

    D3D12_HEAP_PROPERTIES heapProps{};
    heapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
    ID3D12Resource* texture = nullptr;
    if (FAILED(m_d3dDevice->CreateCommittedResource(
            &heapProps,
            D3D12_HEAP_FLAG_NONE,
            &sourceDesc,
            D3D12_RESOURCE_STATE_COPY_DEST,
            nullptr,
            IID_PPV_ARGS(&texture)))) {
        Log("OpenXRManager: Failed to create mono captured texture\n");
        return false;
    }
    SetD3DName(texture, L"OpenXR_mono_snapshot_color");

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        m_monoCapturedFrame.texture = texture;
        m_monoCapturedFrame.width = width;
        m_monoCapturedFrame.height = height;
        m_monoCapturedFrame.format = format;
        m_monoCapturedFrame.serial = 0;
        m_monoCapturedFrame.hasView[0] = false;
        m_monoCapturedFrame.hasView[1] = false;
    }

    Log("OpenXRManager: Mono snapshot resource ready. size=%ux%u format=%u\n", width, height, format);
    return true;
}

// [DEPTH] Accessors implemented in dxgi_factory_wrapper.cpp — the game's pinned
// scene depth resource and its CURRENT (observed) D3D12 resource state.
extern "C" ID3D12Resource* OmoGetSceneDepthResource();
extern "C" unsigned int OmoGetSceneDepthState();
extern "C" unsigned int OmoGetSceneDepthWidth();
extern "C" unsigned int OmoGetSceneDepthHeight();
extern "C" unsigned int OmoGetSceneDepthFormat();
extern "C" ID3D12CommandQueue* OmoGetSceneDepthWriterQueue(); // game's depth-writer queue (safe mono depth capture)

bool OpenXRManager::EnsureDepthSnapshot(ID3D12Resource* gameDepth) {
    if (!gameDepth || !m_d3dDevice) {
        return false;
    }
    // Depth submit is OFF by default (xr_depth_submit=0). Copying the game's LIVE
    // scene-depth resource on our capture queue races the game's own queue (which is
    // simultaneously writing DepthPrepass/GBuffer and reallocating render targets on
    // load/spawn). That cross-queue access caused GPU device-hung (0x887a0006) under
    // VDXR, where the scene depth is an R32-family format the snapshot path accepts.
    // depth gave no confirmed benefit (the left-eye fix was the alternate-eye pose-pair
    // lock, not depth), so keep it gated unless explicitly re-enabled for experiments.
    if (GetDepthSubmit() == 0) {
        if (m_depthLayerSupported) {
            Log("OpenXRManager: [DEPTH] depth submit disabled (xr_depth_submit=0)\n");
        }
        m_depthLayerSupported = false;
        m_depthSwapchainFormat = 0;
        m_depthSnapshotSerial = 0;
        return false;
    }
    const D3D12_RESOURCE_DESC desc = gameDepth->GetDesc();
    // Accept both R32 (32bpp) and R32G8X24 (64bpp) source families. The 64bpp
    // path uses DepthResolve shader to extract plane 0 (the float depth) into
    // a 32bpp D32_FLOAT snapshot which is bit-compatible with the standard
    // depth swapchain — no more DEVICE_HUNG, no more sceneindentation depth=0
    // in submit logs. Old comment about "TYPELESS snapshot required" is
    // obsolete: now the snapshot is typed D32_FLOAT, populated by shader.
    const bool acceptable32 =
        desc.Format == DXGI_FORMAT_R32_TYPELESS ||
        desc.Format == DXGI_FORMAT_D32_FLOAT ||
        desc.Format == DXGI_FORMAT_R32_FLOAT;
    const bool acceptable64 =
        desc.Format == DXGI_FORMAT_R32G8X24_TYPELESS ||
        desc.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
        desc.Format == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS ||
        desc.Format == DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;
    if (!acceptable32 && !acceptable64) {
        if (m_depthLayerSupported) {
            Log("OpenXRManager: [DEPTH] disabling depth layer for unsupported source format=%u\n",
                static_cast<unsigned>(desc.Format));
        }
        m_depthLayerSupported = false;
        m_depthSwapchainFormat = 0;
        m_depthSnapshotSerial = 0;
        return false;
    }
    // Snapshot always D32_FLOAT 32bpp now. For R32-family sources the capture
    // path uses CopyTextureRegion (bit-compat). For 64bpp sources the capture
    // path uses DepthResolve shader (plane 0 extract). Either way, downstream
    // depth swapchain copy works with a single typed format.
    const DXGI_FORMAT snapshotFormat = DXGI_FORMAT_D32_FLOAT;
    // Ensure the CUDA-importable R32 depth exists whenever AER V2 is on, even when
    // the D32 snapshot already exists (otherwise the early-return below skipped its
    // creation and depth-aware silently fell back to depth-free).
    auto ensureR32 = [&](const D3D12_RESOURCE_DESC& d) {
        if (GetAERV2Enabled() == 0) return;
        if (m_depthSnapshotR32) {
            const auto cur = m_depthSnapshotR32->GetDesc();
            if (cur.Width == d.Width && cur.Height == d.Height) return;
            m_depthSnapshotR32->Release();
            m_depthSnapshotR32 = nullptr;
            m_depthSnapshotR32Serial = 0;
        }
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC rd = d;
        rd.Format = DXGI_FORMAT_R32_FLOAT;
        rd.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE cv{}; cv.Format = DXGI_FORMAT_R32_FLOAT;
        if (FAILED(m_d3dDevice->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_SHARED, &rd,
                D3D12_RESOURCE_STATE_COMMON, &cv, IID_PPV_ARGS(&m_depthSnapshotR32)))) {
            Log("OpenXRManager: [DEPTH-AERV2] R32 snapshot create failed (depth-aware off)\n");
            m_depthSnapshotR32 = nullptr;
        } else {
            SetD3DName(m_depthSnapshotR32, L"AERV2_scene_depth_R32_cuda");
            Log("OpenXRManager: [DEPTH-AERV2] R32 depth snapshot created %llux%u\n",
                static_cast<unsigned long long>(d.Width), d.Height);
        }
    };
    if (m_depthSnapshot) {
        const D3D12_RESOURCE_DESC cur = m_depthSnapshot->GetDesc();
        if (cur.Width == desc.Width && cur.Height == desc.Height && cur.Format == snapshotFormat) {
            ensureR32(desc);
            return true;
        }
        m_depthSnapshot->Release();
        m_depthSnapshot = nullptr;
        m_depthSnapshotSerial = 0;
    }
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC sd = desc;
    sd.Format = snapshotFormat;
    sd.Flags = D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
    // Typed depth resources with ALLOW_DEPTH_STENCIL require a clear value.
    D3D12_CLEAR_VALUE clearVal{};
    clearVal.Format = snapshotFormat;
    clearVal.DepthStencil.Depth = 1.0f;
    clearVal.DepthStencil.Stencil = 0;
    const HRESULT hr = m_d3dDevice->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED, &sd,
        D3D12_RESOURCE_STATE_COPY_DEST, &clearVal, IID_PPV_ARGS(&m_depthSnapshot));
    if (FAILED(hr)) {
        Log("OpenXRManager: [DEPTH] CreateCommittedResource(depthSnapshot) failed hr=0x%08X\n", hr);
        m_depthSnapshot = nullptr;
        return false;
    }
    m_depthSnapshotW = static_cast<uint32_t>(desc.Width);
    m_depthSnapshotH = desc.Height;
    m_depthSnapshotSerial = 0;
    SetD3DName(m_depthSnapshot, L"OpenXR_scene_depth_snapshot");
    Log("OpenXRManager: [DEPTH] snapshot created %llux%u srcFmt=%u snapFmt=%u\n",
        static_cast<unsigned long long>(desc.Width), desc.Height,
        static_cast<unsigned>(desc.Format), static_cast<unsigned>(snapshotFormat));

    // [DEPTH-AERV2] Parallel R32_FLOAT COLOR snapshot (CUDA-importable). Only the
    // AER V2 warp uses it; failure is non-fatal (warp falls back to depth-free).
    ensureR32(desc);
    return true;
}

bool OpenXRManager::RecordDepthCapture(ID3D12GraphicsCommandList* cmdList,
                                       ID3D12Resource* gameDepth,
                                       D3D12_RESOURCE_STATES gameDepthState,
                                       bool transitionGameDepth) {
    if (!cmdList || !gameDepth || !m_depthSnapshot) return false;
    const DXGI_FORMAT srcFmt = gameDepth->GetDesc().Format;
    const bool is64bpp =
        srcFmt == DXGI_FORMAT_R32G8X24_TYPELESS ||
        srcFmt == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
        srcFmt == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS ||
        srcFmt == DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;

    if (!is64bpp) {
        // 32bpp path needs the source in COPY_SOURCE. When we're not allowed to
        // transition the game resource, only proceed if it is already there.
        if (!transitionGameDepth && gameDepthState != D3D12_RESOURCE_STATE_COPY_SOURCE) {
            return false;
        }
        // 32bpp path: simple CopyTextureRegion (bit-compat between R32/D32).
        D3D12_RESOURCE_BARRIER pre[2] = {};
        UINT preCount = 0;
        if (transitionGameDepth && gameDepthState != D3D12_RESOURCE_STATE_COPY_SOURCE) {
            pre[preCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            pre[preCount].Transition.pResource = gameDepth;
            pre[preCount].Transition.StateBefore = gameDepthState;
            pre[preCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
            pre[preCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            ++preCount;
        }
        if (m_depthSnapshotSerial != 0) {
            pre[preCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            pre[preCount].Transition.pResource = m_depthSnapshot;
            pre[preCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
            pre[preCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
            pre[preCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            ++preCount;
        }
        if (preCount > 0) cmdList->ResourceBarrier(preCount, pre);
        D3D12_TEXTURE_COPY_LOCATION dst{};
        dst.pResource = m_depthSnapshot;
        dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        dst.SubresourceIndex = 0;
        D3D12_TEXTURE_COPY_LOCATION src{};
        src.pResource = gameDepth;
        src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        src.SubresourceIndex = 0;
        cmdList->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        D3D12_RESOURCE_BARRIER post[2] = {};
        UINT postCount = 0;
        post[postCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        post[postCount].Transition.pResource = m_depthSnapshot;
        post[postCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
        post[postCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
        post[postCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++postCount;
        if (transitionGameDepth && gameDepthState != D3D12_RESOURCE_STATE_COPY_SOURCE) {
            post[postCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            post[postCount].Transition.pResource = gameDepth;
            post[postCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
            post[postCount].Transition.StateAfter = gameDepthState;
            post[postCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            ++postCount;
        }
        cmdList->ResourceBarrier(postCount, post);
        return true;
    }

    // 64bpp path: shader resolve plane 0 → D32_FLOAT DSV.
    if (!m_depthResolve) m_depthResolve = std::make_unique<DepthResolve>();
    if (!m_depthResolve->EnsureInitialized(m_d3dDevice, DXGI_FORMAT_D32_FLOAT,
            m_depthSnapshotW, m_depthSnapshotH)) {
        return false;
    }

    D3D12_RESOURCE_BARRIER pre[2] = {};
    UINT preCount = 0;
    if (transitionGameDepth && gameDepthState != D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE) {
        pre[preCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        pre[preCount].Transition.pResource = gameDepth;
        pre[preCount].Transition.StateBefore = gameDepthState;
        pre[preCount].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        pre[preCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++preCount;
    }
    // Snapshot was last left in COPY_SOURCE if we've written before; first
    // time it's in COPY_DEST (created with that state). Either way go to
    // DEPTH_WRITE for the resolve draw.
    pre[preCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    pre[preCount].Transition.pResource = m_depthSnapshot;
    pre[preCount].Transition.StateBefore = (m_depthSnapshotSerial != 0)
        ? D3D12_RESOURCE_STATE_COPY_SOURCE
        : D3D12_RESOURCE_STATE_COPY_DEST;
    pre[preCount].Transition.StateAfter = D3D12_RESOURCE_STATE_DEPTH_WRITE;
    pre[preCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++preCount;
    cmdList->ResourceBarrier(preCount, pre);

    const bool ok = m_depthResolve->RecordResolve(cmdList, gameDepth, m_depthSnapshot);

    // [DEPTH-AERV2] Second resolve of the SAME gameDepth SRV (still in
    // PIXEL_SHADER_RESOURCE) into the plain R32_FLOAT color snapshot that CUDA can
    // import. Self-contained barriers on m_depthSnapshotR32 only; does not touch
    // the depth-output path above. Non-fatal.
    if (m_depthSnapshotR32) {
        D3D12_RESOURCE_BARRIER rtBar{};
        rtBar.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        rtBar.Transition.pResource = m_depthSnapshotR32;
        rtBar.Transition.StateBefore = (m_depthSnapshotR32Serial != 0)
            ? D3D12_RESOURCE_STATE_COPY_SOURCE : D3D12_RESOURCE_STATE_COMMON;
        rtBar.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
        rtBar.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        cmdList->ResourceBarrier(1, &rtBar);

        m_depthResolve->RecordResolveColor(cmdList, gameDepth, m_depthSnapshotR32);

        rtBar.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
        rtBar.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
        cmdList->ResourceBarrier(1, &rtBar);
        m_depthSnapshotR32Serial = 1;  // marked produced; serial set on publish below
    }

    D3D12_RESOURCE_BARRIER post[2] = {};
    UINT postCount = 0;
    post[postCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    post[postCount].Transition.pResource = m_depthSnapshot;
    post[postCount].Transition.StateBefore = D3D12_RESOURCE_STATE_DEPTH_WRITE;
    post[postCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    post[postCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++postCount;
    if (transitionGameDepth && gameDepthState != D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE) {
        post[postCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        post[postCount].Transition.pResource = gameDepth;
        post[postCount].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        post[postCount].Transition.StateAfter = gameDepthState;
        post[postCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++postCount;
    }
    cmdList->ResourceBarrier(postCount, post);
    return ok;
}

bool OpenXRManager::CaptureMonoDepthOnWriterQueue(uint64_t serial) {
    if (GetMonoDepthCapture() == 0) return false;
    ID3D12Resource* gameDepth = OmoGetSceneDepthResource();
    ID3D12CommandQueue* writerQueue = OmoGetSceneDepthWriterQueue();
    const D3D12_RESOURCE_STATES gameDepthState = static_cast<D3D12_RESOURCE_STATES>(OmoGetSceneDepthState());
    // No writer queue discovered yet (or no explicit depth state observed) -> skip this
    // frame. This is a soft skip (no depth submitted), never a hang.
    if (!m_d3dDevice || !gameDepth || !writerQueue || OmoGetSceneDepthState() == 0) return false;
    if (!EnsureDepthSnapshot(gameDepth)) return false;

    // Lazily create the dedicated resolve list + fence (DIRECT; the writer queue is a
    // graphics queue in CP2077).
    if (!m_depthWriterAlloc) {
        if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_depthWriterAlloc)))) return false;
        SetD3DName(m_depthWriterAlloc, L"OpenXR_depth_writer_alloc");
    }
    if (!m_depthWriterList) {
        if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_depthWriterAlloc, nullptr, IID_PPV_ARGS(&m_depthWriterList)))) return false;
        SetD3DName(m_depthWriterList, L"OpenXR_depth_writer_list");
        m_depthWriterList->Close();
    }
    if (!m_depthWriterFence) {
        if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_depthWriterFence)))) return false;
        SetD3DName(m_depthWriterFence, L"OpenXR_depth_writer_fence");
    }
    // Single-slot: if the previous resolve hasn't finished, skip (don't stomp the list
    // or m_depthSnapshot while the GPU still reads/writes it).
    if (m_depthWriterFenceValue != 0 && m_depthWriterFence->GetCompletedValue() < m_depthWriterFenceValue) {
        return false;
    }
    if (FAILED(m_depthWriterAlloc->Reset()) || FAILED(m_depthWriterList->Reset(m_depthWriterAlloc, nullptr))) {
        return false;
    }
    const bool ok = RecordDepthCapture(m_depthWriterList, gameDepth, gameDepthState);
    m_depthWriterList->Close();
    if (!ok) return false;

    // Execute on the GAME's depth-writer queue: naturally ordered AFTER the depth write
    // that just ran on the same queue (FIFO), so no cross-queue Wait and no hang.
    ID3D12CommandList* lists[] = { m_depthWriterList };
    writerQueue->ExecuteCommandLists(1, lists);
    const UINT64 fv = ++m_depthWriterFenceValue;
    writerQueue->Signal(m_depthWriterFence, fv);
    m_depthSnapshotWriterFence = fv;  // submit path waits on this before reading m_depthSnapshot
    return true;
}

bool OpenXRManager::CaptureMonoPresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, uint64_t serial,
    const XrPosef poses[2], const XrFovf fovs[2], const bool hasView[2]) {
    if (!backBuffer || !hasView[0] || !hasView[1]) {
        return false;
    }

    std::lock_guard<std::mutex> captureLock(m_captureMutex);
    if (!EnsureMonoCaptureResource(sourceDesc)) {
        return false;
    }

    ID3D12Resource* snapshot = nullptr;
    uint64_t previousSerial = 0;
    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        snapshot = m_monoCapturedFrame.texture;
        previousSerial = m_monoCapturedFrame.serial;
        if (snapshot) {
            snapshot->AddRef();
        }
    }
    if (!snapshot) {
        return false;
    }

    m_captureAllocatorIndex = (m_captureAllocatorIndex + 1) % 3;
    ID3D12CommandAllocator* currentAllocator = m_captureCmdAllocators[m_captureAllocatorIndex];
    
    if (m_captureFenceValue >= 3 && m_captureFence->GetCompletedValue() < m_captureFenceValue - 2) {
        m_captureFence->SetEventOnCompletion(m_captureFenceValue - 2, m_captureFenceEvent);
        WaitForSingleObject(m_captureFenceEvent, INFINITE);
    }

    ID3D12GraphicsCommandList* m_captureCmdList = m_captureCmdLists[m_captureAllocatorIndex];

    if (FAILED(currentAllocator->Reset()) || FAILED(m_captureCmdList->Reset(currentAllocator, nullptr))) {
        Log("OpenXRManager: Failed to reset mono capture command list\n");
        snapshot->Release();
        return false;
    }

    D3D12_RESOURCE_BARRIER barriers[2] = {};
    UINT barrierCount = 0;

    barriers[barrierCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[barrierCount].Transition.pResource = backBuffer;
    barriers[barrierCount].Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    barriers[barrierCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barriers[barrierCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++barrierCount;

    if (previousSerial != 0) {
        barriers[barrierCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barriers[barrierCount].Transition.pResource = snapshot;
        barriers[barrierCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
        barriers[barrierCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
        barriers[barrierCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++barrierCount;
    }

    m_captureCmdList->ResourceBarrier(barrierCount, barriers);
    m_captureCmdList->CopyResource(snapshot, backBuffer);

    D3D12_RESOURCE_BARRIER afterCopy[2] = {};
    afterCopy[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[0].Transition.pResource = snapshot;
    afterCopy[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    afterCopy[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    afterCopy[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[1].Transition.pResource = backBuffer;
    afterCopy[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[1].Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    afterCopy[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    m_captureCmdList->ResourceBarrier(2, afterCopy);

    // [DEPTH] Scene-depth snapshot for XR_KHR_composition_layer_depth, recorded on THIS
    // (capture) list so it is FIFO-ordered before the mono submit's depth copy on the
    // SAME queue -> no cross-queue Wait, no fence cycle.
    // CRITICAL: we do NOT transition the game depth (transitionGameDepth=false). D3D12
    // resource state is GLOBAL; issuing a barrier on the game's depth from our side
    // corrupts the state the game itself tracks and device-removes -> froze CP2077,
    // worst at intro / menu-load where the depth resource + state are transient (as the
    // user observed). Instead the resolve reads it as an SRV in whatever shader-readable
    // state the game already left it, so we only capture when:
    //   * mono depth is enabled,
    //   * a menu is NOT open,
    //   * the scene-depth has been the SAME resource (menus closed) for a warmup window
    //     -> skips the intro/menu-load transient depth entirely, and
    //   * it is shader-readable THIS frame.
    // No game state is ever touched => device-remove is impossible; a rare torn read is
    // a harmless one-frame reprojection hint.
    bool depthCaptured = false;
    if (GetMonoDepthCapture() != 0) {
        ID3D12Resource* gameDepth = OmoGetSceneDepthResource();
        const UINT depthStateRaw = OmoGetSceneDepthState();
        const bool menuOpen = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
        const bool srvReadable = (depthStateRaw & D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE) != 0;
        static ID3D12Resource* s_depthGateRes = nullptr;
        static uint32_t s_depthGateStable = 0;
        if (gameDepth && gameDepth == s_depthGateRes && !menuOpen) {
            if (s_depthGateStable < 100000u) ++s_depthGateStable;
        } else {
            s_depthGateStable = 0;   // resource changed / menu open -> restart warmup
        }
        s_depthGateRes = gameDepth;
        const bool gateOk = gameDepth && !menuOpen && s_depthGateStable >= 60; // ~1s stable gameplay depth
        if (gateOk && srvReadable && EnsureDepthSnapshot(gameDepth)) {
            depthCaptured = RecordDepthCapture(m_captureCmdList, gameDepth,
                static_cast<D3D12_RESOURCE_STATES>(depthStateRaw), /*transitionGameDepth=*/false);
        }
    }

    m_captureCmdList->Close();
    ID3D12CommandList* cmdLists[] = {m_captureCmdList};
    m_d3dQueue->ExecuteCommandLists(1, cmdLists);

    ++m_captureFenceValue;
    m_d3dQueue->Signal(m_captureFence, m_captureFenceValue);

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        if (m_monoCapturedFrame.texture == snapshot) {
            m_monoCapturedFrame.serial = serial;
            for (int eye = 0; eye < 2; ++eye) {
                m_monoCapturedFrame.poses[eye] = poses[eye];
                m_monoCapturedFrame.fovs[eye] = fovs[eye];
                m_monoCapturedFrame.hasView[eye] = hasView[eye];
            }
            SetD3DNamef(m_monoCapturedFrame.texture, L"OpenXR_mono_snapshot_serial%llu",
                static_cast<unsigned long long>(serial));
            if (depthCaptured) {
                m_depthSnapshotSerial = serial;
                if (m_depthSnapshotR32) m_depthSnapshotR32Serial = serial;
            }
        }
    }
    if (m_monoPresentEvent) {
        SetEvent(m_monoPresentEvent);
    }

    snapshot->Release();
    if (g_verboseLog && (serial % 300) == 1) {
        Log("OpenXRManager: Mono frame captured. serial=%llu\n",
            static_cast<unsigned long long>(serial));
    }
    return true;
}

bool OpenXRManager::EnsureAERCaptureResources(const D3D12_RESOURCE_DESC& sourceDesc) {
    if (!m_d3dDevice || !m_d3dQueue) {
        return false;
    }

    const uint32_t width = static_cast<uint32_t>(sourceDesc.Width);
    const uint32_t height = sourceDesc.Height;
    const uint32_t format = static_cast<uint32_t>(sourceDesc.Format);
    if (width == 0 || height == 0 || format == 0) {
        return false;
    }

    if (!m_captureCmdAllocators[0]) {
        for (int i = 0; i < 3; ++i) {
            if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_captureCmdAllocators[i])))) {
                Log("OpenXRManager: Failed to create AER capture command allocator %d\n", i);
                return false;
            }
            SetD3DName(m_captureCmdAllocators[i], L"AERV2_capture_allocator");
        }
    }
    if (!m_captureCmdLists[0]) {
        for (int i = 0; i < 3; ++i) {
            if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_captureCmdAllocators[i], nullptr, IID_PPV_ARGS(&m_captureCmdLists[i])))) {
                Log("OpenXRManager: Failed to create AER capture command list %d\n", i);
                return false;
            }
            SetD3DName(m_captureCmdLists[i], L"AERV2_capture_command_list");
            m_captureCmdLists[i]->Close();
        }
    }
    if (!m_captureFence) {
        if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_captureFence)))) {
            Log("OpenXRManager: Failed to create AER capture fence\n");
            return false;
        }
        SetD3DName(m_captureFence, L"AERV2_capture_fence");
    }
    if (!m_captureFenceEvent) {
        m_captureFenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!m_captureFenceEvent) {
            Log("OpenXRManager: Failed to create AER capture fence event\n");
            return false;
        }
    }

    const bool aerV2Enabled = GetAERV2Enabled() != 0;
    const DXGI_FORMAT opticalFlowFormat = GetAERV2OpticalFlowFormat(sourceDesc.Format);
    auto framesMatch = [width, height, format, aerV2Enabled](const CapturedEyeFrame* frames) {
        for (int eye = 0; eye < 2; ++eye) {
            const CapturedEyeFrame& frame = frames[eye];
            if (!frame.texture || frame.width != width || frame.height != height || frame.format != format) {
                return false;
            }
            if (aerV2Enabled && !frame.textureShareable) {
                return false;
            }
        }
        return true;
    };
    auto opticalFlowFramesMatch = [aerV2Enabled, opticalFlowFormat](const CapturedEyeFrame* frames) {
        if (!aerV2Enabled) {
            return true;
        }
        for (int eye = 0; eye < 2; ++eye) {
            if (!frames[eye].opticalFlowTexture) {
                return false;
            }
            if (frames[eye].opticalFlowTexture->GetDesc().Format != opticalFlowFormat) {
                return false;
            }
        }
        return true;
    };

    const bool texturesMatch =
        framesMatch(m_capturedEyeFrames) &&
        framesMatch(m_previousCapturedEyeFrames) &&
        framesMatch(m_pendingEyeFrames) &&
        opticalFlowFramesMatch(m_capturedEyeFrames) &&
        opticalFlowFramesMatch(m_previousCapturedEyeFrames) &&
        opticalFlowFramesMatch(m_pendingEyeFrames);
    if (texturesMatch) {
        return true;
    }

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        auto releaseFrames = [](CapturedEyeFrame* frames) {
            for (int eye = 0; eye < 2; ++eye) {
                CapturedEyeFrame& frame = frames[eye];
                if (frame.texture) {
                    frame.texture->Release();
                    frame.texture = nullptr;
                }
                if (frame.opticalFlowTexture) {
                    frame.opticalFlowTexture->Release();
                    frame.opticalFlowTexture = nullptr;
                }
                if (frame.depthTexture) {
                    frame.depthTexture->Release();
                    frame.depthTexture = nullptr;
                }
                frame.width = 0;
                frame.height = 0;
                frame.format = 0;
                frame.textureShareable = false;
                frame.depthWidth = 0;
                frame.depthHeight = 0;
                frame.depthFormat = 0;
                frame.serial = 0;
                frame.pairId = 0;
                frame.depthSerial = 0;
                // Reset the convert-fence value: the optical-flow module (and thus its
                // convert fence) is rebuilt on resolution change, so a stale value here
                // would make the frame thread's GPU-Wait block forever on a fence that
                // never reaches it.
                frame.opticalFlowConvertValue = 0;
                frame.depthInCopySource = false;
                frame.pose = {};
                frame.pose.orientation.w = 1.0f;
                frame.fov = {};
                frame.hasView = false;
            }
        };
        releaseFrames(m_capturedEyeFrames);
        releaseFrames(m_previousCapturedEyeFrames);
        releaseFrames(m_pendingEyeFrames);
    }

    D3D12_HEAP_PROPERTIES heapProps{};
    heapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
    const D3D12_HEAP_FLAGS sharedHeapFlags = aerV2Enabled ? D3D12_HEAP_FLAG_SHARED : D3D12_HEAP_FLAG_NONE;
    D3D12_RESOURCE_DESC opticalFlowDesc{};
    opticalFlowDesc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    opticalFlowDesc.Width = width;
    opticalFlowDesc.Height = height;
    opticalFlowDesc.DepthOrArraySize = 1;
    opticalFlowDesc.MipLevels = 1;
    opticalFlowDesc.Format = opticalFlowFormat;
    opticalFlowDesc.SampleDesc.Count = 1;
    opticalFlowDesc.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
    opticalFlowDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    auto createFrames = [&](CapturedEyeFrame* frames, const char* label, const wchar_t* nameLabel) {
        for (int eye = 0; eye < 2; ++eye) {
            CapturedEyeFrame& frame = frames[eye];
            if (FAILED(m_d3dDevice->CreateCommittedResource(
                    &heapProps,
                    sharedHeapFlags,
                    &sourceDesc,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    nullptr,
                    IID_PPV_ARGS(&frame.texture)))) {
                Log("OpenXRManager: Failed to create AER %s texture\n", label);
                return false;
            }
            SetD3DNamef(frame.texture, L"AERV2_%ls_eye%d_color", nameLabel, eye);
            frame.textureShareable = aerV2Enabled;
            if (aerV2Enabled) {
                if (FAILED(m_d3dDevice->CreateCommittedResource(
                        &heapProps,
                        sharedHeapFlags,
                        &opticalFlowDesc,
                        D3D12_RESOURCE_STATE_COMMON,
                        nullptr,
                        IID_PPV_ARGS(&frame.opticalFlowTexture)))) {
                    Log("OpenXRManager: Failed to create AER V2 %s optical-flow texture\n", label);
                    return false;
                }
                SetD3DNamef(frame.opticalFlowTexture, L"AERV2_%ls_eye%d_ofinput", nameLabel, eye);
            }

            frame.width = width;
            frame.height = height;
            frame.format = format;
            frame.depthWidth = 0;
            frame.depthHeight = 0;
            frame.depthFormat = 0;
            frame.serial = 0;
            frame.pairId = 0;
            frame.depthSerial = 0;
            frame.depthInCopySource = false;
            frame.pose = {};
            frame.pose.orientation.w = 1.0f;
            frame.fov = {};
            frame.hasView = false;
        }
        return true;
    };

    if (!createFrames(m_capturedEyeFrames, "completed", L"completed") ||
        !createFrames(m_previousCapturedEyeFrames, "previous", L"previous") ||
        !createFrames(m_pendingEyeFrames, "pending", L"pending")) {
        return false;
    }

    // Phase 3: scratch RT for depth-based stereo reprojection. Always created
    // for AER (independent of V2 NvOF synth flag) because the reprojection
    // path is the FPS-per-eye fix and runs unconditionally on AER submit.
    {
        D3D12_RESOURCE_DESC stereoDesc = sourceDesc;
        stereoDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE stereoClear{};
        stereoClear.Format = sourceDesc.Format;
        bool recreate = true;
        if (m_stereoSynthEye) {
            auto cur = m_stereoSynthEye->GetDesc();
            if (cur.Width == stereoDesc.Width && cur.Height == stereoDesc.Height &&
                cur.Format == stereoDesc.Format) {
                recreate = false;
            } else {
                m_stereoSynthEye.Reset();
            }
        }
        if (recreate) {
            if (FAILED(m_d3dDevice->CreateCommittedResource(
                    &heapProps, D3D12_HEAP_FLAG_NONE,
                    &stereoDesc, D3D12_RESOURCE_STATE_COMMON,
                    &stereoClear, IID_PPV_ARGS(&m_stereoSynthEye)))) {
                Log("OpenXRManager: Failed to create stereo-synth scratch\n");
                return false;
            }
            SetD3DName(m_stereoSynthEye.Get(), L"AER_stereo_synth_scratch");
        }
    }

    // Phase 2b output target: scratch RT for the MV-warp pass. Only created
    // when AER V2 is enabled (the warp consumes engine MV captured via NGX
    // hook, which only matters for V2 frame-gen).
    if (aerV2Enabled) {
        // alternate-eye NvOF midpoint outputs. These are imported into CUDA as
        // writable surfaces, then copied into the XR eye swapchain on submit.
        // Kept in logical COPY_SOURCE state so submit can CopyResource without
        // extra barriers after CUDA signals the shared fence.
        D3D12_RESOURCE_DESC synthDesc = sourceDesc;
        synthDesc.Flags = D3D12_RESOURCE_FLAG_NONE;
        for (int eye = 0; eye < 2; ++eye) {
            for (int slot = 0; slot < 2; ++slot) {
                if (m_aerV2SynthEye[eye][slot]) {
                    auto cur = m_aerV2SynthEye[eye][slot]->GetDesc();
                    if (cur.Width == synthDesc.Width && cur.Height == synthDesc.Height && cur.Format == synthDesc.Format) {
                        continue;
                    }
                    m_aerV2SynthEye[eye][slot].Reset();
                }
                if (FAILED(m_d3dDevice->CreateCommittedResource(
                        &heapProps,
                        sharedHeapFlags,
                        &synthDesc,
                        D3D12_RESOURCE_STATE_COPY_SOURCE,
                        nullptr,
                        IID_PPV_ARGS(&m_aerV2SynthEye[eye][slot])))) {
                    Log("OpenXRManager: Failed to create AER V2 NvOF synth eye=%d slot=%d\n", eye, slot);
                    return false;
                }
                SetD3DNamef(m_aerV2SynthEye[eye][slot].Get(), L"AERV2_nvof_synth_eye%d_slot%d", eye, slot);

                if (m_aerV2SubmitEye[eye][slot]) {
                    auto cur = m_aerV2SubmitEye[eye][slot]->GetDesc();
                    if (!(cur.Width == synthDesc.Width && cur.Height == synthDesc.Height && cur.Format == synthDesc.Format)) {
                        m_aerV2SubmitEye[eye][slot].Reset();
                    }
                }
                if (!m_aerV2SubmitEye[eye][slot]) {
                    if (FAILED(m_d3dDevice->CreateCommittedResource(
                            &heapProps,
                            D3D12_HEAP_FLAG_NONE,
                            &synthDesc,
                            D3D12_RESOURCE_STATE_COMMON,
                            nullptr,
                            IID_PPV_ARGS(&m_aerV2SubmitEye[eye][slot])))) {
                        Log("OpenXRManager: Failed to create AER V2 submit eye=%d slot=%d\n", eye, slot);
                        return false;
                    }
                    SetD3DNamef(m_aerV2SubmitEye[eye][slot].Get(), L"AERV2_submit_synth_eye%d_slot%d", eye, slot);
                    m_aerV2SubmitEyeReady[eye][slot] = false;
                }

                // Half-rate in-between frame for both eyes: identical desc to the
                // synth eye. CUDA writes m_aerV2InBetween (COPY_SOURCE);
                // submit copies via m_aerV2InBetweenSubmit so submit and CUDA
                // never touch the same resource concurrently.
                if (m_aerV2InBetween[eye][slot]) {
                    auto cur = m_aerV2InBetween[eye][slot]->GetDesc();
                    if (!(cur.Width == synthDesc.Width && cur.Height == synthDesc.Height && cur.Format == synthDesc.Format)) {
                        m_aerV2InBetween[eye][slot].Reset();
                    }
                }
                if (!m_aerV2InBetween[eye][slot]) {
                    if (FAILED(m_d3dDevice->CreateCommittedResource(
                            &heapProps,
                            sharedHeapFlags,
                            &synthDesc,
                            D3D12_RESOURCE_STATE_COPY_SOURCE,
                            nullptr,
                            IID_PPV_ARGS(&m_aerV2InBetween[eye][slot])))) {
                        Log("OpenXRManager: Failed to create AER V2 in-between eye=%d slot=%d\n", eye, slot);
                        return false;
                    }
                    SetD3DNamef(m_aerV2InBetween[eye][slot].Get(), L"AERV2_nvof_inbetween_eye%d_slot%d", eye, slot);
                }
                if (m_aerV2InBetweenSubmit[eye][slot]) {
                    auto cur = m_aerV2InBetweenSubmit[eye][slot]->GetDesc();
                    if (!(cur.Width == synthDesc.Width && cur.Height == synthDesc.Height && cur.Format == synthDesc.Format)) {
                        m_aerV2InBetweenSubmit[eye][slot].Reset();
                    }
                }
                if (!m_aerV2InBetweenSubmit[eye][slot]) {
                    if (FAILED(m_d3dDevice->CreateCommittedResource(
                            &heapProps,
                            D3D12_HEAP_FLAG_NONE,
                            &synthDesc,
                            D3D12_RESOURCE_STATE_COMMON,
                            nullptr,
                            IID_PPV_ARGS(&m_aerV2InBetweenSubmit[eye][slot])))) {
                        Log("OpenXRManager: Failed to create AER V2 in-between submit eye=%d slot=%d\n", eye, slot);
                        return false;
                    }
                    SetD3DNamef(m_aerV2InBetweenSubmit[eye][slot].Get(), L"AERV2_submit_inbetween_eye%d_slot%d", eye, slot);
                    m_aerV2InBetweenSubmitReady[eye][slot] = false;
                }
            }
        }

        D3D12_RESOURCE_DESC warpDesc = sourceDesc;
        warpDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE warpClear{};
        warpClear.Format = sourceDesc.Format;
        for (int eye = 0; eye < 2; ++eye) {
            if (m_mvWarpedEye[eye]) {
                auto cur = m_mvWarpedEye[eye]->GetDesc();
                if (cur.Width == warpDesc.Width && cur.Height == warpDesc.Height && cur.Format == warpDesc.Format) {
                    continue;
                }
                m_mvWarpedEye[eye].Reset();
            }
            if (FAILED(m_d3dDevice->CreateCommittedResource(
                    &heapProps, D3D12_HEAP_FLAG_NONE,
                    &warpDesc, D3D12_RESOURCE_STATE_COMMON,
                    &warpClear, IID_PPV_ARGS(&m_mvWarpedEye[eye])))) {
                Log("OpenXRManager: Failed to create MV-warp scratch eye=%d\n", eye);
                return false;
            }
            SetD3DNamef(m_mvWarpedEye[eye].Get(), L"AERV2_mvwarped_eye%d", eye);
        }
    }

    Log("OpenXRManager: AER capture resources ready. size=%ux%u format=%u tripleBuffered=1\n", width, height, format);
    return true;
}

bool OpenXRManager::CapturePresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, int eyeIndex, uint64_t serial, uint64_t pairId) {
    if (!backBuffer || eyeIndex < 0 || eyeIndex > 1) {
        return false;
    }

    std::lock_guard<std::mutex> captureLock(m_captureMutex);
    if (!EnsureAERCaptureResources(sourceDesc)) {
        return false;
    }

    CapturedEyeFrame* frame = &m_pendingEyeFrames[eyeIndex];
    if (!frame->texture) {
        return false;
    }

    m_captureAllocatorIndex = (m_captureAllocatorIndex + 1) % 3;
    ID3D12CommandAllocator* currentAllocator = m_captureCmdAllocators[m_captureAllocatorIndex];
    
    if (m_captureFenceValue >= 3 && m_captureFence->GetCompletedValue() < m_captureFenceValue - 2) {
        m_captureFence->SetEventOnCompletion(m_captureFenceValue - 2, m_captureFenceEvent);
        WaitForSingleObject(m_captureFenceEvent, INFINITE);
    }

    ID3D12GraphicsCommandList* m_captureCmdList = m_captureCmdLists[m_captureAllocatorIndex];

    if (FAILED(currentAllocator->Reset()) || FAILED(m_captureCmdList->Reset(currentAllocator, nullptr))) {
        Log("OpenXRManager: Failed to reset AER capture command list\n");
        return false;
    }

    D3D12_RESOURCE_BARRIER barriers[6] = {};
    UINT barrierCount = 0;

    barriers[barrierCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[barrierCount].Transition.pResource = backBuffer;
    barriers[barrierCount].Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    barriers[barrierCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barriers[barrierCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++barrierCount;

    if (frame->serial != 0) {
        barriers[barrierCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barriers[barrierCount].Transition.pResource = frame->texture;
        barriers[barrierCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
        barriers[barrierCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
        barriers[barrierCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++barrierCount;
    }

    m_captureCmdList->ResourceBarrier(barrierCount, barriers);
    m_captureCmdList->CopyResource(frame->texture, backBuffer);

    D3D12_RESOURCE_BARRIER afterCopy[3] = {};
    afterCopy[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[0].Transition.pResource = frame->texture;
    afterCopy[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    afterCopy[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    UINT afterCopyCount = 1;
    afterCopy[afterCopyCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[afterCopyCount].Transition.pResource = backBuffer;
    afterCopy[afterCopyCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[afterCopyCount].Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    afterCopy[afterCopyCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++afterCopyCount;
    m_captureCmdList->ResourceBarrier(afterCopyCount, afterCopy);

    // [DEPTH] Snapshot the game's scene depth into m_depthSnapshot on the SAME AER
    // capture list, mirroring the mono path (CaptureMonoPresentedFrame). This gives
    // the AER submit a depth buffer to chain as XR_KHR_composition_layer_depth so the
    // runtime can do DEPTH-AWARE (positional) reprojection of the half-rate stale eye.
    // Observed-state barriers only (never a guessed StateBefore).
    //
    // Gated on GetAerXQueueWait() (default 0). Depth capture forces a cross-queue GPU
    // wait (CyberpunkVRPort_WaitOnAllGameSignals) that serializes the present queue
    // behind CP2077's async-compute -> depressed sim rate (NPC/animation stutter while
    // shader-time things stay smooth). With it OFF (the mono default) the AER V2 warp
    // falls back to NvOF-only flow (its depth-reprojection path is agreement-gated and
    // self-disables), which is smooth at the cost of positional reprojection quality.
    bool depthCaptured = false;
    bool frameDepthCaptured = false;
    const bool aerDepthEnabled = (GetAerXQueueWait() != 0);
    {
        ID3D12Resource* gameDepth = aerDepthEnabled ? OmoGetSceneDepthResource() : nullptr;
        const D3D12_RESOURCE_STATES gameDepthState = static_cast<D3D12_RESOURCE_STATES>(OmoGetSceneDepthState());
        if (gameDepth && OmoGetSceneDepthState() != 0 && EnsureDepthSnapshot(gameDepth)) {
            depthCaptured = RecordDepthCapture(m_captureCmdList, gameDepth, gameDepthState);
            if (depthCaptured && GetAERV2Enabled() != 0 && m_depthSnapshot) {
                const D3D12_RESOURCE_DESC depthDesc = m_depthSnapshot->GetDesc();
                bool recreateFrameDepth = true;
                if (frame->depthTexture) {
                    const D3D12_RESOURCE_DESC cur = frame->depthTexture->GetDesc();
                    if (cur.Width == depthDesc.Width && cur.Height == depthDesc.Height && cur.Format == depthDesc.Format) {
                        recreateFrameDepth = false;
                    } else {
                        frame->depthTexture->Release();
                        frame->depthTexture = nullptr;
                        frame->depthSerial = 0;
                        frame->depthInCopySource = false;
                    }
                }
                if (recreateFrameDepth) {
                    D3D12_HEAP_PROPERTIES hp{};
                    hp.Type = D3D12_HEAP_TYPE_DEFAULT;
                    D3D12_CLEAR_VALUE clearVal{};
                    clearVal.Format = depthDesc.Format;
                    clearVal.DepthStencil.Depth = 1.0f;
                    clearVal.DepthStencil.Stencil = 0;
                    if (FAILED(m_d3dDevice->CreateCommittedResource(
                            &hp,
                            D3D12_HEAP_FLAG_SHARED,
                            &depthDesc,
                            D3D12_RESOURCE_STATE_COPY_DEST,
                            &clearVal,
                            IID_PPV_ARGS(&frame->depthTexture)))) {
                        Log("OpenXRManager: Failed to create AER V2 frame depth texture eye=%d\n", eyeIndex);
                        frame->depthTexture = nullptr;
                        frame->depthSerial = 0;
                        frame->depthInCopySource = false;
                    } else {
                        frame->depthWidth = static_cast<uint32_t>(depthDesc.Width);
                        frame->depthHeight = depthDesc.Height;
                        frame->depthFormat = static_cast<uint32_t>(depthDesc.Format);
                        frame->depthSerial = 0;
                        frame->depthInCopySource = false;
                        SetD3DNamef(frame->depthTexture, L"AERV2_pending_eye%d_depth", eyeIndex);
                    }
                }
                if (frame->depthTexture) {
                    D3D12_RESOURCE_BARRIER bars[2] = {};
                    UINT bc = 0;
                    if (frame->depthInCopySource) {
                        bars[bc].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        bars[bc].Transition.pResource = frame->depthTexture;
                        bars[bc].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                        bars[bc].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                        bars[bc].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        ++bc;
                    }
                    if (bc > 0) m_captureCmdList->ResourceBarrier(bc, bars);
                    m_captureCmdList->CopyResource(frame->depthTexture, m_depthSnapshot);
                    bars[0] = {};
                    bars[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                    bars[0].Transition.pResource = frame->depthTexture;
                    bars[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                    bars[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                    bars[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                    m_captureCmdList->ResourceBarrier(1, bars);
                    frameDepthCaptured = true;
                }
            }
        }
    }

    m_captureCmdList->Close();
    // Cross-queue safety for depth read (AER capture path). Mirrors the mono
    // path's guard in CaptureMonoPresentedFrame so VDXR depth submit no longer
    // races the game's render queue → no more DEVICE_HUNG on save/load when
    // the depth resource is being recycled.
    if (depthCaptured) {
        CyberpunkVRPort_WaitOnAllGameSignals(m_d3dQueue);
    }
    ID3D12CommandList* cmdLists[] = {m_captureCmdList};
    m_d3dQueue->ExecuteCommandLists(1, cmdLists);

    ++m_captureFenceValue;
    m_d3dQueue->Signal(m_captureFence, m_captureFenceValue);

    // [AER V2 unified producer] Convert this eye's freshly captured color into its
    // NvOF input texture ONCE per capture (R8G8B8A8 -> BGRA swizzle). Ordered AFTER
    // the capture copy above via a GPU-side Wait on m_captureFence (no race), and
    // run HERE on the free-running present thread so the 90 Hz frame thread's warp/
    // submit hot path never stalls on it. FrameThreadMain later imports this
    // opticalFlowTexture into CUDA for the temporal warp. No-op unless V2 is on and
    // the flow-input texture exists.
    uint64_t flowConvertValue = 0;
    if (GetAERV2Enabled() != 0 && m_opticalFlow && frame->opticalFlowTexture && frame->texture) {
        const uint32_t cw = sourceDesc.Width != 0 ? static_cast<uint32_t>(sourceDesc.Width) : frame->width;
        const uint32_t ch = sourceDesc.Height != 0 ? sourceDesc.Height : frame->height;
        if (m_opticalFlow->EnsureInitialized(m_d3dDevice, cw, ch, sourceDesc.Format)) {
            // Fire-and-forget: ordered after the capture copy via m_captureFence,
            // signals the convert fence to flowConvertValue, returns immediately (no
            // CPU stall on this present thread). The frame thread GPU-Waits on it.
            if (!m_opticalFlow->ConvertToInputTexture(frame->texture, frame->opticalFlowTexture,
                                                      m_captureFence, m_captureFenceValue,
                                                      &flowConvertValue) &&
                (serial % 600) == 1) {
                Log("OpenXRManager: [AER V2] flow-input conversion failed eye=%d serial=%llu\n",
                    eyeIndex, static_cast<unsigned long long>(serial));
            }
        }
    }

    {
        LARGE_INTEGER captureQpc{};
        QueryPerformanceCounter(&captureQpc);
        std::lock_guard<std::mutex> lock(m_presentMutex);
        frame->serial = serial;
        frame->pairId = pairId;
        frame->captureQpc = static_cast<uint64_t>(captureQpc.QuadPart);
        frame->opticalFlowConvertValue = flowConvertValue;
        if (depthCaptured) {
            m_depthSnapshotSerial = serial;
            if (m_depthSnapshotR32) m_depthSnapshotR32Serial = serial;
        }
        if (frameDepthCaptured) {
            frame->depthSerial = serial;
            frame->depthInCopySource = true;
        } else {
            frame->depthSerial = 0;
        }
        SetD3DNamef(frame->texture, L"AERV2_pending_eye%d_color_pair%llu_serial%llu", eyeIndex,
            static_cast<unsigned long long>(pairId),
            static_cast<unsigned long long>(serial));
        SetD3DNamef(frame->opticalFlowTexture, L"AERV2_pending_eye%d_ofinput_pair%llu_serial%llu", eyeIndex,
            static_cast<unsigned long long>(pairId),
            static_cast<unsigned long long>(serial));
        if (frameDepthCaptured) {
            SetD3DNamef(frame->depthTexture, L"AERV2_pending_eye%d_depth_pair%llu_serial%llu", eyeIndex,
                static_cast<unsigned long long>(pairId),
                static_cast<unsigned long long>(serial));
        }
    }

    if (g_verboseLog && (serial % 300) == 1) {
        Log("OpenXRManager: AER frame captured. eye=%d serial=%llu pair=%llu\n",
            eyeIndex,
            static_cast<unsigned long long>(serial),
            static_cast<unsigned long long>(pairId));
    }
    return true;
}

bool OpenXRManager::EnsureMonoSubmitResources() {
    if (!m_monoSubmitEnabled.load(std::memory_order_relaxed)) {
        return false;
    }
    if (!m_d3dDevice || !m_d3dQueue || m_session == XR_NULL_HANDLE) {
        return false;
    }

    uint32_t width = 0;
    uint32_t height = 0;
    uint32_t format = 0;
    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        width = m_lastPresentedWidth;
        height = m_lastPresentedHeight;
        format = m_lastPresentedFormat;
    }

    if (width == 0 || height == 0 || format == 0) {
        return false;
    }

    uint32_t runtimeFormatCount = 0;
    XrResult xrRes = xrEnumerateSwapchainFormats(m_session, 0, &runtimeFormatCount, nullptr);
    if (XR_FAILED(xrRes) || runtimeFormatCount == 0) {
        Log("OpenXRManager: xrEnumerateSwapchainFormats count failed (res=%d count=%u)\n", xrRes, runtimeFormatCount);
        return false;
    }

    std::vector<int64_t> runtimeFormats(runtimeFormatCount);
    xrRes = xrEnumerateSwapchainFormats(m_session, runtimeFormatCount, &runtimeFormatCount, runtimeFormats.data());
    if (XR_FAILED(xrRes) || runtimeFormatCount == 0) {
        Log("OpenXRManager: xrEnumerateSwapchainFormats list failed (res=%d count=%u)\n", xrRes, runtimeFormatCount);
        return false;
    }

    const int64_t selectedFormat = PickMonoSwapchainFormat(
        runtimeFormats,
        static_cast<int64_t>(format),
        IsRuntimeVirtualDesktop());

    // Pick a runtime-supported depth format ONLY AFTER the game's scene depth resource
    // has been pinned. This remains intentionally conservative: only the R32-family
    // depth path is considered stable. The 64-bit R32G8X24 typeless family caused
    // repeated GPU removal during snapshot/submission experiments, so depth is kept
    // disabled there to preserve a working Mono baseline.
    ID3D12Resource* pinnedDepth = OmoGetSceneDepthResource();
    const DXGI_FORMAT pinnedDepthFormat = pinnedDepth ? pinnedDepth->GetDesc().Format : DXGI_FORMAT_UNKNOWN;
    int64_t selectedDepthFormat = 0;
    // CP2077 mono-only mode hangs at start-up when a depth swapchain is created
    // but never populated (VirtualDesktopXR stalls waiting on the depth layer).
    // Skip depth swapchain creation unless either (a) AER is on so the AER
    // capture path will fill the depth, or (b) the user explicitly opted in to
    // mono depth capture.
    const bool aerSubmitOn = IsAERSubmitEnabled();
    const bool depthWanted = GetDepthSubmit() != 0 && (aerSubmitOn || GetMonoDepthCapture() != 0);
    if (depthWanted && m_depthLayerSupported && pinnedDepth) {
        // Only R32-family (D32_FLOAT 32bpp) is supported for now. CP2077's
        // R32-family (32bpp) accepted directly. R32G8X24-family (64bpp) accepted
        // too: the depth-plane resolve shader (DepthResolve) converts plane 0
        // of the typeless source into the same 32bpp D32_FLOAT snapshot used
        // by the 32bpp path, so the depth swapchain is always D32_FLOAT
        // regardless of game depth format.
        const bool gameIs32bpp =
            pinnedDepthFormat == DXGI_FORMAT_R32_TYPELESS ||
            pinnedDepthFormat == DXGI_FORMAT_D32_FLOAT ||
            pinnedDepthFormat == DXGI_FORMAT_R32_FLOAT;
        const bool gameIs64bpp =
            pinnedDepthFormat == DXGI_FORMAT_R32G8X24_TYPELESS ||
            pinnedDepthFormat == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
            pinnedDepthFormat == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS ||
            pinnedDepthFormat == DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;
        if (gameIs32bpp || gameIs64bpp) {
            for (int64_t rf : runtimeFormats) {
                if (rf == static_cast<int64_t>(DXGI_FORMAT_D32_FLOAT)) {
                    selectedDepthFormat = rf;
                    break;
                }
            }
        }
        if (selectedDepthFormat == 0) {
            // Log once per format transition only — EnsureMonoSubmitResources runs
            // every frame and would otherwise flood the log with thousands of
            // duplicate lines.
            static DXGI_FORMAT s_lastLoggedRejected = DXGI_FORMAT_UNKNOWN;
            if (s_lastLoggedRejected != pinnedDepthFormat) {
                s_lastLoggedRejected = pinnedDepthFormat;
                Log("OpenXRManager: depth layer disabled (gameFmt=%u not depth-resolvable, or runtime lacks D32_FLOAT)\n",
                    static_cast<unsigned>(pinnedDepthFormat));
            }
            m_depthLayerSupported = false;
        } else {
            static DXGI_FORMAT s_lastLoggedSelected = DXGI_FORMAT_UNKNOWN;
            static int64_t s_lastLoggedDepthSel = 0;
            if (s_lastLoggedSelected != pinnedDepthFormat ||
                s_lastLoggedDepthSel != selectedDepthFormat) {
                s_lastLoggedSelected = pinnedDepthFormat;
                s_lastLoggedDepthSel = selectedDepthFormat;
                Log("OpenXRManager: depth format gameFmt=%u selected=%lld\n",
                    static_cast<unsigned>(pinnedDepthFormat),
                    selectedDepthFormat);
            }
        }
    }
    const bool wantDepthSwapchains = m_depthLayerSupported && selectedDepthFormat != 0;
    if (wantDepthSwapchains && selectedDepthFormat != m_depthSwapchainFormat) {
        Log("OpenXRManager: depth swapchain format selected=%lld (pinnedDepthFmt=%u)\n",
            selectedDepthFormat, static_cast<unsigned>(pinnedDepthFormat));
    }

    uint32_t viewCount = 0;
    xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, 0, &viewCount, nullptr);
    m_viewConfigViews.resize(viewCount, {XR_TYPE_VIEW_CONFIGURATION_VIEW});
    xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, viewCount, &viewCount, m_viewConfigViews.data());
    m_views.resize(viewCount, {XR_TYPE_VIEW});

    const bool haveDepthSwapchains = !wantDepthSwapchains ||
        (!m_eyeSwapchains.empty() &&
         m_eyeSwapchains[0].depthHandle != XR_NULL_HANDLE &&
         (m_eyeSwapchains.size() < 2 || m_eyeSwapchains[1].depthHandle != XR_NULL_HANDLE));
    const bool colorResourcesReady = !m_eyeSwapchains.empty() &&
        m_eyeSwapchains[0].width == static_cast<int32_t>(width) &&
        m_eyeSwapchains[0].height == static_cast<int32_t>(height) &&
        m_cmdAllocators[0] && m_cmdLists[0] && m_fence && m_fenceEvent;
    if (colorResourcesReady && (!wantDepthSwapchains || haveDepthSwapchains)) {
        return true;
    }

    // ADD-DEPTH FAST PATH: the color swapchains/fence/lists are already good and the
    // ONLY thing missing is the depth swapchains (the game's depth was just discovered
    // mid-session, flipping wantDepthSwapchains on). Do NOT fall through to the full
    // teardown below -- destroying the LIVE color swapchains + fence + command lists
    // while GPU frames are in flight (no GPU idle) froze the present thread here (the
    // hang reproduced exactly when depth turned on at ~serial 301, before any
    // [DEPTHDBG] logged). Instead create the depth swapchains ADDITIVELY on the existing
    // eye swapchains -- pure creation touches no in-flight resource, so it can't hang.
    if (colorResourcesReady && wantDepthSwapchains && !haveDepthSwapchains) {
        bool addedOk = true;
        for (size_t eye = 0; eye < m_eyeSwapchains.size(); ++eye) {
            if (m_eyeSwapchains[eye].depthHandle != XR_NULL_HANDLE) continue;
            XrSwapchainCreateInfo depthInfo{XR_TYPE_SWAPCHAIN_CREATE_INFO};
            depthInfo.usageFlags = XR_SWAPCHAIN_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT | XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT;
            depthInfo.format = selectedDepthFormat;
            depthInfo.sampleCount = 1;
            depthInfo.width = m_eyeSwapchains[eye].width;
            depthInfo.height = m_eyeSwapchains[eye].height;
            depthInfo.faceCount = 1;
            depthInfo.arraySize = 1;
            depthInfo.mipCount = 1;
            const XrResult dres = xrCreateSwapchain(m_session, &depthInfo, &m_eyeSwapchains[eye].depthHandle);
            if (XR_FAILED(dres)) {
                Log("OpenXRManager: [DEPTH] add-depth: xrCreateSwapchain failed eye %zu (res=%d) -> depth disabled\n", eye, dres);
                m_eyeSwapchains[eye].depthHandle = XR_NULL_HANDLE;
                m_depthLayerSupported = false;
                addedOk = false;
                break;
            }
            uint32_t dImageCount = 0;
            xrEnumerateSwapchainImages(m_eyeSwapchains[eye].depthHandle, 0, &dImageCount, nullptr);
            m_eyeSwapchains[eye].depthImages.resize(dImageCount, {XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
            xrEnumerateSwapchainImages(m_eyeSwapchains[eye].depthHandle, dImageCount, &dImageCount,
                reinterpret_cast<XrSwapchainImageBaseHeader*>(m_eyeSwapchains[eye].depthImages.data()));
        }
        m_depthSwapchainFormat = addedOk ? selectedDepthFormat : m_depthSwapchainFormat;
        Log("OpenXRManager: [DEPTH] depth swapchains added in-place (no color rebuild) ok=%d fmt=%lld\n",
            addedOk ? 1 : 0, selectedDepthFormat);
        return true;
    }

    for (auto& eye : m_eyeSwapchains) {
        if (eye.handle != XR_NULL_HANDLE) {
            xrDestroySwapchain(eye.handle);
            eye.handle = XR_NULL_HANDLE;
        }
        if (eye.depthHandle != XR_NULL_HANDLE) {
            xrDestroySwapchain(eye.depthHandle);
            eye.depthHandle = XR_NULL_HANDLE;
        }
    }
    m_eyeSwapchains.clear();

    // Drop the cached last-good textures: a swapchain (re)create may change size/
    // format, which would mismatch CopyResource. They re-create lazily next frame.
    m_lastGoodValid = false;
    for (int e = 0; e < 2; ++e) { m_lastGoodEye[e].Reset(); m_lastGoodEyeInited[e] = false; }

    if (m_fenceEvent) {
        CloseHandle(m_fenceEvent);
        m_fenceEvent = nullptr;
    }
    if (m_fence) {
        m_fence->Release();
        m_fence = nullptr;
    }
    for (int i = 0; i < 3; ++i) {
        if (m_cmdLists[i]) {
            m_cmdLists[i]->Release();
            m_cmdLists[i] = nullptr;
        }
        if (m_cmdAllocators[i]) {
            m_cmdAllocators[i]->Release();
            m_cmdAllocators[i] = nullptr;
        }
    }

    m_eyeSwapchains.resize(viewCount);

    for (uint32_t eye = 0; eye < viewCount; ++eye) {
        XrSwapchainCreateInfo swapchainInfo{XR_TYPE_SWAPCHAIN_CREATE_INFO};
        swapchainInfo.usageFlags = XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT | XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT;
        swapchainInfo.format = selectedFormat;
        swapchainInfo.sampleCount = 1;
        swapchainInfo.width = static_cast<int32_t>(width);
        swapchainInfo.height = static_cast<int32_t>(height);
        swapchainInfo.faceCount = 1;
        swapchainInfo.arraySize = 1;
        swapchainInfo.mipCount = 1;

        const XrResult res = xrCreateSwapchain(m_session, &swapchainInfo, &m_eyeSwapchains[eye].handle);
        if (XR_FAILED(res)) {
            Log("OpenXRManager: Failed to create mono swapchain for eye %u (res=%d)\n", eye, res);
            return false;
        }

        m_eyeSwapchains[eye].width = swapchainInfo.width;
        m_eyeSwapchains[eye].height = swapchainInfo.height;

        uint32_t imageCount = 0;
        xrEnumerateSwapchainImages(m_eyeSwapchains[eye].handle, 0, &imageCount, nullptr);
        m_eyeSwapchains[eye].images.resize(imageCount, {XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
        xrEnumerateSwapchainImages(
            m_eyeSwapchains[eye].handle,
            imageCount,
            &imageCount,
            reinterpret_cast<XrSwapchainImageBaseHeader*>(m_eyeSwapchains[eye].images.data()));

        if (wantDepthSwapchains) {
            XrSwapchainCreateInfo depthInfo{XR_TYPE_SWAPCHAIN_CREATE_INFO};
            depthInfo.usageFlags = XR_SWAPCHAIN_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT | XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT;
            depthInfo.format = selectedDepthFormat;
            depthInfo.sampleCount = 1;
            depthInfo.width = static_cast<int32_t>(width);
            depthInfo.height = static_cast<int32_t>(height);
            depthInfo.faceCount = 1;
            depthInfo.arraySize = 1;
            depthInfo.mipCount = 1;
            const XrResult dres = xrCreateSwapchain(m_session, &depthInfo, &m_eyeSwapchains[eye].depthHandle);
            if (XR_FAILED(dres)) {
                Log("OpenXRManager: Failed to create depth swapchain for eye %u (res=%d) — disabling depth layer\n", eye, dres);
                m_eyeSwapchains[eye].depthHandle = XR_NULL_HANDLE;
                m_depthLayerSupported = false;
            } else {
                uint32_t dImageCount = 0;
                xrEnumerateSwapchainImages(m_eyeSwapchains[eye].depthHandle, 0, &dImageCount, nullptr);
                m_eyeSwapchains[eye].depthImages.resize(dImageCount, {XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
                xrEnumerateSwapchainImages(
                    m_eyeSwapchains[eye].depthHandle,
                    dImageCount,
                    &dImageCount,
                    reinterpret_cast<XrSwapchainImageBaseHeader*>(m_eyeSwapchains[eye].depthImages.data()));
            }
        }
    }
    m_depthSwapchainFormat = selectedDepthFormat;

    char formatSummary[512] = {};
    int summaryPos = sprintf_s(formatSummary, "OpenXRManager: Mono swapchain formats. game=%u selected=%lld runtime:", format, selectedFormat);
    if (summaryPos > 0) {
        for (uint32_t i = 0; i < runtimeFormatCount && summaryPos > 0 && summaryPos < static_cast<int>(sizeof(formatSummary) - 32); ++i) {
            summaryPos += sprintf_s(formatSummary + summaryPos, sizeof(formatSummary) - summaryPos, " %lld", runtimeFormats[i]);
        }
        Log("%s\n", formatSummary);
    }

    for (int i = 0; i < 3; ++i) {
        if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_cmdAllocators[i])))) {
            Log("OpenXRManager: Failed to create submit command allocator %d\n", i);
            return false;
        }
        SetD3DName(m_cmdAllocators[i], L"OpenXR_submit_allocator");
    }
    for (int i = 0; i < 3; ++i) {
        if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_cmdAllocators[i], nullptr, IID_PPV_ARGS(&m_cmdLists[i])))) {
            Log("OpenXRManager: Failed to create submit command list %d\n", i);
            return false;
        }
        SetD3DName(m_cmdLists[i], L"OpenXR_submit_command_list");
        m_cmdLists[i]->Close();
    }

    if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_fence)))) {
        Log("OpenXRManager: Failed to create mono fence\n");
        return false;
    }
    SetD3DName(m_fence, L"OpenXR_submit_fence");
    m_fenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
    if (!m_fenceEvent) {
        Log("OpenXRManager: Failed to create mono fence event\n");
        return false;
    }

    Log("OpenXRManager: Mono submit resources ready. game=%ux%u eye0=%dx%d rec0=%ux%u format=%u\n",
        width,
        height,
        viewCount != 0 ? m_eyeSwapchains[0].width : 0,
        viewCount != 0 ? m_eyeSwapchains[0].height : 0,
        viewCount != 0 ? m_viewConfigViews[0].recommendedImageRectWidth : 0,
        viewCount != 0 ? m_viewConfigViews[0].recommendedImageRectHeight : 0,
        format);
    return true;
}
