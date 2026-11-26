#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/DesktopMirror.hpp"

#include "Render/ColorBlit.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <mutex>
#include <vector>
#include <wrl/client.h>
#if CVR_RUST_VRIK
// The exports' tables are attached by rust/crates/cvr-render/src/export_attach.rs as the plugin loads;
// nothing of this file remains in the Rust build.
#else

// Ported from the TofuExpress fork (2026-10-16). Changes against it: only the visible crop is copied
// out of the backbuffer (about half the bytes on a tall render), and nothing at all is recorded while
// the window already has the render's aspect -- which, since the window is fitted to the render
// (2026-10-18, GetDesktopMonitorWindowRect), is always, unless the window is held at another shape.

extern void Log(const char* fmt, ...);
extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();
extern "C" UINT GetForcedWindowWidth();

namespace {
using Microsoft::WRL::ComPtr;

using GetClientRectFn = BOOL(WINAPI*)(HWND, LPRECT);
using SetWindowPosFn = BOOL(WINAPI*)(HWND, HWND, int, int, int, int, UINT);

// The user32 originals, not our IAT hooks (Hooks/OsInput.cpp): GetClientRect is hooked to report the
// virtual size, and SetWindowPos to place the game window -- both would answer about themselves.
GetClientRectFn OriginalGetClientRect() {
    static auto fn = reinterpret_cast<GetClientRectFn>(
        GetProcAddress(GetModuleHandleW(L"user32.dll"), "GetClientRect"));
    return fn;
}

SetWindowPosFn OriginalSetWindowPos() {
    static auto fn = reinterpret_cast<SetWindowPosFn>(
        GetProcAddress(GetModuleHandleW(L"user32.dll"), "SetWindowPos"));
    return fn;
}

// A window size set in the launcher that differs from the render size is the player's; leave it alone.
bool UsesAutomaticDesktopWindow() {
    const UINT windowWidth = GetForcedWindowWidth();
    const UINT renderWidth = GetForcedDisplayModeWidth();
    return windowWidth == 0 || windowWidth == renderWidth;
}

// Follows the window to whichever monitor it was moved to. Cheap when nothing changed: a handful of
// user32 reads, and SetWindowPos only on a real change.
bool UpdateWindowForCurrentMonitor(HWND hwnd) {
    if (!hwnd || !UsesAutomaticDesktopWindow() || IsIconic(hwnd)) return false;
    // Not while the player is dragging it: the drop decides the monitor, and pulling the window back
    // every frame would fight the drag.
    GUITHREADINFO gui{sizeof(gui)};
    if (GetGUIThreadInfo(GetWindowThreadProcessId(hwnd, nullptr), &gui) && (gui.flags & GUI_INMOVESIZE)) {
        return false;
    }

    RECT outer{};
    RECT monitorRect{};
    RECT targetClientRect{};
    if (!GetDesktopMonitorWindowRect(hwnd, outer, &monitorRect, &targetClientRect)) return false;

    RECT client{};
    POINT origin{};
    if (GetDesktopPhysicalClientRect(hwnd, client, origin)) {
        const int clientWidth = client.right - client.left;
        const int clientHeight = client.bottom - client.top;
        const int targetWidth = targetClientRect.right - targetClientRect.left;
        const int targetHeight = targetClientRect.bottom - targetClientRect.top;
        if (origin.x == targetClientRect.left && origin.y == targetClientRect.top &&
            clientWidth == targetWidth && clientHeight == targetHeight) {
            return true;
        }
    }

    const auto setWindowPos = OriginalSetWindowPos();
    if (!setWindowPos || !setWindowPos(hwnd, nullptr, outer.left, outer.top,
                                       outer.right - outer.left, outer.bottom - outer.top,
                                       SWP_NOZORDER | SWP_NOACTIVATE)) {
        return false;
    }
    Log("DesktopMirror: client placed at (%ld,%ld)-(%ld,%ld) on monitor (%ld,%ld)-(%ld,%ld)\n",
        targetClientRect.left, targetClientRect.top, targetClientRect.right, targetClientRect.bottom,
        monitorRect.left, monitorRect.top, monitorRect.right, monitorRect.bottom);
    return true;
}

struct FrameContext {
    ComPtr<ID3D12CommandAllocator> allocator;
    uint64_t fenceValue = 0;
};

struct DesktopMirrorState {
    std::mutex mutex;
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12GraphicsCommandList> commandList;
    ComPtr<ID3D12Resource> cropCopy;   // the visible crop only, at its own size
    UINT cropWidth = 0;
    UINT cropHeight = 0;
    ComPtr<ID3D12Fence> fence;
    HANDLE fenceEvent = nullptr;
    uint64_t nextFenceValue = 1;
    std::vector<FrameContext> frames;
    ColorBlit blit;
    UINT width = 0;
    UINT height = 0;
    DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN;
};

DesktopMirrorState g_state;

bool WaitForFenceLocked(uint64_t value) {
    if (!value || !g_state.fence || g_state.fence->GetCompletedValue() >= value) return true;
    if (!g_state.fenceEvent || FAILED(g_state.fence->SetEventOnCompletion(value, g_state.fenceEvent))) {
        return false;
    }
    return WaitForSingleObject(g_state.fenceEvent, 1000) == WAIT_OBJECT_0;
}

bool WaitForAllFramesLocked() {
    uint64_t latest = 0;
    for (const auto& frame : g_state.frames) latest = (std::max)(latest, frame.fenceValue);
    return WaitForFenceLocked(latest);
}

void ResetResourcesLocked() {
    WaitForAllFramesLocked();
    g_state.frames.clear();
    g_state.commandList.Reset();
    g_state.cropCopy.Reset();
    g_state.cropWidth = 0;
    g_state.cropHeight = 0;
    g_state.fence.Reset();
    if (g_state.fenceEvent) {
        CloseHandle(g_state.fenceEvent);
        g_state.fenceEvent = nullptr;
    }
    g_state.blit.Shutdown();
    g_state.nextFenceValue = 1;
    g_state.width = 0;
    g_state.height = 0;
    g_state.format = DXGI_FORMAT_UNKNOWN;
}

bool EnsureResourcesLocked(IDXGISwapChain3* swapChain, ID3D12Resource* backBuffer) {
    if (!swapChain || !backBuffer || !g_state.device || !g_state.queue) return false;

    DXGI_SWAP_CHAIN_DESC swapDesc{};
    if (FAILED(swapChain->GetDesc(&swapDesc)) || swapDesc.BufferCount == 0) return false;
    const D3D12_RESOURCE_DESC resourceDesc = backBuffer->GetDesc();
    const UINT width = static_cast<UINT>(resourceDesc.Width);
    const UINT height = resourceDesc.Height;
    const DXGI_FORMAT format = swapDesc.BufferDesc.Format;
    if (g_state.commandList && g_state.width == width && g_state.height == height &&
        g_state.format == format && g_state.frames.size() == swapDesc.BufferCount) {
        return true;
    }

    ResetResourcesLocked();
    g_state.width = width;
    g_state.height = height;
    g_state.format = format;

    g_state.frames.resize(swapDesc.BufferCount);
    for (auto& frame : g_state.frames) {
        if (FAILED(g_state.device->CreateCommandAllocator(
                D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&frame.allocator)))) {
            ResetResourcesLocked();
            return false;
        }
    }
    if (FAILED(g_state.device->CreateCommandList(
            0, D3D12_COMMAND_LIST_TYPE_DIRECT, g_state.frames[0].allocator.Get(), nullptr,
            IID_PPV_ARGS(&g_state.commandList))) ||
        FAILED(g_state.commandList->Close()) ||
        FAILED(g_state.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&g_state.fence)))) {
        ResetResourcesLocked();
        return false;
    }
    g_state.fenceEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!g_state.fenceEvent || !g_state.blit.EnsureInitialized(
            g_state.device.Get(), format, width, height)) {
        ResetResourcesLocked();
        return false;
    }

    Log("DesktopMirror: initialized center-crop pass %ux%u format=%u buffers=%u\n",
        width, height, static_cast<unsigned>(format), swapDesc.BufferCount);
    return true;
}

