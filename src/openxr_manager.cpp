#include "openxr_manager.h"
#include <cstdio>
#include <vector>
#include <dxgi1_4.h>
#include <cstring>
#include <cmath>
#include <utility>

extern void Log(const char* fmt, ...);
extern "C" int GetDisableRoll();
extern "C" float GetForcedFov();
extern "C" float GetMenuFov();
extern "C" int GetMenuRectMode();
extern "C" int GetMenuMode();
extern "C" int GetSyncSequential();
extern "C" int Get3DofMovement();
extern "C" int GetAERPairGate();
extern "C" int GetAERStartEye();
extern "C" int GetAERDebugEye();
extern "C" int GetAERWarmupFrames();


static XrQuaternionf MultiplyQuat(const XrQuaternionf& a, const XrQuaternionf& b) {
    XrQuaternionf out{};
    out.x = a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y;
    out.y = a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x;
    out.z = a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w;
    out.w = a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z;
    return out;
}

static XrQuaternionf ConjugateQuat(const XrQuaternionf& q) {
    XrQuaternionf out{ -q.x, -q.y, -q.z, q.w };
    return out;
}

static XrVector3f RotateVector(const XrQuaternionf& q, const XrVector3f& v) {
    const XrQuaternionf pure{v.x, v.y, v.z, 0.0f};
    const XrQuaternionf rotated = MultiplyQuat(MultiplyQuat(q, pure), ConjugateQuat(q));
    return {rotated.x, rotated.y, rotated.z};
}

static bool WaitForQueueIdle(ID3D12CommandQueue* queue, ID3D12Fence* fence, HANDLE fenceEvent, UINT64& fenceValue) {
    if (!queue || !fence || !fenceEvent) return false;

    fenceValue++;
    if (FAILED(queue->Signal(fence, fenceValue))) return false;
    if (fence->GetCompletedValue() < fenceValue) {
        if (FAILED(fence->SetEventOnCompletion(fenceValue, fenceEvent))) return false;
        WaitForSingleObject(fenceEvent, INFINITE);
    }
    return true;
}

static bool ContainsSwapchainFormat(const std::vector<int64_t>& formats, int64_t candidate) {
    for (const int64_t format : formats) {
        if (format == candidate) {
            return true;
        }
    }
    return false;
}

static int64_t PickMonoSwapchainFormat(const std::vector<int64_t>& runtimeFormats, int64_t gameFormat) {
    if (ContainsSwapchainFormat(runtimeFormats, gameFormat)) {
        return gameFormat;
    }

    const int64_t preferredFormats[] = {
        static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM),
        static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM),
        static_cast<int64_t>(DXGI_FORMAT_R16G16B16A16_FLOAT)
    };
    for (const int64_t preferred : preferredFormats) {
        if (ContainsSwapchainFormat(runtimeFormats, preferred)) {
            return preferred;
        }
    }

    return runtimeFormats.empty() ? gameFormat : runtimeFormats[0];
}

static XrFovf ApplyForcedProjectionFov(const XrFovf& sourceFov, float width, float height) {
    const float forceFov = GetForcedFov();
    if (forceFov <= 1.0f || forceFov >= 170.0f) {
        return sourceFov;
    }

    float aspect = 1.0f;
    if (width > 1.0f && height > 1.0f) {
        aspect = width / height;
    }
    if (aspect < 0.01f) {
        aspect = 1.0f;
    }

    const float halfFovH = (forceFov * 3.1415926535f / 180.0f) * 0.5f;
    const float halfFovV = atanf(tanf(halfFovH) / aspect);
    XrFovf fov{};
    fov.angleLeft = -halfFovH;
    fov.angleRight = halfFovH;
    fov.angleDown = -halfFovV;
    fov.angleUp = halfFovV;
    return fov;
}

DWORD WINAPI OpenXRManager::FrameThreadThunk(LPVOID param) {
    return static_cast<OpenXRManager*>(param)->FrameThreadMain();
}

OpenXRManager& OpenXRManager::Get() {
    static OpenXRManager instance;
    return instance;
}

void OpenXRManager::RequestRecenter() {
    m_recenterRequested.store(true, std::memory_order_relaxed);
}

void OpenXRManager::SetMonoSubmitEnabled(bool enabled) {
    m_monoSubmitEnabled.store(enabled, std::memory_order_relaxed);
}

