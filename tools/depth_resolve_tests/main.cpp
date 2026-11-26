// The depth plane resolve for the depth layer (include/Render/DepthResolve.hpp) on a WARP device: scene depths of the
// three source families the game can hand it -- a planar D32S8 (R32G8X24_TYPELESS) and D24S8 (R24G8_TYPELESS) depth
// buffer and a plain R32_TYPELESS texture -- with a distinct depth in every texel and a stencil beside it, resolved into
// a D32_FLOAT depth target that held other values, read back and checked texel for texel (exact for a float source,
// within one 24-bit step for D24). "trace" prints a hash of every byte read back, so the C++ build and the Rust build
// can be compared bit for bit (cmake/ParityTrace.cmake).
#include "Render/DepthResolve.hpp"
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

void Log(const char* fmt, ...) { (void)fmt; }   // the C++ resolve logs through the plugin's Log

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

static uint32_t TexelBytes(DXGI_FORMAT f) {
    switch (f) {
    case DXGI_FORMAT_R8_TYPELESS: case DXGI_FORMAT_R8_UINT: return 1;
    case DXGI_FORMAT_R32G8X24_TYPELESS: return 8;
    default: return 4;
    }
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
    constexpr uint32_t W = 16, H = 16;
    const auto buffer = [&](D3D12_HEAP_TYPE type, UINT64 size) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = type;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; d.Width = size; d.Height = 1; d.DepthOrArraySize = 1;
        d.MipLevels = 1; d.SampleDesc.Count = 1; d.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d,
            type == D3D12_HEAP_TYPE_UPLOAD ? D3D12_RESOURCE_STATE_GENERIC_READ : D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&r)), "buffer");
        return r;
    };
    const auto texture = [&](DXGI_FORMAT f, D3D12_RESOURCE_FLAGS flags, D3D12_RESOURCE_STATES state, const D3D12_CLEAR_VALUE* clear) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; d.Width = W; d.Height = H; d.DepthOrArraySize = 1;
        d.MipLevels = 1; d.Format = f; d.SampleDesc.Count = 1; d.Flags = flags;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, state, clear, IID_PPV_ARGS(&r)), "texture");
        return r;
    };
    // The depth a texel holds: distinct, inside [0, 1] (SV_Depth is clamped to the viewport's range).
    const auto depthOf = [](uint32_t i) { return (float((i * 37u) % 256u) + 0.25f) / 256.0f; };

    DepthResolve resolve;
    Check(!resolve.EnsureInitialized(device.Get(), DXGI_FORMAT_D32_FLOAT, 0, H), "zero width accepted");
    Check(!resolve.EnsureInitialized(nullptr, DXGI_FORMAT_D32_FLOAT, W, H), "null device accepted");
    Check(!resolve.RecordResolve(list.Get(), nullptr, nullptr), "record before init accepted");
    Check(resolve.EnsureInitialized(device.Get(), DXGI_FORMAT_D16_UNORM, W, H), "D16 output not built");
    Check(resolve.EnsureInitialized(device.Get(), DXGI_FORMAT_D32_FLOAT, W, H), "resolve not initialised");
    Check(resolve.EnsureInitialized(device.Get(), DXGI_FORMAT_D32_FLOAT, W, H), "resolve not reused");

    struct Case { DXGI_FORMAT format; bool depthStencil; };
    const Case cases[] = {{DXGI_FORMAT_R32G8X24_TYPELESS, true}, {DXGI_FORMAT_R24G8_TYPELESS, true}, {DXGI_FORMAT_R32_TYPELESS, false}};
    uint64_t hash = 1469598103934665603ull;
    double worst = 0;
    for (const Case& c : cases) {
        auto sourceDesc = D3D12_RESOURCE_DESC{};
        auto source = texture(c.format, c.depthStencil ? D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL : D3D12_RESOURCE_FLAG_NONE,
                              D3D12_RESOURCE_STATE_COPY_DEST, nullptr);
        sourceDesc = source->GetDesc();
        const UINT planes = c.depthStencil ? 2 : 1;
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT layouts[2]{}; UINT rows[2]{}; UINT64 rowBytes[2]{}, total = 0;
        device->GetCopyableFootprints(&sourceDesc, 0, planes, 0, layouts, rows, rowBytes, &total);
        auto upload = buffer(D3D12_HEAP_TYPE_UPLOAD, total);
        {
            uint8_t* p = nullptr; Hr(upload->Map(0, nullptr, reinterpret_cast<void**>(&p)), "map upload");
            std::memset(p, 0, size_t(total));
            for (UINT plane = 0; plane < planes; ++plane) {
                const auto& fp = layouts[plane];
                const uint32_t bytes = TexelBytes(fp.Footprint.Format);
                for (uint32_t y = 0; y < H; ++y) for (uint32_t x = 0; x < W; ++x) {
                    uint8_t* t = p + fp.Offset + y * fp.Footprint.RowPitch + x * bytes;
                    const uint32_t i = y * W + x;
                    if (plane == 1 || bytes == 1) { t[0] = uint8_t(i ^ 0xA5); continue; }
                    if (c.format == DXGI_FORMAT_R24G8_TYPELESS) {
                        const uint32_t unorm = uint32_t(std::lround(double(depthOf(i)) * 16777215.0)) | (uint32_t(i ^ 0x3C) << 24);
                        std::memcpy(t, &unorm, 4);
                    } else {
                        const float d = depthOf(i); std::memcpy(t, &d, 4);
                        if (bytes == 8) { t[4] = uint8_t(i ^ 0xA5); }
                    }
                }
            }
            upload->Unmap(0, nullptr);
        }
        D3D12_CLEAR_VALUE clear{}; clear.Format = DXGI_FORMAT_D32_FLOAT; clear.DepthStencil.Depth = 0.75f;
        auto target = texture(DXGI_FORMAT_D32_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL, D3D12_RESOURCE_STATE_DEPTH_WRITE, &clear);
        const auto targetDesc = target->GetDesc();
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT out{}; UINT outRows = 0; UINT64 outRowBytes = 0, outTotal = 0;
        device->GetCopyableFootprints(&targetDesc, 0, 1, 0, &out, &outRows, &outRowBytes, &outTotal);
        auto readback = buffer(D3D12_HEAP_TYPE_READBACK, outTotal);

        Hr(allocator->Reset(), "allocator reset"); Hr(list->Reset(allocator.Get(), nullptr), "list reset");
        for (UINT plane = 0; plane < planes; ++plane) {
            D3D12_TEXTURE_COPY_LOCATION dst{}, src{};
            src.pResource = upload.Get(); src.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; src.PlacedFootprint = layouts[plane];
            dst.pResource = source.Get(); dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX; dst.SubresourceIndex = plane;
            list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        }
        // The target held another depth (cleared to 0.75): the resolve must overwrite every texel.
        D3D12_DESCRIPTOR_HEAP_DESC dh{}; dh.Type = D3D12_DESCRIPTOR_HEAP_TYPE_DSV; dh.NumDescriptors = 1;
        ComPtr<ID3D12DescriptorHeap> clearHeap; Hr(device->CreateDescriptorHeap(&dh, IID_PPV_ARGS(&clearHeap)), "clear heap");
        device->CreateDepthStencilView(target.Get(), nullptr, clearHeap->GetCPUDescriptorHandleForHeapStart());
        list->ClearDepthStencilView(clearHeap->GetCPUDescriptorHandleForHeapStart(), D3D12_CLEAR_FLAG_DEPTH, 0.75f, 0, 0, nullptr);
        auto b = Transition(source.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
        list->ResourceBarrier(1, &b);
        Check(resolve.RecordResolve(list.Get(), source.Get(), target.Get()), "record failed");
        b = Transition(target.Get(), D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->ResourceBarrier(1, &b);
        D3D12_TEXTURE_COPY_LOCATION rd{}, rs{};
        rd.pResource = readback.Get(); rd.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; rd.PlacedFootprint = out;
        rs.pResource = target.Get(); rs.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&rd, 0, 0, 0, &rs, nullptr);
        Hr(list->Close(), "close");
        ID3D12CommandList* lists[]{list.Get()};
        queue->ExecuteCommandLists(1, lists);
        wait();
        const uint8_t* p = nullptr; Hr(readback->Map(0, nullptr, reinterpret_cast<void**>(const_cast<uint8_t**>(&p))), "map readback");
        for (uint32_t y = 0; y < H; ++y) for (uint32_t x = 0; x < W; ++x) {
            const uint8_t* t = p + out.Offset + y * out.Footprint.RowPitch + x * 4;
            for (int k = 0; k < 4; ++k) { hash ^= t[k]; hash *= 1099511628211ull; }
            float got = 0; std::memcpy(&got, t, 4);
            const uint32_t i = y * W + x;
            if (c.format == DXGI_FORMAT_R24G8_TYPELESS) {
                const double expected = std::lround(double(depthOf(i)) * 16777215.0) / 16777215.0;
                const double step = std::abs(double(got) - expected) * 16777215.0;
                worst = (std::max)(worst, step);
            } else {
                Check(got == depthOf(i), "float depth not resolved exactly");
            }
        }
        readback->Unmap(0, nullptr);
    }
    if (trace) { std::printf("depth_resolve trace %016llx\n", static_cast<unsigned long long>(hash)); return 0; }
    Check(worst <= 1.0, "D24 depth off by more than one step");
    resolve.Shutdown();
    Check(!resolve.RecordResolve(list.Get(), nullptr, nullptr), "record after shutdown");
    std::printf("PASS D32S8/D24S8/R32 sources resolved into D32 (float exact, D24 within %.3f step), reuse, shutdown\n", worst);
    return 0;
} catch (const std::exception& e) { std::fprintf(stderr, "%s\n", e.what()); return 1; }
