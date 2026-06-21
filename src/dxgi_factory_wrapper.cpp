#include "dxgi_factory_wrapper.h"
#include "imgui_overlay.h"
#include "ngx_hook.h"
#include <cstdint>
#include <cstdio>
#include <unordered_map>
#include <unordered_set>
#include <mutex>
#include <atomic>
#include <wrl.h>

extern void Log(const char* fmt, ...);
extern volatile int g_verboseLog; // gate per-frame spam (ClipCursor / depth-diag)
extern "C" void PrepareStartupLiveControls();
extern "C" UINT GetForcedSwapchainWidth();
extern "C" UINT GetForcedSwapchainHeight();
extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();
extern "C" UINT GetForcedWindowWidth();
extern "C" UINT GetForcedWindowHeight();
extern "C" int GetMenuMode();

namespace {
using PresentFn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
using EnumOutputsFn = HRESULT(STDMETHODCALLTYPE*)(IDXGIAdapter*, UINT, IDXGIOutput**);
using GetDisplayModeListFn = HRESULT(STDMETHODCALLTYPE*)(IDXGIOutput*, DXGI_FORMAT, UINT, UINT*, DXGI_MODE_DESC*);
using GetDisplayModeList1Fn = HRESULT(STDMETHODCALLTYPE*)(IDXGIOutput1*, DXGI_FORMAT, UINT, UINT*, DXGI_MODE_DESC1*);
using SetFullscreenStateFn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain*, BOOL, IDXGIOutput*);
using ResizeBuffersFn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT, UINT, DXGI_FORMAT, UINT);
using ResizeBuffers1Fn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain3*, UINT, UINT, UINT, DXGI_FORMAT, UINT, const UINT*, IUnknown* const*);

std::unordered_map<uintptr_t, void*> g_originalVtableMethods;
std::mutex g_vtableMutex;
std::mutex g_dredMutex;
Microsoft::WRL::ComPtr<ID3D12Device> g_dredDevice;
bool g_dredDumped = false;
bool g_cursorClipped = false;

// [ECL-DIAG] Temporary tearing diagnostic. Hypothesis: the swapchain backbuffer
// is rendered on a command queue different from the present queue (m_d3dQueue),
// so our capture copy (issued on the present queue) races the game's render ->
// torn source frame. If the present queue receives ~0% of the game's command
// lists while other queues receive the bulk, the hypothesis is confirmed.
using ExecuteCommandListsFn = void(STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
std::atomic<void*> g_presentQueue{nullptr};
std::atomic<uint64_t> g_eclTotalCalls{0};
std::atomic<uint64_t> g_eclTotalLists{0};
std::atomic<uint64_t> g_eclPresentCalls{0};
std::atomic<uint64_t> g_eclPresentLists{0};
std::atomic<uint32_t> g_eclDistinctQueues{0};
std::atomic<void*> g_eclSeenQueues[16];

const char* WideToUtf8(const wchar_t* value, char* buffer, size_t bufferSize) {
    if (!value) {
        return "<unnamed>";
    }
    if (!buffer || bufferSize == 0) {
        return "<invalid-buffer>";
    }
    const int written = WideCharToMultiByte(CP_UTF8, 0, value, -1, buffer, static_cast<int>(bufferSize), nullptr, nullptr);
    return written > 0 ? buffer : "<wide-conversion-failed>";
}

const char* DebugName(const char* ansiName, const wchar_t* wideName, char* buffer, size_t bufferSize) {
    if (ansiName && ansiName[0] != '\0') {
        return ansiName;
    }
    return WideToUtf8(wideName, buffer, bufferSize);
}

void RememberDredDevice(ID3D12Device* device) {
    if (!device) {
        return;
    }
    std::lock_guard<std::mutex> lock(g_dredMutex);
    if (g_dredDevice.Get() != device) {
        g_dredDevice = device;
        g_dredDumped = false;
        Log("[DRED] Tracking D3D12 device %p\n", device);
    }
}

void RememberDredDeviceFromSwapChain(IDXGISwapChain* swapChain) {
    if (!swapChain) {
        return;
    }

    Microsoft::WRL::ComPtr<ID3D12Device> device;
    HRESULT hr = swapChain->GetDevice(IID_PPV_ARGS(&device));
    if (FAILED(hr)) {
        Microsoft::WRL::ComPtr<ID3D12Resource> backBuffer;
        hr = swapChain->GetBuffer(0, IID_PPV_ARGS(&backBuffer));
        if (SUCCEEDED(hr) && backBuffer) {
            hr = backBuffer->GetDevice(IID_PPV_ARGS(&device));
        }
    }

    if (SUCCEEDED(hr) && device) {
        RememberDredDevice(device.Get());
    }
}

// DRED auto-breadcrumbs + page-fault reporting must be enabled BEFORE
// D3D12CreateDevice runs; otherwise GetAutoBreadcrumbsOutput/
// GetPageFaultAllocationOutput return empty data on device-removed and our
// DumpDredOnce() logs nothing useful. Call from each CreateDXGIFactory* entry
// — those run before any D3D12 device is created.
void EnableDredOnce() {
    static std::once_flag s_flag;
    std::call_once(s_flag, []() {
        HMODULE d3d12 = LoadLibraryA("d3d12.dll");
        if (!d3d12) {
            Log("[DRED] LoadLibrary(d3d12.dll) failed; DRED not enabled\n");
            return;
        }
        using PFN_D3D12GetDebugInterface = HRESULT(WINAPI*)(REFIID, void**);
        auto fn = reinterpret_cast<PFN_D3D12GetDebugInterface>(
            GetProcAddress(d3d12, "D3D12GetDebugInterface"));
        if (!fn) {
            Log("[DRED] D3D12GetDebugInterface export missing; DRED not enabled\n");
            return;
        }

        // Prefer Settings1 (Windows 10 2004+) — gives BreadcrumbContext for
        // richer DRED dumps; fall back to base Settings on older Windows.
        Microsoft::WRL::ComPtr<ID3D12DeviceRemovedExtendedDataSettings1> settings1;
        if (SUCCEEDED(fn(IID_PPV_ARGS(&settings1))) && settings1) {
            settings1->SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings1->SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings1->SetBreadcrumbContextEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            Log("[DRED] Enabled (settings1): AutoBreadcrumbs+PageFault+BreadcrumbContext\n");
            return;
        }
        Microsoft::WRL::ComPtr<ID3D12DeviceRemovedExtendedDataSettings> settings;
        if (SUCCEEDED(fn(IID_PPV_ARGS(&settings))) && settings) {
            settings->SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings->SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            Log("[DRED] Enabled (settings): AutoBreadcrumbs+PageFault\n");
            return;
        }
        Log("[DRED] D3D12GetDebugInterface QI for DRED settings failed; DRED not enabled\n");
    });
}

bool IsDeviceRemovedHr(HRESULT hr) {
    return hr == DXGI_ERROR_DEVICE_REMOVED ||
        hr == DXGI_ERROR_DEVICE_HUNG ||
        hr == DXGI_ERROR_DEVICE_RESET;
}

void LogDredBreadcrumbs(ID3D12DeviceRemovedExtendedData* dred) {
    D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT breadcrumbs{};
    const HRESULT hr = dred->GetAutoBreadcrumbsOutput(&breadcrumbs);
    if (FAILED(hr)) {
        Log("[DRED] GetAutoBreadcrumbsOutput failed: hr=0x%08X\n", static_cast<unsigned>(hr));
        return;
    }

    unsigned nodeIndex = 0;
    for (const D3D12_AUTO_BREADCRUMB_NODE* node = breadcrumbs.pHeadAutoBreadcrumbNode;
         node != nullptr && nodeIndex < 64;
         node = node->pNext, ++nodeIndex) {
        char listName[256]{};
        char queueName[256]{};
        const UINT last = node->pLastBreadcrumbValue ? *node->pLastBreadcrumbValue : UINT_MAX;
        const unsigned op = (node->pCommandHistory && last < node->BreadcrumbCount)
            ? static_cast<unsigned>(node->pCommandHistory[last])
            : UINT_MAX;
        Log("[DRED] Breadcrumb[%u] queue=%s list=%s completed=%u/%u op=%u queuePtr=%p listPtr=%p\n",
            nodeIndex,
            DebugName(node->pCommandQueueDebugNameA, node->pCommandQueueDebugNameW, queueName, sizeof(queueName)),
            DebugName(node->pCommandListDebugNameA, node->pCommandListDebugNameW, listName, sizeof(listName)),
            last,
            node->BreadcrumbCount,
            op,
            node->pCommandQueue,
            node->pCommandList);
    }
}

void LogDredAllocations(const char* label, const D3D12_DRED_ALLOCATION_NODE* head) {
    unsigned index = 0;
    for (const D3D12_DRED_ALLOCATION_NODE* node = head;
         node != nullptr && index < 128;
         node = node->pNext, ++index) {
        char name[256]{};
        Log("[DRED] %s[%u] type=%u name=%s\n",
            label,
            index,
            static_cast<unsigned>(node->AllocationType),
            DebugName(node->ObjectNameA, node->ObjectNameW, name, sizeof(name)));
    }
}

void LogDredPageFault(ID3D12DeviceRemovedExtendedData* dred) {
    D3D12_DRED_PAGE_FAULT_OUTPUT pageFault{};
    const HRESULT hr = dred->GetPageFaultAllocationOutput(&pageFault);
    if (FAILED(hr)) {
        Log("[DRED] GetPageFaultAllocationOutput failed: hr=0x%08X\n", static_cast<unsigned>(hr));
        return;
    }

    Log("[DRED] PageFaultVA=0x%016llX\n", static_cast<unsigned long long>(pageFault.PageFaultVA));
    LogDredAllocations("ExistingAllocation", pageFault.pHeadExistingAllocationNode);
    LogDredAllocations("RecentFreedAllocation", pageFault.pHeadRecentFreedAllocationNode);
}

void DumpDredOnce(IDXGISwapChain* swapChain, HRESULT presentHr) {
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    {
        std::lock_guard<std::mutex> lock(g_dredMutex);
        if (g_dredDumped) {
            return;
        }
        device = g_dredDevice;
        g_dredDumped = true;
    }

    if (!device && swapChain) {
        swapChain->GetDevice(IID_PPV_ARGS(&device));
    }
    if (!device) {
        Log("[DRED] Present failed with hr=0x%08X but no D3D12 device is tracked\n", static_cast<unsigned>(presentHr));
        return;
    }

    const HRESULT removedReason = device->GetDeviceRemovedReason();
    Log("[DRED] Device removed detected. presentHr=0x%08X reason=0x%08X device=%p\n",
        static_cast<unsigned>(presentHr),
        static_cast<unsigned>(removedReason),
        device.Get());

    Microsoft::WRL::ComPtr<ID3D12DeviceRemovedExtendedData> dred;
    const HRESULT qiHr = device.As(&dred);
    if (FAILED(qiHr) || !dred) {
        Log("[DRED] ID3D12DeviceRemovedExtendedData unavailable: hr=0x%08X\n", static_cast<unsigned>(qiHr));
        return;
    }

    LogDredBreadcrumbs(dred.Get());
    LogDredPageFault(dred.Get());
}

static bool IsGameWindowForeground(HWND hwnd) {
    if (!hwnd || !IsWindow(hwnd)) return false;

    HWND foreground = GetForegroundWindow();
    if (!foreground) return false;
    if (foreground == hwnd || GetAncestor(foreground, GA_ROOT) == hwnd) {
        return true;
    }

    DWORD foregroundPid = 0;
    GetWindowThreadProcessId(foreground, &foregroundPid);
    return foregroundPid == GetCurrentProcessId();
}

static bool HookIAT(HMODULE hMod, const char* dllName, const char* funcName, void* newFunc, void** oldFunc) {
    if (!hMod) {
        Log("HookIAT: Invalid module handle\n");
        return false;
    }

    PIMAGE_DOS_HEADER dosHeader = reinterpret_cast<PIMAGE_DOS_HEADER>(hMod);
    if (dosHeader->e_magic != IMAGE_DOS_SIGNATURE) {
        Log("HookIAT: Invalid DOS signature for module %p\n", hMod);
        return false;
    }

    PIMAGE_NT_HEADERS ntHeaders = reinterpret_cast<PIMAGE_NT_HEADERS>(reinterpret_cast<BYTE*>(hMod) + dosHeader->e_lfanew);
    if (ntHeaders->Signature != IMAGE_NT_SIGNATURE) {
        Log("HookIAT: Invalid NT signature for module %p\n", hMod);
        return false;
    }

    DWORD importDirRVA = ntHeaders->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress;
    if (!importDirRVA) {
        Log("HookIAT: No import directory for module %p\n", hMod);
        return false;
    }

    PIMAGE_IMPORT_DESCRIPTOR importDesc = reinterpret_cast<PIMAGE_IMPORT_DESCRIPTOR>(reinterpret_cast<BYTE*>(hMod) + importDirRVA);
    bool dllFound = false;

    while (importDesc->Name) {
        const char* name = reinterpret_cast<const char*>(reinterpret_cast<BYTE*>(hMod) + importDesc->Name);
        if (_stricmp(name, dllName) == 0) {
            dllFound = true;
            PIMAGE_THUNK_DATA thunk = reinterpret_cast<PIMAGE_THUNK_DATA>(reinterpret_cast<BYTE*>(hMod) + importDesc->FirstThunk);
            PIMAGE_THUNK_DATA origThunk = reinterpret_cast<PIMAGE_THUNK_DATA>(reinterpret_cast<BYTE*>(hMod) + importDesc->OriginalFirstThunk);
            
            while (origThunk->u1.AddressOfData) {
                if (!(origThunk->u1.Ordinal & IMAGE_ORDINAL_FLAG)) {
                    PIMAGE_IMPORT_BY_NAME importByName = reinterpret_cast<PIMAGE_IMPORT_BY_NAME>(reinterpret_cast<BYTE*>(hMod) + origThunk->u1.AddressOfData);
                    if (strcmp(reinterpret_cast<const char*>(importByName->Name), funcName) == 0) {
                        DWORD oldProtect;
                        VirtualProtect(&thunk->u1.Function, sizeof(uintptr_t), PAGE_EXECUTE_READWRITE, &oldProtect);
                        if (oldFunc) {
                            *oldFunc = reinterpret_cast<void*>(thunk->u1.Function);
                        }
                        thunk->u1.Function = reinterpret_cast<uintptr_t>(newFunc);
                        VirtualProtect(&thunk->u1.Function, sizeof(uintptr_t), oldProtect, &oldProtect);
                        Log("HookIAT: Successfully hooked '%s!%s' in module %p (original=%p, hooked=%p)\n", dllName, funcName, hMod, oldFunc ? *oldFunc : nullptr, newFunc);
                        return true;
                    }
                }
                thunk++;
                origThunk++;
            }
        }
        importDesc++;
    }

    Log("HookIAT: Failed to find function '%s' in DLL '%s' for module %p%s\n", funcName, dllName, hMod, dllFound ? "" : " (DLL import descriptor not found)");
    return false;
}

using GetCursorPosFn = BOOL(WINAPI*)(LPPOINT);
using SetCursorPosFn = BOOL(WINAPI*)(int, int);
using SetWindowPosFn = BOOL(WINAPI*)(HWND, HWND, int, int, int, int, UINT);
using MoveWindowFn = BOOL(WINAPI*)(HWND, int, int, int, int, BOOL);
using GetClientRectFn = BOOL(WINAPI*)(HWND, LPRECT);
using GetWindowRectFn = BOOL(WINAPI*)(HWND, LPRECT);
using GetCursorInfoFn = BOOL(WINAPI*)(PCURSORINFO);
using ClipCursorFn = BOOL(WINAPI*)(const RECT*);
using GetSystemMetricsFn = int(WINAPI*)(int);
using GetMessagePosFn = DWORD(WINAPI*)(VOID);

static GetCursorPosFn g_origGetCursorPos = nullptr;
static SetCursorPosFn g_origSetCursorPos = nullptr;
static SetWindowPosFn g_origSetWindowPos = nullptr;
static MoveWindowFn g_origMoveWindow = nullptr;
static GetClientRectFn g_origGetClientRect = nullptr;
static GetWindowRectFn g_origGetWindowRect = nullptr;
static GetCursorInfoFn g_origGetCursorInfo = nullptr;
static ClipCursorFn g_origClipCursor = nullptr;
static GetSystemMetricsFn g_origGetSystemMetrics = nullptr;
static GetMessagePosFn g_origGetMessagePos = nullptr;
static HWND g_gameHwnd = nullptr;

static BOOL WINAPI HookedGetCursorPos(LPPOINT lpPoint) {
    BOOL res = g_origGetCursorPos ? g_origGetCursorPos(lpPoint) : FALSE;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 1000 == 0) {
        Log("HookedGetCursorPos: called %d times. lpPoint=%p, pos=(%ld,%ld) g_gameHwnd=%p\n",
            callCount, lpPoint, lpPoint ? lpPoint->x : 0, lpPoint ? lpPoint->y : 0, g_gameHwnd);
    }
    if (res && lpPoint && g_gameHwnd) {
        UINT virtualWidth = GetForcedDisplayModeWidth();
        UINT virtualHeight = GetForcedDisplayModeHeight();
        
        if (virtualWidth > 0 && virtualHeight > 0) {
            RECT rect;
            BOOL getRectRes = g_origGetClientRect ? g_origGetClientRect(g_gameHwnd, &rect) : GetClientRect(g_gameHwnd, &rect);
            if (getRectRes) {
                int winWidth = rect.right - rect.left;
                int winHeight = rect.bottom - rect.top;
                
                if (winWidth > 0 && winHeight > 0 && (static_cast<UINT>(winWidth) != virtualWidth || static_cast<UINT>(winHeight) != virtualHeight)) {
                    POINT clientPt = *lpPoint;
                    if (ScreenToClient(g_gameHwnd, &clientPt)) {
                        clientPt.x = (clientPt.x * virtualWidth) / winWidth;
                        clientPt.y = (clientPt.y * virtualHeight) / winHeight;
                        ClientToScreen(g_gameHwnd, &clientPt);
                        *lpPoint = clientPt;
                    }
                }
            }
        }
    }
    return res;
}

