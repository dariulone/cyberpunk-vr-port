#include "Render/LoadingCapture.hpp"
#include "Core/Paths.hpp"
#include <wrl.h>
#include <algorithm>
#include <array>
#include <cstdio>
#include <filesystem>
#include <vector>

extern void Log(const char*, ...);

namespace cvr::loading::diagnostic {
namespace {
using Microsoft::WRL::ComPtr;
struct Image {
    ComPtr<ID3D12Resource> buffer;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
    uint64_t bytes{};
};
struct Sample {
    Image window, eye;
    ComPtr<ID3D12Fence> fence;
    uint64_t value{}, tick{};
    unsigned number{};
    Frame frame{};
};
struct State {
    std::array<Sample, 2> slots;
    std::filesystem::path directory;
    uint64_t pollAt{}, armedAt{}, nextSample{};
    unsigned count{};
    bool armed{}, loading{}, finishing{};
} state;

bool Prepare(ID3D12Device* device, ID3D12Resource* source, Image& out) {
    if (!source) return false;
    const auto desc = source->GetDesc();
    if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count != 1 ||
        desc.DepthOrArraySize != 1 || desc.Width > 8192 || desc.Height > 8192 ||
        (desc.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS && desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM &&
         desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM_SRGB)) return false;
    UINT64 bytes{};
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
    device->GetCopyableFootprints(&desc, 0, 1, 0, &footprint, nullptr, nullptr, &bytes);
    if (bytes > 128ull * 1024 * 1024) return false;
    if (!out.buffer || bytes != out.bytes) {
        D3D12_RESOURCE_DESC buffer{};
        buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        buffer.Width = bytes; buffer.Height = 1; buffer.DepthOrArraySize = 1;
        buffer.MipLevels = 1; buffer.SampleDesc.Count = 1; buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
        ComPtr<ID3D12Resource> made;
        if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &buffer,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&made)))) return false;
        out.buffer = std::move(made); out.bytes = bytes;
    }
    out.footprint = footprint;
    return true;
}

void Copy(ID3D12GraphicsCommandList* list, ID3D12Resource* source, const Image& image) {
    D3D12_TEXTURE_COPY_LOCATION from{}, to{};
    from.pResource = source; from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    to.pResource = image.buffer.Get(); to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint = image.footprint;
    list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
}

bool Save(const Image& image, const std::filesystem::path& path) {
    const auto& fp = image.footprint;
    const UINT step = (std::max)(1u, (fp.Footprint.Width + 479) / 480);
    const UINT w = (fp.Footprint.Width + step - 1) / step;
    const UINT h = (fp.Footprint.Height + step - 1) / step;
    void* pixels{};
    D3D12_RANGE range{0, static_cast<SIZE_T>(image.bytes)};
    if (FAILED(image.buffer->Map(0, &range, &pixels))) return false;
    std::vector<unsigned char> rgb(size_t(w) * h * 3);
    const auto* data = static_cast<const unsigned char*>(pixels) + fp.Offset;
    for (UINT y = 0; y < h; ++y) for (UINT x = 0; x < w; ++x) {
        const auto* p = data + size_t(y * step) * fp.Footprint.RowPitch + size_t(x * step) * 4;
        std::copy_n(p, 3, rgb.data() + (size_t(y) * w + x) * 3);
    }
    const D3D12_RANGE none{}; image.buffer->Unmap(0, &none);
    FILE* file = _wfopen(path.c_str(), L"wb");
    if (!file) return false;
    const bool ok = std::fprintf(file, "P6\n%u %u\n255\n", w, h) > 0 &&
        std::fwrite(rgb.data(), 1, rgb.size(), file) == rgb.size();
    const bool closed = std::fclose(file) == 0;
    return ok && closed;
}