// The crop texture follows the window's aspect, so it is rebuilt only when the window changes shape.
bool EnsureCropCopyLocked(const D3D12_RESOURCE_DESC& backDesc, UINT cropWidth, UINT cropHeight) {
    if (g_state.cropCopy && g_state.cropWidth == cropWidth && g_state.cropHeight == cropHeight) return true;
    WaitForAllFramesLocked();
    g_state.cropCopy.Reset();
    g_state.cropWidth = 0;
    g_state.cropHeight = 0;

    D3D12_RESOURCE_DESC copyDesc = backDesc;
    copyDesc.Width = cropWidth;
    copyDesc.Height = cropHeight;
    copyDesc.MipLevels = 1;
    copyDesc.Flags = D3D12_RESOURCE_FLAG_NONE;
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    if (FAILED(g_state.device->CreateCommittedResource(
            &heap, D3D12_HEAP_FLAG_NONE, &copyDesc,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, nullptr,
            IID_PPV_ARGS(&g_state.cropCopy)))) {
        Log("DesktopMirror: failed to create crop copy %ux%u format=%u\n",
            cropWidth, cropHeight, static_cast<unsigned>(backDesc.Format));
        return false;
    }
    g_state.cropWidth = cropWidth;
    g_state.cropHeight = cropHeight;
    return true;
}
} // namespace

