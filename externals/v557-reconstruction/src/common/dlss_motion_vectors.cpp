#include "xrfg/dlss_motion_vectors.hpp"
#include "xrfg/bridge_flight_logger.hpp"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstring>
#include <limits>
#include <map>
#include <mutex>
#include <tuple>
#include <unordered_map>
#include <utility>
#include <vector>

namespace xrfg {
namespace {

using Microsoft::WRL::ComPtr;

constexpr std::uint64_t kMaximumStereoPublicationGap = 4;

// An eye image the size of a DLSS output, or (V524) cropped from it by a few
// texels at its far edges: R.E.A.L. VR's Cyberpunk 2077 runs DLSS at
// 4480x3760 and submits 4479x3760 eyes. An eye pixel is still the same output
// pixel, and the view lies inside the output as the draw requires. Two
// texels, or 1/256 of the extent where that is more - 17 at 4479 - is no
// mirror or spectator view.
[[nodiscard]] bool output_fits_extent(std::uint64_t output, std::uint64_t image) noexcept {
    return output >= image && output - image <= std::max<std::uint64_t>(2U, image / 256U);
}

// Where a camera stands along its own right vector; NaN without a camera.
[[nodiscard]] float camera_side(const DlssMotionVectorFrame& frame) noexcept {
    if (!frame.view.known) return std::numeric_limits<float>::quiet_NaN();
    const auto& p = frame.view.position;
    const auto& r = frame.view.right;
    return p[0] * r[0] + p[1] * r[1] + p[2] * r[2];
}

// V524: one shared stream's last two evaluations, both made before either
// eye's release, and where their cameras stood, for record 845. The earlier
// is the left eye's. The cameras do not decide: R.E.A.L. VR's Cyberpunk 2077
// gives Streamline the same camera for both eyes and moves the eye after it,
// so cameras that stand apart have moved with the player between the two
// evaluations - taking the one further left as the left eye swapped the eyes
// whenever the player moved. method 1 the cameras stand apart (a millimetre,
// or a part in a million of where they stand), 2 both known but not apart,
// 3 a camera unknown.
struct SharedStreamOrder {
    bool left_is_second{};
    unsigned method{};
    float first_side{};
    float second_side{};
};

[[nodiscard]] SharedStreamOrder shared_stream_eye_order(
    const DlssMotionVectorFrame& first, const DlssMotionVectorFrame& second) noexcept {
    SharedStreamOrder order;
    order.first_side = camera_side(first);
    order.second_side = camera_side(second);
    if (!std::isfinite(order.first_side) || !std::isfinite(order.second_side)) {
        order.method = 3;
        return order;
    }
    const float apart = std::max(1.0e-3F,
        1.0e-6F * std::max(std::fabs(order.first_side), std::fabs(order.second_side)));
    if (std::fabs(order.first_side - order.second_side) <= apart) {
        order.method = 2;
        return order;
    }
    order.method = 1;
    return order;
}

// 845 (V524): an eye's choice from one shared stream evaluated twice before
// either release, the first 512 and every 2000th after: a the eye << 32 | the
// chosen publication, b the earlier evaluation's camera side (float bits)
// << 32 | the later's, c the method | bit 8 the later is the left eye | bit
// 9 the chosen is the later.
void record_shared_stream_choice(std::uint64_t eye, const SharedStreamOrder& order,
    const DlssMotionVectorFrame& chosen, bool chosen_is_second) noexcept {
    static std::atomic<std::uint64_t> records{0};
    const std::uint64_t record = records.fetch_add(1, std::memory_order_relaxed);
    if (record >= 512 && record % 2000 != 0) return;
    try {
        std::uint32_t first_bits = 0, second_bits = 0;
        std::memcpy(&first_bits, &order.first_side, sizeof(first_bits));
        std::memcpy(&second_bits, &order.second_side, sizeof(second_bits));
        bridge_flight_logger().event(BridgeFlightOperation::presenter_transition, 845,
            (eye << 32U) | (chosen.publication & 0xFFFFFFFFULL),
            (static_cast<std::uint64_t>(first_bits) << 32U) | second_bits,
            order.method | (order.left_is_second ? 1U << 8U : 0U) |
                (chosen_is_second ? 1U << 9U : 0U));
    } catch (...) {
    }
}
// V542: eight under alternating eyes, an eye's guide up to two of its own
// evaluations behind its newest (set_dlss_alternating_skips).
constexpr std::size_t kSnapshotSlotCount = 8;
// The snapshot ring's length: four, and six under alternating eyes (V524),
// whose lagging eye takes the evaluation three before the newest - its
// snapshot has to outlast the two evaluations of the frame after.
[[nodiscard]] std::size_t snapshot_ring_length() noexcept;
constexpr D3D12_RESOURCE_STATES kSnapshotReadState =
    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE |
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;

struct SnapshotSlot {
    ComPtr<ID3D12Resource> resource;
    D3D12_RESOURCE_DESC description{};
    D3D12_RESOURCE_STATES state{D3D12_RESOURCE_STATE_COMMON};
};

struct StreamState {
    std::uint64_t epoch{1};
    std::uint64_t serial{};
    std::uint64_t first_publication{};
    float jitter_x{};
    float jitter_y{};
    std::shared_ptr<const DlssMotionVectorFrame> previous;
    std::shared_ptr<const DlssMotionVectorFrame> latest;
    // V524, alternating eyes: the two before `previous`; V542, two more.
    std::shared_ptr<const DlssMotionVectorFrame> previous2;
    std::shared_ptr<const DlssMotionVectorFrame> previous3;
    std::shared_ptr<const DlssMotionVectorFrame> previous4;
    std::shared_ptr<const DlssMotionVectorFrame> previous5;
    // V524, alternating eyes: each eye's evaluation at this frame's release
    // watermark and at the frame before (the resolver's).
    struct EyeTrack {
        std::uint64_t watermark{};
        std::shared_ptr<const DlssMotionVectorFrame> current;
        std::shared_ptr<const DlssMotionVectorFrame> previous;
    };
    std::array<EyeTrack, 2> eyes{};
    std::array<SnapshotSlot, kSnapshotSlotCount> snapshots;
    std::size_t next_snapshot{};
    std::array<SnapshotSlot, kSnapshotSlotCount> depth_snapshots;
    std::size_t next_depth_snapshot{};
    // The queue the stream's snapshot copies run on (V519): the game's for a
    // native D3D12 session, the layer's for a bridged one.
    ComPtr<ID3D12CommandQueue> producer_queue;
    // V524: alternating eyes, the epoch the eyes were anchored in and the
    // left eye's serial parity (anchor_dlss_alternating_eyes).
    std::uint64_t anchor_epoch{};
    std::uint32_t left_parity{};
};

// Snapshot textures the registry has let go of (V519): a retired stream, a
// slot reallocated for a new size, the streams of a disabled capture. Their
// copies may still be queued on the producer queue - in a bridged session
// behind a wait for the game's D3D11 or Vulkan work - and freeing a texture
// the GPU will still write corrupts memory: xrfg_ngx_guide_capture_tests
// releasing a feature right after its evaluations took the PC down three
// times (bugcheck 0x139 in NVIDIA's kernel driver, then two hard freezes),
// the same bugcheck as games releasing DLSS at a VR exit or an upscaler
// switch. They are kept until a fence signalled on the producer queue when
// they were let go has passed, and kRetiredGrace has gone by: on a native
// D3D12 session the copy sits in the game's command list, which the game may
// submit after that signal.
constexpr std::chrono::milliseconds kRetiredGrace{2000};
constexpr std::chrono::milliseconds kRetiredGraceWithoutFence{10000};

struct RetiredSnapshots {
    StreamState stream;
    ComPtr<ID3D12Resource> resource;
    ComPtr<ID3D12Fence> fence;
    std::chrono::steady_clock::time_point retired_at;
};

struct Registry {
    std::mutex mutex;
    std::unordered_map<std::uint64_t, StreamState> streams;
    std::vector<RetiredSnapshots> retired;
    DlssMotionVectorStatistics statistics;
    // V533: set_dlss_eye_view.
    std::array<DlssEyeView, kDlssMotionVectorEyeCount> eye_views{};
    std::array<bool, kDlssMotionVectorEyeCount> eye_view_known{};
    // V540: the alternating eyes' tags (anchor_dlss_alternating_eye_tags).
    std::uint64_t left_eye_tag{};
    std::uint64_t right_eye_tag{};
    // V542: set_dlss_alternating_skips - the newest evaluation's eye's, the
    // other's.
    std::array<std::uint32_t, 2> alternating_skips{0, 1};
    // V546: the measured eye order of two streams nothing else tells apart
    // (report_dlss_eye_order), by the pair's lower and higher stream.
    struct EyeOrder {
        // The pixels that favoured each stream as the left eye's.
        std::uint64_t lower_left_votes{};
        std::uint64_t higher_left_votes{};
        std::uint64_t measurements{};
        std::uint64_t left{};
    };
    std::map<std::pair<std::uint64_t, std::uint64_t>, EyeOrder> eye_orders;
};

Registry& registry() {
    static Registry value;
    return value;
}

std::atomic_bool g_tracking_enabled{false};
std::atomic_bool g_alternating_eyes{false};

std::size_t snapshot_ring_length() noexcept {
    return g_alternating_eyes.load(std::memory_order_acquire) ? kSnapshotSlotCount : 4U;
}

ComPtr<IUnknown> identity(IUnknown* object) noexcept {
    ComPtr<IUnknown> value;
    if (object != nullptr) {
        static_cast<void>(object->QueryInterface(IID_PPV_ARGS(value.GetAddressOf())));
    }
    return value;
}

bool same_identity(IUnknown* left, IUnknown* right) noexcept {
    const auto a = identity(left);
    const auto b = identity(right);
    return a && b && a.Get() == b.Get();
}

// Reserved for the short, serialized backing challenge; never frame pixels.
constexpr GUID kBackingChallengeGuid{
    0xf1d70763, 0xc534, 0x4b89, {0xa8, 0x43, 0x64, 0x42, 0x9c, 0x97, 0x18, 0x5b}};

struct BackingChallenge {
    std::uint64_t left{}, right{}, phase{};
    bool operator==(const BackingChallenge&) const = default;
};

// Same backing through two interfaces: a value set through one is read back
// through the other, both ways, then both are cleared again. Only runs when
// neither interface holds a value under the reserved key already.
bool prove_same_backing(ID3D12Device* left, ID3D12Device* right, IUnknown* a, IUnknown* b) noexcept {
    UINT size = 0;
    const HRESULT left_empty = left->GetPrivateData(kBackingChallengeGuid, &size, nullptr);
    size = 0;
    const HRESULT right_empty = right->GetPrivateData(kBackingChallengeGuid, &size, nullptr);
    if (left_empty != DXGI_ERROR_NOT_FOUND || right_empty != DXGI_ERROR_NOT_FOUND) {
        return false;
    }
    BackingChallenge sent{reinterpret_cast<std::uintptr_t>(a), reinterpret_cast<std::uintptr_t>(b), 1};
    BackingChallenge received{};
    bool forward = false, backward = false;
    if (SUCCEEDED(left->SetPrivateData(kBackingChallengeGuid, sizeof(sent), &sent))) {
        size = sizeof(received);
        forward = SUCCEEDED(right->GetPrivateData(kBackingChallengeGuid, &size, &received)) &&
            size == sizeof(sent) && received == sent;
    }
    if (forward) {
        sent.phase = 2;
        received = {};
        if (SUCCEEDED(right->SetPrivateData(kBackingChallengeGuid, sizeof(sent), &sent))) {
            size = sizeof(received);
            backward = SUCCEEDED(left->GetPrivateData(kBackingChallengeGuid, &size, &received)) &&
                size == sizeof(sent) && received == sent;
        }
    }
    static_cast<void>(left->SetPrivateData(kBackingChallengeGuid, 0, nullptr));
    static_cast<void>(right->SetPrivateData(kBackingChallengeGuid, 0, nullptr));
    return forward && backward;
}

struct BackingCache {
    std::mutex mutex;
    struct Entry {
        ComPtr<IUnknown> left, right;
        bool same{};
    };
    std::array<Entry, 8> entries{};
    std::size_t count{};
};

BackingCache& backing_cache() {
    static BackingCache value;
    return value;
}

}  // namespace

bool same_d3d12_device_backing(ID3D12Device* left, ID3D12Device* right) noexcept {
    try {
        if (left == nullptr || right == nullptr) return false;
        const auto a = identity(left);
        const auto b = identity(right);
        if (!a || !b) return false;
        if (a.Get() == b.Get()) return true;
        const LUID al = left->GetAdapterLuid();
        const LUID bl = right->GetAdapterLuid();
        if (al.LowPart != bl.LowPart || al.HighPart != bl.HighPart) return false;
        auto& cache = backing_cache();
        std::scoped_lock lock(cache.mutex);
        for (std::size_t index = 0; index < cache.count; ++index) {
            const auto& entry = cache.entries[index];
            if ((entry.left.Get() == a.Get() && entry.right.Get() == b.Get()) ||
                (entry.left.Get() == b.Get() && entry.right.Get() == a.Get())) {
                return entry.same;
            }
        }
        if (cache.count == cache.entries.size()) return false;  // bounded; never guess
        auto& entry = cache.entries[cache.count++];
        entry.left = a;   // pinned: no pointer reuse behind the cache
        entry.right = b;
        entry.same = prove_same_backing(left, right, a.Get(), b.Get());
        return entry.same;
    } catch (...) {
        return false;
    }
}

namespace {

bool valid(const DlssMotionVectorPublication& publication) noexcept {
    if (publication.stream == 0 || publication.output == nullptr ||
        publication.motion_vectors == nullptr ||
        publication.producer_queue == nullptr ||
        publication.producer_command_list == nullptr || publication.output_width == 0 ||
        publication.output_height == 0 || publication.motion_width == 0 ||
        publication.motion_height == 0 || !std::isfinite(publication.scale_x) ||
        !std::isfinite(publication.scale_y) || !std::isfinite(publication.jitter_x) ||
        !std::isfinite(publication.jitter_y)) {
        return false;
    }
    const auto output = publication.output->GetDesc();
    const auto motion = publication.motion_vectors->GetDesc();
    const bool base_valid = output.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D &&
        motion.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D &&
        static_cast<std::uint64_t>(publication.output_x) + publication.output_width <=
            output.Width &&
        static_cast<std::uint64_t>(publication.output_y) + publication.output_height <=
            output.Height &&
        static_cast<std::uint64_t>(publication.motion_x) + publication.motion_width <=
            motion.Width &&
        static_cast<std::uint64_t>(publication.motion_y) + publication.motion_height <=
            motion.Height;
    return base_valid;
}

// The optional depth, judged on its own (V505): a depth the layer cannot use
// leaves the vectors published without it rather than refused.
bool valid_depth(const DlssMotionVectorPublication& publication) noexcept {
    if (publication.depth == nullptr) return false;
    const auto depth = publication.depth->GetDesc();
    return depth.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D &&
        publication.depth_width != 0 && publication.depth_height != 0 &&
        static_cast<std::uint64_t>(publication.depth_x) + publication.depth_width <=
            depth.Width &&
        static_cast<std::uint64_t>(publication.depth_y) + publication.depth_height <=
            depth.Height &&
        std::isfinite(publication.frame_time_delta_ms) &&
        publication.frame_time_delta_ms > 0.0F &&
        std::isfinite(publication.camera_near) &&
        std::isfinite(publication.camera_far);
}

bool matching_description(
    const D3D12_RESOURCE_DESC& left,
    const D3D12_RESOURCE_DESC& right) noexcept {
    return left.Dimension == right.Dimension && left.Alignment == right.Alignment &&
        left.Width == right.Width && left.Height == right.Height &&
        left.DepthOrArraySize == right.DepthOrArraySize &&
        left.MipLevels == right.MipLevels && left.Format == right.Format &&
        left.SampleDesc.Count == right.SampleDesc.Count &&
        left.SampleDesc.Quality == right.SampleDesc.Quality &&
        left.Layout == right.Layout && left.Flags == right.Flags;
}

D3D12_RESOURCE_BARRIER transition_barrier(
    ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before,
    D3D12_RESOURCE_STATES after) noexcept {
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = resource;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = before;
    barrier.Transition.StateAfter = after;
    return barrier;
}

// A fence signalled on the queue after everything queued there so far, or
// null when the queue will not take one.
ComPtr<ID3D12Fence> signal_after_queued_work(ID3D12CommandQueue* queue) noexcept {
    if (queue == nullptr) return {};
    ComPtr<ID3D12Device> device;
    if (FAILED(queue->GetDevice(IID_PPV_ARGS(device.GetAddressOf())))) return {};
    ComPtr<ID3D12Fence> fence;
    if (FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(fence.GetAddressOf())))) {
        return {};
    }
    if (FAILED(queue->Signal(fence.Get(), 1))) return {};
    return fence;
}

