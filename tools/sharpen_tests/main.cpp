// CAS sharpening (include/Render/SharpenPass.hpp) on a WARP device with the debug layer: 30 draws recorded before any
// execution (typeless/sRGB targets, in place and into another target, resized), checked against a scalar CAS reference.
// "trace" prints a hash of every output byte instead, so the C++ build and the Rust build can be compared bit for bit
// (cmake/ParityTrace.cmake).
#include "Render/SharpenPass.hpp"
#include "Render/CommandResources.hpp"
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <stdexcept>
#include <vector>

using Microsoft::WRL::ComPtr;
static bool g_trace = false;   // the trace prints only its hash (the Rust pass logs elsewhere)
void Log(const char* fmt, ...) { if (g_trace) return; va_list args; va_start(args, fmt); vprintf(fmt, args); va_end(args); }
void Check(bool ok, const char* message) { if (!ok) throw std::runtime_error(message); }
void Hr(HRESULT result) { Check(SUCCEEDED(result), "D3D12 call failed"); }
void Transition(ID3D12GraphicsCommandList* list, ID3D12Resource* resource,
                D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after};
    list->ResourceBarrier(1, &barrier);
}

struct Image {
    unsigned width{}, height{};
    std::vector<uint8_t> pixels;
    ComPtr<ID3D12Resource> source, target, upload, readback;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
    float emphasis{}, amount{};
};

// Scalar CAS reference. Amount=1 passes filterParam.y=4, with final saturation.
uint8_t Reference(const Image& image, int x, int y, int channel) {
    auto tap = [&](int dx, int dy) {
        const int ix = std::clamp(x + dx, 0, int(image.width) - 1);
        const int iy = std::clamp(y + dy, 0, int(image.height) - 1);
        return image.pixels[(iy * image.width + ix) * 4 + channel] / 255.0f;
    };
    const float center = tap(0, 0);
    float crossMin = center, crossMax = center, allMin = center, allMax = center, sum = 0;
    for (int dy = -1; dy <= 1; ++dy) for (int dx = -1; dx <= 1; ++dx) {
        const float value = tap(dx, dy);
        allMin = std::min(allMin, value); allMax = std::max(allMax, value);
        if (std::abs(dx) + std::abs(dy) == 1) {
            crossMin = std::min(crossMin, value); crossMax = std::max(crossMax, value); sum += value;
        }
    }
    const float minimum = crossMin + allMin, maximum = crossMax + allMax;
    const float amp = std::sqrt(std::clamp(std::min(minimum, 2 - maximum) / std::max(maximum, 1e-5f), 0.0f, 1.0f));
    const float weight = -amp / (8 - 3 * image.emphasis);
    const float cas = std::clamp((center + weight * sum) / (1 + 4 * weight), 0.0f, 1.0f);
    return uint8_t(std::lround(255 * std::clamp(center + 4 * image.amount * (cas - center), 0.0f, 1.0f)));
}