void OpenXRManager::SetAERSubmitEnabled(bool enabled) {
    m_aerSubmitEnabled.store(enabled, std::memory_order_relaxed);
    m_renderEyeIndex.store(GetAERStartEye() != 0 ? 1 : 0, std::memory_order_relaxed);
    m_aerWarmupRemaining = GetAERWarmupFrames();

    std::lock_guard<std::mutex> lock(m_presentMutex);
    for (CapturedEyeFrame& frame : m_capturedEyeFrames) {
        frame.serial = 0;
        frame.pairId = 0;
        frame.hasView = false;
    }
    for (CapturedEyeFrame& frame : m_pendingEyeFrames) {
        frame.serial = 0;
        frame.pairId = 0;
        frame.hasView = false;
    }
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

    if (!m_captureCmdAllocator) {
        if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_captureCmdAllocator)))) {
            Log("OpenXRManager: Failed to create AER capture command allocator\n");
            return false;
        }
    }
    if (!m_captureCmdList) {
        if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_captureCmdAllocator, nullptr, IID_PPV_ARGS(&m_captureCmdList)))) {
            Log("OpenXRManager: Failed to create AER capture command list\n");
            return false;
        }
        m_captureCmdList->Close();
    }
    if (!m_captureFence) {
        if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_captureFence)))) {
            Log("OpenXRManager: Failed to create AER capture fence\n");
            return false;
        }
    }
    if (!m_captureFenceEvent) {
        m_captureFenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!m_captureFenceEvent) {
            Log("OpenXRManager: Failed to create AER capture fence event\n");
            return false;
        }
    }

    auto framesMatch = [width, height, format](const CapturedEyeFrame* frames) {
        for (int eye = 0; eye < 2; ++eye) {
            const CapturedEyeFrame& frame = frames[eye];
            if (!frame.texture || frame.width != width || frame.height != height || frame.format != format) {
                return false;
            }
        }
        return true;
    };

    const bool texturesMatch = framesMatch(m_capturedEyeFrames) && framesMatch(m_pendingEyeFrames);
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
                frame.width = 0;
                frame.height = 0;
                frame.format = 0;
                frame.serial = 0;
                frame.pairId = 0;
                frame.pose = {};
                frame.pose.orientation.w = 1.0f;
                frame.fov = {};
                frame.hasView = false;
            }
        };
        releaseFrames(m_capturedEyeFrames);
        releaseFrames(m_pendingEyeFrames);
    }

    D3D12_HEAP_PROPERTIES heapProps{};
    heapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
    auto createFrames = [&](CapturedEyeFrame* frames, const char* label) {
        for (int eye = 0; eye < 2; ++eye) {
            CapturedEyeFrame& frame = frames[eye];
            if (FAILED(m_d3dDevice->CreateCommittedResource(
                    &heapProps,
                    D3D12_HEAP_FLAG_NONE,
                    &sourceDesc,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    nullptr,
                    IID_PPV_ARGS(&frame.texture)))) {
                Log("OpenXRManager: Failed to create AER %s texture\n", label);
                return false;
            }

            frame.width = width;
            frame.height = height;
            frame.format = format;
            frame.serial = 0;
            frame.pairId = 0;
            frame.pose = {};
            frame.pose.orientation.w = 1.0f;
            frame.fov = {};
            frame.hasView = false;
        }
        return true;
    };

    if (!createFrames(m_capturedEyeFrames, "completed") ||
        !createFrames(m_pendingEyeFrames, "pending")) {
        return false;
    }

    Log("OpenXRManager: AER capture resources ready. size=%ux%u format=%u doubleBuffered=1\n", width, height, format);
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

    if (FAILED(m_captureCmdAllocator->Reset()) || FAILED(m_captureCmdList->Reset(m_captureCmdAllocator, nullptr))) {
        Log("OpenXRManager: Failed to reset AER capture command list\n");
        return false;
    }

    D3D12_RESOURCE_BARRIER barriers[4] = {};
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

    D3D12_RESOURCE_BARRIER afterCopy[2] = {};
    afterCopy[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[0].Transition.pResource = frame->texture;
    afterCopy[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    afterCopy[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    afterCopy[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[1].Transition.pResource = backBuffer;
    afterCopy[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[1].Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    afterCopy[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    m_captureCmdList->ResourceBarrier(2, afterCopy);

    m_captureCmdList->Close();
    ID3D12CommandList* cmdLists[] = {m_captureCmdList};
    m_d3dQueue->ExecuteCommandLists(1, cmdLists);
    if (!WaitForQueueIdle(m_d3dQueue, m_captureFence, m_captureFenceEvent, m_captureFenceValue)) {
        Log("OpenXRManager: AER capture queue wait failed\n");
        return false;
    }

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        frame->serial = serial;
        frame->pairId = pairId;
    }

    if ((serial % 300) == 1) {
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

    const int64_t selectedFormat = PickMonoSwapchainFormat(runtimeFormats, static_cast<int64_t>(format));

    uint32_t viewCount = 0;
    xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, 0, &viewCount, nullptr);
    m_viewConfigViews.resize(viewCount, {XR_TYPE_VIEW_CONFIGURATION_VIEW});
    xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, viewCount, &viewCount, m_viewConfigViews.data());
    m_views.resize(viewCount, {XR_TYPE_VIEW});

    if (!m_eyeSwapchains.empty() &&
        m_eyeSwapchains[0].width == static_cast<int32_t>(width) &&
        m_eyeSwapchains[0].height == static_cast<int32_t>(height) &&
        m_cmdAllocator && m_cmdList && m_fence && m_fenceEvent) {
        return true;
    }

    for (auto& eye : m_eyeSwapchains) {
        if (eye.handle != XR_NULL_HANDLE) {
            xrDestroySwapchain(eye.handle);
            eye.handle = XR_NULL_HANDLE;
        }
    }
    m_eyeSwapchains.clear();

    if (m_fenceEvent) {
        CloseHandle(m_fenceEvent);
        m_fenceEvent = nullptr;
    }
    if (m_fence) {
        m_fence->Release();
        m_fence = nullptr;
    }
    if (m_cmdList) {
        m_cmdList->Release();
        m_cmdList = nullptr;
    }
    if (m_cmdAllocator) {
        m_cmdAllocator->Release();
        m_cmdAllocator = nullptr;
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
    }

    char formatSummary[512] = {};
    int summaryPos = sprintf_s(formatSummary, "OpenXRManager: Mono swapchain formats. game=%u selected=%lld runtime:", format, selectedFormat);
    if (summaryPos > 0) {
        for (uint32_t i = 0; i < runtimeFormatCount && summaryPos > 0 && summaryPos < static_cast<int>(sizeof(formatSummary) - 32); ++i) {
            summaryPos += sprintf_s(formatSummary + summaryPos, sizeof(formatSummary) - summaryPos, " %lld", runtimeFormats[i]);
        }
        Log("%s\n", formatSummary);
    }

    if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_cmdAllocator)))) {
        Log("OpenXRManager: Failed to create mono command allocator\n");
        return false;
    }
    if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_cmdAllocator, nullptr, IID_PPV_ARGS(&m_cmdList)))) {
        Log("OpenXRManager: Failed to create mono command list\n");
        return false;
    }
    m_cmdList->Close();

    if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_fence)))) {
        Log("OpenXRManager: Failed to create mono fence\n");
        return false;
    }
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

bool OpenXRManager::Init() {
    if (m_initialized) return true;

    Log("OpenXRManager: Initializing...\n");

    // Extensions we need
    std::vector<const char*> extensions = {
        XR_KHR_D3D12_ENABLE_EXTENSION_NAME
    };

    XrInstanceCreateInfo createInfo{XR_TYPE_INSTANCE_CREATE_INFO};
    createInfo.applicationInfo.apiVersion = XR_CURRENT_API_VERSION;
    strcpy_s(createInfo.applicationInfo.applicationName, "CyberpunkVRPort");
    createInfo.enabledExtensionCount = static_cast<uint32_t>(extensions.size());
    createInfo.enabledExtensionNames = extensions.data();

    XrResult res = xrCreateInstance(&createInfo, &m_instance);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to create XrInstance (res=%d)\n", res);
        return false;
    }

    XrSystemGetInfo systemInfo{XR_TYPE_SYSTEM_GET_INFO};
    systemInfo.formFactor = XR_FORM_FACTOR_HEAD_MOUNTED_DISPLAY;
    res = xrGetSystem(m_instance, &systemInfo, &m_systemId);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to get XrSystemId (res=%d)\n", res);
        xrDestroyInstance(m_instance);
        m_instance = XR_NULL_HANDLE;
        return false;
    }

    uint32_t viewCount = 0;
    if (XR_SUCCEEDED(xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, 0, &viewCount, nullptr)) && viewCount > 0) {
        m_viewConfigViews.resize(viewCount, {XR_TYPE_VIEW_CONFIGURATION_VIEW});
        xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, viewCount, &viewCount, m_viewConfigViews.data());
    }

    Log("OpenXRManager: OpenXR Initialized. SystemID=%llu\n", m_systemId);
    m_initialized = true;
    return true;
}

bool OpenXRManager::GetRecommendedRenderTargetSize(uint32_t* width, uint32_t* height) const {
    if (m_viewConfigViews.empty()) return false;
    if (width) *width = m_viewConfigViews[0].recommendedImageRectWidth;
    if (height) *height = m_viewConfigViews[0].recommendedImageRectHeight;
    return true;
}

