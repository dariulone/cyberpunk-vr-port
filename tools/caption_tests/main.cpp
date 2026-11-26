// Lines of text under a hung screen (include/Render/Caption.hpp) on a WARP device: the boot notice's two lines in
// Latin, Cyrillic and Japanese, then Arabic right to left, each set with DirectWrite into a picture with all its mips
// and uploaded on a list; every level read back. Checked: the band (top, bottom, width), the level count, letters lit
// in the top level and only within the band's lines, the mips the means of the level above (within a step), the same
// text kept without a new upload, a new text drawn anew, Shutdown drawing it again. "trace" prints a hash of every byte
// of every level, so the C++ build and the Rust build can be compared bit for bit (cmake/ParityTrace.cmake).
#include "Render/Caption.hpp"
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

void Log(const char* fmt, ...) { (void)fmt; }   // the C++ caption logs through the plugin's Log

using Microsoft::WRL::ComPtr;
static void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
static void Hr(HRESULT hr, const char* text) { if (FAILED(hr)) throw std::runtime_error(text); }

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
};

// Every level of `picture` read back into the hash; the top level's texels returned (one byte each, the red).
static std::vector<uint8_t> ReadBack(Gpu& gpu, ID3D12Resource* picture, uint64_t& hash, std::vector<std::vector<uint8_t>>* levelsOut) {
    const auto desc = picture->GetDesc();
    const UINT levels = desc.MipLevels;
    std::vector<D3D12_PLACED_SUBRESOURCE_FOOTPRINT> fp(levels); std::vector<UINT> rows(levels); std::vector<UINT64> rowBytes(levels);
    UINT64 total = 0;
    gpu.device->GetCopyableFootprints(&desc, 0, levels, 0, fp.data(), rows.data(), rowBytes.data(), &total);
    D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC b{}; b.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; b.Width = total; b.Height = 1; b.DepthOrArraySize = 1;
    b.MipLevels = 1; b.SampleDesc.Count = 1; b.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ComPtr<ID3D12Resource> buffer;
    Hr(gpu.device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &b, D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&buffer)), "readback");
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = picture;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    gpu.list->ResourceBarrier(1, &barrier);
    for (UINT mip = 0; mip < levels; ++mip) {
        D3D12_TEXTURE_COPY_LOCATION to{}, from{};
        to.pResource = buffer.Get(); to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; to.PlacedFootprint = fp[mip];
        from.pResource = picture; from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX; from.SubresourceIndex = mip;
        gpu.list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    }
    std::swap(barrier.Transition.StateBefore, barrier.Transition.StateAfter);
    gpu.list->ResourceBarrier(1, &barrier);
    gpu.Run();
    const uint8_t* p = nullptr; Hr(buffer->Map(0, nullptr, reinterpret_cast<void**>(const_cast<uint8_t**>(&p))), "map");
    std::vector<uint8_t> top;
    for (UINT mip = 0; mip < levels; ++mip) {
        const UINT w = fp[mip].Footprint.Width, h = fp[mip].Footprint.Height;
        std::vector<uint8_t> level;
        for (UINT y = 0; y < h; ++y) {
            const uint8_t* row = p + fp[mip].Offset + y * fp[mip].Footprint.RowPitch;
            for (UINT k = 0; k < w * 4; ++k) { hash ^= row[k]; hash *= 1099511628211ull; }
            for (UINT x = 0; x < w; ++x) {
                Check(row[x * 4] == row[x * 4 + 1] && row[x * 4] == row[x * 4 + 2] && row[x * 4 + 3] == 255, "a texel is not opaque grey");
                level.push_back(row[x * 4]);
            }
        }
        if (mip == 0) top = level;
        if (levelsOut) levelsOut->push_back(std::move(level));
    }
    buffer->Unmap(0, nullptr);
    return top;
}

static double Linear(uint8_t v) {
    const double c = v / 255.0;
    return c <= 0.04045 ? c / 12.92 : std::pow((c + 0.055) / 1.055, 2.4);
}

