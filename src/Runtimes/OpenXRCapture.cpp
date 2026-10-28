#include "Utils/DebugGate.hpp"
#include "Render/CommandResources.hpp"
#include "Render/GpuStageProfile.hpp"
// openxr_capture.cpp - mono frame + depth capture and submit-resource setup.
// Split verbatim from openxr_manager.cpp (OpenXRManager methods). Shared module
// state/helpers via openxr_internal.h (inline).
#include <atomic>
extern "C" int CyberpunkVR_BindPoseToImage;
#include "Runtimes/OpenXRManager.hpp"
#include "Camera/ImagePoseIdentity.hpp"
#include "Camera/ImagePoseMath.hpp"
#include "Camera/CameraState.hpp"
#include "Overlay/ImGuiOverlay.hpp"   // OverlayRecordIntoTarget, the second-eye pass
#include "Runtimes/OpenXRInternal.hpp"
#include "Utils/XrMath.hpp"
#include "Utils/SharedSlots.hpp"
#include "Utils/TestMarker.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
#include "Stereo/EngineStereo.hpp"
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
// THE SECOND EYE WITHOUT A COMPOSITE IS A COPY. When no HUD is composited into eye 1, ColorBlit's
// plain blit samples the _UNORM_SRGB snapshot (decoded to linear) into a _UNORM_SRGB target
// (encoded back) at the same size -- an identity, apart from forcing alpha to 1, which the
// projection layer ignores (no SOURCE_ALPHA flag; eye 0 is a CopyResource and keeps its alpha the
// same way). The GPU stage profile measured that pass at 134 us per frame against 34 us for
// eye 0's copy (run 834). 1 = CopyResource when size and the 8-bit RGBA family match, 0 = blit.
extern "C" __declspec(dllexport) int CyberpunkVR_SecondEyeCopy = 1;
extern "C" __declspec(dllexport) std::atomic<unsigned long long> CyberpunkVR_DebugSecondEyeCopies{0};
namespace {
int CopyFamily(DXGI_FORMAT f) {
    switch (f) {
        case DXGI_FORMAT_R8G8B8A8_TYPELESS: case DXGI_FORMAT_R8G8B8A8_UNORM: case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB: return 1;
        case DXGI_FORMAT_B8G8R8A8_TYPELESS: case DXGI_FORMAT_B8G8R8A8_UNORM: case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB: return 2;
        default: return 0;
    }
}
// CopyResource legality plus "the blit would not have scaled": same size, one mip, one layer,
// no MSAA, same 8-bit RGBA copy family.
bool CopyEquivalent(ID3D12Resource* src, ID3D12Resource* dst) {
    if (!src || !dst) return false;
    const D3D12_RESOURCE_DESC a = src->GetDesc(), b = dst->GetDesc();
    return a.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D && b.Dimension == a.Dimension &&
        a.Width == b.Width && a.Height == b.Height && a.DepthOrArraySize == 1 && b.DepthOrArraySize == 1 &&
        a.MipLevels == 1 && b.MipLevels == 1 && a.SampleDesc.Count == 1 && b.SampleDesc.Count == 1 &&
        CopyFamily(a.Format) && CopyFamily(a.Format) == CopyFamily(b.Format);
}
// One eye's half of a side-by-side backbuffer into an eye-sized image (engine stereo).
void CopyEyeHalf(ID3D12GraphicsCommandList* list, ID3D12Resource* dst, ID3D12Resource* src, uint32_t x,
                 uint32_t width, uint32_t height) {
    D3D12_TEXTURE_COPY_LOCATION to{};
    to.pResource = dst;
    to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION from{};
    from.pResource = src;
    from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    const D3D12_BOX box{x, 0, 0, x + width, height, 1};
    list->CopyTextureRegion(&to, 0, 0, 0, &from, &box);
}
}

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

