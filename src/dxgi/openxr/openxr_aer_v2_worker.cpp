// openxr_aer_v2_worker.cpp - AER V2 (NvOF frame-generation) worker thread.
// Split verbatim from openxr_manager.cpp; these are OpenXRManager methods. Shared
// module state/helpers come from openxr_internal.h (inline, single instance).
#include "openxr_manager.h"
#include "openxr_internal.h"
#include "openxr_math.h"
#include "shared_slots.h"
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <cmath>
#include <vector>
#include <thread>
#include <memory>
#include <utility>
#include <chrono>
#include <algorithm>

void OpenXRManager::ReleaseAERV2JobRefs(AERV2Job& job) {
    for (int eye = 0; eye < 2; ++eye) {
        if (job.flowPrevSource[eye]) { job.flowPrevSource[eye]->Release(); job.flowPrevSource[eye] = nullptr; }
        if (job.flowCurrSource[eye]) { job.flowCurrSource[eye]->Release(); job.flowCurrSource[eye] = nullptr; }
        if (job.flowPrev[eye])       { job.flowPrev[eye]->Release();       job.flowPrev[eye] = nullptr; }
        if (job.flowCurr[eye])       { job.flowCurr[eye]->Release();       job.flowCurr[eye] = nullptr; }
        if (job.prevDepth[eye])      { job.prevDepth[eye]->Release();      job.prevDepth[eye] = nullptr; }
        if (job.currDepth[eye])      { job.currDepth[eye]->Release();      job.currDepth[eye] = nullptr; }
    }
    if (job.engineMv)    { job.engineMv->Release();    job.engineMv = nullptr; }
    if (job.engineDepth) { job.engineDepth->Release(); job.engineDepth = nullptr; }
}

void OpenXRManager::StartAERV2WorkerIfNeeded() {
    if (m_aerV2WorkerThread.joinable()) {
        return;
    }
    m_aerV2WorkerShutdown.store(false, std::memory_order_release);
    m_aerV2WorkerThread = std::thread(&OpenXRManager::AERV2WorkerThreadMain, this);
}

void OpenXRManager::StopAERV2Worker() {
    if (!m_aerV2WorkerThread.joinable()) {
        // Still might have a pending job sitting from before the worker started.
        std::unique_ptr<OpenXRManager::AERV2Job> stale;
        {
            std::lock_guard<std::mutex> lock(m_aerV2JobMutex);
            stale = std::move(m_aerV2PendingJob);
        }
        if (stale) {
            ReleaseAERV2JobRefs(*stale);
        }
        return;
    }
    {
        std::lock_guard<std::mutex> lock(m_aerV2JobMutex);
        m_aerV2WorkerShutdown.store(true, std::memory_order_release);
    }
    m_aerV2JobCv.notify_all();
    m_aerV2WorkerThread.join();
    // Drain any leftover queued job (worker exited without picking it up).
    std::unique_ptr<OpenXRManager::AERV2Job> stale;
    {
        std::lock_guard<std::mutex> lock(m_aerV2JobMutex);
        stale = std::move(m_aerV2PendingJob);
    }
    if (stale) {
        ReleaseAERV2JobRefs(*stale);
    }
    m_aerV2BusyPairId.store(0, std::memory_order_relaxed);
}