static BOOL WINAPI HookedSetCursorPos(int X, int Y) {
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 100 == 0) {
        Log("HookedSetCursorPos: called %d times, target=(%d,%d) g_gameHwnd=%p\n", callCount, X, Y, g_gameHwnd);
    }
    if (g_gameHwnd) {
        UINT virtualWidth = GetForcedDisplayModeWidth();
        UINT virtualHeight = GetForcedDisplayModeHeight();
        
        if (virtualWidth > 0 && virtualHeight > 0) {
            RECT rect;
            BOOL getRectRes = g_origGetClientRect ? g_origGetClientRect(g_gameHwnd, &rect) : GetClientRect(g_gameHwnd, &rect);
            if (getRectRes) {
                int winWidth = rect.right - rect.left;
                int winHeight = rect.bottom - rect.top;
                
                if (winWidth > 0 && winHeight > 0 && (static_cast<UINT>(winWidth) != virtualWidth || static_cast<UINT>(winHeight) != virtualHeight)) {
                    POINT clientPt = { X, Y };
                    if (ScreenToClient(g_gameHwnd, &clientPt)) {
                        clientPt.x = (clientPt.x * winWidth) / virtualWidth;
                        clientPt.y = (clientPt.y * winHeight) / virtualHeight;
                        ClientToScreen(g_gameHwnd, &clientPt);
                        X = clientPt.x;
                        Y = clientPt.y;
                    }
                }
            }
        }
    }
    return g_origSetCursorPos ? g_origSetCursorPos(X, Y) : FALSE;
}

static BOOL WINAPI HookedGetCursorInfo(PCURSORINFO pci) {
    BOOL res = g_origGetCursorInfo ? g_origGetCursorInfo(pci) : FALSE;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 1000 == 0) {
        Log("HookedGetCursorInfo: called %d times. ptScreenPos=(%ld,%ld)\n",
            callCount, (res && pci) ? pci->ptScreenPos.x : 0, (res && pci) ? pci->ptScreenPos.y : 0);
    }
    if (res && pci && g_gameHwnd) {
        UINT virtualWidth = GetForcedDisplayModeWidth();
        UINT virtualHeight = GetForcedDisplayModeHeight();
        
        if (virtualWidth > 0 && virtualHeight > 0) {
            RECT rect;
            BOOL getRectRes = g_origGetClientRect ? g_origGetClientRect(g_gameHwnd, &rect) : GetClientRect(g_gameHwnd, &rect);
            if (getRectRes) {
                int winWidth = rect.right - rect.left;
                int winHeight = rect.bottom - rect.top;
                
                if (winWidth > 0 && winHeight > 0 && (static_cast<UINT>(winWidth) != virtualWidth || static_cast<UINT>(winHeight) != virtualHeight)) {
                    POINT clientPt = pci->ptScreenPos;
                    if (ScreenToClient(g_gameHwnd, &clientPt)) {
                        clientPt.x = (clientPt.x * virtualWidth) / winWidth;
                        clientPt.y = (clientPt.y * virtualHeight) / winHeight;
                        ClientToScreen(g_gameHwnd, &clientPt);
                        pci->ptScreenPos = clientPt;
                    }
                }
            }
        }
    }
    return res;
}