bool ComputeDesktopCoverTransform(
        UINT sourceWidth, UINT sourceHeight,
        UINT destinationWidth, UINT destinationHeight,
        DesktopCoverTransform& out) {
    out = {};
    if (!sourceWidth || !sourceHeight || !destinationWidth || !destinationHeight) return false;

    out.sourceWidth = static_cast<double>(sourceWidth);
    out.sourceHeight = static_cast<double>(sourceHeight);
    out.destinationWidth = destinationWidth;
    out.destinationHeight = destinationHeight;

    const double sourceAspect = static_cast<double>(sourceWidth) / sourceHeight;
    const double destinationAspect = static_cast<double>(destinationWidth) / destinationHeight;
    if (sourceAspect < destinationAspect) {
        out.sourceHeight = static_cast<double>(sourceWidth) / destinationAspect;
        out.sourceY = (static_cast<double>(sourceHeight) - out.sourceHeight) * 0.5;
    } else if (sourceAspect > destinationAspect) {
        out.sourceWidth = static_cast<double>(sourceHeight) * destinationAspect;
        out.sourceX = (static_cast<double>(sourceWidth) - out.sourceWidth) * 0.5;
    }
    return true;
}

bool GetDesktopPhysicalClientRect(HWND hwnd, RECT& clientRect, POINT& clientOrigin) {
    clientRect = {};
    clientOrigin = {};
    const auto getClientRect = OriginalGetClientRect();
    if (!hwnd || !getClientRect || !getClientRect(hwnd, &clientRect)) return false;
    return ClientToScreen(hwnd, &clientOrigin) != FALSE;
}