// Registry mutex held.
void retire_snapshots(Registry& state, StreamState stream, ComPtr<ID3D12Resource> resource,
    ID3D12CommandQueue* queue) {
    RetiredSnapshots retired;
    retired.fence = signal_after_queued_work(queue);
    retired.stream = std::move(stream);
    retired.resource = std::move(resource);
    retired.retired_at = std::chrono::steady_clock::now();
    state.retired.push_back(std::move(retired));
}

// Registry mutex held: frees what the GPU is done with.
void release_finished_snapshots(Registry& state) noexcept {
    if (state.retired.empty()) return;
    const auto now = std::chrono::steady_clock::now();
    std::erase_if(state.retired, [&](const RetiredSnapshots& retired) {
        const auto age = now - retired.retired_at;
        if (!retired.fence) return age >= kRetiredGraceWithoutFence;
        return age >= kRetiredGrace && retired.fence->GetCompletedValue() >= 1;
    });
}

HRESULT snapshot_resource(
    std::array<SnapshotSlot, kSnapshotSlotCount>& snapshots,
    std::size_t& next_snapshot,
    ID3D12Resource* source,
    D3D12_RESOURCE_STATES source_state,
    ID3D12GraphicsCommandList* producer_command_list,
    ID3D12CommandQueue* producer_queue,
    ID3D12Device* verified_producer_device,
    ComPtr<ID3D12Resource>* snapshot,
    ComPtr<ID3D12Resource>* replaced) noexcept {
    if (source == nullptr || producer_command_list == nullptr ||
        producer_queue == nullptr || snapshot == nullptr) return E_POINTER;
    snapshot->Reset();

    ComPtr<ID3D12Device> source_device;
    ComPtr<ID3D12Device> command_device;
    ComPtr<ID3D12Device> queue_device;
    HRESULT result = source->GetDevice(
        IID_PPV_ARGS(source_device.GetAddressOf()));
    if (FAILED(result)) return result;
    result = producer_command_list->GetDevice(
        IID_PPV_ARGS(command_device.GetAddressOf()));
    if (FAILED(result)) return result;
    result = producer_queue->GetDevice(
        IID_PPV_ARGS(queue_device.GetAddressOf()));
    if (FAILED(result)) return result;
    ComPtr<ID3D12Device> allocation_device;
    if (verified_producer_device != nullptr) {
        // The producer has already unwrapped and identity-checked all related
        // objects. Comparing their reported wrapper devices here would
        // recreate the Streamline false mismatch this field avoids.
        allocation_device = verified_producer_device;
    } else {
        // Streamline's proxies report another device than the resource.
        if (!same_d3d12_device_backing(source_device.Get(), command_device.Get()) ||
            !same_d3d12_device_backing(source_device.Get(), queue_device.Get())) {
            return E_INVALIDARG;
        }
        allocation_device = source_device;
    }

    const D3D12_RESOURCE_DESC description = source->GetDesc();
    SnapshotSlot& slot = snapshots[next_snapshot];
    if (!slot.resource || !matching_description(slot.description, description)) {
        D3D12_HEAP_PROPERTIES properties{};
        properties.Type = D3D12_HEAP_TYPE_DEFAULT;
        properties.CreationNodeMask = 1;
        properties.VisibleNodeMask = 1;
        ComPtr<ID3D12Resource> resource;
        result = allocation_device->CreateCommittedResource(
            &properties,
            D3D12_HEAP_FLAG_NONE,
            &description,
            D3D12_RESOURCE_STATE_COMMON,
            nullptr,
            IID_PPV_ARGS(resource.GetAddressOf()));
        if (FAILED(result)) return result;
        // The texture it replaces may still be written by a queued copy.
        if (replaced != nullptr) *replaced = std::move(slot.resource);
        slot.resource = std::move(resource);
        slot.description = description;
        slot.state = D3D12_RESOURCE_STATE_COMMON;
    }

    std::array<D3D12_RESOURCE_BARRIER, 2> before{};
    UINT before_count = 0;
    if (source_state != D3D12_RESOURCE_STATE_COPY_SOURCE) {
        before[before_count++] = transition_barrier(
            source,
            source_state,
            D3D12_RESOURCE_STATE_COPY_SOURCE);
    }
    if (slot.state != D3D12_RESOURCE_STATE_COPY_DEST) {
        before[before_count++] = transition_barrier(
            slot.resource.Get(), slot.state, D3D12_RESOURCE_STATE_COPY_DEST);
    }
    if (before_count != 0) {
        producer_command_list->ResourceBarrier(before_count, before.data());
    }
    producer_command_list->CopyResource(slot.resource.Get(), source);

    std::array<D3D12_RESOURCE_BARRIER, 2> after{};
    UINT after_count = 0;
    if (source_state != D3D12_RESOURCE_STATE_COPY_SOURCE) {
        after[after_count++] = transition_barrier(
            source,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            source_state);
    }
    after[after_count++] = transition_barrier(
        slot.resource.Get(), D3D12_RESOURCE_STATE_COPY_DEST, kSnapshotReadState);
    producer_command_list->ResourceBarrier(after_count, after.data());
    slot.state = kSnapshotReadState;
    *snapshot = slot.resource;
    next_snapshot = (next_snapshot + 1) % snapshot_ring_length();
    return S_OK;
}

struct Candidate {
    std::uint64_t first_publication{};
    std::shared_ptr<const DlssMotionVectorFrame> frame;
    std::shared_ptr<const DlssMotionVectorFrame> previous;
};

// Where an eye's DLSS inputs sit in their render targets. Both eyes often
// share one double-wide target with the left eye on the left - UEVR's depth,
// for one - so the eye further left is eye 0 whichever evaluates first.
//
// V553: the depth's offset first, where the publication carries a depth, then
// the vectors' and the output's. V497 read only the last two, and UEVR's
// Bodycam ties on both - each eye's vectors in a texture of its own at 0,
// each output at 0 - so two streams sorted by first seen; its depth is one
// double-wide target, the right eye's rectangle at x = 1392 of 2784. After a
// death or a level change the engine recreated both features with the right
// eye's first, each eye took the other's vectors (record 860: eye 0 without
// usable depth, eye 1 with it), and a Virtual Desktop restart, which starts
// the order afresh, put them back. OFXR 0.3 orders by the depth's offset.
[[nodiscard]] std::tuple<std::uint32_t, std::uint32_t, std::uint32_t> input_offset(
    const DlssMotionVectorFrame& frame) noexcept {
    return {frame.depth ? frame.depth_x : 0U, frame.motion_x, frame.output_x};
}

// 828: the resolver's running totals, every 300 resolves. a matched << 32 |
// stale pairs, b queue mismatches << 32 | missing streams, c used << 32 |
// hybrid pairs.
void record_resolver_statistics(const DlssMotionVectorStatistics& statistics) noexcept {
    if (statistics.resolve_calls % 300 != 0) return;
    try {
        bridge_flight_logger().event(
            BridgeFlightOperation::embedded_configuration, 828,
            (statistics.matched << 32U) | (statistics.resolve_stale_pairs & 0xFFFFFFFFULL),
            (statistics.resolve_queue_mismatches << 32U) |
                (statistics.resolve_missing_streams & 0xFFFFFFFFULL),
            (statistics.used << 32U) | (statistics.hybrid_used & 0xFFFFFFFFULL));
    } catch (...) {
    }
}

// V547, 869: how a swapchain-per-eye session's two untold streams went to an
// eye at its release, the first 16 and every 600th after: a the eye << 40 |
// the path << 32 | the chosen stream's low 32 bits, b the paths so far: 0
// undecided, both evaluations before either release << 32 | 1 undecided, one
// between the releases; c 2 decided, both before << 32 | 3 decided, one
// between. V548: a bit 44 where the eye took its stream's previous
// evaluation, not its newest, and the count of those so far in b's bits
// 48-63 (16 bits, saturating).
void record_pair_path(std::uint64_t eye, unsigned path, bool behind,
    const DlssMotionVectorFrame* chosen) noexcept {
    if (chosen == nullptr || path > 3) return;
    static std::array<std::atomic<std::uint64_t>, 4> paths{};
    static std::atomic<std::uint64_t> behinds{0};
    static std::atomic<std::uint64_t> records{0};
    paths[path].fetch_add(1, std::memory_order_relaxed);
    if (behind) behinds.fetch_add(1, std::memory_order_relaxed);
    const std::uint64_t record = records.fetch_add(1, std::memory_order_relaxed);
    if (record >= 16 && record % 600 != 0) return;
    const auto count = [&](unsigned index) {
        return paths[index].load(std::memory_order_relaxed) & 0xFFFFFFFFULL;
    };
    try {
        bridge_flight_logger().event(BridgeFlightOperation::embedded_configuration, 869,
            (eye << 40U) | (behind ? 1ULL << 44U : 0ULL) |
                (static_cast<std::uint64_t>(path) << 32U) | (chosen->stream & 0xFFFFFFFFULL),
            (std::min<std::uint64_t>(behinds.load(std::memory_order_relaxed), 0xFFFFULL) << 48U) |
                ((count(0) & 0xFFFFULL) << 32U) | count(1),
            (count(2) << 32U) | count(3));
    } catch (...) {
    }
}

}  // namespace

