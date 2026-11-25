#include "test_common.hpp"
#include "Render/CommandResources.hpp"
#include "Render/FixedVrs.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/SinglePassPipelines.hpp"
#include "Render/ViewInstancedPipeline.hpp"
#include "Stereo/SinglePass.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Utils/DebugGate.hpp"
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <thread>
#if defined(VRS_HAVE_VI_SHADERS)
#include "vi_vs.h"
#include "vi_ps.h"
#endif
using namespace vrs_test;

void Log(const char*, ...) {}
extern "C" {
__declspec(dllexport) int CyberpunkVR_RuntimeDiagnostics = 0;
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PresentTiming[8]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SinglePassPipelineCounts[12]{};
__declspec(dllexport) char CyberpunkVR_SinglePassError[256]{};
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_SinglePassMismatch[12];
__declspec(dllexport) extern char CyberpunkVR_FixedVrsReport[1024];
}

static ID3D12RootSignature *g_engineRootPtr{}, *g_twinRootPtr{};
static ID3D12PipelineState *g_engineOpaquePsoPtr{}, *g_twinOpaquePsoPtr{};
namespace cvr::single_pass {
struct Pipeline { int dummy = 0; };
static Pipeline g_dummyPipeline{};
bool Available() { return true; }
ID3D12RootSignature* TwinRoot(ID3D12RootSignature* r) { return (r && r == g_engineRootPtr) ? g_twinRootPtr : nullptr; }
Pipeline* Find(ID3D12PipelineState* p) { return (p && p == g_engineOpaquePsoPtr) ? &g_dummyPipeline : nullptr; }
ID3D12RootSignature* RootOf(const Pipeline*) { return g_engineRootPtr; }
uint32_t InputSlots(const Pipeline*) { return 0; }
ID3D12PipelineState* Twin(Pipeline*, ID3D12PipelineState* eng, ID3D12PipelineState* bound) {
    return (eng == g_engineOpaquePsoPtr && bound == g_engineOpaquePsoPtr) ? g_twinOpaquePsoPtr : nullptr;
}
}

static void* g_rawVtable[81]{};
static ID3D12GraphicsCommandList* g_testListPtr{};
static cvr::detail::CommandListVtableHook g_hookEntry{};
const cvr::detail::CommandListVtableHook* cvr::detail::command_list_hook_entry(ID3D12GraphicsCommandList* list) {
    return (list && list == g_testListPtr) ? &g_hookEntry : nullptr;
}