int main(int argc, char** argv) try {
    g_trace = argc > 1 && std::strcmp(argv[1], "trace") == 0;
    // The debug layer (Windows' optional "Graphics Tools") validates every call when installed; without it the pixels
    // are still checked.
    ComPtr<ID3D12Debug> debug;
    const bool validated = SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)));
    if (validated) debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device; Hr(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)));
    ComPtr<ID3D12InfoQueue> info; if (validated) Hr(device.As(&info));
    D3D12_COMMAND_QUEUE_DESC qdesc{}; qdesc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ComPtr<ID3D12CommandQueue> queue; Hr(device->CreateCommandQueue(&qdesc, IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12Fence> fence; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)));
    ComPtr<ID3D12CommandAllocator> allocator; Hr(device->CreateCommandAllocator(qdesc.Type, IID_PPV_ARGS(&allocator)));
    ComPtr<ID3D12GraphicsCommandList> list; Hr(device->CreateCommandList(0, qdesc.Type, allocator.Get(), nullptr, IID_PPV_ARGS(&list)));
    auto buffer = [&](uint64_t size, D3D12_HEAP_TYPE type, D3D12_RESOURCE_STATES state) {
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = type;
        D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        desc.Width = size; desc.Height = 1; desc.DepthOrArraySize = 1; desc.MipLevels = 1;
        desc.SampleDesc.Count = 1; desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> result;
        Hr(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, nullptr, IID_PPV_ARGS(&result)));
        return result;
    };

    SharpenPass pass;
    std::vector<Image> images;
    // Multiple eyes and varying constants are all recorded BEFORE any execution.
    // Changing dimensions also retires scratch textures while their draws wait.
    for (unsigned draw = 0; draw < 30; ++draw) {
        const bool inPlace = (draw % 2) != 0;
        Image image; image.width = draw < 12 ? 17 : 23; image.height = 11;
        image.amount = float(draw % 3) * 0.5f; image.emphasis = draw % 2 ? 1.0f : 0.0f;
        image.pixels.resize(image.width * image.height * 4);
        for (unsigned y = 0; y < image.height; ++y) for (unsigned x = 0; x < image.width; ++x) {
            auto* pixel = image.pixels.data() + (y * image.width + x) * 4;
            pixel[0] = uint8_t((x * 11 + y * 7 + draw * 17) % 256);
            pixel[1] = x < image.width / 2 ? 90 : 155;
            pixel[2] = draw < 3 ? 0 : (draw < 6 ? 255 : 127); pixel[3] = 255;
        }
        D3D12_RESOURCE_DESC desc{};
        desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; desc.Width = image.width; desc.Height = image.height;
        desc.DepthOrArraySize = 1; desc.MipLevels = 1; desc.SampleDesc.Count = 1;
        desc.Format = DXGI_FORMAT_R8G8B8A8_TYPELESS;
        if (inPlace) desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
        Hr(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&image.source)));
        desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        if (draw >= 24) desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
        Hr(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_RENDER_TARGET, nullptr, IID_PPV_ARGS(&image.target)));
        UINT64 bytes{}; device->GetCopyableFootprints(&desc, 0, 1, 0, &image.footprint, nullptr, nullptr, &bytes);
        image.upload = buffer(bytes, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
        image.readback = buffer(bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
        void* mapped{}; Hr(image.upload->Map(0, nullptr, &mapped));
        for (unsigned y = 0; y < image.height; ++y)
            std::memcpy(static_cast<uint8_t*>(mapped) + y * image.footprint.Footprint.RowPitch,
                        image.pixels.data() + y * image.width * 4, image.width * 4);
        image.upload->Unmap(0, nullptr);
        D3D12_TEXTURE_COPY_LOCATION upload{}, source{}, target{}, readback{};
        upload.pResource = image.upload.Get(); upload.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; upload.PlacedFootprint = image.footprint;
        source.pResource = image.source.Get(); source.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        target.pResource = inPlace ? image.source.Get() : image.target.Get(); target.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        readback.pResource = image.readback.Get(); readback.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; readback.PlacedFootprint = image.footprint;
        list->CopyTextureRegion(&source, 0, 0, 0, &upload, nullptr);
        Transition(list.Get(), image.source.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
        Check(pass.EnsureInitialized(device.Get(), desc.Format, image.width, image.height), "sharpen init");
        Check(pass.RecordSharpen(list.Get(), image.source.Get(), target.pResource, image.emphasis, image.amount), "sharpen draw");
        if (!inPlace) Transition(list.Get(), image.target.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->CopyTextureRegion(&readback, 0, 0, 0, &target, nullptr);
        images.push_back(std::move(image));
    }
    // Command ownership must outlive the helper, including PSO and descriptor heap.
    pass.Shutdown();
    Hr(list->Close()); ID3D12CommandList* commands[]{list.Get()};
    auto resources = cvr::gpu::PrepareCommandResources(1, commands);
    queue->ExecuteCommandLists(1, commands); cvr::gpu::SubmitCommandResources(queue.Get(), std::move(resources));
    Hr(queue->Signal(fence.Get(), 1));
    const auto event = CreateEventW(nullptr, FALSE, FALSE, nullptr); Check(event != nullptr, "fence event");
    Hr(fence->SetEventOnCompletion(1, event));
    const auto completed = WaitForSingleObject(event, 10000); CloseHandle(event); Check(completed == WAIT_OBJECT_0, "GPU timeout");

    unsigned changed = 0; int worstError = 0;
    uint64_t hash = 1469598103934665603ull;
    for (const auto& image : images) {
        void* mapped{}; Hr(image.readback->Map(0, nullptr, &mapped));
        for (unsigned y = 0; y < image.height; ++y) for (unsigned x = 0; x < image.width; ++x) {
            const auto* pixel = static_cast<uint8_t*>(mapped) + y * image.footprint.Footprint.RowPitch + x * 4;
            for (int k = 0; k < 4; ++k) { hash ^= pixel[k]; hash *= 1099511628211ull; }
            for (int channel = 0; channel < 3; ++channel) {
                const auto expected = Reference(image, x, y, channel);
                const int error = std::abs(int(pixel[channel]) - expected); worstError = std::max(worstError, error);
                Check(error <= 1, "pixel differs from CAS reference");
                changed += std::abs(int(pixel[channel]) - image.pixels[(y * image.width + x) * 4 + channel]) > 1;
            }
            Check(pixel[3] == 255, "alpha changed");
        }
        image.readback->Unmap(0, nullptr);
    }
    Check(changed > 0, "CAS sharpening did not affect pixels");
    for (UINT64 i = 0; info && i < info->GetNumStoredMessagesAllowedByRetrievalFilter(); ++i) {
        SIZE_T bytes{}; info->GetMessage(i, nullptr, &bytes); std::vector<uint8_t> storage(bytes);
        auto* message = reinterpret_cast<D3D12_MESSAGE*>(storage.data()); Hr(info->GetMessage(i, message, &bytes));
        if (message->Severity <= D3D12_MESSAGE_SEVERITY_WARNING) {
            std::cerr << message->pDescription << '\n'; throw std::runtime_error("D3D12 validation warning/error");
        }
    }
    list.Reset(); cvr::gpu::CollectCommandResources();
    if (g_trace) { std::printf("sharpen trace %016llx\n", static_cast<unsigned long long>(hash)); return 0; }
    std::cout << "PASS 30 queued draws, per-eye inputs/constants, resize/shutdown lifetime, typeless/sRGB views, CAS Sharpening 0/0.5/1, max byte error " << worstError
              << (validated ? ", debug layer clean" : ", no debug layer installed (not validated)") << '\n';
    return 0;
} catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