void publish_dlss_motion_vectors(
    const DlssMotionVectorPublication& publication) noexcept {
    try {
        if (!g_tracking_enabled.load(std::memory_order_acquire)) return;
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        release_finished_snapshots(state);
        if (!valid(publication)) {
            ++state.statistics.invalid_publications;
            state.statistics.status = DlssMotionVectorStatus::invalid_input;
            return;
        }

        auto& stream = state.streams[publication.stream];
        if (publication.reset) {
            ++stream.epoch;
            stream.serial = 0;
            stream.previous.reset();
            stream.latest.reset();
            stream.previous2.reset();
            stream.previous3.reset();
            stream.previous4.reset();
            stream.previous5.reset();
        }

        ComPtr<ID3D12Resource> motion_snapshot;
        ComPtr<ID3D12Resource> replaced;
        const HRESULT motion_result = snapshot_resource(stream.snapshots, stream.next_snapshot,
                publication.motion_vectors, publication.resource_state,
                publication.producer_command_list, publication.producer_queue,
                publication.verified_producer_device,
                &motion_snapshot, &replaced);
        if (replaced) {
            retire_snapshots(state, {}, std::move(replaced), stream.producer_queue.Get());
        }
        if (SUCCEEDED(motion_result)) stream.producer_queue = publication.producer_queue;
        if (FAILED(motion_result)) {
            ++state.statistics.snapshot_failures;
            state.statistics.status = DlssMotionVectorStatus::invalid_input;
            return;
        }
        ++state.statistics.snapshot_copies;

        ComPtr<ID3D12Resource> depth_snapshot;
        if (valid_depth(publication)) {
            const HRESULT depth_result = snapshot_resource(stream.depth_snapshots,
                    stream.next_depth_snapshot, publication.depth,
                    publication.depth_resource_state,
                    publication.producer_command_list,
                    publication.producer_queue,
                    publication.verified_producer_device, &depth_snapshot, &replaced);
            if (replaced) {
                retire_snapshots(state, {}, std::move(replaced), stream.producer_queue.Get());
            }
            if (FAILED(depth_result)) {
                // The vectors go on without it.
                ++state.statistics.snapshot_failures;
                depth_snapshot.Reset();
            } else {
                ++state.statistics.snapshot_copies;
                ++state.statistics.depth_publications;
            }
        }

        auto frame = std::make_shared<DlssMotionVectorFrame>();
        frame->stream = publication.stream;
        frame->epoch = stream.epoch;
        frame->previous_serial = stream.serial;
        frame->serial = ++stream.serial;
        frame->publication = ++state.statistics.published;
        frame->output = publication.output;
        frame->motion_vectors = std::move(motion_snapshot);
        frame->producer_queue = publication.producer_queue;
        frame->output_x = publication.output_x;
        frame->output_y = publication.output_y;
        frame->output_width = publication.output_width;
        frame->output_height = publication.output_height;
        frame->output_slice = 0;
        frame->motion_x = publication.motion_x;
        frame->motion_y = publication.motion_y;
        frame->motion_width = publication.motion_width;
        frame->motion_height = publication.motion_height;
        frame->motion_slice = 0;
        frame->scale_x = publication.scale_x == 0.0F ? 1.0F : publication.scale_x;
        frame->scale_y = publication.scale_y == 0.0F ? 1.0F : publication.scale_y;
        frame->jitter_x = publication.jitter_x;
        frame->jitter_y = publication.jitter_y;
        frame->previous_jitter_x = stream.jitter_x;
        frame->previous_jitter_y = stream.jitter_y;
        frame->resource_state = kSnapshotReadState;
        frame->jittered = publication.jittered;
        frame->reset = publication.reset;
        frame->depth = std::move(depth_snapshot);
        frame->depth_x = publication.depth_x;
        frame->depth_y = publication.depth_y;
        frame->depth_width = publication.depth_width;
        frame->depth_height = publication.depth_height;
        frame->depth_resource_state = kSnapshotReadState;
        frame->frame_time_delta_ms = publication.frame_time_delta_ms;
        frame->camera_near = publication.camera_near;
        frame->camera_far = publication.camera_far;
        frame->depth_inverted = publication.depth_inverted;
        frame->depth_infinite = publication.depth_infinite;
        frame->view = publication.view;
        frame->eye_tag = publication.eye_tag;
        // V516: the stream's previous camera (a reset above cleared it).
        if (stream.latest) frame->previous_view = stream.latest->view;
        stream.jitter_x = publication.jitter_x;
        stream.jitter_y = publication.jitter_y;
        if (stream.first_publication == 0) {
            stream.first_publication = frame->publication;
        }
        stream.previous5 = std::move(stream.previous4);
        stream.previous4 = std::move(stream.previous3);
        stream.previous3 = std::move(stream.previous2);
        stream.previous2 = std::move(stream.previous);
        stream.previous = std::move(stream.latest);
        stream.latest = std::move(frame);
        if (state.statistics.used == 0) {
            state.statistics.status = DlssMotionVectorStatus::output_not_direct;
        }
    } catch (...) {
        report_dlss_motion_vector_status(DlssMotionVectorStatus::invalid_input);
    }
}

void configure_dlss_motion_vector_tracking(bool enabled) noexcept {
    const bool previous = g_tracking_enabled.exchange(enabled, std::memory_order_acq_rel);
    if (enabled == previous) return;
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        if (!enabled) {
            // Do not release the private resources here. The desired setting is
            // visible to DLSS before the OpenXR layer reaches xrEndFrame and its
            // GPU-idle fence, so snapshots may still be referenced by submitted
            // synthesis work. The layer retires them explicitly after that fence.
            state.statistics.status = DlssMotionVectorStatus::disabled;
            return;
        }
        // Re-enabling without a layer-side retirement must not expose stale
        // vectors. Preserve the slot resources (and therefore any in-flight GPU
        // lifetime), but start fresh stream epochs and temporal ownership.
        for (auto& [unused_stream_id, stream] : state.streams) {
            static_cast<void>(unused_stream_id);
            ++stream.epoch;
            stream.serial = 0;
            stream.first_publication = 0;
            stream.jitter_x = 0.0F;
            stream.jitter_y = 0.0F;
            stream.previous.reset();
            stream.latest.reset();
            stream.previous2.reset();
            stream.previous3.reset();
            stream.previous4.reset();
            stream.previous5.reset();
        }
        state.statistics = {};
        state.statistics.status = DlssMotionVectorStatus::waiting_for_dlss;
    } catch (...) {
    }
}

void retire_disabled_dlss_motion_vector_resources() noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        if (g_tracking_enabled.load(std::memory_order_acquire)) return;
        // The consumer queue is idle here, the producer queues not always
        // (a native session's game queue): through the retired list.
        for (auto& [unused_stream_id, stream] : state.streams) {
            static_cast<void>(unused_stream_id);
            ComPtr<ID3D12CommandQueue> queue = stream.producer_queue;
            if (queue) retire_snapshots(state, std::move(stream), {}, queue.Get());
        }
        state.streams.clear();
        state.eye_orders.clear();
        release_finished_snapshots(state);
        state.statistics = {};
        state.statistics.status = DlssMotionVectorStatus::disabled;
    } catch (...) {
    }
}

void retire_dlss_motion_vector_stream(std::uint64_t stream) noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        const auto found = state.streams.find(stream);
        if (found == state.streams.end()) return;
        ComPtr<ID3D12CommandQueue> queue = found->second.producer_queue;
        // Never published: no copy of it was ever queued.
        if (queue) retire_snapshots(state, std::move(found->second), {}, queue.Get());
        state.streams.erase(found);
        // V546: a pair with this stream in it is a pair no more.
        std::erase_if(state.eye_orders, [&](const auto& entry) {
            return entry.first.first == stream || entry.first.second == stream;
        });
        release_finished_snapshots(state);
    } catch (...) {
    }
}

std::shared_ptr<const DlssMotionVectorFrame> split_both_eyes_guide(
    const DlssMotionVectorFrame& frame,
    std::uint32_t view_width,
    std::uint32_t view_height,
    std::uint32_t eye) noexcept {
    try {
        if (eye > 1 || view_width == 0 || view_height == 0 || frame.output_x != 0 ||
            frame.output_y != 0 || frame.output_width != view_width * 2U ||
            frame.output_height != view_height || frame.motion_width < 2) {
            return {};
        }
        auto half = std::make_shared<DlssMotionVectorFrame>(frame);
        half->output_width = view_width;
        const std::uint32_t motion_half = frame.motion_width / 2U;
        half->motion_x = frame.motion_x + eye * motion_half;
        half->motion_width = motion_half;
        if (frame.depth && frame.depth_width >= 2) {
            const std::uint32_t depth_half = frame.depth_width / 2U;
            half->depth_x = frame.depth_x + eye * depth_half;
            half->depth_width = depth_half;
        }
        return half;
    } catch (...) {
        return {};
    }
}

namespace {

// How far a guide's camera may fall short of an eye's field and still be
// taken to cover it: a hundredth of the field each side.
constexpr double kViewCoverTolerance = 0.01;
// A stream with a camera is recent while its newest evaluation is within
// this many publications of the newest of all: MSFS 2024's foveated
// rendering publishes four a frame, and a stream the game stopped
// evaluating (a menu's, the desktop mirror's) keeps its last frame.
constexpr std::uint64_t kMaximumViewPublicationGap = 8;

[[nodiscard]] double view_fit_error(const DlssViewGeometry& g, const DlssEyeView& eye) noexcept {
    return std::fabs(static_cast<double>(g.tan_left) - eye.tan_left) +
        std::fabs(static_cast<double>(g.tan_right) - eye.tan_right) +
        std::fabs(static_cast<double>(g.tan_up) - eye.tan_up) +
        std::fabs(static_cast<double>(g.tan_down) - eye.tan_down);
}

// Along the two cameras' mean right vector: the smaller is the left eye.
[[nodiscard]] double camera_side(const DlssViewGeometry& g, const std::array<float, 3>& right) noexcept {
    return static_cast<double>(g.position[0]) * right[0] +
        static_cast<double>(g.position[1]) * right[1] +
        static_cast<double>(g.position[2]) * right[2];
}

[[nodiscard]] double camera_distance(const DlssViewGeometry& a, const DlssViewGeometry& b) noexcept {
    const double x = static_cast<double>(a.position[0]) - b.position[0];
    const double y = static_cast<double>(a.position[1]) - b.position[1];
    const double z = static_cast<double>(a.position[2]) - b.position[2];
    return std::sqrt(x * x + y * y + z * z);
}

// An inset's field (V516): inside the eye's, to the cover tolerance, and
// clearly narrower than it - MSFS 2024's is FoveatedRenderingScale of it.
[[nodiscard]] bool inside_eye_field(const DlssViewGeometry& g, const DlssEyeView& eye) noexcept {
    const double eye_width = static_cast<double>(eye.tan_right) - eye.tan_left;
    const double eye_height = static_cast<double>(eye.tan_up) - eye.tan_down;
    const double width = static_cast<double>(g.tan_right) - g.tan_left;
    const double height = static_cast<double>(g.tan_up) - g.tan_down;
    const double slack_x = kViewCoverTolerance * eye_width;
    const double slack_y = kViewCoverTolerance * eye_height;
    return g.known && eye_width > 0.0 && eye_height > 0.0 && width > 0.0 && height > 0.0 &&
        width < 0.9 * eye_width && height < 0.9 * eye_height &&
        g.tan_left >= eye.tan_left - slack_x && g.tan_right <= eye.tan_right + slack_x &&
        g.tan_up <= eye.tan_up + slack_y && g.tan_down >= eye.tan_down - slack_y;
}

// A stream's evaluation for an eye's release: its newest by the watermark.
[[nodiscard]] std::shared_ptr<const DlssMotionVectorFrame> evaluation_by_release(
    const Candidate& candidate, std::uint64_t released_at) noexcept {
    if (released_at == 0 || candidate.frame->publication <= released_at) return candidate.frame;
    return candidate.previous && candidate.previous->epoch == candidate.frame->epoch &&
            candidate.previous->publication <= released_at
        ? candidate.previous : nullptr;
}

}  // namespace

