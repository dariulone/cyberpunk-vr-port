#pragma once

#include <windows.h>
#include <d3d12.h>
#include <wrl.h>
#include <atomic>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>

#include "depth_resolve.h"
#include "sharpen_pass.h"
#include "color_blit.h"

struct IDXGISwapChain;

struct OpenXRHeadPose {
    float posX;
    float posY;
    float posZ;
    float oriX;
    float oriY;
    float oriZ;
    float oriW;
    bool valid;
};

// Aggregated VR-controller snapshot, queried each frame from the XInput hook
// to merge into the game's XINPUT_GAMEPAD state. Button bits intentionally
// mirror XInput's XINPUT_GAMEPAD_* constants so we can OR them directly.
struct VRControllerState {
    uint16_t buttons = 0;
    float    leftTrigger  = 0.0f;
    float    rightTrigger = 0.0f;
    float    leftThumbX   = 0.0f;
    float    leftThumbY   = 0.0f;
    float    rightThumbX  = 0.0f;
    float    rightThumbY  = 0.0f;
    float    leftGrip     = 0.0f;
    float    rightGrip    = 0.0f;
    bool     leftHandValid  = false;
    bool     rightHandValid = false;
};

extern "C" int GetXrRuntimeMode();
extern "C" float GetWeaponPitch();
extern "C" float GetWeaponYaw();
extern "C" float GetWeaponRoll();
extern "C" float GetWeaponOffsetX();
extern "C" float GetWeaponOffsetY();
extern "C" float GetWeaponOffsetZ();

// 1 (default) = mono submits from the XR thread on every runtime, so the headset gets one
// submission per DISPLAY frame instead of one per game frame. 0 = the old rule (threaded
// only on SteamVR). See UseThreadedSubmit below.
extern "C" __declspec(dllexport) int CyberpunkVR_ThreadedMonoSubmit;

class OpenXRManager {
public:
    static OpenXRManager& Get();

    bool Init();
    void Shutdown();

    // D3D12 specific initialization
    bool InitGraphics(ID3D12Device* device, ID3D12CommandQueue* queue);
    bool GetHeadPose(OpenXRHeadPose* out) const;
    // Fresh xrLocateSpace at a given display time -- see the definition for why the camera write
    // must not read the cached atomics above.
    bool LocateHeadPoseAt(XrTime displayTime, OpenXRHeadPose* out);
    void RequestRecenter();
    void OnPresent(IDXGISwapChain* swapChain);
    // Run one XR frame inline on the Present thread instead of a dedicated
    // frame thread.
    void PumpInlineFrame();
    void SetMonoSubmitEnabled(bool enabled);
    // True when the dedicated submit thread (NOT the Present thread) should own the
    // XR frame loop. SteamVR's xrWaitFrame / compositor pacing stalls the game's Present
    // thread when the loop is driven inline (freezes/lags); a dedicated thread decouples
    // it. VDXR and other runtimes keep the proven inline mono path on the Present thread.
    // Submit on the XR thread for mono on EVERY runtime, not just SteamVR.
    //
    // Driving the submit from Present means exactly one xrEndFrame per GAME frame. While the
    // game keeps up with the headset that is fine -- every display cycle gets a fresh frame.
    // Below it, display cycles go by with no submission at all, and a runtime with nothing
    // new simply repeats what it has: no re-projection, no timewarp against the newer head
    // pose. That is the hold-then-jump, and its size grows exactly as the game rate falls
    // under the display rate -- invisible above ~80 fps, obvious at 50-60 on 90 Hz. It also
    // explains why SpaceWarp changes nothing: the runtime is not short of GPU time, it is
    // short of submissions.
    //
    // The XR thread instead runs one cycle per display frame and re-submits the latest
    // snapshot WITH ITS OWN pose (see CyberpunkVR_XrPaceByRuntime), so every display cycle
    // gets a layer the runtime can timewarp against the current head pose. That is what the
    // established mods get for free by rendering inside the XR frame; we cannot do that from
    // outside the engine, so we decouple the submit instead.
    bool UseThreadedSubmit() const {
        return (m_runtimeIsSteamVR.load(std::memory_order_relaxed) ||
                CyberpunkVR_ThreadedMonoSubmit != 0) &&
               m_monoSubmitEnabled.load(std::memory_order_relaxed);
    }
    int GetCurrentRenderEyeIndex() const { return m_renderEyeIndex.load(std::memory_order_relaxed); }
    // Record the exact OpenXR head pose a given eye's frame was rendered with
    // Frame logic
    void StoreRenderEyePose(int eye, const OpenXRHeadPose& pose, uint32_t seq);

    // THE pose the game camera was driven with for the frame currently being built.
    //
    // Everything else here reads the pose CACHE (m_ori*/m_pos*), which the XR thread keeps
    // refreshing. The camera injection samples that cache while the engine builds a frame;
    // by the time the finished frame reaches Present the cache has moved on, so attaching
    // the cache there labels the image with a NEWER pose than it was rendered from. The
    // compositor then places it ahead and the next frame catches up -- lag-and-snap whose
    // size is one frame period, which is why it is invisible above ~80 fps and obvious at
    // 50-60, and why SpaceWarp cannot help: the pose is wrong, not missing.
    //
    // So the injection hands its exact sample over here, and Present attaches THIS to the
    // snapshot. Direct hand-off, no sequence queue that can silently miss.
    // A RING, not a single slot, because the engine renders ahead: while frame N is being
    // presented the CPU is already building N+1, so the newest injected pose may belong to a
    // frame that is not on screen yet. Handing that one to the compositor makes it over-warp,
    // and the error refreshes with every new game frame -- judder that grows as the frame
    // period does. How many frames deep the pipeline runs is not something to reason about,
    // so CyberpunkVR_PoseFrameLag selects how far back to look and the right value is found
    // by trying 0/1/2 live.
    // Each entry is stamped with the PRESENT INTERVAL it was injected in, not with a push
    // index. Counting pushes backwards is fragile: the camera injection runs 1.01-1.07 times
    // per present (measured), so every hundred frames or so an interval contains two pushes,
    // the count slips, and one frame gets a pose from the wrong interval -- a single visible
    // jolt roughly every couple of hundred frames. Selecting by interval collapses double
    // injections correctly: the frame was rendered with the LAST camera write of its
    // interval, which is exactly what "newest entry stamped <= N-1-lag" returns.
    // Take the scene depth AT THE BARRIER that makes it shader-readable, inline on the
    // engine's own command list.
    //
    // Capturing at Present was a guess about timing: by then the engine has usually put its
    // depth back into DEPTH_WRITE (measured: state 0x10 on a large share of frames with the
    // VRCAM component on), so the snapshot silently did not happen and the depth layer
    // flickered in and out. The engine transitions the buffer to shader-readable itself for
    // its own post passes, and that is the one moment when reading it is legal.
    //
    // Only a COPY is injected into the engine's list. A copy does not disturb pipeline state,
    // so the engine's recording is unaffected; a resolve DRAW would set root signature, PSO
    // and heaps, and D3D12 offers no way to restore what was there. The copy lands in our own
    // staging texture, and the format resolve then runs later on OUR list, on OUR resource,
    // at a time we control.
    void CaptureSceneDepthInline(ID3D12GraphicsCommandList* list, ID3D12Resource* gameDepth,
                                 unsigned int stateAfter);

    // THE pose of a specific frame, bound the same way its depth is.
    //
    // Everything before this guessed how far the engine renders ahead -- a lag counted in
    // presents, tuned by hand. RealVR never guesses: the pose rides in the queue slot next to
    // the image, keyed by the frame's own index, so the two cannot come apart. We have the
    // same anchor available: the barrier that makes the frame's depth readable runs INSIDE
    // that frame, on the render thread, and the depth stage taken there is already stamped
    // with the present that will show it -- and the depth matches perfectly as a result.
    // Recording the head pose at that same instant gives the image, its depth and its pose
    // one shared identity instead of three timelines.
    // Same mutex the publication takes (PushRenderHeadPose) -- it moved off the depth path.
    bool GetFramePoseForSerial(uint64_t serial, OpenXRHeadPose* out) {
        std::lock_guard<std::mutex> lock(m_pendingRenderPoseMutex);
        if (!out || m_framePoseSerial == 0 || m_framePoseSerial != serial) return false;
        *out = m_framePose;
        return out->valid;
    }

    // The recenter base, published so the SUBMIT path can put a rendered pose back into the
    // space the composition layers actually live in.
    //
    // GetHeadPose() -- the pose the camera injection renders from -- is expressed RELATIVE to
    // this base: relOri = conj(base.ori) * raw, relPos = conj(base.ori) * (raw - base.pos).
    // The projection layer, on the other hand, is submitted with space = m_localSpace, and
    // xrLocateViews fills m_views in that same local space. Handing the layer a
    // recenter-relative pose is therefore a space error the size of the recenter -- invisible
    // while the base is identity (a fresh LOCAL space usually is), and a rotated world the
    // moment the user recenters facing anywhere else.
    void GetRecenterBase(XrPosef* out) const {
        if (!out) return;
        out->orientation.x = m_baseOriX.load(std::memory_order_relaxed);
        out->orientation.y = m_baseOriY.load(std::memory_order_relaxed);
        out->orientation.z = m_baseOriZ.load(std::memory_order_relaxed);
        out->orientation.w = m_baseOriW.load(std::memory_order_relaxed);
        out->position.x = m_basePosX.load(std::memory_order_relaxed);
        out->position.y = m_basePosY.load(std::memory_order_relaxed);
        out->position.z = m_basePosZ.load(std::memory_order_relaxed);
    }
    void PublishRecenterBase(const XrPosef& base) {
        m_baseOriX.store(base.orientation.x, std::memory_order_relaxed);
        m_baseOriY.store(base.orientation.y, std::memory_order_relaxed);
        m_baseOriZ.store(base.orientation.z, std::memory_order_relaxed);
        m_baseOriW.store(base.orientation.w, std::memory_order_relaxed);
        m_basePosX.store(base.position.x, std::memory_order_relaxed);
        m_basePosY.store(base.position.y, std::memory_order_relaxed);
        m_basePosZ.store(base.position.z, std::memory_order_relaxed);
    }