static BOOL WINAPI HookedClipCursor(const RECT* lpRect) {
    RECT scaledRect{};
    const RECT* rectToUse = lpRect;
    
    if (g_verboseLog) Log("HookedClipCursor called: rect=%p (%ld,%ld)-(%ld,%ld) g_gameHwnd=%p\n",
        lpRect, lpRect ? lpRect->left : 0, lpRect ? lpRect->top : 0, lpRect ? lpRect->right : 0, lpRect ? lpRect->bottom : 0, g_gameHwnd);

    if (lpRect && g_gameHwnd) {
        int w = lpRect->right - lpRect->left;
        int h = lpRect->bottom - lpRect->top;
        if (w <= 1 && h <= 1) {
            bool isMenuVisible = (GetMenuMode() != 0) || OverlayIsVisible();
            if (isMenuVisible) {
                if (g_verboseLog) Log("HookedClipCursor: Ignored centering clip (%ld,%ld) because menu mode is active\n", lpRect->left, lpRect->top);
                return g_origClipCursor ? g_origClipCursor(nullptr) : ClipCursor(nullptr);
            }
        }

        UINT virtualWidth = GetForcedDisplayModeWidth();
        UINT virtualHeight = GetForcedDisplayModeHeight();
        
        if (virtualWidth > 0 && virtualHeight > 0) {
            RECT rect;
            BOOL getRectRes = g_origGetClientRect ? g_origGetClientRect(g_gameHwnd, &rect) : GetClientRect(g_gameHwnd, &rect);
            if (getRectRes) {
                int winWidth = rect.right - rect.left;
                int winHeight = rect.bottom - rect.top;
                
                if (winWidth > 0 && winHeight > 0 && (static_cast<UINT>(winWidth) != virtualWidth || static_cast<UINT>(winHeight) != virtualHeight)) {
                    int inputWidth = lpRect->right - lpRect->left;
                    int inputHeight = lpRect->bottom - lpRect->top;
                    
                    if (inputWidth > winWidth || inputHeight > winHeight || lpRect->right > winWidth || lpRect->bottom > winHeight) {
                        POINT winPos = { 0, 0 };
                        ClientToScreen(g_gameHwnd, &winPos);
                        
                        scaledRect.left = ((lpRect->left - winPos.x) * winWidth) / virtualWidth + winPos.x;
                        scaledRect.top = ((lpRect->top - winPos.y) * winHeight) / virtualHeight + winPos.y;
                        scaledRect.right = ((lpRect->right - winPos.x) * winWidth) / virtualWidth + winPos.x;
                        scaledRect.bottom = ((lpRect->bottom - winPos.y) * winHeight) / virtualHeight + winPos.y;
                        
                        rectToUse = &scaledRect;
                        if (g_verboseLog) Log("ClipCursor scaled: (%ld,%ld)-(%ld,%ld) -> (%ld,%ld)-(%ld,%ld)\n",
                            lpRect->left, lpRect->top, lpRect->right, lpRect->bottom,
                            scaledRect.left, scaledRect.top, scaledRect.right, scaledRect.bottom);
                    }
                }
            }
        }
    }
    
    return g_origClipCursor ? g_origClipCursor(rectToUse) : ClipCursor(rectToUse);
}

static BOOL WINAPI HookedGetClientRect(HWND hWnd, LPRECT lpRect) {
    BOOL res = g_origGetClientRect ? g_origGetClientRect(hWnd, lpRect) : FALSE;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 100 == 0) {
        Log("HookedGetClientRect: called %d times. hWnd=%p g_gameHwnd=%p\n", callCount, hWnd, g_gameHwnd);
    }
    if (res && lpRect) {
        if (hWnd && (hWnd == g_gameHwnd || (g_gameHwnd == nullptr && IsGameWindowForeground(hWnd)))) {
            UINT virtualWidth = GetForcedDisplayModeWidth();
            UINT virtualHeight = GetForcedDisplayModeHeight();
            if (virtualWidth > 0 && virtualHeight > 0) {
                lpRect->left = 0;
                lpRect->top = 0;
                lpRect->right = virtualWidth;
                lpRect->bottom = virtualHeight;
            }
        }
    }
    return res;
}

static BOOL WINAPI HookedGetWindowRect(HWND hWnd, LPRECT lpRect) {
    BOOL res = g_origGetWindowRect ? g_origGetWindowRect(hWnd, lpRect) : FALSE;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 100 == 0) {
        Log("HookedGetWindowRect: called %d times. hWnd=%p g_gameHwnd=%p\n", callCount, hWnd, g_gameHwnd);
    }
    if (res && lpRect) {
        if (hWnd && (hWnd == g_gameHwnd || (g_gameHwnd == nullptr && IsGameWindowForeground(hWnd)))) {
            UINT virtualWidth = GetForcedDisplayModeWidth();
            UINT virtualHeight = GetForcedDisplayModeHeight();
            if (virtualWidth > 0 && virtualHeight > 0) {
                lpRect->right = lpRect->left + virtualWidth;
                lpRect->bottom = lpRect->top + virtualHeight;
            }
        }
    }
    return res;
}

static BOOL WINAPI HookedSetWindowPos(HWND hWnd, HWND hWndInsertAfter, int X, int Y, int cx, int cy, UINT uFlags) {
    if (!(uFlags & SWP_NOSIZE)) {
        HMONITOR monitor = MonitorFromWindow(hWnd, MONITOR_DEFAULTTOPRIMARY);
        MONITORINFO mi = { sizeof(MONITORINFO) };
        if (GetMonitorInfoA(monitor, &mi)) {
            int monWidth = mi.rcMonitor.right - mi.rcMonitor.left;
            int monHeight = mi.rcMonitor.bottom - mi.rcMonitor.top;
            
            if (cx > monWidth) {
                cx = monWidth;
                if (!(uFlags & SWP_NOMOVE)) X = mi.rcMonitor.left;
            }
            if (cy > monHeight) {
                cy = monHeight;
                if (!(uFlags & SWP_NOMOVE)) Y = mi.rcMonitor.top;
            }
        }
    }
    return g_origSetWindowPos ? g_origSetWindowPos(hWnd, hWndInsertAfter, X, Y, cx, cy, uFlags) : FALSE;
}

static BOOL WINAPI HookedMoveWindow(HWND hWnd, int X, int Y, int nWidth, int nHeight, BOOL bRepaint) {
    HMONITOR monitor = MonitorFromWindow(hWnd, MONITOR_DEFAULTTOPRIMARY);
    MONITORINFO mi = { sizeof(MONITORINFO) };
    if (GetMonitorInfoA(monitor, &mi)) {
        int monWidth = mi.rcMonitor.right - mi.rcMonitor.left;
        int monHeight = mi.rcMonitor.bottom - mi.rcMonitor.top;
        
        if (nWidth > monWidth) {
            nWidth = monWidth;
            X = mi.rcMonitor.left;
        }
        if (nHeight > monHeight) {
            nHeight = monHeight;
            Y = mi.rcMonitor.top;
        }
    }
    return g_origMoveWindow ? g_origMoveWindow(hWnd, X, Y, nWidth, nHeight, bRepaint) : FALSE;
}

static int WINAPI HookedGetSystemMetrics(int nIndex) {
    int res = g_origGetSystemMetrics ? g_origGetSystemMetrics(nIndex) : 0;
    
    UINT virtualWidth = GetForcedDisplayModeWidth();
    UINT virtualHeight = GetForcedDisplayModeHeight();
    
    if (virtualWidth > 0 && virtualHeight > 0) {
        // SM_CXSCREEN = 0, SM_CYSCREEN = 1, SM_CXFULLSCREEN = 16, SM_CYFULLSCREEN = 17, SM_CXVIRTUALSCREEN = 78, SM_CYVIRTUALSCREEN = 79
        if (nIndex == SM_CXSCREEN || nIndex == SM_CXFULLSCREEN || nIndex == SM_CXVIRTUALSCREEN) {
            if (g_verboseLog) Log("GetSystemMetrics(%d) override: %d -> %d\n", nIndex, res, virtualWidth);
            return virtualWidth;
        }
        if (nIndex == SM_CYSCREEN || nIndex == SM_CYFULLSCREEN || nIndex == SM_CYVIRTUALSCREEN) {
            if (g_verboseLog) Log("GetSystemMetrics(%d) override: %d -> %d\n", nIndex, res, virtualHeight);
            return virtualHeight;
        }
    }
    return res;
}

static DWORD WINAPI HookedGetMessagePos(VOID) {
    DWORD res = g_origGetMessagePos ? g_origGetMessagePos() : 0;
    if (g_gameHwnd) {
        UINT virtualWidth = GetForcedDisplayModeWidth();
        UINT virtualHeight = GetForcedDisplayModeHeight();
        
        if (virtualWidth > 0 && virtualHeight > 0) {
            RECT rect;
            BOOL getRectRes = g_origGetClientRect ? g_origGetClientRect(g_gameHwnd, &rect) : GetClientRect(g_gameHwnd, &rect);
            if (getRectRes) {
                int winWidth = rect.right - rect.left;
                int winHeight = rect.bottom - rect.top;
                
                if (winWidth > 0 && winHeight > 0 && (static_cast<UINT>(winWidth) != virtualWidth || static_cast<UINT>(winHeight) != virtualHeight)) {
                    int x = (short)LOWORD(res);
                    int y = (short)HIWORD(res);
                    
                    POINT clientPt = { x, y };
                    if (ScreenToClient(g_gameHwnd, &clientPt)) {
                        clientPt.x = (clientPt.x * virtualWidth) / winWidth;
                        clientPt.y = (clientPt.y * virtualHeight) / winHeight;
                        ClientToScreen(g_gameHwnd, &clientPt);
                        
                        DWORD newRes = MAKELONG(static_cast<WORD>(clientPt.x), static_cast<WORD>(clientPt.y));
                        static int logCount = 0;
    if (g_verboseLog && logCount++ % 100 == 0) {
        Log("GetMessagePos override: (%d,%d) -> (%ld,%ld) win=%dx%d virt=%ux%u\n",
                                x, y, clientPt.x, clientPt.y, winWidth, winHeight, virtualWidth, virtualHeight);
                        }
                        return newRes;
                    }
                }
            }
        }
    }
    return res;
}

