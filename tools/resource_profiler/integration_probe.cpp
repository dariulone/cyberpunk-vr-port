// A bounded workload for end-to-end validation. Never shipped to testers.
#include <windows.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <iostream>
#include <thread>
#include <vector>

using Microsoft::WRL::ComPtr;
static uint64_t ticks(FILETIME f) { return (static_cast<uint64_t>(f.dwHighDateTime) << 32) | f.dwLowDateTime; }
int main() {
    std::atomic<bool> stop = false;
    std::atomic<DWORD> busyTid = 0, idleTid = 0;
    std::atomic<uint64_t> busyTicks = 0;
    std::thread busy([&] {
        SetThreadDescription(GetCurrentThread(), L"Profiler test: busy, \"core\"");
        busyTid = GetCurrentThreadId();
        uint64_t n = 1;
        while (!stop.load(std::memory_order_relaxed)) { n = n * 6364136223846793005ULL + 1; _ReadWriteBarrier(); }
        FILETIME created{}, exited{}, kernel{}, user{};
        GetThreadTimes(GetCurrentThread(), &created, &exited, &kernel, &user);
        busyTicks = ticks(kernel) + ticks(user);
        if (n == 0) std::cerr << "unreachable value\n";
    });
    std::thread idle([&] {
        SetThreadDescription(GetCurrentThread(), L"Profiler test: sleeping");
        idleTid = GetCurrentThreadId();
        while (!stop.load(std::memory_order_relaxed)) Sleep(100);
    });
    // The profiler is already running. Discover a GPU user created later,
    // rather than only the instances that existed when its query was opened.
    Sleep(1500);
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    D3D_FEATURE_LEVEL level{};
    const auto hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, nullptr, 0,
        D3D11_SDK_VERSION, &device, &level, &context);
    std::vector<ComPtr<ID3D11Texture2D>> textures;
    std::vector<ComPtr<ID3D11RenderTargetView>> views;
    if (SUCCEEDED(hr)) {
        for (int i = 0; i < 4; ++i) {
            D3D11_TEXTURE2D_DESC desc{};
            desc.Width = 4096; desc.Height = 4096; desc.MipLevels = 1; desc.ArraySize = 1;
            desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.SampleDesc.Count = 1;
            desc.Usage = D3D11_USAGE_DEFAULT; desc.BindFlags = D3D11_BIND_RENDER_TARGET;
            ComPtr<ID3D11Texture2D> texture;
            ComPtr<ID3D11RenderTargetView> view;
            if (FAILED(device->CreateTexture2D(&desc, nullptr, &texture)) || FAILED(device->CreateRenderTargetView(texture.Get(), nullptr, &view))) break;
            textures.push_back(texture); views.push_back(view);
        }
    }
    const auto end = std::chrono::steady_clock::now() + std::chrono::seconds(6);
    unsigned frame = 0;
    while (std::chrono::steady_clock::now() < end) {
        const float color[4]{static_cast<float>(frame++ % 100) / 100.0f, 0.2f, 0.3f, 1.0f};
        for (const auto& view : views) context->ClearRenderTargetView(view.Get(), color);
        if (context) context->Flush();
        Sleep(10);
    }
    stop = true;
    busy.join(); idle.join();
    std::cout << "{\"pid\":" << GetCurrentProcessId() << ",\"busy_tid\":" << busyTid.load()
        << ",\"idle_tid\":" << idleTid.load() << ",\"busy_cpu_100ns\":" << busyTicks.load()
        << ",\"gpu_textures\":" << textures.size() << "}\n";
    return 0;
}