struct HwState {
    ID3D12PipelineState* pso{}; UINT vpCount{}, scCount{};
    D3D12_SHADING_RATE shadingRate = D3D12_SHADING_RATE_1X1;
    std::array<D3D12_SHADING_RATE_COMBINER, 2> combiners{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH, D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
    ID3D12Resource* shadingImage{};
};
struct RecordedEvent {
    enum class Kind { SetPso, Viewports, Scissors, ShadingRate, ShadingImage, Draw, DrawIndexed };
    Kind kind{}; ID3D12PipelineState* pso{}; UINT vpCount{}, scCount{};
    D3D12_SHADING_RATE shadingRate = D3D12_SHADING_RATE_1X1;
    std::array<D3D12_SHADING_RATE_COMBINER, 2> combiners{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH, D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
    ID3D12Resource* shadingImage{};
};
static HwState g_hwState{};
static std::vector<RecordedEvent> g_events;
static void PushHw(RecordedEvent::Kind k) {
    g_events.push_back({k, g_hwState.pso, g_hwState.vpCount, g_hwState.scCount, g_hwState.shadingRate, g_hwState.combiners, g_hwState.shadingImage});
}

static void STDMETHODCALLTYPE SpyDrawInstanced(ID3D12GraphicsCommandList* l, UINT vtx, UINT inst, UINT sv, UINT si) {
    PushHw(RecordedEvent::Kind::Draw); reinterpret_cast<cvr::detail::PFN_DrawInstanced>(g_rawVtable[12])(l, vtx, inst, sv, si);
}
static void STDMETHODCALLTYPE SpyDrawIndexedInstanced(ID3D12GraphicsCommandList* l, UINT idx, UINT inst, UINT si, INT bv, UINT sinst) {
    PushHw(RecordedEvent::Kind::DrawIndexed); reinterpret_cast<cvr::detail::PFN_DrawIndexedInstanced>(g_rawVtable[13])(l, idx, inst, si, bv, sinst);
}
static void STDMETHODCALLTYPE SpySetPipelineState(ID3D12GraphicsCommandList* l, ID3D12PipelineState* pso) {
    g_hwState.pso = pso; PushHw(RecordedEvent::Kind::SetPso); reinterpret_cast<cvr::detail::PFN_SetPipelineState>(g_rawVtable[25])(l, pso);
}
static void STDMETHODCALLTYPE SpyRSSetViewports(ID3D12GraphicsCommandList* l, UINT n, const D3D12_VIEWPORT* v) {
    g_hwState.vpCount = n; PushHw(RecordedEvent::Kind::Viewports); reinterpret_cast<cvr::detail::PFN_RSSetViewports>(g_rawVtable[21])(l, n, v);
}
static void STDMETHODCALLTYPE SpyRSSetScissorRects(ID3D12GraphicsCommandList* l, UINT n, const D3D12_RECT* r) {
    g_hwState.scCount = n; PushHw(RecordedEvent::Kind::Scissors); reinterpret_cast<cvr::detail::PFN_RSSetScissorRects>(g_rawVtable[22])(l, n, r);
}
static void STDMETHODCALLTYPE SpyRSSetShadingRate(ID3D12GraphicsCommandList5* l5, D3D12_SHADING_RATE rate, const D3D12_SHADING_RATE_COMBINER* c) {
    g_hwState.shadingRate = rate;
    g_hwState.combiners = c ? std::array{c[0], c[1]} : std::array{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH, D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
    PushHw(RecordedEvent::Kind::ShadingRate); reinterpret_cast<cvr::detail::PFN_ProbeShadingRate>(g_rawVtable[77])(l5, rate, c);
}
static void STDMETHODCALLTYPE SpyRSSetShadingRateImage(ID3D12GraphicsCommandList5* l5, ID3D12Resource* img) {
    g_hwState.shadingImage = img; PushHw(RecordedEvent::Kind::ShadingImage); reinterpret_cast<cvr::detail::PFN_ProbeShadingImage>(g_rawVtable[78])(l5, img);
}
static void STDMETHODCALLTYPE vrs_set_shading_rate(ID3D12GraphicsCommandList5* l5, D3D12_SHADING_RATE rate, const D3D12_SHADING_RATE_COMBINER* c) {
    auto* base = static_cast<ID3D12GraphicsCommandList*>(l5);
    if (const auto* e = cvr::detail::command_list_hook_entry(base); e && e->shading_rate_original)
        if (cvr::stereo::probe::internalCommands || !cvr::single_pass::ShadingRate(base, rate, c)) e->shading_rate_original(l5, rate, c);
}
static void STDMETHODCALLTYPE vrs_set_shading_image(ID3D12GraphicsCommandList5* l5, ID3D12Resource* img) {
    auto* base = static_cast<ID3D12GraphicsCommandList*>(l5);
    if (const auto* e = cvr::detail::command_list_hook_entry(base); e && e->shading_image_original)
        if (cvr::stereo::probe::internalCommands || !cvr::single_pass::ShadingImage(base, img)) e->shading_image_original(l5, img);
}
static void STDMETHODCALLTYPE ProbeShadingRate(ID3D12GraphicsCommandList5* l5, D3D12_SHADING_RATE rate, const D3D12_SHADING_RATE_COMBINER* c) {
    auto* base = static_cast<ID3D12GraphicsCommandList*>(l5);
    if (base != g_testListPtr) { reinterpret_cast<cvr::detail::PFN_ProbeShadingRate>(g_rawVtable[77])(l5, rate, c); return; }
    const auto* e = cvr::detail::command_list_hook_entry(base); if (!e || !e->shading_rate_original) return;
    if (cvr::stereo::probe::internalCommands) { e->shading_rate_original(l5, rate, c); return; }
    cvr::vrs::Restore(base);
    if (!cvr::single_pass::ShadingRate(base, rate, c)) e->shading_rate_original(l5, rate, c);
    cvr::vrs::ShadingRate(base, rate, c);
}
static void STDMETHODCALLTYPE ProbeShadingImage(ID3D12GraphicsCommandList5* l5, ID3D12Resource* img) {
    auto* base = static_cast<ID3D12GraphicsCommandList*>(l5);
    if (base != g_testListPtr) { reinterpret_cast<cvr::detail::PFN_ProbeShadingImage>(g_rawVtable[78])(l5, img); return; }
    const auto* e = cvr::detail::command_list_hook_entry(base); if (!e || !e->shading_image_original) return;
    if (cvr::stereo::probe::internalCommands) { e->shading_image_original(l5, img); return; }
    cvr::vrs::Restore(base);
    if (!cvr::single_pass::ShadingImage(base, img)) e->shading_image_original(l5, img);
    cvr::vrs::ShadingImage(base, img);
}

static std::vector<RecordedEvent> CollectDrawEvents() {
    std::vector<RecordedEvent> out;
    for (const auto& ev : g_events) if (ev.kind == RecordedEvent::Kind::Draw || ev.kind == RecordedEvent::Kind::DrawIndexed) out.push_back(ev);
    return out;
}
static void InitHookTable(void** vt) {
    for (size_t i = 0; i < 81; ++i) g_rawVtable[i] = vt[i];
    g_hookEntry = {}; g_hookEntry.vtable = vt;
    g_hookEntry.original = reinterpret_cast<cvr::detail::PFN_OMSetRenderTargets>(g_rawVtable[46]);
    g_hookEntry.barrier_original = g_hookEntry.barrier_call = reinterpret_cast<cvr::detail::PFN_ResourceBarrier>(g_rawVtable[26]);
    g_hookEntry.copyres = reinterpret_cast<cvr::detail::PFN_CopyResource>(g_rawVtable[17]);
    g_hookEntry.copytex = reinterpret_cast<cvr::detail::PFN_CopyTextureRegion>(g_rawVtable[16]);
    g_hookEntry.indirect_original = reinterpret_cast<cvr::detail::PFN_ExecuteIndirect>(g_rawVtable[59]);
    g_hookEntry.draw_original = &SpyDrawInstanced; g_hookEntry.drawidx_original = &SpyDrawIndexedInstanced;
    g_hookEntry.cbr_original = reinterpret_cast<cvr::detail::PFN_CopyBufferRegion>(g_rawVtable[15]);
    g_hookEntry.dispatch_original = reinterpret_cast<cvr::detail::PFN_Dispatch>(g_rawVtable[14]);
    g_hookEntry.viewports_original = &SpyRSSetViewports; g_hookEntry.scissor_original = &SpyRSSetScissorRects;
    g_hookEntry.reset_original = reinterpret_cast<cvr::detail::PFN_GfxReset>(g_rawVtable[10]);
    g_hookEntry.setpso_original = &SpySetPipelineState;
    g_hookEntry.iavb_original = reinterpret_cast<cvr::detail::PFN_IASetVertexBuffers>(g_rawVtable[44]);
    g_hookEntry.cleardsv_original = reinterpret_cast<cvr::detail::PFN_ClearDepthStencilView>(g_rawVtable[47]);
    g_hookEntry.crootsig_original = reinterpret_cast<cvr::detail::PFN_SetComputeRootSignature>(g_rawVtable[29]);
    g_hookEntry.crootdt_original = reinterpret_cast<cvr::detail::PFN_SetComputeRootDescriptorTable>(g_rawVtable[31]);
    g_hookEntry.groot_original = reinterpret_cast<cvr::detail::PFN_SetComputeRootSignature>(g_rawVtable[30]);
    g_hookEntry.grootdt_original = reinterpret_cast<cvr::detail::PFN_SetComputeRootDescriptorTable>(g_rawVtable[32]);
    g_hookEntry.grootcbv_original = reinterpret_cast<cvr::detail::PFN_SetGraphicsRootCbv>(g_rawVtable[38]);
    g_hookEntry.iaib_original = reinterpret_cast<cvr::detail::PFN_IASetIndexBuffer>(g_rawVtable[43]);
    g_hookEntry.predication_original = reinterpret_cast<cvr::detail::PFN_SetPredication>(g_rawVtable[55]);
    g_hookEntry.heaps_original = reinterpret_cast<cvr::detail::PFN_ProbeHeaps>(g_rawVtable[28]);
    g_hookEntry.topology_original = reinterpret_cast<cvr::detail::PFN_ProbeTopology>(g_rawVtable[20]);
    g_hookEntry.stencil_original = reinterpret_cast<cvr::detail::PFN_ProbeStencil>(g_rawVtable[24]);
    g_hookEntry.shading_rate_original = &SpyRSSetShadingRate; g_hookEntry.shading_image_original = &SpyRSSetShadingRateImage;
    g_hookEntry.shading_state_known = true;
    g_hookEntry.close_original = reinterpret_cast<cvr::detail::PFN_GfxClose>(g_rawVtable[9]);
    g_hookEntry.clear_state_original = reinterpret_cast<decltype(g_hookEntry.clear_state_original)>(g_rawVtable[11]);
    g_hookEntry.bundle_original = reinterpret_cast<decltype(g_hookEntry.bundle_original)>(g_rawVtable[27]);
    g_hookEntry.discard_original = reinterpret_cast<decltype(g_hookEntry.discard_original)>(g_rawVtable[51]);
    g_hookEntry.blend_factor_original = reinterpret_cast<decltype(g_hookEntry.blend_factor_original)>(g_rawVtable[23]);
    g_hookEntry.resolve_query_original = reinterpret_cast<decltype(g_hookEntry.resolve_query_original)>(g_rawVtable[54]);
}

static std::pair<unsigned, unsigned> VerifyStereoPixels(
    const std::vector<uint8_t>& baseline, const std::vector<uint8_t>& actual, cvr::vrs::Settings leftS, cvr::vrs::Settings rightS,
    UINT w, UINT h, UINT tileSize, bool additionalRates, unsigned* outAsymmetry = nullptr) {
    const auto rates = cvr::vrs::BuildStereoRates(leftS, rightS, w, h, tileSize, additionalRates);
    const UINT cols = (w + tileSize - 1) / tileSize, eyeW = w / 2;
    unsigned leftDiff = 0, rightDiff = 0, asymmetry = 0;
    for (UINT y = 0; y < h; ++y) for (UINT x = 0; x < w; ++x) {
        const size_t off = (size_t(y) * w + x) * 4;
        const bool same = std::memcmp(baseline.data() + off, actual.data() + off, 4) == 0;
        if (rates[size_t(y / tileSize) * cols + x / tileSize] == D3D12_SHADING_RATE_1X1) Check(same, "Center 1x1 pixel differed");
        else if (!same) (x < eyeW ? ++leftDiff : ++rightDiff);
        if (outAsymmetry && x < eyeW && same != (std::memcmp(baseline.data() + (size_t(y) * w + x + eyeW) * 4, actual.data() + (size_t(y) * w + x + eyeW) * 4, 4) == 0)) ++asymmetry;
    }
    Check(leftDiff > 0 && rightDiff > 0, "Peripheral coarse tiles did not differ in both halves");
    if (outAsymmetry) *outAsymmetry = asymmetry;
    return {leftDiff, rightDiff};
}

int main() try {
#if !defined(VRS_HAVE_VI_SHADERS)
    std::cout << "SKIP VRS_HAVE_VI_SHADERS not defined (dxc unavailable)\n";
    return 77;
#else
    if (!InitDebugLayerOrSkip()) return 77;
    auto sel = FindTier2Device(true);
    if (!sel.device) { std::cout << "SKIP no hardware adapter supports VRS Tier 2 + ViewInstancing + SM 6.1\n"; return 77; }
    auto& device = sel.device;
    const auto& options = sel.options6;

    D3D12_COMMAND_QUEUE_DESC qd{}; ComPtr<ID3D12CommandQueue> queue; Hr(device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12Fence> fence; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)));
    UniqueHandle fenceEvent(CreateEventW(nullptr, FALSE, FALSE, nullptr)); Check(fenceEvent.handle != nullptr, "CreateEventW failed");
    uint64_t serial = 0;
    auto wait = [&] {
        Hr(queue->Signal(fence.Get(), ++serial)); Hr(fence->SetEventOnCompletion(serial, fenceEvent.handle));
        Check(WaitForSingleObject(fenceEvent.handle, 10000) == WAIT_OBJECT_0, "GPU completion timeout");
    };