bool GetDesktopMonitorWindowRect(
        HWND hwnd, RECT& windowRect, RECT* monitorRect, RECT* targetClientRect) {
    windowRect = {};
    const HMONITOR monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    MONITORINFO mi{sizeof(mi)};
    if (!hwnd || !monitor || !GetMonitorInfoW(monitor, &mi)) return false;

    const UINT monitorWidth = static_cast<UINT>(mi.rcMonitor.right - mi.rcMonitor.left);
    const UINT monitorHeight = static_cast<UINT>(mi.rcMonitor.bottom - mi.rcMonitor.top);
    const UINT sourceWidth = GetForcedDisplayModeWidth();
    const UINT sourceHeight = GetForcedDisplayModeHeight();
    if (!monitorWidth || !monitorHeight || !sourceWidth || !sourceHeight) return false;

    // THE WHOLE FRAME, at the render's own aspect, as large as the monitor holds and never upscaled
    // (2026-10-18, no crop on the monitor). Filling the monitor's width cut a tall render
    // to its middle: 2648x2864 on 2560x1440 showed half the height. Now it is 1331x1440 in the middle
    // of the monitor, and the crop pass below finds nothing to do.
    const double scale = (std::min)({1.0, static_cast<double>(monitorWidth) / sourceWidth,
                                     static_cast<double>(monitorHeight) / sourceHeight});
    const UINT clientWidth = (std::max)(1u, static_cast<UINT>(std::lround(sourceWidth * scale)));
    const UINT clientHeight = (std::max)(1u, static_cast<UINT>(std::lround(sourceHeight * scale)));
    const LONG clientLeft = mi.rcMonitor.left +
        (static_cast<LONG>(monitorWidth) - static_cast<LONG>(clientWidth)) / 2;
    const LONG clientTop = mi.rcMonitor.top +
        (static_cast<LONG>(monitorHeight) - static_cast<LONG>(clientHeight)) / 2;
    const RECT desiredClient{
        clientLeft,
        clientTop,
        clientLeft + static_cast<LONG>(clientWidth),
        clientTop + static_cast<LONG>(clientHeight)};

    RECT outer{0, 0, static_cast<LONG>(clientWidth), static_cast<LONG>(clientHeight)};
    const DWORD style = static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_STYLE));
    const DWORD exStyle = static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_EXSTYLE));
    if (!AdjustWindowRectEx(&outer, style, GetMenu(hwnd) != nullptr, exStyle)) return false;

    windowRect.left = desiredClient.left + outer.left;
    windowRect.top = desiredClient.top + outer.top;
    windowRect.right = windowRect.left + (outer.right - outer.left);
    windowRect.bottom = windowRect.top + (outer.bottom - outer.top);
    if (monitorRect) *monitorRect = mi.rcMonitor;
    if (targetClientRect) *targetClientRect = desiredClient;
    return true;
}

void DesktopMirrorSetDeviceAndQueue(ID3D12Device* device, ID3D12CommandQueue* queue) {
    std::lock_guard<std::mutex> lock(g_state.mutex);
    if (g_state.device.Get() == device && g_state.queue.Get() == queue) return;
    ResetResourcesLocked();
    g_state.device = device;
    g_state.queue = queue;
}

void DesktopMirrorInvalidate() {
    std::lock_guard<std::mutex> lock(g_state.mutex);
    ResetResourcesLocked();
}

