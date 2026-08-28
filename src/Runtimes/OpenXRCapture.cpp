// openxr_capture.cpp - mono frame + depth capture and submit-resource setup.
// Split verbatim from openxr_manager.cpp (OpenXRManager methods). Shared module
// state/helpers via openxr_internal.h (inline).
#include <atomic>
#include "Runtimes/OpenXRManager.hpp"
#include "Overlay/ImGuiOverlay.hpp"   // OverlayRecordIntoTarget, the second-eye pass
#include "Runtimes/OpenXRInternal.hpp"
#include "Utils/XrMath.hpp"
#include "Utils/SharedSlots.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
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

// Right eye source, from the stereo module. "Fresh" returns null once the VRCAM view stops
// updating, which is what makes the fallback to mono automatic.
extern "C" ID3D12Resource* CyberpunkVR_GetVrcamEyeTextureFresh();
// The finished HUD surface, snapshotted by sync_stereo, plus the engine's own composite
// constants read out of its b6 buffer.
extern "C" ID3D12Resource* CyberpunkVR_GetHudTexture();
// The scanner's object outline. The second eye renders its OWN -- the writer is
// CRenderNode_RenderVisionElements, which dispatches for both views (it never showed up in the
// draw census because it does not draw). What VRCAM never gets is the chain that composites the
// layer, so sync_stereo snapshots it and it is blended in here. Copying MAIN's would be wrong on
// principle: an outline traces on-screen silhouettes, so MAIN's would sit at MAIN's parallax and
// read as double vision.
extern "C" ID3D12Resource* CyberpunkVR_GetVisionTexture();
extern "C" int CyberpunkVR_VisionToSecondEye;
extern "C" int CyberpunkVR_VisionDebug;         // 0 blend, 1 opaque replace, 2 additive
extern "C" int CyberpunkVR_VisionFit;           // 1 = pixel-exact rather than stretched
extern "C" float CyberpunkVR_VisionOffX;
extern "C" float CyberpunkVR_VisionOffY;
extern "C" unsigned long long CyberpunkVR_DebugVisionOverlays;
// The barrel dot, published by the ImGui overlay (imgui_overlay.cpp) in NDC after its own zoom
// compensation. Taking the finished number rather than re-projecting keeps the two eyes' dots
// identical by construction.
extern "C" float    CyberpunkVR_BarrelDotNdcX;
extern "C" float    CyberpunkVR_BarrelDotNdcX2;   // the second eye's own value
extern "C" float    CyberpunkVR_BarrelDotNdcY;
extern "C" float    CyberpunkVR_BarrelDotRadiusPx;
extern "C" unsigned long long CyberpunkVR_BarrelDotTick;
extern "C" int      CyberpunkVR_BarrelDotSecondEye;
extern "C" int      CyberpunkVR_BarrelDotSecondVisible;
extern "C" unsigned long long CyberpunkVR_DebugBarrelDotDraws;
extern "C" ID3D12Resource* CyberpunkVR_GetHudBlurTexture();
extern "C" ID3D12Resource* CyberpunkVR_GetHudExposureBuffer();
extern "C" ID3D12Resource* CyberpunkVR_GetFrameConstantBuffer();
extern "C" ID3D12Resource* CyberpunkVR_GetHudConstantBuffer();
extern "C" void CyberpunkVR_NoteHudCompositeInputs(const void* hud, const void* blur,
                                                   const void* expo, const void* frameCb,
                                                   const void* hudCb);
extern "C" ID3D12Resource* CyberpunkVR_GetMainOutTexture();
extern "C" ID3D12Resource* CyberpunkVR_GetMainSceneTexture();
// Not extern "C" (it returns a C++ struct), so it keeps sync_stereo's namespace.
namespace cvr { ColorBlit::HudParams CyberpunkVR_GetHudParams(); }
extern "C" int CyberpunkVR_StereoSubmit;
// How far away the composited HUD should sit, in metres. 0 = leave it at infinity, which is what
// pasting it at the same pixel in both eyes amounts to. Live-tunable rather than a vrport.ini
// setting because the comfortable value is personal and worth sweeping in the headset.
//
// DEFAULT IS 0: NO SHIFT. It was 1.8 m, and at that distance the second eye's HUD is moved by
// IPD/1.8 radians -- which is correct as optics and wrong as a picture on this headset: markers and
// labels sit at visibly different places in the two eyes, which is what the disparity IS. The
// mechanism stays, gated on this value, because the finding that produced it is real and belongs to a
// different headset:
//
//   at zero disparity a Pimax tester reported "both images too far to the sides to converge" --
//   optical infinity means every icon splits by the full vergence angle when the eyes are focused an
//   arm's length away.
//
// So this is a per-headset preference with two defensible settings, not a bug with a fix. Set it to
// 1.8 (or sweep it) to get the finite-distance HUD back; leave it at 0 for identical placement in both
// eyes. The shift code below reads it every frame, so it can be moved live from the debugger.
extern "C" __declspec(dllexport) float CyberpunkVR_HudDistanceM = 0.0f;

