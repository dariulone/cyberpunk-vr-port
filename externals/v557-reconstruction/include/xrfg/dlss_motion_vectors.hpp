#pragma once

#include <d3d12.h>
#include <wrl/client.h>

#include <cstdint>
#include <array>
#include <memory>

namespace xrfg {

// The camera a DLSS evaluation rendered (V515), from NVIDIA Streamline's
// per-viewport constants: the tangents of its image's edges - left and
// right where the image's x runs from its left to its right edge, up and
// down at its top and bottom row - and where the camera sits. MSFS 2024's
// foveated rendering evaluates DLSS once per view, four views for two eyes,
// none of them the size of an eye image; this is what places each in the
// eye it belongs to.
struct DlssViewGeometry {
    bool known{};
    std::uint32_t viewport{};
    float tan_left{};
    float tan_right{};
    float tan_up{};
    float tan_down{};
    std::array<float, 3> position{};
    std::array<float, 3> right{};
    // V524: the rest of the camera's basis (Streamline's cameraUp and
    // cameraFwd), for the rotation between two evaluations' cameras.
    std::array<float, 3> up{};
    std::array<float, 3> forward{};
};

// Four tangents in one flight-log word (V515): left, right, up, down from
// the low bits, each a signed 16-bit count of 4096ths.
[[nodiscard]] inline std::uint64_t pack_view_tangents(
    float left, float right, float up, float down) noexcept {
    const auto field = [](float value) -> std::uint64_t {
        float scaled = value * 4096.0F;
        if (!(scaled > -32768.0F)) scaled = -32768.0F;  // and NaN
        if (scaled > 32767.0F) scaled = 32767.0F;
        return static_cast<std::uint64_t>(static_cast<std::uint16_t>(
            static_cast<std::int16_t>(scaled)));
    };
    return field(left) | (field(right) << 16U) | (field(up) << 32U) | (field(down) << 48U);
}

struct DlssMotionVectorPublication {
    std::uint64_t stream{};
    ID3D12Resource* output{};
    ID3D12Resource* motion_vectors{};
    ID3D12CommandQueue* producer_queue{};
    std::uint32_t output_x{};
    std::uint32_t output_y{};
    std::uint32_t output_width{};
    std::uint32_t output_height{};
    std::uint32_t motion_x{};
    std::uint32_t motion_y{};
    std::uint32_t motion_width{};
    std::uint32_t motion_height{};
    float scale_x{1.0F};
    float scale_y{1.0F};
    float jitter_x{};
    float jitter_y{};
    D3D12_RESOURCE_STATES resource_state{
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    bool jittered{};
    bool reset{};
    ID3D12GraphicsCommandList* producer_command_list{};
    // Optional frame-generation guides. The established DLSS-vector route
    // remains valid when these fields are absent.
    ID3D12Resource* depth{};
    std::uint32_t depth_x{};
    std::uint32_t depth_y{};
    std::uint32_t depth_width{};
    std::uint32_t depth_height{};
    D3D12_RESOURCE_STATES depth_resource_state{
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    float frame_time_delta_ms{16.6667F};
    float camera_near{0.1F};
    float camera_far{1000.0F};
    bool depth_inverted{};
    bool depth_infinite{};
    // A cooperating producer may supply the native D3D12 device after it has
    // unwrapped and identity-checked every object in the publication.
    ID3D12Device* verified_producer_device{};
    // V515: the evaluation's camera, where the producer knows it.
    DlssViewGeometry view{};
    // V540: under R.E.A.L. VR, the DLSS feature handle its fix evaluated
    // through - one per eye - as a small number (first seen order + 1); 0
    // elsewhere.
    std::uint64_t eye_tag{};
};

struct DlssMotionVectorFrame {
    std::uint64_t stream{};
    std::uint64_t epoch{};
    std::uint64_t serial{};
    std::uint64_t previous_serial{};
    std::uint64_t publication{};
    Microsoft::WRL::ComPtr<ID3D12Resource> output;
    Microsoft::WRL::ComPtr<ID3D12Resource> motion_vectors;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> producer_queue;
    std::uint32_t output_x{};
    std::uint32_t output_y{};
    std::uint32_t output_width{};
    std::uint32_t output_height{};
    std::uint32_t output_slice{};
    std::uint32_t motion_x{};
    std::uint32_t motion_y{};
    std::uint32_t motion_width{};
    std::uint32_t motion_height{};
    std::uint32_t motion_slice{};
    float scale_x{1.0F};
    float scale_y{1.0F};
    float jitter_x{};
    float jitter_y{};
    float previous_jitter_x{};
    float previous_jitter_y{};
    D3D12_RESOURCE_STATES resource_state{
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    bool jittered{};
    bool reset{};
    Microsoft::WRL::ComPtr<ID3D12Resource> depth;
    std::uint32_t depth_x{};
    std::uint32_t depth_y{};
    std::uint32_t depth_width{};
    std::uint32_t depth_height{};
    D3D12_RESOURCE_STATES depth_resource_state{
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    float frame_time_delta_ms{16.6667F};
    float camera_near{0.1F};
    float camera_far{1000.0F};
    bool depth_inverted{};
    bool depth_infinite{};
    DlssViewGeometry view{};
    // V540: the publication's eye_tag.
    std::uint64_t eye_tag{};
    // V515: a guide placed by its camera rather than pixel for pixel. The
    // output rectangle is then the eye's image rectangle the guide covers,
    // and placed_motion_rect (x, y, width, height in vector texels, not
    // whole) the part of the vectors that lands on it: MSFS 2024's foveated
    // periphery spreads 892 render columns over a 5092-pixel eye.
    bool placed{};
    std::array<float, 4> placed_motion_rect{};
    // V516: the camera of the stream's previous evaluation (known = false
    // after a reset or for a stream's first). MSFS 2024's inset follows the
    // eyes, so its field moves from one frame to the next.
    DlssViewGeometry previous_view{};
    // V516: on a periphery placed by its camera, the eye's inset - the view
    // at full density where the eye looks - evaluated for the same frame.
    // On the inset frame itself, where its field lands in the eye image now
    // and at its previous evaluation (x, y, width, height in image pixels,
    // not whole).
    std::shared_ptr<const DlssMotionVectorFrame> inset;
    std::array<float, 4> view_rect{};
    std::array<float, 4> previous_view_rect{};
    bool previous_view_rect_known{};
};

constexpr std::uint32_t kDlssMotionVectorEyeCount = 2;

// An eye's view in its XR image (V515): the tangents of its field of view
// (XrFovf) and its image rectangle.
struct DlssEyeView {
    float tan_left{};
    float tan_right{};
    float tan_up{};
    float tan_down{};
    std::int32_t x{};
    std::int32_t y{};
    std::uint32_t width{};
    std::uint32_t height{};
};

struct DlssMotionVectorSet {
    std::array<std::shared_ptr<const DlssMotionVectorFrame>,
               kDlssMotionVectorEyeCount>
        eyes;
    std::uint32_t eye_count{};
    // V546: an eye of a swapchain per eye whose two DLSS streams nothing
    // tells apart (the same input offset, no camera) while their order is
    // still being measured: the other stream's evaluation, and the eye (0
    // left, 1 right) this set was resolved for. The synthesizer compares
    // both against the eye's images (report_dlss_eye_order).
    std::shared_ptr<const DlssMotionVectorFrame> alternate;
    int resolved_eye{-1};
};

enum class DlssMotionVectorStatus : std::uint32_t {
    disabled,
    waiting_for_dlss,
    output_not_direct,
    queue_mismatch,
    invalid_input,
    temporal_mismatch,
    used,
};

struct DlssMotionVectorStatistics {
    std::uint64_t published{};
    std::uint64_t depth_publications{};
    std::uint64_t invalid_publications{};
    std::uint64_t snapshot_copies{};
    std::uint64_t snapshot_failures{};
    std::uint64_t resolve_calls{};
    std::uint64_t resolve_missing_streams{};
    std::uint64_t resolve_stale_pairs{};
    std::uint64_t resolve_queue_mismatches{};
    std::uint64_t matched{};
    std::uint64_t temporal_rejections{};
    std::uint64_t invalid_rejections{};
    std::uint64_t used{};
    // Pairs composed by the hybrid: vectors and optical flow both (V497).
    std::uint64_t hybrid_used{};
    // V515: eye pairs joined by their cameras, and attempts that found
    // cameras but no guide covering both eyes.
    std::uint64_t view_matched{};
    std::uint64_t view_unmatched{};
    // V516: eye pairs joined by cameras whose insets were found too.
    std::uint64_t inset_matched{};
    // V533: evaluations placed on an eye image that is a window of them.
    std::uint64_t inset_placed{};
    // V519: let-go snapshot sets still waiting for the GPU to finish with
    // them (a retired stream, a reallocated slot), counted when read.
    std::uint64_t retired_snapshots_held{};
    std::uint64_t last_used_publication{};
    DlssMotionVectorStatus status{DlssMotionVectorStatus::waiting_for_dlss};
};

void publish_dlss_motion_vectors(
    const DlssMotionVectorPublication& publication) noexcept;
// Whether two ID3D12Device interfaces are the same device underneath. An
// interposer such as NVIDIA Streamline hands the game a proxy device and
// proxy queues while resources report the real device, so pointer identity
// says "different" for one and the same GPU device. Identical pointers are
// the same; otherwise, on one adapter, a private-data value set through one
// interface and read back through the other proves shared backing. Verified
// pairs are remembered; the check never guesses.
bool same_d3d12_device_backing(ID3D12Device* left, ID3D12Device* right) noexcept;
void configure_dlss_motion_vector_tracking(bool enabled) noexcept;
// Call only after all consumer queue work that may reference published
// snapshots has completed. A live disable intentionally retains them until
// this safe point. Since V519 the textures still wait for their producer
// queues (retire_dlss_motion_vector_stream too) before they are freed.
void retire_disabled_dlss_motion_vector_resources() noexcept;
void retire_dlss_motion_vector_stream(std::uint64_t stream) noexcept;
// One DLSS evaluation for both eyes side by side, each eye then in a
// swapchain of its own at that swapchain's origin (V513, No Man's Sky): the
// guide eye `eye` (0 left, 1 right) reads is the evaluation's half, placed at
// the view's origin - its output the view's size, its vector and depth
// rectangles the evaluation's matching halves. Null unless the evaluation
// spans exactly two views of view_width x view_height side by side.
std::shared_ptr<const DlssMotionVectorFrame> split_both_eyes_guide(
    const DlssMotionVectorFrame& frame,
    std::uint32_t view_width,
    std::uint32_t view_height,
    std::uint32_t eye) noexcept;
// The guides for an XR image. With an array image both eyes come as a
// stereo set. With a one-slice image (a swapchain per eye) and its eye (0 or
// 1; -1 unknown), the set holds that eye's own evaluation: chosen by the
// publication watermark at the eye's colour release (released_at, with the
// other eye's other_released_at), or without one by the order the stereo
// path gives two - two streams, or one stream alternating eyes (V497, from
// 0.3's R15).
std::shared_ptr<const DlssMotionVectorSet> resolve_dlss_motion_vectors(
    ID3D12Resource* output,
    ID3D12CommandQueue* consumer_queue,
    int eye = -1,
    std::uint64_t released_at = 0,
    std::uint64_t other_released_at = 0) noexcept;
// A guide placed into an eye's view by its camera (V515): the copy whose
// output rectangle is the view's rectangle and whose placed_motion_rect is
// the part of its vectors (and depth) that lands there. Null unless the
// guide's camera covers the view, to within a hundredth of its field.
std::shared_ptr<const DlssMotionVectorFrame> place_guide_in_view(
    const DlssMotionVectorFrame& frame,
    const DlssEyeView& view) noexcept;
// Where a camera's field lands in an eye's view (V516): x, y, width, height
// in the view's image pixels, not whole; the view's tangents map linearly.
std::array<float, 4> view_rect_in_eye(
    const DlssViewGeometry& view,
    const DlssEyeView& eye) noexcept;
// Both eyes' guides from evaluations whose cameras are known (V515), for a
// game whose DLSS outputs are views of their own rather than its eye images
// - MSFS 2024's foveated rendering: per eye a periphery over the whole field
// at a fraction of its pixels, and an inset. Each eye takes the recent
// stream whose camera covers its field and fits it best, placed scaled; the
// evaluation is the stream's newest by the eye's release watermark. V549:
// where the two releases differ, the streams evaluated before the first
// eye's release are that eye's and those between the releases the other's,
// and the fit chooses within each group; the cameras' positions decide
// between two that fit alike only where the releases say nothing. Both
// eyes or neither, from two streams. V516: each eye's periphery carries the
// eye's inset when one is found - a recent stream whose field lies inside
// the eye's, narrower than it, whose camera sits where the eye's periphery
// camera does - with where its field lands now and at its previous
// evaluation.
std::array<std::shared_ptr<const DlssMotionVectorSet>, kDlssMotionVectorEyeCount>
resolve_view_placed_dlss_guides(
    ID3D12CommandQueue* consumer_queue,
    const std::array<DlssEyeView, kDlssMotionVectorEyeCount>& eyes,
    const std::array<std::uint64_t, kDlssMotionVectorEyeCount>& released_at) noexcept;
// The process-monotonic publication count: a release watermark.
std::uint64_t dlss_motion_vector_publications() noexcept;
// V524: alternating-eye rendering (Luke Ross's R.E.A.L. VR). The game renders
// one eye per frame and evaluates one DLSS stream for both, in turn; which
// eye an evaluation is for cannot be read from its inputs (the same camera,
// the same rectangles, the order of the two in a frame changing whenever a
// frame carries one evaluation or none). While required, such a stream's
// evaluations go to the eyes by their serial's parity once the stream has
// been anchored in its epoch - its left eye takes `left_parity` - and guide
// no eye before. A reset (a new epoch) needs a new anchor.
// V540: R.E.A.L. VR's fix evaluates each eye through a DLSS feature handle
// of its own (RealVR64.dll keeps the handle on its frame parity, the eye it
// renders), and an evaluation's eye_tag says which. Anchored by the two
// tags, the eyes go by tag: the whole session, through resets and every
// stream the handles publish into. A stream without them (the fix not
// running: one handle) keeps the serial parity.
void configure_dlss_alternating_eyes(bool required) noexcept;
void anchor_dlss_alternating_eyes(
    std::uint64_t stream, std::uint64_t epoch, std::uint32_t left_parity) noexcept;
void anchor_dlss_alternating_eye_tags(std::uint64_t left_tag, std::uint64_t right_tag) noexcept;
// V541: the eye (0 left, 1 right, -1 before the anchor) an alternating
// stream's evaluation of this epoch, serial and eye_tag goes to.
int dlss_alternating_eye(
    std::uint64_t stream, std::uint64_t epoch, std::uint64_t serial, std::uint64_t tag) noexcept;
// V542: how many of its own evaluations each eye's picture lags its newest at
// a release, 0 to 2: `newest` for the eye that owns the stream's newest
// evaluation, `other` for the other eye (the two trade places when a frame
// carries one evaluation). Cyberpunk 2077 under AERv2: 0 and 1 on
// 2026-10-19 (V524's rule, the default), 1 and 1 then about 2 and 1 on
// 2026-10-20 - the eye given a newer evaluation's vectors than its picture
// had its contour off by the strafe's speed. Measured (the eye measurement's
// votes, record 864).
void set_dlss_alternating_skips(std::uint32_t newest, std::uint32_t other) noexcept;
// V546: two DLSS streams, one per eye, that nothing tells apart - each eye
// in a swapchain of its own, both inputs at the same offset, no camera (Star
// Citizen) - went to the eyes by which one the resolver saw first, and that
// came from the order the capture iterated its features: right after a
// session that started with Hybrid on, swapped after Hybrid was switched on
// in game. The order is now measured. While a pair is undecided, each eye's
// set carries the other stream's evaluation (DlssMotionVectorSet::alternate)
// and the synthesizer reports, over the pixels where the two streams'
// vectors part, how many the eye's own images favour each: the current
// pixel against the previous image read at the end of each vector. Once the
// votes agree, the pair's left stream is fixed for the session.
// `informative` is the pixels compared.
void report_dlss_eye_order(std::uint32_t eye, std::uint64_t assigned_stream,
    std::uint64_t alternate_stream, std::uint64_t assigned_votes,
    std::uint64_t alternate_votes, std::uint64_t informative) noexcept;
// The left stream decided for the pair, or 0 while it is undecided.
std::uint64_t dlss_eye_order_left(std::uint64_t first, std::uint64_t second) noexcept;
struct DlssStreamPosition {
    bool known{};
    // Its newest evaluation has an eye: by its tag, or by its serial parity
    // in the stream's current epoch.
    bool anchored{};
    // V540: by its tag.
    bool tag_anchored{};
    std::uint64_t epoch{};
    std::uint64_t serial{};
    std::uint64_t publication{};
};
// Where a stream stands: its newest publication's epoch, serial and number.
DlssStreamPosition dlss_motion_vector_stream_position(std::uint64_t stream) noexcept;
// V533: each eye's view as the layer submits it (its field and its
// rectangle on the eye image), set at every xrEndFrame mapping. An eye image
// can be a window of a wider DLSS output: R.E.A.L. VR's Ghost of Tsushima
// renders 4192 x 3360 with a camera of +-1.2043 and submits each eye as its
// middle 4003 x 3360, +-1.1501. A stream whose camera holds the eye's field
// is then that eye's guide, placed on the window (place_guide_in_view).
// V534: without a camera (Kingdom Come calls NGX directly), an alternating
// stream at most 6% larger per axis than a symmetric eye's image is placed
// on its middle, at the output's scale. V537: one smaller than the image, of
// its shape within 1% and at most 3 times smaller, is placed over the whole
// image (Avatar: Frontiers of Pandora under R.E.A.L. VR).
void set_dlss_eye_view(std::uint32_t eye, const DlssEyeView& view) noexcept;
// V533: where the newest evaluation of `stream` puts its output's origin on
// eye `eye`'s image, in that image's pixels, when the eye is such a window
// at the output's own scale (within a percent); false otherwise.
bool dlss_inset_origin(std::uint64_t stream, std::uint32_t eye,
    std::int32_t* x, std::int32_t* y) noexcept;
void report_dlss_motion_vector_status(DlssMotionVectorStatus status) noexcept;
void report_dlss_motion_vector_use() noexcept;
// A pair the hybrid composition used: counted as used, and as hybrid.
void report_dlss_hybrid_use() noexcept;
DlssMotionVectorStatistics dlss_motion_vector_statistics() noexcept;

}  // namespace xrfg