void Drain() {
    bool pending = false;
    for (auto& slot : state.slots) {
        if (!slot.value) continue;
        const auto done = slot.fence->GetCompletedValue();
        if (done == UINT64_MAX) { slot.value = 0; state.finishing = true; continue; }
        if (done < slot.value) { pending = true; continue; }
        wchar_t name[64];
        swprintf_s(name, L"%03u-window.ppm", slot.number);
        bool ok = Save(slot.window, state.directory / name);
        swprintf_s(name, L"%03u-eye.ppm", slot.number);
        ok = Save(slot.eye, state.directory / name) && ok;
        FILE* json = _wfopen((state.directory / L"frames.jsonl").c_str(), L"a");
        if (json) {
            const auto& f = slot.frame;
            const auto& w = slot.window.footprint.Footprint;
            const auto& e = slot.eye.footprint.Footprint;
            ok = std::fprintf(json,
                "{\"sample\":%u,\"tick\":%llu,\"serial\":%llu,\"loading\":%d,\"fresh\":%d,"
                "\"world\":%d,\"eye\":%u,\"windowSize\":[%u,%u],\"eyeSize\":[%u,%u],\"saved\":%d}\n",
                slot.number, slot.tick, f.serial, f.loading, f.freshPicture, f.worldDrawn,
                f.eye, w.Width, w.Height, e.Width, e.Height, ok) > 0 && ok;
            ok = std::fclose(json) == 0 && ok;
        } else ok = false;
        slot.value = 0;
        if (!ok) { Log("[loading-capture] write failed; stopping\n"); state.finishing = true; }
    }
    if (state.finishing && !pending) {
        Log("[loading-capture] finished: %u samples in %ls\n", state.count, state.directory.c_str());
        for (auto& slot : state.slots) slot = {};
        state.armed = state.loading = state.finishing = false;
    }
}
}

void Record(ID3D12Device* device, ID3D12GraphicsCommandList* list, ID3D12Resource* window,
            ID3D12Resource* eye, ID3D12Fence* fence, uint64_t value, const Frame& frame) {
    // Diagnostics must never prevent a frame being presented, including on disk/allocation failure.
    try {
        Drain();
        const auto now = GetTickCount64();
        if (!state.armed && now >= state.pollAt) {
            state.pollAt = now + 1000;
            const auto request = cvr::paths::File(cvr::paths::Folder::Log, L"loading-capture.request");
            if (GetFileAttributesW(request.c_str()) != INVALID_FILE_ATTRIBUTES && DeleteFileW(request.c_str())) {
                state.armed = true; state.armedAt = now; state.count = 0; state.nextSample = 0;
                state.directory = std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Log)) /
                    L"loading-capture" / (std::to_wstring(GetCurrentProcessId()) + L"-" + std::to_wstring(now));
                std::filesystem::create_directories(state.directory);
                Log("[loading-capture] armed for the next load: %ls\n", state.directory.c_str());
            }
        }
        if (!state.armed || state.finishing) return;
        if ((!state.loading && now - state.armedAt > 600000) || (state.loading && !frame.loading)) {
            state.finishing = true; Drain(); return;
        }
        if (!frame.loading || now < state.nextSample || !device || !list || !fence || !value) return;
        state.loading = true;
        auto it = std::find_if(state.slots.begin(), state.slots.end(), [](const auto& s) { return !s.value; });
        if (it == state.slots.end()) return; // GPU is busy: never wait or overwrite an in-flight readback.
        auto& slot = *it;
        if (!Prepare(device, window, slot.window) || !Prepare(device, eye, slot.eye)) {
            Log("[loading-capture] unsupported image/allocation failure; stopping\n");
            state.finishing = true; return;
        }
        D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition = {window, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE};
        list->ResourceBarrier(1, &b); Copy(list, window, slot.window);
        std::swap(b.Transition.StateBefore, b.Transition.StateAfter); list->ResourceBarrier(1, &b);
        Copy(list, eye, slot.eye);
        slot.frame = frame; slot.tick = now; slot.fence = fence; slot.value = value;
        slot.number = state.count++;
        state.nextSample = now + 200;
        if (state.count >= 60) state.finishing = true;
    } catch (...) {
        state.finishing = true;
        Log("[loading-capture] diagnostic exception; stopping capture\n");
    }
}

void Shutdown() {
    state.finishing = true;
    try { Drain(); } catch (...) {}
    // Teardown can run before a capture fence finishes. Retain those allocations
    // until a later Drain (or process exit), never free a GPU write target early.
    for (const auto& slot : state.slots) if (slot.value) return;
    state = {};
}
}