    // The present interval we are currently inside: the serial of the last COMPLETED present.
    // A frame built now will be shown by present (this + 1). The camera writer uses it to
    // compose exactly once per interval and to stamp the pose it hands over.
    uint64_t GetPresentCount() const { return m_presentCount.load(std::memory_order_relaxed); }

    // ---- ONE IDENTITY PER FRAME ---------------------------------------------------------------
    //
    // Everything a frame needs in order to be submitted correctly, captured TOGETHER at locate
    // time and keyed by the serial of the present that will show it.
    //
    // The submit path used to assemble its layer out of two timelines: the head orientation from
    // the frame that was rendered, but the head POSITION, the per-eye offsets and the per-eye FOV
    // from `m_views` as of Present. That mixture is what the compositor cannot reconcile -- it
    // re-projects away a translation that is already in the pixels, one frame late, every frame.
    //
    // Both reference mods solve it the same way and it is worth naming: UEVR keeps
    // `PipelineState[6]` indexed by the engine's frame_count, holding frame_state +
    // view_space_location + stage_views, and its end_frame reads pose, fov AND displayTime out of
    // that one slot. RealVR keeps a 4-slot pose ring in which every slot stores its own frame
    // index and refuses to use a slot whose index does not match ("Rendering pose entry is
    // invalid"). Same shape, same reason. This is that structure for us.
    struct XrFrameSlot {
        uint64_t serial = 0;          // the present that will show the frame built with this
        XrTime   displayTime = 0;     // what the locate was aimed at
        XrPosef  viewPose[2]{};       // per-eye pose, LOCAL space, as located for this frame
        XrFovf   viewFov[2]{};        // per-eye fov, as located for this frame
        XrPosef  headPoseLocal{};     // view-space origin in LOCAL space, same instant
        bool     valid = false;
    };

    void PublishFrameSlot(uint64_t serial, XrTime displayTime,
                          const XrView* views, uint32_t viewCount,
                          const XrPosef& headLocal) {
        if (!views || viewCount < 2) return;
        std::lock_guard<std::mutex> lock(m_frameSlotMutex);
        XrFrameSlot& s = m_frameSlots[serial % kFrameSlots];
        s.serial = serial;
        s.displayTime = displayTime;
        for (int eye = 0; eye < 2; ++eye) {
            s.viewPose[eye] = views[eye].pose;
            s.viewFov[eye] = views[eye].fov;
        }
        s.headPoseLocal = headLocal;
        s.valid = true;
    }

    // Exact slot if it exists; otherwise the NEWEST slot older than the one asked for.
    //
    // Measured: the game presents faster than the XR loop cycles (18469 lookups against 16311
    // cycles), so some present intervals contain no locate at all and never get a slot of their
    // own. That is not an error and it must not fall back to "the current values": a frame built
    // in an interval with no new locate was rendered with the LAST pose located, so the newest
    // older slot is precisely the right answer -- it carries the pose, the eye offsets and the
    // FOV that the image actually holds.
    //
    // RealVR has exactly this branch: when its render counter outruns the pose high-water mark it
    // reuses the last sampled pose from the ring rather than fetching a fresh one. `exact` is
    // reported back so the two cases stay separately countable; a slot NEWER than the request is
    // never acceptable, since that pose is not in the pixels yet.
    bool GetFrameSlot(uint64_t serial, XrFrameSlot* out, bool* exact = nullptr) const {
        if (!out) return false;
        std::lock_guard<std::mutex> lock(m_frameSlotMutex);
        const XrFrameSlot& s = m_frameSlots[serial % kFrameSlots];
        if (s.valid && s.serial == serial) {
            *out = s;
            if (exact) *exact = true;
            return true;
        }
        const XrFrameSlot* best = nullptr;
        for (uint32_t i = 0; i < kFrameSlots; ++i) {
            const XrFrameSlot& c = m_frameSlots[i];
            if (!c.valid || c.serial > serial) continue;
            if (!best || c.serial > best->serial) best = &c;
        }
        if (!best) return false;
        *out = *best;
        if (exact) *exact = false;
        return true;
    }

    // ---- WHEN WILL THE FRAME BEING BUILT ACTUALLY BE SHOWN -------------------------------------
    //
    // The XR loop runs on its own thread, paced by xrWaitFrame at the headset's rate; the game
    // presents at whatever rate it manages. There is therefore NO fixed offset between "the cycle
    // that located this pose" and "the cycle that will display the frame built from it" -- the two
    // rates beat against each other continuously, and that beat is what a fixed +N-period fudge
    // cannot follow.
    //
    // RealVR measures the relationship instead of assuming it: sub_18030D480, called every frame
    // from vr_render_frame_step, is a rolling 100-sample least-squares fit of
    // displayTime = k*frameIndex + b (accumulating means, Sxx and Sxy, double-buffered so the
    // window restarts without a discontinuity), and its slope/intercept become the `period`/`base`
    // that its pose fetch predicts with. This is the same fit.
    //
    // Conditioning matters here: serials reach six figures and XrTime is ~1e18 ns, so the window
    // is re-anchored on every restart and the published line is kept as (anchor point + slope)
    // rather than a raw intercept. Fitting on raw magnitudes cancels catastrophically and yields
    // a slope of noise.
    void FitAddDisplayTime(uint64_t serial, XrTime displayTime) {
        std::lock_guard<std::mutex> lock(m_fitMutex);
        if (!m_fitAnchored) { m_fitWx0 = serial; m_fitWy0 = displayTime; m_fitAnchored = true; }
        const double x = static_cast<double>(static_cast<int64_t>(serial - m_fitWx0));
        const double y = static_cast<double>(displayTime - m_fitWy0);
        m_fitN += 1.0; m_fitSx += x; m_fitSy += y; m_fitSxx += x * x; m_fitSxy += x * y;
        if (m_fitN >= 8.0) {
            const double den = m_fitN * m_fitSxx - m_fitSx * m_fitSx;
            if (den > 1e-6) {
                const double k = (m_fitN * m_fitSxy - m_fitSx * m_fitSy) / den;
                const double b = (m_fitSy - k * m_fitSx) / m_fitN;
                // Sanity: the slope is "nanoseconds of display time per present". Anything outside
                // 1..200 ms per frame is a broken sample set (a load screen, a pause, a serial
                // jump), not a frame rate -- keep the previous line rather than adopt nonsense.
                if (k > 1.0e6 && k < 2.0e8) {
                    m_fitK = k;
                    m_fitAx = m_fitWx0;
                    m_fitAy = m_fitWy0 + static_cast<int64_t>(b);
                    m_fitValid = true;
                }
            }
        }
        if (m_fitN >= 240.0) {   // restart the window so the fit tracks drift, keep the last line
            m_fitAnchored = false;
            m_fitN = m_fitSx = m_fitSy = m_fitSxx = m_fitSxy = 0.0;
        }
    }

    bool FitPredictDisplayTime(uint64_t serial, XrTime* out) const {
        if (!out) return false;
        std::lock_guard<std::mutex> lock(m_fitMutex);
        if (!m_fitValid) return false;
        const double dx = static_cast<double>(static_cast<int64_t>(serial - m_fitAx));
        *out = m_fitAy + static_cast<XrTime>(m_fitK * dx);
        return true;
    }

    double FitSlopeNs() const {
        std::lock_guard<std::mutex> lock(m_fitMutex);
        return m_fitValid ? m_fitK : 0.0;
    }

    // THE ONE INSTANT THIS FRAME IS AIMED AT.
    //
    // Published by the frame loop the moment it decides where to locate, and read by the camera
    // write so both name the SAME time. Calling xrLocateSpace twice for one target time is not a
    // problem -- the later call simply has fresher tracking data for the same instant -- but
    // computing two different target times is, because then the camera and the layer describe
    // two different moments and the compositor reconciles them by moving the world.
    // Bumping the epoch here, and not anywhere else, is what makes the sample below
    // unambiguous: a new aim time IS a new frame as far as the injection is concerned.
    void SetFrameAimTime(XrTime t) {
        m_frameAimTime.store(t, std::memory_order_release);
        m_frameAimEpoch.fetch_add(1, std::memory_order_acq_rel);
    }
    XrTime   GetFrameAimTime() const { return m_frameAimTime.load(std::memory_order_acquire); }
    uint64_t GetFrameAimEpoch() const { return m_frameAimEpoch.load(std::memory_order_acquire); }

