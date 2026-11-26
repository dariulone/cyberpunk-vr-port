// Pictures hung in the tracking space (include/Render/SpaceQuad.hpp, cvr::render::SpaceQuads) on a WARP device: a
// picture's mip chain built level by level with Resample, then one eye's image -- filled, a solid quad over it, the
// picture on a quad turned away from the eye (read through its mips, anisotropically) blended Over, and again as Light
// with a black point -- read back and checked where the answer is known (the fill where no quad reaches, every quad
// changing the middle), the refusals (an empty rectangle, no picture), and a draw after Shutdown (D3D12 hands one
// adapter's second device back as the first, so a second device cannot be refused here). "trace" prints a hash of
// every byte read back (the eye and the picture's levels), so the C++ build and the Rust build can be compared bit for
// bit (cmake/ParityTrace.cmake).
#include "Render/SpaceQuad.hpp"
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

void Log(const char* fmt, ...) { (void)fmt; }   // the C++ drawer logs through the plugin's Log

using Microsoft::WRL::ComPtr;
static void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
static void Hr(HRESULT hr, const char* text) { if (FAILED(hr)) throw std::runtime_error(text); }

static D3D12_RESOURCE_BARRIER Transition(ID3D12Resource* r, UINT sub, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER b{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition.pResource = r;
    b.Transition.Subresource = sub;
    b.Transition.StateBefore = before;
    b.Transition.StateAfter = after;
    return b;
}

struct Gpu {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Fence> fence;
    uint64_t value = 0;
    explicit Gpu(IDXGIAdapter* adapter) {
        Hr(D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)), "device");
        D3D12_COMMAND_QUEUE_DESC qd{}; qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        Hr(device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue)), "queue");
        Hr(device->CreateCommandAllocator(qd.Type, IID_PPV_ARGS(&allocator)), "allocator");
        Hr(device->CreateCommandList(0, qd.Type, allocator.Get(), nullptr, IID_PPV_ARGS(&list)), "list");
        Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "fence");
    }
    void Run() {
        Hr(list->Close(), "close");
        ID3D12CommandList* lists[]{list.Get()};
        queue->ExecuteCommandLists(1, lists);
        Hr(queue->Signal(fence.Get(), ++value), "signal");
        const HANDLE e = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        Hr(fence->SetEventOnCompletion(value, e), "event");
        Check(WaitForSingleObject(e, 10000) == WAIT_OBJECT_0, "GPU timeout"); CloseHandle(e);
        Hr(allocator->Reset(), "allocator reset"); Hr(list->Reset(allocator.Get(), nullptr), "list reset");
    }
    ComPtr<ID3D12Resource> Buffer(D3D12_HEAP_TYPE type, UINT64 size) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = type;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; d.Width = size; d.Height = 1; d.DepthOrArraySize = 1;
        d.MipLevels = 1; d.SampleDesc.Count = 1; d.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d,
            type == D3D12_HEAP_TYPE_UPLOAD ? D3D12_RESOURCE_STATE_GENERIC_READ : D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&r)), "buffer");
        return r;
    }
    ComPtr<ID3D12Resource> Texture(UINT w, UINT h, UINT16 mips, D3D12_RESOURCE_STATES state) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; d.Width = w; d.Height = h; d.DepthOrArraySize = 1;
        d.MipLevels = mips; d.Format = DXGI_FORMAT_R8G8B8A8_TYPELESS; d.SampleDesc.Count = 1; d.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, state, nullptr, IID_PPV_ARGS(&r)), "texture");
        return r;
    }
};

