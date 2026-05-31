#include "imgui_overlay.h"
#include "live_controls_ui.h"

#include <algorithm>
#include <cfloat>
#include <vector>

#include <imgui.h>
#include <imgui_impl_dx12.h>
#include <imgui_impl_win32.h>

extern void Log(const char* fmt, ...);

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

namespace {
struct FrameContext {
    ID3D12CommandAllocator* allocator = nullptr;
    ID3D12Resource* renderTarget = nullptr;
    D3D12_CPU_DESCRIPTOR_HANDLE rtv{};
};

ID3D12Device* g_device = nullptr;
ID3D12CommandQueue* g_queue = nullptr;
IDXGISwapChain3* g_swapChain3 = nullptr;
ID3D12DescriptorHeap* g_rtvHeap = nullptr;
ID3D12DescriptorHeap* g_srvHeap = nullptr;
ID3D12GraphicsCommandList* g_cmdList = nullptr;
ID3D12Fence* g_fence = nullptr;
HANDLE g_fenceEvent = nullptr;
UINT64 g_fenceValue = 0;
UINT g_rtvDescriptorSize = 0;
UINT g_frameCount = 0;
DXGI_FORMAT g_rtvFormat = DXGI_FORMAT_UNKNOWN;
std::vector<FrameContext> g_frames;

HWND g_hwnd = nullptr;
WNDPROC g_originalWndProc = nullptr;
bool g_imguiInitialized = false;
bool g_menuVisible = false;

template <typename T>
void SafeRelease(T*& value) {
    if (value) {
        value->Release();
        value = nullptr;
    }
}

bool CheckboxInt(const char* label, int* value) {
    bool checked = *value != 0;
    const bool changed = ImGui::Checkbox(label, &checked);
    if (changed) {
        *value = checked ? 1 : 0;
    }
    return changed;
}

bool SliderIntClamped(const char* label, int* value, int minValue, int maxValue) {
    int temp = *value;
    const bool changed = ImGui::SliderInt(label, &temp, minValue, maxValue);
    if (changed) {
        *value = std::clamp(temp, minValue, maxValue);
    }
    return changed;
}

bool InputIntClamped(const char* label, int* value, int minValue, int maxValue) {
    int temp = *value;
    const bool changed = ImGui::InputInt(label, &temp, 1, 64);
    if (changed) {
        *value = std::clamp(temp, minValue, maxValue);
    }
    return changed;
}

bool DrawFovControl(LiveControlsUiState& state) {
    bool changed = false;
    bool useRuntime = state.xrForceFov <= 0.0f;
    if (ImGui::Checkbox("Use OpenXR runtime projection FOV", &useRuntime)) {
        state.xrForceFov = useRuntime ? 0.0f : 112.0f;
        changed = true;
    }

    if (useRuntime) {
        ImGui::BeginDisabled();
    }
    float fov = state.xrForceFov <= 0.0f ? 112.0f : state.xrForceFov;
    if (ImGui::SliderFloat("OpenXR projection layer FOV", &fov, 80.0f, 140.0f, "%.1f deg")) {
        state.xrForceFov = fov;
        changed = true;
    }
    if (useRuntime) {
        ImGui::EndDisabled();
    }
    ImGui::TextUnformatted("This changes the OpenXR projection layer FOV, not the CP2077 camera FOV.");
    return changed;
}

void ReleaseGameMouseCapture() {
    ClipCursor(nullptr);
    ReleaseCapture();

    CURSORINFO cursorInfo{sizeof(CURSORINFO)};
    if (GetCursorInfo(&cursorInfo) && (cursorInfo.flags & CURSOR_SHOWING) == 0) {
        while (ShowCursor(TRUE) < 0) {
        }
    }
}

void UpdateImGuiMouseFromCursor(HWND hwnd, float backbufferWidth, float backbufferHeight) {
    ImGuiIO& io = ImGui::GetIO();

    RECT client{};
    POINT cursor{};
    if (hwnd && GetClientRect(hwnd, &client) && GetCursorPos(&cursor) && ScreenToClient(hwnd, &cursor)) {
        io.AddMousePosEvent(
            static_cast<float>(cursor.x),
            static_cast<float>(cursor.y));
    }

    io.AddMouseButtonEvent(0, (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0);
    io.AddMouseButtonEvent(1, (GetAsyncKeyState(VK_RBUTTON) & 0x8000) != 0);
    io.AddMouseButtonEvent(2, (GetAsyncKeyState(VK_MBUTTON) & 0x8000) != 0);
}

bool DrawLiveControls(LiveControlsUiState& state) {
    bool changed = false;

    if (ImGui::Button("Recenter HMD (F7)")) {
        RequestLiveControlsRecenter();
    }
    ImGui::SameLine();
    if (ImGui::Button("Save")) {
        SetLiveControlsUiState(&state, 1);
    }

    if (ImGui::CollapsingHeader("View / Resolution", ImGuiTreeNodeFlags_DefaultOpen)) {
        changed |= DrawFovControl(state);
        changed |= CheckboxInt("VR menu quad", &state.xrMenuRect);
        changed |= ImGui::SliderFloat("VR menu FOV", &state.xrMenuFov, 30.0f, 120.0f, "%.1f deg");
    }

    if (ImGui::CollapsingHeader("Stereo / AER", ImGuiTreeNodeFlags_DefaultOpen)) {
        const char* renderModes[] = {"Mono", "AER"};
        int renderMode = state.xrAERSubmit != 0 ? 1 : 0;
        if (ImGui::Combo("Render mode", &renderMode, renderModes, IM_ARRAYSIZE(renderModes))) {
            state.xrMonoSubmit = 1;
            state.xrAERSubmit = renderMode == 1 ? 1 : 0;
            changed = true;
        }
        changed |= CheckboxInt("AER pair gate", &state.xrAERPairGate);
        changed |= CheckboxInt("AER start right eye", &state.xrAERStartEye);
        const char* debugEyes[] = {"Normal", "Both right", "Both left", "Swap L/R"};
        int debugEye = state.xrAERDebugEye;
        if (debugEye < 0 || debugEye > 3) debugEye = 0;
        if (ImGui::Combo("AER debug eye", &debugEye, debugEyes, IM_ARRAYSIZE(debugEyes))) {
            state.xrAERDebugEye = debugEye;
            changed = true;
        }
        changed |= ImGui::SliderFloat("Motion prediction (ms)", &state.xrMotionPredictMs, 0.0f, 60.0f, "%.1f ms");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Forward-predicts the head pose by this many ms using head\n"
                              "velocity, hiding AER render-to-photon latency. 0 = off.\n"
                              "Tune up until motion feels responsive without overshoot.");
        }
        changed |= ImGui::SliderFloat("Stereo separation x", &state.xrStereoScale, 0.25f, 5.0f, "%.2fx");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Personal fine-tune on the auto IPD. 1.0 = calibrated natural\n"
                              "separation, auto-scaled to the headset's runtime IPD. Nudge\n"
                              "0.8-1.2 for taste; crank to 3-5x to exaggerate depth and make\n"
                              "the eye alternation obvious on the flat monitor for testing.");
        }
        changed |= CheckboxInt("Render-pose submit", &state.xrRenderPoseSubmit);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Submit each eye with the exact head pose its frame was rendered\n"
                              "with, so the runtime time-warps the older (1/2-rate) eye forward\n"
                              "to display time. Fixes left-eye judder on head turns. Off =\n"
                              "legacy present-time pose (both eyes share one latched pose).");
        }
        changed |= CheckboxInt("Half-rate submit (SSW/ASW)", &state.xrAERHalfRate);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Present only fresh AER pairs (1/2 rate) instead of resubmitting\n"
                              "stale ones, so the runtime engages motion smoothing (SSW/ASW)\n"
                              "to synthesize the in-between frames. REQUIRES SSW/Spacewarp\n"
                              "enabled in the runtime (Virtual Desktop / SteamVR); without it\n"
                              "the image will look like half-fps judder.");
        }
        changed |= CheckboxInt("AER V2 optical flow (WIP)", &state.xrAERV2);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Experimental optical-flow AER V2 path. Current build only\n"
                              "initializes NVIDIA Optical Flow and records the previous/current\n"
                              "per-eye frame history needed for interpolation. The synthesized\n"
                              "submit path is the next step.");
        }
    }

    if (ImGui::CollapsingHeader("Tracking / Camera")) {
        changed |= CheckboxInt("Fix Head", &state.xr3DofMovement);
        if (state.xr3DofMovement == 0) {
            changed |= ImGui::SliderFloat("Head X right", &state.xrHeadOffsetX, -0.50f, 0.50f, "%.3f m");
            changed |= ImGui::SliderFloat("Head Y forward", &state.xrHeadOffsetY, -0.50f, 0.50f, "%.3f m");
            changed |= ImGui::SliderFloat("Head Z up", &state.xrHeadOffsetZ, -0.50f, 0.50f, "%.3f m");
        }
    }

    if (ImGui::CollapsingHeader("DLSS / Debug")) {
        changed |= CheckboxInt("DLSS matrix hook", &state.xrDLSSMatrixHook);
        changed |= SliderIntClamped("DLSS slot mode", &state.xrDLSSSlotMode, 0, 8);
        changed |= SliderIntClamped("DLSS log stride", &state.xrDLSSLogStride, 0, 3000);
    }

    return changed;
}

