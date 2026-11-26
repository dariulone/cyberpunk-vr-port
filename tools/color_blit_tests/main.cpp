// The colour blitter (include/Render/ColorBlit.hpp) on a WARP device. Every call is drawn and read back: the plain and
// cropped blit, the overlay blends 0..4 (and an unknown mode) with pixel-exact placement, offsets, explicit sizes,
// tints, source and target mips, the separable blur and the black point, the HUD composite with and without the
// engine's constants, layer-only and every bisection mode, and the barrel dot. Where the answer is known exactly it is
// checked; "trace" prints a hash of every byte read back and every answer instead, so the C++ build and the Rust
// build can be compared bit for bit (cmake/ParityTrace.cmake). With Windows' optional "Graphics Tools" installed the
// D3D12 debug layer validates every call as well.
#include "Render/ColorBlit.hpp"
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <stdexcept>
#include <string>
#include <vector>

void Log(const char* fmt, ...) { (void)fmt; }   // the C++ blitter logs through the plugin's Log

using Microsoft::WRL::ComPtr;
static void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
static void Hr(HRESULT hr, const char* text) { if (FAILED(hr)) throw std::runtime_error(text); }

struct Rgba { uint8_t r, g, b, a; };
using Pattern = std::function<Rgba(unsigned x, unsigned y, unsigned mip)>;
constexpr auto kShaderRead = static_cast<D3D12_RESOURCE_STATES>(0xC0);   // both shader-resource states

struct Gpu {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12InfoQueue> info;
    std::vector<ComPtr<ID3D12Resource>> uploads;   // alive until the list that copies them has run
    uint64_t fenceValue = 0;