// Our private right-eye target: the eye swapchain's exact format and size, so the submit can
// copy it with the same plain CopyResource it uses for MAIN. Kept in COPY_SOURCE between
// frames -- that is where the blit's own barriers leave it and where the submit wants it.
bool OpenXRManager::EnsureVrcamEyeTexture(uint32_t width, uint32_t height, DXGI_FORMAT format) {
    if (!m_d3dDevice || !width || !height || format == DXGI_FORMAT_UNKNOWN) return false;
    if (m_vrcamEyePool[0] && m_vrcamEyeW == width && m_vrcamEyeH == height &&
        m_vrcamEyeFmt == static_cast<uint32_t>(format)) {
        return true;
    }
    for (int i = 0; i < kVrcamEyeSlots; ++i) {
        if (m_vrcamEyePool[i]) { m_vrcamEyePool[i]->Release(); m_vrcamEyePool[i] = nullptr; }
        m_vrcamEyePoolSerial[i] = 0;
    }
    m_vrcamEyeSlot = 0;
    m_vrcamEyeSerial = 0;

    D3D12_RESOURCE_DESC d{};
    d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    d.Width = width;
    d.Height = height;
    d.DepthOrArraySize = 1;
    d.MipLevels = 1;
    d.Format = format;
    d.SampleDesc.Count = 1;
    d.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
    d.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;   // ColorBlit draws into it
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_DEFAULT;
    for (int i = 0; i < kVrcamEyeSlots; ++i) {
        if (FAILED(m_d3dDevice->CreateCommittedResource(
                &hp, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_SOURCE, nullptr,
                IID_PPV_ARGS(&m_vrcamEyePool[i])))) {
            for (int k = 0; k <= i; ++k) {
                if (m_vrcamEyePool[k]) { m_vrcamEyePool[k]->Release(); m_vrcamEyePool[k] = nullptr; }
            }
            Log("OpenXRManager: Failed to create stereo capture texture eye=1 slot=%d %ux%u fmt=%u\n",
                i, width, height, static_cast<unsigned>(format));
            return false;
        }
        SetD3DNamef(m_vrcamEyePool[i], L"OpenXR_vrcam_eye_slot%d", i);
    }
    m_vrcamEyeW = width;
    m_vrcamEyeH = height;
    m_vrcamEyeFmt = static_cast<uint32_t>(format);
    Log("OpenXRManager: stereo capture pool ready, %d slots. eye=1 %ux%u fmt=%u\n",
        kVrcamEyeSlots, width, height, static_cast<unsigned>(format));
    return true;
}

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
        if (m_monoPool[0] &&
            m_monoCapturedFrame.width == width &&
            m_monoCapturedFrame.height == height &&
            m_monoCapturedFrame.format == format) {
            return true;
        }

        // The pool owns the references now; m_monoCapturedFrame.texture just points at
        // whichever pool entry currently holds the newest frame.
        for (int i = 0; i < 3; ++i) {
            if (m_monoPool[i]) { m_monoPool[i]->Release(); m_monoPool[i] = nullptr; }
        }
        m_monoCapturedFrame.texture = nullptr;
        m_monoCapturedFrame.width = 0;
        m_monoCapturedFrame.height = 0;
        m_monoCapturedFrame.format = 0;
        m_monoCapturedFrame.serial = 0;
        m_monoCapturedFrame.hasView[0] = false;
        m_monoCapturedFrame.hasView[1] = false;
    }

    D3D12_HEAP_PROPERTIES heapProps{};
    heapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
    ID3D12Resource* created[3] = {};
    for (int i = 0; i < 3; ++i) {
        if (FAILED(m_d3dDevice->CreateCommittedResource(
                &heapProps,
                D3D12_HEAP_FLAG_NONE,
                &sourceDesc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                nullptr,
                IID_PPV_ARGS(&created[i])))) {
            Log("OpenXRManager: Failed to create mono captured texture %d\n", i);
            for (int k = 0; k < i; ++k) created[k]->Release();
            return false;
        }
        SetD3DNamef(created[i], L"OpenXR_mono_snapshot_color%d", i);
    }

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        for (int i = 0; i < 3; ++i) m_monoPool[i] = created[i];
        m_monoPoolSlot = 0;
        m_monoCapturedFrame.texture = m_monoPool[0];
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