void ReleaseRenderTargets() {
    for (FrameContext& frame : g_frames) {
        SafeRelease(frame.renderTarget);
        SafeRelease(frame.allocator);
    }
    g_frames.clear();
    SafeRelease(g_rtvHeap);
    SafeRelease(g_cmdList);
    SafeRelease(g_swapChain3);
    g_frameCount = 0;
    g_rtvFormat = DXGI_FORMAT_UNKNOWN;
}

void ShutdownOverlay() {
    ReleaseRenderTargets();
    if (g_imguiInitialized) {
        ImGui_ImplDX12_Shutdown();
        ImGui_ImplWin32_Shutdown();
        ImGui::DestroyContext();
        g_imguiInitialized = false;
    }
    SafeRelease(g_srvHeap);
    SafeRelease(g_fence);
    if (g_fenceEvent) {
        CloseHandle(g_fenceEvent);
        g_fenceEvent = nullptr;
    }
}

void WaitForOverlayGpu() {
    if (!g_queue || !g_fence || !g_fenceEvent) return;
    ++g_fenceValue;
    if (FAILED(g_queue->Signal(g_fence, g_fenceValue))) return;
    if (g_fence->GetCompletedValue() < g_fenceValue) {
        if (SUCCEEDED(g_fence->SetEventOnCompletion(g_fenceValue, g_fenceEvent))) {
            WaitForSingleObject(g_fenceEvent, INFINITE);
        }
    }
}