std::array<float, 4> view_rect_in_eye(
    const DlssViewGeometry& view,
    const DlssEyeView& eye) noexcept {
    const double eye_width = static_cast<double>(eye.tan_right) - eye.tan_left;
    const double eye_height = static_cast<double>(eye.tan_up) - eye.tan_down;
    if (!view.known || !(eye_width > 0.0) || !(eye_height > 0.0)) return {};
    const double x = eye.x + (view.tan_left - eye.tan_left) / eye_width * eye.width;
    const double width = (static_cast<double>(view.tan_right) - view.tan_left) / eye_width * eye.width;
    const double y = eye.y + (eye.tan_up - view.tan_up) / eye_height * eye.height;
    const double height = (static_cast<double>(view.tan_up) - view.tan_down) / eye_height * eye.height;
    if (!std::isfinite(x) || !std::isfinite(y) || !(width > 0.0) || !(height > 0.0)) return {};
    return {static_cast<float>(x), static_cast<float>(y), static_cast<float>(width),
        static_cast<float>(height)};
}

std::shared_ptr<const DlssMotionVectorFrame> place_guide_in_view(
    const DlssMotionVectorFrame& frame,
    const DlssEyeView& view) noexcept {
    try {
        const DlssViewGeometry& g = frame.view;
        const double eye_width = static_cast<double>(view.tan_right) - view.tan_left;
        const double eye_height = static_cast<double>(view.tan_up) - view.tan_down;
        if (!g.known || view.x < 0 || view.y < 0 || view.width == 0 || view.height == 0 ||
            !(eye_width > 0.0) || !(eye_height > 0.0) || frame.motion_width == 0 ||
            frame.motion_height == 0) {
            return {};
        }
        // The guide's edges in the image's pixels.
        const double x0 = view.x, y0 = view.y;
        const double x1 = x0 + view.width, y1 = y0 + view.height;
        const double left = x0 + (g.tan_left - view.tan_left) / eye_width * view.width;
        const double right = x0 + (g.tan_right - view.tan_left) / eye_width * view.width;
        const double top = y0 + (view.tan_up - g.tan_up) / eye_height * view.height;
        const double bottom = y0 + (view.tan_up - g.tan_down) / eye_height * view.height;
        const double slack_x = kViewCoverTolerance * view.width;
        const double slack_y = kViewCoverTolerance * view.height;
        // Written so that a NaN refuses.
        if (!(right - left > 1.0) || !(bottom - top > 1.0) || !(left <= x0 + slack_x) ||
            !(right >= x1 - slack_x) || !(top <= y0 + slack_y) || !(bottom >= y1 - slack_y)) {
            return {};
        }
        // The vectors under the view: texel = origin + (pixel - edge) x
        // texels per pixel, kept inside the vectors where the camera falls
        // short within the tolerance.
        const double per_x = frame.motion_width / (right - left);
        const double per_y = frame.motion_height / (bottom - top);
        const double motion_left = frame.motion_x, motion_top = frame.motion_y;
        const double motion_right = motion_left + frame.motion_width;
        const double motion_bottom = motion_top + frame.motion_height;
        const double mx0 = std::clamp(motion_left + (x0 - left) * per_x, motion_left, motion_right);
        const double mx1 = std::clamp(motion_left + (x1 - left) * per_x, motion_left, motion_right);
        const double my0 = std::clamp(motion_top + (y0 - top) * per_y, motion_top, motion_bottom);
        const double my1 = std::clamp(motion_top + (y1 - top) * per_y, motion_top, motion_bottom);
        if (!(mx1 - mx0 > 0.5) || !(my1 - my0 > 0.5)) return {};
        auto placed = std::make_shared<DlssMotionVectorFrame>(frame);
        placed->output_x = static_cast<std::uint32_t>(view.x);
        placed->output_y = static_cast<std::uint32_t>(view.y);
        placed->output_width = view.width;
        placed->output_height = view.height;
        placed->placed = true;
        placed->placed_motion_rect = {static_cast<float>(mx0), static_cast<float>(my0),
            static_cast<float>(mx1 - mx0), static_cast<float>(my1 - my0)};
        return placed;
    } catch (...) {
        return {};
    }
}

std::array<std::shared_ptr<const DlssMotionVectorSet>, kDlssMotionVectorEyeCount>
resolve_view_placed_dlss_guides(
    ID3D12CommandQueue* consumer_queue,
    const std::array<DlssEyeView, kDlssMotionVectorEyeCount>& eyes,
    const std::array<std::uint64_t, kDlssMotionVectorEyeCount>& released_at) noexcept {
    std::array<std::shared_ptr<const DlssMotionVectorSet>, kDlssMotionVectorEyeCount> none{};
    try {
        if (!g_tracking_enabled.load(std::memory_order_acquire) || consumer_queue == nullptr) {
            return none;
        }
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        std::vector<Candidate> candidates;
        std::uint64_t newest = 0;
        for (const auto& [unused_stream_id, stream] : state.streams) {
            static_cast<void>(unused_stream_id);
            if (!stream.latest || !stream.latest->view.known ||
                !same_identity(stream.latest->producer_queue.Get(), consumer_queue)) continue;
            candidates.push_back({stream.first_publication, stream.latest, stream.previous});
            newest = std::max(newest, stream.latest->publication);
        }
        if (candidates.empty()) return none;
        std::erase_if(candidates, [&](const Candidate& c) {
            return newest - c.frame->publication > kMaximumViewPublicationGap;
        });

        // The streams whose camera covers each eye, and how well each fits.
        constexpr double kNoFit = 1e30;
        std::vector<std::array<double, kDlssMotionVectorEyeCount>> fit(
            candidates.size(), {kNoFit, kNoFit});
        for (std::size_t index = 0; index < candidates.size(); ++index) {
            for (std::uint32_t eye = 0; eye < kDlssMotionVectorEyeCount; ++eye) {
                if (place_guide_in_view(*candidates[index].frame, eyes[eye])) {
                    fit[index][eye] = view_fit_error(candidates[index].frame->view, eyes[eye]);
                }
            }
        }
        // V549: the evaluations' order against the two eyes' releases picks
        // each eye's streams before the cameras do. A game evaluates a view's
        // DLSS and then releases that view's image - the image holds the
        // output - so the streams whose newest evaluation came before the
        // first eye's release are that eye's, and those evaluated between the
        // two releases the other's. MSFS 2024 foveated on a Pimax Dream Air
        // (#33): both peripheries carry the same symmetric field, the fit
        // ties every frame, and the cameras' positions along their right
        // vectors put the eyes the wrong way round about half the time; the
        // left eye then took the right periphery's previous evaluation, the
        // other eye's vectors a frame old, and juddered alone. The release
        // order never varied in those frames. The cameras decide only when
        // the order says nothing: both eyes evaluated before either release,
        // or no covering stream in one of the two groups.
        constexpr std::uint8_t kNoGroup = 2;
        std::vector<std::uint8_t> group(candidates.size(), kNoGroup);
        std::uint32_t first_eye = kDlssMotionVectorEyeCount;
        if (released_at[0] != 0 && released_at[1] != 0 && released_at[0] != released_at[1]) {
            first_eye = released_at[0] < released_at[1] ? 0U : 1U;
            for (std::size_t index = 0; index < candidates.size(); ++index) {
                const std::uint64_t publication = candidates[index].frame->publication;
                if (publication <= released_at[first_eye]) {
                    group[index] = static_cast<std::uint8_t>(first_eye);
                } else if (publication <= released_at[1 - first_eye]) {
                    group[index] = static_cast<std::uint8_t>(1 - first_eye);
                }
            }
        }
        // The pair of streams, one an eye, that fits best among those
        // allowed; equal fits prefer the newer evaluations (a stream left
        // from the frame before, in the first eye's group).
        std::size_t best_left = 0, best_right = 0;
        double best = kNoFit;
        const auto search = [&](auto&& allowed) {
            best = kNoFit;
            std::uint64_t best_recency = 0;
            for (std::size_t left = 0; left < candidates.size(); ++left) {
                for (std::size_t right = 0; right < candidates.size(); ++right) {
                    if (left == right || fit[left][0] >= kNoFit || fit[right][1] >= kNoFit ||
                        !allowed(left, right)) continue;
                    const double cost = fit[left][0] + fit[right][1];
                    const std::uint64_t recency =
                        candidates[left].frame->publication + candidates[right].frame->publication;
                    if (cost < best - 1e-9 || (std::fabs(cost - best) <= 1e-9 && recency > best_recency)) {
                        best = cost;
                        best_recency = recency;
                        best_left = left;
                        best_right = right;
                    }
                }
            }
            return best < kNoFit;
        };
        bool by_release = false;
        if (first_eye < kDlssMotionVectorEyeCount) {
            by_release = search([&](std::size_t left, std::size_t right) {
                return group[left] == 0 && group[right] == 1;
            });
        }
        if (!by_release) {
            search([](std::size_t, std::size_t) { return true; });
        }
        if (best >= kNoFit) {
            // 836, the first eight: a the recent streams with a camera |
            // those covering the left eye << 8 | the right << 16, b and c
            // the eyes' fields (pack_view_tangents); 834 has the cameras'.
            if (state.statistics.view_unmatched++ < 8) {
                std::uint64_t covering_left = 0, covering_right = 0;
                for (const auto& f : fit) {
                    covering_left += f[0] < kNoFit ? 1U : 0U;
                    covering_right += f[1] < kNoFit ? 1U : 0U;
                }
                try {
                    bridge_flight_logger().event(
                        BridgeFlightOperation::embedded_configuration, 836,
                        (candidates.size() & 0xFFU) | ((covering_left & 0xFFU) << 8U) |
                            ((covering_right & 0xFFU) << 16U),
                        pack_view_tangents(eyes[0].tan_left, eyes[0].tan_right,
                            eyes[0].tan_up, eyes[0].tan_down),
                        pack_view_tangents(eyes[1].tan_left, eyes[1].tan_right,
                            eyes[1].tan_up, eyes[1].tan_down));
                } catch (...) {
                }
            }
            state.statistics.status = DlssMotionVectorStatus::output_not_direct;
            return none;
        }
        // Two cameras that fit both eyes alike (a symmetric field): the
        // camera further left is the left eye. V549: only where the releases
        // said nothing; where they decided, the cameras' verdict is logged
        // against theirs (870) and nothing more.
        const bool fit_tie = fit[best_right][0] < kNoFit && fit[best_left][1] < kNoFit &&
            std::fabs((fit[best_right][0] + fit[best_left][1]) - best) < 1e-4;
        double side_left = 0.0, side_right = 0.0;
        bool cameras_tell = false;
        {
            const auto& a = candidates[best_left].frame->view;
            const auto& b = candidates[best_right].frame->view;
            const std::array<float, 3> right{
                a.right[0] + b.right[0], a.right[1] + b.right[1], a.right[2] + b.right[2]};
            side_left = camera_side(a, right);
            side_right = camera_side(b, right);
            cameras_tell = std::fabs(side_left - side_right) > 1e-6;
        }
        unsigned method = by_release ? 1U : fit_tie ? 3U : 2U;
        if (fit_tie && !by_release) {
            if (!cameras_tell) {
                ++state.statistics.view_unmatched;
                state.statistics.status = DlssMotionVectorStatus::temporal_mismatch;
                return none;
            }
            if (side_left > side_right) {
                std::swap(best_left, best_right);
                std::swap(side_left, side_right);
            }
        }

        // Each eye's evaluation: its stream's newest by the eye's release.
        std::array<std::shared_ptr<const DlssMotionVectorSet>, kDlssMotionVectorEyeCount> result{};
        const std::array<std::size_t, kDlssMotionVectorEyeCount> chosen_streams{best_left, best_right};
        // V516: the eyes' insets. An eye's inset is the recent stream whose
        // field lies inside the eye's, narrower than it, and whose camera
        // sits with that eye's periphery camera - well within a quarter of
        // the two eyes' spacing, where the other eye's is a whole spacing
        // away. None found, the eye keeps its periphery alone (V515).
        const double eye_spacing = camera_distance(
            candidates[best_left].frame->view, candidates[best_right].frame->view);
        std::uint32_t insets_found = 0;
        for (std::uint32_t eye = 0; eye < kDlssMotionVectorEyeCount; ++eye) {
            const Candidate& candidate = candidates[chosen_streams[eye]];
            const auto chosen = evaluation_by_release(candidate, released_at[eye]);
            auto placed = chosen ? place_guide_in_view(*chosen, eyes[eye]) : nullptr;
            if (!placed) {
                ++state.statistics.resolve_stale_pairs;
                state.statistics.status = DlssMotionVectorStatus::temporal_mismatch;
                return none;
            }
            // V549: where the releases chose the peripheries, the eye's inset
            // comes from the same release group - the camera nearest its
            // periphery's among those, however far.
            std::shared_ptr<DlssMotionVectorFrame> inset;
            if (by_release || eye_spacing > 1e-6) {
                std::size_t nearest = candidates.size();
                double nearest_distance = by_release ? kNoFit : 0.25 * eye_spacing;
                for (std::size_t index = 0; index < candidates.size(); ++index) {
                    if (index == best_left || index == best_right ||
                        (by_release && group[index] != eye) ||
                        !inside_eye_field(candidates[index].frame->view, eyes[eye])) continue;
                    const double distance =
                        camera_distance(candidates[index].frame->view, candidate.frame->view);
                    if (distance < nearest_distance) {
                        nearest_distance = distance;
                        nearest = index;
                    }
                }
                const auto evaluation = nearest < candidates.size()
                    ? evaluation_by_release(candidates[nearest], released_at[eye]) : nullptr;
                if (evaluation && evaluation->motion_width != 0 && evaluation->motion_height != 0 &&
                    inside_eye_field(evaluation->view, eyes[eye])) {
                    inset = std::make_shared<DlssMotionVectorFrame>(*evaluation);
                    inset->view_rect = view_rect_in_eye(evaluation->view, eyes[eye]);
                    inset->previous_view_rect_known = evaluation->previous_view.known;
                    inset->previous_view_rect = evaluation->previous_view.known
                        ? view_rect_in_eye(evaluation->previous_view, eyes[eye])
                        : inset->view_rect;
                    if (!(inset->view_rect[2] > 0.0F) || !(inset->previous_view_rect[2] > 0.0F)) {
                        inset.reset();
                    }
                }
            }
            auto set = std::make_shared<DlssMotionVectorSet>();
            set->eye_count = 1;
            if (inset) {
                auto owner = std::make_shared<DlssMotionVectorFrame>(*placed);
                owner->inset = std::move(inset);
                set->eyes[0] = std::move(owner);
                ++insets_found;
            } else {
                set->eyes[0] = std::move(placed);
            }
            result[eye] = std::move(set);
        }
        state.statistics.matched += kDlssMotionVectorEyeCount;
        ++state.statistics.view_matched;
        if (insets_found == kDlssMotionVectorEyeCount) ++state.statistics.inset_matched;
        // 870 (V549): how the eyes were joined, the first 16, every 512th, and
        // each change of streams, method or the cameras' verdict (the first
        // 1024 changes): a the method (1 the releases' order, 2 the cameras'
        // fit, 3 the cameras' positions) | bit 4 the releases told the eyes
        // apart | bit 5 the eye released first | bit 6 the cameras' positions
        // tell | bit 7 they would swap the eyes | the left eye's stream's low
        // 24 bits << 8 | the right's << 32; b the left camera's position
        // along the two cameras' summed right vector (float bits) << 32 | the
        // right's; c the streams considered | the first eye's group << 8 | the
        // second's << 16 | bit 24 both insets found | the joins so far << 32.
        {
            static std::uint64_t joins = 0;
            static std::uint64_t changes = 0;
            static std::uint64_t last_key = ~0ULL;
            const bool cameras_swap = cameras_tell && side_left > side_right;
            const std::uint64_t left_stream = candidates[best_left].frame->stream & 0xFFFFFFULL;
            const std::uint64_t right_stream = candidates[best_right].frame->stream & 0xFFFFFFULL;
            const std::uint64_t key = method | (cameras_swap ? 1ULL << 7U : 0ULL) |
                (left_stream << 8U) | (right_stream << 32U);
            const std::uint64_t join = joins++;
            const bool changed = key != last_key;
            if (changed) last_key = key;
            if (join < 16 || join % 512 == 0 || (changed && changes++ < 1024)) {
                std::uint64_t first_group = 0, second_group = 0;
                for (const auto g : group) {
                    if (first_eye < kDlssMotionVectorEyeCount && g == first_eye) ++first_group;
                    else if (g != kNoGroup) ++second_group;
                }
                const auto bits = [](double value) -> std::uint64_t {
                    const float f = static_cast<float>(value);
                    std::uint32_t b = 0;
                    std::memcpy(&b, &f, sizeof(b));
                    return b;
                };
                try {
                    bridge_flight_logger().event(BridgeFlightOperation::embedded_configuration, 870,
                        key | (first_eye < kDlssMotionVectorEyeCount ? 1ULL << 4U : 0ULL) |
                            (first_eye == 1U ? 1ULL << 5U : 0ULL) | (cameras_tell ? 1ULL << 6U : 0ULL),
                        (bits(side_left) << 32U) | bits(side_right),
                        (candidates.size() & 0xFFULL) | ((first_group & 0xFFULL) << 8U) |
                            ((second_group & 0xFFULL) << 16U) |
                            (insets_found == kDlssMotionVectorEyeCount ? 1ULL << 24U : 0ULL) |
                            (join << 32U));
                } catch (...) {
                }
            }
        }
        return result;
    } catch (...) {
        return none;
    }
}

