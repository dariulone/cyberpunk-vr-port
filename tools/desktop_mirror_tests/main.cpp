// The desktop mirror (include/Render/DesktopMirror.hpp) on a WARP device: a tall 64x96 swapchain on a wide 120x60 hidden
// window held at its own size (a launcher window size of the player's), so the centre crop runs: the rows 32..63 of the
// frame stretched over the whole backbuffer. The backbuffer is read back and checked against that, with the cover
// transform of a range of sizes and the window geometry. "trace" prints a hash of all of it instead, so the C++ and
// the Rust build compare bit for bit (cmake/ParityTrace.cmake).
#include "Render/DesktopMirror.hpp"
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>

void Log(const char*, ...) {}
// The launcher's sizes (Core/VrCore.cpp): a 64x96 render, and a window width of the player's own, so the mirror never
// moves the window and the crop is what it does.
extern "C" UINT GetForcedDisplayModeWidth() { return 64; }
extern "C" UINT GetForcedDisplayModeHeight() { return 96; }
extern "C" UINT GetForcedWindowWidth() { return 1234; }
#if CVR_RUST_VRIK
// The plugin hands the Rust mirror these sizes as it loads (rust/crates/cvr-render/src/export_attach.rs); the test here.
extern "C" void cvr_desktop_mirror_attach(UINT (*)(), UINT (*)(), UINT (*)());
#endif

using Microsoft::WRL::ComPtr;
static void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
static void Hr(HRESULT hr, const char* text) { if (FAILED(hr)) throw std::runtime_error(text); }

