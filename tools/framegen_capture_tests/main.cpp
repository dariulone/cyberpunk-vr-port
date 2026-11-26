// Framegen's frame capture (CaptureFrame: src/Framegen/FrameCapture.cpp, or with CVR_RUST_VRIK
// rust/crates/cvr-gpu/src/frame_capture.rs) on a WARP device. Three rounds against the switch CyberpunkVR_FramegenCapture:
// disarmed (nothing is written); armed with a frame shown again, a gap that restarts the sequence and three frames that
// follow each other; armed with an eye whose depth is missing (the switch reads -1), then three frames with one eye
// without motion and unusual camera values. Checked: the switch, the files written, the raw colour, depth and motion
// against the uploaded pixels (rows without their padding). "trace" prints every file's name, size and hash and
// capture.json with its QPC stamps blanked, so the C++ build and the Rust build can be compared byte for byte
// (cmake/ParityTrace.cmake).
#include "Core/Paths.hpp"
#include "Framegen/FrameCapture.hpp"
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <regex>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

extern "C" __declspec(dllexport) std::atomic<int32_t> CyberpunkVR_FramegenCapture;
void Log(const char*, ...) {}  // the C++ capture's log line
// cvr_rust.lib's settings parser reaches the C++ subsystems through this adapter (src/Core/LiveControlsParse.cpp).
extern "C" bool cvr_cpp_settings_parse_subsystem(int, const char*, void*) { return false; }

namespace {
using Microsoft::WRL::ComPtr;
void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
void Hr(HRESULT hr, const char* text) { if (FAILED(hr)) throw std::runtime_error(text); }

struct Gpu {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Fence> fence;
    uint64_t value{};
    void Wait() {
        Hr(queue->Signal(fence.Get(), ++value), "signal");
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        fence->SetEventOnCompletion(value, event);
        WaitForSingleObject(event, 10000);
        CloseHandle(event);
        Check(fence->GetCompletedValue() >= value, "gpu wait");
    }
};
Gpu g;

struct Texture {
    ComPtr<ID3D12Resource> resource;
    D3D12_RESOURCE_STATES state{D3D12_RESOURCE_STATE_COPY_DEST};
    D3D12_RESOURCE_STATES rest{};
    uint32_t width{}, height{}, pixel{};
    std::vector<uint8_t> pixels;  // the last upload, rows without padding
};
Texture Make(uint32_t width, uint32_t height, DXGI_FORMAT format, uint32_t pixel, D3D12_RESOURCE_STATES rest) {
    Texture t;
    t.width = width; t.height = height; t.pixel = pixel; t.rest = rest;
    D3D12_RESOURCE_DESC d{};
    d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; d.Width = width; d.Height = height; d.DepthOrArraySize = 1;
    d.MipLevels = 1; d.Format = format; d.SampleDesc.Count = 1;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    Hr(g.device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_DEST, nullptr,
                                         IID_PPV_ARGS(&t.resource)), "texture");
    return t;
}
void Barrier(ID3D12Resource* r, D3D12_RESOURCE_STATES a, D3D12_RESOURCE_STATES b) {
    if (a == b) return;
    D3D12_RESOURCE_BARRIER v{}; v.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    v.Transition = {r, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, a, b};
    g.list->ResourceBarrier(1, &v);
}
// New pixels from `seed` into `t`, left in its resting state.
void Upload(Texture& t, uint32_t seed) {
    const auto desc = t.resource->GetDesc();
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{}; UINT rows{}; UINT64 rowBytes{}, bytes{};
    g.device->GetCopyableFootprints(&desc, 0, 1, 0, &footprint, &rows, &rowBytes, &bytes);
    D3D12_RESOURCE_DESC b{};
    b.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; b.Width = bytes; b.Height = 1; b.DepthOrArraySize = 1; b.MipLevels = 1;
    b.SampleDesc.Count = 1; b.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_UPLOAD;
    ComPtr<ID3D12Resource> upload;
    Hr(g.device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &b, D3D12_RESOURCE_STATE_GENERIC_READ, nullptr,
                                         IID_PPV_ARGS(&upload)), "upload");
    uint8_t* data{}; Hr(upload->Map(0, nullptr, reinterpret_cast<void**>(&data)), "map");
    t.pixels.assign(size_t(rows) * rowBytes, 0);
    uint32_t x = seed * 2654435761u + 0x9E3779B9u;
    for (UINT row = 0; row < rows; ++row)
        for (UINT64 i = 0; i < rowBytes; ++i) {
            x ^= x << 13; x ^= x >> 17; x ^= x << 5;
            const uint8_t v = uint8_t(x >> 24);
            data[footprint.Offset + size_t(row) * footprint.Footprint.RowPitch + i] = v;
            t.pixels[size_t(row) * rowBytes + i] = v;
        }
    upload->Unmap(0, nullptr);
    Hr(g.allocator->Reset(), "allocator"); Hr(g.list->Reset(g.allocator.Get(), nullptr), "list");
    Barrier(t.resource.Get(), t.state, D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_TEXTURE_COPY_LOCATION from{}, to{};
    from.pResource = upload.Get(); from.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; from.PlacedFootprint = footprint;
    to.pResource = t.resource.Get(); to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    g.list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    Barrier(t.resource.Get(), D3D12_RESOURCE_STATE_COPY_DEST, t.rest);
    t.state = t.rest;
    Hr(g.list->Close(), "close");
    ID3D12CommandList* lists[] = {g.list.Get()};
    g.queue->ExecuteCommandLists(1, lists);
    g.Wait();
}

