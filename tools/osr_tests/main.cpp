// OSR, the port's reconstruction of the other eye (include/Render/Osr.hpp) on a WARP device: a scene of a far wall and
// a near box, the left eye drawn, the right eye made from it; then the eyes swapped, so the made eye's previous frame
// (its history) takes part; then Reset and once more without history; and the refusals (no request, a depth that is not
// R32_FLOAT, a drawn eye smaller than the made one). Checked: the made eye shows the wall where only the wall is, the box
// moved by the parallax, the statistics and the exported counters. "trace" prints a hash of every byte made, so the C++
// build and the Rust build can be compared bit for bit (cmake/ParityTrace.cmake).
#include "Render/Osr.hpp"
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

void Log(const char* fmt, ...) { (void)fmt; }   // the C++ reconstruction logs through the plugin's Log
#if CVR_RUST_VRIK
// The plugin defines these exports in Rust (rust/crates/cvr-render/src/used_moved_exports_rest.rs); the test, with the
// C++ definitions' values.
extern "C" __declspec(dllexport) std::atomic<int64_t> CyberpunkVR_OsrStats[3]{};
extern "C" __declspec(dllexport) int32_t CyberpunkVR_OsrTune[4] = {30, 384, 100, 75};
#endif
extern "C" std::atomic<int64_t> CyberpunkVR_OsrStats[3];

using Microsoft::WRL::ComPtr;
static void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
static void Hr(HRESULT hr, const char* text) { if (FAILED(hr)) throw std::runtime_error(text); }

constexpr UINT W = 64, H = 64;
constexpr float kNear = 0.02f, kFar = 16000.0f;

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
    ComPtr<ID3D12Resource> Texture(UINT w, UINT h, DXGI_FORMAT f, D3D12_RESOURCE_STATES state) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; d.Width = w; d.Height = h; d.DepthOrArraySize = 1;
        d.MipLevels = 1; d.Format = f; d.SampleDesc.Count = 1;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, state, nullptr, IID_PPV_ARGS(&r)), "texture");
        return r;
    }
    // `bytes` (tightly packed rows of `texel` bytes) into `t`, which goes from COPY_DEST to `after`.
    void Upload(ID3D12Resource* t, const std::vector<uint8_t>& bytes, UINT texel, D3D12_RESOURCE_STATES after) {
        const auto desc = t->GetDesc();
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{}; UINT64 total = 0;
        device->GetCopyableFootprints(&desc, 0, 1, 0, &fp, nullptr, nullptr, &total);
        auto up = Buffer(D3D12_HEAP_TYPE_UPLOAD, total);
        uint8_t* p = nullptr; Hr(up->Map(0, nullptr, reinterpret_cast<void**>(&p)), "map");
        for (UINT y = 0; y < desc.Height; ++y) std::memcpy(p + fp.Offset + y * fp.Footprint.RowPitch, bytes.data() + size_t(y) * desc.Width * texel, desc.Width * texel);
        up->Unmap(0, nullptr);
        D3D12_TEXTURE_COPY_LOCATION dst{}, src{};
        src.pResource = up.Get(); src.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; src.PlacedFootprint = fp;
        dst.pResource = t; dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION; b.Transition.pResource = t;
        b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES; b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
        b.Transition.StateAfter = after;
        if (after != D3D12_RESOURCE_STATE_COPY_DEST) list->ResourceBarrier(1, &b);
        Run();
    }
    // `t` (in COPY_SOURCE) read back, tightly packed.
    std::vector<uint8_t> Read(ID3D12Resource* t) {
        const auto desc = t->GetDesc();
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{}; UINT64 total = 0;
        device->GetCopyableFootprints(&desc, 0, 1, 0, &fp, nullptr, nullptr, &total);
        auto rb = Buffer(D3D12_HEAP_TYPE_READBACK, total);
        D3D12_TEXTURE_COPY_LOCATION dst{}, src{};
        dst.pResource = rb.Get(); dst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; dst.PlacedFootprint = fp;
        src.pResource = t; src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        Run();
        std::vector<uint8_t> out(size_t(desc.Width) * desc.Height * 4);
        const uint8_t* p = nullptr; Hr(rb->Map(0, nullptr, reinterpret_cast<void**>(const_cast<uint8_t**>(&p))), "map readback");
        for (UINT y = 0; y < desc.Height; ++y) std::memcpy(out.data() + size_t(y) * desc.Width * 4, p + fp.Offset + y * fp.Footprint.RowPitch, desc.Width * 4);
        rb->Unmap(0, nullptr);
        return out;
    }
};