    Gpu() {
        ComPtr<ID3D12Debug> debug;
        if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
        ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "factory");
        ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)), "warp");
        Hr(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)), "device");
        if (debug) device.As(&info);
        D3D12_COMMAND_QUEUE_DESC qd{}; qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        Hr(device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue)), "queue");
        Hr(device->CreateCommandAllocator(qd.Type, IID_PPV_ARGS(&allocator)), "allocator");
        Hr(device->CreateCommandList(0, qd.Type, allocator.Get(), nullptr, IID_PPV_ARGS(&list)), "list");
        Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "fence");
    }
    void Submit() {
        Hr(list->Close(), "close");
        ID3D12CommandList* lists[]{list.Get()};
        queue->ExecuteCommandLists(1, lists);
        Hr(queue->Signal(fence.Get(), ++fenceValue), "signal");
        const HANDLE e = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        Hr(fence->SetEventOnCompletion(fenceValue, e), "event");
        const bool done = WaitForSingleObject(e, 10000) == WAIT_OBJECT_0; CloseHandle(e);
        Check(done, "GPU timeout");
        Hr(allocator->Reset(), "allocator reset"); Hr(list->Reset(allocator.Get(), nullptr), "list reset");
        uploads.clear();
    }
    ComPtr<ID3D12Resource> Buffer(UINT64 size, D3D12_HEAP_TYPE type, D3D12_RESOURCE_STATES state) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = type;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; d.Width = size; d.Height = 1;
        d.DepthOrArraySize = 1; d.MipLevels = 1; d.SampleDesc.Count = 1; d.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, state, nullptr, IID_PPV_ARGS(&r)), "buffer");
        return r;
    }
    // Created in COPY_DEST and filled from `pattern`, then left in `state`.
    ComPtr<ID3D12Resource> Texture(unsigned w, unsigned h, DXGI_FORMAT f, unsigned mips, bool rt, const Pattern& pattern,
                                   D3D12_RESOURCE_STATES state) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; d.Width = w; d.Height = h;
        d.DepthOrArraySize = 1; d.MipLevels = UINT16(mips); d.Format = f; d.SampleDesc.Count = 1;
        if (rt) d.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_DEST, nullptr,
                                           IID_PPV_ARGS(&r)), "texture");
        Fill(r.Get(), pattern, D3D12_RESOURCE_STATE_COPY_DEST, state);
        return r;
    }
    void Barrier(ID3D12Resource* r, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
        if (before == after) return;
        D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition = {r, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after};
        list->ResourceBarrier(1, &b);
    }
    struct Layout {
        std::vector<D3D12_PLACED_SUBRESOURCE_FOOTPRINT> footprints;
        std::vector<UINT> rows;
        UINT64 total = 0;
    };
    Layout Footprints(ID3D12Resource* t) {
        const auto d = t->GetDesc();
        Layout l; l.footprints.resize(d.MipLevels); l.rows.resize(d.MipLevels);
        device->GetCopyableFootprints(&d, 0, d.MipLevels, 0, l.footprints.data(), l.rows.data(), nullptr, &l.total);
        return l;
    }
    // Every mip of a four-byte-per-texel texture rewritten from `pattern`.
    void Fill(ID3D12Resource* t, const Pattern& pattern, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to) {
        const auto l = Footprints(t);
        auto upload = Buffer(l.total, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
        uint8_t* p = nullptr; Hr(upload->Map(0, nullptr, reinterpret_cast<void**>(&p)), "map upload");
        for (UINT mip = 0; mip < l.footprints.size(); ++mip) {
            const auto& f = l.footprints[mip];
            for (UINT y = 0; y < l.rows[mip]; ++y) for (UINT x = 0; x < f.Footprint.Width; ++x) {
                const Rgba c = pattern(x, y, mip);
                std::memcpy(p + f.Offset + UINT64(y) * f.Footprint.RowPitch + x * 4, &c, 4);
            }
        }
        upload->Unmap(0, nullptr);
        Barrier(t, from, D3D12_RESOURCE_STATE_COPY_DEST);
        for (UINT mip = 0; mip < l.footprints.size(); ++mip) {
            D3D12_TEXTURE_COPY_LOCATION dst{}, src{};
            dst.pResource = t; dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX; dst.SubresourceIndex = mip;
            src.pResource = upload.Get(); src.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; src.PlacedFootprint = l.footprints[mip];
            list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        }
        Barrier(t, D3D12_RESOURCE_STATE_COPY_DEST, to);
        uploads.push_back(upload);
    }
    // Runs everything recorded so far and returns every mip of `t` (in `state`), rows packed tight.
    std::vector<uint8_t> Read(ID3D12Resource* t, D3D12_RESOURCE_STATES state) {
        const auto l = Footprints(t);
        auto readback = Buffer(l.total, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
        Barrier(t, state, D3D12_RESOURCE_STATE_COPY_SOURCE);
        for (UINT mip = 0; mip < l.footprints.size(); ++mip) {
            D3D12_TEXTURE_COPY_LOCATION dst{}, src{};
            dst.pResource = readback.Get(); dst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; dst.PlacedFootprint = l.footprints[mip];
            src.pResource = t; src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX; src.SubresourceIndex = mip;
            list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        }
        Barrier(t, D3D12_RESOURCE_STATE_COPY_SOURCE, state);
        Submit();
        std::vector<uint8_t> out;
        const uint8_t* p = nullptr; Hr(readback->Map(0, nullptr, reinterpret_cast<void**>(const_cast<uint8_t**>(&p))), "map readback");
        for (UINT mip = 0; mip < l.footprints.size(); ++mip) {
            const auto& f = l.footprints[mip];
            for (UINT y = 0; y < l.rows[mip]; ++y) {
                const uint8_t* row = p + f.Offset + UINT64(y) * f.Footprint.RowPitch;
                out.insert(out.end(), row, row + f.Footprint.Width * 4);
            }
        }
        readback->Unmap(0, nullptr);
        return out;
    }
    // Any debug-layer warning or error fails the run.
    void Validate() {
        if (!info) return;
        for (UINT64 i = 0; i < info->GetNumStoredMessagesAllowedByRetrievalFilter(); ++i) {
            SIZE_T bytes = 0; info->GetMessage(i, nullptr, &bytes);
            std::vector<uint8_t> storage(bytes); auto* m = reinterpret_cast<D3D12_MESSAGE*>(storage.data());
            Hr(info->GetMessage(i, m, &bytes), "message");
            if (m->Severity <= D3D12_MESSAGE_SEVERITY_WARNING) {
                std::fprintf(stderr, "%s\n", m->pDescription);
                throw std::runtime_error("D3D12 validation warning/error");
            }
        }
    }
};

static double Decode(uint8_t v) { const double c = v / 255.0; return c <= 0.04045 ? c / 12.92 : std::pow((c + 0.055) / 1.055, 2.4); }

int main(int argc, char** argv) try {
    const bool trace = argc > 1 && std::strcmp(argv[1], "trace") == 0;
    Gpu gpu;
    ID3D12GraphicsCommandList* list = gpu.list.Get();
    uint64_t hash = 1469598103934665603ull;
    const auto mix = [&](const void* data, size_t n) {
        for (size_t i = 0; i < n; ++i) { hash ^= static_cast<const uint8_t*>(data)[i]; hash *= 1099511628211ull; }
    };
    const auto answer = [&](bool v) { const uint8_t b = v ? 1 : 0; mix(&b, 1); return v; };
    const auto read = [&](ID3D12Resource* t, D3D12_RESOURCE_STATES state) {
        auto bytes = gpu.Read(t, state); mix(bytes.data(), bytes.size()); return bytes;
    };
    constexpr unsigned W = 32, H = 16;
    const Pattern source = [](unsigned x, unsigned y, unsigned mip) {
        return Rgba{uint8_t(x * 37 + y * 11 + mip * 50), uint8_t(x * 5 + y * 29), uint8_t(255 - x * 7), uint8_t(x * 16 + y * 8)};
    };
    const Pattern scenePattern = [](unsigned x, unsigned y, unsigned) {
        return Rgba{uint8_t(x * 8), uint8_t(y * 16), uint8_t((x ^ y) * 9), 255};
    };
    const Pattern blurPattern = [](unsigned x, unsigned y, unsigned mip) {
        return Rgba{uint8_t(90 + x * 3), uint8_t(60 + y * 5), uint8_t(30 + mip * 40), uint8_t(40 + x * 11 + y * 7)};
    };
    const Pattern background = [](unsigned, unsigned, unsigned) { return Rgba{40, 80, 120, 200}; };
    const auto RT = D3D12_RESOURCE_STATE_RENDER_TARGET;

    // ---- the plain and the cropped copy into a UNORM target
    ColorBlit blit;
    auto src = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_UNORM, 1, false, source, kShaderRead);
    auto dst = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_UNORM, 1, true, background, RT);
    Check(!answer(blit.RecordBlit(list, src.Get(), dst.Get())), "blit before EnsureInitialized");
    Check(!answer(blit.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_R8G8B8A8_TYPELESS, W, H)), "typeless RTV accepted");
    Check(!answer(blit.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_R16G16B16A16_TYPELESS, W, H)), "typeless RTV accepted");
    Check(!answer(blit.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_UNKNOWN, W, H)), "unknown format accepted");
    Check(!answer(blit.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, 0, H)), "zero width accepted");
    Check(answer(blit.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, W, H)), "blit not initialised");
    Check(answer(blit.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, W, H)), "blit not reused");
    Check(answer(blit.RecordBlit(list, src.Get(), dst.Get())), "blit failed");
    const auto copy = read(dst.Get(), RT);
    for (unsigned y = 0; y < H; ++y) for (unsigned x = 0; x < W; ++x) {
        const Rgba e = source(x, y, 0); const uint8_t* t = &copy[(y * W + x) * 4];
        Check(std::abs(t[0] - e.r) <= 1 && std::abs(t[1] - e.g) <= 1 && std::abs(t[2] - e.b) <= 1 && t[3] == 255, "plain blit is not a copy");
    }
    Check(answer(blit.RecordBlit(list, src.Get(), dst.Get(), 0.25f, 0.25f, 0.75f, 0.75f)), "cropped blit failed");
    Check(read(dst.Get(), RT) != copy, "cropped blit drew the whole source");

    // ---- every overlay blend into an sRGB target with two mips
    ColorBlit over;
    auto hudSource = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_TYPELESS, 3, false, source, kShaderRead);
    auto overTarget = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_TYPELESS, 2, true, background, RT);
    Check(!answer(over.RecordOverlay(list, hudSource.Get(), overTarget.Get())), "overlay before EnsureInitialized");
    Check(answer(over.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, W, H)), "overlay not initialised");
    struct Case { int mode; bool exact; float offX, offY, w, h; bool tint; uint32_t srcMip, dstMip; int blur; float black; };
    const Case cases[] = {
        {0, false, 0, 0, 0, 0, false, 0, 0, 0, 0}, {1, false, 0, 0, 0, 0, false, 0, 0, 0, 0},
        {2, false, 0, 0, 0, 0, false, 0, 0, 0, 0}, {3, false, 0, 0, 0, 0, false, 0, 0, 0, 0},
        {4, false, 0, 0, 0, 0, false, 0, 0, 0, 0}, {7, false, 0, 0, 0, 0, false, 0, 0, 0, 0},
        {0, false, 0, 0, 0, 0, true, 0, 0, 0, 0},  {1, false, 0, 0, 0, 0, true, 0, 0, 0, 0},
        {3, true, 3, 2, 0, 0, false, 0, 0, 0, 0},  {4, true, 0, 0, 0, 0, false, 1, 1, 0, 0},
        {4, true, 0, 0, 0, 0, false, 0, 0, 1, 0},  {4, true, 0, 0, 0, 0, false, 1, 0, 2, 0},
        {0, true, 1, 1, 20, 10, true, 0, 0, 0, 0.2f}, {2, true, -4, -3, 0, 0, true, 2, 1, 1, 0.5f},
    };
    const float tint[4] = {0.5f, 0.75f, 1.0f, 0.6f};
    for (const Case& c : cases) {
        gpu.Fill(overTarget.Get(), background, RT, RT);
        Check(answer(over.RecordOverlay(list, hudSource.Get(), overTarget.Get(), c.mode, c.exact, c.offX, c.offY, c.w, c.h,
                                        c.tint ? tint : nullptr, c.srcMip, c.dstMip, c.blur, c.black)), "overlay failed");
        const auto out = read(overTarget.Get(), RT);
        if ((c.mode == 1 || c.mode == 4) && !c.exact) {
            // Opaque and RGBA replace: the sRGB view decodes, the sRGB target re-encodes -- the source comes back.
            for (unsigned y = 0; y < H; ++y) for (unsigned x = 0; x < W; ++x) {
                const Rgba e = source(x, y, 0); const uint8_t* t = &out[(y * W + x) * 4];
                const double k[4] = {c.tint && c.mode == 4 ? tint[0] : 1.0, c.tint && c.mode == 4 ? tint[1] : 1.0,
                                     c.tint && c.mode == 4 ? tint[2] : 1.0, c.tint && c.mode == 4 ? tint[3] : 1.0};
                const double lin[3] = {Decode(e.r) * k[0], Decode(e.g) * k[1], Decode(e.b) * k[2]};
                for (int i = 0; i < 3; ++i) {
                    const double s = lin[i] <= 0.0031308 ? 12.92 * lin[i] : 1.055 * std::pow(lin[i], 1 / 2.4) - 0.055;
                    Check(std::abs(t[i] - int(std::lround(s * 255))) <= 1, "replace overlay changed the colour");
                }
                Check(c.mode == 1 ? t[3] == 255 : std::abs(t[3] - int(std::lround(e.a * k[3]))) <= 1, "replace overlay alpha");
            }
        }
        if (c.mode == 0 && !c.exact && !c.tint) {
            // Premultiplied: out = src + dst * (1 - a), in linear light.
            for (unsigned i = 0; i < W * H; ++i) {
                const Rgba e = source(i % W, i / W, 0); const Rgba b = background(0, 0, 0);
                const double a = e.a / 255.0, l = Decode(e.r) + Decode(b.r) * (1 - a);
                const double s = std::min(1.0, l <= 0.0031308 ? 12.92 * l : 1.055 * std::pow(l, 1 / 2.4) - 0.055);
                Check(std::abs(out[i * 4] - int(std::lround(s * 255))) <= 1, "premultiplied blend");
            }
        }
    }

    // ---- the HUD composite: the engine's constants absent and present, layer-only, the bisection modes
    ColorBlit hud;
    auto scene = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_UNORM, 1, false, scenePattern, kShaderRead);
    auto hudTexture = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_TYPELESS, 5, false, source, kShaderRead);
    auto hudBlur = gpu.Texture(W / 2, H / 2, DXGI_FORMAT_R8G8B8A8_TYPELESS, 4, false, blurPattern, kShaderRead);
    auto hudTarget = gpu.Texture(W, H, DXGI_FORMAT_R8G8B8A8_TYPELESS, 1, true, background, RT);
    const auto constants = [&](const std::function<void(float*)>& fill) {
        auto b = gpu.Buffer(512, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
        float* p = nullptr; Hr(b->Map(0, nullptr, reinterpret_cast<void**>(&p)), "map constants");
        std::memset(p, 0, 512); fill(p); b->Unmap(0, nullptr);
        return b;
    };
    auto exposure = constants([](float* p) { p[6] = 1.5f; });
    auto frameCB = constants([](float* p) { p[0] = 1.25f; });
    auto hudCBForeign = constants([](float* p) { p[16 * 4 + 2] = 1920; p[16 * 4 + 3] = 1080; });
    auto hudCBOwn = constants([](float* p) {
        auto v = [p](int reg, int c, float x) { p[reg * 4 + c] = x; };
        v(16, 2, float(W)); v(16, 3, float(H));
        v(3, 0, 0.02f); v(3, 1, 0.05f); v(3, 2, 0.001f); v(3, 3, 0.0015f);
        v(7, 0, -0.004f); v(7, 1, -0.002f); v(7, 2, -0.009f); v(7, 3, -0.006f);
        v(8, 0, -0.011f); v(8, 1, -0.007f); v(8, 2, 0.1f); v(8, 3, 0.09f); v(9, 0, 0.05f);
        v(6, 2, 0.9f); v(6, 3, 0.0002f); v(1, 0, 0.8f); v(1, 2, 0.7f);
        v(4, 0, 0.9f); v(4, 1, 0.5f); v(5, 0, 0.5f); v(5, 1, 1.0f);
    });
    const ColorBlit::HudParams defaults{};
    Check(!answer(hud.RecordHudComposite(list, scene.Get(), hudTexture.Get(), hudBlur.Get(), exposure.Get(), frameCB.Get(),
                                         hudCBForeign.Get(), hudTarget.Get(), defaults)), "composite before EnsureInitialized");
    Check(answer(hud.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, W, H)), "composite not initialised");
    Check(!answer(hud.RecordHudComposite(list, scene.Get(), hudTexture.Get(), hudBlur.Get(), nullptr, frameCB.Get(),
                                         hudCBForeign.Get(), hudTarget.Get(), defaults)), "composite without exposure");
    std::vector<std::pair<ColorBlit::HudParams, ID3D12Resource*>> composites;
    composites.push_back({defaults, hudCBForeign.Get()});
    composites.push_back({defaults, hudCBOwn.Get()});
    for (int mode = 1; mode <= 5; ++mode) { auto p = defaults; p.debugMode = float(mode); composites.push_back({p, hudCBOwn.Get()}); }
    { auto p = defaults; p.layerOnly = 1; p.panelBrightness = 0.8f; p.panelShadow = 0.5f; p.panelGlow = 0.7f; composites.push_back({p, hudCBOwn.Get()}); }
    { auto p = defaults; p.layerOnly = 1; p.panelEngineCurve = 1; composites.push_back({p, hudCBForeign.Get()}); }
    { auto p = defaults; p.hudShiftU = 0.01f; p.flicker = 0; p.time = 3; p.targetW = 1; p.targetH = 1; composites.push_back({p, hudCBForeign.Get()}); }
    std::vector<std::vector<uint8_t>> composed;
    for (const auto& [params, cb] : composites) {
        gpu.Fill(hudTarget.Get(), background, RT, RT);
        Check(answer(hud.RecordHudComposite(list, scene.Get(), hudTexture.Get(), hudBlur.Get(), exposure.Get(), frameCB.Get(),
                                            cb, hudTarget.Get(), params)), "composite failed");
        composed.push_back(read(hudTarget.Get(), RT));
        for (size_t i = 3; i < composed.back().size(); i += 4)
            Check(params.layerOnly > 0.5f || composed.back()[i] == 255, "full composite is not opaque");
    }
    Check(composed[0] != composed[1], "the engine's own constants were ignored");
    Check(composed[1] != composed[2], "bisection mode 1 changed nothing");

    // ---- the barrel dot
    gpu.Fill(hudTarget.Get(), background, RT, RT);
    Check(answer(hud.RecordDot(list, hudTarget.Get(), 0.1f, -0.2f, 4.0f, 1, 0, 0, 1)), "dot failed");
    Check(!answer(hud.RecordDot(list, hudTarget.Get(), 2.0f, 0.0f, 4.0f, 1, 0, 0, 1)), "off-screen dot drawn");
    Check(!answer(hud.RecordDot(list, hudTarget.Get(), 0.0f, 0.0f, 0.0f, 1, 0, 0, 1)), "zero-radius dot drawn");
    Check(!answer(hud.RecordDot(list, hudTarget.Get(), std::nanf(""), 0.0f, 4.0f, 1, 0, 0, 1)), "NaN dot drawn");
    const auto dot = read(hudTarget.Get(), RT);
    const uint8_t* centre = &dot[(9 * W + 17) * 4];
    Check(centre[0] == 255 && centre[1] == 0 && centre[2] == 0, "dot missing at its centre");
    Check(dot[0] == 40 && dot[1] == 80 && dot[2] == 120, "dot drew outside its quad");

    // ---- shutdown and rebuild
    blit.Shutdown();
    Check(!answer(blit.RecordBlit(list, src.Get(), dst.Get())), "blit after Shutdown");
    Check(answer(blit.EnsureInitialized(gpu.device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, W, H)), "blit not rebuilt");
    Check(answer(blit.RecordBlit(list, src.Get(), dst.Get())), "rebuilt blit failed");
    Check(read(dst.Get(), RT) == copy, "rebuilt blit differs");
    gpu.Validate();
    if (trace) { std::printf("color_blit trace %016llx\n", static_cast<unsigned long long>(hash)); return 0; }
    std::printf("PASS blit/crop, overlay modes 0-4 (+unknown) with pixel-exact/offset/size/tint/mips/blur/black point, "
                "HUD composite x%zu, dot, shutdown/rebuild%s\n", composites.size(),
                gpu.info ? ", debug layer clean" : ", no debug layer installed (not validated)");
    return 0;
} catch (const std::exception& e) { std::fprintf(stderr, "%s\n", e.what()); return 1; }