static void InstallOSHooks() {
    if (g_verboseLog) Log("InstallOSHooks: Installing IAT hooks for user32.dll functions...\n");
    
    HMODULE hUser32 = GetModuleHandleA("user32.dll");
    if (hUser32) {
        g_origGetCursorPos = reinterpret_cast<GetCursorPosFn>(GetProcAddress(hUser32, "GetCursorPos"));
        g_origSetCursorPos = reinterpret_cast<SetCursorPosFn>(GetProcAddress(hUser32, "SetCursorPos"));
        g_origSetWindowPos = reinterpret_cast<SetWindowPosFn>(GetProcAddress(hUser32, "SetWindowPos"));
        g_origMoveWindow = reinterpret_cast<MoveWindowFn>(GetProcAddress(hUser32, "MoveWindow"));
        g_origGetClientRect = reinterpret_cast<GetClientRectFn>(GetProcAddress(hUser32, "GetClientRect"));
        g_origGetWindowRect = reinterpret_cast<GetWindowRectFn>(GetProcAddress(hUser32, "GetWindowRect"));
        g_origGetCursorInfo = reinterpret_cast<GetCursorInfoFn>(GetProcAddress(hUser32, "GetCursorInfo"));
        g_origClipCursor = reinterpret_cast<ClipCursorFn>(GetProcAddress(hUser32, "ClipCursor"));
        g_origGetSystemMetrics = reinterpret_cast<GetSystemMetricsFn>(GetProcAddress(hUser32, "GetSystemMetrics"));
        g_origGetMessagePos = reinterpret_cast<GetMessagePosFn>(GetProcAddress(hUser32, "GetMessagePos"));
        if (g_verboseLog) Log("InstallOSHooks: Dynamically resolved all user32 original functions.\n");
    } else {
        Log("InstallOSHooks: Warning: user32.dll not found in process!\n");
    }
    
    HMODULE hMainExe = GetModuleHandleA(nullptr);
    HMODULE hCurrentDll = nullptr;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       reinterpret_cast<LPCSTR>(InstallOSHooks), &hCurrentDll);

    HMODULE modules[] = { hMainExe, hCurrentDll };
    
    for (HMODULE hMod : modules) {
        if (!hMod) continue;
        if (g_verboseLog) Log("InstallOSHooks: Hooking module %p...\n", hMod);
        HookIAT(hMod, "user32.dll", "GetCursorPos", reinterpret_cast<void*>(HookedGetCursorPos), reinterpret_cast<void**>(&g_origGetCursorPos));
        HookIAT(hMod, "user32.dll", "SetCursorPos", reinterpret_cast<void*>(HookedSetCursorPos), reinterpret_cast<void**>(&g_origSetCursorPos));
        HookIAT(hMod, "user32.dll", "SetWindowPos", reinterpret_cast<void*>(HookedSetWindowPos), reinterpret_cast<void**>(&g_origSetWindowPos));
        HookIAT(hMod, "user32.dll", "MoveWindow", reinterpret_cast<void*>(HookedMoveWindow), reinterpret_cast<void**>(&g_origMoveWindow));
        HookIAT(hMod, "user32.dll", "GetClientRect", reinterpret_cast<void*>(HookedGetClientRect), reinterpret_cast<void**>(&g_origGetClientRect));
        HookIAT(hMod, "user32.dll", "GetWindowRect", reinterpret_cast<void*>(HookedGetWindowRect), reinterpret_cast<void**>(&g_origGetWindowRect));
        HookIAT(hMod, "user32.dll", "GetCursorInfo", reinterpret_cast<void*>(HookedGetCursorInfo), reinterpret_cast<void**>(&g_origGetCursorInfo));
        HookIAT(hMod, "user32.dll", "ClipCursor", reinterpret_cast<void*>(HookedClipCursor), reinterpret_cast<void**>(&g_origClipCursor));
        HookIAT(hMod, "user32.dll", "GetSystemMetrics", reinterpret_cast<void*>(HookedGetSystemMetrics), reinterpret_cast<void**>(&g_origGetSystemMetrics));
        HookIAT(hMod, "user32.dll", "GetMessagePos", reinterpret_cast<void*>(HookedGetMessagePos), reinterpret_cast<void**>(&g_origGetMessagePos));
    }
}

static bool GetClampedClientRect(HWND hwnd, RECT* outRect) {
    if (!hwnd || !outRect || !IsWindow(hwnd) || IsIconic(hwnd)) return false;

    RECT client{};
    BOOL getRectRes = g_origGetClientRect ? g_origGetClientRect(hwnd, &client) : GetClientRect(hwnd, &client);
    if (!getRectRes) return false;

    POINT topLeft{client.left, client.top};
    POINT bottomRight{client.right, client.bottom};
    if (!ClientToScreen(hwnd, &topLeft) || !ClientToScreen(hwnd, &bottomRight)) {
        return false;
    }

    RECT screenRect{topLeft.x, topLeft.y, bottomRight.x, bottomRight.y};
    HMONITOR monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    MONITORINFO monitorInfo{sizeof(MONITORINFO)};
    if (monitor && GetMonitorInfoA(monitor, &monitorInfo)) {
        RECT clipped{};
        if (IntersectRect(&clipped, &screenRect, &monitorInfo.rcMonitor)) {
            screenRect = clipped;
        } else {
            screenRect = monitorInfo.rcMonitor;
        }
    }

    if (screenRect.right <= screenRect.left || screenRect.bottom <= screenRect.top) {
        return false;
    }

    *outRect = screenRect;
    return true;
}

static void UpdateCursorCapture(HWND hwnd) {
    if (OverlayIsVisible()) {
        if (g_cursorClipped) {
            ClipCursor(nullptr);
            g_cursorClipped = false;
        }
        return;
    }

    RECT clipRect{};
    if (IsGameWindowForeground(hwnd) && GetClampedClientRect(hwnd, &clipRect)) {
        ClipCursor(&clipRect);
        g_cursorClipped = true;
        return;
    }

    if (g_cursorClipped) {
        ClipCursor(nullptr);
        g_cursorClipped = false;
    }
}

static uintptr_t MakeVtableSlotKey(void** vtable, size_t slot) {
    return reinterpret_cast<uintptr_t>(vtable) ^ (static_cast<uintptr_t>(slot) * 0x9E3779B97F4A7C15ull);
}

template <typename T>
T GetOriginalMethod(void** vtable, size_t slot) {
    std::lock_guard<std::mutex> lock(g_vtableMutex);
    auto it = g_originalVtableMethods.find(MakeVtableSlotKey(vtable, slot));
    return it != g_originalVtableMethods.end() ? reinterpret_cast<T>(it->second) : nullptr;
}

static bool PatchVtableMethod(void** vtable, size_t slot, void* hook) {
    if (!vtable || !hook) return false;

    void* methodSlot = vtable[slot];
    if (!methodSlot) return false;

    const uintptr_t key = MakeVtableSlotKey(vtable, slot);
    std::lock_guard<std::mutex> lock(g_vtableMutex);
    if (methodSlot == hook) {
        return g_originalVtableMethods.find(key) != g_originalVtableMethods.end();
    }

    DWORD oldProtect = 0;
    if (!VirtualProtect(&vtable[slot], sizeof(void*), PAGE_EXECUTE_READWRITE, &oldProtect)) {
        return false;
    }

    if (g_originalVtableMethods.find(key) == g_originalVtableMethods.end()) {
        g_originalVtableMethods[key] = methodSlot;
    }
    vtable[slot] = hook;

    DWORD restoreProtect = 0;
    VirtualProtect(&vtable[slot], sizeof(void*), oldProtect, &restoreProtect);
    FlushInstructionCache(GetCurrentProcess(), &vtable[slot], sizeof(void*));
    return true;
}

static void RecordEclQueue(void* queue) {
    for (uint32_t i = 0; i < 16; ++i) {
        void* seen = g_eclSeenQueues[i].load(std::memory_order_relaxed);
        if (seen == queue) return;
        if (seen == nullptr) {
            void* expected = nullptr;
            if (g_eclSeenQueues[i].compare_exchange_strong(expected, queue, std::memory_order_relaxed)) {
                g_eclDistinctQueues.fetch_add(1, std::memory_order_relaxed);
                return;
            }
            if (g_eclSeenQueues[i].load(std::memory_order_relaxed) == queue) return;
        }
    }
}

// [DEPTH-DIAG] Scene-depth identification spike (groundwork for alternate-eye depth
// submission). Tearing is the flat color-only projection layer reprojected without
// depth; the fix is submitting the game's depth as XR_KHR_composition_layer_depth.
// First step: reliably PIN the game's main scene depth-stencil resource. This is
// pure observation (descriptor map + vtable read) — NO GPU copy/readback/barrier,
// so it cannot trigger the device-removed crashes seen with prior GPU passes.
using CreateDepthStencilViewFn = void(STDMETHODCALLTYPE*)(ID3D12Device*, ID3D12Resource*, const D3D12_DEPTH_STENCIL_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);
using OMSetRenderTargetsFn = void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, const D3D12_CPU_DESCRIPTOR_HANDLE*, BOOL, const D3D12_CPU_DESCRIPTOR_HANDLE*);
using ResourceBarrierFn = void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, const D3D12_RESOURCE_BARRIER*);

struct DepthDsvInfo {
    ID3D12Resource* resource;
    UINT format;
    UINT width;
    UINT height;
};
std::unordered_map<SIZE_T, DepthDsvInfo> g_dsvMap;
std::mutex g_dsvMutex;
std::atomic<uint32_t> g_dsvCount{0};

std::atomic<ID3D12Resource*> g_sceneDepthRes{nullptr};
// Guards the AddRef/Release lifecycle of the cached scene-depth resource. The
// cached slot owns exactly ONE reference so the pointer stays valid even after
// the game frees its own depth (e.g. across a save-load resource recreation).
// Without this, a later gameDepth->GetDesc() / copy dereferences a freed vtable
// and crashes with a DEP execution violation.
std::mutex g_sceneDepthRefMutex;
std::atomic<UINT> g_sceneDepthW{0};
std::atomic<UINT> g_sceneDepthH{0};
std::atomic<UINT> g_sceneDepthFmt{0};
std::atomic<uint64_t> g_sceneDepthArea{0};
std::atomic<UINT> g_sceneDepthState{0}; // D3D12_RESOURCE_STATE_COMMON until an explicit transition is observed
std::atomic<uint64_t> g_omSetRtCalls{0};
std::mutex g_cmdVtMutex;
std::unordered_set<void**> g_patchedCmdVtables;
std::atomic<uint32_t> g_distinctCmdVtables{0};