    ComPtr<ID3D12CommandAllocator> allocator; Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator)));
    ComPtr<ID3D12GraphicsCommandList> list; Hr(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator.Get(), nullptr, IID_PPV_ARGS(&list)));
    ComPtr<ID3D12GraphicsCommandList5> list5; Hr(list.As(&list5));
    Check(list5.Get() == static_cast<ID3D12GraphicsCommandList*>(list.Get()), "list5 identity mismatch");
    Check(*reinterpret_cast<void***>(list5.Get()) == *reinterpret_cast<void***>(list.Get()), "list5 vtable mismatch");
    Hr(list->Close());

    g_testListPtr = list.Get();
    void** vtable = *reinterpret_cast<void***>(list.Get());
    InitHookTable(vtable);
    {
        DWORD oldProt{}, ignored{};
        Check(VirtualProtect(vtable, 81 * sizeof(void*), PAGE_EXECUTE_READWRITE, &oldProt) != FALSE, "VirtualProtect RW failed");
        InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable + 77), reinterpret_cast<void*>(&ProbeShadingRate));
        InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable + 78), reinterpret_cast<void*>(&ProbeShadingImage));
        cvr::single_pass::Install(vtable, 78);
        VirtualProtect(vtable, 81 * sizeof(void*), oldProt, &ignored);
    }
    cvr::single_pass::Activate();
    struct VtableRestorer {
        void** vt{}; bool active = true;
        void Restore() {
            if (!active || !vt) return; active = false; DWORD oldProt{}, ignored{};
            if (VirtualProtect(vt, 81 * sizeof(void*), PAGE_EXECUTE_READWRITE, &oldProt)) {
                for (size_t i = 9; i <= 78; ++i) vt[i] = g_rawVtable[i];
                VirtualProtect(vt, 81 * sizeof(void*), oldProt, &ignored);
            }
        }
        ~VtableRestorer() { Restore(); }
    } vtableGuard{vtable};

    const auto shaders = CompileTestShaders();
    auto createRoot = [&](UINT n) {
        std::vector<D3D12_DESCRIPTOR_RANGE> r(n); std::vector<D3D12_ROOT_PARAMETER> p(n);
        for (UINT i = 0; i < n; ++i) { r[i] = {D3D12_DESCRIPTOR_RANGE_TYPE_CBV, 1, 0, i, D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND};
            p[i] = {D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE, {.DescriptorTable = {1, &r[i]}}, D3D12_SHADER_VISIBILITY_ALL}; }
        D3D12_ROOT_SIGNATURE_DESC d{n, p.data()}; ComPtr<ID3DBlob> b, e;
        Hr(D3D12SerializeRootSignature(&d, D3D_ROOT_SIGNATURE_VERSION_1, &b, &e));
        ComPtr<ID3D12RootSignature> root; Hr(device->CreateRootSignature(0, b->GetBufferPointer(), b->GetBufferSize(), IID_PPV_ARGS(&root)));
        return root;
    };
    ComPtr<ID3D12RootSignature> engineRoot = createRoot(36), twinRoot = createRoot(38);
    g_engineRootPtr = engineRoot.Get(); g_twinRootPtr = twinRoot.Get();

    const auto opaquePd = Make3MrtPsoDesc(engineRoot.Get(), {shaders.vs->GetBufferPointer(), shaders.vs->GetBufferSize()},
                                          {shaders.ps->GetBufferPointer(), shaders.ps->GetBufferSize()}, false);
    const auto blendedPd = Make3MrtPsoDesc(engineRoot.Get(), opaquePd.VS, opaquePd.PS, true);
    ComPtr<ID3D12PipelineState> engineOpaquePso, blendedPso, twinOpaquePso;
    Hr(device->CreateGraphicsPipelineState(&opaquePd, IID_PPV_ARGS(&engineOpaquePso))); cvr::vrs::GraphicsCreated(engineOpaquePso.Get(), opaquePd);
    Hr(device->CreateGraphicsPipelineState(&blendedPd, IID_PPV_ARGS(&blendedPso))); cvr::vrs::GraphicsCreated(blendedPso.Get(), blendedPd);
    auto viPd = opaquePd; viPd.pRootSignature = twinRoot.Get(); viPd.VS = {g_vi_vs, sizeof(g_vi_vs)}; viPd.PS = {g_vi_ps, sizeof(g_vi_ps)};
    ComPtr<ID3D12Device2> device2; Hr(device.As(&device2));
    Hr(cvr::stereo::CreateViewInstancedPipeline(device2.Get(), viPd, {{{{0, 0}, {1, 0}}}}, &twinOpaquePso));
    g_engineOpaquePsoPtr = engineOpaquePso.Get(); g_twinOpaquePsoPtr = twinOpaquePso.Get();

    auto cbResource = MakeBuffer(device.Get(), 256, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
    { void* p{}; Hr(cbResource->Map(0, nullptr, &p)); std::memset(p, 0, 256); cbResource->Unmap(0, nullptr); }
    ComPtr<ID3D12DescriptorHeap> cbvHeap;
    {
        D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 1, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, 0};
        Hr(device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&cbvHeap)));
        D3D12_CONSTANT_BUFFER_VIEW_DESC cbv{cbResource->GetGPUVirtualAddress(), 256};
        device->CreateConstantBufferView(&cbv, cbvHeap->GetCPUDescriptorHandleForHeapStart());
    }
    const auto cbvGpu = cbvHeap->GetGPUDescriptorHandleForHeapStart();

    auto ibResource = MakeBuffer(device.Get(), 256, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
    { void* p{}; Hr(ibResource->Map(0, nullptr, &p)); std::memset(p, 0, 256); const uint16_t idx[3]{0, 1, 2}; std::memcpy(p, idx, sizeof(idx)); ibResource->Unmap(0, nullptr); }
    const D3D12_INDEX_BUFFER_VIEW ibv{ibResource->GetGPUVirtualAddress(), 3 * sizeof(uint16_t), DXGI_FORMAT_R16_UINT};

    constexpr UINT stereoWidth = 512, stereoHeight = 192, eyeWidth = 256;
    auto stereoTarget = CreateTargetSet(device.Get(), stereoWidth, stereoHeight), scopeTarget = CreateTargetSet(device.Get(), eyeWidth, stereoHeight);
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT sFp{}; UINT64 readbackBytes = 0; const auto sColorDesc = stereoTarget.colors[0]->GetDesc();
    device->GetCopyableFootprints(&sColorDesc, 0, 1, 0, &sFp, nullptr, nullptr, &readbackBytes);
    auto stereoReadback = MakeBuffer(device.Get(), readbackBytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);

    const D3D12_VIEWPORT vpLeft{0.f, 0.f, float(eyeWidth), float(stereoHeight), 0.f, 1.f}, vpRight{float(eyeWidth), 0.f, float(eyeWidth), float(stereoHeight), 0.f, 1.f};
    const D3D12_RECT scLeft{0, 0, LONG(eyeWidth), LONG(stereoHeight)}, scRight{LONG(eyeWidth), 0, LONG(stereoWidth), LONG(stereoHeight)};

    auto IssueReset = [&](ID3D12PipelineState* pso) {
        Hr(allocator->Reset()); Hr(g_hookEntry.reset_original(list.Get(), allocator.Get(), pso));
        cvr::gpu::ResetCommandResources(list.Get()); cvr::single_pass::Reset(list.Get());
        cvr::vrs::Reset(list.Get(), g_hookEntry.shading_rate_original != nullptr, pso); g_hwState = {pso};
    };
    auto IssueSetPso = [&](ID3D12PipelineState* pso) {
        cvr::vrs::Pipeline(list.Get(), pso); if (!cvr::single_pass::PipelineSet(list.Get(), pso, pso)) g_hookEntry.setpso_original(list.Get(), pso);
    };
    auto IssueViewports = [&](UINT n, const D3D12_VIEWPORT* vp) {
        if (!cvr::single_pass::Viewports(list.Get(), n, vp)) g_hookEntry.viewports_original(list.Get(), n, vp); cvr::vrs::Viewports(list.Get(), n, vp);
    };
    auto IssueScissors = [&](UINT n, const D3D12_RECT* sc) {
        if (!cvr::single_pass::Scissors(list.Get(), n, sc)) g_hookEntry.scissor_original(list.Get(), n, sc);
    };
    auto IssueTargets = [&](const TargetSet& ts, UINT w, UINT h, bool blocked) {
        list->OMSetRenderTargets(3, &ts.rtv, TRUE, &ts.dsv); cvr::vrs::Targets(list.Get(), 3, true, w, h, blocked);
    };
    auto ClearTargetSet = [&](const TargetSet& ts) {
        const float cc[4]{0.f, 0.f, 0.f, 1.f};
        for (UINT i = 0; i < 3; ++i) list->ClearRenderTargetView({ts.rtv.ptr + SIZE_T(i) * ts.rtvStride}, cc, 0, nullptr);
        list->ClearDepthStencilView(ts.dsv, D3D12_CLEAR_FLAG_DEPTH, 1.f, 0, 0, nullptr);
    };
    auto IssueClose = [&] { cvr::single_pass::Flush(list.Get()); cvr::vrs::Restore(list.Get()); Hr(list->Close()); };
    auto IssueDraw = [&](uint32_t node, int eye, bool indexed = false) {
        cvr::vrs::PrepareDraw(list.Get(), node, eye, g_hookEntry.shading_rate_original ? vrs_set_shading_rate : nullptr,
                              g_hookEntry.shading_image_original ? vrs_set_shading_image : nullptr);
        const uint32_t args[5]{3, 1, 0, 0, 0};
        if (!cvr::single_pass::Draw(list.Get(), node, indexed ? 2u : 1u, args)) {
            if (indexed) g_hookEntry.drawidx_original(list.Get(), 3, 1, 0, 0, 0); else g_hookEntry.draw_original(list.Get(), 3, 1, 0, 0);
        }
    };
    auto beginStereoRecording = [&](ID3D12PipelineState* pso, const D3D12_VIEWPORT& vp, const D3D12_RECT& sc, bool clear = true) {
        g_events.clear(); IssueReset(nullptr); ID3D12DescriptorHeap* heaps[]{cbvHeap.Get()}; list->SetDescriptorHeaps(1, heaps);
        list->SetGraphicsRootSignature(engineRoot.Get()); list->SetGraphicsRootDescriptorTable(0, cbvGpu); list->SetGraphicsRootDescriptorTable(18, cbvGpu);
        list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST); IssueSetPso(pso); IssueViewports(1, &vp); IssueScissors(1, &sc);
        IssueTargets(stereoTarget, stereoWidth, stereoHeight, false); if (clear) ClearTargetSet(stereoTarget);
        list5->RSSetShadingRate(D3D12_SHADING_RATE_1X1, nullptr); list5->RSSetShadingRateImage(nullptr);
    };
    auto submitOnly = [&] {
        IssueClose(); ID3D12CommandList* lists[]{list.Get()}; auto res = cvr::gpu::PrepareCommandResources(1, lists);
        queue->ExecuteCommandLists(1, lists); cvr::gpu::SubmitCommandResources(queue.Get(), std::move(res)); wait();
    };
    auto submitAndReadStereo = [&] {
        Barrier(list.Get(), stereoTarget.colors[0].Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION from{}, to{}; from.pResource = stereoTarget.colors[0].Get(); to.pResource = stereoReadback.Get();
        to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; to.PlacedFootprint = sFp;
        list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
        Barrier(list.Get(), stereoTarget.colors[0].Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET);
        submitOnly();
        std::vector<uint8_t> pixels(size_t(stereoWidth) * stereoHeight * 4); void* data{}; Hr(stereoReadback->Map(0, nullptr, &data));
        for (UINT y = 0; y < stereoHeight; ++y)
            std::memcpy(pixels.data() + size_t(y) * stereoWidth * 4, static_cast<const uint8_t*>(data) + sFp.Offset + size_t(y) * sFp.Footprint.RowPitch, size_t(stereoWidth) * 4);
        stereoReadback->Unmap(0, nullptr); return pixels;
    };
    auto activateSettingsAndWaitMap = [&](const cvr::vrs::Settings& s) {
        const uint32_t savedSp = CyberpunkVR_SinglePass.exchange(0); cvr::vrs::SetSettings(s);
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(2500);
        char expectedApplied[32]{}; std::snprintf(expectedApplied, sizeof(expectedApplied), "\"applied\":%d,", s.mode);
        bool ready = false;
        while (std::chrono::steady_clock::now() < deadline) {
            cvr::vrs::Tick(device.Get(), queue.Get());
            beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft, false);
            cvr::vrs::PrepareDraw(list.Get(), cvr::vrs::kRenderElementsNode, 0, vrs_set_shading_rate, vrs_set_shading_image);
            cvr::vrs::Restore(list.Get()); submitOnly();
            cvr::vrs::Tick(device.Get(), queue.Get()); wait(); cvr::vrs::Tick(device.Get(), queue.Get());
            if (cvr::vrs::GetStatus().readyMaps >= 1 && std::strstr(CyberpunkVR_FixedVrsReport, expectedApplied)) { ready = true; break; }
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
        CyberpunkVR_SinglePass.store(savedSp); Check(ready, "Timed out waiting for 512x192 stereo VRS map");
    };

    // Scenario 1: Baseline native 1x1 reference
    CyberpunkVR_SinglePass = 0; CyberpunkVR_SinglePassLazy = 1; cvr::vrs::SetSettings({});
    beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft);
    IssueDraw(cvr::vrs::kRenderElementsNode, 0); IssueViewports(1, &vpRight); IssueScissors(1, &scRight); IssueDraw(cvr::vrs::kRenderElementsNode, 1);
    const auto baseline = submitAndReadStereo();
    Check(CollectDrawEvents().size() == 2, "Scenario 1 baseline should emit 2 hardware draws");
    std::cout << "PASS Scenario 1: Baseline native 1x1 reference rendered\n";

    // Scenario 2: View-instanced pair collapse with VRS 2x2 (eye=0 on both halves) + direct RSSetShadingRate
    const cvr::vrs::Settings vrs2x2{1, 40.f, 40.f, 0.f, 0.f};
    activateSettingsAndWaitMap(vrs2x2); CyberpunkVR_SinglePass = 1; CyberpunkVR_SinglePassLazy = 1;
    const uint64_t kept2 = CyberpunkVR_SinglePassCounts[0].load(), inst2 = CyberpunkVR_SinglePassCounts[1].load();
    const auto vrsBefore = cvr::vrs::GetStatus();
    beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft);
    IssueDraw(cvr::vrs::kRenderElementsNode, 0); IssueViewports(1, &vpRight); IssueScissors(1, &scRight);
    IssueDraw(cvr::vrs::kRenderElementsNode, 0); // eye=0 on vpRight: FixedVrs infers eye 1 from vpX
    const auto viPixels = submitAndReadStereo();
    const auto vrsAfter = cvr::vrs::GetStatus();
    Check(vrsAfter.draws[0] == vrsBefore.draws[0] + 1 && vrsAfter.draws[1] == vrsBefore.draws[1] + 1 &&
          vrsAfter.eligibleDraws[0] == vrsBefore.eligibleDraws[0] + 1 && vrsAfter.eligibleDraws[1] == vrsBefore.eligibleDraws[1] + 1, "Scenario 2 eye=0 inference failed");
    Check(CyberpunkVR_SinglePassCounts[0].load() == kept2 + 1 && CyberpunkVR_SinglePassCounts[1].load() == inst2 + 1 &&
          CyberpunkVR_SinglePassMismatch[9].load() == 0 && CyberpunkVR_SinglePassMismatch[10].load() == 0, "Scenario 2 SinglePass counters failed");
    {
        const auto draws = CollectDrawEvents();
        Check(draws.size() == 1 && draws[0].pso == twinOpaquePso.Get() && draws[0].vpCount == 2 &&
              draws[0].shadingImage != nullptr && draws[0].combiners[1] == D3D12_SHADING_RATE_COMBINER_OVERRIDE, "Scenario 2 hw draw failed");
    }
    const auto [l2, r2] = VerifyStereoPixels(baseline, viPixels, vrs2x2, vrs2x2, stereoWidth, stereoHeight, options.ShadingRateImageTileSize, options.AdditionalShadingRatesSupported != FALSE);
    std::cout << "PASS Scenario 2: View-instanced pair collapse with VRS 2x2 (1 hw draw, center exact, peripheral diff L=" << l2 << " R=" << r2 << ")\n";
    {
        const D3D12_SHADING_RATE_COMBINER nc[2]{D3D12_SHADING_RATE_COMBINER_MAX, D3D12_SHADING_RATE_COMBINER_MIN};
        beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft); list5->RSSetShadingRate(D3D12_SHADING_RATE_2X1, nc);
        Check(g_hwState.shadingRate == D3D12_SHADING_RATE_2X1 && g_hwState.combiners[0] == nc[0] && g_hwState.combiners[1] == nc[1], "Direct RSSetShadingRate unheld failed");
        IssueDraw(cvr::vrs::kRenderElementsNode, 0); ID3D12Resource* const activeMap = g_hwState.shadingImage; Check(activeMap != nullptr, "VRS map missing");
        list5->RSSetShadingRate(D3D12_SHADING_RATE_2X1, nc); list5->RSSetShadingRateImage(nullptr); Check(g_hwState.shadingImage == activeMap, "Direct RSSetShadingRate/Image clobbered held VRS image");
        submitOnly(); const auto draws = CollectDrawEvents();
        Check(draws.size() == 1 && draws[0].shadingImage == activeMap && draws[0].combiners[1] == D3D12_SHADING_RATE_COMBINER_OVERRIDE, "Flushed draw lost VRS");
        Check(g_hwState.shadingImage == nullptr && g_hwState.shadingRate == D3D12_SHADING_RATE_2X1 && g_hwState.combiners[0] == nc[0] && g_hwState.combiners[1] == nc[1], "Native VRS not restored");
    }

    // Scenario 3A/3B/3C: Held-left-draw flush ordering before incompatible draws
    auto runFlushCase = [&](const char* label, ID3D12PipelineState* pso2, auto&& issueSecond) {
        beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft); IssueDraw(cvr::vrs::kRenderElementsNode, 0); issueSecond(); submitOnly();
        size_t i1 = SIZE_MAX, i2 = SIZE_MAX; std::vector<RecordedEvent> draws;
        for (size_t i = 0; i < g_events.size(); ++i)
            if (g_events[i].kind == RecordedEvent::Kind::Draw || g_events[i].kind == RecordedEvent::Kind::DrawIndexed) {
                if (i1 == SIZE_MAX) i1 = i; else if (i2 == SIZE_MAX) i2 = i; draws.push_back(g_events[i]);
            }
        Check(draws.size() == 2 && draws[0].pso == engineOpaquePso.Get() && draws[0].shadingImage != nullptr && draws[0].combiners[1] == D3D12_SHADING_RATE_COMBINER_OVERRIDE &&
              draws[1].pso == pso2 && draws[1].shadingImage == nullptr && draws[1].shadingRate == D3D12_SHADING_RATE_1X1 &&
              draws[1].combiners[0] == D3D12_SHADING_RATE_COMBINER_PASSTHROUGH && draws[1].combiners[1] == D3D12_SHADING_RATE_COMBINER_PASSTHROUGH, label);
        bool sawImg = false, sawRate = false;
        for (size_t i = i1 + 1; i < i2; ++i) {
            if (g_events[i].kind == RecordedEvent::Kind::ShadingImage && !g_events[i].shadingImage) sawImg = true;
            if (g_events[i].kind == RecordedEvent::Kind::ShadingRate && g_events[i].shadingRate == D3D12_SHADING_RATE_1X1 &&
                g_events[i].combiners[0] == D3D12_SHADING_RATE_COMBINER_PASSTHROUGH && g_events[i].combiners[1] == D3D12_SHADING_RATE_COMBINER_PASSTHROUGH) sawRate = true;
        }
        Check(sawImg && sawRate, "VRS restore missing between Draw #1 and Draw #2");
        std::cout << "PASS " << label << '\n';
    };
    runFlushCase("Scenario 3A: Held left draw flushed with VRS before blended PSO draw", blendedPso.Get(), [&] { IssueSetPso(blendedPso.Get()); IssueDraw(cvr::vrs::kRenderElementsNode, 0); });
    runFlushCase("Scenario 3B: Held left draw flushed with VRS before non-RenderElements node draw", engineOpaquePso.Get(), [&] { IssueDraw(0x123456u, 0); });
    runFlushCase("Scenario 3C: Held left draw flushed with VRS before blocked magnifying scope draw", engineOpaquePso.Get(), [&] {
        IssueTargets(scopeTarget, eyeWidth, stereoHeight, true); ClearTargetSet(scopeTarget); IssueDraw(cvr::vrs::kRenderElementsNode, 0);
    });

    // Scenario 4: Replay mode (SinglePass=2) byte-identical parity
    CyberpunkVR_SinglePass = 2;
    beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft);
    IssueDraw(cvr::vrs::kRenderElementsNode, 0); IssueViewports(1, &vpRight); IssueScissors(1, &scRight); IssueDraw(cvr::vrs::kRenderElementsNode, 1);
    const auto replayPixels = submitAndReadStereo();
    { const auto draws = CollectDrawEvents(); Check(draws.size() == 2, "Scenario 4 expected 2 draws");
      for (const auto& d : draws) Check(d.pso == engineOpaquePso.Get() && d.shadingImage && d.combiners[1] == D3D12_SHADING_RATE_COMBINER_OVERRIDE, "Scenario 4 VRS check failed"); }
    Check(replayPixels == viPixels, "Scenario 4 replay pixels differed from SinglePass=1");
    std::cout << "PASS Scenario 4: Replay mode (SinglePass=2) byte-identical parity to view-instanced render\n";

    // Scenario 5: Indexed DrawIndexedInstanced + 4x4 VRS (mode == 2)
    const cvr::vrs::Settings vrs4x4{2, 40.f, 40.f, 0.f, 0.f};
    activateSettingsAndWaitMap(vrs4x4); CyberpunkVR_SinglePass = 1; CyberpunkVR_SinglePassLazy = 1;
    const uint64_t idxK = CyberpunkVR_SinglePassCounts[0].load(), idxI = CyberpunkVR_SinglePassCounts[1].load();
    beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft); list->IASetIndexBuffer(&ibv);
    IssueDraw(cvr::vrs::kRenderElementsNode, 0, true); IssueViewports(1, &vpRight); IssueScissors(1, &scRight); IssueDraw(cvr::vrs::kRenderElementsNode, 0, true);
    const auto vi4x4Pixels = submitAndReadStereo();
    Check(CyberpunkVR_SinglePassCounts[0].load() == idxK + 1 && CyberpunkVR_SinglePassCounts[1].load() == idxI + 1, "Scenario 5 counts failed");
    for (size_t i = 0; i < 12; ++i) Check(CyberpunkVR_SinglePassMismatch[i].load() == 0, "Scenario 5 mismatch non-zero");
    { const auto draws = CollectDrawEvents();
      Check(draws.size() == 1 && draws[0].kind == RecordedEvent::Kind::DrawIndexed && draws[0].pso == twinOpaquePso.Get() &&
            draws[0].vpCount == 2 && draws[0].shadingImage && draws[0].combiners[1] == D3D12_SHADING_RATE_COMBINER_OVERRIDE, "Scenario 5 DrawIndexed check failed"); }
    const auto [l4, r4] = VerifyStereoPixels(baseline, vi4x4Pixels, vrs4x4, vrs4x4, stereoWidth, stereoHeight, options.ShadingRateImageTileSize, options.AdditionalShadingRatesSupported != FALSE);
    std::cout << "PASS Scenario 5: Indexed DrawIndexedInstanced pair collapse with VRS 4x4 (1 hw draw, center exact, peripheral diff L=" << l4 << " R=" << r4 << ")\n";

    // Scenario 6: Eye-tracked mid-recording slot promotion & vergence pinning
    const cvr::vrs::Settings eyeTracked4x4{2, 40.f, 40.f, 0.f, 0.f, true}; cvr::vrs::SetSettings(eyeTracked4x4);
    const cvr::vrs::DynamicEyeOffsets gazeA{true, {20.f, -20.f}, {-10.f, 10.f}}, gazeB{true, {-20.f, 20.f}, {10.f, -10.f}};
    cvr::vrs::Tick(device.Get(), queue.Get(), &gazeA); wait(); cvr::vrs::Tick(device.Get(), queue.Get(), &gazeA);
    const uint64_t s6k = CyberpunkVR_SinglePassCounts[0].load(), s6i = CyberpunkVR_SinglePassCounts[1].load();
    beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft); IssueDraw(cvr::vrs::kRenderElementsNode, 0);
    Check(CyberpunkVR_SinglePassCounts[0].load() == s6k + 1 && CollectDrawEvents().empty(), "Scenario 6 left draw not held");
    ID3D12Resource* const imgA = g_hwState.shadingImage; Check(imgA != nullptr, "Scenario 6 imgA not bound");
    cvr::vrs::Tick(device.Get(), queue.Get(), &gazeB); wait(); cvr::vrs::Tick(device.Get(), queue.Get(), &gazeB);
    IssueViewports(1, &vpRight); IssueScissors(1, &scRight); IssueDraw(cvr::vrs::kRenderElementsNode, 0); submitOnly();
    Check(CyberpunkVR_SinglePassMismatch[9].load() == 0 && CyberpunkVR_SinglePassMismatch[10].load() == 0 &&
          CyberpunkVR_SinglePassCounts[1].load() == s6i + 1, "Scenario 6 pinning mismatch");
    { const auto draws = CollectDrawEvents();
      Check(draws.size() == 1 && draws[0].pso == twinOpaquePso.Get() && draws[0].vpCount == 2 &&
            draws[0].shadingImage == imgA && draws[0].combiners[1] == D3D12_SHADING_RATE_COMBINER_OVERRIDE, "Scenario 6 draw not pinned to imgA"); }
    const uint64_t s6n = CyberpunkVR_SinglePassCounts[1].load();
    beginStereoRecording(engineOpaquePso.Get(), vpLeft, scLeft); IssueDraw(cvr::vrs::kRenderElementsNode, 0);
    IssueViewports(1, &vpRight); IssueScissors(1, &scRight); IssueDraw(cvr::vrs::kRenderElementsNode, 0);
    const auto gazeBPixels = submitAndReadStereo();
    Check(CyberpunkVR_SinglePassCounts[1].load() == s6n + 1 && CyberpunkVR_SinglePassMismatch[10].load() == 0, "Scenario 6 next recording failed");
    ID3D12Resource* imgB = nullptr;
    { const auto draws = CollectDrawEvents();
      Check(draws.size() == 1 && draws[0].pso == twinOpaquePso.Get() && draws[0].vpCount == 2 && draws[0].combiners[1] == D3D12_SHADING_RATE_COMBINER_OVERRIDE, "Scenario 6 next draw failed");
      imgB = draws[0].shadingImage; Check(imgB && imgB != imgA, "Scenario 6 did not promote imgB != imgA"); }
    auto lB = eyeTracked4x4, rB = eyeTracked4x4; lB.offsetX = gazeB.offsetX[0]; lB.offsetY = gazeB.offsetY[0]; rB.offsetX = gazeB.offsetX[1]; rB.offsetY = gazeB.offsetY[1];
    unsigned asym = 0; VerifyStereoPixels(baseline, gazeBPixels, lB, rB, stereoWidth, stereoHeight, options.ShadingRateImageTileSize, options.AdditionalShadingRatesSupported != FALSE, &asym);
    Check(asym > 0, "Scenario 6 gazeB Left and Right halves did not diverge");
    std::cout << "PASS Scenario 6: Eye-tracked mid-recording slot promotion & vergence pinning (pinned imgA=" << imgA << ", promoted imgB=" << imgB << ", asymmetry=" << asym << ")\n";

    // Scenario 7: Clean shutdown & zero D3D12 validation warnings
    CyberpunkVR_SinglePass = 0; cvr::gpu::CollectCommandResources(); vtableGuard.Restore(); cvr::vrs::Shutdown();
    VerifyInfoQueueClean(device.Get());
    std::cout << "PASS Scenario 7: Clean shutdown and 0 D3D12 validation warnings/errors\n";
    return 0;
#endif
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
