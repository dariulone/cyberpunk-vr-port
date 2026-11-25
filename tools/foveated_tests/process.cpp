#include "mock_nvngx_dlss.h"
#include "Render/FoveatedDlss.hpp"
#include "Render/CommandResources.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Stereo/EngineStereoDlss.hpp"
#include "Runtimes/EyeGaze.hpp"
#include <MinHook.h>
#include <json/json.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <map>
#include <memory>
#include <stdexcept>
#include <string>
#include <variant>
#include <vector>

using Microsoft::WRL::ComPtr;
static int g_menuMode = 0, g_evaluatingEye = -1;
static bool g_engineStereo = false;
static cvr::camera::PoseIdentity g_pose{};

void Log(const char* fmt, ...) { va_list a; va_start(a, fmt); std::vprintf(fmt, a); va_end(a); }
extern "C" int GetMenuMode() { return g_menuMode; }
extern "C" int CyberpunkVR_MainIsRightEye = 0;
namespace cvr::camera { PoseIdentity CurrentNodePoseIdentity() { return g_pose; } }
namespace cvr::engine_stereo { bool Enabled() { return g_engineStereo; } }
namespace cvr::engine_stereo::dlss { int EvaluatingEye() { return g_evaluatingEye; } }

extern "C" {
extern std::atomic<int32_t> CyberpunkVR_TestEyeGazeEnable;
extern std::atomic<float> CyberpunkVR_TestEyeGazeTanX, CyberpunkVR_TestEyeGazeTanY;
extern char CyberpunkVR_FoveatedReport[32768];
extern std::atomic<uint32_t> CyberpunkVR_FoveatedReportSeq;
XRAPI_ATTR XrResult XRAPI_CALL xrGetSystemProperties(XrInstance, XrSystemId, XrSystemProperties*) { return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrCreateActionSet(XrInstance, const XrActionSetCreateInfo*, XrActionSet*) { return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrCreateAction(XrActionSet, const XrActionCreateInfo*, XrAction*) { return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrDestroyActionSet(XrActionSet) { return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrStringToPath(XrInstance, const char*, XrPath*) { return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrSuggestInteractionProfileBindings(XrInstance, const XrInteractionProfileSuggestedBinding*) { return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrCreateActionSpace(XrSession, const XrActionSpaceCreateInfo*, XrSpace*) { return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrDestroySpace(XrSpace) { return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateBoundSourcesForAction(XrSession, const XrBoundSourcesForActionEnumerateInfo*, uint32_t, uint32_t* n, XrPath*) { if (n) *n = 0; return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrGetActionStatePose(XrSession, const XrActionStateGetInfo*, XrActionStatePose* s) { if (s) s->isActive = XR_FALSE; return XR_SUCCESS; }
XRAPI_ATTR XrResult XRAPI_CALL xrLocateSpace(XrSpace, XrSpace, XrTime, XrSpaceLocation* loc) { if (loc) loc->locationFlags = 0; return XR_SUCCESS; }
}

namespace {
void Check(bool ok, const char* msg) { if (!ok) throw std::runtime_error(msg); }
void Hr(HRESULT hr, const char* msg = "D3D12 failure") { if (FAILED(hr)) throw std::runtime_error(msg); }

class FakeParameters final : public cvr::foveated::ngx::Parameters {
    std::map<std::string, std::variant<unsigned, float, ID3D12Resource*>> values;
    template<class T> cvr::foveated::ngx::Result ReadValue(const char* n, T* v) const {
        if (!n || !v) return 0xBAD00005u;
        const auto it = values.find(n);
        if (it == values.end()) return 0xBAD0000Au;
        if (const auto* f = std::get_if<T>(&it->second)) { *v = *f; return 1u; }
        return 0xBAD00005u;
    }
public:
    void Set(const char* n, unsigned v) override { values[n] = v; }
    void Set(const char* n, int v) override { values[n] = static_cast<unsigned>(v); }
    void Set(const char* n, float v) override { values[n] = v; }
    void Set(const char* n, ID3D12Resource* v) override { values[n] = v; }
    cvr::foveated::ngx::Result Get(const char* n, unsigned* v) const override { return ReadValue(n, v); }
    cvr::foveated::ngx::Result Get(const char* n, int* v) const override {
        unsigned u{}; const auto r = ReadValue(n, &u); if (cvr::foveated::ngx::Success(r) && v) *v = static_cast<int>(u); return r;
    }
    cvr::foveated::ngx::Result Get(const char* n, float* v) const override { return ReadValue(n, v); }
    cvr::foveated::ngx::Result Get(const char* n, ID3D12Resource** v) const override { return ReadValue(n, v); }
    void Set(const char*, unsigned long long) override {} void Set(const char*, double) override {}
    void Set(const char*, ID3D11Resource*) override {} void Set(const char*, void*) override {}
    cvr::foveated::ngx::Result Get(const char*, unsigned long long*) const override { return 0xBAD00005u; }
    cvr::foveated::ngx::Result Get(const char*, double*) const override { return 0xBAD00005u; }
    cvr::foveated::ngx::Result Get(const char*, ID3D11Resource**) const override { return 0xBAD00005u; }
    cvr::foveated::ngx::Result Get(const char*, void**) const override { return 0xBAD00005u; }
    void Reset() override { values.clear(); }
};

void Barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* res, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to) {
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {res, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}

float HalfToFloat(uint16_t h) {
    const uint32_t s = static_cast<uint32_t>(h & 0x8000u) << 16u, e = (h >> 10u) & 0x1Fu, m = h & 0x03FFu;
    uint32_t bits = 0;
    if (e == 0u) {
        if (!m) bits = s;
        else { uint32_t mm = m, ee = 113u; while (!(mm & 0x0400u)) { mm <<= 1u; --ee; } bits = s | (ee << 23u) | ((mm & 0x03FFu) << 13u); }
    } else bits = s | ((e == 31u ? 0xFFu : e + 112u) << 23u) | (m << 13u);
    float out = 0.0f; std::memcpy(&out, &bits, sizeof(out)); return out;
}

void WaitForAppliedSettings(ID3D12Device* dev, ID3D12CommandQueue* q, const cvr::foveated::Settings& exp, uint64_t timeoutMs = 2500) {
    for (const uint64_t end = GetTickCount64() + timeoutMs;; Sleep(15)) {
        cvr::foveated::Tick(dev, q);
        if (cvr::foveated::GetAppliedSettings() == exp) return;
        Check(GetTickCount64() < end, "timed out waiting for applied settings");
    }
}

Json::Value WaitAndParseReport(ID3D12Device* dev, ID3D12CommandQueue* q, uint64_t timeoutMs = 2000) {
    uint32_t seqBefore = CyberpunkVR_FoveatedReportSeq.load(std::memory_order_acquire);
    while (seqBefore & 1u) { YieldProcessor(); seqBefore = CyberpunkVR_FoveatedReportSeq.load(std::memory_order_acquire); }
    for (const uint64_t end = GetTickCount64() + timeoutMs;; Sleep(15)) {
        cvr::foveated::Tick(dev, q);
        const uint32_t seqNow = CyberpunkVR_FoveatedReportSeq.load(std::memory_order_acquire);
        if (!(seqNow & 1u) && seqNow != seqBefore) break;
        Check(GetTickCount64() < end, "timed out waiting for fresh foveated report");
    }
    std::string text; bool okSeq = false;
    for (int i = 0; i < 64; ++i) {
        const uint32_t s0 = CyberpunkVR_FoveatedReportSeq.load(std::memory_order_acquire);
        if (s0 & 1u) { YieldProcessor(); continue; }
        text.assign(CyberpunkVR_FoveatedReport, strnlen(CyberpunkVR_FoveatedReport, sizeof(CyberpunkVR_FoveatedReport)));
        std::atomic_thread_fence(std::memory_order_acquire);
        if (s0 == CyberpunkVR_FoveatedReportSeq.load(std::memory_order_relaxed)) { okSeq = true; break; }
        text.clear(); YieldProcessor();
    }
    Check(okSeq && !text.empty(), "foveated report missing or unstable");
    Json::CharReaderBuilder b; Json::Value root; std::string errs;
    std::unique_ptr<Json::CharReader> reader(b.newCharReader());
    Check(reader->parse(text.data(), text.data() + text.size(), &root, &errs), errs.c_str());
    return root;
}
}  // namespace

int main() try {
    auto ngxState = std::make_unique<MockNgxState>();
    auto State = [&]() -> const MockNgxState& { Check(MockNgxReadState(ngxState.get()), "MockNgxReadState"); return *ngxState; };

    char reqBuf[16]{};
    const bool requireDebug = GetEnvironmentVariableA("CVR_REQUIRE_D3D12_DEBUG", reqBuf, sizeof(reqBuf)) == 1 && reqBuf[0] == '1';
    ComPtr<ID3D12Debug> debug;
    if (const HRESULT hr = D3D12GetDebugInterface(IID_PPV_ARGS(&debug)); FAILED(hr)) {
        if (!requireDebug) { std::cout << "SKIP: D3D12 Debug Layer unavailable (hr=0x" << std::hex << static_cast<unsigned long>(hr) << std::dec << ")\n"; return 77; }
        Hr(hr, "D3D12GetDebugInterface");
    }
    debug->EnableDebugLayer();

    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device; Hr(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)));
    ComPtr<ID3D12InfoQueue> infoQueue; Hr(device.As(&infoQueue));
    ComPtr<ID3D12CommandQueue> queue; D3D12_COMMAND_QUEUE_DESC qd{}; Hr(device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12CommandAllocator> alloc; Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&alloc)));
    ComPtr<ID3D12GraphicsCommandList> list; Hr(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, alloc.Get(), nullptr, IID_PPV_ARGS(&list)));
    ComPtr<ID3D12Fence> fence; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)));
    HANDLE fenceEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr); Check(fenceEvent != nullptr, "CreateEventW");
    uint64_t fenceVal = 0;

    auto ExecuteList = [&]() {
        Hr(list->Close()); ID3D12CommandList* lists[]{list.Get()};
        cvr::foveated::BeforeSubmit(queue.Get(), 1, lists);
        auto res = cvr::gpu::PrepareCommandResources(1, lists);
        queue->ExecuteCommandLists(1, lists); cvr::gpu::SubmitCommandResources(queue.Get(), std::move(res));
        Hr(queue->Signal(fence.Get(), ++fenceVal)); Hr(fence->SetEventOnCompletion(fenceVal, fenceEvent));
        Check(WaitForSingleObject(fenceEvent, 10000) == WAIT_OBJECT_0, "GPU timeout");
        cvr::foveated::ResetList(list.Get()); cvr::gpu::ResetCommandResources(list.Get()); cvr::gpu::CollectCommandResources();
        Hr(alloc->Reset()); Hr(list->Reset(alloc.Get(), nullptr));
    };
    auto Texture = [&](unsigned w, unsigned h, DXGI_FORMAT fmt, D3D12_RESOURCE_FLAGS fl, D3D12_RESOURCE_STATES st) {
        D3D12_RESOURCE_DESC d{D3D12_RESOURCE_DIMENSION_TEXTURE2D, 0, w, h, 1, 1, fmt, {1, 0}, D3D12_TEXTURE_LAYOUT_UNKNOWN, fl};
        D3D12_HEAP_PROPERTIES hp{D3D12_HEAP_TYPE_DEFAULT};
        ComPtr<ID3D12Resource> r; Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, st, nullptr, IID_PPV_ARGS(&r))); return r;
    };
    auto Buffer = [&](UINT64 bytes, D3D12_HEAP_TYPE type, D3D12_RESOURCE_STATES st) {
        D3D12_RESOURCE_DESC d{D3D12_RESOURCE_DIMENSION_BUFFER, 0, bytes, 1, 1, 1, DXGI_FORMAT_UNKNOWN, {1, 0}, D3D12_TEXTURE_LAYOUT_ROW_MAJOR};
        D3D12_HEAP_PROPERTIES hp{type};
        ComPtr<ID3D12Resource> r; Hr(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d, st, nullptr, IID_PPV_ARGS(&r))); return r;
    };

    constexpr unsigned kRenderW = 96, kRenderH = 72, kOutputW = 128, kOutputH = 96;
    auto color = Texture(kRenderW, kRenderH, DXGI_FORMAT_R32G32B32A32_FLOAT, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST);
    auto depth = Texture(kRenderW, kRenderH, DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST);
    auto motion = Texture(kRenderW, kRenderH, DXGI_FORMAT_R32G32_FLOAT, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST);
    auto output = Texture(kOutputW, kOutputH, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);

    std::vector<ComPtr<ID3D12Resource>> uploads;
    for (auto [tex, comps] : {std::pair{color.Get(), 4u}, {depth.Get(), 1u}, {motion.Get(), 2u}}) {
        const auto d = tex->GetDesc(); D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{}; UINT64 bytes = 0;
        device->GetCopyableFootprints(&d, 0, 1, 0, &fp, nullptr, nullptr, &bytes);
        auto up = Buffer(bytes, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
        void* p = nullptr; Hr(up->Map(0, nullptr, &p));
        for (unsigned y = 0; y < d.Height; ++y) {
            auto* row = reinterpret_cast<float*>(static_cast<char*>(p) + y * fp.Footprint.RowPitch);
            for (unsigned x = 0; x < d.Width; ++x) {
                float* v = row + x * comps;
                if (comps == 4) { v[0] = 0.25f; v[1] = 0.5f; v[2] = 0.75f; v[3] = 1.0f; }
                else if (comps == 2) { v[0] = 0.25f; v[1] = -0.125f; } else v[0] = 0.5f;
            }
        }
        up->Unmap(0, nullptr);
        D3D12_TEXTURE_COPY_LOCATION src{up.Get(), D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT, fp}, dst{tex, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, {}};
        list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        Barrier(list.Get(), tex, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        uploads.push_back(std::move(up));
    }
    ExecuteList(); uploads.clear();

    auto Readback = [&](ID3D12Resource* res, auto&& convert) {
        using Pixel = decltype(convert(static_cast<const char*>(nullptr), 0u));
        const auto d = res->GetDesc(); D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{}; UINT64 bytes = 0;
        device->GetCopyableFootprints(&d, 0, 1, 0, &fp, nullptr, nullptr, &bytes);
        auto rb = Buffer(bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
        Barrier(list.Get(), res, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION src{res, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, {}}, dst{rb.Get(), D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT, fp};
        list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        Barrier(list.Get(), res, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        ExecuteList();
        std::vector<Pixel> pixels; pixels.reserve(static_cast<size_t>(d.Width) * d.Height);
        void* mapped = nullptr; Hr(rb->Map(0, nullptr, &mapped));
        for (unsigned y = 0; y < d.Height; ++y)
            for (unsigned x = 0; x < d.Width; ++x)
                pixels.push_back(convert(static_cast<const char*>(mapped) + y * fp.Footprint.RowPitch, x));
        rb->Unmap(0, nullptr);
        return pixels;
    };

    Check(MH_Initialize() == MH_OK, "MH_Initialize failed");
    HMODULE ngxMod = LoadLibraryW(L"nvngx_dlss.dll");
    Check(ngxMod && GetProcAddress(ngxMod, "NVSDK_NGX_D3D12_EvaluateFeature") &&
              GetProcAddress(ngxMod, "NVSDK_NGX_D3D12_EvaluateFeature_C"), "missing NGX exports");
    MockNgxReset(); cvr::foveated::Tick(device.Get(), queue.Get());
    Check(cvr::foveated::GetStatus().loaded && cvr::foveated::GetStatus().ready, "hooks did not install");

    g_pose = {1, 1, 0, 1, {0, 0, 0, 0, 0, 0, 1, true, 0, {}, 0, false, -1}};
    FakeParameters params;
    for (auto [k, v] : {std::pair{"Width", kRenderW}, {"Height", kRenderH},
                        {"DLSS.Render.Subrect.Dimensions.Width", kRenderW}, {"DLSS.Render.Subrect.Dimensions.Height", kRenderH},
                        {"OutWidth", kOutputW}, {"OutHeight", kOutputH}, {"PerfQualityValue", 1u}, {"DLSS.Feature.Create.Flags", 0x4Bu},
                        {"DLSS.Hint.Render.Preset.DLAA", 11u}, {"DLSS.Hint.Render.Preset.Quality", 11u}, {"DLSS.Hint.Render.Preset.Balanced", 11u},
                        {"DLSS.Hint.Render.Preset.Performance", 13u}, {"DLSS.Hint.Render.Preset.UltraPerformance", 12u}, {"DLSS.Hint.Render.Preset.UltraQuality", 11u}})
        params.Set(k, v);
    for (const char* z : {"DLSS.Input.Color.Subrect.Base.X", "DLSS.Input.Color.Subrect.Base.Y", "DLSS.Input.Depth.Subrect.Base.X",
                          "DLSS.Input.Depth.Subrect.Base.Y", "DLSS.Input.MV.Subrect.Base.X", "DLSS.Input.MV.Subrect.Base.Y",
                          "DLSS.Output.Subrect.Base.X", "DLSS.Output.Subrect.Base.Y", "Reset"}) params.Set(z, 0u);
    params.Set("MV.Scale.X", 1.0f); params.Set("MV.Scale.Y", 1.0f); params.Set("Jitter.Offset.X", 0.25f); params.Set("Jitter.Offset.Y", -0.25f);
    params.Set("Color", color.Get()); params.Set("Depth", depth.Get()); params.Set("MotionVectors", motion.Get()); params.Set("Output", output.Get());

    cvr::foveated::ngx::Handle* gameHandle = nullptr;
    Check(cvr::foveated::ngx::Success(NVSDK_NGX_D3D12_CreateFeature(list.Get(), 1u, &params, &gameHandle)) && gameHandle, "game CreateFeature");
    Check(State().createCount == 1, "expected 1 CreateFeature");

    auto RunEval = [&](bool useC = false) {
        auto fn = useC ? NVSDK_NGX_D3D12_EvaluateFeature_C : NVSDK_NGX_D3D12_EvaluateFeature;
        Check(cvr::foveated::ngx::Success(fn(list.Get(), gameHandle, &params, nullptr)), "EvaluateFeature failed");
        ExecuteList();
    };
    auto Report = [&]() { return WaitAndParseReport(device.Get(), queue.Get()); };

    const cvr::foveated::Settings base{2, 50.0f, 50.0f, false, 0.0f, 0.0f, 0, true};
    cvr::foveated::SetSettings(base); Check(cvr::foveated::GetAppliedSettings().mode == 0, "initial mode must debounce");
    WaitForAppliedSettings(device.Get(), queue.Get(), base);
    auto moved = base; moved.offsetXPercent = 6.0f; moved.offsetYPercent = -4.0f; moved.eyeTracked = false;
    cvr::foveated::SetSettings(moved); Check(cvr::foveated::GetAppliedSettings() == moved, "SameAllocation change immediate");
    cvr::foveated::SetSettings(base); Check(cvr::foveated::GetAppliedSettings() == base, "SameAllocation restore immediate");

    g_menuMode = 1; RunEval();
    {
        const auto& st = State();
        Check(st.createCount == 1 && st.evaluateCount == 1 && st.evaluates[0].handle == gameHandle &&
                  st.evaluates[0].outWidth == kOutputW && st.evaluates[0].outHeight == kOutputH, "menu-mode native bypass");
    }

    constexpr float kDeg = 3.1415926535f / 180.0f;
    const cvr::eyegaze::FrustumTangents q3[2]{
        {std::tan(-54.0f * kDeg), std::tan(40.0f * kDeg), std::tan(-49.0f * kDeg), std::tan(50.0f * kDeg)},
        {std::tan(-40.0f * kDeg), std::tan(54.0f * kDeg), std::tan(-49.0f * kDeg), std::tan(50.0f * kDeg)},
    };
    auto SetGaze = [&](float tx, float ty) {
        CyberpunkVR_TestEyeGazeTanX.store(tx, std::memory_order_relaxed);
        CyberpunkVR_TestEyeGazeTanY.store(ty, std::memory_order_relaxed);
        cvr::eyegaze::Update(XR_NULL_HANDLE, XR_SESSION_STATE_FOCUSED, XR_NULL_HANDLE, 0, true, q3, true, 0.064f);
    };
    g_menuMode = 0; g_engineStereo = true; CyberpunkVR_TestEyeGazeEnable.store(1, std::memory_order_relaxed); SetGaze(0.0f, 0.0f);

    g_evaluatingEye = 0; Check(cvr::foveated::ngx::Success(NVSDK_NGX_D3D12_EvaluateFeature(list.Get(), gameHandle, &params, nullptr)), "left eval");
    g_evaluatingEye = 1; Check(cvr::foveated::ngx::Success(NVSDK_NGX_D3D12_EvaluateFeature(list.Get(), gameHandle, &params, nullptr)), "right eval");
    ExecuteList();

    const cvr::foveated::ngx::Handle *leftPeriph = nullptr, *leftCenter = nullptr;
    {
        const auto& st = State();
        Check(st.createCount == 5 && st.evaluateCount == 5, "expected 4 private handles & 5 total evaluates");
        Check(st.evaluates[1].reset == 1 && st.evaluates[2].reset == 1 && st.evaluates[3].reset == 1 && st.evaluates[4].reset == 1, "first frame Reset==1");
        Check(st.evaluates[1].handle != st.evaluates[3].handle && st.evaluates[2].handle != st.evaluates[4].handle, "per-eye handles");
        Check(st.evaluates[1].perfQualityValue == 5 && st.evaluates[2].perfQualityValue == 1, "periphery DLAA / center quality");
        leftPeriph = st.evaluates[1].handle; leftCenter = st.evaluates[2].handle;
    }
    {
        const auto r = Report(); const auto& views = r["view_details"];
        Check(views.size() == 2 && views[0]["eye"].asString() == "left" && views[1]["eye"].asString() == "right", "left/right views");
        Check(views[0]["offset_x_percent"].asFloat() > 0.0f && views[1]["offset_x_percent"].asFloat() < 0.0f &&
                  views[0]["crop_x"].asUInt() > views[1]["crop_x"].asUInt(), "stereo vergence crop divergence");
    }

    SetGaze(0.08f, -0.05f); g_evaluatingEye = 0; RunEval();
    ID3D12Resource* centerMv = nullptr;
    {
        const auto& st = State();
        Check(st.createCount == 5 && st.evaluateCount == 7 &&
                  st.evaluates[5].handle == leftPeriph && st.evaluates[6].handle == leftCenter &&
                  st.evaluates[5].reset == 0 && st.evaluates[6].reset == 0, "smooth pursuit reuse & Reset==0");
        centerMv = st.evaluates[6].motionVectors;
    }
    Check(centerMv != nullptr, "missing center motionVectors");
    const auto shiftedMv = Readback(centerMv, [](const char* row, unsigned x) {
        const auto* f = reinterpret_cast<const float*>(row) + x * 2; return std::array<float, 2>{f[0], f[1]};
    });
    Check(std::fabs(shiftedMv[0][0] - 0.25f) > 0.5f && std::fabs(shiftedMv[0][1] + 0.125f) > 0.5f, "cropDelta motion compensation");
    {
        const auto r = Report(); const auto& v0 = r["view_details"][0];
        const unsigned cx = v0["crop_x"].asUInt(), cy = v0["crop_y"].asUInt(), cw = v0["crop_width"].asUInt(), ch = v0["crop_height"].asUInt();
        Check(cw >= 16 && ch >= 16 && cx + cw <= kOutputW && cy + ch <= kOutputH, "crop bounds");
        float pCol[4]{}, cCol[4]{};
        Check(MockNgxGetHandleClearColor(leftPeriph, pCol) && MockNgxGetHandleClearColor(leftCenter, cCol) && std::fabs(pCol[0] - cCol[0]) > 0.01f, "distinct clear colors");
        const auto outPx = Readback(output.Get(), [](const char* row, unsigned x) {
            const auto* h = reinterpret_cast<const uint16_t*>(row) + x * 4;
            return std::array<float, 4>{HalfToFloat(h[0]), HalfToFloat(h[1]), HalfToFloat(h[2]), HalfToFloat(h[3])};
        });
        const auto& inPx = outPx[size_t(cy + ch / 2) * kOutputW + (cx + cw / 2)];
        const auto& exPx = outPx[size_t((cy >= 4) ? 1u : (kOutputH - 2u)) * kOutputW + ((cx >= 4) ? 1u : (kOutputW - 2u))];
        for (size_t c = 0; c < 4; ++c) {
            Check(std::fabs(inPx[c] - cCol[c]) < 1e-3f, "composited foveal interior mismatch");
            Check(std::fabs(exPx[c] - pCol[c]) < 1e-3f, "composited periphery mismatch");
        }
    }

    SetGaze(-0.65f, -0.05f); g_evaluatingEye = 0; RunEval();
    { const auto& st = State(); Check(st.createCount == 5 && st.evaluateCount == 9 && st.evaluates[7].reset == 0 && st.evaluates[8].reset == 1, "saccade reset"); }
    {
        const auto r = Report();
        Check(r["eye_tracked"].asBool() && r["gaze_state"].asInt() == static_cast<int>(cvr::eyegaze::TrackingState::Active) &&
                  r["view_details"].size() == 2 && r["view_details"][0]["eye"].asString() == "left" &&
                  r["view_details"][1]["eye"].asString() == "right" && r["view_details"][0]["resets"].asUInt64() >= 1, "saccade report");
    }

    SetGaze(0.0f, 0.0f);
    const auto gazeSnap = cvr::eyegaze::GetSnapshot();
    g_engineStereo = false; g_evaluatingEye = -1; CyberpunkVR_MainIsRightEye = 1; g_pose.view = 1; g_pose.head.afwEye = -1;
    RunEval();
    { const auto r = Report(); const auto& v0 = r["view_details"][0]; Check(v0["eye"].asString() == "MAIN" && std::fabs(v0["offset_x_percent"].asFloat() - gazeSnap.offsetX[1]) < 1e-3f, "MAIN routing"); }
    g_pose.head.afwEye = 0; RunEval(true);
    Check(State().evaluateCCount == 2, "EvaluateFeature_C dispatch");
    Check(std::fabs(Report()["view_details"][0]["offset_x_percent"].asFloat() - gazeSnap.offsetX[0]) < 1e-3f, "afwEye=0 routing");

    const auto evalBeforeFail = State().evaluateCount;
    MockNgxSetFailNextEvaluate(true);
    Check(cvr::foveated::ngx::Success(NVSDK_NGX_D3D12_EvaluateFeature(list.Get(), gameHandle, &params, nullptr)), "NGX fallback");
    {
        const auto& st = State();
        Check(st.evaluateCount == evalBeforeFail + 2 && st.evaluates[evalBeforeFail].result == 0xBAD00005u &&
                  st.evaluates[evalBeforeFail + 1].handle == gameHandle && st.evaluates[evalBeforeFail + 1].reset == 1, "native fallback Reset==1");
    }
    ExecuteList(); Check(Report()["apis"][1]["failures"].asUInt64() >= 1, "failures counter");

    cvr::foveated::Shutdown(); cvr::foveated::Tick(device.Get(), queue.Get());
    { const auto& st = State(); Check(st.releaseCount == 4 && st.aliveHandles == 1 && MockNgxIsHandleAlive(gameHandle), "deferred release"); }
    Check(cvr::foveated::ngx::Success(NVSDK_NGX_D3D12_ReleaseFeature(gameHandle)) && State().aliveHandles == 0, "game release");

    Hr(list->Close()); CloseHandle(fenceEvent);
    Check(MH_DisableHook(MH_ALL_HOOKS) == MH_OK && MH_Uninitialize() == MH_OK, "MinHook shutdown");
    MockNgxReset();

    for (UINT64 i = 0, n = infoQueue->GetNumStoredMessages(); i < n; ++i) {
        SIZE_T len = 0; infoQueue->GetMessage(i, nullptr, &len); std::vector<char> b(len);
        auto* m = reinterpret_cast<D3D12_MESSAGE*>(b.data()); infoQueue->GetMessage(i, m, &len);
        if (m->Severity <= D3D12_MESSAGE_SEVERITY_WARNING) { std::cerr << m->pDescription << '\n'; throw std::runtime_error("D3D12 validation"); }
    }
    std::cout << "PASS FoveatedDlss end-to-end orchestration\n";
    return 0;
} catch (const std::exception& e) { std::cerr << "FAIL: " << e.what() << '\n'; return 1; }