std::uint64_t dlss_motion_vector_publications() noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        return state.statistics.published;
    } catch (...) {
        return 0;
    }
}

void configure_dlss_alternating_eyes(bool required) noexcept {
    g_alternating_eyes.store(required, std::memory_order_release);
}

void anchor_dlss_alternating_eyes(
    std::uint64_t stream, std::uint64_t epoch, std::uint32_t left_parity) noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        const auto found = state.streams.find(stream);
        if (found == state.streams.end() || found->second.epoch != epoch) return;
        found->second.anchor_epoch = epoch;
        found->second.left_parity = left_parity & 1U;
    } catch (...) {
    }
}

void anchor_dlss_alternating_eye_tags(std::uint64_t left_tag, std::uint64_t right_tag) noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        state.left_eye_tag = left_tag;
        state.right_eye_tag = right_tag;
    } catch (...) {
    }
}

// V540: an alternating stream's evaluation's eye, 0 left and 1 right, or -1
// before its anchor: by its tag where the tags are anchored, else by its
// serial's parity where its epoch is.
[[nodiscard]] static int alternating_eye(const Registry& state, const StreamState& stream,
    std::uint64_t epoch, std::uint64_t serial, std::uint64_t tag) noexcept {
    if (tag != 0 && tag == state.left_eye_tag) return 0;
    if (tag != 0 && tag == state.right_eye_tag) return 1;
    if (stream.anchor_epoch == 0 || stream.anchor_epoch != epoch) return -1;
    return (serial & 1U) == stream.left_parity ? 0 : 1;
}

[[nodiscard]] static int alternating_eye(
    const Registry& state, const StreamState& stream, const DlssMotionVectorFrame& frame) noexcept {
    return alternating_eye(state, stream, frame.epoch, frame.serial, frame.eye_tag);
}

int dlss_alternating_eye(
    std::uint64_t stream, std::uint64_t epoch, std::uint64_t serial, std::uint64_t tag) noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        const auto found = state.streams.find(stream);
        if (found == state.streams.end()) return -1;
        return alternating_eye(state, found->second, epoch, serial, tag);
    } catch (...) {
        return -1;
    }
}

void set_dlss_alternating_skips(std::uint32_t newest, std::uint32_t other) noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        state.alternating_skips = {std::min(newest, 2U), std::min(other, 2U)};
    } catch (...) {
    }
}

// V546: when the eye images' votes decide a stream pair's order. A
// measurement counts with at least kEyeOrderMinimumPixels compared; the
// pair is decided after kEyeOrderMeasurements of them and kEyeOrderVotes
// pixels, once one order has half as many votes again as the other. A head
// still and a scene standing still give no pixel where the two streams'
// vectors part, so no vote: the decision waits for motion.
constexpr std::uint64_t kEyeOrderMinimumPixels = 64;
constexpr std::uint64_t kEyeOrderMeasurements = 24;
constexpr std::uint64_t kEyeOrderVotes = 20000;

void report_dlss_eye_order(std::uint32_t eye, std::uint64_t assigned_stream,
    std::uint64_t alternate_stream, std::uint64_t assigned_votes,
    std::uint64_t alternate_votes, std::uint64_t informative) noexcept {
    try {
        if (eye > 1 || assigned_stream == alternate_stream) return;
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        // 867: a measurement, the first 64 and every 100th after: a the eye
        // << 56 | the assigned stream's low 28 bits << 28 | the alternate's,
        // b the pixels that favoured the assigned stream << 32 | the
        // alternate, c the pixels compared.
        static std::uint64_t measured = 0;
        if (measured < 64 || measured % 100 == 0) {
            bridge_flight_logger().event(BridgeFlightOperation::embedded_configuration, 867,
                (static_cast<std::uint64_t>(eye) << 56U) |
                    ((assigned_stream & 0xFFFFFFFULL) << 28U) | (alternate_stream & 0xFFFFFFFULL),
                (std::min<std::uint64_t>(assigned_votes, 0xFFFFFFFFULL) << 32U) |
                    std::min<std::uint64_t>(alternate_votes, 0xFFFFFFFFULL),
                informative);
        }
        ++measured;
        const auto key = std::minmax(assigned_stream, alternate_stream);
        auto& order = state.eye_orders[{key.first, key.second}];
        if (order.left != 0 || informative < kEyeOrderMinimumPixels) return;
        // An assigned stream that matches is this eye's; the left eye's for
        // eye 0, and for eye 1 it makes the other stream the left one.
        const std::uint64_t left_if_assigned = eye == 0 ? assigned_stream : alternate_stream;
        const std::uint64_t left_if_alternate = eye == 0 ? alternate_stream : assigned_stream;
        (left_if_assigned == key.first ? order.lower_left_votes : order.higher_left_votes) +=
            assigned_votes;
        (left_if_alternate == key.first ? order.lower_left_votes : order.higher_left_votes) +=
            alternate_votes;
        ++order.measurements;
        const std::uint64_t lower = order.lower_left_votes;
        const std::uint64_t higher = order.higher_left_votes;
        if (order.measurements < kEyeOrderMeasurements || lower + higher < kEyeOrderVotes) return;
        if (lower * 2U >= higher * 3U) {
            order.left = key.first;
        } else if (higher * 2U >= lower * 3U) {
            order.left = key.second;
        } else {
            return;
        }
        // 868: the pair's order, once a session (kept through the flight
        // log's wraps): a the left stream, b the right, c the votes for the
        // lower stream as the left << 40 | for the higher << 16 (24 bits
        // each) | the measurements (16 bits).
        bridge_flight_logger().event(BridgeFlightOperation::embedded_configuration, 868,
            order.left, order.left == key.first ? key.second : key.first,
            (std::min<std::uint64_t>(lower, 0xFFFFFFULL) << 40U) |
                (std::min<std::uint64_t>(higher, 0xFFFFFFULL) << 16U) |
                std::min<std::uint64_t>(order.measurements, 0xFFFFULL));
    } catch (...) {
    }
}

std::uint64_t dlss_eye_order_left(std::uint64_t first, std::uint64_t second) noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        const auto key = std::minmax(first, second);
        const auto found = state.eye_orders.find({key.first, key.second});
        return found != state.eye_orders.end() ? found->second.left : 0U;
    } catch (...) {
        return 0;
    }
}

// V534: an eye image that is the middle window of a slightly larger DLSS
// output, without a camera to say so. R.E.A.L. VR's Kingdom Come renders
// 3776 x 3088 and submits each eye on 3679 x 3088 swapchains with no
// Streamline: the window Ghost of Tsushima's camera measured (V533) was its
// output's exact middle at the output's scale, and this is placed the same
// way. Only for alternating-eye streams (configure_dlss_alternating_eyes),
// a symmetric eye field, and an output at most kCentredWindowMargin larger
// per axis; otherwise null, the reason recorded once each (859: a 1 not
// alternating, 2 the field not symmetric, 3 the output too large, 4 smaller
// and not of the image's shape).
//
// V537: an output smaller than the eye image, of its shape, is the whole eye
// image enlarged. R.E.A.L. VR's Avatar: Frontiers of Pandora evaluates DLSS
// into the corner of a 3280 x 2752 texture, 1902 x 1596 from 1929 x 1619,
// and the eyes are 3278 x 2752: the same aspect within 0.05%, 1.72 times the
// size. Its whole output is placed over the whole eye image (the vectors
// scale into the image's pixels as MSFS 2024's periphery does, V515), up to
// kStretchLimit times and within kStretchAspect of the image's aspect.
constexpr double kCentredWindowMargin = 0.06;
constexpr double kStretchLimit = 3.0;
constexpr double kStretchAspect = 0.01;