bool EnsureSwapchainResources(IDXGISwapChain* swapChain) {
    if (!swapChain || !g_device || !g_queue || !g_hwnd) return false;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc)) || desc.BufferCount == 0) {
        return false;
    }

    IDXGISwapChain3* swapChain3 = nullptr;
    if (FAILED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3))) || !swapChain3) {
        return false;
    }

    const bool needsResources = !g_swapChain3 || g_frameCount != desc.BufferCount || g_rtvFormat != desc.BufferDesc.Format;
    if (!needsResources) {
        swapChain3->Release();
        return true;
    }

    ReleaseRenderTargets();
    g_swapChain3 = swapChain3;
    g_frameCount = desc.BufferCount;
    g_rtvFormat = desc.BufferDesc.Format;

    D3D12_DESCRIPTOR_HEAP_DESC rtvDesc{};
    rtvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    rtvDesc.NumDescriptors = g_frameCount;
    if (FAILED(g_device->CreateDescriptorHeap(&rtvDesc, IID_PPV_ARGS(&g_rtvHeap)))) {
        ReleaseRenderTargets();
        return false;
    }
    g_rtvDescriptorSize = g_device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);

    g_frames.resize(g_frameCount);
    D3D12_CPU_DESCRIPTOR_HANDLE rtv = g_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    for (UINT i = 0; i < g_frameCount; ++i) {
        FrameContext& frame = g_frames[i];
        if (FAILED(g_device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&frame.allocator)))) {
            ReleaseRenderTargets();
            return false;
        }
        if (FAILED(swapChain->GetBuffer(i, IID_PPV_ARGS(&frame.renderTarget)))) {
            ReleaseRenderTargets();
            return false;
        }
        frame.rtv = rtv;
        g_device->CreateRenderTargetView(frame.renderTarget, nullptr, frame.rtv);
        rtv.ptr += g_rtvDescriptorSize;
    }

    if (FAILED(g_device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, g_frames[0].allocator, nullptr, IID_PPV_ARGS(&g_cmdList)))) {
        ReleaseRenderTargets();
        return false;
    }
    g_cmdList->Close();
    return true;
}