bool OpenXRManager::InitGraphics(ID3D12Device* device, ID3D12CommandQueue* queue) {
    if (!m_initialized || m_session != XR_NULL_HANDLE) return false;

    Log("OpenXRManager: Initializing D3D12 Graphics Binding...\n");

    // Load D3D12 extension function
    PFN_xrGetD3D12GraphicsRequirementsKHR pfnGetD3D12GraphicsRequirementsKHR = nullptr;
    xrGetInstanceProcAddr(m_instance, "xrGetD3D12GraphicsRequirementsKHR", 
        (PFN_xrVoidFunction*)&pfnGetD3D12GraphicsRequirementsKHR);

    if (!pfnGetD3D12GraphicsRequirementsKHR) {
        Log("OpenXRManager: xrGetD3D12GraphicsRequirementsKHR not found!\n");
        return false;
    }

    XrGraphicsRequirementsD3D12KHR reqs{XR_TYPE_GRAPHICS_REQUIREMENTS_D3D12_KHR};
    pfnGetD3D12GraphicsRequirementsKHR(m_instance, m_systemId, &reqs);

    m_graphicsBinding.type = XR_TYPE_GRAPHICS_BINDING_D3D12_KHR;
    m_graphicsBinding.device = device;
    m_graphicsBinding.queue = queue;

    XrSessionCreateInfo sessionInfo{XR_TYPE_SESSION_CREATE_INFO};
    sessionInfo.next = &m_graphicsBinding;
    sessionInfo.systemId = m_systemId;

    XrResult res = xrCreateSession(m_instance, &sessionInfo, &m_session);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to create XrSession (res=%d)\n", res);
        return false;
    }

    m_d3dDevice = device;
    m_d3dQueue = queue;
    if (m_d3dDevice) m_d3dDevice->AddRef();
    if (m_d3dQueue) m_d3dQueue->AddRef();

    Log("OpenXRManager: Pose-only mode active until xr_mono_submit is enabled.\n");

    XrReferenceSpaceCreateInfo localSpaceInfo{XR_TYPE_REFERENCE_SPACE_CREATE_INFO};
    localSpaceInfo.referenceSpaceType = XR_REFERENCE_SPACE_TYPE_LOCAL;
    localSpaceInfo.poseInReferenceSpace.orientation.w = 1.0f;
    res = xrCreateReferenceSpace(m_session, &localSpaceInfo, &m_localSpace);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to create local space (res=%d)\n", res);
        return false;
    }

    XrReferenceSpaceCreateInfo viewSpaceInfo{XR_TYPE_REFERENCE_SPACE_CREATE_INFO};
    viewSpaceInfo.referenceSpaceType = XR_REFERENCE_SPACE_TYPE_VIEW;
    viewSpaceInfo.poseInReferenceSpace.orientation.w = 1.0f;
    res = xrCreateReferenceSpace(m_session, &viewSpaceInfo, &m_viewSpace);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to create view space (res=%d)\n", res);
        return false;
    }

    m_stopFrameThread.store(false, std::memory_order_relaxed);
    m_frameThread = CreateThread(nullptr, 0, &OpenXRManager::FrameThreadThunk, this, 0, nullptr);
    if (!m_frameThread) {
        Log("OpenXRManager: Failed to create frame thread\n");
        return false;
    }

    Log("OpenXRManager: Session created successfully.\n");
    return true;
}

bool OpenXRManager::BeginSession() {
    if (m_session == XR_NULL_HANDLE || m_sessionRunning.load(std::memory_order_relaxed)) return false;

    XrSessionBeginInfo beginInfo{XR_TYPE_SESSION_BEGIN_INFO};
    beginInfo.primaryViewConfigurationType = XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO;
    XrResult res = xrBeginSession(m_session, &beginInfo);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: xrBeginSession failed (res=%d)\n", res);
        return false;
    }

    m_sessionRunning.store(true, std::memory_order_relaxed);
    Log("OpenXRManager: Session begun.\n");
    return true;
}

void OpenXRManager::EndSession() {
    if (m_session == XR_NULL_HANDLE || !m_sessionRunning.load(std::memory_order_relaxed)) return;
    xrEndSession(m_session);
    m_sessionRunning.store(false, std::memory_order_relaxed);
    Log("OpenXRManager: Session ended.\n");
}

void OpenXRManager::PollEvents() {
    if (m_instance == XR_NULL_HANDLE) return;

    XrEventDataBuffer event{XR_TYPE_EVENT_DATA_BUFFER};
    while (xrPollEvent(m_instance, &event) == XR_SUCCESS) {
        if (event.type == XR_TYPE_EVENT_DATA_SESSION_STATE_CHANGED) {
            auto* changed = reinterpret_cast<XrEventDataSessionStateChanged*>(&event);
            m_sessionState = changed->state;
            Log("OpenXRManager: Session state -> %d\n", static_cast<int>(m_sessionState));

            if (m_sessionState == XR_SESSION_STATE_READY) {
                BeginSession();
            } else if (m_sessionState == XR_SESSION_STATE_STOPPING) {
                EndSession();
            } else if (m_sessionState == XR_SESSION_STATE_EXITING || m_sessionState == XR_SESSION_STATE_LOSS_PENDING) {
                m_stopFrameThread.store(true, std::memory_order_relaxed);
            }
        }

        event = {XR_TYPE_EVENT_DATA_BUFFER};
    }
}