std::shared_ptr<const DlssMotionVectorFrame> place_centred_window(
    const DlssMotionVectorFrame& frame, const DlssEyeView& view) noexcept {
    try {
        const auto refuse = [](std::uint64_t reason) {
            static std::array<std::atomic_bool, 5> recorded{};
            if (reason < recorded.size() && !recorded[reason].exchange(true)) {
                try {
                    bridge_flight_logger().event(
                        BridgeFlightOperation::embedded_configuration, 859, reason);
                } catch (...) {
                }
            }
            return std::shared_ptr<const DlssMotionVectorFrame>{};
        };
        if (!g_alternating_eyes.load(std::memory_order_acquire)) return refuse(1);
        if (view.x < 0 || view.y < 0 || view.width == 0 || view.height == 0 ||
            frame.output_x != 0 || frame.output_y != 0 || frame.output_width == 0 ||
            frame.output_height == 0 || frame.motion_width == 0 || frame.motion_height == 0) {
            return {};
        }
        if (frame.output_width < view.width || frame.output_height < view.height) {
            // V537: the whole image, enlarged.
            const double scale_x = static_cast<double>(view.width) / frame.output_width;
            const double scale_y = static_cast<double>(view.height) / frame.output_height;
            if (!(scale_x >= 1.0) || !(scale_y >= 1.0) || scale_x > kStretchLimit ||
                scale_y > kStretchLimit ||
                std::fabs(scale_x / scale_y - 1.0) > kStretchAspect) {
                return refuse(4);
            }
            auto placed = std::make_shared<DlssMotionVectorFrame>(frame);
            placed->output_x = static_cast<std::uint32_t>(view.x);
            placed->output_y = static_cast<std::uint32_t>(view.y);
            placed->output_width = view.width;
            placed->output_height = view.height;
            placed->placed = true;
            placed->placed_motion_rect = {static_cast<float>(frame.motion_x),
                static_cast<float>(frame.motion_y), static_cast<float>(frame.motion_width),
                static_cast<float>(frame.motion_height)};
            return placed;
        }
        const double width = static_cast<double>(view.tan_right) - view.tan_left;
        const double height = static_cast<double>(view.tan_up) - view.tan_down;
        if (!(width > 0.0) || !(height > 0.0) ||
            !(std::fabs(static_cast<double>(view.tan_left) + view.tan_right) <= 0.01 * width) ||
            !(std::fabs(static_cast<double>(view.tan_up) + view.tan_down) <= 0.01 * height)) {
            return refuse(2);
        }
        if (frame.output_width - view.width > kCentredWindowMargin * view.width ||
            frame.output_height - view.height > kCentredWindowMargin * view.height) {
            return refuse(3);
        }
        const double per_x = static_cast<double>(frame.motion_width) / frame.output_width;
        const double per_y = static_cast<double>(frame.motion_height) / frame.output_height;
        const double x0 = (static_cast<double>(frame.output_width) - view.width) / 2.0;
        const double y0 = (static_cast<double>(frame.output_height) - view.height) / 2.0;
        auto placed = std::make_shared<DlssMotionVectorFrame>(frame);
        placed->output_x = static_cast<std::uint32_t>(view.x);
        placed->output_y = static_cast<std::uint32_t>(view.y);
        placed->output_width = view.width;
        placed->output_height = view.height;
        placed->placed = true;
        placed->placed_motion_rect = {
            static_cast<float>(frame.motion_x + x0 * per_x),
            static_cast<float>(frame.motion_y + y0 * per_y),
            static_cast<float>(view.width * per_x),
            static_cast<float>(view.height * per_y)};
        return placed;
    } catch (...) {
        return {};
    }
}

// The eye's window of `frame`: by its camera (V533), else in its middle (V534).
// V537: an alternating stream whose camera does not hold the eye's field is
// recorded once (859 a = 5, b the eye's tangents, c the camera's, as 836's).
std::shared_ptr<const DlssMotionVectorFrame> place_on_eye_window(
    const DlssMotionVectorFrame& frame, const DlssEyeView& view) noexcept {
    if (!frame.view.known) return place_centred_window(frame, view);
    auto placed = place_guide_in_view(frame, view);
    if (!placed && g_alternating_eyes.load(std::memory_order_acquire)) {
        static std::atomic_bool recorded{false};
        if (!recorded.exchange(true)) {
            try {
                bridge_flight_logger().event(BridgeFlightOperation::embedded_configuration, 859, 5U,
                    pack_view_tangents(view.tan_left, view.tan_right, view.tan_up, view.tan_down),
                    pack_view_tangents(frame.view.tan_left, frame.view.tan_right,
                        frame.view.tan_up, frame.view.tan_down));
            } catch (...) {
            }
        }
    }
    return placed;
}

void set_dlss_eye_view(std::uint32_t eye, const DlssEyeView& view) noexcept {
    try {
        if (eye >= kDlssMotionVectorEyeCount) return;
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        state.eye_views[eye] = view;
        state.eye_view_known[eye] = view.width != 0 && view.height != 0 &&
            view.tan_right > view.tan_left && view.tan_up > view.tan_down;
    } catch (...) {
    }
}

bool dlss_inset_origin(std::uint64_t stream, std::uint32_t eye,
    std::int32_t* x, std::int32_t* y) noexcept {
    try {
        if (eye >= kDlssMotionVectorEyeCount || x == nullptr || y == nullptr) return false;
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        const auto found = state.streams.find(stream);
        if (found == state.streams.end() || !found->second.latest ||
            !state.eye_view_known[eye]) return false;
        const DlssMotionVectorFrame& frame = *found->second.latest;
        if (frame.output_width == 0 || frame.output_height == 0) return false;
        if (!frame.view.known) {
            // V534: the middle window, at the output's scale.
            const DlssEyeView& view = state.eye_views[eye];
            // V537: an enlarged output is not at the image's scale.
            if (frame.output_width < view.width || frame.output_height < view.height ||
                !place_centred_window(frame, view)) {
                return false;
            }
            *x = static_cast<std::int32_t>(std::lround(
                view.x - (static_cast<double>(frame.output_width) - view.width) / 2.0));
            *y = static_cast<std::int32_t>(std::lround(
                view.y - (static_cast<double>(frame.output_height) - view.height) / 2.0));
            return true;
        }
        const auto placed = place_guide_in_view(frame, state.eye_views[eye]);
        if (!placed) return false;
        // The output's edges on the eye image, as place_guide_in_view finds
        // them, and its pixels' size there.
        const DlssEyeView& view = state.eye_views[eye];
        const DlssViewGeometry& g = frame.view;
        const double eye_width = static_cast<double>(view.tan_right) - view.tan_left;
        const double eye_height = static_cast<double>(view.tan_up) - view.tan_down;
        const double left = view.x + (g.tan_left - view.tan_left) / eye_width * view.width;
        const double right = view.x + (g.tan_right - view.tan_left) / eye_width * view.width;
        const double top = view.y + (view.tan_up - g.tan_up) / eye_height * view.height;
        const double bottom = view.y + (view.tan_up - g.tan_down) / eye_height * view.height;
        const double scale_x = (right - left) / frame.output_width;
        const double scale_y = (bottom - top) / frame.output_height;
        if (!(std::fabs(scale_x - 1.0) <= 0.01) || !(std::fabs(scale_y - 1.0) <= 0.01)) return false;
        *x = static_cast<std::int32_t>(std::lround(left));
        *y = static_cast<std::int32_t>(std::lround(top));
        return true;
    } catch (...) {
        return false;
    }
}

DlssStreamPosition dlss_motion_vector_stream_position(std::uint64_t stream) noexcept {
    DlssStreamPosition position;
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        const auto found = state.streams.find(stream);
        if (found == state.streams.end()) return position;
        const StreamState& value = found->second;
        position.known = true;
        position.epoch = value.epoch;
        position.serial = value.serial;
        position.publication = value.latest ? value.latest->publication : 0;
        position.anchored = value.latest
            ? alternating_eye(state, value, *value.latest) >= 0
            : value.anchor_epoch != 0 && value.anchor_epoch == value.epoch;
        position.tag_anchored = value.latest && value.latest->eye_tag != 0 &&
            (value.latest->eye_tag == state.left_eye_tag ||
                value.latest->eye_tag == state.right_eye_tag);
    } catch (...) {
    }
    return position;
}