int main(int argc, char** argv) try {
    const bool trace = argc > 1 && std::string(argv[1]) == "trace";
    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "factory");
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)), "warp");
    Gpu gpu(adapter.Get());
    auto* device = gpu.device.Get();
    auto* list = gpu.list.Get();
    constexpr UINT PW = 32, PH = 32, LEVELS = 6, EW = 64, EH = 64;
    constexpr DXGI_FORMAT UNORM = DXGI_FORMAT_R8G8B8A8_UNORM;

    // The picture: a level 0 pattern uploaded, the levels below resampled from it one by one.
    auto picture = gpu.Texture(PW, PH, LEVELS, D3D12_RESOURCE_STATE_COPY_DEST);
    const auto pictureDesc = picture->GetDesc();
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT levels[LEVELS]{}; UINT rows[LEVELS]{}; UINT64 rowBytes[LEVELS]{}, total = 0;
    device->GetCopyableFootprints(&pictureDesc, 0, LEVELS, 0, levels, rows, rowBytes, &total);
    auto upload = gpu.Buffer(D3D12_HEAP_TYPE_UPLOAD, total);
    {
        uint8_t* p = nullptr; Hr(upload->Map(0, nullptr, reinterpret_cast<void**>(&p)), "map upload");
        for (UINT y = 0; y < PH; ++y) for (UINT x = 0; x < PW; ++x) {
            uint8_t* t = p + levels[0].Offset + y * levels[0].Footprint.RowPitch + x * 4;
            const bool check = ((x / 4) ^ (y / 4)) & 1;
            t[0] = uint8_t(x * 8); t[1] = uint8_t(y * 8); t[2] = check ? 230 : 20; t[3] = 255;
        }
        upload->Unmap(0, nullptr);
    }
    D3D12_TEXTURE_COPY_LOCATION dst{}, src{};
    src.pResource = upload.Get(); src.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; src.PlacedFootprint = levels[0];
    src.PlacedFootprint.Footprint.Format = DXGI_FORMAT_R8G8B8A8_TYPELESS;
    dst.pResource = picture.Get(); dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
    auto b = Transition(picture.Get(), 0, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
    list->ResourceBarrier(1, &b);
    cvr::render::SpaceQuads quads;
    const float whole[4] = {0, 0, 1, 1};
    for (UINT mip = 1; mip < LEVELS; ++mip) {
        b = Transition(picture.Get(), mip, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_RENDER_TARGET);
        list->ResourceBarrier(1, &b);
        const cvr::render::SpaceQuads::Target level{picture.Get(), UNORM, {0, 0, LONG(PW >> mip), LONG(PH >> mip)}, mip};
        Check(quads.Resample(device, list, level, picture.Get(), UNORM, mip - 1, whole), "resample refused");
        b = Transition(picture.Get(), mip, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
        list->ResourceBarrier(1, &b);
    }

    // One eye: looking down -Z from the origin, 90 x 90 degrees.
    auto eyeImage = gpu.Texture(EW, EH, 1, D3D12_RESOURCE_STATE_RENDER_TARGET);
    const cvr::render::SpaceQuads::Target eye{eyeImage.Get(), UNORM, {0, 0, LONG(EW), LONG(EH)}, 0};
    const XrPosef head{{0, 0, 0, 1}, {0, 0, 0}};
    const XrFovf fov{-0.7853982f, 0.7853982f, 0.7853982f, -0.7853982f};
    const float dark[4] = {0.1f, 0.2f, 0.3f, 1.0f}, red[4] = {0.9f, 0.1f, 0.1f, 0.5f};
    Check(quads.Fill(device, list, eye, dark), "fill refused");
    cvr::render::SpaceQuad solid;
    solid.pose = {{0, 0, 0, 1}, {-0.4f, 0.3f, -2.0f}}; solid.width = 1.2f; solid.height = 0.8f;
    Check(quads.Solid(device, list, eye, head, fov, solid, red), "solid refused");
    // Turned 50 degrees about Y: read through its mips by each pixel's footprint.
    cvr::render::SpaceQuad turned;
    const float half = 0.4363323f;
    turned.pose = {{0, std::sin(half), 0, std::cos(half)}, {0.3f, -0.2f, -2.5f}}; turned.width = 2.0f; turned.height = 1.2f;
    turned.uv[0] = 0.1f; turned.uv[1] = 0.05f; turned.uv[2] = 0.9f; turned.uv[3] = 0.95f;
    Check(quads.Draw(device, list, eye, head, fov, picture.Get(), UNORM, turned, cvr::render::Blend::Over, 0.8f), "over refused");
    cvr::render::SpaceQuad light = turned;
    light.pose = {{0, 0, 0, 1}, {0.0f, 0.0f, -3.0f}}; light.width = 1.0f; light.height = 1.0f;
    Check(quads.Draw(device, list, eye, head, fov, picture.Get(), UNORM, light, cvr::render::Blend::Light, 1.5f, 0.2f), "light refused");
    // The refusals.
    const cvr::render::SpaceQuads::Target empty{eyeImage.Get(), UNORM, {10, 10, 10, 20}, 0};
    Check(!quads.Fill(device, list, empty, dark), "empty rectangle drawn");
    Check(!quads.Draw(device, list, eye, head, fov, nullptr, UNORM, turned, cvr::render::Blend::Over, 1.0f), "no picture drawn");

    // The eye and the picture's levels read back.
    b = Transition(eyeImage.Get(), 0, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
    list->ResourceBarrier(1, &b);
    b = Transition(picture.Get(), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
    list->ResourceBarrier(1, &b);
    const auto eyeDesc = eyeImage->GetDesc();
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT eyeFp{}; UINT eyeRows = 0; UINT64 eyeRowBytes = 0, eyeTotal = 0;
    device->GetCopyableFootprints(&eyeDesc, 0, 1, 0, &eyeFp, &eyeRows, &eyeRowBytes, &eyeTotal);
    auto eyeRead = gpu.Buffer(D3D12_HEAP_TYPE_READBACK, eyeTotal);
    auto pictureRead = gpu.Buffer(D3D12_HEAP_TYPE_READBACK, total);
    D3D12_TEXTURE_COPY_LOCATION rd{}, rs{};
    rd.pResource = eyeRead.Get(); rd.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; rd.PlacedFootprint = eyeFp;
    rs.pResource = eyeImage.Get(); rs.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    list->CopyTextureRegion(&rd, 0, 0, 0, &rs, nullptr);
    for (UINT mip = 0; mip < LEVELS; ++mip) {
        rd.pResource = pictureRead.Get(); rd.PlacedFootprint = levels[mip];
        rs.pResource = picture.Get(); rs.SubresourceIndex = mip;
        list->CopyTextureRegion(&rd, 0, 0, 0, &rs, nullptr);
    }
    gpu.Run();

    uint64_t hash = 1469598103934665603ull;
    const auto mix = [&](const uint8_t* t, size_t n) { for (size_t k = 0; k < n; ++k) { hash ^= t[k]; hash *= 1099511628211ull; } };
    const uint8_t* e = nullptr; Hr(eyeRead->Map(0, nullptr, reinterpret_cast<void**>(const_cast<uint8_t**>(&e))), "map eye");
    for (UINT y = 0; y < EH; ++y) mix(e + eyeFp.Offset + y * eyeFp.Footprint.RowPitch, EW * 4);
    const uint8_t* corner = e + eyeFp.Offset;
    const uint8_t* middle = e + eyeFp.Offset + (EH / 2) * eyeFp.Footprint.RowPitch + (EW / 2) * 4;
    const uint8_t* solidPixel = e + eyeFp.Offset + 22 * eyeFp.Footprint.RowPitch + 22 * 4;   // inside the solid quad only
    const bool cornerFill = std::abs(int(corner[0]) - 26) <= 1 && std::abs(int(corner[1]) - 51) <= 1 && std::abs(int(corner[2]) - 77) <= 1;
    const bool middleChanged = std::abs(int(middle[0]) - int(corner[0])) > 8 || std::abs(int(middle[2]) - int(corner[2])) > 8;
    const bool solidBlended = solidPixel[0] > 120 && solidPixel[0] < 140;   // 0.5 x 0.9 + 0.5 x 0.1 = 0.5
    eyeRead->Unmap(0, nullptr);
    const uint8_t* p = nullptr; Hr(pictureRead->Map(0, nullptr, reinterpret_cast<void**>(const_cast<uint8_t**>(&p))), "map picture");
    for (UINT mip = 0; mip < LEVELS; ++mip)
        for (UINT y = 0; y < (PH >> mip); ++y) mix(p + levels[mip].Offset + y * levels[mip].Footprint.RowPitch, (PW >> mip) * 4);
    // Level 1's first texel: the mean of level 0's top-left 2 x 2 (red 0 and 8 -> 4, green likewise, blue 20).
    const uint8_t* l1 = p + levels[1].Offset;
    const bool meanOk = std::abs(int(l1[0]) - 4) <= 1 && std::abs(int(l1[1]) - 4) <= 1 && std::abs(int(l1[2]) - 20) <= 1;
    pictureRead->Unmap(0, nullptr);
    if (trace) { std::printf("space_quad trace %016llx\n", static_cast<unsigned long long>(hash)); return 0; }
    Check(cornerFill, "the fill is not where no quad reaches");
    Check(middleChanged, "the quads left the middle as the fill");
    Check(solidBlended, "the solid quad is not blended by its alpha");
    Check(meanOk, "a resampled level is not the mean of the one above");

    // Shutdown: everything made again at the next draw.
    quads.Shutdown();
    auto again = gpu.Texture(8, 8, 1, D3D12_RESOURCE_STATE_RENDER_TARGET);
    Check(quads.Fill(device, list, cvr::render::SpaceQuads::Target{again.Get(), UNORM, {0, 0, 8, 8}, 0}, dark), "nothing made again after Shutdown");
    gpu.Run();
    std::printf("PASS resampled mip chain, fill, solid, picture Over/Light through its mips, refusals, shutdown\n");
    return 0;
} catch (const std::exception& e) { std::fprintf(stderr, "%s\n", e.what()); return 1; }