DWORD OpenXRManager::FrameThreadMain() {
    Log("OpenXRManager: Frame thread started.\n");
    uint64_t monoWaitLogCounter = 0;
    uint64_t runtimeViewLogCounter = 0;

    while (!m_stopFrameThread.load(std::memory_order_relaxed)) {
        PollEvents();

        if (!m_sessionRunning.load(std::memory_order_relaxed)) {
            Sleep(10);
            continue;
        }

        XrFrameWaitInfo waitInfo{XR_TYPE_FRAME_WAIT_INFO};
        XrFrameState frameState{XR_TYPE_FRAME_STATE};
        XrResult res = xrWaitFrame(m_session, &waitInfo, &frameState);
        if (XR_FAILED(res)) {
            Sleep(10);
            continue;
        }

        XrFrameBeginInfo beginInfo{XR_TYPE_FRAME_BEGIN_INFO};
        xrBeginFrame(m_session, &beginInfo);

        uint32_t viewCountOutput = 0;
        const bool monoEnabled = m_monoSubmitEnabled.load(std::memory_order_relaxed);
        const bool menuRectActive = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
        const bool aerEnabled = monoEnabled && m_aerSubmitEnabled.load(std::memory_order_relaxed);
        const bool monoReady = monoEnabled && EnsureMonoSubmitResources() && !m_eyeSwapchains.empty();
        if (monoReady && !m_views.empty()) {
            XrViewLocateInfo viewLocateInfo{XR_TYPE_VIEW_LOCATE_INFO};
            viewLocateInfo.viewConfigurationType = XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO;
            viewLocateInfo.displayTime = frameState.predictedDisplayTime;
            viewLocateInfo.space = m_localSpace;

            XrViewState viewState{XR_TYPE_VIEW_STATE};
            std::lock_guard<std::mutex> viewLock(m_viewMutex);
            const XrResult locateRes = xrLocateViews(m_session, &viewLocateInfo, &viewState, static_cast<uint32_t>(m_views.size()), &viewCountOutput, m_views.data());
            if (XR_FAILED(locateRes)) {
                Log("OpenXRManager: xrLocateViews failed (res=%d)\n", locateRes);
                viewCountOutput = 0;
            } else if (viewCountOutput >= 2) {
                const float hfov0 = (m_views[0].fov.angleRight - m_views[0].fov.angleLeft) * (180.0f / 3.1415926535f);
                const float hfov1 = (m_views[1].fov.angleRight - m_views[1].fov.angleLeft) * (180.0f / 3.1415926535f);
                const float vfov0 = (m_views[0].fov.angleUp - m_views[0].fov.angleDown) * (180.0f / 3.1415926535f);
                const float vfov1 = (m_views[1].fov.angleUp - m_views[1].fov.angleDown) * (180.0f / 3.1415926535f);
                const float dx = m_views[1].pose.position.x - m_views[0].pose.position.x;
                const float dy = m_views[1].pose.position.y - m_views[0].pose.position.y;
                const float dz = m_views[1].pose.position.z - m_views[0].pose.position.z;
                const float ipd = sqrtf(dx * dx + dy * dy + dz * dz);

                m_runtimeHorizontalFovDeg.store((hfov0 + hfov1) * 0.5f, std::memory_order_relaxed);
                m_runtimeVerticalFovDeg.store((vfov0 + vfov1) * 0.5f, std::memory_order_relaxed);
                m_runtimeIpd.store(ipd, std::memory_order_relaxed);

                if (((++runtimeViewLogCounter % 300) == 1)) {
                    Log("OpenXRManager: Runtime view data. hfov=(%.2f, %.2f) vfov=(%.2f, %.2f) ipd=%.4f leftPos=(%.4f, %.4f, %.4f) rightPos=(%.4f, %.4f, %.4f)\n",
                        hfov0,
                        hfov1,
                        vfov0,
                        vfov1,
                        ipd,
                        m_views[0].pose.position.x,
                        m_views[0].pose.position.y,
                        m_views[0].pose.position.z,
                        m_views[1].pose.position.x,
                        m_views[1].pose.position.y,
                        m_views[1].pose.position.z);
                }
            }
        }

        XrSpaceLocation location{XR_TYPE_SPACE_LOCATION};
        res = xrLocateSpace(m_viewSpace, m_localSpace, frameState.predictedDisplayTime, &location);
        const bool headPoseLocated = XR_SUCCEEDED(res) &&
            (location.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) &&
            (location.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT);
        if (headPoseLocated) {
            if (!m_basePoseSet || m_recenterRequested.exchange(false, std::memory_order_relaxed)) {
                m_basePose = location.pose;
                m_basePoseSet = true;
                Log("OpenXRManager: Base pose captured.\n");
            }

            XrQuaternionf baseInv = ConjugateQuat(m_basePose.orientation);
            XrVector3f relPosWorld{};
            relPosWorld.x = location.pose.position.x - m_basePose.position.x;
            relPosWorld.y = location.pose.position.y - m_basePose.position.y;
            relPosWorld.z = location.pose.position.z - m_basePose.position.z;
            XrVector3f relPos = RotateVector(baseInv, relPosWorld);
            XrQuaternionf relOri = MultiplyQuat(baseInv, location.pose.orientation);

            m_posX.store(relPos.x, std::memory_order_relaxed);
            m_posY.store(relPos.y, std::memory_order_relaxed);
            m_posZ.store(relPos.z, std::memory_order_relaxed);
            m_oriX.store(relOri.x, std::memory_order_relaxed);
            m_oriY.store(relOri.y, std::memory_order_relaxed);
            m_oriZ.store(relOri.z, std::memory_order_relaxed);
            m_oriW.store(relOri.w, std::memory_order_relaxed);
            m_poseValid.store(true, std::memory_order_relaxed);
        }

        if (monoReady && viewCountOutput == m_eyeSwapchains.size()) {
            if (aerEnabled) {
                ID3D12Resource* eyeSources[2] = {};
                uint64_t eyeSerials[2] = {};
                uint64_t eyePairIds[2] = {};
                XrPosef eyePoses[2]{};
                XrFovf eyeFovs[2]{};
                bool eyeHasView[2] = {};
                {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    for (int eye = 0; eye < 2; ++eye) {
                        if (m_capturedEyeFrames[eye].texture) {
                            eyeSources[eye] = m_capturedEyeFrames[eye].texture;
                            eyeSources[eye]->AddRef();
                            eyeSerials[eye] = m_capturedEyeFrames[eye].serial;
                            eyePairIds[eye] = m_capturedEyeFrames[eye].pairId;
                            eyePoses[eye] = m_capturedEyeFrames[eye].pose;
                            eyeFovs[eye] = m_capturedEyeFrames[eye].fov;
                            eyeHasView[eye] = m_capturedEyeFrames[eye].hasView;
                        }
                    }
                }

                const uint64_t submitSerial = eyeSerials[0] > eyeSerials[1] ? eyeSerials[0] : eyeSerials[1];
                const bool completePair = GetAERPairGate() == 0 || (eyePairIds[0] != 0 && eyePairIds[0] == eyePairIds[1]);
                if (eyeSources[0] && eyeSources[1] && eyeSerials[0] != 0 && eyeSerials[1] != 0 && completePair && eyeHasView[0] && eyeHasView[1] &&
                    SUCCEEDED(m_cmdAllocator->Reset()) && SUCCEEDED(m_cmdList->Reset(m_cmdAllocator, nullptr))) {
                    bool copyReady = true;
                    bool releaseOk = true;
                    std::vector<bool> acquiredEyes(viewCountOutput, false);
                    std::vector<XrCompositionLayerProjectionView> projectionViews(viewCountOutput);
                    for (uint32_t i = 0; i < viewCountOutput; ++i) {
                        projectionViews[i] = {XR_TYPE_COMPOSITION_LAYER_PROJECTION_VIEW};
                    }

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        uint32_t sourceEye = eye;
                        const int debugEye = GetAERDebugEye();
                        if (debugEye == 1) {
                            sourceEye = 1;
                        } else if (debugEye == 2) {
                            sourceEye = 0;
                        } else if (debugEye == 3) {
                            sourceEye = eye ^ 1;
                        }

                        uint32_t imageIndex = 0;
                        XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                        const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].handle, &acquireInfo, &imageIndex);
                        if (XR_FAILED(acquireRes)) {
                            Log("OpenXRManager: xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                            copyReady = false;
                            break;
                        }
                        acquiredEyes[eye] = true;

                        XrSwapchainImageWaitInfo waitSwapchainInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                        waitSwapchainInfo.timeout = XR_INFINITE_DURATION;
                        const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].handle, &waitSwapchainInfo);
                        if (XR_FAILED(waitRes)) {
                            Log("OpenXRManager: xrWaitSwapchainImage failed for eye %u (res=%d)\n", eye, waitRes);
                            copyReady = false;
                            break;
                        }

                        ID3D12Resource* texture = m_eyeSwapchains[eye].images[imageIndex].texture;
                        if (!texture || sourceEye >= 2 || !eyeSources[sourceEye]) {
                            Log("OpenXRManager: AER source/target missing for eye %u sourceEye %u image %u\n", eye, sourceEye, imageIndex);
                            copyReady = false;
                            break;
                        }

                        D3D12_RESOURCE_BARRIER toCopyDest{};
                        toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        toCopyDest.Transition.pResource = texture;
                        toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                        toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                        toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        m_cmdList->ResourceBarrier(1, &toCopyDest);

                        m_cmdList->CopyResource(texture, eyeSources[sourceEye]);

                        D3D12_RESOURCE_BARRIER toCommon{};
                        toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        toCommon.Transition.pResource = texture;
                        toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                        toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                        toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        m_cmdList->ResourceBarrier(1, &toCommon);

                        projectionViews[eye].pose = eyePoses[eye];
                        projectionViews[eye].fov = eyeFovs[eye];
                        projectionViews[eye].subImage.swapchain = m_eyeSwapchains[eye].handle;
                        projectionViews[eye].subImage.imageRect.offset = {0, 0};
                        projectionViews[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                        projectionViews[eye].subImage.imageArrayIndex = 0;
                    }

                    m_cmdList->Close();
                    ID3D12CommandList* cmdLists[] = {m_cmdList};
                    m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                    WaitForQueueIdle(m_d3dQueue, m_fence, m_fenceEvent, m_fenceValue);

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        if (!acquiredEyes[eye]) {
                            continue;
                        }
                        XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                        const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                        if (XR_FAILED(releaseRes)) {
                            Log("OpenXRManager: xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                            releaseOk = false;
                        }
                    }

                    if (copyReady && releaseOk) {
                        XrCompositionLayerProjection layerProj{XR_TYPE_COMPOSITION_LAYER_PROJECTION};
                        XrCompositionLayerQuad layerQuad{XR_TYPE_COMPOSITION_LAYER_QUAD};
                        const XrCompositionLayerBaseHeader* layers[1] = {nullptr};

                        if (menuRectActive) {
                            layerQuad.space = m_localSpace;
                            layerQuad.eyeVisibility = XR_EYE_VISIBILITY_BOTH;
                            layerQuad.subImage = projectionViews[0].subImage;

                            XrPosef menuPose{};
                            menuPose.orientation.w = 1.0f;
                            if (headPoseLocated) {
                                menuPose = location.pose;
                            } else if (m_basePoseSet) {
                                menuPose = m_basePose;
                            }
                            XrQuaternionf baseOri = menuPose.orientation;
                             
                            // Flatten the orientation to pure yaw to keep the menu perfectly vertical
                            float fx = -2.0f * (baseOri.x * baseOri.z + baseOri.y * baseOri.w);
                            float fz = 2.0f * (baseOri.x * baseOri.x + baseOri.y * baseOri.y) - 1.0f;
                            float flatYaw = atan2f(-fx, -fz);
                            XrQuaternionf qYaw = {0.0f, sinf(flatYaw * 0.5f), 0.0f, cosf(flatYaw * 0.5f)};

                            XrQuaternionf menuOri = qYaw;

                            XrVector3f fwd = {0.0f, 0.0f, -1.5f};
                            XrVector3f rotatedFwd = RotateVector(qYaw, fwd);

                            layerQuad.pose.orientation = menuOri;
                            layerQuad.pose.position.x = menuPose.position.x + rotatedFwd.x;
                            layerQuad.pose.position.y = menuPose.position.y + rotatedFwd.y;
                            layerQuad.pose.position.z = menuPose.position.z + rotatedFwd.z;

                            float quadWidth = 2.0f * 1.5f * tanf(GetMenuFov() * 3.14159f / 180.0f * 0.5f);
                            layerQuad.size = {quadWidth, quadWidth};
                            layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerQuad);
                        } else {
                            layerProj.space = m_localSpace;
                            layerProj.viewCount = viewCountOutput;
                            layerProj.views = projectionViews.data();
                            layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerProj);
                        }

                        XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
                        endInfo.displayTime = frameState.predictedDisplayTime;
                        endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
                        endInfo.layerCount = 1;
                        endInfo.layers = layers;
                        const XrResult endRes = xrEndFrame(m_session, &endInfo);
                        if (XR_SUCCEEDED(endRes)) {
                            if (eyePairIds[0] == 1 || (eyePairIds[0] % 300) == 0) {
                                Log("OpenXRManager: AER frame submitted. left=%llu right=%llu pair=(%llu,%llu) fresh=%d shouldRender=%d debugEye=%d\n",
                                    static_cast<unsigned long long>(eyeSerials[0]),
                                    static_cast<unsigned long long>(eyeSerials[1]),
                                    static_cast<unsigned long long>(eyePairIds[0]),
                                    static_cast<unsigned long long>(eyePairIds[1]),
                                    submitSerial != m_lastSubmittedSerial ? 1 : 0,
                                    frameState.shouldRender ? 1 : 0,
                                    GetAERDebugEye());
                            }
                            m_lastSubmittedSerial = submitSerial;
                            eyeSources[0]->Release();
                            eyeSources[1]->Release();
                            continue;
                        }

                        Log("OpenXRManager: xrEndFrame AER submit failed (res=%d)\n", endRes);
                    }
                } else if (((++monoWaitLogCounter % 300) == 1)) {
                    Log("OpenXRManager: AER submit waiting. left=%llu right=%llu pair=(%llu,%llu) complete=%d leftView=%d rightView=%d views=%u shouldRender=%d\n",
                        static_cast<unsigned long long>(eyeSerials[0]),
                        static_cast<unsigned long long>(eyeSerials[1]),
                        static_cast<unsigned long long>(eyePairIds[0]),
                        static_cast<unsigned long long>(eyePairIds[1]),
                        completePair ? 1 : 0,
                        eyeHasView[0] ? 1 : 0,
                        eyeHasView[1] ? 1 : 0,
                        viewCountOutput,
                        frameState.shouldRender ? 1 : 0);
                }

                if (eyeSources[0]) {
                    eyeSources[0]->Release();
                }
                if (eyeSources[1]) {
                    eyeSources[1]->Release();
                }
            } else {
                ID3D12Resource* backBuffer = nullptr;
                uint64_t presentSerial = 0;
                uint32_t presentedWidth = 0;
                uint32_t presentedHeight = 0;
                {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    if (m_lastPresentedBackBuffer) {
                        backBuffer = m_lastPresentedBackBuffer;
                        backBuffer->AddRef();
                        presentSerial = m_lastPresentSerial;
                        presentedWidth = m_lastPresentedWidth;
                        presentedHeight = m_lastPresentedHeight;
                    }
                }

                if (backBuffer &&
                    SUCCEEDED(m_cmdAllocator->Reset()) && SUCCEEDED(m_cmdList->Reset(m_cmdAllocator, nullptr))) {
                    bool copyReady = true;
                    std::vector<bool> acquiredEyes(viewCountOutput, false);
                    std::vector<XrCompositionLayerProjectionView> projectionViews(viewCountOutput);
                    for (uint32_t i = 0; i < viewCountOutput; ++i) {
                        projectionViews[i] = {XR_TYPE_COMPOSITION_LAYER_PROJECTION_VIEW};
                    }

                    D3D12_RESOURCE_BARRIER backBufferToCopy{};
                    backBufferToCopy.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                    backBufferToCopy.Transition.pResource = backBuffer;
                    backBufferToCopy.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
                    backBufferToCopy.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                    backBufferToCopy.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                    m_cmdList->ResourceBarrier(1, &backBufferToCopy);

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        uint32_t imageIndex = 0;
                        XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                        const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].handle, &acquireInfo, &imageIndex);
                        if (XR_FAILED(acquireRes)) {
                            Log("OpenXRManager: xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                            copyReady = false;
                            break;
                        }
                        acquiredEyes[eye] = true;

                        XrSwapchainImageWaitInfo waitSwapchainInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                        waitSwapchainInfo.timeout = XR_INFINITE_DURATION;
                        const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].handle, &waitSwapchainInfo);
                        if (XR_FAILED(waitRes)) {
                            Log("OpenXRManager: xrWaitSwapchainImage failed for eye %u (res=%d)\n", eye, waitRes);
                            copyReady = false;
                            break;
                        }

                        ID3D12Resource* texture = m_eyeSwapchains[eye].images[imageIndex].texture;
                        if (!texture) {
                            Log("OpenXRManager: XR swapchain texture missing for eye %u image %u\n", eye, imageIndex);
                            copyReady = false;
                            break;
                        }

                        D3D12_RESOURCE_BARRIER toCopyDest{};
                        toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        toCopyDest.Transition.pResource = texture;
                        toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                        toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                        toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        m_cmdList->ResourceBarrier(1, &toCopyDest);

                        m_cmdList->CopyResource(texture, backBuffer);

                        D3D12_RESOURCE_BARRIER toCommon{};
                        toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        toCommon.Transition.pResource = texture;
                        toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                        toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                        toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        m_cmdList->ResourceBarrier(1, &toCommon);

                        projectionViews[eye].pose = m_views[eye].pose;
                        projectionViews[eye].fov = ApplyForcedProjectionFov(
                            m_views[eye].fov,
                            static_cast<float>(presentedWidth != 0 ? presentedWidth : m_eyeSwapchains[eye].width),
                            static_cast<float>(presentedHeight != 0 ? presentedHeight : m_eyeSwapchains[eye].height));
                        if (menuRectActive) {
                            const float menuFovDeg = GetMenuFov();
                            if (menuFovDeg > 1.0f && menuFovDeg < 170.0f) {
                                const float halfFov = (menuFovDeg * 3.1415926535f / 180.0f) * 0.5f;
                                projectionViews[eye].fov.angleLeft = -halfFov;
                                projectionViews[eye].fov.angleRight = halfFov;
                                projectionViews[eye].fov.angleDown = -halfFov;
                                projectionViews[eye].fov.angleUp = halfFov;
                            }
                        }
                        projectionViews[eye].subImage.swapchain = m_eyeSwapchains[eye].handle;
                        projectionViews[eye].subImage.imageRect.offset = {0, 0};
                        projectionViews[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                        projectionViews[eye].subImage.imageArrayIndex = 0;
                    }

                    if (!copyReady) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: xrReleaseSwapchainImage cleanup failed for eye %u (res=%d)\n", eye, releaseRes);
                            }
                        }

                        D3D12_RESOURCE_BARRIER backBufferToPresent{};
                        backBufferToPresent.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        backBufferToPresent.Transition.pResource = backBuffer;
                        backBufferToPresent.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                        backBufferToPresent.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
                        backBufferToPresent.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        m_cmdList->ResourceBarrier(1, &backBufferToPresent);
                        m_cmdList->Close();
                        ID3D12CommandList* cmdLists[] = {m_cmdList};
                        m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                        WaitForQueueIdle(m_d3dQueue, m_fence, m_fenceEvent, m_fenceValue);
                        backBuffer->Release();
                    } else {
                        D3D12_RESOURCE_BARRIER backBufferToPresent{};
                        backBufferToPresent.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        backBufferToPresent.Transition.pResource = backBuffer;
                        backBufferToPresent.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                        backBufferToPresent.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
                        backBufferToPresent.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        m_cmdList->ResourceBarrier(1, &backBufferToPresent);

                        m_cmdList->Close();
                        ID3D12CommandList* cmdLists[] = {m_cmdList};
                        m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                        WaitForQueueIdle(m_d3dQueue, m_fence, m_fenceEvent, m_fenceValue);

                        bool releaseOk = true;
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                                releaseOk = false;
                            }
                        }

                        if (releaseOk) {
                            XrCompositionLayerProjection layerProj{XR_TYPE_COMPOSITION_LAYER_PROJECTION};
                            XrCompositionLayerQuad layerQuad{XR_TYPE_COMPOSITION_LAYER_QUAD};
                            const XrCompositionLayerBaseHeader* layers[1] = {nullptr};

                            if (menuRectActive) {
                                layerQuad.space = m_localSpace;
                                layerQuad.eyeVisibility = XR_EYE_VISIBILITY_BOTH;
                                layerQuad.subImage = projectionViews[0].subImage;

                            XrPosef menuPose{};
                            menuPose.orientation.w = 1.0f;
                            if (headPoseLocated) {
                                menuPose = location.pose;
                            } else if (m_basePoseSet) {
                                menuPose = m_basePose;
                            }
                            XrQuaternionf baseOri = menuPose.orientation;
                             
                            // Flatten the orientation to pure yaw to keep the menu perfectly vertical
                            float fx = -2.0f * (baseOri.x * baseOri.z + baseOri.y * baseOri.w);
                            float fz = 2.0f * (baseOri.x * baseOri.x + baseOri.y * baseOri.y) - 1.0f;
                            float flatYaw = atan2f(-fx, -fz);
                            XrQuaternionf qYaw = {0.0f, sinf(flatYaw * 0.5f), 0.0f, cosf(flatYaw * 0.5f)};

                            XrQuaternionf menuOri = qYaw;

                            XrVector3f fwd = {0.0f, 0.0f, -1.5f};
                            XrVector3f rotatedFwd = RotateVector(qYaw, fwd);

                            layerQuad.pose.orientation = menuOri;
                            layerQuad.pose.position.x = menuPose.position.x + rotatedFwd.x;
                            layerQuad.pose.position.y = menuPose.position.y + rotatedFwd.y;
                            layerQuad.pose.position.z = menuPose.position.z + rotatedFwd.z;

                                float quadWidth = 2.0f * 1.5f * tanf(GetMenuFov() * 3.14159f / 180.0f * 0.5f);
                                layerQuad.size = {quadWidth, quadWidth};
                                layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerQuad);
                            } else {
                                layerProj.space = m_localSpace;
                                layerProj.viewCount = viewCountOutput;
                                layerProj.views = projectionViews.data();
                                layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerProj);
                            }

                            XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
                            endInfo.displayTime = frameState.predictedDisplayTime;
                            endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
                            endInfo.layerCount = 1;
                            endInfo.layers = layers;
                            const XrResult endRes = xrEndFrame(m_session, &endInfo);
                            if (XR_SUCCEEDED(endRes)) {
                                if ((presentSerial % 300) == 1) {
                                    Log("OpenXRManager: Mono frame submitted. serial=%llu fresh=%d views=%u shouldRender=%d\n",
                                        static_cast<unsigned long long>(presentSerial),
                                        presentSerial != m_lastSubmittedSerial ? 1 : 0,
                                        viewCountOutput,
                                        frameState.shouldRender ? 1 : 0);
                                }
                                m_lastSubmittedSerial = presentSerial;
                                backBuffer->Release();
                                continue;
                            }

                            Log("OpenXRManager: xrEndFrame mono submit failed (res=%d)\n", endRes);
                        }
                    }
                }

                if (backBuffer) {
                    backBuffer->Release();
                }
            }
        } else if (monoEnabled && ((++monoWaitLogCounter % 300) == 1)) {
            Log("OpenXRManager: %s submit waiting. ready=%d views=%zu shouldRender=%d\n",
                aerEnabled ? "AER" : "Mono",
                monoReady ? 1 : 0,
                m_views.size(),
                frameState.shouldRender ? 1 : 0);
        }

        XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
        endInfo.displayTime = frameState.predictedDisplayTime;
        endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
        endInfo.layerCount = 0;
        endInfo.layers = nullptr;
        xrEndFrame(m_session, &endInfo);
    }

    Log("OpenXRManager: Frame thread stopped.\n");
    return 0;
}