std::shared_ptr<const DlssMotionVectorSet> resolve_dlss_motion_vectors(
    ID3D12Resource* output,
    ID3D12CommandQueue* consumer_queue,
    int eye,
    std::uint64_t released_at,
    std::uint64_t other_released_at) noexcept {
    try {
        if (!g_tracking_enabled.load(std::memory_order_acquire)) {
            report_dlss_motion_vector_status(DlssMotionVectorStatus::disabled);
            return {};
        }
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        release_finished_snapshots(state);
        ++state.statistics.resolve_calls;
        record_resolver_statistics(state.statistics);
        if (output == nullptr || consumer_queue == nullptr) {
            ++state.statistics.invalid_rejections;
            state.statistics.status = DlssMotionVectorStatus::invalid_input;
            return {};
        }

        const auto description = output->GetDesc();
        if (description.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D) {
            ++state.statistics.invalid_rejections;
            state.statistics.status = DlssMotionVectorStatus::invalid_input;
            return {};
        }
        std::uint32_t eye_count = std::min<std::uint32_t>(
            std::max<std::uint32_t>(description.DepthOrArraySize, 1U),
            kDlssMotionVectorEyeCount);

        std::vector<Candidate> candidates;
        std::uint32_t streams_with_frames = 0;
        candidates.reserve(state.streams.size());
        for (const auto& [unused_stream_id, stream] : state.streams) {
            static_cast<void>(unused_stream_id);
            if (!stream.latest) continue;
            ++streams_with_frames;
            if (same_identity(stream.latest->producer_queue.Get(), consumer_queue)) {
                candidates.push_back(
                    {stream.first_publication, stream.latest, stream.previous});
            }
        }
        const auto stale = [&]() -> std::shared_ptr<const DlssMotionVectorSet> {
            ++state.statistics.resolve_stale_pairs;
            state.statistics.status = DlssMotionVectorStatus::temporal_mismatch;
            return {};
        };
        // V546: two streams sorted by input offset, then first seen. Where
        // nothing tells them apart - the same input offset and no camera -
        // the order the pair's measurement decided replaces first seen
        // (report_dlss_eye_order); true while the pair is still undecided,
        // and its sets then carry the other stream for the measurement.
        const auto untold_pair = [&](const std::vector<Candidate>& pair) {
            return pair.size() == 2 && pair[0].frame && pair[1].frame &&
                pair[0].frame->stream != pair[1].frame->stream &&
                input_offset(*pair[0].frame) == input_offset(*pair[1].frame) &&
                !pair[0].frame->view.known && !pair[1].frame->view.known;
        };
        const auto order_measured_pair = [&](std::vector<Candidate>& pair) {
            if (!untold_pair(pair)) return false;
            const auto key = std::minmax(pair[0].frame->stream, pair[1].frame->stream);
            const auto found = state.eye_orders.find({key.first, key.second});
            const std::uint64_t left =
                found != state.eye_orders.end() ? found->second.left : 0U;
            if (left == 0) return true;
            if (pair[1].frame->stream == left) std::swap(pair[0], pair[1]);
            return false;
        };
        const auto attach_alternate = [&](const std::shared_ptr<DlssMotionVectorSet>& result,
                                          const std::shared_ptr<const DlssMotionVectorFrame>& other) {
            if (!result || !other || result->eye_count != 1 || !result->eyes[0] ||
                other->stream == result->eyes[0]->stream) {
                return;
            }
            result->alternate = other;
            result->resolved_eye = eye;
        };
        // One evaluation for both eyes, on texture arrays with a slice per
        // eye: both eyes take the same publication, each reading its slice,
        // and the eye's previous evaluation is the stream's previous one.
        const auto array_eyes = [](const DlssMotionVectorFrame& frame) {
            if (!frame.output || !frame.motion_vectors) return false;
            return frame.output->GetDesc().DepthOrArraySize >= kDlssMotionVectorEyeCount &&
                frame.motion_vectors->GetDesc().DepthOrArraySize >= kDlssMotionVectorEyeCount;
        };
        // V533: an evaluation larger than this image whose camera holds this
        // eye's field: the eye image is a window of it (R.E.A.L. VR's Ghost of
        // Tsushima), and the evaluation guides it placed on that window.
        const bool eye_view = (eye == 0 || eye == 1) &&
            state.eye_view_known[static_cast<std::size_t>(eye)];
        const auto fits_image = [&](const DlssMotionVectorFrame& frame) {
            return frame.output_x == 0 && frame.output_y == 0 &&
                output_fits_extent(frame.output_width, description.Width) &&
                output_fits_extent(frame.output_height, description.Height);
        };
        // V537: an output smaller than the image too, without a camera or -
        // for R.E.A.L. VR's alternating streams - with one (Avatar: Frontiers
        // of Pandora evaluates inside Streamline, its camera +-1.1560 x
        // +-0.9702 the whole 1902 x 1596 output's). Elsewhere a camera-placed
        // smaller output is the per-view join's (MSFS 2024's periphery).
        const bool alternating = g_alternating_eyes.load(std::memory_order_acquire);
        const auto inset_of_image = [&](const DlssMotionVectorFrame& frame) {
            return eye_view && frame.output_x == 0 && frame.output_y == 0 &&
                (!frame.view.known || alternating ||
                    (frame.output_width >= description.Width &&
                        frame.output_height >= description.Height)) &&
                place_on_eye_window(frame, state.eye_views[static_cast<std::size_t>(eye)]) != nullptr;
        };
        // 858, the first eight placements: a the eye << 32 | the stream's low
        // bits | bit 63 placed without a camera (V534), bit 62 an output
        // smaller than the image, enlarged over it (V537), b the
        // output's width << 32 | height, c the placed vector rectangle (x, y,
        // width, height in eighths of a texel, 16 bits each).
        const auto place_inset = [&](DlssMotionVectorFrame& frame) {
            if (!eye_view || fits_image(frame)) return;
            const bool centred = !frame.view.known;
            const bool enlarged =
                frame.output_width < state.eye_views[static_cast<std::size_t>(eye)].width;
            const auto placed =
                place_on_eye_window(frame, state.eye_views[static_cast<std::size_t>(eye)]);
            if (!placed) return;
            frame = *placed;
            if (state.statistics.inset_placed++ < 8) {
                const auto eighths = [](float value) {
                    return static_cast<std::uint64_t>(std::clamp(value * 8.0F, 0.0F, 65535.0F)) & 0xFFFFU;
                };
                try {
                    bridge_flight_logger().event(BridgeFlightOperation::embedded_configuration, 858,
                        (static_cast<std::uint64_t>(eye) << 32U) | (frame.stream & 0xFFFFFFFFULL) |
                            (centred ? 1ULL << 63U : 0ULL) | (enlarged ? 1ULL << 62U : 0ULL),
                        (static_cast<std::uint64_t>(frame.output_width) << 32U) | frame.output_height,
                        eighths(frame.placed_motion_rect[0]) | (eighths(frame.placed_motion_rect[1]) << 16U) |
                            (eighths(frame.placed_motion_rect[2]) << 32U) |
                            (eighths(frame.placed_motion_rect[3]) << 48U));
                } catch (...) {
                }
            }
        };
        const auto one_eye = [&](std::shared_ptr<const DlssMotionVectorFrame> chosen,
                                 bool shared_stream) {
            auto frame = std::make_shared<DlssMotionVectorFrame>(*chosen);
            place_inset(*frame);
            if (array_eyes(*chosen) && (eye == 0 || eye == 1)) {
                frame->output_slice = static_cast<std::uint32_t>(eye);
                frame->motion_slice = static_cast<std::uint32_t>(eye);
            } else if (shared_stream) {
                // One stream alternating eyes: this eye's previous evaluation
                // is two serials back, which is what a valid pair checks.
                frame->previous_serial = frame->serial > 2 ? frame->serial - 2 : 0;
            }
            auto result = std::make_shared<DlssMotionVectorSet>();
            result->eye_count = 1;
            result->eyes[0] = std::move(frame);
            ++state.statistics.matched;
            return result;
        };

        // An evaluation whose output is this very image belongs to it,
        // whatever else matches its size.
        bool direct_match = false;
        {
            std::vector<Candidate> direct;
            for (const auto& candidate : candidates) {
                if (same_identity(candidate.frame->output.Get(), output)) {
                    direct.push_back(candidate);
                }
            }
            if (!direct.empty()) {
                candidates = std::move(direct);
                direct_match = true;
            }
        }

        // Native Stereo can place two eye viewports side by side in one
        // array-size-one XR image while DLSS keeps one persistent stream per
        // eye. Restrict the candidate set to streams whose published output is
        // exactly one horizontal half of the XR image.
        bool packed_stereo = false;
        if (eye_count == 1 && !direct_match) {
            std::vector<Candidate> halves;
            for (const auto& candidate : candidates) {
                if (candidate.frame->output_width != 0 &&
                    candidate.frame->output_height != 0 &&
                    description.Width ==
                        static_cast<std::uint64_t>(candidate.frame->output_width) * 2U &&
                    description.Height == candidate.frame->output_height) {
                    halves.push_back(candidate);
                }
            }
            if (halves.size() >= kDlssMotionVectorEyeCount) {
                candidates = std::move(halves);
                eye_count = kDlssMotionVectorEyeCount;
                packed_stereo = true;
            }
        }

        if (eye_count == 1 && !direct_match) {
            // Composition/UI swapchains share the scene queue but are not the
            // upscaler output, and a mirror's evaluation is not an eye's.
            // Only an evaluation the size of this image (or a few texels
            // larger, output_fits_extent) can be its guide.
            std::erase_if(candidates, [&](const Candidate& c) {
                return !fits_image(*c.frame) && !inset_of_image(*c.frame);
            });
        }

        // A swapchain per eye, each the size of a DLSS output: every eye's
        // evaluation matches each eye's image by size, and the newest went to
        // both - with the capture at xrEndFrame, after both eyes, always the
        // eye DLSS ran last - so the other eye followed its motion, a double
        // image wherever the two eyes' motion differs. The eye this image is
        // submitted as takes its own evaluation: by its colour's release
        // (released_at, the publication watermark then), or failing that by
        // the order the stereo path gives two - two streams, or the game's
        // one stream alternating eyes.
        if (!direct_match && eye_count == 1 && released_at != 0 &&
            (eye == 0 || eye == 1)) {
            std::uint64_t newest_publication = 0;
            for (const auto& candidate : candidates)
                newest_publication = std::max(newest_publication, candidate.frame->publication);
            std::erase_if(candidates, [&](const Candidate& candidate) {
                return newest_publication - candidate.frame->publication >
                    kMaximumStereoPublicationGap;
            });
            if (candidates.empty()) {
                if (streams_with_frames >= eye_count) {
                    ++state.statistics.resolve_queue_mismatches;
                    state.statistics.status = DlssMotionVectorStatus::queue_mismatch;
                } else {
                    ++state.statistics.resolve_missing_streams;
                    state.statistics.status = state.statistics.published == 0
                        ? DlssMotionVectorStatus::waiting_for_dlss
                        : DlssMotionVectorStatus::output_not_direct;
                }
                return {};
            }
            std::vector<std::shared_ptr<const DlssMotionVectorFrame>> frames;
            for (const auto& candidate : candidates) {
                frames.push_back(candidate.frame);
                if (candidate.previous && candidate.previous->epoch == candidate.frame->epoch)
                    frames.push_back(candidate.previous);
            }
            std::sort(frames.begin(), frames.end(), [](const auto& a, const auto& b) {
                return a->publication > b->publication;
            });
            // V524: alternating eyes (configure_dlss_alternating_eyes). One
            // stream for both eyes, its eyes by serial parity once anchored
            // (unanchored, it guides no eye), each eye's previous two serials
            // back. R.E.A.L. VR's AERv2 hands over one eye a frame late: in
            // Cyberpunk 2077 each eye image matched (record 851, ~0.72 against
            // ~0.35) either the newest evaluation, for that evaluation's eye,
            // or - for the other eye - not its newest but the one before it,
            // its evaluation of the previous frame. So the newest
            // evaluation's eye takes it, and the other eye takes its
            // second-newest. V542: as many of its own as the eye measurement
            // found its picture behind (set_dlss_alternating_skips).
            if (g_alternating_eyes.load(std::memory_order_acquire) &&
                candidates.size() == 1 && !array_eyes(*candidates.front().frame)) {
                const auto& latest = candidates.front().frame;
                const auto found = state.streams.find(latest->stream);
                if (found == state.streams.end() ||
                    alternating_eye(state, found->second, *latest) < 0) {
                    return stale();
                }
                StreamState& stream = found->second;
                // V540: by tag, or by serial parity (alternating_eye).
                const auto ours = [&](const DlssMotionVectorFrame& frame) {
                    return alternating_eye(state, stream, frame) == eye;
                };
                std::vector<std::shared_ptr<const DlssMotionVectorFrame>> history;
                for (const auto* frame : {&stream.latest, &stream.previous, &stream.previous2,
                         &stream.previous3, &stream.previous4, &stream.previous5}) {
                    if (*frame && (*frame)->epoch == latest->epoch &&
                        (*frame)->publication <= released_at) {
                        history.push_back(*frame);
                    }
                }
                std::sort(history.begin(), history.end(),
                    [](const auto& a, const auto& b) { return a->serial > b->serial; });
                if (history.empty()) return stale();
                const bool current = ours(*history.front());
                std::uint32_t skip = state.alternating_skips[current ? 0 : 1];
                std::shared_ptr<const DlssMotionVectorFrame> chosen;
                for (const auto& frame : history) {
                    if (!ours(*frame)) continue;
                    if (skip != 0) {
                        --skip;
                        continue;
                    }
                    chosen = frame;
                    break;
                }
                if (!chosen) return stale();
                // Per eye, against its frame before (the release watermark
                // tells the frames apart): never back in time - where the
                // eyes trade places at a frame of one evaluation, the lagging
                // eye's second-newest is older than what it showed, and its
                // picture is shown again instead - its previous the guide it
                // had that frame, so a pair holds across the trade, and a
                // repeat (the same evaluation, the same picture) without
                // motion. Each of these was a pair the composition refused
                // (815, 1 in 5 of R.E.A.L. VR's pairs) and drew with the flow
                // alone: Hybrid and flow in turn, a flicker standing still.
                StreamState::EyeTrack& track = stream.eyes[static_cast<std::size_t>(eye)];
                if (track.current && track.current->epoch != chosen->epoch) track = {};
                if (released_at != track.watermark) {
                    track.previous = track.current;
                    track.watermark = released_at;
                }
                if (track.previous && chosen->serial < track.previous->serial) chosen = track.previous;
                track.current = chosen;
                auto frame = std::make_shared<DlssMotionVectorFrame>(*chosen);
                frame->previous_serial = track.previous ? track.previous->serial
                    : (frame->serial > 2 ? frame->serial - 2 : 0);
                if (track.previous && track.previous->serial == frame->serial) {
                    frame->scale_x = 1.0e-6F;
                    frame->scale_y = 1.0e-6F;
                }
                // The right eye as R.E.A.L. VR shows it (DominantEye=0): not
                // its evaluation as rendered but that render rotated to the
                // left eye's pose, both eyes submitted with one orientation.
                // In Cyberpunk 2077 its picture sat up to 13 output pixels to
                // the side of its evaluation's, following the head's turn
                // (record 851), the left eye's at none. The rotation is the
                // one between the two evaluations' cameras (Streamline's), and
                // where the left camera's forward falls in the right one's
                // projection is how far the right eye's vectors are read
                // from: a rectangle placed by the camera, as MSFS 2024's
                // periphery is.
                const StreamState::EyeTrack& left = stream.eyes[0];
                // V533: as a shift of the vectors read (in texels), applied
                // after the eye's window is placed.
                double shift_x = 0.0, shift_y = 0.0;
                bool shifted = false;
                if (eye == 1 && left.current && left.watermark == released_at &&
                    left.current->serial != frame->serial && left.current->view.known &&
                    frame->view.known && frame->output_width != 0 && frame->output_height != 0 &&
                    frame->motion_width != 0 && frame->motion_height != 0) {
                    const auto dot = [](const std::array<float, 3>& a, const std::array<float, 3>& b) {
                        return static_cast<double>(a[0]) * b[0] + static_cast<double>(a[1]) * b[1] +
                            static_cast<double>(a[2]) * b[2];
                    };
                    const auto& forward = left.current->view.forward;
                    const double z = dot(forward, frame->view.forward);
                    const double span_x = static_cast<double>(frame->view.tan_right) - frame->view.tan_left;
                    const double span_y = static_cast<double>(frame->view.tan_up) - frame->view.tan_down;
                    if (z > 0.5 && span_x > 0.0 && span_y > 0.0) {
                        const double tan_x = dot(forward, frame->view.right) / z;
                        const double tan_y = dot(forward, frame->view.up) / z;
                        const double dx = tan_x / span_x * frame->output_width;
                        const double dy = -tan_y / span_y * frame->output_height;
                        if (std::isfinite(dx) && std::isfinite(dy) &&
                            std::fabs(dx) < frame->output_width / 4.0 &&
                            std::fabs(dy) < frame->output_height / 4.0) {
                            const double per_x = static_cast<double>(frame->motion_width) / frame->output_width;
                            const double per_y = static_cast<double>(frame->motion_height) / frame->output_height;
                            shift_x = dx * per_x;
                            shift_y = dy * per_y;
                            shifted = true;
                        }
                    }
                }
                place_inset(*frame);
                if (shifted) {
                    if (!frame->placed) {
                        frame->placed = true;
                        frame->placed_motion_rect = {static_cast<float>(frame->motion_x),
                            static_cast<float>(frame->motion_y), static_cast<float>(frame->motion_width),
                            static_cast<float>(frame->motion_height)};
                    }
                    frame->placed_motion_rect[0] += static_cast<float>(shift_x);
                    frame->placed_motion_rect[1] += static_cast<float>(shift_y);
                }
                auto result = std::make_shared<DlssMotionVectorSet>();
                result->eye_count = 1;
                result->eyes[0] = std::move(frame);
                ++state.statistics.matched;
                return result;
            }
            const auto newest_by = [&](std::uint64_t limit) {
                for (const auto& frame : frames)
                    if (frame->publication <= limit) return frame;
                return std::shared_ptr<const DlssMotionVectorFrame>{};
            };
            auto chosen = newest_by(released_at);
            const auto other = other_released_at ? newest_by(other_released_at) : nullptr;
            bool shared_stream = candidates.size() == 1;
            if (chosen && shared_stream && array_eyes(*chosen)) {
                return one_eye(chosen, false);
            }
            std::shared_ptr<const DlssMotionVectorFrame> alternate;
            // V547: two streams nothing tells apart go to the eyes by their
            // measured order on this path too. V546 applied the order only
            // where both evaluations came before either release (the tie
            // below); where one came between the two releases, each eye took
            // the newest evaluation before its own, whichever stream that
            // was - Star Citizen switched live to 3X on Virtual Desktop got
            // the other eye's vectors in each eye, with the pair's order
            // measured and decided. The release now only says which of the
            // decided stream's evaluations an eye takes, and an undecided
            // pair is measured whichever way its eyes were given.
            std::uint64_t accept_limit = released_at;
            if (candidates.size() == 2 && untold_pair(candidates)) {
                std::sort(candidates.begin(), candidates.end(),
                    [](const Candidate& a, const Candidate& b) {
                        return a.first_publication < b.first_publication;
                    });
                const bool undecided = order_measured_pair(candidates);
                // V548: the later of the two eyes' watermarks bounds both
                // eyes. The bridged releases of a frame's two eyes run back
                // to back at the game's xrEndFrame, each publishing the
                // evaluations whose Vulkan submissions its wait for the
                // game's queue completed, then reading its watermark. When
                // the second eye's wait is what completes the first eye's
                // DLSS submission - Star Citizen evaluates its eyes in either
                // order - the first eye's own evaluation lands above its
                // watermark and below the other's, and V547 gave that eye its
                // stream's previous evaluation: vectors one frame older than
                // the image, a valid pair that chained, and a double image on
                // every head move (Virtual Desktop, 3X, a quarter of the
                // resolves on that path). Both releases belong to one game
                // frame, so what either had published is this frame's.
                const std::uint64_t limit =
                    other_released_at != 0 ? std::max(released_at, other_released_at) : released_at;
                accept_limit = limit;
                const auto at_release = [&](const Candidate& candidate) {
                    return candidate.frame->publication <= limit
                        ? candidate.frame : candidate.previous;
                };
                const bool tie = chosen && other && chosen == other;
                unsigned path = 0;
                bool behind = false;
                if (!undecided) {
                    const Candidate& own = candidates[static_cast<std::size_t>(eye)];
                    chosen = at_release(own);
                    behind = chosen != own.frame;
                    path = tie ? 2U : 3U;
                } else if (tie) {
                    const Candidate& own = candidates[static_cast<std::size_t>(eye)];
                    chosen = at_release(own);
                    behind = chosen != own.frame;
                    alternate = at_release(candidates[static_cast<std::size_t>(1 - eye)]);
                    path = 0;
                } else if (chosen) {
                    const Candidate& unchosen =
                        candidates[0].frame->stream == chosen->stream ? candidates[1] : candidates[0];
                    alternate = at_release(unchosen);
                    path = 1;
                }
                record_pair_path(static_cast<std::uint64_t>(eye), path, behind, chosen.get());
            } else if (chosen && other && chosen == other) {
                if (candidates.size() >= 2) {
                    // Both evaluated before either release. Exactly two
                    // active streams keep the input-offset ordering; a
                    // third same-size stream is ambiguous.
                    if (candidates.size() != 2) return stale();
                    std::sort(candidates.begin(), candidates.end(),
                        [](const Candidate& a, const Candidate& b) {
                            const auto ao = input_offset(*a.frame), bo = input_offset(*b.frame);
                            return ao != bo ? ao < bo : a.first_publication < b.first_publication;
                        });
                    const bool measuring = order_measured_pair(candidates);
                    const auto at_release = [&](const Candidate& candidate) {
                        return candidate.frame->publication <= released_at
                            ? candidate.frame : candidate.previous;
                    };
                    chosen = at_release(candidates[static_cast<std::size_t>(eye)]);
                    if (measuring) alternate = at_release(candidates[static_cast<std::size_t>(1 - eye)]);
                } else {
                    // One shared stream, both eyes evaluated before either
                    // release: the cameras say which was the left
                    // (shared_stream_eye_order), else left first.
                    const auto later = std::find(frames.begin(), frames.end(), chosen);
                    const auto earlier = later + 1;
                    const auto second = *later;
                    const auto first = earlier != frames.end() ? *earlier : nullptr;
                    const SharedStreamOrder order = first
                        ? shared_stream_eye_order(*first, *second) : SharedStreamOrder{};
                    const bool take_second = (eye == 0) == order.left_is_second;
                    chosen = take_second ? second : first;
                    if (chosen && !take_second && (chosen->stream != second->stream ||
                        chosen->epoch != second->epoch ||
                        chosen->serial + 1 != second->serial)) return stale();
                    if (chosen && first) {
                        record_shared_stream_choice(static_cast<std::uint64_t>(eye), order,
                            *chosen, take_second);
                    }
                }
            }
            if (!chosen || frames.empty() || chosen->publication > accept_limit ||
                frames.front()->publication - chosen->publication > kMaximumStereoPublicationGap)
                return stale();
            if (other && other != chosen)
                shared_stream = other->stream == chosen->stream;
            auto result = one_eye(chosen, shared_stream);
            attach_alternate(result, alternate);
            return result;
        }

        if (!direct_match && eye_count == 1 && (eye == 0 || eye == 1)) {
            if (candidates.size() >= 2) {
                std::sort(candidates.begin(), candidates.end(),
                    [](const Candidate& a, const Candidate& b) {
                        return a.frame->publication > b.frame->publication;
                    });
                candidates.resize(2);
                const auto newest = candidates[0].frame->publication;
                const auto oldest = candidates[1].frame->publication;
                if (newest < oldest || newest - oldest > kMaximumStereoPublicationGap) {
                    return stale();
                }
                std::sort(candidates.begin(), candidates.end(),
                    [](const Candidate& a, const Candidate& b) {
                        const auto ao = input_offset(*a.frame), bo = input_offset(*b.frame);
                        if (ao != bo) return ao < bo;
                        return a.first_publication < b.first_publication;
                    });
                const bool measuring = order_measured_pair(candidates);
                auto frame = std::make_shared<DlssMotionVectorFrame>(
                    *candidates[static_cast<std::size_t>(eye)].frame);
                place_inset(*frame);
                auto result = std::make_shared<DlssMotionVectorSet>();
                result->eye_count = 1;
                result->eyes[0] = std::move(frame);
                ++state.statistics.matched;
                if (measuring) {
                    attach_alternate(result, candidates[static_cast<std::size_t>(1 - eye)].frame);
                }
                return result;
            }
            if (candidates.size() == 1 && streams_with_frames == 1) {
                const auto& current = candidates.front().frame;
                const auto& previous = candidates.front().previous;
                if (array_eyes(*current)) return one_eye(current, false);
                if (!previous && eye == 1) return stale();
                if (previous && current->stream == previous->stream &&
                    current->epoch == previous->epoch &&
                    current->serial == previous->serial + 1 &&
                    current->publication > previous->publication &&
                    current->publication - previous->publication <=
                        kMaximumStereoPublicationGap) {
                    // One stream for both eyes: the odd serial is the left
                    // eye, as for alternating-eye renderers, unless the
                    // inputs say otherwise. Each eye sees every other serial.
                    const auto& odd = (current->serial & 1U) != 0 ? current : previous;
                    const auto& even = (current->serial & 1U) != 0 ? previous : current;
                    const bool swapped = input_offset(*even) < input_offset(*odd);
                    const auto& chosen = (eye == 0) != swapped ? odd : even;
                    // The chosen eye's evaluation of this frame has to be the
                    // newest or the one just before it: a left eye evaluated
                    // with the right still to come is this frame's.
                    if (chosen == current || (current->serial & 1U) == 0) {
                        return one_eye(chosen, true);
                    }
                    return stale();
                }
                // A reset/gap cannot turn a requested eye into whichever
                // evaluation happened last. Only a first left publication
                // may continue through the ordinary path.
                if (previous) return stale();
            }
        }

        // Alternating-eye renderers such as Ghost of Tsushima's AER path reuse
        // one DLSS handle for both eyes. A complete stereo guide set therefore
        // consists of two consecutive publications from that single stream.
        // Even local serials close the stable eye-0/eye-1 pair; odd serials are
        // deliberately left pending until the second eye arrives.
        if (eye_count == 2 && candidates.size() == 1) {
            const auto& current = candidates.front().frame;
            const auto& previous = candidates.front().previous;
            if (previous && current->stream == previous->stream &&
                current->epoch == previous->epoch &&
                current->serial == previous->serial + 1 &&
                (current->serial & 1U) == 0 &&
                current->publication > previous->publication &&
                current->publication - previous->publication <=
                    kMaximumStereoPublicationGap) {
                auto left = std::make_shared<DlssMotionVectorFrame>(*previous);
                auto right = std::make_shared<DlssMotionVectorFrame>(*current);
                // The first of the pair is the left eye unless the inputs
                // say otherwise.
                if (input_offset(*right) < input_offset(*left)) std::swap(left, right);
                left->previous_serial = left->serial > 2 ? left->serial - 2 : 0;
                right->previous_serial = right->serial > 2 ? right->serial - 2 : 0;
                auto result = std::make_shared<DlssMotionVectorSet>();
                result->eye_count = 2;
                result->eyes[0] = std::move(left);
                result->eyes[1] = std::move(right);
                ++state.statistics.matched;
                return result;
            }
        }

        if (candidates.size() < eye_count) {
            if (streams_with_frames >= eye_count) {
                ++state.statistics.resolve_queue_mismatches;
                state.statistics.status = DlssMotionVectorStatus::queue_mismatch;
            } else {
                ++state.statistics.resolve_missing_streams;
                state.statistics.status = state.statistics.published == 0
                    ? DlssMotionVectorStatus::waiting_for_dlss
                    : DlssMotionVectorStatus::output_not_direct;
            }
            return {};
        }

        std::sort(candidates.begin(), candidates.end(), [](const Candidate& a,
                                                           const Candidate& b) {
            return a.frame->publication > b.frame->publication;
        });
        candidates.resize(eye_count);
        const auto newest = candidates.front().frame->publication;
        const auto oldest = candidates.back().frame->publication;
        if (newest < oldest || newest - oldest > kMaximumStereoPublicationGap) {
            return stale();
        }

        // Eye order follows the two DLSS evaluation streams rather than the
        // final colour resource, which survives any post-DLSS shader passes:
        // by where each stream's inputs sit, then by first-seen order.
        std::sort(candidates.begin(), candidates.end(), [](const Candidate& a,
                                                           const Candidate& b) {
            const auto ao = input_offset(*a.frame), bo = input_offset(*b.frame);
            if (ao != bo) return ao < bo;
            return a.first_publication < b.first_publication;
        });

        auto result = std::make_shared<DlssMotionVectorSet>();
        result->eye_count = eye_count;
        for (std::uint32_t index = 0; index < eye_count; ++index) {
            if (packed_stereo) {
                auto frame = std::make_shared<DlssMotionVectorFrame>(*candidates[index].frame);
                // Each mono producer uses local coordinates. Only the resolved
                // XR join maps colour to its half; native input rects stay.
                frame->output_x = index * frame->output_width;
                result->eyes[index] = std::move(frame);
            } else {
                result->eyes[index] = candidates[index].frame;
            }
        }
        ++state.statistics.matched;
        return result;
    } catch (...) {
        report_dlss_motion_vector_status(DlssMotionVectorStatus::invalid_input);
        return {};
    }
}

void report_dlss_motion_vector_status(DlssMotionVectorStatus status) noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        if (status == DlssMotionVectorStatus::temporal_mismatch) {
            ++state.statistics.temporal_rejections;
        } else if (status == DlssMotionVectorStatus::invalid_input) {
            ++state.statistics.invalid_rejections;
        }
        state.statistics.status = status;
    } catch (...) {
    }
}

void report_dlss_motion_vector_use() noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        ++state.statistics.used;
        state.statistics.last_used_publication = state.statistics.published;
        state.statistics.status = DlssMotionVectorStatus::used;
    } catch (...) {
    }
}

void report_dlss_hybrid_use() noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        ++state.statistics.used;
        ++state.statistics.hybrid_used;
        state.statistics.last_used_publication = state.statistics.published;
        state.statistics.status = DlssMotionVectorStatus::used;
    } catch (...) {
    }
}

DlssMotionVectorStatistics dlss_motion_vector_statistics() noexcept {
    try {
        auto& state = registry();
        std::scoped_lock lock(state.mutex);
        auto statistics = state.statistics;
        statistics.retired_snapshots_held = state.retired.size();
        return statistics;
    } catch (...) {
        return {};
    }
}

}  // namespace xrfg