void DesktopMirrorRender(IDXGISwapChain* swapChain, HWND hwnd) {
    if (!swapChain || !hwnd) return;
    UpdateWindowForCurrentMonitor(hwnd);

    RECT client{};
    POINT origin{};
    if (!GetDesktopPhysicalClientRect(hwnd, client, origin)) return;
    const UINT destinationWidth = static_cast<UINT>((std::max)(0L, client.right - client.left));
    const UINT destinationHeight = static_cast<UINT>((std::max)(0L, client.bottom - client.top));
    if (!destinationWidth || !destinationHeight) return;

    ComPtr<IDXGISwapChain3> swapChain3;
    if (FAILED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3))) || !swapChain3) return;
    const UINT frameIndex = swapChain3->GetCurrentBackBufferIndex();
    ComPtr<ID3D12Resource> backBuffer;
    if (FAILED(swapChain3->GetBuffer(frameIndex, IID_PPV_ARGS(&backBuffer))) || !backBuffer) return;

    const D3D12_RESOURCE_DESC backDesc = backBuffer->GetDesc();
    const UINT backWidth = static_cast<UINT>(backDesc.Width);
    DesktopCoverTransform transform{};
    if (!ComputeDesktopCoverTransform(backWidth, backDesc.Height,
                                      destinationWidth, destinationHeight, transform)) {
        return;
    }
    // Same aspect: the stretch alone is already right. A window rounded to whole pixels is off by up
    // to half a window pixel, a source pixel or so on a halved render; that is not worth a crop.
    if (transform.sourceWidth > backWidth - 1.5 && transform.sourceHeight > backDesc.Height - 1.5) return;

    const UINT x0 = static_cast<UINT>(std::floor(transform.sourceX));
    const UINT y0 = static_cast<UINT>(std::floor(transform.sourceY));
    const UINT x1 = (std::min)(backWidth, static_cast<UINT>(std::ceil(transform.sourceX + transform.sourceWidth)));
    const UINT y1 = (std::min)(backDesc.Height, static_cast<UINT>(std::ceil(transform.sourceY + transform.sourceHeight)));
    if (x1 <= x0 || y1 <= y0) return;
    const UINT cropWidth = x1 - x0;
    const UINT cropHeight = y1 - y0;

    std::lock_guard<std::mutex> lock(g_state.mutex);
    if (!EnsureResourcesLocked(swapChain3.Get(), backBuffer.Get()) || frameIndex >= g_state.frames.size()) return;
    if (!EnsureCropCopyLocked(backDesc, cropWidth, cropHeight)) return;
    FrameContext& frame = g_state.frames[frameIndex];
    if (!WaitForFenceLocked(frame.fenceValue) || FAILED(frame.allocator->Reset()) ||
        FAILED(g_state.commandList->Reset(frame.allocator.Get(), nullptr))) {
        return;
    }

    D3D12_RESOURCE_BARRIER barriers[2]{};
    barriers[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[0].Transition.pResource = backBuffer.Get();
    barriers[0].Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    barriers[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barriers[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barriers[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[1].Transition.pResource = g_state.cropCopy.Get();
    barriers[1].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
    barriers[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    g_state.commandList->ResourceBarrier(2, barriers);

    D3D12_TEXTURE_COPY_LOCATION dst{};
    dst.pResource = g_state.cropCopy.Get();
    dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION src{};
    src.pResource = backBuffer.Get();
    src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    const D3D12_BOX box{x0, y0, 0, x1, y1, 1};
    g_state.commandList->CopyTextureRegion(&dst, 0, 0, 0, &src, &box);

    barriers[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barriers[0].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    std::swap(barriers[1].Transition.StateBefore, barriers[1].Transition.StateAfter);
    g_state.commandList->ResourceBarrier(2, barriers);

    // The crop's exact (sub-pixel) rectangle inside the whole-pixel copy.
    const float u0 = static_cast<float>((transform.sourceX - x0) / cropWidth);
    const float v0 = static_cast<float>((transform.sourceY - y0) / cropHeight);
    const float u1 = static_cast<float>((transform.sourceX + transform.sourceWidth - x0) / cropWidth);
    const float v1 = static_cast<float>((transform.sourceY + transform.sourceHeight - y0) / cropHeight);
    const bool blitted = g_state.blit.RecordBlit(
        g_state.commandList.Get(), g_state.cropCopy.Get(), backBuffer.Get(), u0, v0, u1, v1);

    D3D12_RESOURCE_BARRIER toPresent{};
    toPresent.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    toPresent.Transition.pResource = backBuffer.Get();
    toPresent.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    toPresent.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    toPresent.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    g_state.commandList->ResourceBarrier(1, &toPresent);
    // Submitted even when the blit failed: the barriers above must still reach the queue, or the
    // backbuffer is left in COPY_SOURCE for Present.
    if (FAILED(g_state.commandList->Close())) return;

    ID3D12CommandList* lists[] = {g_state.commandList.Get()};
    g_state.queue->ExecuteCommandLists(1, lists);
    const uint64_t fenceValue = g_state.nextFenceValue++;
    if (SUCCEEDED(g_state.queue->Signal(g_state.fence.Get(), fenceValue))) {
        frame.fenceValue = fenceValue;
    }
    if (!blitted) {
        static bool logged = false;
        if (!logged) { logged = true; Log("DesktopMirror: crop blit failed; the desktop shows the uncropped frame\n"); }
    }
}
#endif
#endif