bool EnsureImGui(IDXGISwapChain* swapChain) {
    if (!swapChain || !g_device || !g_queue || !g_hwnd) return false;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) {
        return false;
    }

    if (!g_imguiInitialized) {
        D3D12_DESCRIPTOR_HEAP_DESC srvDesc{};
        srvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        srvDesc.NumDescriptors = 1;
        srvDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (FAILED(g_device->CreateDescriptorHeap(&srvDesc, IID_PPV_ARGS(&g_srvHeap)))) {
            return false;
        }

        IMGUI_CHECKVERSION();
        ImGui::CreateContext();
        ImGuiIO& io = ImGui::GetIO();
        io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
        io.MouseDrawCursor = true;
        io.IniFilename = nullptr;
        io.FontGlobalScale = 1.35f;
        ImGui::StyleColorsDark();
        ImGui::GetStyle().ScaleAllSizes(1.35f);

        if (!ImGui_ImplWin32_Init(g_hwnd)) {
            ShutdownOverlay();
            return false;
        }
        if (!ImGui_ImplDX12_Init(g_device, static_cast<int>(std::max<UINT>(desc.BufferCount, 2)), desc.BufferDesc.Format, g_srvHeap,
                g_srvHeap->GetCPUDescriptorHandleForHeapStart(), g_srvHeap->GetGPUDescriptorHandleForHeapStart())) {
            ShutdownOverlay();
            return false;
        }

        if (FAILED(g_device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&g_fence)))) {
            ShutdownOverlay();
            return false;
        }
        g_fenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!g_fenceEvent) {
            ShutdownOverlay();
            return false;
        }

        g_imguiInitialized = true;
        Log("ImGui overlay initialized. Toggle with F10 or Insert.\n");
    }

    return EnsureSwapchainResources(swapChain);
}

extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();

bool IsBlockedInputMessage(UINT msg) {
    switch (msg) {
    case WM_INPUT:
    case WM_INPUT_DEVICE_CHANGE:
    case WM_MOUSEMOVE:
    case WM_NCMOUSEMOVE:
    case WM_LBUTTONDOWN:
    case WM_LBUTTONUP:
    case WM_RBUTTONDOWN:
    case WM_RBUTTONUP:
    case WM_MBUTTONDOWN:
    case WM_MBUTTONUP:
    case WM_MOUSEWHEEL:
    case WM_MOUSEHWHEEL:
    case WM_KEYDOWN:
    case WM_KEYUP:
    case WM_SYSKEYDOWN:
    case WM_SYSKEYUP:
    case WM_CHAR:
        return true;
    default:
        return false;
    }
}