void STDMETHODCALLTYPE HookedCreateDepthStencilView(ID3D12Device* device, ID3D12Resource* resource, const D3D12_DEPTH_STENCIL_VIEW_DESC* desc, D3D12_CPU_DESCRIPTOR_HANDLE dest) {
    void** vtable = *reinterpret_cast<void***>(device);
    CreateDepthStencilViewFn originalFn = GetOriginalMethod<CreateDepthStencilViewFn>(vtable, 21);
    if (originalFn) {
        originalFn(device, resource, desc, dest);
    }
    if (resource && dest.ptr) {
        D3D12_RESOURCE_DESC rd = resource->GetDesc();
        DepthDsvInfo info{};
        info.resource = resource;
        info.format = desc ? static_cast<UINT>(desc->Format) : static_cast<UINT>(rd.Format);
        info.width = static_cast<UINT>(rd.Width);
        info.height = rd.Height;
        std::lock_guard<std::mutex> lock(g_dsvMutex);
        if (g_dsvMap.find(dest.ptr) == g_dsvMap.end()) {
            g_dsvCount.fetch_add(1, std::memory_order_relaxed);
        }
        g_dsvMap[dest.ptr] = info;
    }
}

void STDMETHODCALLTYPE HookedOMSetRenderTargets(ID3D12GraphicsCommandList* list, UINT numRTVs, const D3D12_CPU_DESCRIPTOR_HANDLE* rtvs, BOOL singleHandle, const D3D12_CPU_DESCRIPTOR_HANDLE* dsv) {
    void** vtable = *reinterpret_cast<void***>(list);
    OMSetRenderTargetsFn originalFn = GetOriginalMethod<OMSetRenderTargetsFn>(vtable, 46);
    if (originalFn) {
        originalFn(list, numRTVs, rtvs, singleHandle, dsv);
    }
    g_omSetRtCalls.fetch_add(1, std::memory_order_relaxed);
    if (dsv && dsv->ptr) {
        DepthDsvInfo info{};
        bool found = false;
        {
            std::lock_guard<std::mutex> lock(g_dsvMutex);
            auto it = g_dsvMap.find(dsv->ptr);
            if (it != g_dsvMap.end()) { info = it->second; found = true; }
        }
        if (found) {
            // Scene depth = the largest depth-stencil ever bound to OM (the main
            // geometry pass). HUD / shadow / UI depths are smaller or never bound here.
            const uint64_t area = static_cast<uint64_t>(info.width) * static_cast<uint64_t>(info.height);
            if (area > g_sceneDepthArea.load(std::memory_order_relaxed)) {
                // Take ownership of one reference on the new resource and drop the
                // reference we held on the previous one. Keeping a ref means the
                // pointer is always safe to dereference, even if the game has since
                // freed its own handle to that depth buffer.
                std::lock_guard<std::mutex> reflock(g_sceneDepthRefMutex);
                if (area > g_sceneDepthArea.load(std::memory_order_relaxed)) {
                    if (info.resource) {
                        info.resource->AddRef();
                    }
                    ID3D12Resource* old = g_sceneDepthRes.exchange(info.resource, std::memory_order_relaxed);
                    g_sceneDepthArea.store(area, std::memory_order_relaxed);
                    g_sceneDepthW.store(info.width, std::memory_order_relaxed);
                    g_sceneDepthH.store(info.height, std::memory_order_relaxed);
                    g_sceneDepthFmt.store(info.format, std::memory_order_relaxed);
                    if (old && old != info.resource) {
                        old->Release();
                    }
                }
            }
        }
    }
    // Status is logged from the ECL hook so it appears even if OM is never called.
}

void STDMETHODCALLTYPE HookedResourceBarrier(ID3D12GraphicsCommandList* list, UINT numBarriers, const D3D12_RESOURCE_BARRIER* barriers) {
    void** vtable = *reinterpret_cast<void***>(list);
    ResourceBarrierFn originalFn = GetOriginalMethod<ResourceBarrierFn>(vtable, 26);
    if (originalFn) {
        originalFn(list, numBarriers, barriers);
    }
    // Track the scene depth's current resource state so a later (Present-time) copy
    // uses the CORRECT StateBefore. A wrong transition is the classic device-removed
    // cause — we never guess; we observe the game's own explicit transitions.
    ID3D12Resource* depth = g_sceneDepthRes.load(std::memory_order_relaxed);
    if (depth && barriers) {
        for (UINT i = 0; i < numBarriers; ++i) {
            if (barriers[i].Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION &&
                barriers[i].Transition.pResource == depth) {
                g_sceneDepthState.store(static_cast<UINT>(barriers[i].Transition.StateAfter), std::memory_order_relaxed);
            }
        }
    }
}

void TryHookCmdListVtable(ID3D12CommandList* anyList) {
    if (!anyList) return;
    // Only DIRECT command lists issue OMSetRenderTargets. Patch EVERY distinct
    // graphics-list vtable we observe (do NOT latch on the first) — if the engine
    // uses more than one command-list vtable, a single-latch hook would miss the
    // one that actually records the scene pass.
    if (anyList->GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT) return;
    void** vtable = *reinterpret_cast<void***>(anyList);
    {
        std::lock_guard<std::mutex> lock(g_cmdVtMutex);
        if (g_patchedCmdVtables.count(vtable)) return;
        g_patchedCmdVtables.insert(vtable);
    }
    const uint32_t n = g_distinctCmdVtables.fetch_add(1, std::memory_order_relaxed) + 1;
    if (PatchVtableMethod(vtable, 46, reinterpret_cast<void*>(&HookedOMSetRenderTargets))) {
        if (g_verboseLog) {
            Log("[DEPTH-DIAG] Hooked OMSetRenderTargets on direct-list vtable=%p (distinctVtables=%u)\n",
                reinterpret_cast<void*>(vtable), n);
        }
    }
    // Same vtable: track depth state transitions (slot 26 = ResourceBarrier).
    PatchVtableMethod(vtable, 26, reinterpret_cast<void*>(&HookedResourceBarrier));
}

void InstallDepthCaptureHooks(ID3D12Device* device) {
    if (!device) return;
    void** vtable = *reinterpret_cast<void***>(device);
    if (PatchVtableMethod(vtable, 21, reinterpret_cast<void*>(&HookedCreateDepthStencilView))) {
        if (g_verboseLog) Log("[DEPTH-DIAG] Hooked ID3D12Device::CreateDepthStencilView (slot 21)\n");
    }
}

void TryHookQueueSignalVtable(ID3D12CommandQueue* queue); // defined below

void STDMETHODCALLTYPE HookedExecuteCommandLists(ID3D12CommandQueue* queue, UINT numLists, ID3D12CommandList* const* lists) {
    g_eclTotalCalls.fetch_add(1, std::memory_order_relaxed);
    g_eclTotalLists.fetch_add(numLists, std::memory_order_relaxed);
    RecordEclQueue(queue);
    // Hook Signal on each distinct queue vtable so we can later GPU-Wait on the
    // game's most recent fence values (cross-queue sync for depth capture).
    // ExecuteCommandLists is the perfect discovery point: every queue the game
    // uses will eventually run something through here.
    TryHookQueueSignalVtable(queue);
    if (lists) {
        for (UINT i = 0; i < numLists; ++i) {
            TryHookCmdListVtable(lists[i]);
        }
    }
    if (g_verboseLog && (g_eclTotalCalls.load(std::memory_order_relaxed) % 600) == 1) {
        Log("[DEPTH-DIAG] status: sceneDepth res=%p %ux%u fmt=%u | distinctDSV=%u omCalls=%llu cmdVtables=%u\n",
            g_sceneDepthRes.load(std::memory_order_relaxed),
            g_sceneDepthW.load(std::memory_order_relaxed),
            g_sceneDepthH.load(std::memory_order_relaxed),
            g_sceneDepthFmt.load(std::memory_order_relaxed),
            g_dsvCount.load(std::memory_order_relaxed),
            static_cast<unsigned long long>(g_omSetRtCalls.load(std::memory_order_relaxed)),
            g_distinctCmdVtables.load(std::memory_order_relaxed));
    }
    if (queue == g_presentQueue.load(std::memory_order_relaxed)) {
        g_eclPresentCalls.fetch_add(1, std::memory_order_relaxed);
        g_eclPresentLists.fetch_add(numLists, std::memory_order_relaxed);
    }
    void** vtable = *reinterpret_cast<void***>(queue);
    ExecuteCommandListsFn originalFn = GetOriginalMethod<ExecuteCommandListsFn>(vtable, 10);
    if (originalFn) {
        originalFn(queue, numLists, lists);
    }
}

// ID3D12CommandQueue vtable slot 10 == ExecuteCommandLists. All command queues
// of the same type share one vtable, so patching once observes every queue.
void InstallCommandQueueDiagHook(ID3D12CommandQueue* queue) {
    if (!queue) return;
    g_presentQueue.store(queue, std::memory_order_relaxed);
    void** vtable = *reinterpret_cast<void***>(queue);
    PatchVtableMethod(vtable, 10, reinterpret_cast<void*>(&HookedExecuteCommandLists));
}

// Cross-queue fence tracker: the game writes scene depth on its own render
// queue while we want to copy that resource on the swapchain (present) queue.
// On VDXR + R32_TYPELESS the format guard passes our path through, and without
// explicit cross-queue sync our copy can race the game's writer → GPU hang.
// Hook ID3D12CommandQueue::Signal (vtable slot 14) on every distinct queue
// vtable we see and record (queue → last (fence, value)). Before our depth
// copy, our queue Waits on each tracked queue's last value — GPU-side, no
// CPU stall. If the game has not yet signaled anything we just no-op.
using SignalFn = HRESULT(STDMETHODCALLTYPE*)(ID3D12CommandQueue*, ID3D12Fence*, UINT64);

struct QueueSignalState {
    ID3D12Fence* fence = nullptr;
    UINT64 value = 0;
};
std::mutex g_queueSignalMutex;
std::unordered_map<ID3D12CommandQueue*, QueueSignalState> g_queueLastSignal;
// Dedicated mutex for the hooked-vtable set. We CANNOT reuse g_vtableMutex
// here because PatchVtableMethod itself locks g_vtableMutex, which would
// cause a recursive lock on a non-recursive std::mutex (= undefined
// behavior, MSVC throws 0xE06D7363 C++ exception). Lesson learned the hard
// way: per-set membership tracking gets its own lightweight mutex.
std::mutex g_signalHookSetMutex;
std::unordered_set<void**> g_signalHookedVtables;

HRESULT STDMETHODCALLTYPE HookedQueueSignal(ID3D12CommandQueue* queue, ID3D12Fence* fence, UINT64 value) {
    void** vtable = *reinterpret_cast<void***>(queue);
    SignalFn originalFn = GetOriginalMethod<SignalFn>(vtable, 14);
    const HRESULT hr = originalFn ? originalFn(queue, fence, value) : E_FAIL;
    if (SUCCEEDED(hr) && fence) {
        std::lock_guard<std::mutex> lock(g_queueSignalMutex);
        auto& entry = g_queueLastSignal[queue];
        // Track by queue ptr; same queue may signal multiple fences but we
        // want the LATEST per-queue gate, regardless of which fence. Multiple
        // game queues each carry their own latest signal independently.
        if (entry.fence != fence) {
            if (entry.fence) entry.fence->Release();
            fence->AddRef();
            entry.fence = fence;
        }
        entry.value = value;
    }
    return hr;
}