int main(int argc, char** argv) try {
    const bool trace = argc > 1 && std::string(argv[1]) == "trace";
    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "factory");
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)), "warp");
    Gpu gpu(adapter.Get());
    uint64_t hash = 1469598103934665603ull;

    cvr::render::Caption caption;
    Check(!caption.Picture(gpu.device.Get(), gpu.list.Get()), "a picture with no lines");
    cvr::render::CaptionText notice;
    notice.lines = {{L"cyberpunkvrport is an experimental fan-made mod.", 250.0f},
                    {L"cyberpunkvrport — экспериментальный фанатский мод.", 333.0f},
                    {L"cyberpunkvrport はファンによる実験的なMODです。", 416.0f}};
    notice.locale = L"ja-jp";
    caption.Set(notice);
    Check(!caption.Picture(nullptr, gpu.list.Get()), "a picture with no device");
    ID3D12Resource* picture = caption.Picture(gpu.device.Get(), gpu.list.Get());
    Check(picture != nullptr, "no picture");
    gpu.Run();
    Check(caption.Top() == 250.0f - 51.0f && caption.Bottom() == 416.0f + 51.0f && caption.Width() == 3840.0f, "the band");
    const auto desc = picture->GetDesc();
    Check(desc.Width == 3840 && desc.Height == UINT(std::ceil(caption.Bottom() - caption.Top())) && desc.MipLevels == 12 &&
          desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, "the picture's size and levels");
    std::vector<std::vector<uint8_t>> levels;
    const auto top = ReadBack(gpu, picture, hash, &levels);
    // Lit letters, never brighter than `light`, and nothing at the very top and bottom rows (the band's margin).
    uint8_t peak = 0; bool marginDark = true;
    for (UINT x = 0; x < 3840; ++x) marginDark = marginDark && top[x] == 0 && top[size_t(desc.Height - 1) * 3840 + x] == 0;
    for (uint8_t v : top) peak = (std::max)(peak, v);
    Check(peak > 100 && Linear(peak) <= 0.35 + 0.01, "the letters' light");
    Check(marginDark, "light outside the lines");
    // Level 1 the mean of level 0 (in linear light, within a step of the encoding).
    double worst = 0;
    for (UINT y = 0; y < desc.Height / 2; ++y) for (UINT x = 0; x < 1920; ++x) {
        const auto at = [&](UINT xx, UINT yy) { return Linear(levels[0][size_t(yy) * 3840 + xx]); };
        const double mean = 0.25 * (at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1));
        worst = (std::max)(worst, std::abs(Linear(levels[1][size_t(y) * 1920 + x]) - mean));
    }
    Check(worst < 0.01, "a mip is not the mean of the level above");

    // The same text: the same picture, nothing recorded.
    caption.Set(notice);
    Check(caption.Picture(gpu.device.Get(), gpu.list.Get()) == picture, "the same text drawn anew");
    // Arabic, right to left: a new picture.
    cvr::render::CaptionText arabic;
    arabic.lines = {{L"cyberpunkvrport تعديل تجريبي من صنع المعجبين.", 250.0f}, {L"مشروع غير رسمي لا يرتبط بشركة CD PROJEKT RED.", 333.0f}};
    arabic.locale = L"ar-sa"; arabic.rightToLeft = true; arabic.size = 40.0f; arabic.light = 0.5f;
    caption.Set(arabic);
    ID3D12Resource* second = caption.Picture(gpu.device.Get(), gpu.list.Get());
    Check(second && second != picture, "a new text not drawn anew");
    gpu.Run();
    ReadBack(gpu, second, hash, nullptr);
    Check(caption.Top() == 250.0f - 40.0f && caption.Bottom() == 333.0f + 40.0f, "the second band");
    // Shutdown: the text drawn again at the next call.
    caption.Shutdown();
    ID3D12Resource* third = caption.Picture(gpu.device.Get(), gpu.list.Get());
    Check(third != nullptr, "nothing drawn after Shutdown");
    gpu.Run();
    ReadBack(gpu, third, hash, nullptr);
    if (trace) { std::printf("caption trace %016llx\n", static_cast<unsigned long long>(hash)); return 0; }
    std::printf("PASS notice lines (Latin, Cyrillic, Japanese) and Arabic right to left: band, 12 levels, light %u, mips within %.4f, kept, redrawn, shutdown\n",
                unsigned(peak), worst);
    return 0;
} catch (const std::exception& e) { std::fprintf(stderr, "%s\n", e.what()); return 1; }