LRESULT CALLBACK OverlayWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    static int totalMsgCount = 0;
    if (totalMsgCount++ % 5000 == 0) {
        Log("OverlayWndProc: msg=%u, hwnd=%p, count=%d\n", msg, hwnd, totalMsgCount);
    }

    // 1. Scale mouse coordinates FIRST so ImGui and game receive the scaled input
    if (msg == WM_MOUSEMOVE || msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP || msg == WM_RBUTTONDOWN || msg == WM_RBUTTONUP || msg == WM_MBUTTONDOWN || msg == WM_MBUTTONUP) {
        UINT virtualWidth = GetForcedDisplayModeWidth();
        UINT virtualHeight = GetForcedDisplayModeHeight();
        if (virtualWidth > 0 && virtualHeight > 0) {
            RECT rect;
            if (GetClientRect(hwnd, &rect)) {
                int winWidth = rect.right - rect.left;
                int winHeight = rect.bottom - rect.top;
                
                int x = (short)LOWORD(lParam);
                int y = (short)HIWORD(lParam);
                int oldX = x;
                int oldY = y;
                
                if (winWidth > 0 && winHeight > 0 && (winWidth != static_cast<int>(virtualWidth) || winHeight != static_cast<int>(virtualHeight))) {
                    x = (x * static_cast<int>(virtualWidth)) / winWidth;
                    y = (y * static_cast<int>(virtualHeight)) / winHeight;
                    
                    lParam = MAKELPARAM(static_cast<WORD>(x), static_cast<WORD>(y));
                }

                static int mouseLogCount = 0;
                if (mouseLogCount++ % 100 == 0) {
                    Log("OverlayWndProc (scaled): msg=%u physical=(%d,%d) -> scaled=(%d,%d) win=%dx%d virt=%ux%u g_menuVisible=%d\n",
                        msg, oldX, oldY, x, y, winWidth, winHeight, virtualWidth, virtualHeight, g_menuVisible ? 1 : 0);
                }
            }
        }
    }

    // 2. Handle menu toggle
    if ((msg == WM_KEYUP || msg == WM_SYSKEYUP) && (wParam == VK_F10 || wParam == VK_INSERT)) {
        g_menuVisible = !g_menuVisible;
        if (g_menuVisible) {
            ReleaseGameMouseCapture();
        }
        Log("ImGui overlay %s.\n", g_menuVisible ? "shown" : "hidden");
        return 0;
    }

    // 3. Feed to ImGui if visible
    if (g_menuVisible) {
        if (g_imguiInitialized && ImGui_ImplWin32_WndProcHandler(hwnd, msg, wParam, lParam)) {
            return 1;
        }
        if (IsBlockedInputMessage(msg)) {
            return 0;
        }
    }

    return g_originalWndProc ? CallWindowProcA(g_originalWndProc, hwnd, msg, wParam, lParam) : DefWindowProcA(hwnd, msg, wParam, lParam);
}
}

void OverlaySetDeviceAndQueue(ID3D12Device* device, ID3D12CommandQueue* queue) {
    if (device == g_device && queue == g_queue) return;

    ShutdownOverlay();
    SafeRelease(g_device);
    SafeRelease(g_queue);
    if (device) {
        g_device = device;
        g_device->AddRef();
    }
    if (queue) {
        g_queue = queue;
        g_queue->AddRef();
    }
}

void OverlaySetWindow(HWND hwnd) {
    Log("OverlaySetWindow called: hwnd=%p (previous g_hwnd=%p)\n", hwnd, g_hwnd);
    if (!hwnd || hwnd == g_hwnd) return;

    if (g_hwnd && g_originalWndProc) {
        SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(g_originalWndProc));
        Log("OverlaySetWindow: Restored original WndProc on old hwnd %p\n", g_hwnd);
    }
    g_hwnd = hwnd;
    g_originalWndProc = reinterpret_cast<WNDPROC>(SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(OverlayWndProc)));
    Log("OverlaySetWindow: Subclassed hwnd %p, original WndProc=%p, new WndProc=%p\n", g_hwnd, g_originalWndProc, OverlayWndProc);
}

