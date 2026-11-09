#pragma once

#include "xrfg/d3d12_history.hpp"
#include "xrfg/dlss_motion_vectors.hpp"
#include "xrfg/fps_overlay_model.hpp"
#include "xrfg/pose.hpp"

#include <d3d12.h>

#include <atomic>
#include <cstdint>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <span>

namespace xrfg {

struct D3D12FieldOfView {
    float angle_left{};
    float angle_right{};
    float angle_up{};
    float angle_down{};
};

struct D3D12ImageRect {
    std::uint32_t offset_x{};
    std::uint32_t offset_y{};
    std::uint32_t width{};
    std::uint32_t height{};
};

struct D3D12ReprojectionView {
    Pose pose{};
    D3D12FieldOfView fov{};
    // A zero-sized rectangle preserves the legacy full-resource contract for
    // direct synthesizer callers. The OpenXR layer always supplies the exact
    // submitted XrSwapchainSubImage rectangle.
    D3D12ImageRect image_rect{};
    // OpenXR callers provide the physical texture-array slice explicitly.
    // The sentinel preserves the legacy direct-call contract where one view
    // per array slice is supplied in slice order.
    std::uint32_t array_slice{
        std::numeric_limits<std::uint32_t>::max()};
};

struct D3D12FrameSynthesisTicket {
    std::uint64_t previous_serial{};
    std::uint64_t current_serial{};
    // The value the whole pair completes at, which on the deferred path is
    // what the held-back current copy will signal - so it is not signalled
    // until flush_current_copy submits that copy, a display period later.
    std::uint64_t fence_value{};
    // The value at which the synthetic's pixels exist, signalled as soon as
    // the synthesis work is queued. Anything waiting for the synthetic and
    // only the synthetic must use this: waiting on fence_value instead
    // blocks until a copy that has not been submitted yet completes.
    std::uint64_t synthetic_fence_value{};
    std::uint32_t work_slot{};
    std::uint32_t synthetic_destination_index{
        std::numeric_limits<std::uint32_t>::max()};
    std::uint32_t current_destination_index{};
};

// Completed direct-queue GPU intervals for one NVIDIA Optical Flow pair.
// Values are expressed in microseconds and become available asynchronously
// after the pair's completion fence has passed.
struct D3D12NvidiaGpuTiming {
    std::uint64_t previous_serial{};
    std::uint64_t current_serial{};
    std::uint64_t pack_microseconds{};
    std::uint64_t eye0_microseconds{};
    std::uint64_t eye1_microseconds{};
    std::uint64_t composition_microseconds{};
    std::uint64_t total_microseconds{};
    // The pair's whole GPU span, as QueryPerformanceCounter values. Zero
    // when the device offered no calibration.
    std::uint64_t gpu_begin_qpc{};
    std::uint64_t gpu_end_qpc{};
    std::uint32_t eye_count{};
};

enum class D3D12OpticalFlowBackend {
    fidelity_fx,
    nvidia,
};

enum class D3D12NvidiaPerformancePreset {
    slow,
    medium,
    fast,
};

// Ratio the optical-flow input is packed at. Shared by both backends: the
// FidelityFX path ran at full resolution and honoured no scale at all, while
// NVIDIA has defaulted to half per axis all along. One control now covers both.
enum class D3D12OpticalFlowInputScale {
    full,
    three_quarter,
    half,
    quarter,
};

// The name the NVIDIA options and the tray settings already use.
using D3D12NvidiaInputScale = D3D12OpticalFlowInputScale;

struct D3D12NvidiaOpticalFlowOptions {
    D3D12NvidiaPerformancePreset preset{
        D3D12NvidiaPerformancePreset::medium};
    // Applies to whichever backend is running: the FidelityFX pack and flow
    // honour it too, so there is one input-resolution control rather than
    // one per backend.
    D3D12OpticalFlowInputScale input_scale{
        D3D12OpticalFlowInputScale::half};
    bool bidirectional{};
    // Hybrid: the game's motion vectors, where the session receives them,
    // composed per pixel with whichever optical flow this backend runs. Off,
    // vectors replace the flow for a pair that has them (the vectors-only
    // composition); on, both run and the pixel takes the one that explains
    // the two frames better. Pairs without vectors run the flow alone either
    // way.
    bool hybrid{};
};

// A second synthetic produced from the same pair, for a session that hands
// the runtime three frames per application frame. The optical flow is the
// pair's and is computed once; this costs one more composition pass, into
// its own destination image and at its own instant between the two captures.
struct D3D12ExtraSynthetic {
    std::uint32_t destination_index{};
    // As submit_pair's interpolation_fraction: 0 at the previous capture, 1
    // at the current one.
    float interpolation_fraction{0.5F};
};

// Owns a rolling exclusive history lease. Each resource supports at most two
// bounded projection views. They ordinarily map one-to-one to at most two
// array slices, while a single slice may also contain two non-overlapping
// viewports for double-wide stereo. Each logical view is extracted into
// independent mono A/B/S resources. FidelityFX uses one persistent optical-flow
// context and temporal history per view; NVIDIA uses the same explicit A/B/S
// boundary with one OFA context per eye and a strict cross-context fence chain.
// submit_pair subtracts the
// image-space component generated by the OpenXR orientation/FOV mapping and
// renders residual scene motion in B's camera plane. The target camera must
// equal B, so current remains a bit-exact B copy and both outputs use B pose/FOV
// metadata. Translation remains depth-unaware. A repeated capture ticket is a
// legal unchanged-resource input: each view reuses only its own retained mono
// image while camera metadata advances. Submission calls never wait on the CPU.
class D3D12FrameSynthesizer final {
public:
    D3D12FrameSynthesizer() noexcept;
    ~D3D12FrameSynthesizer();