int main(int argc, char** argv) try {
#if CVR_RUST_VRIK
    cvr_desktop_mirror_attach(&GetForcedDisplayModeWidth, &GetForcedDisplayModeHeight, &GetForcedWindowWidth);
#endif
    const bool trace = argc > 1 && std::strcmp(argv[1], "trace") == 0;
    uint64_t hash = 1469598103934665603ull;
    const auto mix = [&](const void* p, size_t n) {
        for (size_t i = 0; i < n; ++i) { hash ^= static_cast<const uint8_t*>(p)[i]; hash *= 1099511628211ull; }
    };
    // The cover transform over sizes of every shape, and the zero sizes it refuses.
    const UINT sizes[] = {0, 1, 7, 64, 96, 1440, 2560, 2648, 2864};
    for (UINT sw : sizes) for (UINT sh : sizes) for (UINT dw : {0u, 120u, 1920u}) for (UINT dh : {0u, 60u, 1080u}) {
        DesktopCoverTransform t{};
        const bool ok = ComputeDesktopCoverTransform(sw, sh, dw, dh, t);
        mix(&ok, 1); mix(&t, sizeof(t));
    }

    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"cvr_desktop_mirror_test";
    RegisterClassW(&wc);
    const HWND hwnd = CreateWindowExW(0, wc.lpszClassName, L"", WS_POPUP, 0, 0, 120, 60, nullptr, nullptr, wc.hInstance, nullptr);
    Check(hwnd != nullptr, "window");
    RECT client{}; POINT origin{};
    Check(GetDesktopPhysicalClientRect(hwnd, client, origin) && client.right - client.left == 120 && client.bottom - client.top == 60, "client rect");
    RECT window{}, monitor{}, target{};
    const bool placed = GetDesktopMonitorWindowRect(hwnd, window, &monitor, &target);
    Check(placed && target.right - target.left > 0 && target.bottom - target.top > 0, "monitor window rect");
    // The render's aspect, never upscaled: a 64x96 render fits any monitor at its own size.
    Check(target.right - target.left == 64 && target.bottom - target.top == 96, "the window is not the render's own size");
    mix(&placed, 1); mix(&window, sizeof(window)); mix(&monitor, sizeof(monitor)); mix(&target, sizeof(target));

    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "factory");
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)), "warp");
    ComPtr<ID3D12Device> device; Hr(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)), "device");
    D3D12_COMMAND_QUEUE_DESC qd{}; qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ComPtr<ID3D12CommandQueue> queue; Hr(device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue)), "queue");
    DXGI_SWAP_CHAIN_DESC1 sd{}; sd.Width = 64; sd.Height = 96; sd.Format = DXGI_FORMAT_R8G8B8A8_UNORM; sd.SampleDesc.Count = 1;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; sd.BufferCount = 2; sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    ComPtr<IDXGISwapChain1> swap1; Hr(factory->CreateSwapChainForHwnd(queue.Get(), hwnd, &sd, nullptr, nullptr, &swap1), "swapchain");
    ComPtr<IDXGISwapChain3> swap; Hr(swap1.As(&swap), "swapchain3");
    ComPtr<ID3D12Resource> back; Hr(swap->GetBuffer(swap->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&back)), "backbuffer");

    ComPtr<ID3D12CommandAllocator> allocator; Hr(device->CreateCommandAllocator(qd.Type, IID_PPV_ARGS(&allocator)), "allocator");
    ComPtr<ID3D12GraphicsCommandList> list; Hr(device->CreateCommandList(0, qd.Type, allocator.Get(), nullptr, IID_PPV_ARGS(&list)), "list");
    ComPtr<ID3D12Fence> fence; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "fence");
    uint64_t fenceValue = 0;
    const auto finish = [&] {
        Hr(queue->Signal(fence.Get(), ++fenceValue), "signal");
        const HANDLE e = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        Hr(fence->SetEventOnCompletion(fenceValue, e), "event");
        const bool done = WaitForSingleObject(e, 10000) == WAIT_OBJECT_0; CloseHandle(e);
        Check(done, "GPU timeout");
    };
    const auto buffer = [&](UINT64 size, D3D12_HEAP_TYPE type, D3D12_RESOURCE_STATES state) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = type;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; d.Width = size; d.Height = 1; d.DepthOrArraySize = 1;
        d.MipLevels = 1; d.SampleDesc.Count = 1; d.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> r; Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, state, nullptr, IID_PPV_ARGS(&r)), "buffer");
        return r;
    };
    const auto transition = [&](D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
        D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition = {back.Get(), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after}; list->ResourceBarrier(1, &b);
    };
    constexpr UINT pitch = 256, W = 64, H = 96;
    // The frame: red is the column (copied 1:1 across), green the row (stretched three times over).
    auto upload = buffer(pitch * H, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
    uint8_t* p = nullptr; Hr(upload->Map(0, nullptr, reinterpret_cast<void**>(&p)), "map upload");
    for (UINT y = 0; y < H; ++y) for (UINT x = 0; x < W; ++x) { uint8_t* t = p + y * pitch + x * 4; t[0] = uint8_t(x * 4); t[1] = uint8_t(y * 2); t[2] = 77; t[3] = 0; }
    upload->Unmap(0, nullptr);
    D3D12_TEXTURE_COPY_LOCATION from{}, to{};
    from.pResource = upload.Get(); from.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; from.PlacedFootprint.Footprint = {DXGI_FORMAT_R8G8B8A8_UNORM, W, H, 1, pitch};
    to.pResource = back.Get(); to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    transition(D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_DEST);
    list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    transition(D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT);
    Hr(list->Close(), "close"); { ID3D12CommandList* l[]{list.Get()}; queue->ExecuteCommandLists(1, l); } finish();

    DesktopMirrorSetDeviceAndQueue(device.Get(), queue.Get());
    DesktopMirrorRender(swap.Get(), hwnd);
    finish();

    auto readback = buffer(pitch * H, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    Hr(allocator->Reset(), "allocator reset"); Hr(list->Reset(allocator.Get(), nullptr), "list reset");
    D3D12_TEXTURE_COPY_LOCATION rd{}, rs{};
    rd.pResource = readback.Get(); rd.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; rd.PlacedFootprint.Footprint = {DXGI_FORMAT_R8G8B8A8_UNORM, W, H, 1, pitch};
    rs.pResource = back.Get(); rs.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    transition(D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE);
    list->CopyTextureRegion(&rd, 0, 0, 0, &rs, nullptr);
    transition(D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT);
    Hr(list->Close(), "close"); { ID3D12CommandList* l[]{list.Get()}; queue->ExecuteCommandLists(1, l); } finish();
    const uint8_t* r = nullptr; Hr(readback->Map(0, nullptr, reinterpret_cast<void**>(const_cast<uint8_t**>(&r))), "map readback");
    for (UINT y = 0; y < H; ++y) {
        mix(r + y * pitch, W * 4);
        // Rows 32..63 over 96: the source row under the centre of output row y.
        const double source = 32.0 + (y + 0.5) / 3.0 - 0.5;
        for (UINT x = 0; x < W; ++x) {
            const uint8_t* t = r + y * pitch + x * 4;
            Check(t[0] == uint8_t(x * 4), "columns moved");
            Check(std::abs(t[1] - std::lround(std::clamp(source, 32.0, 63.0) * 2)) <= 2, "not the centre rows stretched");
            Check(t[2] == 77 && t[3] == 255, "colour or alpha changed");
        }
    }
    readback->Unmap(0, nullptr);
    DesktopMirrorInvalidate();
    if (trace) { std::printf("desktop_mirror trace %016llx\n", static_cast<unsigned long long>(hash)); return 0; }
    std::printf("PASS cover transform, window geometry, 64x96 frame centre-cropped to a 120x60 window\n");
    DestroyWindow(hwnd);
    return 0;
} catch (const std::exception& e) { std::fprintf(stderr, "%s\n", e.what()); return 1; }
