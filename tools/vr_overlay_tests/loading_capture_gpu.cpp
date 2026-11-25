#include "Render/LoadingCapture.hpp"
#include "Render/ColorBlit.hpp"
#include "Core/Paths.hpp"
#include <windows.h>
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <wrl.h>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>

using Microsoft::WRL::ComPtr;
void Log(const char*, ...) {}
namespace cvr::paths {
const std::wstring& Dir(Folder) {
    static const auto path = (std::filesystem::current_path() /
        (L"loading-capture-test-" + std::to_wstring(GetCurrentProcessId()))).wstring() + L"\\";
    return path;
}
std::wstring File(Folder f, const wchar_t* name) { return Dir(f) + name; }
}
void Check(bool ok, const char* text) { if (!ok) throw std::runtime_error(text); }
void Hr(HRESULT hr) { Check(SUCCEEDED(hr), "D3D12 call failed"); }
void ImageIs(const std::filesystem::path& path, unsigned char r, unsigned char g, unsigned char b,
             bool bordered = false) {
    std::ifstream in(path, std::ios::binary);
    std::string magic; unsigned width{}, height{}, maximum{};
    in >> magic >> width >> height >> maximum; in.get();
    Check(magic == "P6" && width == 64 && height == 32 && maximum == 255, "bad thumbnail header");
    for (unsigned i = 0; i < width * height; ++i) {
        unsigned char rgb[3]{}; in.read(reinterpret_cast<char*>(rgb), 3);
        const auto x = i % width;
        const bool border = bordered && (x < 16 || x >= 48);
        Check(in.good() && rgb[0] == (border ? 255 : r) && rgb[1] == g && rgb[2] == (border ? 0 : b),
              "captured wrong pixels");
    }
}
int main() try {
    ComPtr<ID3D12Debug> debug;
    if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device; Hr(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)));
    ComPtr<ID3D12InfoQueue> errors; device.As(&errors);
    ComPtr<ID3D12CommandQueue> queue;
    D3D12_COMMAND_QUEUE_DESC q{}; Hr(device->CreateCommandQueue(&q, IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12CommandAllocator> allocator; Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator)));
    ComPtr<ID3D12GraphicsCommandList> list;
    Hr(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator.Get(), nullptr, IID_PPV_ARGS(&list)));
    ComPtr<ID3D12Fence> fence; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)));
    ComPtr<ID3D12DescriptorHeap> rtvs;
    D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_RTV, 2};
    Hr(device->CreateDescriptorHeap(&heap, IID_PPV_ARGS(&rtvs)));
    ComPtr<ID3D12Resource> window, eye;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = 64; desc.Height = 32; desc.DepthOrArraySize = 1; desc.MipLevels = 1;
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.SampleDesc.Count = 1; desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    D3D12_HEAP_PROPERTIES memory{}; memory.Type = D3D12_HEAP_TYPE_DEFAULT;
    Hr(device->CreateCommittedResource(&memory, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_RENDER_TARGET, nullptr, IID_PPV_ARGS(&window)));
    Hr(device->CreateCommittedResource(&memory, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_RENDER_TARGET, nullptr, IID_PPV_ARGS(&eye)));
    auto rtv = rtvs->GetCPUDescriptorHandleForHeapStart();
    const float red[]{1,0,0,1}, blue[]{0,0,1,1};
    device->CreateRenderTargetView(window.Get(), nullptr, rtv); list->ClearRenderTargetView(rtv, red, 0, nullptr);
    const D3D12_RECT artwork{16, 0, 48, 32};
    list->ClearRenderTargetView(rtv, blue, 1, &artwork);
    rtv.ptr += device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    device->CreateRenderTargetView(eye.Get(), nullptr, rtv); list->ClearRenderTargetView(rtv, red, 0, nullptr);
    D3D12_RESOURCE_BARRIER barrier[2]{};
    for (auto& b : barrier) b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier[0].Transition = {window.Get(), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE};
    list->ResourceBarrier(1, barrier);
    ColorBlit blit;
    Check(blit.EnsureInitialized(device.Get(), desc.Format, 64, 32), "loading-panel blit init failed");
    // As in the game, stretch only the artwork across the packed eye texture.
    // The half-texel inset here keeps linear filtering inside the solid blue test patch.
    Check(blit.RecordBlit(list.Get(), window.Get(), eye.Get(), 16.5f/64, 0, 47.5f/64, 1), "artwork blit failed");
    barrier[0].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barrier[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    barrier[1].Transition = {eye.Get(), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE};
    list->ResourceBarrier(2, barrier);
    using namespace cvr::loading::diagnostic;
    const auto root = std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Log));
    // No request: a loading frame must not create output or allocate a recording.
    Record(device.Get(), list.Get(), window.Get(), eye.Get(), fence.Get(), 1, {1,true,true,false,1});
    Check(!std::filesystem::exists(root), "unarmed recorder wrote files");
    Shutdown(); // reset the one-second polling deadline for this test
    std::filesystem::create_directories(root);
    { std::ofstream request(root / "loading-capture.request"); }
    Record(device.Get(), list.Get(), window.Get(), eye.Get(), fence.Get(), 1, {2,false,false,true,1});
    Check(!std::filesystem::exists(root / "loading-capture.request"), "request was not consumed");
    const auto directory = std::filesystem::directory_iterator(root / "loading-capture")->path();
    Check(std::filesystem::is_empty(directory), "recording started before a load");
    Record(device.Get(), list.Get(), window.Get(), eye.Get(), fence.Get(), 1, {3,true,true,false,1});
    Hr(list->Close());
    Shutdown(); // must retain pending GPU write targets without reading/freeing them
    Check(std::filesystem::is_empty(directory), "readback mapped before its fence completed");
    ID3D12CommandList* lists[]{list.Get()}; queue->ExecuteCommandLists(1, lists); Hr(queue->Signal(fence.Get(), 1));
    HANDLE done = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    Check(done != nullptr, "event allocation failed"); Hr(fence->SetEventOnCompletion(1, done));
    Check(WaitForSingleObject(done, 10000) == WAIT_OBJECT_0, "GPU timeout"); CloseHandle(done);
    Record(nullptr, nullptr, nullptr, nullptr, nullptr, 0, {4,false,false,true,1});
    ImageIs(directory / "000-window.ppm", 0, 0, 255, true);
    ImageIs(directory / "000-eye.ppm", 0, 0, 255);
    std::ifstream metadata(directory / "frames.jsonl");
    const std::string text{std::istreambuf_iterator<char>(metadata), {}};
    Check(text.find("\"serial\":3") != std::string::npos && text.find("\"saved\":1") != std::string::npos, "metadata mismatch");
    if (errors) for (UINT64 i = 0; i < errors->GetNumStoredMessages(); ++i) {
        SIZE_T size{}; errors->GetMessage(i, nullptr, &size);
        auto data = std::make_unique<unsigned char[]>(size); auto* m = reinterpret_cast<D3D12_MESSAGE*>(data.get());
        Hr(errors->GetMessage(i, m, &size));
        if (m->Severity <= D3D12_MESSAGE_SEVERITY_ERROR) throw std::runtime_error(m->pDescription);
    }
    Shutdown();
    std::cout << "Loading pixels: opt-in, deferred start, fence ownership, shutdown and paired thumbnails passed\n";
    return 0;
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