    // ---- ONE HEAD SAMPLE PER FRAME, SHARED BY EVERYTHING THAT RENDERS IT ---------------------
    //
    // This exists because the view was being built from TWO different samples of the head, and
    // labelled with a THIRD.
    //
    //   position    -- LocateCamera took it from GetHeadPose(), i.e. the m_pos*/m_ori* cache,
    //                  which the frame loop refreshes once per cycle AND runs through
    //                  filterTrackingPose() with xr_hmd_smooth (0.35 in the shipped ini).
    //   orientation -- PatchCamera took a fresh, UNFILTERED xrLocateSpace at the aim time.
    //   the label   -- the submit took the frame slot's own xrLocateSpace, a third call again.
    //
    // The filter is the damaging half. adaptiveFollow with strength 0.35 follows at 1/8 per
    // cycle while the head is nearly still and at ~1.0 once it moves past ~2 deg/frame, so the
    // effective lag on the head POSITION slides between roughly 130 ms and zero as a function of
    // how fast you are moving -- and it crosses over right in the middle of ordinary head
    // motion. The eye is therefore placed at a lagging position while looking in the current
    // direction, and the layer is labelled with neither. A steady lag reprojects away perfectly;
    // a lag that breathes with your own movement cannot be reprojected away by anything, and
    // that is what is left of the judder.
    //
    // The OpenXR guide puts the rule plainly -- "rendering with one pose but submitting with a
    // different pose causes visual artifacts and motion sickness" -- and the frame-timing
    // guidance adds that every stage must use the same display time for one application frame.
    // One sample, taken once per aim epoch, is how both are satisfied at once.
    //
    // Whoever asks first performs the locate; everyone else in the same epoch gets that exact
    // struct back. A benign double-locate on a race is fine: the first store wins and both
    // callers return the stored one.
    bool AcquireFrameHeadSample(OpenXRHeadPose* out) {
        if (!out) return false;
        const uint64_t epoch = m_frameAimEpoch.load(std::memory_order_acquire);
        {
            std::lock_guard<std::mutex> lock(m_frameSampleMutex);
            if (m_frameSampleEpoch == epoch && m_frameSample.valid) {
                *out = m_frameSample;
                return true;
            }
        }
        OpenXRHeadPose p{};
        bool ok = false;
        const XrTime aim = m_frameAimTime.load(std::memory_order_acquire);
        if (aim > 0) ok = LocateHeadPoseAt(aim, &p) && p.valid;
        if (!ok) ok = GetHeadPose(&p) && p.valid;   // pre-session / locate failure
        if (!ok) return false;
        {
            std::lock_guard<std::mutex> lock(m_frameSampleMutex);
            if (m_frameSampleEpoch != epoch || !m_frameSample.valid) {
                m_frameSample = p;
                m_frameSampleEpoch = epoch;
            }
            *out = m_frameSample;
        }
        return out->valid;
    }

    // ---- THE POSE THAT IS ACTUALLY IN THE FRAME, READ BACK FROM THE ENGINE -------------------
    //
    // Everything above still contains one guess: that the frame arriving at present N was built
    // with the camera we wrote during interval N-1. It is a guess because the engine's simulation
    // and its render graph run on different threads, and our composition installs a new pose only
    // once per aim epoch -- so the render snapshot lands before or after that install depending on
    // sub-frame phase, and the sequence of poses actually rendered comes out as
    // P1 P2 P2 P3 P5 P5 P6: occasional repeats and skips against a smoothly moving head. Each one
    // is a frame labelled with a pose it was not rendered from, which is judder, and it is driven
    // by phase noise so it never settles no matter how the prediction is tuned.
    //
    // A repeat is NOT itself a problem: the compositor reprojects each frame to its own display
    // time, so a repeated viewpoint still looks smooth -- provided the label says so. The whole
    // error is in the labelling.
    //
    // So stop guessing and read it back. CRenderNode_PrepareSceneRendering is the third node of
    // the FIRST stage of the frame graph and the graph's tail is CRenderNode_Present, so a hit
    // there belongs to a frame that has not been presented yet, and hits and presents are 1:1.
    // The hook reads the camera quaternion the engine is about to render with, matches it against
    // the ring of quaternions we wrote, and pushes the XR sample that produced it here. Present
    // pops the oldest. Exact pairing, no pipeline-depth constant anywhere.
    void PushRenderedFramePose(const OpenXRHeadPose& pose) {
        std::lock_guard<std::mutex> lock(m_renderedFrameMutex);
        m_renderedFrameQ[m_renderedFrameTail & (kRenderedFrameQ - 1)] = pose;
        ++m_renderedFrameTail;
        // Bounded: if the engine ever runs a graph without presenting it, drop the oldest rather
        // than let the queue drift permanently out of step.
        if (m_renderedFrameTail - m_renderedFrameHead > 4) {
            m_renderedFrameHead = m_renderedFrameTail - 4;
        }
    }
    bool PopRenderedFramePose(OpenXRHeadPose* out) {
        if (!out) return false;
        std::lock_guard<std::mutex> lock(m_renderedFrameMutex);
        if (m_renderedFrameHead == m_renderedFrameTail) return false;
        *out = m_renderedFrameQ[m_renderedFrameHead & (kRenderedFrameQ - 1)];
        ++m_renderedFrameHead;
        return out->valid;
    }
    uint32_t RenderedFrameQueueDepth() const {
        std::lock_guard<std::mutex> lock(m_renderedFrameMutex);
        return static_cast<uint32_t>(m_renderedFrameTail - m_renderedFrameHead);
    }

    void PushRenderHeadPose(const OpenXRHeadPose& pose) {
        std::lock_guard<std::mutex> lock(m_pendingRenderPoseMutex);
        const uint32_t slot = static_cast<uint32_t>(m_renderPoseRingHead & 15);
        const uint64_t present = m_presentCount.load(std::memory_order_relaxed);
        m_renderPoseRing[slot] = pose;
        m_renderPoseStamp[slot] = present;
        ++m_renderPoseRingHead;
        m_pendingRenderPoseValid = true;

        // Bind this pose to the frame being built, HERE.
        //
        // It used to be published as a side effect of the depth capture, which only runs when
        // the depth barrier fires for MAIN that frame. Measured, that missed roughly a
        // quarter of frames (PoseExact 23589 vs PoseEstimated 7078), and each miss fell back
        // to the interval-lag estimate -- a pose that does not belong to the image, i.e.
        // visible wobble on head turns. Whether depth was captured has nothing to do with
        // which pose the frame was rendered with, so the binding does not belong there.
        //
        // The camera injection runs inside the frame the NEXT present will show, so that is
        // the serial. It can run more than once per frame (measured 1.01-1.07x); the last
        // write before the present wins, which is precisely the pose the frame was built with.
        m_framePose = pose;
        m_framePoseSerial = pose.valid ? (present + 1) : 0;
    }
    // presentSerial = the serial of the present being processed. lag is in INTERVALS: the
    // engine renders ahead, so the image on screen was built an interval or more before the
    // present that shows it.
    bool GetRenderHeadPoseForPresent(uint64_t presentSerial, int lag, OpenXRHeadPose* out) {
        std::lock_guard<std::mutex> lock(m_pendingRenderPoseMutex);
        if (!m_pendingRenderPoseValid || !out) return false;
        if (lag < 0) lag = 0;
        if (lag > 8) lag = 8;
        const uint64_t want = static_cast<uint64_t>(lag) + 1;
        if (presentSerial < want) return false;
        const uint64_t cutoff = presentSerial - want;      // stamp must be <= cutoff
        const uint64_t n = m_renderPoseRingHead < 16 ? m_renderPoseRingHead : 16;
        for (uint64_t i = 1; i <= n; ++i) {                 // newest first
            const uint32_t slot = static_cast<uint32_t>((m_renderPoseRingHead - i) & 15);
            if (m_renderPoseStamp[slot] <= cutoff) {
                *out = m_renderPoseRing[slot];
                return out->valid;
            }
        }
        return false;
    }

    // Write hand positions/orientations + HMD orientation to shared memory.
    // Called from OnLocateCameraCallback (BEFORE render) to eliminate the 1-frame
    // hand lag that causes hand displacement artifacts.
    // POSE PAIR LOCKING (45 Hz skeleton). Snapshots the whole tracking state (head
    // pose, hands, HMD ori, physical height). Called from OnPresent at the PAIR
    // BOUNDARY (follower eye) so the snapshot is published BEFORE the next pair's
    // animation pass — the VRIK plugin reads shared memory during anim eval, which
    // runs before render/LocateCamera, so taking the snapshot in LocateCamera was
    // one stage too late and tore the skeleton across the pair.
    void UpdatePairLock();

    // Write the pair-locked hands + HMD orientation + body height ([0..19],[89],[90])
    // to shared memory. Reads the frozen snapshot, so both eyes of a pair feed VRIK
    // identical inputs. Called right after UpdatePairLock in OnPresent (early enough
    // that the next frame's animation reads it).
    void FlushHandsToShared();

    // Pair-locked head pose for the camera: returns the snapshot UpdatePairLock
    // froze for the current stereo pair (both eyes share ONE head pose), so the
    // engine stops re-sampling tracking ~11 ms apart per eye and VRIK stops
    // rebuilding a different skeleton per eye.
    bool GetPairLockedHeadPose(OpenXRHeadPose* out);

    // Hands
    bool GetHandPose(int handIndex, OpenXRHeadPose* out) const;
    void SetWeaponOffsets(float pitch, float yaw, float roll, float dx, float dy, float dz);