// One eye: at `x` along the eyes' line, looking down -Z, 90 x 90 degrees.
static cvr::osr::Camera Eye(float x) {
    cvr::osr::Camera c;
    c.position[0] = x; c.position[1] = 1.7f; c.position[2] = 0.0f;
    c.right[0] = 1.0f; c.up[1] = 1.0f; c.forward[2] = -1.0f;
    c.fov = {-0.7853982f, 0.7853982f, 0.7853982f, -0.7853982f};
    c.nearPlane = kNear; c.farPlane = kFar; c.inverted = true; c.infinite = false;
    return c;
}

// The scene as `eye` sees it: a wall 6 m away (2 m stripes, some 10 px), and a box 0.8 m away in the middle (flat orange).
static void Draw(const cvr::osr::Camera& eye, std::vector<uint8_t>& colour, std::vector<uint8_t>& depth) {
    colour.assign(size_t(W) * H * 4, 0);
    depth.assign(size_t(W) * H * 4, 0);
    const double A = -kNear / double(kFar - kNear), B = double(kNear) * kFar / double(kFar - kNear);
    for (UINT y = 0; y < H; ++y) for (UINT x = 0; x < W; ++x) {
        const double tx = -1.0 + 2.0 * (x + 0.5) / W, ty = 1.0 - 2.0 * (y + 0.5) / H;   // tangents (90 degrees)
        // The box: |X| < 0.15, |Y - 1.7| < 0.15 at z = 0.8 (world), seen from the eye.
        const double bx = eye.position[0] + tx * 0.8, by = 1.7 + ty * 0.8;
        const bool box = std::abs(bx) < 0.15 && std::abs(by - 1.7) < 0.15;
        const double z = box ? 0.8 : 6.0;
        const double wx = eye.position[0] + tx * z;
        uint8_t* c = colour.data() + (size_t(y) * W + x) * 4;
        if (box) { c[0] = 240; c[1] = 140; c[2] = 20; }
        else { const int stripe = int(std::floor(wx * 0.5)) & 1; c[0] = stripe ? 30 : 200; c[1] = uint8_t(60 + y); c[2] = stripe ? 180 : 40; }
        c[3] = 255;
        const float d = float(A + B / z);
        std::memcpy(depth.data() + (size_t(y) * W + x) * 4, &d, 4);
    }
}