    D3D12FrameSynthesizer(const D3D12FrameSynthesizer&) = delete;
    D3D12FrameSynthesizer& operator=(const D3D12FrameSynthesizer&) = delete;

    [[nodiscard]] HRESULT initialize(
        ID3D12Device* device,
        ID3D12CommandQueue* queue,
        std::shared_ptr<D3D12SwapchainHistory> history,
        std::span<ID3D12Resource* const> current_destination_images,
        std::span<ID3D12Resource* const> synthetic_destination_images,
        DXGI_FORMAT view_format,
        // Native D3D12 OpenXR destinations use RENDER_TARGET. Shared D3D11
        // interop mirrors use COMMON at both API ownership boundaries.
        D3D12_RESOURCE_STATES release_state,
        D3D12OpticalFlowBackend backend =
            D3D12OpticalFlowBackend::fidelity_fx,
        D3D12NvidiaOpticalFlowOptions nvidia_options = {},
        bool enable_nvidia_gpu_timing = false,
        // Native Presenter already owns B. NVIDIA flow-only mode can omit its
        // redundant real output/copy; current and synthetic destinations may alias.
        bool reconstruction_only = false) noexcept;

    [[nodiscard]] HRESULT submit_prime(
        const D3D12HistoryCaptureTicket& current,
        std::span<const D3D12ReprojectionView> current_source_views,
        std::uint32_t current_destination_index,
        D3D12FrameSynthesisTicket* ticket,
        std::shared_ptr<const DlssMotionVectorSet> motion_vectors = {}) noexcept;

    [[nodiscard]] HRESULT submit_pair(
        const D3D12HistoryCaptureTicket& current,
        std::span<const D3D12ReprojectionView> current_source_views,
        std::span<const D3D12ReprojectionView> synthetic_target_views,
        std::uint32_t synthetic_destination_index,
        std::uint32_t current_destination_index,
        D3D12FrameSynthesisTicket* ticket,
        std::optional<OverlayPlacement> debug_marker = std::nullopt,
        std::shared_ptr<const DlssMotionVectorSet> motion_vectors = {},
        // Records the current output's copy but leaves it unsubmitted for
        // flush_current_copy to hand over once the synthetic frame has gone
        // to the runtime. Defaults off: a caller that never flushes would
        // publish a stale current frame.
        bool defer_current_copy = false,
        // Where the synthetic sits between the previous capture and the
        // current one, 0 at the previous and 1 at the current. Half is right
        // only when the application runs at exactly half the display rate,
        // because only then are the two captures two display periods apart
        // and the synthetic shown one period before the current frame.
        // Clamped internally; out-of-range values fall back to half.
        float interpolation_fraction = 0.5F,
        // A second synthetic from the same pair, written in the same
        // submission and complete at the same fence value. Its destination
        // is another image of the synthetic set and must differ from
        // synthetic_destination_index.
        std::optional<D3D12ExtraSynthetic> extra_synthetic = std::nullopt,
        // Native mono reconstruction only: enqueue packing/OFA now, then call
        // flush_reconstruction after BOTH eyes have prepared. No completion
        // receipt may be joined onto this queue before both flushes.
        bool defer_reconstruction = false) noexcept;

    [[nodiscard]] HRESULT flush_reconstruction() noexcept;

    // Executes the current output's copy, which submit_pair records but
    // deliberately leaves unsubmitted. The synthetic frame is handed to the
    // runtime a display period before the current one, and a runtime waits
    // for the whole queue when it takes a frame, so a copy left queued ahead
    // of the synthetic makes the frame with the tighter deadline wait for a
    // full-resolution copy it never reads. The caller submits the synthetic
    // first and calls this immediately afterwards, leaving the copy a whole
    // period to finish before the current frame needs it.
    //
    // Safe to call when nothing is pending. Submission entry points flush
    // any copy still outstanding themselves, so a caller that never gets
    // here costs a frame of latency rather than correctness.
    // Submits the held-back copy. consumer_queue, when the synthesis runs on
    // a queue of its own, is made to wait for that copy here rather than at
    // the pair's submission: at submission the copy has not been queued yet,
    // so the wait would park the consumer for a whole display period.
    // copy_fence_value is this pair's own D3D12FrameSynthesisTicket
    // fence_value - the value the held-back copy signals. It is a parameter
    // rather than read from the synthesiser's latest submission because more
    // than one pair can be in flight: the latest value may belong to a later
    // pair whose synthesis has not run, and joining the consumer to that
    // parks the real frame's hand-over for a whole synthesis cycle.
    [[nodiscard]] HRESULT flush_current_copy(
        ID3D12CommandQueue* consumer_queue,
        std::uint64_t copy_fence_value) noexcept;