void TryHookQueueSignalVtable(ID3D12CommandQueue* queue) {
    if (!queue) return;
    void** vtable = *reinterpret_cast<void***>(queue);
    {
        std::lock_guard<std::mutex> lock(g_signalHookSetMutex);
        if (g_signalHookedVtables.count(vtable)) return;
        g_signalHookedVtables.insert(vtable);
    }
    // PatchVtableMethod locks g_vtableMutex internally — must NOT be
    // called while we hold any other lock that PatchVtableMethod might
    // recursively try to take.
    PatchVtableMethod(vtable, 14, reinterpret_cast<void*>(&HookedQueueSignal));
}

// Issued on the consumer's queue (m_d3dQueue) BEFORE a CopyResource that may
// race game-side depth writes. Tries to wait on every tracked queue's most
// recent signal; harmless if none tracked yet.
extern "C" void CyberpunkVRPort_WaitOnAllGameSignals(ID3D12CommandQueue* consumerQueue) {
    if (!consumerQueue) return;
    std::vector<QueueSignalState> snapshot;
    {
        std::lock_guard<std::mutex> lock(g_queueSignalMutex);
        snapshot.reserve(g_queueLastSignal.size());
        for (auto& kv : g_queueLastSignal) {
            if (kv.first == consumerQueue) continue; // no self-wait
            if (kv.second.fence) {
                kv.second.fence->AddRef();
                snapshot.push_back(kv.second);
            }
        }
    }
    for (auto& s : snapshot) {
        consumerQueue->Wait(s.fence, s.value);
        s.fence->Release();
    }
}

static bool HasMode(const DXGI_MODE_DESC* modes, UINT count, UINT width, UINT height) {
    if (!modes) return false;
    for (UINT i = 0; i < count; ++i) {
        if (modes[i].Width == width && modes[i].Height == height) {
            return true;
        }
    }
    return false;
}

static bool HasMode1(const DXGI_MODE_DESC1* modes, UINT count, UINT width, UINT height) {
    if (!modes) return false;
    for (UINT i = 0; i < count; ++i) {
        if (modes[i].Width == width && modes[i].Height == height) {
            return true;
        }
    }
    return false;
}

static void FillMode(DXGI_MODE_DESC& mode, DXGI_FORMAT format, UINT width, UINT height) {
    mode.Width = width;
    mode.Height = height;
    mode.RefreshRate.Numerator = 90;
    mode.RefreshRate.Denominator = 1;
    mode.Format = format;
    mode.ScanlineOrdering = DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED;
    mode.Scaling = DXGI_MODE_SCALING_UNSPECIFIED;
}

static void FillMode1(DXGI_MODE_DESC1& mode, DXGI_FORMAT format, UINT width, UINT height) {
    mode.Width = width;
    mode.Height = height;
    mode.RefreshRate.Numerator = 90;
    mode.RefreshRate.Denominator = 1;
    mode.Format = format;
    mode.ScanlineOrdering = DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED;
    mode.Scaling = DXGI_MODE_SCALING_UNSPECIFIED;
    mode.Stereo = FALSE;
}

HRESULT STDMETHODCALLTYPE HookedGetDisplayModeList(IDXGIOutput* output, DXGI_FORMAT EnumFormat, UINT Flags, UINT* pNumModes, DXGI_MODE_DESC* pDesc) {
    void** vtable = *reinterpret_cast<void***>(output);
    GetDisplayModeListFn originalFn = GetOriginalMethod<GetDisplayModeListFn>(vtable, 8);
    
    if (!pNumModes) {
        return originalFn ? originalFn(output, EnumFormat, Flags, pNumModes, pDesc) : DXGI_ERROR_INVALID_CALL;
    }

    const UINT capacity = pDesc ? *pNumModes : 0;
    UINT originalCount = *pNumModes;
    HRESULT hr = originalFn ? originalFn(output, EnumFormat, Flags, &originalCount, pDesc) : DXGI_ERROR_INVALID_CALL;
    if (FAILED(hr) && hr != DXGI_ERROR_MORE_DATA) {
        *pNumModes = originalCount;
        return hr;
    }

    const UINT forcedWidth = GetForcedDisplayModeWidth();
    const UINT forcedHeight = GetForcedDisplayModeHeight();
    if (forcedWidth == 0 || forcedHeight == 0 || HasMode(pDesc, originalCount, forcedWidth, forcedHeight)) {
        *pNumModes = originalCount;
        return hr == DXGI_ERROR_MORE_DATA ? hr : S_OK;
    }

    const UINT requiredCount = originalCount + 1;
    if (!pDesc) {
        *pNumModes = requiredCount;
        return S_OK;
    }

    if (capacity <= originalCount || hr == DXGI_ERROR_MORE_DATA) {
        *pNumModes = requiredCount;
        return DXGI_ERROR_MORE_DATA;
    }

    FillMode(pDesc[originalCount], EnumFormat, forcedWidth, forcedHeight);
    *pNumModes = requiredCount;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE HookedGetDisplayModeList1(IDXGIOutput1* output, DXGI_FORMAT EnumFormat, UINT Flags, UINT* pNumModes, DXGI_MODE_DESC1* pDesc) {
    void** vtable = *reinterpret_cast<void***>(output);
    GetDisplayModeList1Fn originalFn = GetOriginalMethod<GetDisplayModeList1Fn>(vtable, 19);

    if (!pNumModes) {
        return originalFn ? originalFn(output, EnumFormat, Flags, pNumModes, pDesc) : DXGI_ERROR_INVALID_CALL;
    }

    const UINT capacity = pDesc ? *pNumModes : 0;
    UINT originalCount = *pNumModes;
    HRESULT hr = originalFn ? originalFn(output, EnumFormat, Flags, &originalCount, pDesc) : DXGI_ERROR_INVALID_CALL;
    if (FAILED(hr) && hr != DXGI_ERROR_MORE_DATA) {
        *pNumModes = originalCount;
        return hr;
    }

    const UINT forcedWidth = GetForcedDisplayModeWidth();
    const UINT forcedHeight = GetForcedDisplayModeHeight();
    if (forcedWidth == 0 || forcedHeight == 0 || HasMode1(pDesc, originalCount, forcedWidth, forcedHeight)) {
        *pNumModes = originalCount;
        return hr == DXGI_ERROR_MORE_DATA ? hr : S_OK;
    }

    const UINT requiredCount = originalCount + 1;
    if (!pDesc) {
        *pNumModes = requiredCount;
        return S_OK;
    }

    if (capacity <= originalCount || hr == DXGI_ERROR_MORE_DATA) {
        *pNumModes = requiredCount;
        return DXGI_ERROR_MORE_DATA;
    }

    FillMode1(pDesc[originalCount], EnumFormat, forcedWidth, forcedHeight);
    *pNumModes = requiredCount;
    return S_OK;
}

void InstallOutputHook(IDXGIOutput* output) {
    if (!output) return;
    void*** objectVtable = reinterpret_cast<void***>(output);
    if (!objectVtable || !*objectVtable) return;
    void** vtable = *objectVtable;
    PatchVtableMethod(vtable, 8, reinterpret_cast<void*>(&HookedGetDisplayModeList));

    IDXGIOutput1* output1 = nullptr;
    if (SUCCEEDED(output->QueryInterface(IID_PPV_ARGS(&output1))) && output1) {
        void** vtable1 = *reinterpret_cast<void***>(output1);
        PatchVtableMethod(vtable1, 19, reinterpret_cast<void*>(&HookedGetDisplayModeList1));
        output1->Release();
    }
}

HRESULT STDMETHODCALLTYPE HookedEnumOutputs(IDXGIAdapter* adapter, UINT Output, IDXGIOutput** ppOutput) {
    void** vtable = *reinterpret_cast<void***>(adapter);
    EnumOutputsFn originalFn = GetOriginalMethod<EnumOutputsFn>(vtable, 7);

    HRESULT hr = originalFn ? originalFn(adapter, Output, ppOutput) : DXGI_ERROR_INVALID_CALL;
    if (SUCCEEDED(hr) && ppOutput && *ppOutput) {
        InstallOutputHook(*ppOutput);
    }
    return hr;
}

void InstallAdapterHook(IDXGIAdapter* adapter) {
    if (!adapter) return;
    void*** objectVtable = reinterpret_cast<void***>(adapter);
    if (!objectVtable || !*objectVtable) return;
    void** vtable = *objectVtable;
    PatchVtableMethod(vtable, 7, reinterpret_cast<void*>(&HookedEnumOutputs));
}

HRESULT STDMETHODCALLTYPE HookedPresent(IDXGISwapChain* swapChain, UINT syncInterval, UINT flags) {
    RememberDredDeviceFromSwapChain(swapChain);

    DXGI_SWAP_CHAIN_DESC desc{};
    const bool hasDesc = SUCCEEDED(swapChain->GetDesc(&desc));
    if (hasDesc) {
        UpdateCursorCapture(desc.OutputWindow);
    }

    // DLSS is loaded lazily after the renderer first evaluates a frame, so
    // nvngx_dlss.dll may not yet be in the process at startup. Try to install
    // the NGX EvaluateFeature hook on every Present until it succeeds; once
    // installed the function returns true cheaply.
    static std::atomic<bool> s_ngxHookTried{false};
    if (!s_ngxHookTried.load(std::memory_order_acquire)) {
        if (NgxInstallEvaluateFeatureHook()) {
            s_ngxHookTried.store(true, std::memory_order_release);
        }
    }

    OverlayRender(swapChain);
    OpenXRManager::Get().OnPresent(swapChain);
    void** vtable = *reinterpret_cast<void***>(swapChain);
    PresentFn originalFn = GetOriginalMethod<PresentFn>(vtable, 8);
    const HRESULT hr = originalFn ? originalFn(swapChain, syncInterval, flags) : DXGI_ERROR_INVALID_CALL;
    if (FAILED(hr)) {
        Log("[DRED] Present failure observed. hr=0x%08X removed=%d swapChain=%p\n",
            static_cast<unsigned>(hr),
            IsDeviceRemovedHr(hr) ? 1 : 0,
            swapChain);
    }
    if (FAILED(hr) && IsDeviceRemovedHr(hr)) {
        DumpDredOnce(swapChain, hr);
    }

    static uint64_t presentLogCounter = 0;
    if (hr != S_OK || ((++presentLogCounter % 600) == 1)) {
        HWND foreground = GetForegroundWindow();
        RECT clientRect{};
        RECT windowRect{};
        if (hasDesc && desc.OutputWindow) {
            GetClientRect(desc.OutputWindow, &clientRect);
            GetWindowRect(desc.OutputWindow, &windowRect);
        }
        Log("Present result: hr=0x%08X hwnd=%p fg=%p windowed=%d desc=%ux%u client=%ldx%ld window=(%ld,%ld)-(%ld,%ld) cursorClipped=%d\n",
            static_cast<unsigned int>(hr),
            hasDesc ? desc.OutputWindow : nullptr,
            foreground,
            hasDesc ? (desc.Windowed ? 1 : 0) : -1,
            hasDesc ? desc.BufferDesc.Width : 0,
            hasDesc ? desc.BufferDesc.Height : 0,
            clientRect.right - clientRect.left,
            clientRect.bottom - clientRect.top,
            windowRect.left,
            windowRect.top,
            windowRect.right,
            windowRect.bottom,
            g_cursorClipped ? 1 : 0);

        const uint64_t totalLists = g_eclTotalLists.load(std::memory_order_relaxed);
        const uint64_t presentLists = g_eclPresentLists.load(std::memory_order_relaxed);
        Log("[ECL-DIAG] presentQueue=%p lists present=%llu/%llu calls=%llu/%llu share=%.1f%% distinctQueues=%u\n",
            g_presentQueue.load(std::memory_order_relaxed),
            static_cast<unsigned long long>(presentLists),
            static_cast<unsigned long long>(totalLists),
            static_cast<unsigned long long>(g_eclPresentCalls.load(std::memory_order_relaxed)),
            static_cast<unsigned long long>(g_eclTotalCalls.load(std::memory_order_relaxed)),
            totalLists ? (100.0 * static_cast<double>(presentLists) / static_cast<double>(totalLists)) : 0.0,
            g_eclDistinctQueues.load(std::memory_order_relaxed));
    }
    return hr;
}

HRESULT STDMETHODCALLTYPE HookedSetFullscreenState(IDXGISwapChain* swapChain, BOOL fullscreen, IDXGIOutput* target) {
    if (target) {
        InstallOutputHook(target);
    }

    void** vtable = *reinterpret_cast<void***>(swapChain);
    SetFullscreenStateFn originalFn = GetOriginalMethod<SetFullscreenStateFn>(vtable, 10);
    Log("SetFullscreenState intercepted: fullscreen=%d target=%p.\n", fullscreen ? 1 : 0, target);
    if (fullscreen) {
        return DXGI_ERROR_NOT_CURRENTLY_AVAILABLE;
    }
    return originalFn ? originalFn(swapChain, FALSE, target) : DXGI_ERROR_INVALID_CALL;
}

HRESULT STDMETHODCALLTYPE HookedResizeBuffers(IDXGISwapChain* swapChain, UINT bufferCount, UINT width, UINT height, DXGI_FORMAT newFormat, UINT flags) {
    const UINT forcedWidth = GetForcedSwapchainWidth();
    const UINT forcedHeight = GetForcedSwapchainHeight();
    const UINT outWidth = forcedWidth != 0 ? forcedWidth : width;
    const UINT outHeight = forcedHeight != 0 ? forcedHeight : height;
    if (outWidth != width || outHeight != height) {
        Log("ResizeBuffers override: %ux%u -> %ux%u\n", width, height, outWidth, outHeight);
    }

    void** vtable = *reinterpret_cast<void***>(swapChain);
    ResizeBuffersFn originalFn = GetOriginalMethod<ResizeBuffersFn>(vtable, 13);
    OverlayInvalidateSwapchainResources();
    return originalFn ? originalFn(swapChain, bufferCount, outWidth, outHeight, newFormat, flags) : DXGI_ERROR_INVALID_CALL;
}

HRESULT STDMETHODCALLTYPE HookedResizeBuffers1(IDXGISwapChain3* swapChain, UINT bufferCount, UINT width, UINT height, DXGI_FORMAT format, UINT flags, const UINT* creationNodeMask, IUnknown* const* presentQueue) {
    const UINT forcedWidth = GetForcedSwapchainWidth();
    const UINT forcedHeight = GetForcedSwapchainHeight();
    const UINT outWidth = forcedWidth != 0 ? forcedWidth : width;
    const UINT outHeight = forcedHeight != 0 ? forcedHeight : height;
    if (outWidth != width || outHeight != height) {
        Log("ResizeBuffers1 override: %ux%u -> %ux%u\n", width, height, outWidth, outHeight);
    }

    void** vtable = *reinterpret_cast<void***>(swapChain);
    ResizeBuffers1Fn originalFn = GetOriginalMethod<ResizeBuffers1Fn>(vtable, 39);
    OverlayInvalidateSwapchainResources();
    return originalFn ? originalFn(swapChain, bufferCount, outWidth, outHeight, format, flags, creationNodeMask, presentQueue) : DXGI_ERROR_INVALID_CALL;
}

void InstallSwapchainHooks(IDXGISwapChain* swapChain) {
    if (!swapChain) return;

    void*** objectVtable = reinterpret_cast<void***>(swapChain);
    if (!objectVtable || !*objectVtable) return;

    void** vtable = *objectVtable;
    PatchVtableMethod(vtable, 8, reinterpret_cast<void*>(&HookedPresent));
    PatchVtableMethod(vtable, 10, reinterpret_cast<void*>(&HookedSetFullscreenState));
    PatchVtableMethod(vtable, 13, reinterpret_cast<void*>(&HookedResizeBuffers));

    IDXGISwapChain3* swapChain3 = nullptr;
    if (SUCCEEDED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3))) && swapChain3) {
        void** vtable3 = *reinterpret_cast<void***>(swapChain3);
        PatchVtableMethod(vtable3, 39, reinterpret_cast<void*>(&HookedResizeBuffers1));
        swapChain3->Release();
    }
}
}