    // VR hand-tracking activation, driven from the in-headset overlay menu. The
    // value is published into shared-memory slot [32] each present; the RED4ext
    // plugin polls it, installs/arms, and sets g_VRBind to this value. Use 4 =
    // full-arm IK (same as the CET button); 0 = off. 1-3 are legacy fallbacks.
    void SetVRHandTrackingMode(int mode) { m_vrHandTrackingMode.store(mode, std::memory_order_relaxed); }
    int GetVRHandTrackingMode() const { return m_vrHandTrackingMode.load(std::memory_order_relaxed); }
    // "Bullet from weapon barrel" VR aim: published to shared[58] (enable), read by the RED4ext
    // plugin's GetOrientation VMT override.
    void SetWeaponAimEnable(int v) { m_weaponAimEnable.store(v, std::memory_order_relaxed); }
    int GetWeaponAimEnable() const { return m_weaponAimEnable.load(std::memory_order_relaxed); }
    // Weapon holster mode: 1 = immersive (visual-holster equip), 0 = simple slot mapping.
    // Published to shared[23], read by the CET Holster mod via GetVRSharedSlot(23).
    void SetImmersiveHolsters(int v) { m_immersiveHolsters.store(v, std::memory_order_relaxed); }
    int GetImmersiveHolsters() const { return m_immersiveHolsters.load(std::memory_order_relaxed); }
    // Read a shared-mem slot (plugin publishes the muzzle world forward to [24..26], valid [27]).
    // BOUND = 256: the mapping is 1024 bytes = 256 floats. The old `< 128` bound silently
    // no-opped BOTH sides of the weapon flag when it moved from [126] to [144] (the barrel
    // laser dot died that day: write dropped, read returned 0). Audit find (gpt-5.5) --
    // and the user called the <128 boundary from memory before the audit confirmed it.
    float GetSharedSlot(int i) const { float* p = m_sharedHandsPtr; return (p && i >= 0 && i < 256) ? p[i] : 0.0f; }
    // Write a shared-mem slot (XInput merge publishes the melee power-trigger flag to [30]).
    void SetSharedSlot(int i, float v) { float* p = m_sharedHandsPtr; if (p && i >= 0 && i < 256) p[i] = v; }

    // Publish IK calibration to the plugin (see m_calib). Order matches the [33..47] slots.
    void SetVRHandCalib(float scaleR, float scaleL, float heightR, float heightL,
                        float swingR, float swingL, float poleR, float poleL,
                        float wRp, float wRy, float wRr, float wLp, float wLy, float wLr) {
        float v[14] = { scaleR, scaleL, heightR, heightL, swingR, swingL, poleR, poleL,
                        wRp, wRy, wRr, wLp, wLy, wLr };
        for (int i = 0; i < 14; ++i) m_calib[i].store(v[i], std::memory_order_relaxed);
        m_calibValid.store(1, std::memory_order_relaxed);
    }
    // Read back the current 14-value calibration block. The overlay syncs its slider state
    // from this (the old one-way flow was the "calibration not saved" bug: static UI defaults
    // stomped the loaded/auto-calibrated values on the first slider touch).
    void GetVRHandCalib(float out[14]) const {
        for (int i = 0; i < 14; ++i) out[i] = m_calib[i].load(std::memory_order_relaxed);
    }

    // ==== HTC VIVE TRACKERS (leg tracking) ====
    // XR_HTCX_vive_tracker_interaction: enumerate connected trackers, bind the
    // body roles (left_foot / right_foot / waist) to a pose action, locate them
    // next to the hand locate each frame, and publish HMD-local poses to shared
    // [157..172] (feet) and [181..188] (waist) so the VRIK plugin can IK the
    // avatar legs onto the feet and place the hips on the waist tracker.
    // Works with real Vive Trackers and with trackers that emulate them.
    void InitViveTrackerSupport();       // from Init(), after xrCreateInstance
    void CreateViveTrackerActions();     // from the action-set setup block
    void EnsureViveTrackerSpaces();      // after session attach; re-tried from the poll
    // Caller holds m_handMutex (same rule as the m_hands writes around it).
    void PollViveTrackers(XrTime locateTime, const XrPosef& headPoseLocal);
    // ==== OPENVR FALLBACK TRACKER PROVIDER (Panda trackers etc.) ====
    // SteamVR's HTCX OpenXR extension only enumerates genuine HTC Vive
    // Trackers; emulated trackers never show up there. They ARE visible
    // through the OpenVR API as TrackedDeviceClass_GenericTracker devices, so
    // this provider reads their poses from a lightweight OpenVR background
    // client and fills any role the HTCX path left invalid. Device -> role
    // assignment is geometric (feet = the two lowest trackers, waist = the
    // nearest hip-height one), refreshed by the T-pose auto-calibration.
    void InitOpenVRTrackerSupport();            // from Init(), after InitViveTrackerSupport
    void PollOpenVRTrackers(const XrPosef& headPoseStage);  // from PollViveTrackers tail
    void RemapOpenVRTrackerRoles(bool explicitRescan);      // T-pose calib + lazy remap
    bool HasOpenVRTrackerSupport() const { return m_openvrSystem != nullptr; }
    // HMD-local tracker pose, same convention as GetHandPose.
    // idx: 0 = left foot, 1 = right foot, 2 = waist.
    bool GetBodyTrackerPose(int idx, OpenXRHeadPose* out) const;
    int  GetViveTrackerCount() const { return m_viveTrackerCount.load(std::memory_order_relaxed); }
    bool HasViveTrackerExtension() const { return m_viveTrackerExt; }
    void SetLegTrackersEnable(int v) { m_legTrackersEnable.store(v != 0 ? 1 : 0, std::memory_order_relaxed); }
    int  GetLegTrackersEnable() const { return m_legTrackersEnable.load(std::memory_order_relaxed); }
    void SetWaistTrackerEnable(int v) { m_waistTrackerEnable.store(v != 0 ? 1 : 0, std::memory_order_relaxed); }
    int  GetWaistTrackerEnable() const { return m_waistTrackerEnable.load(std::memory_order_relaxed); }
    // fix15: kick-damage checkbox (published to shared [140] next to [173]).
    void SetLegKickDamageEnable(int v) { m_legKickDamageEnable.store(v != 0 ? 1 : 0, std::memory_order_relaxed); }
    void SetLegTrackerTuning(float ankleOffsetM, float mountPitchDeg, float mountYawDeg, float mountRollDeg) {
        m_legAnkleOffset.store(ankleOffsetM, std::memory_order_relaxed);
        m_legMountEulerDeg[0].store(mountPitchDeg, std::memory_order_relaxed);
        m_legMountEulerDeg[1].store(mountYawDeg, std::memory_order_relaxed);
        m_legMountEulerDeg[2].store(mountRollDeg, std::memory_order_relaxed);
    }
    // fix12: per-foot MANUAL rotation trim (deg), F10 -> VRIK sliders. Stacked
    // on the calibrated mount at publish time (world space: yaw about up,
    // pitch about body right, roll about body forward). foot: 0 = L, 1 = R.
    void SetLegFootAdjDeg(int foot, float yawDeg, float pitchDeg, float rollDeg) {
        std::atomic<float>* dst = (foot == 0) ? m_legFootAdjDegL : m_legFootAdjDegR;
        dst[0].store(yawDeg, std::memory_order_relaxed);
        dst[1].store(pitchDeg, std::memory_order_relaxed);
        dst[2].store(rollDeg, std::memory_order_relaxed);
    }
    void GetLegTrackerTuning(float* ankleOff, float* mp, float* my, float* mr) const {
        *ankleOff = m_legAnkleOffset.load(std::memory_order_relaxed);
        *mp = m_legMountEulerDeg[0].load(std::memory_order_relaxed);
        *my = m_legMountEulerDeg[1].load(std::memory_order_relaxed);
        *mr = m_legMountEulerDeg[2].load(std::memory_order_relaxed);
    }
    // T-pose measured hip->ankle length (m); 0 = not measured. Published to [174]/[176].
    void SetUserLegLen(float v) {
        m_userLegLen.store(v, std::memory_order_relaxed);
        m_legLenValid.store(v > 0.0f ? 1 : 0, std::memory_order_relaxed);
    }
    // Per-foot mount correction quats (T-pose solved, persisted in
    // vrik_calibration.ini, published hemisphere-packed to shared [137..139] (L) / [178..180] (R)). Identity default.
    void GetLegMountQuat(int foot, float* out4) const {   // foot: 0 = L, 1 = R
        const std::atomic<float>* src = (foot == 0) ? m_legMountQuatL : m_legMountQuatR;
        for (int i = 0; i < 4; ++i) out4[i] = src[i].load(std::memory_order_relaxed);
    }

    // ==== AUTO-CALIBRATION ====
    // T-pose calibration: user holds arms straight out sideways and stands straight.
    // We sample the live HMD + controller positions over `secs` seconds and compute:
    //   * playerHeight    = HMD Y in world (above floor)
    //   * armSpan         = max(|leftCtrl - rightCtrl|) during the sample
    //   * shoulderOffsets = anatomical HMD/body -> shoulder offsets in body-frame OpenXR axes,
    //                       derived from the visible controller/gizmo T-pose
    //   * armScale        = proven near-1.0 reach scale with small per-hand correction from
    //                       measured shoulder-to-controller asymmetry
    // Start with StartAutoCalibration(); after `secs` seconds the result is published via
    // SetVRHandCalib + the shoulder anatomical offsets (also stored in m_calibExt).
    void StartAutoCalibration(float secs = 4.0f);
    void TickAutoCalibration();   // called from the frame thread
    bool IsCalibrating() const { return m_calibState.load(std::memory_order_relaxed) != 0; }
    float GetCalibrationProgress() const { return m_calibProgress.load(std::memory_order_relaxed); }
    int GetCalibrationState() const { return m_calibState.load(std::memory_order_relaxed); }

    // Extra calibration parameters (anatomical HMD->shoulder offsets) that didn't fit in the
    // original 14-slot block. Stored on the manager so the plugin can pull them through shared
    // mem slots [70..76] and Save/Load can persist them.
    void SetShoulderAnatomical(float rx, float ry, float rz, float lx, float ly, float lz) {
        m_calibExt[0].store(rx, std::memory_order_relaxed);
        m_calibExt[1].store(ry, std::memory_order_relaxed);
        m_calibExt[2].store(rz, std::memory_order_relaxed);
        m_calibExt[3].store(lx, std::memory_order_relaxed);
        m_calibExt[4].store(ly, std::memory_order_relaxed);
        m_calibExt[5].store(lz, std::memory_order_relaxed);
        m_calibExtValid.store(1, std::memory_order_relaxed);
    }
    float GetCalibExt(int i) const {
        return (i >= 0 && i < 6) ? m_calibExt[i].load(std::memory_order_relaxed) : 0.0f;
    }