    // Relinquishes the retained rolling source against its last GPU-use fence.
    // This is nonblocking and is required before history invalidation.
    [[nodiscard]] HRESULT retire_previous() noexcept;

    // Called after the runtime has paced the next application frame but before
    // that frame is returned to the application. Gives the previous complete
    // bridge transaction a bounded opportunity to leave the shared graphics
    // queue before the game records and submits its next frame. A timeout is
    // transient ERROR_BUSY; submission remains nonblocking and cannot enqueue
    // another transaction while this fence is pending.
    [[nodiscard]] HRESULT wait_for_previous_submission(
        std::uint32_t timeout_milliseconds) noexcept;

    [[nodiscard]] HRESULT wait_for_idle() noexcept;
    // Caller excludes presenter use and new history captures. Keeps XR destinations.
    [[nodiscard]] HRESULT reconfigure(D3D12OpticalFlowBackend backend,
        D3D12NvidiaOpticalFlowOptions options) noexcept;
    // The same rebuild with the backend and options it has, to turn the
    // diagnostic GPU timing on or off: its queries and readback are created
    // with the contexts, so the flight recorder switched on in a running
    // session has no timings until this runs. On failure the synthesizer is
    // left exactly as it was.
    [[nodiscard]] HRESULT reconfigure_gpu_timing(bool enabled) noexcept;
    [[nodiscard]] bool gpu_timing_enabled() noexcept;

    // Returns S_OK and consumes one completed NVIDIA timing record, S_FALSE
    // when none is ready, or an error for an invalid output pointer/readback.
    [[nodiscard]] HRESULT consume_nvidia_gpu_timing(
        D3D12NvidiaGpuTiming* timing) noexcept;

    // Makes another queue wait on this ticket's completion before its own
    // later work runs. Required when the synthesizer owns a queue of its own
    // and writes into swapchain images: an OpenXR D3D12 runtime synchronizes
    // those images against the queue the application supplied at session
    // create, and sees nothing submitted elsewhere. Joining the two queues
    // here is what makes the runtime's own ordering cover the synthesis. This
    // is a GPU-side wait queued on the target; it never blocks the CPU.
    [[nodiscard]] HRESULT synchronize_consumer_queue(
        ID3D12CommandQueue* queue,
        const D3D12FrameSynthesisTicket& ticket) noexcept;

    // The other half of that join, and the one a fence alone cannot be
    // assumed to cover. xrWaitSwapchainImage makes an image safe to write on
    // the queue the application supplied at session create; the runtime knows
    // nothing about a private synthesis queue and inserts no wait for it. So
    // the guarantee has to be carried across explicitly: the application's
    // queue signals once the acquire has returned, and the synthesis queue
    // waits on that before it writes. Without it the synthesis can land in an
    // image the compositor has not finished with, which shows up as a
    // synthetic that was generated, paced and complete and still never
    // reached the headset. Call once per pair, after the images are acquired
    // and before the pair is submitted.
    [[nodiscard]] HRESULT synchronize_producer_queue(
        ID3D12CommandQueue* queue) noexcept;

    // The fence a ticket's values are signalled on, AddRef'd into *fence.
    // For a caller that has to know whether a pair's output exists yet
    // without taking this object's lock: ID3D12Fence::GetCompletedValue is
    // safe from any thread, so the presenter can poll it while the
    // application thread is inside a submission.
    [[nodiscard]] HRESULT completion_fence(ID3D12Fence** fence) const noexcept;

    // Port display receipt: wait immediately before output writes, after NVIDIA packing/OFA.
    void output_consumer(ID3D12Fence*, std::uint64_t) noexcept;
    // Explicit reconstruction allocations (excludes driver-owned OFA context memory).
    [[nodiscard]] std::uint64_t allocated_bytes() const noexcept;
    [[nodiscard]] bool initialized() const noexcept;

    // The images are a flat panel's (V510): a HUD quad whose transparent
    // parts carry nothing, so the composition looks for the motion of the
    // content beside a pixel the flow left still. Set by the layer before
    // each of the panel's submissions; it outlives a reconfigure.
    void set_flat_panel(bool flat_panel) noexcept;

private:
    struct Impl;

    mutable std::mutex mutex_;
    std::unique_ptr<Impl> impl_;
    std::atomic<bool> flat_panel_{false};
};

// The composition's edge ghosting reduction as the ini sets it, for every
// synthesizer in the process; off on this branch whatever the ini says. (The
// repeating-pattern lock left with V507: not needed beside the hybrid.)
void set_composition_switches(bool edge_ghosting_reduction) noexcept;

// V532: the hybrid's depth search (V505), for every synthesizer in the
// process, from the tray's "Hybrid: depth search". Off until set; the live
// tuning window no longer switches it.
void set_hybrid_depth_search(bool enabled) noexcept;

}  // namespace xrfg