void OverlayRender(IDXGISwapChain* swapChain) {
    if (!g_menuVisible) return;
    ReleaseGameMouseCapture();
    if (!EnsureImGui(swapChain)) return;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) return;

    const UINT frameIndex = g_swapChain3 ? g_swapChain3->GetCurrentBackBufferIndex() : 0;
    if (frameIndex >= g_frames.size()) return;
    FrameContext& frame = g_frames[frameIndex];
    if (!frame.renderTarget || !frame.allocator || !g_cmdList) return;

    ImGui_ImplDX12_NewFrame();
    ImGui_ImplWin32_NewFrame();

    ImGuiIO& io = ImGui::GetIO();
    float backbufferWidth = static_cast<float>(desc.BufferDesc.Width);
    float backbufferHeight = static_cast<float>(desc.BufferDesc.Height);
    if ((backbufferWidth <= 1.0f || backbufferHeight <= 1.0f) && frame.renderTarget) {
        const D3D12_RESOURCE_DESC resourceDesc = frame.renderTarget->GetDesc();
        backbufferWidth = static_cast<float>(resourceDesc.Width);
        backbufferHeight = static_cast<float>(resourceDesc.Height);
    }
    if (backbufferWidth > 1.0f && backbufferHeight > 1.0f) {
        io.DisplaySize = ImVec2(backbufferWidth, backbufferHeight);
        io.DisplayFramebufferScale = ImVec2(1.0f, 1.0f);
    }
    UpdateImGuiMouseFromCursor(desc.OutputWindow, backbufferWidth, backbufferHeight);

    ImGui::NewFrame();

    LiveControlsUiState state{};
    GetLiveControlsUiState(&state);

    const ImVec2 display = ImGui::GetIO().DisplaySize;
    const ImVec2 menuSize(std::min(1000.0f, display.x * 0.58f), std::min(1180.0f, display.y * 0.64f));
    ImGui::SetNextWindowSize(menuSize, ImGuiCond_Appearing);
    ImGui::SetNextWindowPos(ImVec2((display.x - menuSize.x) * 0.5f, (display.y - menuSize.y) * 0.5f), ImGuiCond_Appearing);
    ImGui::Begin("CyberpunkVRPort Controls", &g_menuVisible, ImGuiWindowFlags_NoCollapse);
    ImGui::TextUnformatted("F10 / Insert: toggle menu");
    ImGui::Separator();
    const bool changed = DrawLiveControls(state);
    ImGui::End();

    if (changed) {
        SetLiveControlsUiState(&state, 1);
    }

    ImGui::Render();

    frame.allocator->Reset();
    g_cmdList->Reset(frame.allocator, nullptr);

    D3D12_RESOURCE_BARRIER toRt{};
    toRt.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    toRt.Transition.pResource = frame.renderTarget;
    toRt.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    toRt.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    toRt.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    g_cmdList->ResourceBarrier(1, &toRt);

    g_cmdList->OMSetRenderTargets(1, &frame.rtv, FALSE, nullptr);
    ID3D12DescriptorHeap* heaps[] = {g_srvHeap};
    g_cmdList->SetDescriptorHeaps(1, heaps);
    ImGui_ImplDX12_RenderDrawData(ImGui::GetDrawData(), g_cmdList);

    D3D12_RESOURCE_BARRIER toPresent{};
    toPresent.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    toPresent.Transition.pResource = frame.renderTarget;
    toPresent.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    toPresent.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    toPresent.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    g_cmdList->ResourceBarrier(1, &toPresent);

    g_cmdList->Close();
    ID3D12CommandList* lists[] = {g_cmdList};
    g_queue->ExecuteCommandLists(1, lists);
    WaitForOverlayGpu();
}

void OverlayInvalidateSwapchainResources() {
    ReleaseRenderTargets();
}

bool OverlayIsVisible() {
    return g_menuVisible;
}