    // CAMERA->HEAD bake offset. The CP2077 FPP camera is mounted ~0.45 m ahead of the head bone;
    // the plugin publishes the (head - camera) offset into shared [85..87] and we bake it here so
    // dxgi's LocateCamera shifts the view back onto the avatar's head. The Tracking/Camera Head
    // sliders are applied ON TOP of this (they stay at 0 after baking, for fine adjustment).
    void BakeCameraOffset();              // capture shared [85..87] -> m_camBakeOffset
    void ClearCameraOffset() {
        m_camBakeOffset[0].store(0.0f, std::memory_order_relaxed);
        m_camBakeOffset[1].store(0.0f, std::memory_order_relaxed);
        m_camBakeOffset[2].store(0.0f, std::memory_order_relaxed);
    }
    void SetCameraOffset(float x, float y, float z) {
        m_camBakeOffset[0].store(x, std::memory_order_relaxed);
        m_camBakeOffset[1].store(y, std::memory_order_relaxed);
        m_camBakeOffset[2].store(z, std::memory_order_relaxed);
    }
    void GetCameraOffset(float* out) const {
        out[0] = m_camBakeOffset[0].load(std::memory_order_relaxed);
        out[1] = m_camBakeOffset[1].load(std::memory_order_relaxed);
        out[2] = m_camBakeOffset[2].load(std::memory_order_relaxed);
    }

    // Persist current calibration (everything: scales, heights, swings, poles, wrists, shoulder
    // offsets) to a file next to dxgi.dll. Returns true on success.
    bool SaveCalibrationToFile();
    bool LoadCalibrationFromFile();   // restored at startup so the user doesn't recalibrate each launch
    // Monotonic counter: the plugin dumps a diag whenever the published value changes.
    void RequestVRDiag() { m_logDiagReq.fetch_add(1, std::memory_order_relaxed); }

    // HMD yaw (radians) relative to the recenter base (= body forward), derived
    // from relOri. Used for head-oriented locomotion (rotate on-foot move vector).
    float GetHmdYawRelToBody() const;
    // Same idea for a controller (side: 0=left, 1=right). Used for hand-oriented
    // locomotion (character walks the way the chosen controller points). Returns
    // 0 if the controller pose isn't valid this frame.
    float GetHandYawRelToBody(int side) const;

    // Body-realign support: rotate the recenter base about the vertical axis by
    // `radians`, IN STEP with an equal heading injection (dxgi OnOnFootDeltaHead).
    // relOri/relPos are conj(base)*head, so base*Ry(a) shifts the HMD's relative yaw
    // by -a while the reconstructed raw pose (base*rel) is unchanged: the rendered
    // view and the HMD-local hand poses stay put while the body turns underneath.
    void RotateBaseYaw(float radians);

    // Physical-body yaw estimate (radians, rel recenter base, same convention as
    // GetHmdYawRelToBody) from the CONTROLLER POSITIONS: the left->right hand line
    // approximates the shoulder line, so its perpendicular is where the chest faces --
    // robust no matter where the wrists point (a relaxed grip aims the controllers
    // down, which makes the aim-pose YAW pure wrist noise). False if either hand is
    // untracked or the hands are too close together to define a line.
    bool GetBodyYawFromHands(float* outYaw) const;

    // Per-frame snapshot of all controller buttons/axes (OpenXR action state).
    // Filled by the frame thread under m_inputMutex; copied out by readers.
    bool GetControllerState(VRControllerState* out) const;

    float GetRuntimeHorizontalFovDeg() const { return m_runtimeHorizontalFovDeg.load(std::memory_order_relaxed); }
    bool IsRuntimeSteamVR() const { return m_runtimeIsSteamVR.load(std::memory_order_relaxed); }
    bool IsRuntimeVirtualDesktop() const { return m_runtimeIsVirtualDesktop.load(std::memory_order_relaxed); }
    float GetRuntimeVerticalFovDeg() const { return m_runtimeVerticalFovDeg.load(std::memory_order_relaxed); }
    float GetRuntimeIpd() const { return m_runtimeIpd.load(std::memory_order_relaxed); }
    void MaybeLogRuntimeFovDetails(const XrFovf& left, const XrFovf& right, float runtimeHfovDeg, float runtimeVfovDeg, float runtimeIpdMeters);
    bool GetCurrentEyeCenterOffset(int eye, XrVector3f* out);
    bool GetCurrentEyeFov(int eye, XrFovf* out);
    
    bool GetRecommendedRenderTargetSize(uint32_t* width, uint32_t* height) const;

    bool IsInitialized() const { return m_initialized; }
    bool IsSessionRunning() const { return m_sessionRunning.load(std::memory_order_relaxed); }

private:
    static DWORD WINAPI FrameThreadThunk(LPVOID param);
    DWORD FrameThreadMain();
    // Dedicated submit thread: parks while the inline Present pump owns the loop, and
    // takes the loop over whenever UseThreadedSubmit() is true (SteamVR, or
    // ThreadedMonoSubmit) so the blocking fence/swapchain waits never run on the
    // Present thread, which would freeze the game.
    static DWORD WINAPI SubmitThreadThunk(LPVOID param);
    DWORD SubmitThreadMain();
    void NotifySubmitThread();
    // Single-owner handshake for the XR frame loop. Only the Present thread (Inline)
    // and the threaded-submit frame thread can drive it, and never both at once. The
    // wait is bounded so the Present thread never blocks: on a mode switch it simply
    // skips a present instead of freezing.
    enum class FrameLoopOwner { None, Inline, Threaded };
    bool AcquireFrameLoop(FrameLoopOwner who, unsigned int timeoutMs);
    void ReleaseFrameLoop(FrameLoopOwner who);
    void PollEvents();
    bool BeginSession();
    void EndSession();
    bool EnsureMonoSubmitResources();
    bool EnsureMonoCaptureResource(const D3D12_RESOURCE_DESC& sourceDesc);
    bool EnsureDepthSnapshot(ID3D12Resource* gameDepth);

