// The sRGB decode into an eye swapchain (include/Render/SrgbToLinearPass.hpp) on a WARP device: an 8-bit source with
// every byte value is drawn into an 8-bit UNORM target through the pass, read back and checked against the sRGB curve
// (within one step), for the UNORM (decoded by hand) and _SRGB (decoded by the view) sources of both channel orders.
// "trace" prints a hash of every output byte, so the C++ build and the Rust build can be compared bit for bit
// (cmake/ParityTrace.cmake).
#include "Render/SrgbToLinearPass.hpp"
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

void Log(const char* fmt, ...) { (void)fmt; }   // the C++ pass logs through the plugin's Log

using Microsoft::WRL::ComPtr;
static void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
static void Hr(HRESULT hr, const char* text) { if (FAILED(hr)) throw std::runtime_error(text); }

static D3D12_RESOURCE_BARRIER Transition(ID3D12Resource* r, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER b{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition.pResource = r;
    b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b.Transition.StateBefore = before;
    b.Transition.StateAfter = after;
    return b;
}

int main(int argc, char** argv) try {
    const bool trace = argc > 1 && std::string(argv[1]) == "trace";
    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "factory");
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)), "warp");
    ComPtr<ID3D12Device> device; Hr(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)), "device");
    D3D12_COMMAND_QUEUE_DESC qd{}; qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ComPtr<ID3D12CommandQueue> queue; Hr(device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue)), "queue");
    ComPtr<ID3D12CommandAllocator> allocator; Hr(device->CreateCommandAllocator(qd.Type, IID_PPV_ARGS(&allocator)), "allocator");
    ComPtr<ID3D12GraphicsCommandList> list; Hr(device->CreateCommandList(0, qd.Type, allocator.Get(), nullptr, IID_PPV_ARGS(&list)), "list");
    Hr(list->Close(), "close");
    ComPtr<ID3D12Fence> fence; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "fence");
    uint64_t fenceValue = 0;
    const auto wait = [&] {
        Hr(queue->Signal(fence.Get(), ++fenceValue), "signal");
        const HANDLE e = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        Hr(fence->SetEventOnCompletion(fenceValue, e), "event");
        Check(WaitForSingleObject(e, 10000) == WAIT_OBJECT_0, "GPU timeout"); CloseHandle(e);
    };
    constexpr uint32_t W = 16, H = 16;   // 256 texels: every byte value once per channel
    const auto buffer = [&](D3D12_HEAP_TYPE type, UINT64 size) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = type;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; d.Width = size; d.Height = 1; d.DepthOrArraySize = 1;
        d.MipLevels = 1; d.SampleDesc.Count = 1; d.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d,
            type == D3D12_HEAP_TYPE_UPLOAD ? D3D12_RESOURCE_STATE_GENERIC_READ : D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&r)), "buffer");
        return r;
    };
    const auto texture = [&](DXGI_FORMAT f, D3D12_RESOURCE_FLAGS flags, D3D12_RESOURCE_STATES state) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; d.Width = W; d.Height = H; d.DepthOrArraySize = 1;
        d.MipLevels = 1; d.Format = f; d.SampleDesc.Count = 1; d.Flags = flags;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, state, nullptr, IID_PPV_ARGS(&r)), "texture");
        return r;
    };
    constexpr UINT pitch = 256;   // D3D12_TEXTURE_DATA_PITCH_ALIGNMENT
    auto upload = buffer(D3D12_HEAP_TYPE_UPLOAD, pitch * H);
    {
        uint8_t* p = nullptr; Hr(upload->Map(0, nullptr, reinterpret_cast<void**>(&p)), "map upload");
        for (uint32_t y = 0; y < H; ++y) for (uint32_t x = 0; x < W; ++x) {
            const uint8_t v = uint8_t(y * W + x);
            uint8_t* t = p + y * pitch + x * 4; t[0] = v; t[1] = uint8_t(255 - v); t[2] = uint8_t(v * 7); t[3] = uint8_t(v ^ 0x5A);
        }
        upload->Unmap(0, nullptr);
    }
    auto readback = buffer(D3D12_HEAP_TYPE_READBACK, pitch * H);
    SrgbToLinearPass pass;
    Check(pass.EnsureInitialized(device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, W, H), "pass not initialised");
    Check(pass.EnsureInitialized(device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, W, H), "pass not reused");
    Check(!pass.EnsureInitialized(device.Get(), DXGI_FORMAT_UNKNOWN, W, H), "unknown format accepted");
    Check(pass.EnsureInitialized(device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, W, H), "pass not rebuilt");
    uint64_t hash = 1469598103934665603ull;
    const DXGI_FORMAT sources[] = {DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_B8G8R8A8_UNORM,
                                   DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_TYPELESS};
    int worst = 0;
    for (const DXGI_FORMAT format : sources) {
        auto source = texture(format, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST);
        auto target = texture(DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_STATE_RENDER_TARGET);
        Hr(allocator->Reset(), "allocator reset"); Hr(list->Reset(allocator.Get(), nullptr), "list reset");
        D3D12_TEXTURE_COPY_LOCATION dst{}, src{};
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{}; fp.Footprint = {format, W, H, 1, pitch};
        src.pResource = upload.Get(); src.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; src.PlacedFootprint = fp;
        dst.pResource = source.Get(); dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        auto b = Transition(source.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
        list->ResourceBarrier(1, &b);
        Check(pass.Record(list.Get(), source.Get(), target.Get(), 3), "record failed");
        Check(!pass.Record(list.Get(), source.Get(), target.Get(), 6), "descriptor slot 6 accepted");
        b = Transition(target.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->ResourceBarrier(1, &b);
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT out{}; out.Footprint = {DXGI_FORMAT_R8G8B8A8_UNORM, W, H, 1, pitch};
        D3D12_TEXTURE_COPY_LOCATION rd{}, rs{};
        rd.pResource = readback.Get(); rd.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; rd.PlacedFootprint = out;
        rs.pResource = target.Get(); rs.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&rd, 0, 0, 0, &rs, nullptr);
        Hr(list->Close(), "close");
        ID3D12CommandList* lists[]{list.Get()};
        queue->ExecuteCommandLists(1, lists);
        wait();
        const uint8_t* p = nullptr; Hr(readback->Map(0, nullptr, reinterpret_cast<void**>(const_cast<uint8_t**>(&p))), "map readback");
        const bool bgra = format == DXGI_FORMAT_B8G8R8A8_UNORM || format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB;
        for (uint32_t y = 0; y < H; ++y) for (uint32_t x = 0; x < W; ++x) {
            const uint8_t* t = p + y * pitch + x * 4;
            for (int k = 0; k < 4; ++k) { hash ^= t[k]; hash *= 1099511628211ull; }
            const uint8_t v = uint8_t(y * W + x);
            const uint8_t in[4] = {v, uint8_t(255 - v), uint8_t(v * 7), uint8_t(v ^ 0x5A)};
            for (int k = 0; k < 3; ++k) {
                // The texel's own channel k (BGRA swaps red and blue when the view reads it).
                const uint8_t e = bgra ? in[k == 0 ? 2 : k == 2 ? 0 : 1] : in[k];
                const double c = e / 255.0;
                const double lin = c <= 0.04045 ? c / 12.92 : std::pow((c + 0.055) / 1.055, 2.4);
                const int expected = int(std::lround(lin * 255.0));
                worst = (std::max)(worst, std::abs(int(t[k]) - expected));
            }
            Check(t[3] == in[3], "alpha changed");
        }
        readback->Unmap(0, nullptr);
    }
    if (trace) { std::printf("srgb_pass trace %016llx\n", static_cast<unsigned long long>(hash)); return 0; }
    Check(worst <= 1, "decoded value off the sRGB curve");
    pass.Shutdown();
    Check(!pass.Record(list.Get(), nullptr, nullptr, 0), "record after shutdown");
    std::printf("PASS UNORM/SRGB x RGBA/BGRA/TYPELESS decoded within %d step(s), slots bounded, shutdown\n", worst);
    return 0;
} catch (const std::exception& e) { std::fprintf(stderr, "%s\n", e.what()); return 1; }
