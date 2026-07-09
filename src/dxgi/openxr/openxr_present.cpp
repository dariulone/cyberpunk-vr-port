// openxr_present.cpp - OnPresent(): per-Present capture/submit trigger + eye schedule.
// Split verbatim from openxr_manager.cpp (OpenXRManager method). Shared module
// state/helpers via openxr_internal.h (inline).
#include "openxr_manager.h"
#include "openxr_internal.h"
#include "openxr_math.h"
#include "shared_slots.h"
#include "runtime_fov_correction.h"
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <vector>
#include <utility>
#include <chrono>
#include <thread>
#include <memory>
#include <algorithm>
#include <dxgi1_4.h>

void OpenXRManager::OnPresent(IDXGISwapChain* swapChain) {
    // [HANDS] Shared Memory Output
    static HANDLE s_hMapFile = NULL;
    static float* s_pSharedHands = nullptr;
    if (!s_hMapFile) {
        s_hMapFile = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 1024, "CyberpunkVR_Hands_Shared");
        if (s_hMapFile) {
            s_pSharedHands = (float*)MapViewOfFile(s_hMapFile, FILE_MAP_ALL_ACCESS, 0, 0, 1024);
            m_sharedHandsPtr = s_pSharedHands;   // expose to GetSharedSlot (overlay barrel crosshair)
        }
    }
    if (s_pSharedHands) {
        std::lock_guard<std::mutex> lock(const_cast<std::mutex&>(m_handMutex));
        // Slot [32]: VR hand-tracking request for the RED4ext plugin (set from the overlay menu).
        s_pSharedHands[32] = static_cast<float>(m_vrHandTrackingMode.load(std::memory_order_relaxed));
        s_pSharedHands[58] = static_cast<float>(m_weaponAimEnable.load(std::memory_order_relaxed)); // weapon-aim enable
        // shared[23]: 0/unset = immersive holsters (default), 1 = simple slot mapping. Inverted so the
        // zero-initialized shared block defaults to the immersive (current) behaviour before the first
        // publish. The CET Holster mod reads this via GetVRSharedSlot(23).
        s_pSharedHands[23] = (m_immersiveHolsters.load(std::memory_order_relaxed) != 0) ? 0.0f : 1.0f;
        s_pSharedHands[59] = 5.0f;  // mode 5 = game muzzle xform (the working solution)
        // [70..75]: anatomical HMD/body->shoulder offsets (auto-calibration result).
        // Right (rx,ry,rz), then left (lx,ly,lz). [76] = valid flag. Kept outside
        // [34..47], which is the regular calibration block.
        if (m_calibExtValid.load(std::memory_order_relaxed)) {
            for (int i = 0; i < 6; ++i) s_pSharedHands[70 + i] = m_calibExt[i].load(std::memory_order_relaxed);
            s_pSharedHands[76] = 1.0f;
        }
        // [77..80]: T-pose measured anatomy (real arm length R/L, HMD eye height) + valid flag.
        // The plugin scales the avatar arm bones to match (gizmo-path), straightening a relaxed arm.
        if (m_measureValid.load(std::memory_order_relaxed)) {
            s_pSharedHands[77] = m_userArmLenR.load(std::memory_order_relaxed);
            s_pSharedHands[78] = m_userArmLenL.load(std::memory_order_relaxed);
            s_pSharedHands[79] = m_userEyeHeight.load(std::memory_order_relaxed);
            s_pSharedHands[80] = 1.0f;
        }
        // [89]: HMD PHYSICAL height relative to the recenter base (~0 standing, negative when the
        // user physically squats). The game FPP camera Lua samples is a FIXED eye height, so the
        // plugin needs this to actually lower the body / bend the knees on a real-life squat.
        // [85..88] are written by the plugin (camera->head offset) -- do not touch them here.
        // PAIR-LOCKED: use the frozen physical head height (snapshot at the pair
        // boundary). [89] head height + [90] neck-pivot are now written from the
        // frozen snapshot inside FlushHandsToShared (published at the pair boundary,
        // BEFORE the next pair's animation) together with the hand slots [0..19], so
        // they are no longer sampled live per present here.
        // [91..93]: the ACTIVE baked camera->head offset (game-local right/fwd/up). dxgi shifts the
        // VIEW by this in LocateCamera; the plugin adds the SAME offset to camModelPos so the avatar
        // head sits exactly where the (offset-tuned) view sits -> head = camera, body follows.
        {
            float cb[3]; GetCameraOffset(cb);
            s_pSharedHands[91] = cb[0]; s_pSharedHands[92] = cb[1]; s_pSharedHands[93] = cb[2];
        }
        // IMPORTANT: hand pose slots [0..19] are flushed in OnLocateCameraCallback
        // BEFORE render (FlushHandsToShared). Do NOT rewrite them here after render,
        // or the next frame may see a mixed temporal state (one wrong frame even
        // on the flat monitor, amplified in AER). Keep OnPresent for config/static
        // slots only.

        // [33..47] IK calibration from the overlay; [48] one-shot diag request.
        s_pSharedHands[33] = static_cast<float>(m_calibValid.load(std::memory_order_relaxed));
        for (int i = 0; i < 14; ++i) s_pSharedHands[34 + i] = m_calib[i].load(std::memory_order_relaxed);
        s_pSharedHands[48] = static_cast<float>(m_logDiagReq.load(std::memory_order_relaxed));
    }

    if (!swapChain) return;

    uint64_t s_presentCount = m_presentCount.fetch_add(1, std::memory_order_relaxed) + 1;
    const bool monoEnabled = m_monoSubmitEnabled.load(std::memory_order_relaxed);
    const bool aerEnabled = monoEnabled && IsAERSubmitEnabled();
    const bool menuRectActive = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
    const bool syncSequential = aerEnabled && GetSyncSequential() != 0;
    const int scheduledEye = aerEnabled ? m_renderEyeIndex.load(std::memory_order_relaxed) : 0;
    const int presentEye = aerEnabled ? GetRenderedCameraEye() : 0;
    const bool aerWarmupFrame = aerEnabled && m_aerWarmupRemaining > 0;

    // ===== POSE PAIR LOCKING — publish point (pipeline shift) =====
    // Snapshot the tracking state + write the VRIK shared slots HERE, BEFORE the
    // next animation/render pass (the plugin reads it during anim eval, which
    // precedes render/LocateCamera).
    //
    // Publish EVERY present in both mono AND AER. An earlier AER-only gate
    // (presentEye==1, the pair boundary) published hands only every OTHER
    // present -> VRIK/skeleton updated at HALF the present rate -> hands
    // "teleport" at ~20-45 Hz while the world rendered at 90. Publishing every
    // present gives each eye's render the freshest hand pose (the live-pose path,
    // usePairLock=0 default, already avoids the frozen-pair coherence issue, and
    // any per-eye skeleton tear from a mid-pair update is far less visible than
    // half-rate hands).
    {
        if (aerEnabled) m_pairLockLastEye = presentEye;
        UpdatePairLock();
        FlushHandsToShared();
    }

    // NOTE: NO present-thread SLEEP throttle here. There is no busy-wait/Sleep
    // cap on the game present thread. HMD-pacing for AER V2 is done by the
    // m_frameSyncEvent wait at the end of OnPresent. The old Sleep-to-45-pairs/s
    // "GPU boost" was removed because it stalled the engine to ~7 pairs/s; the
    // current path runs at full HMD rate, one eye per frame. To cap the engine
    // further, use an
    // EXTERNAL fps limiter (in-game / RTSS / driver).

    if (g_verboseLog && aerEnabled && scheduledEye != presentEye && (s_presentCount % 300) == 1) {
        Log("OpenXRManager: AER eye mismatch corrected. scheduled=%d rendered=%d serial=%llu\n",
            scheduledEye,
            presentEye,
            static_cast<unsigned long long>(s_presentCount));
    }

    auto latchSyncedSequentialPair = [this]() {
        m_syncedPoseValid.store(false, std::memory_order_relaxed);
        m_syncedPosX.store(m_posX.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedPosY.store(m_posY.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedPosZ.store(m_posZ.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriX.store(m_oriX.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriY.store(m_oriY.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriZ.store(m_oriZ.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriW.store(m_oriW.load(std::memory_order_relaxed), std::memory_order_relaxed);

        bool viewsValid = false;
        {
            std::lock_guard<std::mutex> viewLock(m_viewMutex);
            if (m_views.size() >= 2) {
                for (int eye = 0; eye < 2; ++eye) {
                    m_syncedEyePoses[eye] = m_views[eye].pose;
                    m_syncedEyeFovs[eye] = m_views[eye].fov;
                }
                viewsValid = true;
            }
        }

        m_syncedEyeViewsValid = viewsValid;
        m_syncedPoseValid.store(true, std::memory_order_relaxed);
        ++m_syncedPairId;
        if (g_verboseLog && (m_syncedPairId == 1 || (m_syncedPairId % 300) == 0)) {
            Log("OpenXRManager: synchronized sequential pair latched. pair=%llu views=%d 3dof=%d\n",
                static_cast<unsigned long long>(m_syncedPairId),
                viewsValid ? 1 : 0,
                Get3DofMovement() != 0 ? 1 : 0);
        }
    };

    if (syncSequential && !m_syncedPoseValid.load(std::memory_order_relaxed)) {
        latchSyncedSequentialPair();
    }
    // Pair id MUST be coupled to the eye-capture cadence, not the global present
    // counter parity. m_renderEyeIndex is reset to 0 on AER enable, but the
    // global present counter parity is arbitrary at that moment; deriving the
    // pair id from (s_presentCount+1)/2 split eye0/eye1 of the first pair across
    // a pair boundary ~50% of toggles, so the pair never completed and only
    // empty frames were submitted (frozen/blank HMD). Bump a dedicated counter
    // on each eye-0 capture so left and right always share the id.
    uint64_t presentPairId = 0;
    if (aerEnabled) {
        if (syncSequential && m_syncedPairId != 0) {
            presentPairId = m_syncedPairId;
        } else {
            if (!aerWarmupFrame && presentEye == 0) {
                ++m_aerPairCounter;
            }
            presentPairId = m_aerPairCounter;
        }
    }

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) {
        Log("OpenXRManager: Present hook could not read swapchain desc.\n");
        return;
    }

    IDXGISwapChain3* swapChain3 = nullptr;
    UINT backBufferIndex = 0;
    if (SUCCEEDED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3)))) {
        backBufferIndex = swapChain3->GetCurrentBackBufferIndex();
        swapChain3->Release();
    }

    ID3D12Resource* backBuffer = nullptr;
    D3D12_RESOURCE_DESC resourceDesc{};
    if (SUCCEEDED(swapChain->GetBuffer(backBufferIndex, IID_PPV_ARGS(&backBuffer)))) {
        resourceDesc = backBuffer->GetDesc();
    }

    XrPosef capturedPose{};
    capturedPose.orientation.w = 1.0f;
    XrFovf capturedFov{};
    bool hasCapturedView = false;
    XrPosef monoCapturedPoses[2]{};
    XrFovf monoCapturedFovs[2]{};
    bool monoCapturedViews[2] = {};
    if (aerEnabled && !menuRectActive) {
        std::lock_guard<std::mutex> viewLock(m_viewMutex);
        const bool useSyncedView = syncSequential && m_syncedEyeViewsValid && presentEye >= 0 && presentEye < 2;
        const bool useCurrentView = presentEye >= 0 && presentEye < static_cast<int>(m_views.size());
        if (useSyncedView || useCurrentView) {
            capturedPose = useSyncedView ? m_syncedEyePoses[presentEye] : m_views[presentEye].pose;
            const XrFovf sourceFov = useSyncedView ? m_syncedEyeFovs[presentEye] : m_views[presentEye].fov;
            float fovWidth = static_cast<float>(desc.BufferDesc.Width);
            float fovHeight = static_cast<float>(desc.BufferDesc.Height);
            if ((fovWidth <= 1.0f || fovHeight <= 1.0f) && resourceDesc.Width != 0 && resourceDesc.Height != 0) {
                fovWidth = static_cast<float>(resourceDesc.Width);
                fovHeight = static_cast<float>(resourceDesc.Height);
            }
            XrFovf pairFovs[2]{};
            const XrFovf* pairFovPtr = nullptr;
            if (m_views.size() >= 2) {
                pairFovs[0] = useSyncedView ? m_syncedEyeFovs[0] : m_views[0].fov;
                pairFovs[1] = useSyncedView ? m_syncedEyeFovs[1] : m_views[1].fov;
                pairFovPtr = pairFovs;
            }
            capturedFov = ApplyForcedProjectionFov(sourceFov, pairFovPtr, presentEye, fovWidth, fovHeight);
            hasCapturedView = true;

            // Render-pose submit (AER V2): replace the present-time pose with the
            // exact head pose this eye's frame was rendered with (captured by the
            // camera hook), so the compositor time-warps the older 1/2-rate eye
            // forward to display time instead of showing it stale. Keep the runtime
            // per-eye offset for the correct stereo baseline; only head pos+ori
            // carry the per-eye render timestamp.
            if (GetRenderPoseSubmit() != 0 && m_views.size() >= 2) {
                std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
                
                uint32_t renderedSeq = GetRenderedCameraSeq();
                int idx = renderedSeq % 256;
                
                XrPosef rp{};
                bool validRp = false;
                if (renderedSeq > 0 && m_poseQueueFrame[idx] == renderedSeq) {
                    rp = m_poseQueue[idx];
                    validRp = true;
                } else if (m_renderEyeHeadPoseValid[presentEye]) {
                    rp = m_renderEyeHeadPose[presentEye];
                    validRp = true;
                }

                if (validRp) {
                    const XrVector3f headCenter{
                        (m_views[0].pose.position.x + m_views[1].pose.position.x) * 0.5f,
                        (m_views[0].pose.position.y + m_views[1].pose.position.y) * 0.5f,
                        (m_views[0].pose.position.z + m_views[1].pose.position.z) * 0.5f};
                    const XrVector3f eyeOffset{
                        m_views[presentEye].pose.position.x - headCenter.x,
                        m_views[presentEye].pose.position.y - headCenter.y,
                        m_views[presentEye].pose.position.z - headCenter.z};
                    capturedPose.orientation = rp.orientation;
                    capturedPose.position = {
                        rp.position.x + eyeOffset.x,
                        rp.position.y + eyeOffset.y,
                        rp.position.z + eyeOffset.z};
                }
            }
            // Submit-pose cant, PAIRED with the render-side cant (dxgi_proxy
            // OnFinalCameraCallback). The render camera of this eye is already canted,
            // so its content carries the cant; the submitted pose must carry the SAME
            // cant for the compositor to reproject it correctly. Both-canted (render +
            // submit) is self-consistent, so screen-space HUD/
            // laser/hands do NOT double (that only happened with submit-ONLY cant,
            // where the un-canted content disagreed with the canted submit pose).
            // No-op on symmetric HMDs (Pico). Mono path is left un-canted (one shared
            // frame can't carry a per-eye render cant).
            ApplyCantToPose(capturedPose, pairFovPtr, presentEye);
        }
    } else if (monoEnabled) {
        std::lock_guard<std::mutex> viewLock(m_viewMutex);
        if (m_views.size() >= 2) {
            float fovWidth = static_cast<float>(desc.BufferDesc.Width);
            float fovHeight = static_cast<float>(desc.BufferDesc.Height);
            if ((fovWidth <= 1.0f || fovHeight <= 1.0f) && resourceDesc.Width != 0 && resourceDesc.Height != 0) {
                fovWidth = static_cast<float>(resourceDesc.Width);
                fovHeight = static_cast<float>(resourceDesc.Height);
            }

            bool hasRenderHeadPose = false;
            XrPosef renderHeadPose{};
            renderHeadPose.orientation.w = 1.0f;
            {
                std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
                uint32_t renderedSeq = GetRenderedCameraSeq();
                int idx = renderedSeq % 256;
                
                if (renderedSeq > 0 && m_poseQueueFrame[idx] == renderedSeq) {
                    renderHeadPose = m_poseQueue[idx];
                    hasRenderHeadPose = true;
                } else if (m_renderEyeHeadPoseValid[0]) {
                    renderHeadPose = m_renderEyeHeadPose[0];
                    hasRenderHeadPose = true;
                }
            }

            const XrVector3f headCenter{
                (m_views[0].pose.position.x + m_views[1].pose.position.x) * 0.5f,
                (m_views[0].pose.position.y + m_views[1].pose.position.y) * 0.5f,
                (m_views[0].pose.position.z + m_views[1].pose.position.z) * 0.5f};
            XrPosef monoCenterPose{};
            monoCenterPose.orientation = m_views[0].pose.orientation;
            monoCenterPose.position = headCenter;
            if (GetRenderPoseSubmit() != 0 && hasRenderHeadPose) {
                monoCenterPose = renderHeadPose;
            }
            for (int eye = 0; eye < 2; ++eye) {
                XrVector3f eyeOffset{
                    m_views[eye].pose.position.x - headCenter.x,
                    m_views[eye].pose.position.y - headCenter.y,
                    m_views[eye].pose.position.z - headCenter.z};
                monoCapturedPoses[eye] = monoCenterPose;
                monoCapturedPoses[eye].position.x += eyeOffset.x;
                monoCapturedPoses[eye].position.y += eyeOffset.y;
                monoCapturedPoses[eye].position.z += eyeOffset.z;
                XrFovf monoPairFovs[2] = { m_views[0].fov, m_views[1].fov };
                monoCapturedFovs[eye] = ApplyForcedProjectionFov(m_views[eye].fov, monoPairFovs, eye, fovWidth, fovHeight);
                // No cant pose rotation (removed): in mono both eyes derive from ONE
                // frame, so a per-eye cant delta doubled the whole image. See the AER
                // note above.
                monoCapturedViews[eye] = true;
            }
        }
    }
    bool monoCaptureOk = false;
    // In AER mode the mono capture is REDUNDANT during gameplay: its output is
    // never submitted (the AER submit path uses m_capturedEyeFrames, not
    // m_monoCapturedFrame), but it still runs a full CopyResource +
    // ExecuteCommandLists + Signal + its own ring-buffer drain wait
    // (WaitForSingleObject INFINITE) EVERY present -- doubling the per-present
    // GPU work and tripping the capture-queue drain wait, which throttles the
    // game's present rate. CP2077 ties simulation ticks to present frames, so a
    // depressed present rate makes sim-driven things (NPC skeletal anim, world
    // logic) stutter while shader-time things (windmills/fans) stay smooth.
    //
    // BUT: in the MENU the AER capture is deliberately skipped (the
    // `!menuRectActive` guard on CapturePresentedFrame below) because menus
    // render as a single mono surface, not alternate-eye. So in the menu we MUST
    // run the mono capture or there is no image at all. Run mono capture when
    // either we're not in AER, or we're in AER but currently in a menu.
    if (monoEnabled && backBuffer && (!aerEnabled || menuRectActive)) {
        monoCaptureOk = CaptureMonoPresentedFrame(backBuffer, resourceDesc, s_presentCount,
            monoCapturedPoses, monoCapturedFovs, monoCapturedViews);
        if (!monoCaptureOk && (s_presentCount % 300) == 1) {
            Log("OpenXRManager: Mono capture failed. serial=%llu views=(%d,%d)\n",
                static_cast<unsigned long long>(s_presentCount),
                monoCapturedViews[0] ? 1 : 0,
                monoCapturedViews[1] ? 1 : 0);
        }
    }

    bool aerCaptureOk = false;
    if (aerEnabled && !menuRectActive && backBuffer && !aerWarmupFrame) {
        aerCaptureOk = CapturePresentedFrame(backBuffer, resourceDesc, presentEye, s_presentCount, presentPairId);
        if (!aerCaptureOk) {
            Log("OpenXRManager: AER capture failed for eye %d serial=%llu\n", presentEye, static_cast<unsigned long long>(s_presentCount));
        }
    } else if (aerWarmupFrame && (presentPairId == 1 || (presentPairId % 300) == 0)) {
        Log("OpenXRManager: AER warmup discarded. eye=%d serial=%llu pair=%llu remaining=%d\n",
            presentEye,
            static_cast<unsigned long long>(s_presentCount),
            static_cast<unsigned long long>(presentPairId),
            m_aerWarmupRemaining);
    }

    bool runAERV2Flow = false;
    uint64_t flowPairId = 0;
    ID3D12Resource* flowPrevSource[2] = {};
    ID3D12Resource* flowCurrSource[2] = {};
    ID3D12Resource* flowPrev[2] = {};
    ID3D12Resource* flowCurr[2] = {};
    ID3D12Resource* flowPrevDepth[2] = {};
    ID3D12Resource* flowCurrDepth[2] = {};
    std::unique_lock<std::mutex> presentLock(m_presentMutex);
        if (m_lastPresentedBackBuffer) {
            m_lastPresentedBackBuffer->Release();
            m_lastPresentedBackBuffer = nullptr;
        }

        m_lastPresentedWidth = resourceDesc.Width != 0 ? static_cast<uint32_t>(resourceDesc.Width) : desc.BufferDesc.Width;
        m_lastPresentedHeight = resourceDesc.Height != 0 ? resourceDesc.Height : desc.BufferDesc.Height;
        m_lastPresentedFormat = resourceDesc.Format != DXGI_FORMAT_UNKNOWN ? static_cast<uint32_t>(resourceDesc.Format) : static_cast<uint32_t>(desc.BufferDesc.Format);
        m_lastPresentedBufferIndex = backBufferIndex;
        m_lastPresentSerial = s_presentCount;
        if (aerEnabled && aerCaptureOk && presentEye >= 0 && presentEye < 2) {
            m_pendingEyeFrames[presentEye].pose = capturedPose;
            m_pendingEyeFrames[presentEye].fov = capturedFov;
            m_pendingEyeFrames[presentEye].hasView = hasCapturedView;

            const bool aerV2On = (GetAERV2Enabled() != 0);
            const bool pairReady =
                m_pendingEyeFrames[0].pairId == presentPairId &&
                m_pendingEyeFrames[1].pairId == presentPairId &&
                m_pendingEyeFrames[0].serial != 0 &&
                m_pendingEyeFrames[1].serial != 0 &&
                m_pendingEyeFrames[0].hasView &&
                m_pendingEyeFrames[1].hasView;
            if (aerV2On || pairReady) {
                if (aerV2On) {
                    std::swap(m_previousCapturedEyeFrames[presentEye], m_capturedEyeFrames[presentEye]);
                    std::swap(m_capturedEyeFrames[presentEye], m_pendingEyeFrames[presentEye]);
                } else {
                std::swap(m_previousCapturedEyeFrames[0], m_capturedEyeFrames[0]);
                std::swap(m_previousCapturedEyeFrames[1], m_capturedEyeFrames[1]);
                std::swap(m_capturedEyeFrames[0], m_pendingEyeFrames[0]);
                std::swap(m_capturedEyeFrames[1], m_pendingEyeFrames[1]);
                }
                // Per-frame resource renaming (nameFrameRole) removed: it formatted
                // 6 wide strings + called SetName every present UNDER m_presentMutex,
                // contending with the frame thread's submit. The resources are already
                // named at creation (L"AERV2_pending_eye%d_color/depth"), so the
                // per-frame pair/serial suffix is redundant debug verbosity. Re-enable
                // here only when chasing a capture-slot race in PIX/VS Graphics Diag.
                if (GetAERV2Enabled() == 0 &&
                    m_interpolatedPairId != 0 &&
                    m_interpolatedPairId != presentPairId) {
                    if ((presentPairId % 300) == 0) {
                        Log("OpenXRManager: AER V2 stale synth dropped. stale=%llu current=%llu lastSubmitted=%llu\n",
                            static_cast<unsigned long long>(m_interpolatedPairId),
                            static_cast<unsigned long long>(presentPairId),
                            static_cast<unsigned long long>(m_lastSubmittedPairId));
                    }
                    m_interpolatedPairId = 0;
                    m_interpolatedSynthSlot = 0;
                    m_interpolatedSyntheticEye = -1;
                    m_interpolatedEyeViewsValid[0] = false;
                    m_interpolatedEyeViewsValid[1] = false;
                }
                const bool synthSlotBusy = (GetAERV2Enabled() == 0) && (m_interpolatedPairId != 0);
                if (GetAERV2Enabled() != 0 && presentPairId == 1) {
                    Log("OpenXRManager: AER V2 optical-flow warmup active until pair=%llu\n",
                        static_cast<unsigned long long>(kAERV2FlowWarmupPairId));
                }
                // The NvOF synth now runs in FrameThreadMain
                // (continuous blendFactor matched to predictedDisplayTime), NOT
                // inline here. Disable the old present-thread V2 producer entirely;
                // OnPresent for V2 just captures+swaps. (V1/legacy worker path
                // below is unchanged.)
                if (false && aerV2On &&
                    m_lastSubmittedPairId != 0 &&
                    presentPairId >= kAERV2FlowWarmupPairId &&
                    !synthSlotBusy) {
                    const int renderedEye = presentEye;
                    const int syntheticEye = presentEye ^ 1;
                    const bool pairAlreadyPublished =
                        m_interpolatedPairId == presentPairId &&
                        m_interpolatedSyntheticEye >= 0;
                    if (!pairAlreadyPublished) {
                        // NvOF flow is TEMPORAL between same-eye
                        // prev/curr captures (not cross-eye). The synthetic eye's
                        // previous render (2 presents ago) is warped forward by
                        // blendFactor using same-eye temporal flow.
                        if (m_previousCapturedEyeFrames[syntheticEye].serial != 0 &&
                            m_previousCapturedEyeFrames[syntheticEye].texture &&
                            m_previousCapturedEyeFrames[syntheticEye].opticalFlowTexture &&
                            m_capturedEyeFrames[syntheticEye].serial != 0 &&
                            m_capturedEyeFrames[syntheticEye].texture &&
                            m_capturedEyeFrames[syntheticEye].opticalFlowTexture &&
                            m_capturedEyeFrames[syntheticEye].hasView) {
                            runAERV2Flow = true;
                            flowPairId = s_presentCount;
                            // Same-eye temporal: prev=older, curr=newer of synthetic eye
                            flowPrevSource[syntheticEye] = m_previousCapturedEyeFrames[syntheticEye].texture;
                            flowCurrSource[syntheticEye] = m_capturedEyeFrames[syntheticEye].texture;
                            flowPrev[syntheticEye] = m_previousCapturedEyeFrames[syntheticEye].opticalFlowTexture;
                            flowCurr[syntheticEye] = m_capturedEyeFrames[syntheticEye].opticalFlowTexture;
                            if (m_previousCapturedEyeFrames[syntheticEye].depthTexture &&
                                m_previousCapturedEyeFrames[syntheticEye].depthSerial == m_previousCapturedEyeFrames[syntheticEye].serial) {
                                flowPrevDepth[syntheticEye] = m_previousCapturedEyeFrames[syntheticEye].depthTexture;
                            }
                            if (m_capturedEyeFrames[syntheticEye].depthTexture &&
                                m_capturedEyeFrames[syntheticEye].depthSerial == m_capturedEyeFrames[syntheticEye].serial) {
                                flowCurrDepth[syntheticEye] = m_capturedEyeFrames[syntheticEye].depthTexture;
                            }
                            flowPrevSource[syntheticEye]->AddRef();
                            flowCurrSource[syntheticEye]->AddRef();
                            flowPrev[syntheticEye]->AddRef();
                            flowCurr[syntheticEye]->AddRef();
                            if (flowPrevDepth[syntheticEye]) flowPrevDepth[syntheticEye]->AddRef();
                            if (flowCurrDepth[syntheticEye]) flowCurrDepth[syntheticEye]->AddRef();
                        }
                        // 1:1 half-rate: also stage renderedEye's own temporal
                        // history so ProcessAERV2Job can warp BOTH eyes into the
                        // in-between pair (m_aerV2InBetween). Only when half-rate.
                        if (runAERV2Flow && GetAERHalfRate() != 0 && !flowPrevSource[renderedEye] &&
                            m_previousCapturedEyeFrames[renderedEye].serial != 0 &&
                            m_previousCapturedEyeFrames[renderedEye].texture &&
                            m_previousCapturedEyeFrames[renderedEye].opticalFlowTexture &&
                            m_capturedEyeFrames[renderedEye].serial != 0 &&
                            m_capturedEyeFrames[renderedEye].texture &&
                            m_capturedEyeFrames[renderedEye].opticalFlowTexture &&
                            m_capturedEyeFrames[renderedEye].hasView) {
                            flowPrevSource[renderedEye] = m_previousCapturedEyeFrames[renderedEye].texture;
                            flowCurrSource[renderedEye] = m_capturedEyeFrames[renderedEye].texture;
                            flowPrev[renderedEye] = m_previousCapturedEyeFrames[renderedEye].opticalFlowTexture;
                            flowCurr[renderedEye] = m_capturedEyeFrames[renderedEye].opticalFlowTexture;
                            if (m_previousCapturedEyeFrames[renderedEye].depthTexture &&
                                m_previousCapturedEyeFrames[renderedEye].depthSerial == m_previousCapturedEyeFrames[renderedEye].serial) {
                                flowPrevDepth[renderedEye] = m_previousCapturedEyeFrames[renderedEye].depthTexture;
                            }
                            if (m_capturedEyeFrames[renderedEye].depthTexture &&
                                m_capturedEyeFrames[renderedEye].depthSerial == m_capturedEyeFrames[renderedEye].serial) {
                                flowCurrDepth[renderedEye] = m_capturedEyeFrames[renderedEye].depthTexture;
                            }
                            flowPrevSource[renderedEye]->AddRef();
                            flowCurrSource[renderedEye]->AddRef();
                            flowPrev[renderedEye]->AddRef();
                            flowCurr[renderedEye]->AddRef();
                            if (flowPrevDepth[renderedEye]) flowPrevDepth[renderedEye]->AddRef();
                            if (flowCurrDepth[renderedEye]) flowCurrDepth[renderedEye]->AddRef();
                        }
                    }
                } else if (!aerV2On &&
                    m_lastSubmittedPairId != 0 &&
                    presentPairId >= kAERV2FlowWarmupPairId &&
                    !synthSlotBusy &&
                    m_previousCapturedEyeFrames[0].serial != 0 &&
                    m_previousCapturedEyeFrames[1].serial != 0 &&
                    m_previousCapturedEyeFrames[0].texture &&
                    m_previousCapturedEyeFrames[1].texture &&
                    m_previousCapturedEyeFrames[0].opticalFlowTexture &&
                    m_previousCapturedEyeFrames[1].opticalFlowTexture &&
                    m_capturedEyeFrames[0].texture &&
                    m_capturedEyeFrames[1].texture &&
                    m_capturedEyeFrames[0].opticalFlowTexture &&
                    m_capturedEyeFrames[1].opticalFlowTexture) {
                    runAERV2Flow = true;
                    flowPairId = presentPairId;
                    flowPrevSource[0] = m_previousCapturedEyeFrames[0].texture;
                    flowPrevSource[1] = m_previousCapturedEyeFrames[1].texture;
                    flowCurrSource[0] = m_capturedEyeFrames[0].texture;
                    flowCurrSource[1] = m_capturedEyeFrames[1].texture;
                    flowPrev[0] = m_previousCapturedEyeFrames[0].opticalFlowTexture;
                    flowPrev[1] = m_previousCapturedEyeFrames[1].opticalFlowTexture;
                    flowCurr[0] = m_capturedEyeFrames[0].opticalFlowTexture;
                    flowCurr[1] = m_capturedEyeFrames[1].opticalFlowTexture;
                    for (int eye = 0; eye < 2; ++eye) {
                        if (m_previousCapturedEyeFrames[eye].depthTexture &&
                            m_previousCapturedEyeFrames[eye].depthSerial == m_previousCapturedEyeFrames[eye].serial) {
                            flowPrevDepth[eye] = m_previousCapturedEyeFrames[eye].depthTexture;
                        }
                        if (m_capturedEyeFrames[eye].depthTexture &&
                            m_capturedEyeFrames[eye].depthSerial == m_capturedEyeFrames[eye].serial) {
                            flowCurrDepth[eye] = m_capturedEyeFrames[eye].depthTexture;
                        }
                        flowPrevSource[eye]->AddRef();
                        flowCurrSource[eye]->AddRef();
                        flowPrev[eye]->AddRef();
                        flowCurr[eye]->AddRef();
                        if (flowPrevDepth[eye]) flowPrevDepth[eye]->AddRef();
                        if (flowCurrDepth[eye]) flowCurrDepth[eye]->AddRef();
                    }
                }
                if ((presentPairId % 300) == 0) {
                    Log("OpenXRManager: AER complete pair promoted. pair=%llu left=%llu right=%llu historyL=%llu historyR=%llu\n",
                        static_cast<unsigned long long>(presentPairId),
                        static_cast<unsigned long long>(m_capturedEyeFrames[0].serial),
                        static_cast<unsigned long long>(m_capturedEyeFrames[1].serial),
                        static_cast<unsigned long long>(m_previousCapturedEyeFrames[0].serial),
                        static_cast<unsigned long long>(m_previousCapturedEyeFrames[1].serial));
                }
            }
        }
    if (runAERV2Flow && m_opticalFlow) {
        if (!m_opticalFlow->EnsureInitialized(m_d3dDevice,
                m_capturedEyeFrames[0].width,
                m_capturedEyeFrames[0].height,
                static_cast<DXGI_FORMAT>(m_capturedEyeFrames[0].format))) {
            Log("OpenXRManager: AER V2 optical-flow init failed at flow stage. pair=%llu\n",
                static_cast<unsigned long long>(flowPairId));
            runAERV2Flow = false;
        }
    }
    if (!runAERV2Flow || !m_opticalFlow) {
        if (presentLock.owns_lock()) {
            presentLock.unlock();
        }
    }

    // Hand off Convert+Execute+Synth to the background worker so OnPresent does
    // NOT CPU-block on ~3.5ms/frame of GPU sync. Source refs are already addref'd
    // above; ownership is transferred into the job (worker releases on completion).
    // Submit thread reads m_interpolatedPairId atomically — if the worker has
    // published for the current pair, V2 frames are used; otherwise we fall back
    // to raw current eye. Single-slot queue: if a fresh pair lands while worker
    // is busy, the older queued (but not yet picked) job is replaced (refs
    // released) so we never grow latency past one synth interval.
    if (presentLock.owns_lock()) {
        presentLock.unlock();
    }
    if (runAERV2Flow && m_opticalFlow) {
        auto job = std::make_unique<AERV2Job>();
        job->pairId = flowPairId;
        // Pair-counter id for the in-between submit cadence (see AERV2Job).
        job->submitPairId = presentPairId;
        if (GetAERV2Enabled() != 0) {
            job->renderedEye = presentEye;
            job->syntheticEye = presentEye ^ 1;
        }
        for (int eye = 0; eye < 2; ++eye) {
            job->flowPrevSource[eye] = flowPrevSource[eye];
            job->flowCurrSource[eye] = flowCurrSource[eye];
            job->flowPrev[eye]       = flowPrev[eye];
            job->flowCurr[eye]       = flowCurr[eye];
            job->prevDepth[eye]      = flowPrevDepth[eye];
            job->currDepth[eye]      = flowCurrDepth[eye];
            job->prevPose[eye] = m_previousCapturedEyeFrames[eye].pose;
            job->currPose[eye] = m_capturedEyeFrames[eye].pose;
            job->fov[eye]      = m_capturedEyeFrames[eye].fov;
            job->hasView[eye]  =
                m_previousCapturedEyeFrames[eye].hasView &&
                m_capturedEyeFrames[eye].hasView;
            // Cleared so the cleanup-fallback below sees only owned-by-job pointers.
            flowPrevSource[eye] = nullptr;
            flowCurrSource[eye] = nullptr;
            flowPrev[eye]       = nullptr;
            flowCurr[eye]       = nullptr;
            flowPrevDepth[eye]  = nullptr;
            flowCurrDepth[eye]  = nullptr;
        }
        // Attach engine-side ground truth: motion vectors + depth captured via
        // Streamline slSetTag hook. These are AddRef'd here and Released by the
        // worker on completion (see ReleaseAERV2JobRefs). They may be null on
        // very early frames before the NGX hook fires; worker handles that.
        job->engineMv = NgxAcquireMotionVectors();
        {
            std::lock_guard<std::mutex> lock(m_presentMutex);
            if (m_depthSnapshot && m_depthSnapshotSerial != 0) {
                m_depthSnapshot->AddRef();
                job->engineDepth = m_depthSnapshot;
            }
        }

        if (!job->engineDepth) {
            job->engineDepth = NgxAcquireDepth();
        }
        // Record each captured eye's serial so the worker can identify the
        // older eye of the pair (lower serial = captured earlier = candidate
        // for MV-warp forward extrapolation).
        job->currSerial[0] = m_capturedEyeFrames[0].serial;
        job->currSerial[1] = m_capturedEyeFrames[1].serial;
        if (GetAERV2Enabled() != 0) {
            ProcessAERV2Job(std::move(job));
        } else {
            StartAERV2WorkerIfNeeded();
            std::unique_ptr<AERV2Job> replacedJob;
            {
                std::lock_guard<std::mutex> lock(m_aerV2JobMutex);
                if (m_aerV2PendingJob) {
                    replacedJob = std::move(m_aerV2PendingJob);
                }
                m_aerV2PendingJob = std::move(job);
            }
            m_aerV2JobCv.notify_one();
            if (replacedJob) {
                ReleaseAERV2JobRefs(*replacedJob);
                if ((flowPairId % 300) == 0) {
                    Log("OpenXRManager: AER V2 worker behind, replaced stale queued pair=%llu with %llu\n",
                        static_cast<unsigned long long>(replacedJob->pairId),
                        static_cast<unsigned long long>(flowPairId));
                }
            }
        }
    }

    // Any refs we addref'd but couldn't transfer (e.g. runAERV2Flow false after
    // the addref above, or m_opticalFlow null) get released here. After the job
    // was built, these are already nulled — Release on null is a no-op.
    for (int eye = 0; eye < 2; ++eye) {
        if (flowPrevSource[eye]) flowPrevSource[eye]->Release();
        if (flowCurrSource[eye]) flowCurrSource[eye]->Release();
        if (flowPrev[eye]) flowPrev[eye]->Release();
        if (flowCurr[eye]) flowCurr[eye]->Release();
        if (flowPrevDepth[eye]) flowPrevDepth[eye]->Release();
        if (flowCurrDepth[eye]) flowCurrDepth[eye]->Release();
    }

    // Idea #2 — synced-pose freshness (per-present low-pass nudge). syncSequential
    // freezes the head pose per pair for coherent stereo, but that lags on head
    // turns. Each present we nudge the synced pose toward the LIVE pose by
    // g_poseBlend (0=frozen, 1=live). The pair-boundary full snap below still
    // runs as the hard reset; this just closes the lag gap between snaps.
    // Both eyes still read the SAME synced pose -> stereo stays coherent; only
    // the lag shrinks. Skipped until the first pair has latched (valid) and when
    // the blend is 0 (no-op).
    if (syncSequential && !aerWarmupFrame) {
        const float pb = GetPoseBlend();
        if (pb > 0.0f && m_syncedPoseValid.load(std::memory_order_relaxed)) {
            const float keep = 1.0f - pb;
            m_syncedPosX.store(m_syncedPosX.load(std::memory_order_relaxed) * keep +
                               m_posX.load(std::memory_order_relaxed) * pb, std::memory_order_relaxed);
            m_syncedPosY.store(m_syncedPosY.load(std::memory_order_relaxed) * keep +
                               m_posY.load(std::memory_order_relaxed) * pb, std::memory_order_relaxed);
            m_syncedPosZ.store(m_syncedPosZ.load(std::memory_order_relaxed) * keep +
                               m_posZ.load(std::memory_order_relaxed) * pb, std::memory_order_relaxed);
            // Orientation: component lerp + renormalize (valid for small per-present deltas).
            float ox = m_syncedOriX.load(std::memory_order_relaxed) * keep + m_oriX.load(std::memory_order_relaxed) * pb;
            float oy = m_syncedOriY.load(std::memory_order_relaxed) * keep + m_oriY.load(std::memory_order_relaxed) * pb;
            float oz = m_syncedOriZ.load(std::memory_order_relaxed) * keep + m_oriZ.load(std::memory_order_relaxed) * pb;
            float ow = m_syncedOriW.load(std::memory_order_relaxed) * keep + m_oriW.load(std::memory_order_relaxed) * pb;
            const float on = std::sqrt(ox*ox + oy*oy + oz*oz + ow*ow);
            if (on > 1e-9f) { const float inv = 1.0f / on;
                ox *= inv; oy *= inv; oz *= inv; ow *= inv; }
            m_syncedOriX.store(ox, std::memory_order_relaxed);
            m_syncedOriY.store(oy, std::memory_order_relaxed);
            m_syncedOriZ.store(oz, std::memory_order_relaxed);
            m_syncedOriW.store(ow, std::memory_order_relaxed);
        }
    }

    if (syncSequential && presentEye == 1 && !aerWarmupFrame) {
        latchSyncedSequentialPair();
    }

    if (aerEnabled) {
        if (aerWarmupFrame) {
            --m_aerWarmupRemaining;
        } else {
            // Advance the scheduled eye by a CLEAN per-present toggle. The old code
            // scheduled from the DETECTED rendered eye (presentEye = GetRenderedCameraEye),
            // but that detection lags the schedule by ~1 present, so the feedback
            // x[n+1] = detected(x[n]) ^ 1  ==  x[n-1] ^ 1 gives a stable PERIOD-4
            // pattern (L,L,R,R -> eyeHist 0x33/0x66/0x99) instead of alternating
            // every frame. The camera reads m_renderEyeIndex, so the flat mirror
            // barely alternated. Toggling from the value used THIS present
            // (scheduledEye) yields a clean period-2 (L,R,L,R) regardless of init;
            // capture keeps using presentEye (the 1-frame-lagged detection), which
            // then correctly labels the backbuffer that render latency produces.
            m_renderEyeIndex.store(scheduledEye ^ 1, std::memory_order_relaxed);
        }
    }

    if (backBuffer) {
        backBuffer->Release();
        backBuffer = nullptr;
    }

    // [HMD-Paced Frame Sync] Lock the game engine to the OpenXR compositor rate.
    // By waiting for the compositor to finish xrEndFrame, we ensure the game
    // never runs ahead, and the next GetHeadPose will have the absolutely
    // fresh predicted display time for the subsequent frame.
    //
    // IMPORTANT: this wait is MONO-ONLY. An earlier attempt extended it to AER
    // V2 (mode-6) on the theory that pacing captures to submits would help, but
    // it introduced a feedback loop with the alternating-eye captures: the game
    // thread waking point drifted vsync-to-vsync, so the rendered eye's pose age
    // at display time varied -> variable ATW delta -> whole-image jitter in BOTH
    // eyes on head turns. Mono is immune (single eye, submits at a consistent
    // pose baseline). AER V2 instead lets the engine free-run and relies on the
    // per-eye mode-6 warp + runtime ATW for smoothness. See
    // docs/aer-v2-mode6-corrected.md.
    // Inline submit runs the XR frame loop directly from the Present hook, so there
    // is no separate frame thread to wait for here.

    if ((s_presentCount % 300) != 1) return;

    Log("OpenXRManager: Present observed. hwnd=%p size=%ux%u format=%u backbufferIndex=%u resourceWidth=%llu resourceHeight=%u sessionRunning=%d aer=%d sync=%d eye=%d warmup=%d pair=%llu\n",
        desc.OutputWindow,
        desc.BufferDesc.Width,
        desc.BufferDesc.Height,
        static_cast<unsigned>(desc.BufferDesc.Format),
        backBufferIndex,
        static_cast<unsigned long long>(resourceDesc.Width),
        resourceDesc.Height,
        IsSessionRunning() ? 1 : 0,
        aerEnabled ? 1 : 0,
        syncSequential ? 1 : 0,
        presentEye,
        aerWarmupFrame ? 1 : 0,
        static_cast<unsigned long long>(presentPairId));
}