Texture color[2], depth[2], motion[2];

cvr::framegen::CameraData Camera(uint32_t frame, unsigned eye, bool odd) {
    cvr::framegen::CameraData c{};
    const float f = float(frame), e = float(eye);
    c.jitter[0] = 0.25f / (f + 1); c.jitter[1] = -0.125f * (e + 1);
    c.motionScale[0] = 1280.f + e; c.motionScale[1] = -720.f;
    for (int i = 0; i < 3; ++i) {
        c.position[i] = f * 0.1f + i - e * 0.064f; c.up[i] = i == 1 ? 1.f : 0.f;
        c.right[i] = i == 0 ? 1.f : 0.f; c.forward[i] = i == 2 ? -1.f : 1e-7f * i;
    }
    c.nearPlane = 0.05f; c.farPlane = odd ? 3.4e38f : 4096.f; c.verticalFov = 1.48352981f + e * 1e-6f;
    c.aspect = 0.9247311f; c.inverted = true; c.infinite = odd; c.jittered = frame % 2 == 0;
    if (odd) { c.jitter[1] = -0.0f; c.position[2] = 1e-30f; c.up[0] = 16777217.f; }
    return c;
}
void Offer(uint32_t frame, bool withMotion1 = true, bool withDepth1 = true, bool odd = false) {
    std::shared_ptr<const cvr::framegen::Inputs> inputs[2];
    cvr::framegen::EyePair cameras[2];
    for (unsigned e = 0; e < 2; ++e) {
        auto in = std::make_shared<cvr::framegen::Inputs>();
        in->depth = e == 1 && !withDepth1 ? nullptr : depth[e].resource;
        in->motion = motion[e].resource;
        in->hasDepth = true;
        in->hasMotion = e == 0 || withMotion1;
        in->frameId = frame;
        in->poseId = 1000 + frame * 2 + e;
        in->camera = Camera(frame, e, odd);
        inputs[e] = in;
        auto& c = cameras[e].current;
        c.orientation[0] = 0.01f * frame; c.orientation[3] = 0.99995f;
        c.fov[0] = -0.942478f + e * 0.244346f; c.fov[1] = 0.698132f + e * 0.244346f; c.fov[2] = 0.778023f; c.fov[3] = -0.954061f;
    }
    ID3D12Resource* colors[2] = {color[0].resource.Get(), color[1].resource.Get()};
    cvr::framegen::CaptureFrame(g.device.Get(), g.queue.Get(), colors, inputs, cameras);
    g.Wait();  // the capture's copies done, so the next frame is never turned away for them
}
void NewPixels(uint32_t frame) {
    for (unsigned e = 0; e < 2; ++e) {
        Upload(color[e], frame * 8 + e); Upload(depth[e], frame * 8 + 2 + e); Upload(motion[e], frame * 8 + 4 + e);
    }
}