bool OpenXRManager::GetHeadPose(OpenXRHeadPose* out) const {
    if (!out) return false;

    const bool useSyncedPose = GetSyncSequential() != 0 && m_syncedPoseValid.load(std::memory_order_relaxed);
    out->valid = useSyncedPose ? true : m_poseValid.load(std::memory_order_relaxed);
    out->posX = useSyncedPose ? m_syncedPosX.load(std::memory_order_relaxed) : m_posX.load(std::memory_order_relaxed);
    out->posY = useSyncedPose ? m_syncedPosY.load(std::memory_order_relaxed) : m_posY.load(std::memory_order_relaxed);
    out->posZ = useSyncedPose ? m_syncedPosZ.load(std::memory_order_relaxed) : m_posZ.load(std::memory_order_relaxed);
    out->oriX = useSyncedPose ? m_syncedOriX.load(std::memory_order_relaxed) : m_oriX.load(std::memory_order_relaxed);
    out->oriY = useSyncedPose ? m_syncedOriY.load(std::memory_order_relaxed) : m_oriY.load(std::memory_order_relaxed);
    out->oriZ = useSyncedPose ? m_syncedOriZ.load(std::memory_order_relaxed) : m_oriZ.load(std::memory_order_relaxed);
    out->oriW = useSyncedPose ? m_syncedOriW.load(std::memory_order_relaxed) : m_oriW.load(std::memory_order_relaxed);
    if (Get3DofMovement() != 0) {
        out->posX = 0.0f;
        out->posY = 0.0f;
        out->posZ = 0.0f;
    }
    return out->valid;
}

