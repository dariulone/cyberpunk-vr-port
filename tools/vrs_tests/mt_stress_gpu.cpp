#include "test_common.hpp"
#include "Render/CommandResources.hpp"
#include "Render/FixedVrs.hpp"
#include <atomic>
#include <chrono>
#include <thread>
using namespace vrs_test;

namespace {
constexpr UINT kWorkers = 6, kMinIters = 60, kSingleW = 256, kSingleH = 192, kStereoW = 512, kStereoH = 192, kEyeW = 256;
constexpr uint32_t kOtherNode = 0x999999u;
void STDMETHODCALLTYPE Rate(ID3D12GraphicsCommandList5* l, D3D12_SHADING_RATE r, const D3D12_SHADING_RATE_COMBINER* c) { l->RSSetShadingRate(r, c); }
void STDMETHODCALLTYPE Image(ID3D12GraphicsCommandList5* l, ID3D12Resource* r) { l->RSSetShadingRateImage(r); }

struct WorkerCtx {
    TargetSet single, stereo;
    std::array<ComPtr<ID3D12CommandAllocator>, 2> allocs;
    std::array<ComPtr<ID3D12GraphicsCommandList>, 2> lists;
    std::array<ComPtr<ID3D12GraphicsCommandList5>, 2> lists5;
    std::array<uint64_t, 2> slotFence{};
    ComPtr<ID3D12Fence> fence; uint64_t fenceVal = 0; UniqueHandle event;
};
WorkerCtx CreateWorkerCtx(ID3D12Device* dev) {
    WorkerCtx ctx{CreateTargetSet(dev, kSingleW, kSingleH), CreateTargetSet(dev, kStereoW, kStereoH)};
    for (UINT s = 0; s < 2; ++s) {
        Hr(dev->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&ctx.allocs[s])));
        Hr(dev->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, ctx.allocs[s].Get(), nullptr, IID_PPV_ARGS(&ctx.lists[s])));
        Hr(ctx.lists[s].As(&ctx.lists5[s])); Hr(ctx.lists[s]->Close());
    }
    Hr(dev->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&ctx.fence)));
    ctx.event = UniqueHandle(CreateEventW(nullptr, FALSE, FALSE, nullptr)); Check(ctx.event.handle != nullptr, "CreateEventW failed");
    return ctx;
}
void WaitSlot(WorkerCtx& ctx, UINT s) {
    const uint64_t t = ctx.slotFence[s]; if (!t || ctx.fence->GetCompletedValue() >= t) return;
    Hr(ctx.fence->SetEventOnCompletion(t, ctx.event.handle));
    Check(WaitForSingleObject(ctx.event.handle, 10000) == WAIT_OBJECT_0, "Worker GPU completion timeout");
}
struct GazeStep { float x0, y0, x1, y1; };
constexpr std::array<GazeStep, 6> kSaccades{{{-22.f, 18.f, 22.f, -18.f}, {20.f, -16.f, -20.f, 16.f}, {-18.f, -20.f, 18.f, 20.f}, {24.f, 0.f, -24.f, 0.f}, {0.f, 22.f, 0.f, -22.f}, {0.f, 0.f, 0.f, 0.f}}};
void UpdateGazePattern(cvr::vrs::DynamicEyeOffsets& g, UINT step) {
    const GazeStep& b = kSaccades[(step / 5u) % kSaccades.size()]; const UINT sub = step % 5u;
    const float j = (sub < 2u) ? 0.f : static_cast<float>(sub - 2u) * 0.04f;
    g = {true, {b.x0 + j, b.x1 - j}, {b.y0 + j, b.y1 - j}};
}
} // namespace