std::filesystem::path Root() { return std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Log)) / L"framegen-capture"; }
int32_t AwaitWritten() {
    for (int i = 0; i < 2000; ++i) {
        const int32_t s = CyberpunkVR_FramegenCapture.load();
        if (s == 2 || s == -1) return s;
        std::this_thread::sleep_for(std::chrono::milliseconds(5));
    }
    return CyberpunkVR_FramegenCapture.load();
}
std::vector<uint8_t> Read(const std::filesystem::path& p) {
    std::ifstream in(p, std::ios::binary);
    return {std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
}
uint64_t Hash(const std::vector<uint8_t>& v) {
    uint64_t h = 1469598103934665603ull;
    for (uint8_t b : v) h = (h ^ b) * 1099511628211ull;
    return h;
}
// The round's folder: its files listed (and capture.json printed) with trace, then removed.
void Report(int round, bool trace, const std::vector<uint32_t>& frames, bool motion1) {
    std::vector<std::filesystem::path> folders;
    if (std::filesystem::exists(Root()))
        for (const auto& d : std::filesystem::directory_iterator(Root())) folders.push_back(d.path());
    Check(folders.size() == 1, "one capture folder");
    std::vector<std::filesystem::path> files;
    for (const auto& f : std::filesystem::directory_iterator(folders[0])) files.push_back(f.path());
    std::sort(files.begin(), files.end());
    Check(files.size() == 1 + frames.size() * (motion1 ? 6 : 5), "file count");
    for (const auto& f : files) {
        auto bytes = Read(f);
        const auto name = f.filename().string();
        std::string text;
        if (name == "capture.json") {
            // The QPC stamps differ from run to run: blanked before the hash.
            text = std::regex_replace(std::string(bytes.begin(), bytes.end()), std::regex("\"qpc\":-?[0-9]+"), "\"qpc\":Q");
            Check(text.rfind("{\"frames\":[\r\n", 0) == 0 && text.size() > 6 && text.substr(text.size() - 6) == "\r\n]}\r\n", "json frame");
            bytes.assign(text.begin(), text.end());
        }
        if (trace) std::printf("round %d %s %zu %016llx\n", round, name.c_str(), bytes.size(), static_cast<unsigned long long>(Hash(bytes)));
        if (trace && !text.empty()) std::printf("%s", text.c_str());
    }
    // The last frame's pixels as uploaded, rows without padding.
    const auto last = frames.size() - 1;
    for (unsigned e = 0; e < 2; ++e) {
        char name[64];
        std::snprintf(name, sizeof(name), "f%zu_e%u_color.raw", last, e);
        Check(Read(folders[0] / name) == color[e].pixels, "color pixels");
        std::snprintf(name, sizeof(name), "f%zu_e%u_depth.raw", last, e);
        Check(Read(folders[0] / name) == depth[e].pixels, "depth pixels");
        if (e == 0 || motion1) {
            std::snprintf(name, sizeof(name), "f%zu_e%u_motion.raw", last, e);
            Check(Read(folders[0] / name) == motion[e].pixels, "motion pixels");
        }
    }
    std::filesystem::remove_all(Root());
}
}  // namespace

int main(int argc, char** argv) {
    const bool trace = argc > 1 && std::strcmp(argv[1], "trace") == 0;
    try {
        ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "factory");
        ComPtr<IDXGIAdapter> warp; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp)), "warp");
        Hr(D3D12CreateDevice(warp.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&g.device)), "device");
        D3D12_COMMAND_QUEUE_DESC q{}; q.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        Hr(g.device->CreateCommandQueue(&q, IID_PPV_ARGS(&g.queue)), "queue");
        Hr(g.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&g.allocator)), "allocator");
        Hr(g.device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, g.allocator.Get(), nullptr, IID_PPV_ARGS(&g.list)), "list");
        g.list->Close();
        Hr(g.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&g.fence)), "fence");
        constexpr auto read = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        for (unsigned e = 0; e < 2; ++e) {
            // Odd widths: the readback rows are padded to 256 bytes, the raw files are not.
            color[e] = Make(37 + e, 23, DXGI_FORMAT_R8G8B8A8_UNORM, 4, D3D12_RESOURCE_STATE_COPY_SOURCE);
            depth[e] = Make(37 + e, 23, DXGI_FORMAT_R32_FLOAT, 4, read);
            motion[e] = Make(19, 12 + e, DXGI_FORMAT_R16G16_FLOAT, 4, read);
        }
        std::filesystem::remove_all(Root());

        // Disarmed: nothing.
        CyberpunkVR_FramegenCapture = 0;
        NewPixels(1); Offer(1); Offer(2); Offer(3);
        Check(!std::filesystem::exists(Root()) || std::filesystem::is_empty(Root()), "disarmed wrote");
        if (trace) std::printf("disarmed %d\n", CyberpunkVR_FramegenCapture.load());

        // A frame shown again, a gap (restarts), then three in a row: 13, 14, 15 written.
        CyberpunkVR_FramegenCapture = 1;
        for (uint32_t f : {10u, 10u, 11u, 13u, 14u, 15u}) { NewPixels(f); Offer(f); }
        const int32_t first = AwaitWritten();
        if (trace) std::printf("round 1 switch %d\n", first);
        Check(first == 2, "round 1 written");
        Report(1, trace, {13, 14, 15}, true);

        // A missing depth fails the capture; armed again, three frames with eye 1 without motion.
        CyberpunkVR_FramegenCapture = 1;
        NewPixels(19); Offer(19, true, false);
        const int32_t failed = CyberpunkVR_FramegenCapture.load();
        if (trace) std::printf("round 2 missing depth %d\n", failed);
        Check(failed == -1, "missing depth");
        CyberpunkVR_FramegenCapture = 1;
        for (uint32_t f : {20u, 21u, 22u}) { NewPixels(f); Offer(f, false, true, true); }
        const int32_t second = AwaitWritten();
        if (trace) std::printf("round 2 switch %d\n", second);
        Check(second == 2, "round 2 written");
        Report(2, trace, {20, 21, 22}, false);
        CyberpunkVR_FramegenCapture = 0;
    } catch (const std::exception& e) {
        std::printf("FAILED: %s\n", e.what());
        return 1;
    }
    if (!trace) std::printf("framegen capture: two captures written and checked\n");
    return 0;
}