// [DEPTH] Accessors implemented in swapchain_hooks.cpp — the game's pinned
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
    if (m_depthSnapshot) {
        const D3D12_RESOURCE_DESC cur = m_depthSnapshot->GetDesc();
        if (cur.Width == desc.Width && cur.Height == desc.Height && cur.Format == snapshotFormat) {
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

    return true;
}

void OpenXRManager::CaptureSceneDepthInline(ID3D12GraphicsCommandList* list,
                                            ID3D12Resource* gameDepth,
                                            unsigned int stateAfter) {
    if (!list || !gameDepth || !m_d3dDevice) return;
    // The engine can make the depth readable more than once per frame; one copy is enough.
    const uint64_t frame = m_presentCount.load(std::memory_order_relaxed);
    if (m_depthStageFrame.load(std::memory_order_acquire) == frame) return;
    if (m_depthStageFrame.exchange(frame, std::memory_order_acq_rel) == frame) return;

    std::lock_guard<std::mutex> lock(m_depthStageMutex);

    D3D12_RESOURCE_DESC srcDesc = gameDepth->GetDesc();
    if (srcDesc.Width == 0 || srcDesc.Height == 0) return;

    // Copy the DEPTH PLANE ONLY, not the whole planar surface.
    //
    // CP2077's scene depth is R32G8X24_TYPELESS: 64 bits per pixel, of which 32 are stencil
    // padding we have no use for. Copying it whole cost ~48 MB per frame at 2444x2444, about
    // 3 GB/s of extra bandwidth on the engine's own command list -- enough to show up as
    // occasional hitches. Plane 0 of that format is the R32 depth, so a 32bpp stage halves
    // the traffic, and it also removes the shader resolve later: the stage is then already
    // in the layout the depth swapchain wants.
    const DXGI_FORMAT srcFmt = srcDesc.Format;
    const bool planar64 =
        srcFmt == DXGI_FORMAT_R32G8X24_TYPELESS ||
        srcFmt == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
        srcFmt == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS ||
        srcFmt == DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;
    const DXGI_FORMAT stageFmt = planar64 ? DXGI_FORMAT_R32_TYPELESS : srcFmt;

    if (!m_depthStage || m_depthStageW != srcDesc.Width || m_depthStageH != srcDesc.Height ||
        m_depthStageFmt != static_cast<uint32_t>(stageFmt)) {
        if (m_depthStage) { m_depthStage->Release(); m_depthStage = nullptr; }
        D3D12_RESOURCE_DESC d = srcDesc;
        d.Format = stageFmt;
        d.Flags = D3D12_RESOURCE_FLAG_NONE;   // plain texture: copy target + copy source
        d.MipLevels = 1;
        D3D12_HEAP_PROPERTIES hp{};
        hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        if (FAILED(m_d3dDevice->CreateCommittedResource(
                &hp, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_DEST, nullptr,
                IID_PPV_ARGS(&m_depthStage)))) {
            m_depthStage = nullptr;
            return;
        }
        m_depthStageW = static_cast<uint32_t>(srcDesc.Width);
        m_depthStageH = srcDesc.Height;
        m_depthStageFmt = static_cast<uint32_t>(stageFmt);
        m_depthStageSerial = 0;
        SetD3DNamef(m_depthStage, L"OpenXR_depth_stage");
    }
    if (!m_depthStage) return;

    const auto before = static_cast<D3D12_RESOURCE_STATES>(stateAfter);
    D3D12_RESOURCE_BARRIER pre[2] = {};
    UINT n = 0;
    // The source is shader-readable right now; a copy needs COPY_SOURCE. Both transitions
    // are restored below, so the engine finds the buffer exactly as it left it.
    pre[n].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    pre[n].Transition.pResource = gameDepth;
    pre[n].Transition.StateBefore = before;
    pre[n].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    pre[n].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++n;
    if (m_depthStageSerial != 0) {          // first use it is already in COPY_DEST
        pre[n].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        pre[n].Transition.pResource = m_depthStage;
        pre[n].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
        pre[n].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
        pre[n].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++n;
    }
    list->ResourceBarrier(n, pre);

    if (planar64) {
        // Subresource 0 of a planar depth-stencil is the depth plane; the stencil plane is
        // left where it is. Half the bytes of a full-surface copy.
        D3D12_TEXTURE_COPY_LOCATION dst{};
        dst.pResource = m_depthStage;
        dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        dst.SubresourceIndex = 0;
        D3D12_TEXTURE_COPY_LOCATION src{};
        src.pResource = gameDepth;
        src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        src.SubresourceIndex = 0;
        list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
    } else {
        list->CopyResource(m_depthStage, gameDepth);
    }

    D3D12_RESOURCE_BARRIER post[2] = {};
    post[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    post[0].Transition.pResource = gameDepth;
    post[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    post[0].Transition.StateAfter = before;                  // exactly as the engine had it
    post[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    post[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    post[1].Transition.pResource = m_depthStage;
    post[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    // COPY_SOURCE, because the stage is now plain 32bpp depth: the later step is a straight
    // copy into the depth swapchain, not a shader resolve.
    post[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    post[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    list->ResourceBarrier(2, post);

    // The frame being built is the one the NEXT present shows, so tag it accordingly.
    m_depthStageSerial = frame + 1;

    // Bind THIS frame's head pose here too, for the same reason and with the same stamp.
    //
    // The camera injection for this frame has already run (it happens before the geometry
    // pass, and this barrier is after it), so the newest entry in the injection ring is
    // unambiguously the pose the engine is drawing with right now. No counting backwards, no
    // lag to tune: the image, its depth and its pose all get one identity, which is what
    // keeps them together through the engine's render-ahead the way RealVR's per-slot pose
    // does.
    {
        // Frame-pose publication moved to PushRenderHeadPose: tying it to the depth
        // barrier meant ~25% of frames had no exact pose and fell back to an estimate.
    }
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

// 1 = while a menu is open, both eyes get the backbuffer (the menu) instead of pairing it with
// VRCAM_s stale view of the world. 0 = the previous behaviour, for A/B.
extern "C" __declspec(dllexport) int CyberpunkVR_MonoMenu = 1;


// ---- WHY A PRESENT DID NOT PRODUCE A CAPTURE -------------------------------------------------
//
// Measured because the frame accounting proved this path is where the jumps come from, and could not
// say which of the five early-outs below causes them.
//
// The evidence: in GAMEPLAY windows (presents 40-70/s, so no menus or loading), a submit carrying an
// image 30-53 ms old happens once every ~3.9 seconds, and in those windows the PRESENT RATE IS
// UNCHANGED -- 50.3/s against 51.4/s in clean windows. The game kept delivering frames; the capture
// did not publish one for two or three display periods. So the cause is here, not in the game.
//
// The fence wait is the prime suspect and is timed rather than just counted: it runs ON THE GAME'S
// PRESENT THREAD, so every millisecond it blocks is a millisecond the game is not presenting -- which
// would explain both the stale submit AND the 1.1/s present-rate difference at once.
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapSkipNoView   = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapSkipNoRes    = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapSkipNoSlot   = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapSkipFence    = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapSkipReset    = 0;
// Defined in the stereo module (src/Stereo/Capture.cpp): the second-eye content age it already
// computes for its own staleness gate. Sampled below rather than recomputed.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DebugVrcamEyeAgeMs;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DebugVrcamEyeAgeUs;

extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapOk           = 0;
// The second eye's CONTENT age -- see the sampling site for why this is not the same thing as [xreye].
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugEyeAgeCount     = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugEyeAgeSumMs     = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugEyeAgeMaxMs     = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugEyeAgeNever     = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugEyeAgeBuckets[4] = {};
// The bounded fence wait: how often it is entered at all, and how long it actually blocked.
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapFenceWaits   = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapFenceUsSum   = 0;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugCapFenceUsMax   = 0;

bool OpenXRManager::CaptureMonoPresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, uint64_t serial,
    const XrPosef poses[2], const XrFovf fovs[2], const bool hasView[2]) {
    if (!backBuffer || !hasView[0] || !hasView[1]) {
        CyberpunkVR_DebugCapSkipNoView.fetch_add(1, std::memory_order_relaxed);
        return false;
    }

    std::lock_guard<std::mutex> captureLock(m_captureMutex);
    if (!EnsureMonoCaptureResource(sourceDesc)) {
        CyberpunkVR_DebugCapSkipNoRes.fetch_add(1, std::memory_order_relaxed);
        return false;
    }

    // Write into the NEXT buffer of the pool, never into the one just published.
    //
    // Skipping the capture while the consumer was busy (what stood here before) kept Present
    // free, but starved the headset: the submit thread holds the snapshot for a good part of
    // every display frame, so at 55-60 fps the game kept landing in a busy moment and the
    // HMD stopped receiving new images -- smooth on the monitor, freezing in the headset.
    // Rotating buffers removes the conflict itself: the producer always has a free one, so
    // it neither waits nor skips.
    ID3D12Resource* snapshot = nullptr;
    uint64_t previousSerial = 0;
    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        m_monoPoolSlot = (m_monoPoolSlot + 1) % 3;
        snapshot = m_monoPool[m_monoPoolSlot];
        previousSerial = m_monoCapturedFrame.serial;
        if (snapshot) {
            snapshot->AddRef();
        }
    }
    if (!snapshot) {
        CyberpunkVR_DebugCapSkipNoSlot.fetch_add(1, std::memory_order_relaxed);
        return false;
    }

    m_captureAllocatorIndex = (m_captureAllocatorIndex + 1) % 3;
    ID3D12CommandAllocator* currentAllocator = m_captureCmdAllocators[m_captureAllocatorIndex];
    
    if (m_captureFenceValue >= 3 && m_captureFence->GetCompletedValue() < m_captureFenceValue - 2) {
        m_captureFence->SetEventOnCompletion(m_captureFenceValue - 2, m_captureFenceEvent);
        // BOUNDED. This runs on the game's Present thread, so an INFINITE wait turns any
        // GPU hiccup into a hung game. Skipping one capture costs nothing now: the submit
        // side re-sends the frame it already holds, with the pose that belongs to it.
        // TIMED, not just counted. This blocks the game's Present thread, so the duration IS the
        // cost -- and a wait that succeeds after 30 ms does more damage than one that times out,
        // because it delays the publish without being recorded as a skip.
        const double fenceEnterMs = XrDiagNowMs();
        const DWORD fenceRes = WaitForSingleObject(m_captureFenceEvent, 100);
        const double fenceMs = XrDiagNowMs() - fenceEnterMs;
        CyberpunkVR_DebugCapFenceWaits.fetch_add(1, std::memory_order_relaxed);
        const unsigned long long fenceUs = (unsigned long long)(fenceMs * 1000.0);
        CyberpunkVR_DebugCapFenceUsSum.fetch_add(fenceUs, std::memory_order_relaxed);
        unsigned long long prevMax =
            CyberpunkVR_DebugCapFenceUsMax.load(std::memory_order_relaxed);
        while (fenceUs > prevMax &&
               !CyberpunkVR_DebugCapFenceUsMax.compare_exchange_weak(
                   prevMax, fenceUs, std::memory_order_relaxed)) {
        }
        if (fenceRes != WAIT_OBJECT_0) {
            CyberpunkVR_DebugCapSkipFence.fetch_add(1, std::memory_order_relaxed);
            snapshot->Release();
            return false;
        }
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

    // ---- right eye: the VRCAM view, converted here and now ---------------------------------
    //
    // Same list, same fence, same moment as MAIN's copy above -- which is the whole point.
    // sync_stereo's snapshot belongs to the engine and the desktop mirror also reads it from
    // another queue; the earlier attempt to sample it from the submit thread at display rate
    // put two queues on one resource and hung the GPU on the first VRCAM frame. Here the
    // engine's frame is already submitted and we are on the Present thread, so this is the
    // one point where reading it is ours to do.
    //
    // It cannot be copied the way MAIN is: MAIN's backbuffer is sRGB-encoded UNORM bytes that
    // match the eye swapchain bit for bit, while this is RenderFinal2D's output -- 2444x2444
    // R11G11B10_FLOAT holding LINEAR values (confirmed live: "rtvpick ... fmt=26"). So it goes
    // through ColorBlit into a target typed as the eye swapchain's own format: the sampler
    // resizes, and writing through an _UNORM_SRGB RTV makes the hardware do the linear->sRGB
    // encode. What lands in the eye slot is then byte-identical in kind to MAIN's snapshot,
    // and the submit copies it with the same plain CopyResource.
    // Sizes and formats exactly as the version that worked (recovered from this project's own
    // EnsureStereoCaptureResources / CaptureStereoPresentedFrame):
    //
    //   size    = MAIN's, not VRCAM's and not the XR image's. Eye 1 is BLITTED, not copied,
    //             so it is sized to MAIN and the blit rescales VRCAM if the two disagree.
    //             They normally match -- the launcher pick drives both the MAIN resolution
    //             override and the VRCAM component -- and a mismatch (MAIN 2560 against a
    //             vrcam_2444x2444 pick) just costs a rescale.
    //   resource= R8G8B8A8_TYPELESS. D3D12 only allows the UNORM -> UNORM_SRGB view cast
    //             through a typeless resource, so this is what makes the encode legal.
    //   view    = R8G8B8A8_UNORM_SRGB. The sRGB RTV makes the hardware encode the linear
    //             VRCAM colour on write -- exact and free, unlike a pow() in the shader.
    //
    // Deriving these from the XR image instead is what killed the GPU twice: that resource is
    // typeless, and a typeless RTV is invalid.
    bool vrcamEyeCaptured = false;
    const uint32_t eyeW = static_cast<uint32_t>(sourceDesc.Width);
    const uint32_t eyeH = sourceDesc.Height;
    // NOT IN A MENU. The right eye below is VRCAM's view of the WORLD; the menu is not in it,
    // because a menu is drawn as one mono surface into the backbuffer. Taking VRCAM here
    // anyway gives the left eye the menu and the right eye a frozen street, which is the
    // reported "the headset shows VRCAM and not the menu": with the two eyes disagreeing
    // that completely, the scene is what reads.
    // Skipping it leaves both eyes on the backbuffer snapshot, i.e. the menu, mono -- which is
    // what a mono surface should look like.
    const bool menuOpen = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
    if (CyberpunkVR_StereoSubmit && eyeW && eyeH && !(menuOpen && CyberpunkVR_MonoMenu)) {
        ID3D12Resource* vrcamSrc = CyberpunkVR_GetVrcamEyeTextureFresh();
        // THE AGE OF THE SECOND EYE'S CONTENT, which is a different quantity from everything else
        // measured so far and the only one that can be asymmetric between the eyes.
        //
        // [xreye] counts whether the eye got its own IMAGE, by serial. But the serial is stamped when
        // the BLIT runs, and the blit copies whatever the second view last produced -- so a frame the
        // engine did not re-render for that view is copied again with a FRESH serial. Paired by stamp,
        // stale by content. That is the one shape which shows in one eye only, and it is what
        // GetVrcamEyeTextureFresh already computes for its own staleness gate.
        //
        // Behind CyberpunkVR_XrDeepDiag, like the rest of that instrument. The staleness GATE itself is
        // not gated -- it decides whether this eye is submitted at all -- only the accumulation for
        // [xrage] is.
        if (CyberpunkVR_XrDeepDiag) {
            // MICROSECONDS. The millisecond value comes off a 15.6 ms clock and reported max 16.0 ms
            // in every window -- one tick, not a measurement, and useless at the 14 ms scale that
            // matters. See StableNowMs in src/Stereo/Capture.cpp.
            const unsigned long long ageUs = CyberpunkVR_DebugVrcamEyeAgeUs;
            const unsigned long long ageMs = CyberpunkVR_DebugVrcamEyeAgeMs;
            if (ageMs != 0xFFFFFFFFull) {
                CyberpunkVR_DebugEyeAgeCount.fetch_add(1, std::memory_order_relaxed);
                CyberpunkVR_DebugEyeAgeSumMs.fetch_add(ageUs, std::memory_order_relaxed);
                unsigned long long prev =
                    CyberpunkVR_DebugEyeAgeMaxMs.load(std::memory_order_relaxed);
                while (ageUs > prev &&
                       !CyberpunkVR_DebugEyeAgeMaxMs.compare_exchange_weak(
                           prev, ageUs, std::memory_order_relaxed)) {
                }
                // Buckets in whole game frames (20 ms), so "one frame behind the other eye" reads as 1.
                int b = (int)(ageUs / 20000ull);
                if (b < 0) b = 0;
                if (b > 3) b = 3;
                CyberpunkVR_DebugEyeAgeBuckets[b].fetch_add(1, std::memory_order_relaxed);
            } else {
                CyberpunkVR_DebugEyeAgeNever.fetch_add(1, std::memory_order_relaxed);
            }
        }
        if (vrcamSrc) {
            if (EnsureVrcamEyeTexture(eyeW, eyeH, DXGI_FORMAT_R8G8B8A8_TYPELESS)) {
                if (!m_colorBlit) m_colorBlit = std::make_unique<ColorBlit>();
                if (m_colorBlit->EnsureInitialized(m_d3dDevice,
                                                   DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
                                                   eyeW, eyeH)) {
                    // No barrier on vrcamSrc: a transition barrier asserts the resource's
                    // current state, and this one is not ours to assert about -- that lie is
                    // exactly what hung the device. A resource resting in COMMON is promoted
                    // to the read state implicitly for the access that needs it and decays
                    // back afterwards, asserting nothing. sync_stereo additionally creates it
                    // with ALLOW_SIMULTANEOUS_ACCESS so the overlap is defined.
                    // ROTATE FIRST, then write. The slot chosen here is the one the submit will
                    // look up by serial, so the producer never touches an image a consumer is
                    // still copying out of -- the whole point of the pool.
                    {
                        std::lock_guard<std::mutex> lock(m_presentMutex);
                        m_vrcamEyeSlot = (m_vrcamEyeSlot + 1) % kVrcamEyeSlots;
                    }
                    ID3D12Resource* const eyeSlotTex = m_vrcamEyePool[m_vrcamEyeSlot];

                    D3D12_RESOURCE_BARRIER toRt{};
                    toRt.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                    toRt.Transition.pResource = eyeSlotTex;   // ours
                    toRt.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                    toRt.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
                    toRt.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                    m_captureCmdList->ResourceBarrier(1, &toRt);

                    // ---- the HUD, for the eye the engine refuses to draw it for ----------
                    //
                    // When the HUD surface is available the composite REPLACES the plain blit:
                    // it is the engine's own HUD pass (PipelineState_576) ported shader-for-
                    // shader, and it reads the scene, so it produces the finished eye in one go
                    // -- curvature, the x2 gain, the mip glow and the chromatic aberration
                    // included. Without a HUD (loading, HUD off) it falls back to the blit.
                    ID3D12Resource* hud     = CyberpunkVR_GetHudTexture();
                    ID3D12Resource* hudBlur = hud ? CyberpunkVR_GetHudBlurTexture() : nullptr;
                    ID3D12Resource* hudExpo = hud ? CyberpunkVR_GetHudExposureBuffer() : nullptr;
                    ID3D12Resource* frameCb = hud ? CyberpunkVR_GetFrameConstantBuffer() : nullptr;
                    ID3D12Resource* hudCb   = hud ? CyberpunkVR_GetHudConstantBuffer() : nullptr;
                    CyberpunkVR_NoteHudCompositeInputs(hud, hudBlur, hudExpo, frameCb, hudCb);
                    // THE COMPOSITE CONSTANTS ARE OPTIONAL, and requiring them here is why the HUD
                    // took minutes to appear in the headset. They are found by fingerprinting the
                    // engine's upload traffic, which can take a long time and in a bad session
                    // never succeeds at all; meanwhile the shader already has a fallback -- it
                    // validates whatever is bound at b6 and uses its own captured constants when
                    // the block does not look like the real thing. The mirror path learned this
                    // and got the fallback; this one, the only path that reaches the headset,
                    // kept the guard. Bind the per-frame constants in their place and let the
                    // shader decide, exactly as the mirror does.
                    if (!hudCb) hudCb = frameCb;
                    if (hud && hudBlur && hudExpo && frameCb && hudCb) {
                        ColorBlit::HudParams hp = cvr::CyberpunkVR_GetHudParams();
                        // PUT THE HUD AT A DISTANCE INSTEAD OF AT INFINITY.
                        //
                        // The engine draws the HUD into MAIN at MAIN's pixel coordinates and we
                        // paste that same surface into this eye at the same coordinates, so its
                        // disparity is zero -- which is optical infinity. A tester on a Pimax put
                        // it exactly right: "both images too far to the sides to converge". Look at
                        // the world an arm's length away and every icon splits by the full
                        // vergence angle, far outside what the eyes can fuse.
                        //
                        // Only this eye can move: MAIN's HUD is the engine's own draw. So the
                        // whole disparity goes here, which also slides the HUD's apparent centre
                        // by half of it -- about 0.4 deg at the default distance, well under what
                        // reads as off-centre.
                        //
                        // shift = IPD / distance, in radians, converted to UV through the eye's
                        // own half-width and half-FOV, so it is right at any resolution or lens.
                        // Negative: an object nearer than infinity sits LEFT of its infinity
                        // position in the right eye.
                        const float hudDistM = CyberpunkVR_HudDistanceM;
                        if (hudDistM > 0.05f && m_eyeSwapchains[1].width > 0) {
                            XrFovf f{};
                            const float ipd = GetRuntimeIpd();
                            if (GetCurrentEyeFov(1, &f) && ipd > 0.03f && ipd < 0.10f) {
                                const float halfTan = 0.5f * (tanf(f.angleRight) - tanf(f.angleLeft));
                                if (halfTan > 0.01f) {
                                    // pixels per radian at the centre = halfWidth / tan(halfFov)
                                    const float uvPerRad = 0.5f / halfTan;
                                    hp.hudShiftU = -(ipd / hudDistM) * uvPerRad;
                                }
                            }
                        }
                        vrcamEyeCaptured = m_colorBlit->RecordHudComposite(
                            m_captureCmdList, vrcamSrc, hud, hudBlur, hudExpo, frameCb, hudCb,
                            eyeSlotTex, hp);
                    }
                    if (!vrcamEyeCaptured) {
                        vrcamEyeCaptured = m_colorBlit->RecordBlit(m_captureCmdList, vrcamSrc,
                                                                   eyeSlotTex);
                    }
                    // The outline goes ON TOP of the finished eye, premultiplied, the way the
                    // engine's own composite adds it -- a separate pass rather than another input
                    // to the HUD shader, because it is a separate surface with its own lifetime
                    // and it is empty whenever nothing is being scanned. GetVisionTexture already
                    // returns null when the layer is stale, so the no-op case costs nothing.
                    if (vrcamEyeCaptured && CyberpunkVR_VisionToSecondEye) {
                        if (ID3D12Resource* vision = CyberpunkVR_GetVisionTexture()) {
                            if (m_colorBlit->RecordOverlay(m_captureCmdList, vision,
                                                           eyeSlotTex,
                                                           CyberpunkVR_VisionDebug,
                                                           CyberpunkVR_VisionFit != 0,
                                                           CyberpunkVR_VisionOffX,
                                                           CyberpunkVR_VisionOffY))
                                ++CyberpunkVR_DebugVisionOverlays;
                        }
                    }

                    // The barrel dot. Eye 0 gets it from the ImGui overlay on the backbuffer;
                    // this is the same point, at the same NDC, stamped into eye 1. It is a
                    // feature (the dot was simply missing on this side) and it is also the
                    // instrument that settles where the sight's reticle really points: with a
                    // dot in BOTH eyes the question stops being "does the right eye's reticle
                    // match the left eye's dot", which no one can judge across a fused pair.
                    if (vrcamEyeCaptured && CyberpunkVR_BarrelDotSecondEye &&
                        CyberpunkVR_BarrelDotSecondVisible &&
                        CyberpunkVR_BarrelDotTick &&
                        GetTickCount64() - CyberpunkVR_BarrelDotTick < 250) {
                        if (m_colorBlit->RecordDot(m_captureCmdList, eyeSlotTex,
                                                   CyberpunkVR_BarrelDotNdcX2,
                                                   CyberpunkVR_BarrelDotNdcY,
                                                   CyberpunkVR_BarrelDotRadiusPx,
                                                   1.0f, 0.045f, 0.045f, 1.0f))
                            ++CyberpunkVR_DebugBarrelDotDraws;
                    }

                    // AND THE OVERLAY ITSELF -- the F10 menu and the mouse cursor, which live in
                    // MAIN's backbuffer because that is where ImGui draws, and were therefore absent
                    // from this eye entirely. Recorded LAST, so the UI sits on top of the HUD, the
                    // scanner outline and the dot, exactly as it does on the flat screen.
                    //
                    // The distance knob is the same idea as CyberpunkVR_HudDistanceM and defaults to
                    // the same 0: only this eye can move, so the whole disparity goes here, and the
                    // shift is the panel's angular offset (ipd / distance) converted to pixels through
                    // this eye's own half-width and half-FOV, so it holds at any resolution or lens.
                    if (vrcamEyeCaptured && CyberpunkVR_OverlaySecondEye) {
                        float shiftPx = 0.0f;
                        const float ovlDistM = CyberpunkVR_OverlaySecondEyeDistM;
                        if (ovlDistM > 0.05f && m_eyeSwapchains[1].width > 0) {
                            XrFovf f{};
                            const float ipd = GetRuntimeIpd();
                            if (GetCurrentEyeFov(1, &f) && ipd > 0.03f && ipd < 0.10f) {
                                const float halfTan = 0.5f * (tanf(f.angleRight) - tanf(f.angleLeft));
                                if (halfTan > 0.01f) {
                                    const float pxPerRad = (0.5f * static_cast<float>(eyeW)) / halfTan;
                                    shiftPx = (ipd / ovlDistM) * pxPerRad;
                                }
                            }
                        }
                        OverlayRecordIntoTarget(m_captureCmdList, eyeSlotTex, shiftPx);
                    }

                    // Why this is done here at all rather than in the engine: eye 0 is MAIN's
                    // backbuffer and already carries the HUD, while eye 1 is the VRCAM view,
                    // for which the composition group bails on a per-view gate we cannot
                    // satisfy without handing the RTT view a full output resource set -- two
                    // crashes established that. sync_stereo snapshots the HUD surface inside
                    // the engine's own list, so what is sampled here is a committed resource
                    // that cannot be aliased out from under us and rests in COMMON, promoted
                    // implicitly for the read: no barrier of ours on anything the engine owns.

                    D3D12_RESOURCE_BARRIER toSrc = toRt;
                    toSrc.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
                    toSrc.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                    m_captureCmdList->ResourceBarrier(1, &toSrc);
                } else {
                    static bool s_blitWarned = false;
                    if (!s_blitWarned) {
                        s_blitWarned = true;
                        Log("OpenXRManager: stereo ColorBlit init failed (%ux%u)"
                            " -- right eye stays on MAIN\n", eyeW, eyeH);
                    }
                }
            }
        }
    }

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
        bool snapOk = false;
        // Resolve from the STAGING copy taken at the readable barrier, not from the engine's
        // buffer. The stage is ours and sits in PIXEL_SHADER_RESOURCE, so the resolve no
        // longer depends on what state the engine happens to leave its depth in at Present --
        // which is what made the depth layer flicker.
        ID3D12Resource* resolveSrc = nullptr;
        D3D12_RESOURCE_STATES resolveState = D3D12_RESOURCE_STATE_COMMON;
        bool fromStage = false;
        {
            std::lock_guard<std::mutex> lock(m_depthStageMutex);
            if (m_depthStage && m_depthStageSerial == serial) {
                resolveSrc = m_depthStage;
                resolveState = D3D12_RESOURCE_STATE_COPY_SOURCE;
                fromStage = true;
            }
        }
        if (!fromStage && srvReadable) {          // fallback: the old direct read
            resolveSrc = gameDepth;
            resolveState = static_cast<D3D12_RESOURCE_STATES>(depthStateRaw);
        }
        if (gateOk && resolveSrc && (snapOk = EnsureDepthSnapshot(gameDepth))) {
            depthCaptured = RecordDepthCapture(m_captureCmdList, resolveSrc, resolveState,
                                               /*transitionGameDepth=*/false);
        }
        // Say WHICH condition is holding depth back. Four separate gates decide this and the
        // submit line only ever reported the outcome (depth=0), which is why it stayed a
        // guess for so long.
        if ((serial % 300) == 1) {
            Log("Depth gate: res=%p stable=%u menu=%d srvReadable=%d(state=0x%X) stage=%d "
                "snapshot=%d captured=%d\n",
                gameDepth, s_depthGateStable, menuOpen ? 1 : 0, srvReadable ? 1 : 0,
                depthStateRaw, fromStage ? 1 : 0, snapOk ? 1 : 0, depthCaptured ? 1 : 0);
        }
    }

    m_captureCmdList->Close();
    ID3D12CommandList* cmdLists[] = {m_captureCmdList};
    m_d3dQueue->ExecuteCommandLists(1, cmdLists);

    ++m_captureFenceValue;
    m_d3dQueue->Signal(m_captureFence, m_captureFenceValue);

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        // Publish the buffer we just filled as the current frame. This is the hand-off:
        // everything the consumer needs -- image, poses, fovs, serial -- becomes visible in
        // one locked step, and the buffer it was reading before stays untouched.
        m_monoCapturedFrame.texture = snapshot;
        if (m_monoCapturedFrame.texture == snapshot) {
            m_monoCapturedFrame.serial = serial;
            m_monoCapturedFrame.captureMs = XrDiagNowMs();
            CyberpunkVR_DebugCapOk.fetch_add(1, std::memory_order_relaxed);
            for (int eye = 0; eye < 2; ++eye) {
                m_monoCapturedFrame.poses[eye] = poses[eye];
                m_monoCapturedFrame.fovs[eye] = fovs[eye];
                m_monoCapturedFrame.hasView[eye] = hasView[eye];
            }
            SetD3DNamef(m_monoCapturedFrame.texture, L"OpenXR_mono_snapshot_serial%llu",
                static_cast<unsigned long long>(serial));
            if (depthCaptured) {
                m_depthSnapshotSerial = serial;
            }
            // Stamp the right eye with the SAME serial as the colour it belongs to. The submit
            // requires the match, so a frame where the VRCAM blit did not happen simply falls
            // back to MAIN in that eye instead of pairing mismatched images.
            // PER-SLOT, so a submit can ask for the frame it is actually carrying rather than the
            // newest one. m_vrcamEyeSerial stays as the newest, for the log lines that report it.
            m_vrcamEyeSerial = vrcamEyeCaptured ? serial : 0;
            if (vrcamEyeCaptured) {
                m_vrcamEyePoolSerial[m_vrcamEyeSlot] = serial;
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
    // Skip depth swapchain creation unless the user explicitly opted in to mono depth
    // capture.
    const bool depthWanted = GetDepthSubmit() != 0 && GetMonoDepthCapture() != 0;
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

        // Publish the eye image's real geometry for the capture thread. It needs the format
        // and size to build its right-eye target, and it cannot look them up here: this vector
        // lives on the submit thread and gets resized out from under any other reader.
        if (eye == 0 && !m_eyeSwapchains[eye].images.empty() &&
            m_eyeSwapchains[eye].images[0].texture) {
            const D3D12_RESOURCE_DESC ed = m_eyeSwapchains[eye].images[0].texture->GetDesc();
            m_eyeImageW.store(static_cast<uint32_t>(ed.Width), std::memory_order_release);
            m_eyeImageH.store(ed.Height, std::memory_order_release);
            m_eyeImageFmt.store(static_cast<uint32_t>(ed.Format), std::memory_order_release);
            // The TYPED companion: what the swapchain was actually created as. The resource
            // above is typeless, and views must be typed.
            m_eyeViewFmt.store(static_cast<uint32_t>(selectedFormat), std::memory_order_release);
            Log("OpenXRManager: eye image %ux%u resourceFmt=%u viewFmt=%u\n",
                static_cast<unsigned>(ed.Width), ed.Height,
                static_cast<unsigned>(ed.Format), static_cast<unsigned>(selectedFormat));
        }

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