void OpenXRManager::ProcessAERV2Job(std::unique_ptr<AERV2Job> job) {
    if (!job) {
        return;
    }
    m_aerV2BusyPairId.store(job->pairId, std::memory_order_release);

    // Phase 2b/2c/2d: run motion-vector forward warp on the OLDER eye of
    // the current pair so the AER submit can substitute that eye's stale
    // image with an "advanced 1 frame forward by engine MV" version. Both
    // submitted eyes then represent the SAME simulation timestamp,
    // collapsing the within-pair gap that produces the "pushed back when
    // walking" jitter.
    bool mvWarpAttempted = false;
    bool mvWarpOk[2] = {false, false};
    const int olderEye = (job->syntheticEye >= 0 && job->syntheticEye < 2)
        ? job->syntheticEye
        : ((job->currSerial[0] <= job->currSerial[1]) ? 0 : 1);
    if (GetAERV2Enabled() == 0 && job->engineMv && job->flowCurrSource[olderEye] &&
        m_d3dDevice && m_mvWarpedEye[olderEye]) {
        mvWarpAttempted = true;
        if (!m_mvWarp) m_mvWarp = std::make_unique<MotionVectorWarp>();
        const D3D12_RESOURCE_DESC dstDesc = m_mvWarpedEye[0]->GetDesc();
        const bool warpInitOk = m_mvWarp->EnsureInitialized(
            m_d3dDevice, dstDesc.Format,
            static_cast<uint32_t>(dstDesc.Width), dstDesc.Height);
        if (!m_mvWarpQueue) {
            D3D12_COMMAND_QUEUE_DESC qd{};
            qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
            if (FAILED(m_d3dDevice->CreateCommandQueue(&qd, IID_PPV_ARGS(&m_mvWarpQueue))) ||
                FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_mvWarpAlloc))) ||
                FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_mvWarpAlloc.Get(), nullptr, IID_PPV_ARGS(&m_mvWarpList))) ||
                FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_mvWarpFence)))) {
                Log("OpenXRManager: [worker] MV-warp queue/list/fence init FAILED\n");
                m_mvWarpQueue.Reset(); m_mvWarpAlloc.Reset();
                m_mvWarpList.Reset(); m_mvWarpFence.Reset();
            } else {
                m_mvWarpQueue->SetName(L"AERV2_mvwarp_queue");
                m_mvWarpAlloc->SetName(L"AERV2_mvwarp_alloc");
                m_mvWarpList->SetName(L"AERV2_mvwarp_list");
                m_mvWarpFence->SetName(L"AERV2_mvwarp_fence");
                m_mvWarpList->Close();
                m_mvWarpEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
            }
        }

        if (warpInitOk && m_mvWarpQueue && m_mvWarpList && m_mvWarpFence && m_mvWarpEvent) {
            if (SUCCEEDED(m_mvWarpAlloc->Reset()) &&
                SUCCEEDED(m_mvWarpList->Reset(m_mvWarpAlloc.Get(), nullptr))) {
                D3D12_RESOURCE_BARRIER barriers[4] = {};
                UINT bc = 0;
                auto addBarrier = [&](ID3D12Resource* r, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
                    if (!r || before == after) return;
                    barriers[bc].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                    barriers[bc].Transition.pResource = r;
                    barriers[bc].Transition.StateBefore = before;
                    barriers[bc].Transition.StateAfter = after;
                    barriers[bc].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                    ++bc;
                };
                addBarrier(job->flowCurrSource[olderEye], D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
                addBarrier(job->engineMv, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
                addBarrier(m_mvWarpedEye[olderEye].Get(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_RENDER_TARGET);
                if (bc > 0) m_mvWarpList->ResourceBarrier(bc, barriers);

                const float kStartScaleX = 1.0f;
                const float kStartScaleY = 1.0f;
                mvWarpOk[olderEye] = m_mvWarp->RecordWarp(
                    m_mvWarpList.Get(),
                    job->flowCurrSource[olderEye],
                    job->engineMv,
                    m_mvWarpedEye[olderEye].Get(),
                    kStartScaleX, kStartScaleY);

                bc = 0;
                addBarrier(job->flowCurrSource[olderEye], D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
                addBarrier(job->engineMv, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
                addBarrier(m_mvWarpedEye[olderEye].Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COMMON);
                if (bc > 0) m_mvWarpList->ResourceBarrier(bc, barriers);
                m_mvWarpList->Close();

                if (mvWarpOk[olderEye]) {
                    ID3D12CommandList* lists[] = { m_mvWarpList.Get() };
                    m_mvWarpQueue->ExecuteCommandLists(1, lists);
                    ++m_mvWarpFenceValue;
                    m_mvWarpQueue->Signal(m_mvWarpFence.Get(), m_mvWarpFenceValue);
                    if (m_mvWarpFence->GetCompletedValue() < m_mvWarpFenceValue) {
                        m_mvWarpFence->SetEventOnCompletion(m_mvWarpFenceValue, m_mvWarpEvent);
                        WaitForSingleObject(m_mvWarpEvent, INFINITE);
                    }
                    m_mvWarpedEyeReady[olderEye] = true;
                    m_mvWarpedValidPairId[olderEye].store(job->pairId, std::memory_order_release);
                }
            }
        }
    }

    const bool mvAvail = (job->engineMv != nullptr);
    const bool depthAvail = (job->engineDepth != nullptr);
    const bool temporalDepthAvail =
        job->prevDepth[0] && job->currDepth[0] &&
        job->prevDepth[1] && job->currDepth[1];
    if ((job->pairId % 300) == 0 || job->pairId == 1) {
        uint32_t mvW = 0, mvH = 0, mvFmt = 0;
        uint32_t dW = 0, dH = 0, dFmt = 0;
        if (job->engineMv) {
            auto d = job->engineMv->GetDesc();
            mvW = static_cast<uint32_t>(d.Width);
            mvH = d.Height;
            mvFmt = static_cast<uint32_t>(d.Format);
        }
        if (job->engineDepth) {
            auto d = job->engineDepth->GetDesc();
            dW = static_cast<uint32_t>(d.Width);
            dH = d.Height;
            dFmt = static_cast<uint32_t>(d.Format);
        }
        Log("OpenXRManager: [worker] pair=%llu engineMv=%p (%ux%u fmt=%u avail=%d) engineDepth=%p avail=%d temporalDepth=%d olderEye=%d (serials=[%llu,%llu]) warpAttempted=%d warpOk=[%d,%d]\n",
            static_cast<unsigned long long>(job->pairId),
            job->engineMv, mvW, mvH, mvFmt, mvAvail ? 1 : 0,
            job->engineDepth, depthAvail ? 1 : 0,
            temporalDepthAvail ? 1 : 0,
            olderEye,
            static_cast<unsigned long long>(job->currSerial[0]),
            static_cast<unsigned long long>(job->currSerial[1]),
            mvWarpAttempted ? 1 : 0,
            mvWarpOk[0] ? 1 : 0,
            mvWarpOk[1] ? 1 : 0);
    }

    bool convertedOk = true;
    bool inBetweenInputsOk = false;  // 1:1 half-rate: renderedEye history also converted
    if (m_opticalFlow) {
        if (GetAERV2Enabled() != 0 && job->renderedEye >= 0 && job->syntheticEye >= 0) {
            // Same-eye temporal: the population path (OnPresent) fills the
            // syntheticEye flow slots (prev+curr captures of that eye), and
            // ProcessTemporalFrame below consumes flowPrev/flowCurr[syntheticEye].
            if (!m_opticalFlow->ConvertToInputTexture(job->flowPrevSource[job->syntheticEye], job->flowPrev[job->syntheticEye]) ||
                !m_opticalFlow->ConvertToInputTexture(job->flowCurrSource[job->syntheticEye], job->flowCurr[job->syntheticEye])) {
                convertedOk = false;
            }
            // 1:1 half-rate: the in-between pair warps BOTH eyes, so also convert
            // the renderedEye's own temporal history (OnPresent stages it only
            // when half-rate is on). Non-fatal — failure just skips the in-between.
            if (convertedOk && GetAERHalfRate() != 0 &&
                job->flowPrevSource[job->renderedEye] && job->flowCurrSource[job->renderedEye] &&
                job->flowPrev[job->renderedEye] && job->flowCurr[job->renderedEye]) {
                inBetweenInputsOk =
                    m_opticalFlow->ConvertToInputTexture(job->flowPrevSource[job->renderedEye], job->flowPrev[job->renderedEye]) &&
                    m_opticalFlow->ConvertToInputTexture(job->flowCurrSource[job->renderedEye], job->flowCurr[job->renderedEye]);
            }
        } else {
            for (int eye = 0; eye < 2 && convertedOk; ++eye) {
                if (!m_opticalFlow->ConvertToInputTexture(job->flowPrevSource[eye], job->flowPrev[eye]) ||
                    !m_opticalFlow->ConvertToInputTexture(job->flowCurrSource[eye], job->flowCurr[eye])) {
                    convertedOk = false;
                }
            }
        }
    } else {
        convertedOk = false;
    }

    bool leftOk = false;
    bool rightOk = false;
    bool leftSynth = false;
    bool rightSynth = false;
    bool realAERV2Used = false;
    bool syntheticOk = false;
    if (convertedOk) {
        const uint32_t synthSlot = static_cast<uint32_t>(job->pairId & 1ull);
        if (GetAERV2Enabled() != 0 && m_d3dDevice && m_d3dQueue && m_aerV2SynthEye[0][synthSlot] && m_aerV2SynthEye[1][synthSlot]) {
            const int syntheticEye = (job->syntheticEye >= 0 && job->syntheticEye < 2) ? job->syntheticEye : olderEye;
            const int renderedEye = (job->renderedEye >= 0 && job->renderedEye < 2) ? job->renderedEye : (syntheticEye ^ 1);
            if (!m_aerV2Pipeline) {
                m_aerV2Pipeline = std::make_unique<aer_v2::AerV2Pipeline>();
            }
            ID3D12Resource* flowInitSource = job->flowCurr[renderedEye] ? job->flowCurr[renderedEye] : job->flowPrev[syntheticEye];
            if (!flowInitSource) {
                Log("OpenXRManager: [worker] missing flow init source pair=%llu syntheticEye=%d renderedEye=%d\n",
                    static_cast<unsigned long long>(job->pairId),
                    syntheticEye,
                    renderedEye);
            } else {
            const D3D12_RESOURCE_DESC flowDesc = flowInitSource->GetDesc();
            m_aerV2Pipeline->SetMode(aer_v2::AerV2Pipeline::Mode::AERv2HighQ);
            m_aerV2Pipeline->SetForceMatchedEyePoses(true);
            if (m_aerV2Pipeline->EnsureInitialized(m_d3dDevice,
                                                  m_d3dQueue,
                                                  static_cast<uint32_t>(flowDesc.Width),
                                                  flowDesc.Height)) {
                const XrPosef predictedMatched = ExtrapolatePose(
                    job->prevPose[renderedEye],
                    job->currPose[renderedEye],
                    kAERV2FrameGenPoseT);
                XrPosef predictedSynthetic = ExtrapolatePose(
                    job->prevPose[syntheticEye],
                    job->currPose[syntheticEye],
                    kAERV2FrameGenPoseT);
                predictedSynthetic.orientation = predictedMatched.orientation;
                const float mvScaleX = NgxGetMvScaleX();
                const float mvScaleY = NgxGetMvScaleY();
                ID3D12Resource* realMvSource = nullptr;
                if (job->engineMv && m_d3dDevice && m_mvWarpQueue && m_mvWarpAlloc && m_mvWarpList && m_mvWarpFence && m_mvWarpEvent) {
                    const D3D12_RESOURCE_DESC mvDesc = job->engineMv->GetDesc();
                    bool recreateMvScratch = true;
                    if (m_aerV2MvScratch) {
                        const auto cur = m_aerV2MvScratch->GetDesc();
                        if (cur.Width == mvDesc.Width && cur.Height == mvDesc.Height && cur.Format == mvDesc.Format) {
                            recreateMvScratch = false;
                        } else {
                            m_aerV2MvScratch.Reset();
                        }
                    }
                    if (recreateMvScratch) {
                        D3D12_HEAP_PROPERTIES hp{};
                        hp.Type = D3D12_HEAP_TYPE_DEFAULT;
                        if (FAILED(m_d3dDevice->CreateCommittedResource(
                                &hp,
                                D3D12_HEAP_FLAG_SHARED,
                                &mvDesc,
                                D3D12_RESOURCE_STATE_COMMON,
                                nullptr,
                                IID_PPV_ARGS(&m_aerV2MvScratch)))) {
                            Log("OpenXRManager: [worker] failed to create shareable MV scratch\n");
                            m_aerV2MvScratch.Reset();
                        } else {
                            SetD3DName(m_aerV2MvScratch.Get(), L"AERV2_engine_mv_scratch_shared");
                        }
                    }
                    if (m_aerV2MvScratch &&
                        SUCCEEDED(m_mvWarpAlloc->Reset()) &&
                        SUCCEEDED(m_mvWarpList->Reset(m_mvWarpAlloc.Get(), nullptr))) {
                        const D3D12_RESOURCE_STATES mvScratchBefore = recreateMvScratch
                            ? D3D12_RESOURCE_STATE_COMMON
                            : D3D12_RESOURCE_STATE_COPY_SOURCE;
                        D3D12_RESOURCE_BARRIER bars[3] = {};
                        UINT bc = 0;
                        auto addBar = [&](ID3D12Resource* r, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
                            if (!r || before == after) return;
                            bars[bc].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            bars[bc].Transition.pResource = r;
                            bars[bc].Transition.StateBefore = before;
                            bars[bc].Transition.StateAfter = after;
                            bars[bc].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            ++bc;
                        };
                        addBar(job->engineMv, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_SOURCE);
                        addBar(m_aerV2MvScratch.Get(), mvScratchBefore, D3D12_RESOURCE_STATE_COPY_DEST);
                        if (bc > 0) m_mvWarpList->ResourceBarrier(bc, bars);
                        m_mvWarpList->CopyResource(m_aerV2MvScratch.Get(), job->engineMv);
                        bc = 0;
                        addBar(job->engineMv, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON);
                        addBar(m_aerV2MvScratch.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
                        if (bc > 0) m_mvWarpList->ResourceBarrier(bc, bars);
                        m_mvWarpList->Close();
                        ID3D12CommandList* lists[] = { m_mvWarpList.Get() };
                        m_mvWarpQueue->ExecuteCommandLists(1, lists);
                        ++m_mvWarpFenceValue;
                        m_mvWarpQueue->Signal(m_mvWarpFence.Get(), m_mvWarpFenceValue);
                        if (m_mvWarpFence->GetCompletedValue() < m_mvWarpFenceValue) {
                            m_mvWarpFence->SetEventOnCompletion(m_mvWarpFenceValue, m_mvWarpEvent);
                            WaitForSingleObject(m_mvWarpEvent, INFINITE);
                        }
                        realMvSource = m_aerV2MvScratch.Get();
                    }
                }
                ID3D12Resource* sourcePrevDepth = job->prevDepth[syntheticEye] ? job->prevDepth[syntheticEye] : job->engineDepth;
                ID3D12Resource* sourceCurrDepth = job->currDepth[syntheticEye] ? job->currDepth[syntheticEye] : job->engineDepth;
                syntheticOk = m_aerV2Pipeline->ProcessTemporalFrame(
                    syntheticEye,
                    job->flowPrev[syntheticEye], job->flowCurr[syntheticEye],
                    job->flowCurrSource[syntheticEye], job->flowCurrSource[syntheticEye],
                    realMvSource, sourcePrevDepth, sourceCurrDepth,
                    mvScaleX, mvScaleY,
                    job->fov[syntheticEye],
                    job->prevPose[syntheticEye], job->currPose[syntheticEye], predictedSynthetic,
                    kAERV2FrameGenPoseT,
                    m_aerV2SynthEye[syntheticEye][synthSlot].Get());
                if (syntheticEye == 0) {
                    leftOk = syntheticOk;
                    leftSynth = syntheticOk;
                } else {
                    rightOk = syntheticOk;
                    rightSynth = syntheticOk;
                }
                realAERV2Used = syntheticOk;

                // ===== 1:1 half-rate: BOTH-EYE in-between frame (mid, blend 0.5) =====
                // Each eye gets its OWN fresh NvOF temporal mid so BOTH eyes update
                // every display interval (90 Hz). The earlier 1-warp version reused
                // the keyframe synth for the syntheticEye half -> that eye froze for
                // 2 intervals (45 Hz, stuttery) = the "not 90 Hz both eyes" jitter.
                // Pure NvOF (engineMv=null): same-eye temporal flow = real stereo.
                // 2 warps/pair; the present-thread throttle that made this too slow
                // before is now removed, so the budget is there.
                if (syntheticOk && GetAERHalfRate() != 0 && inBetweenInputsOk &&
                    m_aerV2InBetween[0][synthSlot] && m_aerV2InBetween[1][synthSlot]) {
                    bool bothEyesOk = true;
                    XrPosef predInB[2]{};
                    for (int eye = 0; eye < 2 && bothEyesOk; ++eye) {
                        if (!job->flowPrev[eye] || !job->flowCurr[eye] || !job->flowCurrSource[eye]) {
                            bothEyesOk = false;
                            break;
                        }
                        predInB[eye] = ExtrapolatePose(job->prevPose[eye], job->currPose[eye], kAERV2FrameGenPoseT);
                        predInB[eye].orientation = predictedMatched.orientation;  // matched stereo
                        ID3D12Resource* pD = job->prevDepth[eye] ? job->prevDepth[eye] : job->engineDepth;
                        ID3D12Resource* cD = job->currDepth[eye] ? job->currDepth[eye] : job->engineDepth;
                        bothEyesOk = m_aerV2Pipeline->ProcessTemporalFrame(
                            eye,
                            job->flowPrev[eye], job->flowCurr[eye],
                            job->flowCurrSource[eye], job->flowCurrSource[eye],
                            nullptr, pD, cD,
                            mvScaleX, mvScaleY,
                            job->fov[eye],
                            job->prevPose[eye], job->currPose[eye], predInB[eye],
                            kAERV2FrameGenPoseT,
                            m_aerV2InBetween[eye][synthSlot].Get());
                    }
                    if (bothEyesOk) {
                        std::lock_guard<std::mutex> publishLock(m_presentMutex);
                        m_inBetweenReadyPairId = job->submitPairId;   // PAIR counter
                        m_inBetweenSlot = synthSlot;
                        m_inBetweenSyntheticEye = syntheticEye;        // -1 also fine now (both eyes fresh)
                        for (int eye = 0; eye < 2; ++eye) {
                            m_inBetweenEyePoses[eye] = predInB[eye];
                            m_inBetweenEyeFovs[eye] = job->fov[eye];
                            m_inBetweenEyeViewsValid[eye] = job->hasView[eye];
                        }
                    }
                    if ((job->submitPairId % 300) == 1) {
                        Log("OpenXRManager: [1:1 producer] BOTH-eye in-between pair=%llu ok=%d publishedReady=%llu\n",
                            static_cast<unsigned long long>(job->submitPairId), bothEyesOk ? 1 : 0,
                            static_cast<unsigned long long>(m_inBetweenReadyPairId));
                    }
                } else if (GetAERHalfRate() != 0 && (job->submitPairId % 300) == 1) {
                    Log("OpenXRManager: [1:1 producer] in-between SKIPPED pair=%llu synthOk=%d inputsOk=%d haveTex0=%d haveTex1=%d\n",
                        static_cast<unsigned long long>(job->submitPairId), syntheticOk ? 1 : 0, inBetweenInputsOk ? 1 : 0,
                        m_aerV2InBetween[0][synthSlot] ? 1 : 0, m_aerV2InBetween[1][synthSlot] ? 1 : 0);
                }
            }
            }
        }

        if (!realAERV2Used && GetAERV2Enabled() == 0 && m_opticalFlow) {
            leftOk  = m_opticalFlow->ExecuteFlow(job->flowPrev[0], job->flowCurr[0], 0);
            rightOk = m_opticalFlow->ExecuteFlow(job->flowPrev[1], job->flowCurr[1], 1);
            if (leftOk)  leftSynth  = m_opticalFlow->SynthesizeMidpoint(job->flowPrev[0], job->flowCurr[0], 0);
            if (rightOk) rightSynth = m_opticalFlow->SynthesizeMidpoint(job->flowPrev[1], job->flowCurr[1], 1);
        }
    }

    bool published = false;
    if (syntheticOk) {
        std::lock_guard<std::mutex> publishLock(m_presentMutex);
        if (job->pairId > m_interpolatedPairId) {
            m_interpolatedPairId = job->pairId;
            m_interpolatedSynthSlot = static_cast<uint32_t>(job->pairId & 1ull);
            m_interpolatedSyntheticEye = olderEye;
            for (int eye = 0; eye < 2; ++eye) {
                if (eye == olderEye) {
                    m_interpolatedEyePoses[eye] = ExtrapolatePose(
                        job->prevPose[eye],
                        job->currPose[eye],
                        kAERV2FrameGenPoseT);
                    m_interpolatedEyePoses[eye].orientation = ExtrapolatePose(
                        job->prevPose[olderEye ^ 1],
                        job->currPose[olderEye ^ 1],
                        kAERV2FrameGenPoseT).orientation;
                    m_interpolatedEyeFovs[eye] = job->fov[eye];
                    m_interpolatedEyeViewsValid[eye] = job->hasView[eye];
                } else {
                    m_interpolatedEyeViewsValid[eye] = false;
                }
            }
            published = true;
        }
        m_aerV2DonePairId.store(job->pairId, std::memory_order_release);
    }

    const bool verbose = (g_verboseLog != 0);
    if (verbose || (job->pairId % 300) == 0 || !leftOk || !rightOk || !leftSynth || !rightSynth) {
        Log("OpenXRManager: [worker] pair=%llu flowL=%d flowR=%d synthL=%d synthR=%d converted=%d published=%d realAERV2=%d (lastSubmittedPair=%llu interpolatedPair=%llu)\n",
            static_cast<unsigned long long>(job->pairId),
            leftOk ? 1 : 0,
            rightOk ? 1 : 0,
            leftSynth ? 1 : 0,
            rightSynth ? 1 : 0,
            convertedOk ? 1 : 0,
            published ? 1 : 0,
            realAERV2Used ? 1 : 0,
            static_cast<unsigned long long>(m_lastSubmittedPairId),
            static_cast<unsigned long long>(m_interpolatedPairId));
    }

    ReleaseAERV2JobRefs(*job);
    m_aerV2BusyPairId.store(0, std::memory_order_release);
}

void OpenXRManager::AERV2WorkerThreadMain() {
    Log("OpenXRManager: AER V2 worker thread started.\n");
    while (true) {
        std::unique_ptr<AERV2Job> job;
        {
            std::unique_lock<std::mutex> lock(m_aerV2JobMutex);
            m_aerV2JobCv.wait(lock, [this]() {
                return m_aerV2WorkerShutdown.load(std::memory_order_acquire) ||
                       m_aerV2PendingJob != nullptr;
            });
            if (m_aerV2WorkerShutdown.load(std::memory_order_acquire) && !m_aerV2PendingJob) {
                break;
            }
            job = std::move(m_aerV2PendingJob);
            if (job) {
                m_aerV2BusyPairId.store(job->pairId, std::memory_order_release);
            }
        }
        if (!job) {
            continue;
        }
        ProcessAERV2Job(std::move(job));
    }
    Log("OpenXRManager: AER V2 worker thread exiting.\n");
}