int main() try {
    if (!InitDebugLayerOrSkip()) return 77;
    auto sel = FindTier2Device(false);
    if (!sel.device) { std::cout << "SKIP hardware has no VRS Tier 2\n"; return 77; }
    auto& device = sel.device;

    D3D12_COMMAND_QUEUE_DESC qd{D3D12_COMMAND_LIST_TYPE_DIRECT}; ComPtr<ID3D12CommandQueue> queue; Hr(device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue)));
    const auto shaders = CompileTestShaders();
    D3D12_ROOT_SIGNATURE_DESC rsDesc{}; ComPtr<ID3DBlob> rsBlob, errBlob;
    Hr(D3D12SerializeRootSignature(&rsDesc, D3D_ROOT_SIGNATURE_VERSION_1, &rsBlob, &errBlob));
    ComPtr<ID3D12RootSignature> root; Hr(device->CreateRootSignature(0, rsBlob->GetBufferPointer(), rsBlob->GetBufferSize(), IID_PPV_ARGS(&root)));

    const auto opaquePd = Make3MrtPsoDesc(root.Get(), {shaders.vs->GetBufferPointer(), shaders.vs->GetBufferSize()}, {shaders.ps->GetBufferPointer(), shaders.ps->GetBufferSize()}, false);
    const auto blendedPd = Make3MrtPsoDesc(root.Get(), opaquePd.VS, opaquePd.PS, true);
    ComPtr<ID3D12PipelineState> opaquePso, blendedPso;
    Hr(device->CreateGraphicsPipelineState(&opaquePd, IID_PPV_ARGS(&opaquePso))); cvr::vrs::GraphicsCreated(opaquePso.Get(), opaquePd);
    Hr(device->CreateGraphicsPipelineState(&blendedPd, IID_PPV_ARGS(&blendedPso))); cvr::vrs::GraphicsCreated(blendedPso.Get(), blendedPd);

    std::vector<WorkerCtx> workers; workers.reserve(kWorkers);
    for (UINT i = 0; i < kWorkers; ++i) workers.push_back(CreateWorkerCtx(device.Get()));

    const D3D12_VIEWPORT vpSingle{0.f, 0.f, float(kSingleW), float(kSingleH), 0.f, 1.f};
    const D3D12_VIEWPORT vpLeft{0.f, 0.f, float(kEyeW), float(kStereoH), 0.f, 1.f}, vpRight{float(kEyeW), 0.f, float(kEyeW), float(kStereoH), 0.f, 1.f};
    const D3D12_RECT scSingle{0, 0, LONG(kSingleW), LONG(kSingleH)}, scLeft{0, 0, LONG(kEyeW), LONG(kStereoH)}, scRight{LONG(kEyeW), 0, LONG(kStereoW), LONG(kStereoH)};

    std::mutex queueMutex, errorMutex; std::string workerError;
    std::atomic<UINT> activeWorkers{kWorkers}; std::atomic<bool> stopWorkers{false}; std::atomic<UINT64> totalIters{0};
    const auto startTime = std::chrono::steady_clock::now(), safetyDeadline = startTime + std::chrono::seconds(20);
    cvr::vrs::SetSettings({1, 50.f, 50.f, 0.f, 0.f, true});
    cvr::vrs::DynamicEyeOffsets gaze{true, {0.f, 0.f}, {0.f, 0.f}};

    std::vector<std::thread> threads; threads.reserve(kWorkers);
    for (UINT wi = 0; wi < kWorkers; ++wi) {
        threads.emplace_back([&, wi]() {
            struct Guard { std::atomic<UINT>& n; ~Guard() { n.fetch_sub(1, std::memory_order_release); } } guard{activeWorkers};
            try {
                WorkerCtx& ctx = workers[wi];
                for (UINT iter = 0; (!stopWorkers.load(std::memory_order_acquire) || iter < kMinIters) && std::chrono::steady_clock::now() < safetyDeadline; ++iter) {
                    const UINT s = iter & 1u; WaitSlot(ctx, s);
                    auto* list = ctx.lists[s].Get(); auto* list5 = ctx.lists5[s].Get();
                    cvr::gpu::ResetCommandResources(list); Hr(ctx.allocs[s]->Reset()); Hr(list->Reset(ctx.allocs[s].Get(), opaquePso.Get()));
                    cvr::vrs::Reset(list, true, opaquePso.Get()); list->SetGraphicsRootSignature(root.Get()); list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    list5->RSSetShadingRate(D3D12_SHADING_RATE_1X1, nullptr); list5->RSSetShadingRateImage(nullptr);
                    cvr::vrs::ShadingRate(list, D3D12_SHADING_RATE_1X1, nullptr); cvr::vrs::ShadingImage(list, nullptr);
                    auto bindView = [&](const D3D12_VIEWPORT& vp, const D3D12_RECT& sc) { list->RSSetViewports(1, &vp); list->RSSetScissorRects(1, &sc); cvr::vrs::Viewports(list, 1, &vp); };
                    const UINT v = (iter + wi) % 4u;
                    if (v == 0u) {
                        list->OMSetRenderTargets(3, &ctx.stereo.rtv, TRUE, &ctx.stereo.dsv); cvr::vrs::Targets(list, 3, true, kStereoW, kStereoH, false);
                        bindView(vpLeft, scLeft); cvr::vrs::PrepareDraw(list, cvr::vrs::kRenderElementsNode, 0, Rate, Image); list->DrawInstanced(3, 1, 0, 0); cvr::vrs::Restore(list);
                        bindView(vpRight, scRight); cvr::vrs::PrepareDraw(list, cvr::vrs::kRenderElementsNode, 0, Rate, Image); list->DrawInstanced(3, 1, 0, 0); cvr::vrs::Restore(list);
                    } else if (v == 1u) {
                        list->OMSetRenderTargets(3, &ctx.stereo.rtv, TRUE, &ctx.stereo.dsv); cvr::vrs::Targets(list, 3, true, kStereoW, kStereoH, false);
                        cvr::vrs::Pipeline(list, opaquePso.Get()); bindView(vpLeft, scLeft); cvr::vrs::PrepareDraw(list, cvr::vrs::kRenderElementsNode, 0, Rate, Image); list->DrawInstanced(3, 1, 0, 0);
                        list->SetPipelineState(blendedPso.Get()); cvr::vrs::Pipeline(list, blendedPso.Get()); cvr::vrs::PrepareDraw(list, cvr::vrs::kRenderElementsNode, 0, Rate, Image); list->DrawInstanced(3, 1, 0, 0);
                        list->SetPipelineState(opaquePso.Get()); cvr::vrs::Pipeline(list, opaquePso.Get()); bindView(vpRight, scRight);
                        cvr::vrs::PrepareDraw(list, cvr::vrs::kRenderElementsNode, ((iter / 4u) & 1u) ? 0 : 1, Rate, Image); list->DrawInstanced(3, 1, 0, 0); cvr::vrs::Restore(list);
                    } else if (v == 2u) {
                        list->OMSetRenderTargets(3, &ctx.single.rtv, TRUE, &ctx.single.dsv); cvr::vrs::Targets(list, 3, true, kSingleW, kSingleH, false);
                        cvr::vrs::Pipeline(list, opaquePso.Get()); bindView(vpSingle, scSingle);
                        cvr::vrs::PrepareDraw(list, cvr::vrs::kRenderElementsNode, int(((iter / 4u) + wi) & 1u), Rate, Image); list->DrawInstanced(3, 1, 0, 0); cvr::vrs::Restore(list);
                    } else {
                        list->OMSetRenderTargets(3, &ctx.stereo.rtv, TRUE, &ctx.stereo.dsv); cvr::vrs::Targets(list, 3, true, kStereoW, kStereoH, true);
                        bindView(vpLeft, scLeft); cvr::vrs::PrepareDraw(list, cvr::vrs::kRenderElementsNode, 0, Rate, Image); list->DrawInstanced(3, 1, 0, 0);
                        cvr::vrs::Targets(list, 3, true, kStereoW, kStereoH, false); cvr::vrs::PrepareDraw(list, kOtherNode, 0, Rate, Image); list->DrawInstanced(3, 1, 0, 0); cvr::vrs::Restore(list);
                    }
                    Hr(list->Close());
                    { std::lock_guard lock(queueMutex); ID3D12CommandList* cmd[]{list}; auto res = cvr::gpu::PrepareCommandResources(1, cmd);
                      queue->ExecuteCommandLists(1, cmd); cvr::gpu::SubmitCommandResources(queue.Get(), std::move(res));
                      Hr(queue->Signal(ctx.fence.Get(), ctx.slotFence[s] = ++ctx.fenceVal)); }
                    totalIters.fetch_add(1, std::memory_order_relaxed);
                    std::this_thread::sleep_until(startTime + std::chrono::milliseconds(static_cast<long long>(iter + 1) * 15));
                }
                for (UINT s = 0; s < 2; ++s) { WaitSlot(ctx, s); cvr::gpu::ResetCommandResources(ctx.lists[s].Get()); }
            } catch (const std::exception& e) { std::lock_guard lock(errorMutex); if (workerError.empty()) workerError = e.what(); }
        });
    }

    enum class Phase { InitialApply, Mode1EyeTracked, Mode2Pending, Mode2Active, Mode0Off, MidShutdown, MidShutdownDone };
    Phase phase = Phase::InitialApply; auto phaseStart = startTime;
    UINT gazeStep = 0, mode2Ticks = 0; uint64_t uploadsBeforeGaze = 0, skippedBeforeGaze = 0, drawsBase = 0;
    bool sawDisable = false, sawReenable = false, sawMode2 = false, sawRecovery = false;

    while (activeWorkers.load(std::memory_order_acquire) > 0) {
        if (phase == Phase::Mode1EyeTracked || phase == Phase::Mode2Pending || phase == Phase::Mode2Active || phase == Phase::MidShutdown)
            UpdateGazePattern(gaze, gazeStep);
        cvr::vrs::Tick(device.Get(), queue.Get(), &gaze);
        const auto now = std::chrono::steady_clock::now();
        if (now >= safetyDeadline) { stopWorkers.store(true, std::memory_order_release); break; }
        const auto st = cvr::vrs::GetStatus(); const uint64_t totalDraws = st.draws[0] + st.draws[1];
        if (phase == Phase::InitialApply) {
            if (st.appliedMode == 1 && st.readyMaps >= 1 && st.uploads > 0 && st.draws[0] > 0 && st.draws[1] > 0) {
                uploadsBeforeGaze = st.uploads; skippedBeforeGaze = st.skippedTicks; gazeStep = 0; phase = Phase::Mode1EyeTracked; phaseStart = now;
            }
        } else if (phase == Phase::Mode1EyeTracked) {
            if (gazeStep == 12u) { cvr::vrs::SetSettings({1, 50.f, 50.f, 12.f, -8.f, false}); Check(!cvr::vrs::GetAppliedSettings().eyeTracked, "SameAllocation disable failed"); sawDisable = true; }
            else if (gazeStep == 24u) { cvr::vrs::SetSettings({1, 50.f, 50.f, -10.f, 8.f, true}); Check(cvr::vrs::GetAppliedSettings().eyeTracked, "SameAllocation reenable failed"); sawReenable = true; }
            if (++gazeStep >= 32u && st.uploads > uploadsBeforeGaze && st.skippedTicks > skippedBeforeGaze) {
                cvr::vrs::SetSettings({2, 45.f, 45.f, 0.f, 0.f, true}); phase = Phase::Mode2Pending; phaseStart = now;
            }
        } else if (phase == Phase::Mode2Pending) {
            ++gazeStep;
            if (st.appliedMode == 2 && st.readyMaps >= 1) { drawsBase = totalDraws; mode2Ticks = 0; phase = Phase::Mode2Active; phaseStart = now; }
        } else if (phase == Phase::Mode2Active) {
            ++gazeStep;
            if (++mode2Ticks >= 12u && totalDraws > drawsBase) { sawMode2 = true; cvr::vrs::SetSettings({0, 45.f, 45.f, 0.f, 0.f, false}); phase = Phase::Mode0Off; phaseStart = now; }
        } else if (phase == Phase::Mode0Off) {
            if (st.appliedMode == 0 && (now - phaseStart) >= std::chrono::milliseconds(100)) {
                cvr::vrs::Shutdown(); cvr::vrs::Tick(device.Get(), queue.Get(), &gaze);
                drawsBase = cvr::vrs::GetStatus().draws[0] + cvr::vrs::GetStatus().draws[1];
                cvr::vrs::SetSettings({1, 65.f, 65.f, 0.f, 0.f, true}); phase = Phase::MidShutdown; phaseStart = now;
            }
        } else if (phase == Phase::MidShutdown) {
            ++gazeStep;
            if (st.appliedMode == 1 && st.readyMaps >= 1 && totalDraws > drawsBase) { sawRecovery = true; phase = Phase::MidShutdownDone; stopWorkers.store(true, std::memory_order_release); }
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    stopWorkers.store(true, std::memory_order_release);
    for (auto& t : threads) t.join();
    if (!workerError.empty()) throw std::runtime_error(workerError);

    ComPtr<ID3D12Fence> drainFence; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&drainFence)));
    UniqueHandle drainEvent(CreateEventW(nullptr, FALSE, FALSE, nullptr)); Check(drainEvent.handle != nullptr, "CreateEventW drain failed");
    Hr(queue->Signal(drainFence.Get(), 1));
    if (drainFence->GetCompletedValue() < 1) { Hr(drainFence->SetEventOnCompletion(1, drainEvent.handle)); Check(WaitForSingleObject(drainEvent.handle, 10000) == WAIT_OBJECT_0, "Queue drain timeout"); }

    cvr::gpu::CollectCommandResources(); cvr::vrs::Shutdown();
    const auto st = cvr::vrs::GetStatus();
    Check(phase == Phase::MidShutdownDone && sawDisable && sawReenable && sawMode2 && sawRecovery, "Phase progression check failed");
    Check(!st.failed && !st.stateCapHit && (st.sceneDraws[0] + st.sceneDraws[1] > 0) && (st.draws[0] + st.draws[1] > 0), "Final status check failed");
    Check(st.uploads > uploadsBeforeGaze && st.skippedTicks > skippedBeforeGaze && st.allocations > 0 && st.allocations <= 16ULL * 3ULL, "Gaze upload/skip/alloc check failed");
    VerifyInfoQueueClean(device.Get());

    const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - startTime).count();
    std::cout << "PASS MT GPU VRS stress (" << kWorkers << " threads, " << totalIters.load() << " total iters in " << ms << " ms); uploads=" << st.uploads
              << ", allocations=" << st.allocations << ", skippedTicks=" << st.skippedTicks
              << ", sceneDraws=[" << st.sceneDraws[0] << "," << st.sceneDraws[1] << "], eligibleDraws=[" << st.eligibleDraws[0] << "," << st.eligibleDraws[1]
              << "], draws=[" << st.draws[0] << "," << st.draws[1] << "]\n";
    return 0;
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