int main(int argc, char** argv) try {
    const bool trace = argc > 1 && std::string(argv[1]) == "trace";
    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "factory");
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)), "warp");
    Gpu gpu(adapter.Get());
    uint64_t hash = 1469598103934665603ull;
    const auto mix = [&](const std::vector<uint8_t>& v) { for (uint8_t b : v) { hash ^= b; hash *= 1099511628211ull; } };

    const auto left = Eye(-0.032f), right = Eye(0.032f);
    auto drawn = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_STATE_COPY_DEST);
    auto depth = gpu.Texture(W, H, DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_STATE_COPY_DEST);
    auto out = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_STATE_COPY_SOURCE);
    std::vector<uint8_t> colourBytes, depthBytes, rightColour, rightDepth;
    Draw(left, colourBytes, depthBytes);
    Draw(right, rightColour, rightDepth);
    gpu.Upload(drawn.Get(), colourBytes, 4, D3D12_RESOURCE_STATE_PRESENT);
    gpu.Upload(depth.Get(), depthBytes, 4, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);

    cvr::osr::Request q;
    q.device = gpu.device.Get(); q.list = gpu.list.Get(); q.drawn = drawn.Get(); q.depth = depth.Get(); q.out = out.Get();
    q.source = left; q.dest = right; q.eye = 0;
    // The refusals first: nothing is made, nothing kept.
    cvr::osr::Request none{};
    Check(!cvr::osr::Record(none), "an empty request recorded");
    Check(std::strcmp(cvr::osr::Statistics().last, "no request") == 0, "the empty request's reason");
    // 1: the left eye drawn, the right made (no history yet).
    Check(cvr::osr::Record(q), "the right eye not made");
    gpu.Run();
    const auto made1 = gpu.Read(out.Get());
    mix(made1);
    // How far the made eye is from the right eye drawn directly: the wall agrees nearly everywhere.
    size_t close = 0;
    for (size_t i = 0; i < made1.size(); i += 4)
        if (std::abs(int(made1[i]) - int(rightColour[i])) < 24 && std::abs(int(made1[i + 2]) - int(rightColour[i + 2])) < 24) ++close;
    // 2: the eyes swapped: the right drawn, the left made, with the left's history from step 1.
    auto drawnRight = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_STATE_COPY_DEST);
    auto depthRight = gpu.Texture(W, H, DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_STATE_COPY_DEST);
    gpu.Upload(drawnRight.Get(), rightColour, 4, D3D12_RESOURCE_STATE_PRESENT);
    gpu.Upload(depthRight.Get(), rightDepth, 4, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    const int64_t historyBefore = CyberpunkVR_OsrStats[2].load();
    q.drawn = drawnRight.Get(); q.depth = depthRight.Get();
    q.source = right; q.dest = left; q.eye = 1; q.debug = 1;   // history tinted: it must show
    Check(cvr::osr::Record(q), "the left eye not made");
    gpu.Run();
    const auto made2 = gpu.Read(out.Get());
    mix(made2);
    const bool usedHistory = CyberpunkVR_OsrStats[2].load() == historyBefore + 1;
    // 3: Reset, the same again without history.
    cvr::osr::Reset();
    q.debug = 0;
    Check(cvr::osr::Record(q), "the left eye not made after Reset");
    gpu.Run();
    mix(gpu.Read(out.Get()));
    const bool noHistory = CyberpunkVR_OsrStats[2].load() == historyBefore + 1;
    // The refusals that reach the textures.
    auto colourDepth = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    auto halfSize = gpu.Texture(W / 2, H / 2, DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_STATE_PRESENT);
    auto bad = q; bad.depth = colourDepth.Get();
    Check(!cvr::osr::Record(bad), "a colour depth recorded");
    Check(std::strcmp(cvr::osr::Statistics().last, "depth not R32_FLOAT") == 0, "the colour depth's reason");
    bad = q; bad.drawn = halfSize.Get();
    Check(!cvr::osr::Record(bad), "a small drawn eye recorded");
    Check(std::strcmp(cvr::osr::Statistics().last, "drawn smaller than the eye") == 0, "the small eye's reason");
    const auto stats = cvr::osr::Statistics();
    if (trace) { std::printf("osr trace %016llx\n", static_cast<unsigned long long>(hash)); return 0; }
    // The first initialisation starts the statistics over (the empty request before it counts in the export only).
    Check(stats.recorded == 3 && stats.failed == 2, "the statistics");
    Check(CyberpunkVR_OsrStats[0].load() == 3 && CyberpunkVR_OsrStats[1].load() == 3, "the exported counters");
    Check(usedHistory && noHistory, "the history taken, and not after Reset");
    if (close <= size_t(W) * H * 85 / 100) std::fprintf(stderr, "close %zu of %u\n", close, W * H);
    Check(close > size_t(W) * H * 85 / 100, "the made eye is not the scene from the other eye");
    std::printf("PASS made eye matches the other eye in %zu of %u pixels, history then Reset, refusals with reasons, counters\n",
                close, W * H);
    return 0;
} catch (const std::exception& e) { std::fprintf(stderr, "%s\n", e.what()); return 1; }