// Extern "C" forwarder so dxgi_proxy.cpp can invoke the anonymous-namespace
// DRED initializer at each CreateDXGIFactory* entry (runs before any D3D12
// device is created — required for breadcrumbs/page-fault data to populate).
extern "C" void CyberpunkVRPort_EnableDredOnce() {
    EnableDredOnce();
}

DXGIFactoryWrapper::DXGIFactoryWrapper(IDXGIFactory7* realFactory) : m_real(realFactory), m_refCount(1) {}

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::QueryInterface(REFIID riid, void** ppvObject) {
    if (riid == __uuidof(IUnknown) || riid == __uuidof(IDXGIObject) || riid == __uuidof(IDXGIFactory) ||
        riid == __uuidof(IDXGIFactory1) || riid == __uuidof(IDXGIFactory2) || riid == __uuidof(IDXGIFactory3) ||
        riid == __uuidof(IDXGIFactory4) || riid == __uuidof(IDXGIFactory5) || riid == __uuidof(IDXGIFactory6) ||
        riid == __uuidof(IDXGIFactory7)) {
        *ppvObject = this;
        AddRef();
        return S_OK;
    }
    return m_real->QueryInterface(riid, ppvObject);
}

ULONG STDMETHODCALLTYPE DXGIFactoryWrapper::AddRef() {
    return InterlockedIncrement(&m_refCount);
}