    // Records barriers + (copy OR shader resolve) into m_captureCmdList that
    // captures the current frame's scene depth into m_depthSnapshot. Uses a
    // simple CopyTextureRegion for R32-family sources (32bpp bit-compat) and
    // the DepthResolve shader for R32G8X24-family sources (64bpp plane 0
    // extract). Returns true if the snapshot was successfully recorded.
    // transitionGameDepth=false: do NOT barrier the game depth (read it as an SRV in
    // its current shader-readable state). Required for the mono path -- transitioning a
    // resource the game owns corrupts its global state tracking and device-removes.
    bool RecordDepthCapture(ID3D12GraphicsCommandList* cmdList,
                            ID3D12Resource* gameDepth,
                            D3D12_RESOURCE_STATES gameDepthState,
                            bool transitionGameDepth = true);
    // Mono scene-depth capture recorded on the GAME's own depth-writer queue
    // (OmoGetSceneDepthWriterQueue): FIFO-ordered after the depth write, so no
    // cross-queue Wait and no CP2077 present-thread hang. Sets m_depthSnapshotWriterFence
    // on success; the caller sets m_depthSnapshotSerial. Skips (returns false) when the
    // writer queue is unknown or the previous resolve is still in flight.
    bool CaptureMonoDepthOnWriterQueue(uint64_t serial);
    bool CaptureMonoPresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, uint64_t serial,
        const XrPosef poses[2], const XrFovf fovs[2], const bool hasView[2]);

    OpenXRManager() = default;
    ~OpenXRManager() = default;

    std::mutex m_initMutex;
    bool m_initialized = false;
    XrInstance m_instance = XR_NULL_HANDLE;
    XrSystemId m_systemId = XR_NULL_SYSTEM_ID;
    XrSession m_session = XR_NULL_HANDLE;
    XrSpace m_localSpace = XR_NULL_HANDLE;
    XrSpace m_viewSpace = XR_NULL_HANDLE;
    XrActionSet m_actionSet = XR_NULL_HANDLE;
    XrAction m_handPoseAction = XR_NULL_HANDLE;          // grip pose (palm) -- used by VRIK
    XrAction m_handAimPoseAction = XR_NULL_HANDLE;       // aim pose (pointing) -- used by hand-locomotion
    XrSpace  m_handAimSpaces[2] = { XR_NULL_HANDLE, XR_NULL_HANDLE };
    // Gameplay input actions (synced each frame, exposed via GetControllerState).
    XrAction m_thumbstickAction = XR_NULL_HANDLE;        // Vector2f, per hand
    XrAction m_triggerAction = XR_NULL_HANDLE;           // Float, per hand
    XrAction m_gripAction = XR_NULL_HANDLE;              // Float, per hand
    XrAction m_thumbstickClickAction = XR_NULL_HANDLE;   // Bool, per hand (L3/R3)
    XrAction m_primaryButtonAction = XR_NULL_HANDLE;     // Bool, per hand (X / A)
    XrAction m_secondaryButtonAction = XR_NULL_HANDLE;   // Bool, per hand (Y / B)
    XrAction m_menuButtonAction = XR_NULL_HANDLE;        // Bool, left only on Touch
    XrPath m_handPaths[2] = { XR_NULL_PATH, XR_NULL_PATH };
    XrSpace m_handSpaces[2] = { XR_NULL_HANDLE, XR_NULL_HANDLE };
    // Latest controller snapshot, owned by the frame thread.
    mutable std::mutex m_inputMutex;
    VRControllerState m_controllerState{};
    // Yaws (radians, recenter-base relative) for hand-oriented locomotion.
    std::atomic<float> m_handYawRelToBody[2]{ {0.0f}, {0.0f} };
    std::atomic<bool>  m_handYawValid[2]{ {false}, {false} };
    struct TrackingPoseFilterState {
        bool initialized = false;
        XrVector3f position{};
        XrQuaternionf orientation{};
    };
    struct TrackingAngleFilterState {
        bool initialized = false;
        float angleRad = 0.0f;
    };
    // Adaptive tracking filters: remove static jitter, but release quickly once
    // the player actually moves the HMD/controller.
    TrackingPoseFilterState m_headFilterState{};
    TrackingPoseFilterState m_handFilterState[2]{};
    TrackingAngleFilterState m_handAimYawFilter[2]{};
    XrSessionState m_sessionState = XR_SESSION_STATE_UNKNOWN;
    std::vector<XrViewConfigurationView> m_viewConfigViews;
    std::vector<XrView> m_views;

    // Hands
    std::mutex m_handMutex;
    OpenXRHeadPose m_hands[2]{};
    // ── Pose Pair Locking (45 Hz skeleton) ── guarded by m_handMutex.
    // Snapshot of the pair-leader eye's tracking, replayed on eye1 so VRIK's
    // skeleton is frozen across the stereo pair. m_pairLockHmdOri is the RAW
    // (un-remapped, un-predicted) OpenXR HMD orientation written to shared[16..19].
    bool m_pairLockHeadValid = false;
    bool m_pairLockHandsValid = false;
    // Last PRESENT eye seen by the OnPresent pair-lock publish — edge-detects the
    // follower present (0->1) so the snapshot is taken once per pair. -1 = none yet.
    int m_pairLockLastEye = -1;
    // Seqlock counter for the shared-memory pose block (FlushHandsToShared). Bumped
    // odd before / even after each write so the VRIK plugin can detect torn reads.
    uint32_t m_sharedSeq = 0;
    OpenXRHeadPose m_pairLockHeadPose{};
    OpenXRHeadPose m_pairLockHands[2]{};
    float m_pairLockHmdOri[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    // Frozen RAW physical head height (m_posY, no prediction) — drives VRIK body
    // height/squat shared slots [89]/[90], which otherwise sampled live per eye and
    // bobbed the body at the alternation rate. Snapshot alongside m_pairLockHmdOri.
    float m_pairLockHmdPosY = 0.0f;
    float m_weaponPitch = 0.0f;
    float m_weaponYaw = 0.0f;
    float m_weaponRoll = 0.0f;
    float m_weaponDx = 0.0f;
    float m_weaponDy = 0.0f;
    float m_weaponDz = 0.0f;
    // Defaults ON: VR hand tracking (mode 4 = full-arm IK) + decoupled VR weapon aim (bullet follows
    // controller). The F10 overlay checkboxes also default to true so the UI stays in sync.
    std::atomic<int> m_vrHandTrackingMode{4};
    std::atomic<int> m_weaponAimEnable{1};
    std::atomic<int> m_immersiveHolsters{1};   // 1 = visual-holster equip (default), 0 = simple slot mapping
    float* m_sharedHandsPtr = nullptr;   // cached shared-mem view (set in OnPresent) for GetSharedSlot

    // VR hand IK calibration, pushed from the overlay into shared-mem slots [33..47] each
    // present; the RED4ext plugin reads them when [33] (valid) is set, else keeps its own
    // baked defaults. [48] = one-shot "write diag" request the plugin clears after dumping.
    std::atomic<int>   m_calibValid{0};
    std::atomic<float> m_calib[14]{}; // scaleR,scaleL,heightR,heightL,swingR,swingL,poleR,poleL, wristR pyr(3), wristL pyr(3)

    // Extra calibration: anatomical HMD/body->shoulder offsets in body-frame OpenXR axes (right, up, back).
    // Layout: [0..2] right shoulder (rx, ry, rz), [3..5] left shoulder (lx, ly, lz).
    std::atomic<int>   m_calibExtValid{0};
    std::atomic<float> m_calibExt[6]{};

    // T-pose measured anatomy published to shared slots [77..80] each present: real arm length
    // per hand (m) + HMD eye height (m). The plugin scales the avatar arm bones to match
    // (gizmo-path), instead of the old position-scale hack. [80] = valid.
    std::atomic<int>   m_measureValid{0};
    std::atomic<float> m_userArmLenR{0.0f};
    std::atomic<float> m_userArmLenL{0.0f};
    std::atomic<float> m_userEyeHeight{0.0f};

    // ---- Body trackers (XR_HTCX_vive_tracker_interaction) --------------------
    // Index convention everywhere: [0] = left foot, [1] = right foot, [2] = waist.
    // m_trackers[] mirrors m_hands[]: HMD-local pose (pos rel HMD in HMD axes, ori
    // rel HMD), written by PollViveTrackers under m_handMutex. m_trackerStagePos[]
    // keeps the raw LOCAL-space position (floor-relative Y) for calibration.
    static constexpr int kBodyTrackerCount = 3;
    bool               m_viveTrackerExt = false;
    PFN_xrEnumerateViveTrackerPathsHTCX m_pfnEnumViveTrackers = nullptr;
    XrAction           m_trackerPoseAction = XR_NULL_HANDLE;
    XrPath             m_trackerRolePaths[kBodyTrackerCount] = { XR_NULL_PATH, XR_NULL_PATH, XR_NULL_PATH };
    XrSpace            m_trackerSpaces[kBodyTrackerCount] = { XR_NULL_HANDLE, XR_NULL_HANDLE, XR_NULL_HANDLE };
    OpenXRHeadPose     m_trackers[kBodyTrackerCount]{};
    XrVector3f         m_trackerStagePos[kBodyTrackerCount]{};
    bool               m_trackerStageValid[kBodyTrackerCount] = { false, false, false };
    TrackingPoseFilterState m_trackerFilterState[kBodyTrackerCount]{};
    OpenXRHeadPose     m_pairLockTrackers[kBodyTrackerCount]{};
    std::atomic<int>   m_viveTrackerCount{0};      // trackers seen with a body role (0..3)
    double             m_lastViveTrackerEnum = 0.0; // QPC seconds of last re-enumerate
    // OpenVR fallback provider state. Types stay opaque here (void*) so this
    // header never drags in the big vendored openvr.h; openvr_trackers.cpp
    // casts m_openvrSystem to vr::IVRSystem*.
    void*              m_openvrModule = nullptr;    // HMODULE of openvr_api.dll
    void*              m_openvrSystem = nullptr;    // vr::IVRSystem*
    uint32_t           m_openvrRoleDevice[kBodyTrackerCount] = { 0xFFFFFFFFu, 0xFFFFFFFFu, 0xFFFFFFFFu };
    uint64_t           m_openvrDeviceSetMask = 0;   // connected GenericTracker fingerprint
    int                m_openvrTrackerCount = 0;    // connected GenericTrackers (last scan)
    double             m_lastOpenvrScan = 0.0;      // FbtNowSeconds() of last device scan
    double             m_lastOpenvrRemap = 0.0;     // FbtNowSeconds() of last remap attempt
    // Floor-relative HMD height from the OpenVR standing universe, refreshed by
    // the per-frame poll. The mod's own m_posY is in the head-origin OpenXR
    // space (~0 while standing), which fails the calibration's eyeHeight gate;
    // calibration prefers this when the OpenVR provider is live.
    float              m_openvrHmdStageY = 0.0f;
    std::atomic<int>   m_legTrackersEnable{0};
    std::atomic<int>   m_waistTrackerEnable{0};    // optional 3rd point: waist drives hips
    std::atomic<int>   m_legKickDamageEnable{0};   // fix15: kicks tap the native melee attack
    std::atomic<float> m_legAnkleOffset{0.10f};    // tracker-on-shoe -> ankle joint (m)
    std::atomic<float> m_legMountEulerDeg[3]{};    // foot mount correction p/y/r (deg, legacy -- superseded by the per-foot quats below)
    // fix12/fix14: manual trim y/p/r (deg), F10 sliders. Defaults are the
    // play-tested boot-mesh correction (same both feet): yaw 0, pitch -45,
    // roll +90 -- the boots render flat with toes forward out of the box.
    std::atomic<float> m_legFootAdjDegL[3] = { 0.0f, -45.0f, 90.0f };
    std::atomic<float> m_legFootAdjDegR[3] = { 0.0f, -45.0f, 90.0f };
    // PER-FOOT mount correction quats, solved by the plugin's T-pose sampler
    // (published to [191..199]), adopted+persisted here, republished as the
    // ACTIVE mounts (hemisphere-packed) on [137..139] (L) / [178..180] (R).
    // Identity = never calibrated.
    std::atomic<float> m_legMountQuatL[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
    std::atomic<float> m_legMountQuatR[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
    // T-pose mount-solve handshake (frame thread only): [190] sampling flag,
    // awaiting state for the plugin's solved-mount publish on [191..199].
    std::atomic<int>   m_mountCalibSampling{0};
    std::atomic<bool>  m_mountSolveAwaiting{false};
    double             m_mountSolveAwaitStart = 0.0;    // frame thread only
    float              m_mountSolveConsumedSeq = 0.0f;  // frame thread only
    std::atomic<int>   m_legLenValid{0};
    std::atomic<float> m_userLegLen{0.0f};         // T-pose hip->ankle (m)
    // Auto-calibration accumulators (stage-space heights, averaged). Feet feed the
    // ankle height; the waist tracker measures hip height directly when present.
    float              m_calibFootYSum[2] = { 0.0f, 0.0f };
    int                m_calibFootSamples[2] = { 0, 0 };
    float              m_calibWaistYSum = 0.0f;
    int                m_calibWaistSamples = 0;

    // Camera->head bake offset (game-local right/forward/up), applied by dxgi's LocateCamera.
    std::atomic<float> m_camBakeOffset[3]{};

    // Auto-calibration state machine:
    //   m_calibState  0 = idle, 1 = sampling, 2 = done
    //   m_calibSeconds = total target duration (typically 3.0)
    //   m_calibStart   = sim time when sampling began
    //   m_calibProgress = 0..1 fraction of the way through sampling
    //   sample stats (best armSpan + accumulators) tracked in cpp
    std::atomic<int>   m_calibState{0};
    std::atomic<float> m_calibSeconds{3.0f};
    std::atomic<float> m_calibProgress{0.0f};
    double             m_calibStart = 0.0;
    float              m_calibArmSpanMax = 0.0f;
    float              m_calibHmdHeightSum = 0.0f;
    int                m_calibSampleCount = 0;
    float              m_calibCtrlPosSumR[3] = {0,0,0};
    float              m_calibCtrlPosSumL[3] = {0,0,0};
    std::atomic<int>   m_logDiagReq{0};

    // Graphics binding
    XrGraphicsBindingD3D12KHR m_graphicsBinding{};
    ID3D12Device* m_d3dDevice = nullptr;
    ID3D12CommandQueue* m_d3dQueue = nullptr;
    ID3D12CommandAllocator* m_cmdAllocators[3] = {};
    uint32_t m_cmdAllocatorIndex = 0;
    ID3D12GraphicsCommandList* m_cmdLists[3] = {};
    ID3D12Fence* m_fence = nullptr;
    HANDLE m_fenceEvent = nullptr;
    UINT64 m_fenceValue = 0;
    std::mutex m_captureMutex;
    ID3D12CommandAllocator* m_captureCmdAllocators[3] = {};
    uint32_t m_captureAllocatorIndex = 0;
    ID3D12GraphicsCommandList* m_captureCmdLists[3] = {};
    ID3D12Fence* m_captureFence = nullptr;
    HANDLE m_captureFenceEvent = nullptr;
    UINT64 m_captureFenceValue = 0;
    HANDLE m_monoPresentEvent = nullptr;
    HANDLE m_frameSyncEvent = nullptr;
    ID3D12DescriptorHeap* m_rtvHeap = nullptr;
    UINT m_rtvDescriptorSize = 0;
    std::mutex m_viewMutex;
    std::mutex m_presentMutex;
    ID3D12Resource* m_lastPresentedBackBuffer = nullptr;
    uint32_t m_lastPresentedWidth = 0;
    uint32_t m_lastPresentedHeight = 0;
    uint32_t m_lastPresentedFormat = 0;
    uint32_t m_lastPresentedBufferIndex = 0;
    uint64_t m_lastPresentSerial = 0;
    uint64_t m_lastSubmittedSerial = 0;

    struct EyeSwapchain {
        XrSwapchain handle = XR_NULL_HANDLE;
        int32_t width = 0;
        int32_t height = 0;
        std::vector<XrSwapchainImageD3D12KHR> images;
        std::vector<D3D12_CPU_DESCRIPTOR_HANDLE> rtvs;
        XrSwapchain depthHandle = XR_NULL_HANDLE;
        std::vector<XrSwapchainImageD3D12KHR> depthImages;
    };
    struct CapturedMonoFrame {
        ID3D12Resource* texture = nullptr;
        uint32_t width = 0;
        uint32_t height = 0;
        uint32_t format = 0;
        uint64_t serial = 0;
        XrPosef poses[2]{};
        XrFovf fovs[2]{};
        bool hasView[2]{};
    };
    std::vector<EyeSwapchain> m_eyeSwapchains;
    CapturedMonoFrame m_monoCapturedFrame;
    // [DEPTH] Game scene-depth snapshot for the XR depth layer (parallel to color).
    ID3D12Resource* m_depthSnapshot = nullptr;
    uint32_t m_depthSnapshotW = 0;
    uint32_t m_depthSnapshotH = 0;
    // A POOL of snapshot buffers, rotated by the producer -- RealVR's 3-slot queue, and it
    // is not decoration. With a single buffer the producer must either block until the
    // consumer finishes (which puts the compositor's pacing on the game's Present) or skip
    // the capture (which starves the headset of new frames while the game itself runs fine
    // -- smooth on the monitor, freezing in the HMD). With three, the producer always writes
    // a buffer nobody is reading and neither side ever waits.
    ID3D12Resource* m_monoPool[3] = {};
    uint32_t        m_monoPoolSlot = 0;

    // ---- right eye (VRCAM) ---------------------------------------------------------------
    // OUR OWN copy of the VRCAM view, produced at Present on the capture list, already in the
    // eye swapchain's format and size. The submit then treats it exactly like MAIN's
    // snapshot: a plain CopyResource into the XR image.
    //
    // The conversion belongs here, not in the submit. sync_stereo's snapshot is written by the
    // engine and also read by the desktop mirror on another queue; reaching for it from the
    // submit thread at display rate had two queues asserting states on one resource, and the
    // GPU hung on the very first VRCAM frame (DXGI_ERROR_DEVICE_HUNG). At Present the engine's
    // frame is finished and we are already on the list that copies the backbuffer -- the one
    // moment that resource is ours to read. Past this point nothing outside the capture ever
    // touches an engine-owned resource.
    // Eye image geometry, cached as plain atomics. The capture runs on the Present thread and
    // must NOT read m_eyeSwapchains: that vector is created and resized by the SUBMIT thread
    // (EnsureMonoSubmitResources), so touching it from here would be a race whose payoff is a
    // dangling element pointer.
    std::atomic<uint32_t> m_eyeImageW{0};
    std::atomic<uint32_t> m_eyeImageH{0};
    // TWO formats, and conflating them kills the device.
    //   m_eyeImageFmt - the eye RESOURCE's format. The runtime allocates it TYPELESS
    //                   (measured: 27 = R8G8B8A8_TYPELESS) so the app can choose a view.
    //                   Our own eye texture must match this for CopyResource to be legal.
    //   m_eyeViewFmt  - the TYPED format the swapchain was created with (_UNORM_SRGB). This
    //                   is what an RTV and a PSO must use; a typeless RTV is invalid, and
    //                   creating one took the GPU down instantly (DXGI_ERROR_DEVICE_HUNG
    //                   right after "stereo capture texture ready ... fmt=27").
    std::atomic<uint32_t> m_eyeImageFmt{0};
    std::atomic<uint32_t> m_eyeViewFmt{0};

    ID3D12Resource* m_vrcamEyeTex = nullptr;
    uint32_t        m_vrcamEyeW = 0;
    uint32_t        m_vrcamEyeH = 0;
    uint32_t        m_vrcamEyeFmt = 0;
    uint64_t        m_vrcamEyeSerial = 0;   // present serial this copy belongs to; 0 = none
    bool EnsureVrcamEyeTexture(uint32_t width, uint32_t height, DXGI_FORMAT format);
    uint64_t m_depthSnapshotSerial = 0; // serial of the color frame this depth matches; 0 = invalid/empty
    // Staging copy of the game depth, taken inline at the readable barrier (see
    // CaptureSceneDepthInline). Same layout as the source; left in PIXEL_SHADER_RESOURCE so
    // the resolve can read it straight away.
    ID3D12Resource* m_depthStage = nullptr;
    uint32_t        m_depthStageW = 0;
    uint32_t        m_depthStageH = 0;
    uint32_t        m_depthStageFmt = 0;
    uint64_t        m_depthStageSerial = 0;   // present serial this staging copy belongs to
    // Head pose of that same frame, recorded at the same instant (see GetFramePoseForSerial).
    OpenXRHeadPose  m_framePose{};
    uint64_t        m_framePoseSerial = 0;
    // Recenter base mirrored as atomics -- see GetRecenterBase().
    std::atomic<float> m_baseOriX{0.0f}, m_baseOriY{0.0f}, m_baseOriZ{0.0f}, m_baseOriW{1.0f};
    std::atomic<float> m_basePosX{0.0f}, m_basePosY{0.0f}, m_basePosZ{0.0f};

    // Per-frame slot ring -- see PublishFrameSlot(). 8 is comfortably deeper than any pipelining
    // the engine does and keeps the modulo free.
    static constexpr uint32_t kFrameSlots = 8;
    XrFrameSlot m_frameSlots[kFrameSlots]{};
    mutable std::mutex m_frameSlotMutex;

    // Rolling least-squares fit of display time against present serial -- see FitAddDisplayTime().
    mutable std::mutex m_fitMutex;
    bool     m_fitAnchored = false;
    uint64_t m_fitWx0 = 0;
    XrTime   m_fitWy0 = 0;
    double   m_fitN = 0.0, m_fitSx = 0.0, m_fitSy = 0.0, m_fitSxx = 0.0, m_fitSxy = 0.0;
    uint64_t m_fitAx = 0;      // published line: displayTime = m_fitAy + m_fitK * (serial - m_fitAx)
    XrTime   m_fitAy = 0;
    double   m_fitK = 0.0;
    bool     m_fitValid = false;
    std::atomic<XrTime> m_frameAimTime{0};   // see SetFrameAimTime()
    // One head sample per aim epoch -- see AcquireFrameHeadSample().
    std::atomic<uint64_t> m_frameAimEpoch{0};
    std::mutex            m_frameSampleMutex;
    uint64_t              m_frameSampleEpoch = ~0ull;
    OpenXRHeadPose        m_frameSample{};
    // Poses read back out of the engine at frame-open -- see PushRenderedFramePose().
    static constexpr uint32_t kRenderedFrameQ = 8;
    mutable std::mutex    m_renderedFrameMutex;
    OpenXRHeadPose        m_renderedFrameQ[kRenderedFrameQ]{};
    uint64_t              m_renderedFrameHead = 0;
    uint64_t              m_renderedFrameTail = 0;
    std::mutex      m_depthStageMutex;
    std::atomic<uint64_t> m_depthStageFrame{~0ull};   // one inline copy per present interval
    bool m_depthLayerSupported = false;  // runtime supports XR_KHR_composition_layer_depth and a depth swapchain format
    int64_t m_depthSwapchainFormat = 0;  // chosen runtime depth format (e.g. DXGI_FORMAT_D32_FLOAT)
    // Dedicated list/fence for the mono depth resolve executed on the game's depth-
    // writer queue (see CaptureMonoDepthOnWriterQueue). Single-slot: skip if in flight.
    ID3D12CommandAllocator* m_depthWriterAlloc = nullptr;
    ID3D12GraphicsCommandList* m_depthWriterList = nullptr;
    ID3D12Fence* m_depthWriterFence = nullptr;
    uint64_t m_depthWriterFenceValue = 0;      // last value signaled on the writer queue
    uint64_t m_depthSnapshotWriterFence = 0;   // writer-fence value guarding the current m_depthSnapshot
    // Cached QueryPerformanceFrequency (constant per boot; avoids re-querying
    // every frame tick).
    LONGLONG m_qpcFreq = 0;

    // 64bpp depth-plane resolve. CP2077 scene depth is R32G8X24_TYPELESS in
    // most scenes; a shader pass converts plane 0 (depth) into a D32_FLOAT
    // texture compatible with the OpenXR depth swapchain. Without this, depth
    // submit only works in the small subset of scenes that happen to use
    // R32-family depth, and `depth=0` shows up in submit logs everywhere
    // else — which prevents compositor positional async-timewarp.
    std::unique_ptr<DepthResolve> m_depthResolve;

    // CAS sharpen pass. Records onto the submit command list right where the eye
    // image is written to the swapchain when xr_sharpness > 0.
    std::unique_ptr<SharpenPass> m_sharpenPass;
    bool m_sharpenReady = false;

    // Reuse-last-frame path: persistent "last good" eye images + their pose/fov.
    // On a stale tick we re-present these with the stashed pose so the runtime
    // reprojects the last clean frame to the current head instead of warping a
    // stale eye.
    Microsoft::WRL::ComPtr<ID3D12Resource> m_lastGoodEye[2];
    bool m_lastGoodEyeInited[2] = {false, false};
    XrPosef m_lastGoodPose[2]{};
    XrFovf m_lastGoodFov[2]{};
    bool m_lastGoodValid = false;
    std::unique_ptr<ColorBlit> m_colorBlit;
    // Engine present pacing: see openxr_manager.cpp OnPresent. The HMD-paced gate is
    // m_frameSyncEvent.
    std::atomic<int64_t> m_predictedDisplayPeriodNs{0};

    HANDLE m_frameThread = nullptr;
    std::atomic<bool> m_stopFrameThread = false;
    // Submit-thread wake + XR frame-loop ownership (see SubmitThreadMain).
    std::mutex m_submitThreadMutex;
    std::condition_variable m_submitThreadWakeCv;
    std::mutex m_frameLoopMutex;
    std::condition_variable m_frameLoopCv;
    FrameLoopOwner m_frameLoopOwner = FrameLoopOwner::None;
    std::atomic<bool> m_sessionRunning = false;
    std::atomic<bool> m_monoSubmitEnabled = false;
    std::atomic<int> m_renderEyeIndex = 0;
    std::atomic<bool> m_poseValid = false;
    std::atomic<float> m_posX = 0.0f;
    std::atomic<float> m_posY = 0.0f;
    std::atomic<float> m_posZ = 0.0f;
    std::atomic<float> m_oriX = 0.0f;
    std::atomic<float> m_oriY = 0.0f;
    std::atomic<float> m_oriZ = 0.0f;
    std::atomic<float> m_oriW = 1.0f;
    // QPC milliseconds (mod 100 s) at which the controller poses were sampled. Published with
    // them under the seqlock so the plugin can report how old the pose it solved from was.
    std::atomic<float> m_handSampleMs = 0.0f;
    // The head position taken in the same locked step as the controller poses. Guarded by
    // m_handMutex, so plain floats.
    float m_handSampleHeadPos[3] = {0.0f, 0.0f, 0.0f};
    bool  m_handSampleHeadValid = false;
    std::atomic<float> m_runtimeHorizontalFovDeg = 0.0f;
    std::atomic<float> m_runtimeVerticalFovDeg = 0.0f;
    std::atomic<float> m_runtimeIpd = 0.0f;
    bool m_runtimeFovLogInitialized = false;
    XrFovf m_loggedRuntimeEyeFovs[2]{};
    float m_loggedRuntimeHorizontalFovDeg = 0.0f;
    float m_loggedRuntimeVerticalFovDeg = 0.0f;
    float m_loggedRuntimeIpd = 0.0f;
    float m_loggedForcedProjectionFovDeg = 0.0f;
    std::atomic<bool> m_runtimeIsSteamVR = false;
    std::atomic<bool> m_runtimeIsVirtualDesktop = false;
    // Head velocity in the base-recentered frame (rad/s, m/s), sampled from
    // xrLocateSpace. See GetHeadPose().
    std::atomic<bool> m_velValid = false;
    std::atomic<float> m_angVelX = 0.0f;
    std::atomic<float> m_angVelY = 0.0f;
    std::atomic<float> m_angVelZ = 0.0f;
    std::atomic<float> m_linVelX = 0.0f;
    std::atomic<float> m_linVelY = 0.0f;
    std::atomic<float> m_linVelZ = 0.0f;
    std::atomic<bool> m_recenterRequested = false;
    std::atomic<bool> m_syncedPoseValid = false;
    std::atomic<float> m_syncedPosX = 0.0f;
    std::atomic<float> m_syncedPosY = 0.0f;
    std::atomic<float> m_syncedPosZ = 0.0f;
    std::atomic<float> m_syncedOriX = 0.0f;
    std::atomic<float> m_syncedOriY = 0.0f;
    std::atomic<float> m_syncedOriZ = 0.0f;
    std::atomic<float> m_syncedOriW = 1.0f;
    XrPosef m_syncedEyePoses[2]{};
    XrFovf m_syncedEyeFovs[2]{};
    bool m_syncedEyeViewsValid = false;
    // Per-eye head pose captured at render time by the camera hook (render-pose submit).
    std::mutex m_renderPoseMutex;
    // The pose the camera injection actually used for the frame being built (see
    // SetPendingRenderHeadPose). Present attaches this to the snapshot it captures.
    std::mutex m_pendingRenderPoseMutex;
    OpenXRHeadPose m_renderPoseRing[16]{};
    uint64_t m_renderPoseStamp[16]{};      // present interval the pose was injected in
    uint64_t m_renderPoseRingHead = 0;
    bool m_pendingRenderPoseValid = false;
    XrPosef m_renderEyeHeadPose[2]{};
    bool m_renderEyeHeadPoseValid[2]{};
    
    // Pose queue for accurately syncing frames with pipeline lag
    std::atomic<uint64_t> m_presentCount{0};
    XrPosef m_poseQueue[256]{};
    uint32_t m_poseQueueFrame[256]{};
    
    bool m_basePoseSet = false;
    XrPosef m_basePose{};

    // MENU PANEL ANCHOR (LAZY-FOLLOW). The menu/map panel is anchored in front of the
    // player when a menu opens and then holds still while the head turns WITHIN a
    // dead-zone; once the head-vs-panel yaw offset exceeds a start threshold the panel
    // smoothly re-centers to the head (and stops at a small band). This avoids both the
    // 1:1 head-lock (UI drags with every micro head motion -> motion sickness) and the
    // rigid world-lock (panel can drift fully out of view). Reset by the frame loop
    // whenever no menu is active. Frame-thread only.
    //   Quad path: m_menuYaw + m_menuPivot -> ComputeMenuQuadPose().
    //   Projection path: m_menuEyePoses[] latched, re-latched (snap) past the threshold.
    bool  m_menuAnchorValid = false;   // quad-layer anchor latched
    bool  m_menuFollowing   = false;   // currently easing toward the head
    float m_menuYaw         = 0.0f;    // current panel yaw (rad, m_localSpace)
    XrVector3f m_menuPivot{};          // panel pivot (head position, followed each frame)
    uint64_t m_menuLastQpc  = 0;       // QPC for the follow ease dt
    bool  m_menuEyeAnchorValid = false;// projection-path per-eye anchor latched
    float m_menuEyeAnchorYaw   = 0.0f; // head yaw the per-eye poses were latched at
    XrPosef m_menuQuadPose{};          // latched/eased quad-layer pose (m_localSpace)
    XrPosef m_menuEyePoses[2]{};       // latched per-eye poses for the projection path

    // Lazy-follow quad-layer menu pose from the live head pose (or base pose when the
    // head isn't located). Maintains m_menuYaw/m_menuPivot/m_menuFollowing with
    // start/stop hysteresis (threshold from GetMenuFollowDeg()) and a fixed ease rate.
    XrPosef ComputeMenuQuadPose(bool headPoseLocated, const XrPosef& headPose);
};