void OpenXRManager::OnPresent(IDXGISwapChain* swapChain) {
    if (!swapChain) return;

    static uint64_t s_presentCount = 0;
    ++s_presentCount;
    const bool aerEnabled = m_aerSubmitEnabled.load(std::memory_order_relaxed);
    const bool syncSequential = aerEnabled && GetSyncSequential() != 0;
    const int presentEye = aerEnabled ? m_renderEyeIndex.load(std::memory_order_relaxed) : 0;
    const bool aerWarmupFrame = aerEnabled && m_aerWarmupRemaining > 0;

    auto latchSyncedSequentialPair = [this]() {
        m_syncedPoseValid.store(false, std::memory_order_relaxed);
        m_syncedPosX.store(m_posX.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedPosY.store(m_posY.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedPosZ.store(m_posZ.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriX.store(m_oriX.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriY.store(m_oriY.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriZ.store(m_oriZ.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriW.store(m_oriW.load(std::memory_order_relaxed), std::memory_order_relaxed);

        bool viewsValid = false;
        {
            std::lock_guard<std::mutex> viewLock(m_viewMutex);
            if (m_views.size() >= 2) {
                for (int eye = 0; eye < 2; ++eye) {
                    m_syncedEyePoses[eye] = m_views[eye].pose;
                    m_syncedEyeFovs[eye] = m_views[eye].fov;
                }
                viewsValid = true;
            }
        }

        m_syncedEyeViewsValid = viewsValid;
        m_syncedPoseValid.store(true, std::memory_order_relaxed);
        ++m_syncedPairId;
        if (m_syncedPairId == 1 || (m_syncedPairId % 300) == 0) {
            Log("OpenXRManager: synchronized sequential pair latched. pair=%llu views=%d 3dof=%d\n",
                static_cast<unsigned long long>(m_syncedPairId),
                viewsValid ? 1 : 0,
                Get3DofMovement() != 0 ? 1 : 0);
        }
    };

    if (syncSequential && !m_syncedPoseValid.load(std::memory_order_relaxed)) {
        latchSyncedSequentialPair();
    }
    const uint64_t presentPairId = aerEnabled ?
        (syncSequential && m_syncedPairId != 0 ? m_syncedPairId : ((s_presentCount + 1) / 2)) :
        0;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) {
        Log("OpenXRManager: Present hook could not read swapchain desc.\n");
        return;
    }

    IDXGISwapChain3* swapChain3 = nullptr;
    UINT backBufferIndex = 0;
    if (SUCCEEDED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3)))) {
        backBufferIndex = swapChain3->GetCurrentBackBufferIndex();
        swapChain3->Release();
    }

    ID3D12Resource* backBuffer = nullptr;
    D3D12_RESOURCE_DESC resourceDesc{};
    if (SUCCEEDED(swapChain->GetBuffer(backBufferIndex, IID_PPV_ARGS(&backBuffer)))) {
        resourceDesc = backBuffer->GetDesc();
    }

    XrPosef capturedPose{};
    capturedPose.orientation.w = 1.0f;
    XrFovf capturedFov{};
    bool hasCapturedView = false;
    if (aerEnabled) {
        std::lock_guard<std::mutex> viewLock(m_viewMutex);
        const bool useSyncedView = syncSequential && m_syncedEyeViewsValid && presentEye >= 0 && presentEye < 2;
        const bool useCurrentView = presentEye >= 0 && presentEye < static_cast<int>(m_views.size());
        if (useSyncedView || useCurrentView) {
            capturedPose = useSyncedView ? m_syncedEyePoses[presentEye] : m_views[presentEye].pose;
            const XrFovf sourceFov = useSyncedView ? m_syncedEyeFovs[presentEye] : m_views[presentEye].fov;
            float fovWidth = static_cast<float>(desc.BufferDesc.Width);
            float fovHeight = static_cast<float>(desc.BufferDesc.Height);
            if ((fovWidth <= 1.0f || fovHeight <= 1.0f) && resourceDesc.Width != 0 && resourceDesc.Height != 0) {
                fovWidth = static_cast<float>(resourceDesc.Width);
                fovHeight = static_cast<float>(resourceDesc.Height);
            }
            capturedFov = ApplyForcedProjectionFov(sourceFov, fovWidth, fovHeight);
            hasCapturedView = true;
        }
    }

    bool aerCaptureOk = false;
    if (aerEnabled && backBuffer && !aerWarmupFrame) {
        aerCaptureOk = CapturePresentedFrame(backBuffer, resourceDesc, presentEye, s_presentCount, presentPairId);
        if (!aerCaptureOk) {
            Log("OpenXRManager: AER capture failed for eye %d serial=%llu\n", presentEye, static_cast<unsigned long long>(s_presentCount));
        }
    } else if (aerWarmupFrame && (presentPairId == 1 || (presentPairId % 300) == 0)) {
        Log("OpenXRManager: AER warmup discarded. eye=%d serial=%llu pair=%llu remaining=%d\n",
            presentEye,
            static_cast<unsigned long long>(s_presentCount),
            static_cast<unsigned long long>(presentPairId),
            m_aerWarmupRemaining);
    }

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        if (m_lastPresentedBackBuffer) {
            m_lastPresentedBackBuffer->Release();
            m_lastPresentedBackBuffer = nullptr;
        }

        m_lastPresentedBackBuffer = backBuffer;
        m_lastPresentedWidth = desc.BufferDesc.Width;
        m_lastPresentedHeight = desc.BufferDesc.Height;
        m_lastPresentedFormat = static_cast<uint32_t>(desc.BufferDesc.Format);
        m_lastPresentedBufferIndex = backBufferIndex;
        m_lastPresentSerial = s_presentCount;
        if (aerEnabled && aerCaptureOk && presentEye >= 0 && presentEye < 2) {
            m_pendingEyeFrames[presentEye].pose = capturedPose;
            m_pendingEyeFrames[presentEye].fov = capturedFov;
            m_pendingEyeFrames[presentEye].hasView = hasCapturedView;

            const bool pairReady =
                m_pendingEyeFrames[0].pairId == presentPairId &&
                m_pendingEyeFrames[1].pairId == presentPairId &&
                m_pendingEyeFrames[0].serial != 0 &&
                m_pendingEyeFrames[1].serial != 0 &&
                m_pendingEyeFrames[0].hasView &&
                m_pendingEyeFrames[1].hasView;
            if (pairReady) {
                std::swap(m_capturedEyeFrames[0], m_pendingEyeFrames[0]);
                std::swap(m_capturedEyeFrames[1], m_pendingEyeFrames[1]);
                if ((presentPairId % 300) == 0) {
                    Log("OpenXRManager: AER complete pair promoted. pair=%llu left=%llu right=%llu\n",
                        static_cast<unsigned long long>(presentPairId),
                        static_cast<unsigned long long>(m_capturedEyeFrames[0].serial),
                        static_cast<unsigned long long>(m_capturedEyeFrames[1].serial));
                }
            }
        }
    }

    if (syncSequential && presentEye == 1 && !aerWarmupFrame) {
        latchSyncedSequentialPair();
    }

    if (aerEnabled) {
        if (aerWarmupFrame) {
            --m_aerWarmupRemaining;
            m_renderEyeIndex.store(presentEye, std::memory_order_relaxed);
        } else if (presentEye == 1) {
            m_aerWarmupRemaining = GetAERWarmupFrames();
            m_renderEyeIndex.store(GetAERStartEye() != 0 ? 1 : 0, std::memory_order_relaxed);
        } else {
            m_renderEyeIndex.store(presentEye ^ 1, std::memory_order_relaxed);
        }
    }

    if ((s_presentCount % 300) != 1) return;

    Log("OpenXRManager: Present observed. hwnd=%p size=%ux%u format=%u backbufferIndex=%u resourceWidth=%llu resourceHeight=%u sessionRunning=%d aer=%d sync=%d eye=%d warmup=%d pair=%llu\n",
        desc.OutputWindow,
        desc.BufferDesc.Width,
        desc.BufferDesc.Height,
        static_cast<unsigned>(desc.BufferDesc.Format),
        backBufferIndex,
        static_cast<unsigned long long>(resourceDesc.Width),
        resourceDesc.Height,
        IsSessionRunning() ? 1 : 0,
        aerEnabled ? 1 : 0,
        syncSequential ? 1 : 0,
        presentEye,
        aerWarmupFrame ? 1 : 0,
        static_cast<unsigned long long>(presentPairId));
}

void OpenXRManager::Shutdown() {
    m_stopFrameThread.store(true, std::memory_order_relaxed);
    if (m_frameThread) {
        WaitForSingleObject(m_frameThread, 2000);
        CloseHandle(m_frameThread);
        m_frameThread = nullptr;
    }

    if (m_viewSpace != XR_NULL_HANDLE) {
        xrDestroySpace(m_viewSpace);
        m_viewSpace = XR_NULL_HANDLE;
    }
    if (m_localSpace != XR_NULL_HANDLE) {
        xrDestroySpace(m_localSpace);
        m_localSpace = XR_NULL_HANDLE;
    }

    EndSession();

    if (m_session != XR_NULL_HANDLE) {
        xrDestroySession(m_session);
        m_session = XR_NULL_HANDLE;
    }

    for (auto& eye : m_eyeSwapchains) {
        if (eye.handle != XR_NULL_HANDLE) {
            xrDestroySwapchain(eye.handle);
            eye.handle = XR_NULL_HANDLE;
        }
    }
    m_eyeSwapchains.clear();
    m_views.clear();
    m_viewConfigViews.clear();

    if (m_fenceEvent) {
        CloseHandle(m_fenceEvent);
        m_fenceEvent = nullptr;
    }
    if (m_fence) {
        m_fence->Release();
        m_fence = nullptr;
    }
    if (m_cmdList) {
        m_cmdList->Release();
        m_cmdList = nullptr;
    }
    if (m_cmdAllocator) {
        m_cmdAllocator->Release();
        m_cmdAllocator = nullptr;
    }
    if (m_captureFenceEvent) {
        CloseHandle(m_captureFenceEvent);
        m_captureFenceEvent = nullptr;
    }
    if (m_captureFence) {
        m_captureFence->Release();
        m_captureFence = nullptr;
    }
    if (m_captureCmdList) {
        m_captureCmdList->Release();
        m_captureCmdList = nullptr;
    }
    if (m_captureCmdAllocator) {
        m_captureCmdAllocator->Release();
        m_captureCmdAllocator = nullptr;
    }
    if (m_rtvHeap) {
        m_rtvHeap->Release();
        m_rtvHeap = nullptr;
    }
    if (m_lastPresentedBackBuffer) {
        m_lastPresentedBackBuffer->Release();
        m_lastPresentedBackBuffer = nullptr;
    }
    if (m_d3dQueue) {
        m_d3dQueue->Release();
        m_d3dQueue = nullptr;
    }
    if (m_d3dDevice) {
        m_d3dDevice->Release();
        m_d3dDevice = nullptr;
    }
    for (CapturedEyeFrame& frame : m_capturedEyeFrames) {
        if (frame.texture) {
            frame.texture->Release();
            frame.texture = nullptr;
        }
        frame.width = 0;
        frame.height = 0;
        frame.format = 0;
        frame.serial = 0;
        frame.pairId = 0;
        frame.pose = {};
        frame.pose.orientation.w = 1.0f;
        frame.fov = {};
        frame.hasView = false;
    }
    for (CapturedEyeFrame& frame : m_pendingEyeFrames) {
        if (frame.texture) {
            frame.texture->Release();
            frame.texture = nullptr;
        }
        frame.width = 0;
        frame.height = 0;
        frame.format = 0;
        frame.serial = 0;
        frame.pairId = 0;
        frame.pose = {};
        frame.pose.orientation.w = 1.0f;
        frame.fov = {};
        frame.hasView = false;
    }

    if (m_instance != XR_NULL_HANDLE) {
        xrDestroyInstance(m_instance);
        m_instance = XR_NULL_HANDLE;
    }
    m_initialized = false;
    m_poseValid.store(false, std::memory_order_relaxed);
    m_basePoseSet = false;
    Log("OpenXRManager: Shutdown complete.\n");
}