// The whole window for a menu under engine stereo, and the blit that squeezes it into one eye's snapshot (the
// snapshot has the backbuffer's own format, which is the blit's target view). Rests in COPY_DEST.
bool OpenXRManager::EnsureMenuWindow(const D3D12_RESOURCE_DESC& backBufferDesc) {
    if (!m_d3dDevice || backBufferDesc.Width < 2 || !backBufferDesc.Height) return false;
    const D3D12_RESOURCE_DESC have = m_menuWindow ? m_menuWindow->GetDesc() : D3D12_RESOURCE_DESC{};
    if (!m_menuWindow || have.Width != backBufferDesc.Width || have.Height != backBufferDesc.Height ||
        have.Format != backBufferDesc.Format) {
        if (m_menuWindow) { m_menuWindow->Release(); m_menuWindow = nullptr; }
        D3D12_RESOURCE_DESC d{};
        d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        d.Width = backBufferDesc.Width;
        d.Height = backBufferDesc.Height;
        d.DepthOrArraySize = 1;
        d.MipLevels = 1;
        d.Format = backBufferDesc.Format;
        d.SampleDesc.Count = 1;
        d.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
        D3D12_HEAP_PROPERTIES hp{};
        hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        if (FAILED(m_d3dDevice->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_DEST,
                                                        nullptr, IID_PPV_ARGS(&m_menuWindow)))) {
            m_menuWindow = nullptr;
            Log("OpenXRManager: Failed to create the engine stereo menu window %llux%u fmt=%u\n",
                static_cast<unsigned long long>(d.Width), d.Height, static_cast<unsigned>(d.Format));
            return false;
        }
        SetD3DName(m_menuWindow, L"OpenXR_engine_stereo_menu_window");
        Log("OpenXRManager: engine stereo menu window ready %llux%u fmt=%u\n",
            static_cast<unsigned long long>(d.Width), d.Height, static_cast<unsigned>(d.Format));
    }
    if (!m_menuBlit) m_menuBlit = std::make_unique<ColorBlit>();
    return m_menuBlit->EnsureInitialized(m_d3dDevice, backBufferDesc.Format,
                                         static_cast<uint32_t>(backBufferDesc.Width / 2), backBufferDesc.Height);
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
    auto captureDesc = sourceDesc;
    captureDesc.Flags |= D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    for (int i = 0; i < 3; ++i) {
        if (FAILED(m_d3dDevice->CreateCommittedResource(
                &heapProps,
                D3D12_HEAP_FLAG_NONE,
                &captureDesc,
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
extern "C" ID3D12Resource* OmoAcquireSceneDepthResource(); // returns one owned reference
extern "C" unsigned int OmoGetSceneDepthState();
extern "C" unsigned int OmoGetSceneDepthWidth();
extern "C" unsigned int OmoGetSceneDepthHeight();
extern "C" unsigned int OmoGetSceneDepthFormat();
extern "C" ID3D12CommandQueue* OmoGetSceneDepthWriterQueue(); // game's depth-writer queue (safe mono depth capture)

bool OpenXRManager::EnsureStereoDepthSwapchains(const cvr::depth::Plan& plan) {
    if(!m_depthLayerSupported || !m_session || m_eyeSwapchains.size()!=2)return false;
    if(!m_depthSwapchainFormat) {
        uint32_t count{};
        if(XR_FAILED(xrEnumerateSwapchainFormats(m_session,0,&count,nullptr)) || !count)return false;
        std::vector<int64_t> formats(count);
        if(XR_FAILED(xrEnumerateSwapchainFormats(m_session,count,&count,formats.data())))return false;
        if(std::find(formats.begin(),formats.end(),int64_t(DXGI_FORMAT_D32_FLOAT))==formats.end()) {
            m_depthLayerSupported=false;cvr::framegen::SetDepthCaptureEnabled(false);return false;
        }
        m_depthSwapchainFormat=DXGI_FORMAT_D32_FLOAT;
    }
    for(unsigned eye=0;eye<2;++eye) {
        auto& swap=m_eyeSwapchains[eye];const auto& v=plan.eyes[eye];
        if(!v.width || !v.height || v.width>16384 || v.height>16384)return false;
        if(swap.depthHandle && swap.depthWidth==v.width && swap.depthHeight==v.height)continue;
        // DRS/resolution changes never destroy a resource still read by our GPU.
        if(!m_stereoDepth.Idle())return false;
        if(swap.depthPending) {
            XrSwapchainImageWaitInfo wait{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};wait.timeout=0;
            if(xrWaitSwapchainImage(swap.depthHandle,&wait)!=XR_SUCCESS)return false;
            XrSwapchainImageReleaseInfo release{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
            if(xrReleaseSwapchainImage(swap.depthHandle,&release)!=XR_SUCCESS)return false;
            swap.depthPending=false;
        }
        if(swap.depthHandle) {
            if(XR_FAILED(xrDestroySwapchain(swap.depthHandle)))return false;
            swap.depthHandle=XR_NULL_HANDLE;swap.depthImages.clear();
        }
        XrSwapchainCreateInfo info{XR_TYPE_SWAPCHAIN_CREATE_INFO};
        info.usageFlags=XR_SWAPCHAIN_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT|XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT;
        info.format=m_depthSwapchainFormat;info.sampleCount=1;info.width=v.width;info.height=v.height;
        info.faceCount=info.arraySize=info.mipCount=1;
        if(XR_FAILED(xrCreateSwapchain(m_session,&info,&swap.depthHandle)))return false;
        uint32_t count{};
        if(XR_FAILED(xrEnumerateSwapchainImages(swap.depthHandle,0,&count,nullptr)) || !count) {
            xrDestroySwapchain(swap.depthHandle);swap.depthHandle=XR_NULL_HANDLE;return false;
        }
        swap.depthImages.assign(count,{XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
        if(XR_FAILED(xrEnumerateSwapchainImages(swap.depthHandle,count,&count,
            reinterpret_cast<XrSwapchainImageBaseHeader*>(swap.depthImages.data())))) {
            xrDestroySwapchain(swap.depthHandle);swap.depthHandle=XR_NULL_HANDLE;swap.depthImages.clear();return false;
        }
        swap.depthWidth=v.width;swap.depthHeight=v.height;
        for(uint32_t i=0;i<count;++i)SetD3DNamef(swap.depthImages[i].texture,L"OpenXR_eye%u_depth_image%u",eye,i);
        Log("OpenXRManager: per-eye depth swapchain eye=%u %ux%u D32_FLOAT\n",eye,v.width,v.height);
    }
    return true;
}

bool OpenXRManager::EnsureDepthSnapshot(ID3D12Resource* gameDepth) {
    std::lock_guard<std::mutex> snapshotLock(m_presentMutex);
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
    // The consumer rejects menu/loading depth too. Do not create/copy transient
    // startup textures that will never be submitted to the headset.
    if (!GetMonoDepthCapture() || !GetDepthSubmit() || GetMenuMode() || GetMenuRectMode() ||
        !m_sessionRunning.load(std::memory_order_relaxed) || m_stopFrameThread.load(std::memory_order_relaxed) ||
        !m_depthStageCaptureAllowed.load(std::memory_order_acquire)) return;
    // The engine can make the depth readable more than once per frame; one copy is enough.
    uint64_t frame = m_presentCount.load(std::memory_order_relaxed);
    if (m_depthStageFrame.load(std::memory_order_acquire) == frame) return;
    std::unique_lock<std::mutex> lock(m_depthStageMutex, std::try_to_lock);
    if (!lock.owns_lock()) return;
    frame = m_presentCount.load(std::memory_order_relaxed);
    if (m_depthStageFrame.load(std::memory_order_relaxed) == frame) return;

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
    // A size change can release the owner's old stage immediately, but every
    // recorded copy retains its exact source/destination until the GPU is done.
    if (!cvr::gpu::KeepCommandResources(list, {gameDepth, m_depthStage})) return;
    // Publish before ResourceBarrier re-enters the depth hook.
    m_depthStageFrame.store(frame, std::memory_order_release);

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
    // keeps them together through the engine's render-ahead.
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
    if (!cvr::gpu::KeepCommandResources(cmdList, {gameDepth, m_depthSnapshot})) return false;
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
    Microsoft::WRL::ComPtr<ID3D12Resource> depthLease;
    depthLease.Attach(OmoAcquireSceneDepthResource());
    ID3D12Resource* gameDepth = depthLease.Get();
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

ID3D12Resource* OpenXRManager::AcquireCapturedVrcamLocked(uint64_t serial,uint32_t maxReuse,
        XrPosef* pose,XrFovf* fov,uint64_t* poseId,bool* reused) {
    int best=-1;
    uint64_t bestSerial=0;
    for(int i=0;i<kVrcamEyeSlots;++i) {
        const auto sourceSerial=m_vrcamEyePoolSerial[i];
        auto* source=m_vrcamEyePool[i];
        if(!source || !sourceSerial || sourceSerial>serial || !m_vrcamEyeLabels[i].valid ||
           m_captureLeases.Writing(reinterpret_cast<uintptr_t>(source)))continue;
        if(sourceSerial!=serial && (!maxReuse || serial-sourceSerial>maxReuse))continue;
        if(sourceSerial>bestSerial) { best=i;bestSerial=sourceSerial; }
    }
    if(best<0)return nullptr;
    auto* resource=m_vrcamEyePool[best];
    if(!m_captureLeases.Pin(reinterpret_cast<uintptr_t>(resource)))return nullptr;
    resource->AddRef();
    const auto& label=m_vrcamEyeLabels[best];
    *pose=label.pose;*fov=label.fov;*poseId=label.poseId;*reused=bestSerial!=serial;
    return resource;
}

void OpenXRManager::ReleaseCapturedRead(ID3D12Resource* resource,uint64_t fence) {
    if(!resource)return;
    std::vector<ID3D12Resource*> release;
    const auto completed=m_fence ? m_fence->GetCompletedValue() : UINT64_MAX;
    {
        std::lock_guard lock(m_presentMutex);
        m_captureLeases.Unpin(reinterpret_cast<uintptr_t>(resource));
        if(fence && completed<fence)m_retiredCaptureReads.push_back({resource,fence});
        else release.push_back(resource);
        for(auto it=m_retiredCaptureReads.begin();it!=m_retiredCaptureReads.end();) {
            if(it->fence<=completed) { release.push_back(it->resource);it=m_retiredCaptureReads.erase(it); }
            else ++it;
        }
    }
    for(auto* item:release)item->Release();
}

bool OpenXRManager::CaptureMonoPresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, uint64_t serial,
    const XrPosef poses[2], const XrFovf fovs[2], const bool hasView[2],const XrPosef headCenters[2]) {
    if (!backBuffer || !hasView[0] || !hasView[1]) {
        CVR_DIAGNOSTIC(CyberpunkVR_DebugCapSkipNoView.fetch_add(1, std::memory_order_relaxed));
        return false;
    }

    std::lock_guard<std::mutex> captureLock(m_captureMutex);
    uintptr_t writeResources[2]{};
    cvr::capture::WriteRelease releaseWrites(m_captureLeases,m_presentMutex,writeResources);
    // ENGINE STEREO (Stereo/EngineStereo.hpp): the backbuffer holds both eyes, left eye at [0, W - W/2), right
    // eye at [W - W/2, W) -- the engine's own split. MAIN's eye takes its half into the snapshot, the other eye
    // its half into the second-eye slot below; each is an eye-sized image, exactly what the swapchains are.
    // A MENU is not split, nor the loading screen: ink draws it once over the whole window (the halves showed each
    // eye the same left half of the inventory). Nor a frame the engine rendered mono (before the main menu: splash
    // screens, intro videos). Both eyes then get the whole window squeezed into the snapshot, mono as in the port's
    // other modes; the panel gets the window's aspect back (OpenXRFrameLoop.cpp, the menu quad). The loading screen
    // over a frame rendered in stereo is drawn into each half already squeezed (EngineStereo.hpp LoadingSideBySide):
    // MAIN's eye's half is that same picture and is copied as it is (not under the main menu, drawn once over the
    // window: its right half alone showed the menu shifted).
    const bool menuOpen = (GetMenuRectMode() != 0) || (GetMenuMode() != 0) || cvr::engine_stereo::OnePicture();
    const bool engineStereo = cvr::engine_stereo::Enabled() && sourceDesc.Width >= 2;
    const bool split = engineStereo && !menuOpen;
    const bool squeeze = engineStereo && menuOpen && !cvr::engine_stereo::LoadingSideBySide();
    // Bounded transition trace, not per-frame logging. Useful for splash variants.
    static unsigned lastLayout = ~0u, layoutLogs = 0;
    const unsigned layout = unsigned(menuOpen) | (unsigned(split) << 1) | (unsigned(squeeze) << 2);
    if (layout != lastLayout) {
        lastLayout = layout;
        if (layoutLogs++ < 32)
            Log("[menu-capture] serial=%llu panel=%d split=%d fullWindow=%d source=%llux%u\n",
                static_cast<unsigned long long>(serial), int(menuOpen), int(split), int(squeeze),
                static_cast<unsigned long long>(sourceDesc.Width), sourceDesc.Height);
    }
    D3D12_RESOURCE_DESC eyeDesc = sourceDesc;
    if (engineStereo) eyeDesc.Width = sourceDesc.Width / 2;
    const uint32_t mainEyeIndex = CyberpunkVR_MainIsRightEye ? 1u : 0u;
    // The x where an eye's half starts in the backbuffer.
    const auto halfX = [&](uint32_t eye) {
        return eye == 0 ? 0u : static_cast<uint32_t>(sourceDesc.Width - eyeDesc.Width);
    };
    // Never fall back to half of a full-window splash if the blitter is unavailable.
    if (squeeze && !EnsureMenuWindow(sourceDesc)) return false;
    if (!EnsureMonoCaptureResource(eyeDesc)) {
        CVR_DIAGNOSTIC(CyberpunkVR_DebugCapSkipNoRes.fetch_add(1, std::memory_order_relaxed));
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
        for(uint32_t step=1;step<=3;++step) {
            const auto slot=(m_monoPoolSlot+step)%3;
            auto* candidate=m_monoPool[slot];
            if(candidate==m_monoCapturedFrame.texture)continue;
            if(m_captureLeases.BeginWrite(reinterpret_cast<uintptr_t>(candidate))) {
                m_monoPoolSlot=slot;snapshot=candidate;
                writeResources[0]=reinterpret_cast<uintptr_t>(candidate);break;
            }
        }
        previousSerial = m_monoCapturedFrame.serial;
        if (snapshot) {
            snapshot->AddRef();
        }
    }
    if (!snapshot) {
        CVR_DIAGNOSTIC(CyberpunkVR_DebugCapSkipNoSlot.fetch_add(1, std::memory_order_relaxed));
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
        const bool diagnostics=cvr::RuntimeDiagnosticsEnabled();
        const double fenceEnterMs = diagnostics ? XrDiagNowMs() : 0;
        const DWORD fenceRes = WaitForSingleObject(m_captureFenceEvent, 100);
        if(diagnostics) {
        const double fenceMs = XrDiagNowMs() - fenceEnterMs;
        CVR_DIAGNOSTIC(CyberpunkVR_DebugCapFenceWaits.fetch_add(1, std::memory_order_relaxed));
        const unsigned long long fenceUs = (unsigned long long)(fenceMs * 1000.0);
        CVR_DIAGNOSTIC(CyberpunkVR_DebugCapFenceUsSum.fetch_add(fenceUs, std::memory_order_relaxed));
        unsigned long long prevMax =
            CyberpunkVR_DebugCapFenceUsMax.load(std::memory_order_relaxed);
        while (fenceUs > prevMax &&
               !CyberpunkVR_DebugCapFenceUsMax.compare_exchange_weak(
                   prevMax, fenceUs, std::memory_order_relaxed)) {
        }
        }
        if (fenceRes != WAIT_OBJECT_0) {
            CVR_DIAGNOSTIC(CyberpunkVR_DebugCapSkipFence.fetch_add(1, std::memory_order_relaxed));
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

    auto gpuProfile = cvr::gpu::profile::Begin(m_captureCmdList);
    {
    cvr::gpu::profile::Scope gpuScope(gpuProfile, cvr::gpu::profile::Stage::CaptureMain);
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
    bool squeezed = false;
    if (squeeze && EnsureMenuWindow(sourceDesc)) {
        m_captureCmdList->CopyResource(m_menuWindow, backBuffer);
        D3D12_RESOURCE_BARRIER blit[2] = {};
        for (auto& barrier : blit) {
            barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        }
        blit[0].Transition.pResource = m_menuWindow;
        blit[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
        blit[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        blit[1].Transition.pResource = snapshot;
        blit[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
        blit[1].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
        m_captureCmdList->ResourceBarrier(2, blit);
        squeezed = m_menuBlit->RecordBlit(m_captureCmdList, m_menuWindow, snapshot);
        for (auto& barrier : blit) std::swap(barrier.Transition.StateBefore, barrier.Transition.StateAfter);
        m_captureCmdList->ResourceBarrier(2, blit);
    }
    if (!squeezed) {
        if (engineStereo) CopyEyeHalf(m_captureCmdList, snapshot, backBuffer, halfX(mainEyeIndex), uint32_t(eyeDesc.Width), eyeDesc.Height);
        else m_captureCmdList->CopyResource(snapshot, backBuffer);
    }

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
    }

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
    ID3D12Resource* vrcamCaptureSource=nullptr;
    const uint32_t eyeW = static_cast<uint32_t>(eyeDesc.Width);
    const uint32_t eyeH = sourceDesc.Height;
    // NOT IN A MENU. The right eye below is VRCAM's view of the WORLD; the menu is not in it,
    // because a menu is drawn as one mono surface into the backbuffer. Taking VRCAM here
    // anyway gives the left eye the menu and the right eye a frozen street, which is the
    // reported "the headset shows VRCAM and not the menu": with the two eyes disagreeing
    // that completely, the scene is what reads.
    // Skipping it leaves both eyes on the backbuffer snapshot, i.e. the menu, mono -- which is
    // what a mono surface should look like.
    if (split) {
        // ENGINE STEREO: the second eye is the other half of the same backbuffer. The engine drew it whole --
        // HUD and scanner per eye -- so it is copied as it is, with nothing composited on top. A plain
        // copy needs the slot's format family, the R8G8B8A8 one the swapchains are picked for.
        const bool rgba8 = CopyFamily(sourceDesc.Format) == 1;
        if (!rgba8) {
            static bool s_formatWarned = false;
            if (!s_formatWarned) {
                s_formatWarned = true;
                Log("[engine-stereo] backbuffer format %u is not R8G8B8A8: the second eye stays on MAIN's half\n",
                    static_cast<unsigned>(sourceDesc.Format));
            }
        }
        if (CyberpunkVR_StereoSubmit && rgba8 && eyeW && eyeH &&
            EnsureVrcamEyeTexture(eyeW, eyeH, DXGI_FORMAT_R8G8B8A8_TYPELESS)) {
            {
                std::lock_guard<std::mutex> lock(m_presentMutex);
                for(int step=1;step<=kVrcamEyeSlots;++step) {
                    const auto slot=(m_vrcamEyeSlot+step)%kVrcamEyeSlots;
                    if(m_vrcamEyePoolSerial[slot]==m_monoCapturedFrame.serial && m_monoCapturedFrame.serial)continue;
                    auto* candidate=m_vrcamEyePool[slot];
                    if(m_captureLeases.BeginWrite(reinterpret_cast<uintptr_t>(candidate))) {
                        m_vrcamEyeSlot=slot;writeResources[1]=reinterpret_cast<uintptr_t>(candidate);break;
                    }
                }
            }
            if(!writeResources[1]) { m_captureCmdList->Close();snapshot->Release();return false; }
            ID3D12Resource* const eyeSlotTex = m_vrcamEyePool[m_vrcamEyeSlot];
            cvr::gpu::profile::Scope gpuScope(gpuProfile, cvr::gpu::profile::Stage::CaptureSecond);
            D3D12_RESOURCE_BARRIER b[2] = {};
            for (auto& barrier : b) {
                barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            }
            b[0].Transition.pResource = backBuffer;
            b[0].Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
            b[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
            b[1].Transition.pResource = eyeSlotTex;
            b[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
            b[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
            m_captureCmdList->ResourceBarrier(2, b);
            CopyEyeHalf(m_captureCmdList, eyeSlotTex, backBuffer, halfX(mainEyeIndex ^ 1u), eyeW, eyeH);
            for (auto& barrier : b) std::swap(barrier.Transition.StateBefore, barrier.Transition.StateAfter);
            m_captureCmdList->ResourceBarrier(2, b);
            vrcamEyeCaptured = true;
        }
    } else if (!engineStereo && CyberpunkVR_StereoSubmit && eyeW && eyeH && !(menuOpen && CyberpunkVR_MonoMenu)) {
        ID3D12Resource* vrcamSrc = CyberpunkVR_GetVrcamEyeTextureFresh();
        vrcamCaptureSource=vrcamSrc;
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
                CVR_DIAGNOSTIC(CyberpunkVR_DebugEyeAgeCount.fetch_add(1, std::memory_order_relaxed));
                CVR_DIAGNOSTIC(CyberpunkVR_DebugEyeAgeSumMs.fetch_add(ageUs, std::memory_order_relaxed));
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
                CVR_DIAGNOSTIC(CyberpunkVR_DebugEyeAgeBuckets[b].fetch_add(1, std::memory_order_relaxed));
            } else {
                CVR_DIAGNOSTIC(CyberpunkVR_DebugEyeAgeNever.fetch_add(1, std::memory_order_relaxed));
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
                        for(int step=1;step<=kVrcamEyeSlots;++step) {
                            const auto slot=(m_vrcamEyeSlot+step)%kVrcamEyeSlots;
                            if(m_vrcamEyePoolSerial[slot]==m_monoCapturedFrame.serial && m_monoCapturedFrame.serial)continue;
                            auto* candidate=m_vrcamEyePool[slot];
                            if(m_captureLeases.BeginWrite(reinterpret_cast<uintptr_t>(candidate))) {
                                m_vrcamEyeSlot=slot;writeResources[1]=reinterpret_cast<uintptr_t>(candidate);break;
                            }
                        }
                    }
                    if(!writeResources[1]) { m_captureCmdList->Close();snapshot->Release();return false; }
                    ID3D12Resource* const eyeSlotTex = m_vrcamEyePool[m_vrcamEyeSlot];
                    cvr::gpu::profile::Scope gpuScope(gpuProfile, cvr::gpu::profile::Stage::CaptureSecond);

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
                        // MAIN's HUD is laid out on MAIN's window; under the asymmetric projection this
                        // eye's window is centred elsewhere, so the HUD moves by the difference of the
                        // two (a positive move samples further left).
                        AsymEyeWindows asym{};
                        if (cvr::projection::Active() && GetAsymmetricEyeWindows(&asym)) {
                            const int mainEye = CyberpunkVR_MainIsRightEye ? 1 : 0;
                            hp.hudShiftU -= 0.5f * cvr::projection::MapNdcX(0.0f, asym.eye[mainEye], asym.eye[1 - mainEye]);
                        }
                        cvr::gpu::profile::Scope hudScope(gpuProfile, cvr::gpu::profile::Stage::SecondHud);
                        vrcamEyeCaptured = m_colorBlit->RecordHudComposite(
                            m_captureCmdList, vrcamSrc, hud, hudBlur, hudExpo, frameCb, hudCb,
                            eyeSlotTex, hp);
                    }
                    if (!vrcamEyeCaptured) {
                        cvr::gpu::profile::Scope blitScope(gpuProfile, cvr::gpu::profile::Stage::SecondBlit);
                        if (CyberpunkVR_SecondEyeCopy && CopyEquivalent(vrcamSrc, eyeSlotTex)) {
                            // vrcamSrc is promoted from COMMON to COPY_SOURCE implicitly, as it is to
                            // a shader-read state for the blit; only our own slot is transitioned.
                            D3D12_RESOURCE_BARRIER b{};
                            b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            b.Transition.pResource = eyeSlotTex;
                            b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            b.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            b.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            m_captureCmdList->ResourceBarrier(1, &b);
                            m_captureCmdList->CopyResource(eyeSlotTex, vrcamSrc);
                            b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            b.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;   // overlays below draw on top
                            m_captureCmdList->ResourceBarrier(1, &b);
                            vrcamEyeCaptured = true;
                            CVR_DIAGNOSTIC(++CyberpunkVR_DebugSecondEyeCopies);
                        } else {
                            vrcamEyeCaptured = m_colorBlit->RecordBlit(m_captureCmdList, vrcamSrc,
                                                                       eyeSlotTex);
                        }
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
                                CVR_DIAGNOSTIC(++CyberpunkVR_DebugVisionOverlays);
                        }
                    }

                    // The barrel dot. Eye 0 gets it from the ImGui overlay on the backbuffer;
                    // this is the same point, at the same NDC, stamped into eye 1. It is a
                    // feature (the dot was simply missing on this side) and it is also the
                    // instrument that settles where the sight's reticle really points: with a
                    // dot in BOTH eyes the question stops being "does the right eye's reticle
                    // match the left eye's dot", which no one can judge across a fused pair.
                    if (vrcamEyeCaptured && CyberpunkVR_BarrelDotSecondEye &&
                        CyberpunkVR_BarrelDotTick &&
                        GetTickCount64() - CyberpunkVR_BarrelDotTick < 250) {
                        if (m_colorBlit->RecordDot(m_captureCmdList, eyeSlotTex,
                                                   CyberpunkVR_BarrelDotNdcX2,
                                                   CyberpunkVR_BarrelDotNdcY,
                                                   CyberpunkVR_BarrelDotRadiusPx,
                                                   1.0f, 0.045f, 0.045f, 1.0f))
                            CVR_DIAGNOSTIC(++CyberpunkVR_DebugBarrelDotDraws);
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
                        // The overlay was laid out on MAIN's window. Under the asymmetric projection the
                        // second eye's window is centred elsewhere, so the same direction sits at another
                        // NDC there: carry it across (adding to DisplayPos moves the overlay LEFT).
                        AsymEyeWindows asym{};
                        if (cvr::projection::Active() && GetAsymmetricEyeWindows(&asym)) {
                            const int mainEye = CyberpunkVR_MainIsRightEye ? 1 : 0;
                            const float move = cvr::projection::MapNdcX(0.0f, asym.eye[mainEye], asym.eye[1 - mainEye]);
                            shiftPx -= move * 0.5f * static_cast<float>(eyeW);
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
    // worst at intro / menu-load where the depth resource + state are transient (as
    // observed). Instead the resolve reads it as an SRV in whatever shader-readable
    // state the game already left it, so we only capture when:
    //   * mono depth is enabled,
    //   * a menu is NOT open,
    //   * the scene-depth has been the SAME resource (menus closed) for a warmup window
    //     -> skips the intro/menu-load transient depth entirely, and
    //   * it is shader-readable THIS frame.
    // Resource references must also survive recording and execution. A CPU-local
    // pointer alone does not keep a D3D12 copy's operands alive on the GPU.
    bool depthCaptured = false;
    // This snapshot has only one camera's depth. A stereo layer needs separate, frame-matched
    // depth for each eye, so do not allocate/copy this mono hint for a two-view submission.
    const bool captureMonoDepth = GetMonoDepthCapture() != 0 && m_eyeSwapchains.size() == 1;
    if (!captureMonoDepth) m_depthStageCaptureAllowed.store(false, std::memory_order_release);
    if (captureMonoDepth) {
        Microsoft::WRL::ComPtr<ID3D12Resource> depthLease;
        depthLease.Attach(OmoAcquireSceneDepthResource());
        ID3D12Resource* gameDepth = depthLease.Get();
        const UINT depthStateRaw = OmoGetSceneDepthState();
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
        m_depthStageCaptureAllowed.store(gateOk && GetDepthSubmit() != 0, std::memory_order_release);
        bool snapOk = false;
        // Resolve from the STAGING copy taken at the readable barrier, not from the engine's
        // buffer. The stage is ours and sits in PIXEL_SHADER_RESOURCE, so the resolve no
        // longer depends on what state the engine happens to leave its depth in at Present --
        // which is what made the depth layer flicker.
        ID3D12Resource* resolveSrc = nullptr;
        Microsoft::WRL::ComPtr<ID3D12Resource> stageLease;
        D3D12_RESOURCE_STATES resolveState = D3D12_RESOURCE_STATE_COMMON;
        bool fromStage = false;
        {
            std::lock_guard<std::mutex> lock(m_depthStageMutex);
            if (m_depthStage && m_depthStageSerial == serial) {
                stageLease = m_depthStage;
                resolveSrc = stageLease.Get();
                resolveState = D3D12_RESOURCE_STATE_COPY_SOURCE;
                fromStage = true;
            }
        }
        if (!fromStage && srvReadable) {          // fallback: the old direct read
            resolveSrc = gameDepth;
            resolveState = static_cast<D3D12_RESOURCE_STATES>(depthStateRaw);
        }
        if (gateOk && resolveSrc && (snapOk = EnsureDepthSnapshot(gameDepth))) {
            cvr::gpu::profile::Scope depthScope(gpuProfile, cvr::gpu::profile::Stage::CaptureDepth);
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

    // Filter each owned real capture once. Framegen and every later repeat see
    // the same filtered endpoint; generated images never run through CAS again.
    const float casAmount = GetVrSharpmix();
    if (casAmount > 0.0f) {
        if (!m_sharpenPass) m_sharpenPass = std::make_unique<SharpenPass>();
        for (auto* color : {snapshot, vrcamEyeCaptured ? m_vrcamEyePool[m_vrcamEyeSlot] : nullptr}) {
            if (!color) continue;
            const auto desc = color->GetDesc();
            if (m_sharpenPass->EnsureInitialized(m_d3dDevice, desc.Format, uint32_t(desc.Width), desc.Height))
                m_sharpenPass->RecordSharpen(m_captureCmdList, color, color, GetVrSharpness(), casAmount);
        }
    }
    m_captureCmdList->Close();
    ID3D12CommandList* cmdLists[] = {m_captureCmdList};
    XrPosef capturedPoses[2]={poses[0],poses[1]};
    uint64_t capturedPoseIds[2]{},capturedImageGenerations[2]{};
    std::shared_ptr<const cvr::framegen::Inputs> capturedInputs[2];
    {
        // Select labels at the actual queue submission, not when a producer
        // merely recorded commands. The same-queue copy now sits between the
        // labelled writes and any subsequent replacement of the source images.
        std::lock_guard submissionLock(cvr::camera::ImageSubmissionMutex());
        const auto mainImage=cvr::camera::ReadImagePose(backBuffer);
        const auto vrcamImage=cvr::camera::ReadImagePose(vrcamCaptureSource);
        const uint32_t vrcamEye=CyberpunkVR_MainIsRightEye ? 0u : 1u;
        for(uint32_t eye=0;eye<2;++eye) {
            // Under engine stereo both eyes are halves of MAIN's own image and carry its label.
            const bool separate=vrcamEyeCaptured && eye==vrcamEye && !split;
            const auto& source=separate ? vrcamImage : mainImage;
            const uint32_t view=separate ? 2u : 1u;
            // Framegen's inputs for this eye: under engine stereo its half's (eye e shows half e; Framegen/Inputs.cpp
            // keeps the left half's in the first pool), none for a menu's squeezed window -- framegen does not
            // generate under a menu.
            const uint32_t inputView=split ? eye+1u : engineStereo ? 0u : view;
            if(CyberpunkVR_BindPoseToImage && !ExternalPoseResetPending() && source && source.pose && source.pose.view==view &&
               source.queue==reinterpret_cast<uintptr_t>(m_d3dQueue) &&
               source.pose.head.originSerial==GetTrackingOriginSerial()) {
                capturedPoses[eye]=cvr::camera::RebaseImageEyePose(headCenters[eye],poses[eye],source.pose.localHead);
                capturedPoseIds[eye]=source.pose.poseId;
                capturedImageGenerations[eye]=source.generation;
                if(inputView && cvr::framegen::CaptureInputsEnabled() && source.pose.hasRenderFrameId)
                    capturedInputs[eye]=cvr::framegen::AcquireInputs(inputView,source.pose.renderFrameId,
                        source.pose.poseId,source.pose.head.originSerial,m_d3dQueue);
                CVR_DIAGNOSTIC(CyberpunkVR_PoseIdCaptured[view-1].fetch_add(1,std::memory_order_relaxed));
                CVR_DIAGNOSTIC(CyberpunkVR_PoseIdLastCapture[eye].store(source.pose.poseId,std::memory_order_relaxed));
                CVR_DIAGNOSTIC(CyberpunkVR_PoseIdImageGeneration[eye].store(source.generation,std::memory_order_relaxed));
            } else {
                CVR_DIAGNOSTIC(CyberpunkVR_PoseIdCaptureMiss[view-1].fetch_add(1,std::memory_order_relaxed));
                CVR_DIAGNOSTIC(CyberpunkVR_PoseIdLastCapture[eye].store(0,std::memory_order_relaxed));
                CVR_DIAGNOSTIC(CyberpunkVR_PoseIdImageGeneration[eye].store(0,std::memory_order_relaxed));
            }
        }
        m_d3dQueue->ExecuteCommandLists(1,cmdLists);
        ++m_captureFenceValue;
        if (SUCCEEDED(m_d3dQueue->Signal(m_captureFence,m_captureFenceValue)))
            cvr::gpu::profile::Submitted(gpuProfile, m_d3dQueue, m_captureFence, m_captureFenceValue);
    }

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        // Publish the buffer we just filled as the current frame. This is the hand-off:
        // everything the consumer needs -- image, poses, fovs, serial -- becomes visible in
        // one locked step, and the buffer it was reading before stays untouched.
        m_monoCapturedFrame.texture = snapshot;
        if (m_monoCapturedFrame.texture == snapshot) {
            m_monoCapturedFrame.serial = serial;
            m_monoCapturedFrame.captureMs = XrDiagNowMs();
            m_monoCapturedFrame.menuPanel = menuOpen;
            // SBS loading UI is a full-window design squeezed into each half too.
            m_monoCapturedFrame.panelAspect = engineStereo && sourceDesc.Height
                ? float(sourceDesc.Width) / float(sourceDesc.Height) : 1.0f;
            CVR_DIAGNOSTIC(CyberpunkVR_DebugCapOk.fetch_add(1, std::memory_order_relaxed));
            for (int eye = 0; eye < 2; ++eye) {
                m_monoCapturedFrame.poses[eye] = capturedPoses[eye];
                m_monoCapturedFrame.poseIds[eye]=capturedPoseIds[eye];
                m_monoCapturedFrame.imageGenerations[eye]=capturedImageGenerations[eye];
                m_monoCapturedFrame.framegenInputs[eye]=capturedInputs[eye];
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
                const auto eye=CyberpunkVR_MainIsRightEye ? 0u : 1u;
                m_vrcamEyeLabels[m_vrcamEyeSlot]={capturedPoses[eye],fovs[eye],capturedPoseIds[eye],capturedImageGenerations[eye],true};
            }
        }
        for(auto& resource:writeResources) {
            if(resource)m_captureLeases.EndWrite(resource);
            resource=0;
        }
    }
    if (m_monoPresentEvent) {
        SetEvent(m_monoPresentEvent);
    }

    if(cvr::framegen::MetricsEnabled())cvr::framegen::OnRealFrame(serial,cvr::framegen::NowMs());

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

    // Once per process: this function runs every frame, and the marker is a file-system query.
    static const bool testConvert = [] {
        const bool on = cvr::TestMarker("cvr_swapchain_convert.txt");
        if (on) Log("OpenXRManager: TEST: sRGB companion swapchain format passed over (cvr_swapchain_convert.txt)\n");
        return on;
    }();
    const MonoSwapchainFormat formatPick =
        PickMonoSwapchainFormat(runtimeFormats, static_cast<int64_t>(format), testConvert);
    const int64_t selectedFormat = formatPick.format;

    // Input collection is shared with framegen, but can run depth-only. Depth
    // swapchains are created lazily from the selected per-eye snapshots below.
    cvr::framegen::SetDepthCaptureEnabled(m_depthLayerSupported && GetDepthSubmit() && GetMonoDepthCapture());

    uint32_t viewCount = 0;
    xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, 0, &viewCount, nullptr);
    m_viewConfigViews.resize(viewCount, {XR_TYPE_VIEW_CONFIGURATION_VIEW});
    xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, viewCount, &viewCount, m_viewConfigViews.data());
    m_views.resize(viewCount, {XR_TYPE_VIEW});

    const bool colorResourcesReady = !m_eyeSwapchains.empty() &&
        m_eyeSwapchains[0].width == static_cast<int32_t>(width) &&
        m_eyeSwapchains[0].height == static_cast<int32_t>(height) &&
        m_cmdAllocators[0] && m_cmdLists[0] && m_fence && m_fenceEvent && m_rtvHeap;
    if (colorResourcesReady) {
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
    if (m_rtvHeap) { m_rtvHeap->Release(); m_rtvHeap = nullptr; }

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
        // The runtime creates these, so nothing here has ever named them, and the debug layer
        // therefore reports every complaint about them as 'Unnamed ID3D12Resource Object'. A name
        // is what turned error 527 from 'some texture' into 'eye 1, image 2' the first time.
        for (uint32_t im = 0; im < imageCount && im < m_eyeSwapchains[eye].images.size(); ++im) {
            if (m_eyeSwapchains[eye].images[im].texture)
                SetD3DNamef(m_eyeSwapchains[eye].images[im].texture,
                            L"OpenXR_eye%u_color_image%u", static_cast<unsigned>(eye), im);
        }

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

    }

    // Stable descriptors for every runtime-owned image. The normal submit path used to
    // only copy into these images and therefore never populated EyeSwapchain::rtvs.
    UINT colorImageCount = 0;
    for (const auto& eye : m_eyeSwapchains) colorImageCount += static_cast<UINT>(eye.images.size());
    D3D12_DESCRIPTOR_HEAP_DESC colorHeap{};
    colorHeap.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    colorHeap.NumDescriptors = colorImageCount;
    if (!colorImageCount || FAILED(m_d3dDevice->CreateDescriptorHeap(&colorHeap, IID_PPV_ARGS(&m_rtvHeap)))) {
        Log("OpenXRManager: failed to create eye RTV descriptors\n");
        return false;
    }
    m_rtvDescriptorSize = m_d3dDevice->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    auto colorRtv = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    for (auto& eye : m_eyeSwapchains) {
        eye.rtvs.clear();
        for (const auto& image : eye.images) {
            D3D12_RENDER_TARGET_VIEW_DESC desc{};
            desc.Format = static_cast<DXGI_FORMAT>(selectedFormat);
            desc.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
            m_d3dDevice->CreateRenderTargetView(image.texture, &desc, colorRtv);
            eye.rtvs.push_back(colorRtv);
            colorRtv.ptr += m_rtvDescriptorSize;
        }
    }
    m_depthSwapchainFormat = 0;

    char formatSummary[512] = {};
    int summaryPos = sprintf_s(formatSummary, "OpenXRManager: Mono swapchain formats. game=%u selected=%lld convert=%d runtime:",
                               format, selectedFormat, formatPick.convert ? 1 : 0);
    if (summaryPos > 0) {
        for (uint32_t i = 0; i < runtimeFormatCount && summaryPos > 0 && summaryPos < static_cast<int>(sizeof(formatSummary) - 32); ++i) {
            summaryPos += sprintf_s(formatSummary + summaryPos, sizeof(formatSummary) - summaryPos, " %lld", runtimeFormats[i]);
        }
        Log("%s\n", formatSummary);
    }
    // No sRGB companion of the game's format: the submit draws each eye through the decode instead of
    // copying it (PickMonoSwapchainFormat).
    m_convertForSwapchain = false;
    if (formatPick.convert) {
        if (!m_srgbToLinearPass) m_srgbToLinearPass = std::make_unique<SrgbToLinearPass>();
        if (!m_srgbToLinearPass->EnsureInitialized(m_d3dDevice, static_cast<DXGI_FORMAT>(selectedFormat), width, height)) {
            Log("OpenXRManager: the sRGB decode for swapchain format %lld failed to initialise\n", selectedFormat);
            return false;
        }
        m_convertForSwapchain = true;
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