ULONG STDMETHODCALLTYPE DXGIFactoryWrapper::Release() {
    ULONG ref = InterlockedDecrement(&m_refCount);
    if (ref == 0) {
        m_real->Release();
        delete this;
    }
    return ref;
}

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::SetPrivateData(REFGUID Name, UINT DataSize, const void* pData) { return m_real->SetPrivateData(Name, DataSize, pData); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::SetPrivateDataInterface(REFGUID Name, const IUnknown* pUnknown) { return m_real->SetPrivateDataInterface(Name, pUnknown); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::GetPrivateData(REFGUID Name, UINT* pDataSize, void* pData) { return m_real->GetPrivateData(Name, pDataSize, pData); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::GetParent(REFIID riid, void** ppParent) { return m_real->GetParent(riid, ppParent); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumAdapters(UINT Adapter, IDXGIAdapter** ppAdapter) {
    HRESULT hr = m_real->EnumAdapters(Adapter, ppAdapter);
    if (SUCCEEDED(hr) && ppAdapter && *ppAdapter) {
        InstallAdapterHook(*ppAdapter);
    }
    return hr;
}
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::MakeWindowAssociation(HWND WindowHandle, UINT Flags) { return m_real->MakeWindowAssociation(WindowHandle, Flags); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::GetWindowAssociation(HWND* pWindowHandle) { return m_real->GetWindowAssociation(pWindowHandle); }

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSwapChain(IUnknown* pDevice, DXGI_SWAP_CHAIN_DESC* pDesc, IDXGISwapChain** ppSwapChain) {
    Log("CreateSwapChain intercepted! pDevice=%p\n", pDevice);
    PrepareStartupLiveControls();

    DXGI_SWAP_CHAIN_DESC localDesc{};
    DXGI_SWAP_CHAIN_DESC* swapDesc = pDesc;
    const UINT forcedWidth = GetForcedDisplayModeWidth();
    const UINT forcedHeight = GetForcedDisplayModeHeight();
    if (pDesc) {
        localDesc = *pDesc;
        bool changed = false;
        if (forcedWidth != 0 && forcedHeight != 0) {
            localDesc.BufferDesc.Width = forcedWidth;
            localDesc.BufferDesc.Height = forcedHeight;
            // Standard DXGI_SWAP_CHAIN_DESC does not have Scaling.
            changed = true;
            Log("CreateSwapChain override: %ux%u -> %ux%u\n",
                pDesc->BufferDesc.Width,
                pDesc->BufferDesc.Height,
                forcedWidth,
                forcedHeight);
        }
        if (!localDesc.Windowed) {
            localDesc.Windowed = TRUE;
            changed = true;
            Log("CreateSwapChain: Forced Windowed=TRUE\n");
        }
        if (changed) {
            swapDesc = &localDesc;
        }
    }

    // pDevice in D3D12 is ID3D12CommandQueue
    ID3D12CommandQueue* pQueue = nullptr;
    if (SUCCEEDED(pDevice->QueryInterface(IID_PPV_ARGS(&pQueue)))) {
        ID3D12Device* d3dDevice = nullptr;
        if (SUCCEEDED(pQueue->GetDevice(IID_PPV_ARGS(&d3dDevice)))) {
            RememberDredDevice(d3dDevice);
            OpenXRManager::Get().InitGraphics(d3dDevice, pQueue);
            OverlaySetDeviceAndQueue(d3dDevice, pQueue);
            InstallCommandQueueDiagHook(pQueue);
            InstallDepthCaptureHooks(d3dDevice);
            d3dDevice->Release();
        }
        pQueue->Release();
    }

    HWND hWnd = swapDesc ? swapDesc->OutputWindow : nullptr;
    g_gameHwnd = hWnd;
    UINT windowWidth = GetForcedWindowWidth();
    UINT windowHeight = GetForcedWindowHeight();
    if (windowWidth == forcedWidth) {
        windowWidth = 0;
    }
    if (windowWidth == 0 && hWnd) {
        HMONITOR monitor = MonitorFromWindow(hWnd, MONITOR_DEFAULTTOPRIMARY);
        MONITORINFO mi = { sizeof(MONITORINFO) };
        if (GetMonitorInfoA(monitor, &mi)) {
            int monWidth = mi.rcMonitor.right - mi.rcMonitor.left;
            int monHeight = mi.rcMonitor.bottom - mi.rcMonitor.top;
            windowWidth = monWidth;
            windowHeight = monHeight;
        }
    }
    if (windowWidth != 0 && windowHeight != 0 && hWnd) {
        SetWindowPos(hWnd, nullptr, 0, 0, windowWidth, windowHeight, SWP_NOMOVE | SWP_NOZORDER);
        Log("CreateSwapChain: Capped window size to %ux%u\n", windowWidth, windowHeight);
    }
    const HRESULT hr = m_real->CreateSwapChain(pDevice, swapDesc, ppSwapChain);
    if (SUCCEEDED(hr) && ppSwapChain && *ppSwapChain) {
        if (swapDesc && swapDesc->OutputWindow) {
            OverlaySetWindow(swapDesc->OutputWindow);
        }
        InstallSwapchainHooks(*ppSwapChain);
        InstallOSHooks();
    }
    return hr;
}

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSoftwareAdapter(HMODULE Module, IDXGIAdapter** ppAdapter) { return m_real->CreateSoftwareAdapter(Module, ppAdapter); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumAdapters1(UINT Adapter, IDXGIAdapter1** ppAdapter) {
    HRESULT hr = m_real->EnumAdapters1(Adapter, ppAdapter);
    if (SUCCEEDED(hr) && ppAdapter && *ppAdapter) {
        InstallAdapterHook(*ppAdapter);
    }
    return hr;
}
BOOL STDMETHODCALLTYPE DXGIFactoryWrapper::IsCurrent() { return m_real->IsCurrent(); }
BOOL STDMETHODCALLTYPE DXGIFactoryWrapper::IsWindowedStereoEnabled() { return m_real->IsWindowedStereoEnabled(); }

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSwapChainForHwnd(IUnknown* pDevice, HWND hWnd, const DXGI_SWAP_CHAIN_DESC1* pDesc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* pFullscreenDesc, IDXGIOutput* pRestrictToOutput, IDXGISwapChain1** ppSwapChain) {
    Log("CreateSwapChainForHwnd intercepted! pDevice=%p\n", pDevice);
    g_gameHwnd = hWnd;
    PrepareStartupLiveControls();

    DXGI_SWAP_CHAIN_DESC1 localDesc{};
    const DXGI_SWAP_CHAIN_DESC1* swapDesc = pDesc;
    const UINT forcedWidth = GetForcedDisplayModeWidth();
    const UINT forcedHeight = GetForcedDisplayModeHeight();
    if (pDesc && forcedWidth != 0 && forcedHeight != 0) {
        localDesc = *pDesc;
        localDesc.Width = forcedWidth;
        localDesc.Height = forcedHeight;
        localDesc.Scaling = DXGI_SCALING_STRETCH;
        swapDesc = &localDesc;
        Log("CreateSwapChainForHwnd override: %ux%u -> %ux%u\n",
            pDesc->Width,
            pDesc->Height,
            forcedWidth,
            forcedHeight);
    }

    DXGI_SWAP_CHAIN_FULLSCREEN_DESC localFsDesc{};
    const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fsDesc = pFullscreenDesc;
    if (pFullscreenDesc && !pFullscreenDesc->Windowed) {
        localFsDesc = *pFullscreenDesc;
        localFsDesc.Windowed = TRUE;
        fsDesc = &localFsDesc;
        Log("CreateSwapChainForHwnd: Forced Windowed=TRUE\n");
    }

    UINT windowWidth = GetForcedWindowWidth();
    UINT windowHeight = GetForcedWindowHeight();
    if (windowWidth == forcedWidth) {
        windowWidth = 0; // If they are the same as swapchain, user wants auto-cap
    }
    if (windowWidth == 0 && hWnd) {
        HMONITOR monitor = MonitorFromWindow(hWnd, MONITOR_DEFAULTTOPRIMARY);
        MONITORINFO mi = { sizeof(MONITORINFO) };
        if (GetMonitorInfoA(monitor, &mi)) {
            int monWidth = mi.rcMonitor.right - mi.rcMonitor.left;
            int monHeight = mi.rcMonitor.bottom - mi.rcMonitor.top;
            windowWidth = monWidth;
            windowHeight = monHeight;
        }
    }
    if (windowWidth != 0 && windowHeight != 0 && hWnd) {
        SetWindowPos(hWnd, nullptr, 0, 0, windowWidth, windowHeight, SWP_NOMOVE | SWP_NOZORDER);
        Log("CreateSwapChainForHwnd: Capped window size to %ux%u\n", windowWidth, windowHeight);
    }

    ID3D12CommandQueue* pQueue = nullptr;
    if (SUCCEEDED(pDevice->QueryInterface(IID_PPV_ARGS(&pQueue)))) {
        ID3D12Device* d3dDevice = nullptr;
        if (SUCCEEDED(pQueue->GetDevice(IID_PPV_ARGS(&d3dDevice)))) {
            RememberDredDevice(d3dDevice);
            OpenXRManager::Get().InitGraphics(d3dDevice, pQueue);
            OverlaySetDeviceAndQueue(d3dDevice, pQueue);
            InstallCommandQueueDiagHook(pQueue);
            InstallDepthCaptureHooks(d3dDevice);
            d3dDevice->Release();
        }
        pQueue->Release();
    }
    const HRESULT hr = m_real->CreateSwapChainForHwnd(pDevice, hWnd, swapDesc, fsDesc, pRestrictToOutput, ppSwapChain);
    if (SUCCEEDED(hr) && ppSwapChain && *ppSwapChain) {
        OverlaySetWindow(hWnd);
        InstallSwapchainHooks(*ppSwapChain);
        InstallOSHooks();
    }
    return hr;
}

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSwapChainForCoreWindow(IUnknown* pDevice, IUnknown* pWindow, const DXGI_SWAP_CHAIN_DESC1* pDesc, IDXGIOutput* pRestrictToOutput, IDXGISwapChain1** ppSwapChain) { return m_real->CreateSwapChainForCoreWindow(pDevice, pWindow, pDesc, pRestrictToOutput, ppSwapChain); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::GetSharedResourceAdapterLuid(HANDLE hResource, LUID* pLuid) { return m_real->GetSharedResourceAdapterLuid(hResource, pLuid); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterStereoStatusWindow(HWND WindowHandle, UINT wMsg, DWORD* pdwCookie) { return m_real->RegisterStereoStatusWindow(WindowHandle, wMsg, pdwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterStereoStatusEvent(HANDLE hEvent, DWORD* pdwCookie) { return m_real->RegisterStereoStatusEvent(hEvent, pdwCookie); }
void STDMETHODCALLTYPE DXGIFactoryWrapper::UnregisterStereoStatus(DWORD dwCookie) { m_real->UnregisterStereoStatus(dwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterOcclusionStatusWindow(HWND WindowHandle, UINT wMsg, DWORD* pdwCookie) { return m_real->RegisterOcclusionStatusWindow(WindowHandle, wMsg, pdwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterOcclusionStatusEvent(HANDLE hEvent, DWORD* pdwCookie) { return m_real->RegisterOcclusionStatusEvent(hEvent, pdwCookie); }
void STDMETHODCALLTYPE DXGIFactoryWrapper::UnregisterOcclusionStatus(DWORD dwCookie) { m_real->UnregisterOcclusionStatus(dwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSwapChainForComposition(IUnknown* pDevice, const DXGI_SWAP_CHAIN_DESC1* pDesc, IDXGIOutput* pRestrictToOutput, IDXGISwapChain1** ppSwapChain) { return m_real->CreateSwapChainForComposition(pDevice, pDesc, pRestrictToOutput, ppSwapChain); }
UINT STDMETHODCALLTYPE DXGIFactoryWrapper::GetCreationFlags() { return m_real->GetCreationFlags(); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumAdapterByLuid(LUID AdapterLuid, REFIID riid, void** ppvAdapter) {
    HRESULT hr = m_real->EnumAdapterByLuid(AdapterLuid, riid, ppvAdapter);
    if (SUCCEEDED(hr) && ppvAdapter && *ppvAdapter) {
        InstallAdapterHook(reinterpret_cast<IDXGIAdapter*>(*ppvAdapter));
    }
    return hr;
}
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumWarpAdapter(REFIID riid, void** ppvAdapter) { return m_real->EnumWarpAdapter(riid, ppvAdapter); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CheckFeatureSupport(DXGI_FEATURE Feature, void* pFeatureSupportData, UINT FeatureSupportDataSize) { return m_real->CheckFeatureSupport(Feature, pFeatureSupportData, FeatureSupportDataSize); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumAdapterByGpuPreference(UINT Adapter, DXGI_GPU_PREFERENCE GpuPreference, REFIID riid, void** ppvAdapter) {
    HRESULT hr = m_real->EnumAdapterByGpuPreference(Adapter, GpuPreference, riid, ppvAdapter);
    if (SUCCEEDED(hr) && ppvAdapter && *ppvAdapter) {
        InstallAdapterHook(reinterpret_cast<IDXGIAdapter*>(*ppvAdapter));
    }
    return hr;
}
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterAdaptersChangedEvent(HANDLE hEvent, DWORD* pdwCookie) { return m_real->RegisterAdaptersChangedEvent(hEvent, pdwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::UnregisterAdaptersChangedEvent(DWORD dwCookie) { return m_real->UnregisterAdaptersChangedEvent(dwCookie); }

// [DEPTH] Accessors for the submit path (openxr_manager) to snapshot the game's
// scene depth with the correct (observed) resource state.
extern "C" ID3D12Resource* OmoGetSceneDepthResource() { return g_sceneDepthRes.load(std::memory_order_relaxed); }
extern "C" unsigned int OmoGetSceneDepthState() { return g_sceneDepthState.load(std::memory_order_relaxed); }
extern "C" unsigned int OmoGetSceneDepthWidth() { return g_sceneDepthW.load(std::memory_order_relaxed); }
extern "C" unsigned int OmoGetSceneDepthHeight() { return g_sceneDepthH.load(std::memory_order_relaxed); }
extern "C" unsigned int OmoGetSceneDepthFormat() { return g_sceneDepthFmt.load(std::memory_order_relaxed); }
