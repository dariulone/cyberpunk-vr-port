#pragma once
// StageReplay -- single-pass CPU recording for scene stages.
//
// The two views record every scene stage separately: RenderElements (0x33DDA4) drives the packet
// consumer 0x334CC0 once per stage per view, and that consumer is where most of the second view's
// recording cost sits (item preparation, constant uploads, batch flushes). Measured 2026-10-08:
// not recording one view's geometry at all is worth up to +14% in the CPU-bound city.
//
// This module lets ONE view (the source) record a stage natively while its D3D12 command stream is
// captured, and then re-issues that stream into the OTHER view's (the target's) command list in
// place of the target's own consumer call. The engine never runs the target's consumer for that
// stage, so its preparation and uploads are skipped; the captured draws read the source's uploads,
// which live until the frame retires. Tables that carry the source camera are rebuilt with the
// target camera, so the geometry is projected for the target eye.
//
// Everything here is pure data handling; the D3D12 side lives in src/Render/StageReplay.cpp.
#include <d3d12.h>
#include <array>
#include <cstdint>
#include <algorithm>
#include <cstring>
#include <vector>

namespace cvr::stereo::stage_replay {

enum class Op : uint8_t {
    None,
    // Graphics state -- replayable and restorable.
    Pipeline, RootSignature, RootTable, RootConstant, RootConstants, RootCbv, RootSrv, RootUav,
    Topology, IndexBuffer, VertexBuffers, Viewports, Scissors, BlendFactor, StencilRef, DepthBounds,
    Heaps, ShadingRate, ShadingImage, ViewInstanceMask,
    // Work -- replayable.
    Draw, DrawIndexed, Indirect,
    // Ignored by replay: diagnostic markers; descriptor copies into the shader-visible heap (device
    // calls, recorded for table contents); compute root state, which the engine's binding flush
    // also sets but no draw reads.
    Marker, Copy, ComputeState,
    // Blocking: a stream containing any of these is not replayed (barriers: see AllowBarriers).
    Dispatch, Targets, Barrier, CopyBuffer, CopyTexture, Clear, Discard, Query, Resolve,
    Immediate, Predication, Bundle, Reset, Other,
    Count
};
constexpr uint32_t OpCount = static_cast<uint32_t>(Op::Count);

#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/stage_replay.rs (tools/single_pass_tests/stage_replay_test runs its cases
// against them in its Rust variant). The stream and the list shadow stay C++ containers of StageReplay.cpp.
struct Command;struct Settings;struct Touched;
namespace detail {
extern "C" {
bool cvr_sr_class(uint32_t,uint8_t);
bool cvr_sr_check(uint32_t,uint64_t,uint64_t,uint64_t,bool);
uint8_t cvr_sr_decide(const Settings*,int32_t,uint32_t,uint32_t);
uint32_t cvr_sr_substitute_camera(uint64_t*,uint32_t,uint64_t,uint64_t,uint64_t,uint64_t);
bool cvr_sr_contains(const void*,uint64_t);
int32_t cvr_sr_substitute_view(uint64_t*,uint32_t,const void*,const void*);
void cvr_sr_touched(const Command*,size_t,Touched*);
}
}
inline bool Replayable(Op op) { return detail::cvr_sr_class(0, uint8_t(op)); }
inline bool Ignored(Op op) { return detail::cvr_sr_class(1, uint8_t(op)); }
inline bool Blocking(Op op) { return detail::cvr_sr_class(2, uint8_t(op)); }
inline bool IsDraw(Op op) { return detail::cvr_sr_class(3, uint8_t(op)); }
#else
constexpr bool Replayable(Op op) { return op >= Op::Pipeline && op <= Op::Indirect; }
constexpr bool Ignored(Op op) { return op == Op::Marker || op == Op::Copy || op == Op::ComputeState; }
constexpr bool Blocking(Op op) { return op >= Op::Dispatch && op < Op::Count; }
constexpr bool IsDraw(Op op) { return op == Op::Draw || op == Op::DrawIndexed || op == Op::Indirect; }
#endif

// 32 bytes. Field use per op is documented at the recording sites; arrays go to the payload.
struct Command {
    Op op{};
    uint8_t flags{};
    uint16_t count{};
    uint32_t a{};
    uint64_t b{}, c{}, d{};
};
static_assert(sizeof(Command) == 32);
constexpr uint64_t NoPayload = ~uint64_t(0);

// Fixed-capacity command stream. Never reallocates after Reserve, so a recording thread can append
// without allocating; an overflow is sticky and makes the stream unusable for replay.
class Stream {
public:
    // Limits are `commands`/`payloadBytes`; only a first slice is allocated up front, the rest grows
    // on demand and is kept by Clear(), so small stages do not commit megabytes each.
    void Reserve(size_t commands, size_t payloadBytes) {
        commands_.reserve(std::min<size_t>(commands, 2048)); payload_.reserve(std::min<size_t>(payloadBytes, 64u << 10));
        maxCommands_ = commands; maxPayload_ = payloadBytes;
    }
    void Clear() { commands_.clear(); payload_.clear(); overflow_ = false; blocking_ = 0; draws_ = 0; }
    // Appends `bytes` of payload (8-byte aligned) and stores its offset in command.d.
    bool Push(Command command, const void* data = nullptr, size_t bytes = 0) {
        if (overflow_) return false;
        if (commands_.size() >= maxCommands_) { overflow_ = true; return false; }
        if (data && bytes) {
            const size_t offset = (payload_.size() + 7) & ~size_t(7);
            if (offset + bytes > maxPayload_) { overflow_ = true; return false; }
            payload_.resize(offset + bytes);
            std::memcpy(payload_.data() + offset, data, bytes);
            command.d = offset;
        } else if (bytes == 0 && data == nullptr && command.d == 0) {
            command.d = NoPayload;
        }
        if (Blocking(command.op)) ++blocking_;
        if (IsDraw(command.op)) ++draws_;
        commands_.push_back(command);
        return true;
    }
    const std::vector<Command>& Commands() const { return commands_; }
#if CVR_RUST_VRIK
    const uint8_t* Payload(uint64_t offset, size_t bytes) const {
        return detail::cvr_sr_check(2, offset, bytes, payload_.size(), false) ? payload_.data() + offset : nullptr;
    }
#else
    const uint8_t* Payload(uint64_t offset, size_t bytes) const {
        if (offset == NoPayload || offset > payload_.size() || bytes > payload_.size() - offset) return nullptr;
        return payload_.data() + offset;
    }
#endif
    size_t PayloadBytes() const { return payload_.size(); }
    bool Overflowed() const { return overflow_; }
    uint32_t BlockingOps() const { return blocking_; }
    uint32_t Draws() const { return draws_; }
private:
    std::vector<Command> commands_;
    std::vector<uint8_t> payload_;
    size_t maxCommands_{}, maxPayload_{};
    bool overflow_{};
    uint32_t blocking_{}, draws_{};
};

// What the consumer hook does for one (side, stage) call.
enum class Action : uint8_t { Forward, Capture, Skip, Replay };
enum class Mode : uint32_t { Off = 0, Census = 1, Skip = 2, Replay = 3, ReplayCamera = 4 };

struct Settings {
    uint32_t mode{};
    uint64_t stages{};      // bit s: stage s (arguments+0x10, < 64) participates
    uint32_t source{1};     // side that records natively (1 = VRCAM)
};
#if CVR_RUST_VRIK
inline bool StageSelected(uint64_t mask, uint32_t stage) { return detail::cvr_sr_check(0, mask, stage, 0, false); }
inline Action Decide(const Settings& s, int32_t side, uint32_t node, uint32_t stage) {
    return static_cast<Action>(detail::cvr_sr_decide(&s, side, node, stage));
}
#else
constexpr bool StageSelected(uint64_t mask, uint32_t stage) { return stage < 64 && ((mask >> stage) & 1); }
constexpr Action Decide(const Settings& s, int32_t side, uint32_t node, uint32_t stage) {
    if (node != 0x33DDA4u || side < 0 || side > 1 || !StageSelected(s.stages, stage)) return Action::Forward;
    const bool target = uint32_t(side) != s.source;
    switch (static_cast<Mode>(s.mode)) {
    case Mode::Census: return Action::Capture;
    case Mode::Skip: return target ? Action::Skip : Action::Forward;
    case Mode::Replay:
    case Mode::ReplayCamera: return target ? Action::Replay : Action::Capture;
    default: return Action::Forward;
    }
}
#endif

// Graphics state of one command list since its last Reset, mirrored from the API calls. It is
// what the engine believes is bound, so replay restores exactly these values afterwards. A field
// that was never set since Reset is never restored: the engine has not set it either, so it will
// bind it before use.
struct Shadow {
    struct Param { uint8_t kind{}; uint8_t constants{}; uint64_t value{}; std::array<uint32_t, 16> words{}; uint16_t wordMask{}; };
    enum Kind : uint8_t { None = 0, Table = 1, Cbv = 2, Srv = 3, Uav = 4, Constants = 5 };
    uint64_t list{};
    uint32_t generation{};   // tracking generation the Reset was observed in (see StageReplay.cpp)
    bool valid{};            // tracking started at a Reset/ClearState
    bool overflow{};         // a value this mirror cannot hold was set (e.g. constants past word 16)
    ID3D12PipelineState* pso{}; bool psoSet{};
    ID3D12RootSignature* root{}; bool rootSet{};
    std::array<Param, 64> params{};
    D3D12_PRIMITIVE_TOPOLOGY topology{}; bool topologySet{};
    D3D12_INDEX_BUFFER_VIEW index{}; bool indexSet{}, indexNull{};
    std::array<D3D12_VERTEX_BUFFER_VIEW, 16> vertices{}; uint32_t vertexMask{};
    std::array<D3D12_VIEWPORT, 16> viewports{}; uint32_t viewportCount{}; bool viewportsSet{};
    std::array<D3D12_RECT, 16> scissors{}; uint32_t scissorCount{}; bool scissorsSet{};
    std::array<float, 4> blend{}; bool blendSet{};
    uint32_t stencil{}; bool stencilSet{};
    float depthMin{}, depthMax{1}; bool depthBoundsSet{};
    std::array<ID3D12DescriptorHeap*, 2> heaps{}; uint32_t heapCount{}; bool heapsSet{};
    D3D12_SHADING_RATE rate{}; std::array<D3D12_SHADING_RATE_COMBINER, 2> combiners{}; bool rateSet{};
    ID3D12Resource* rateImage{}; bool rateImageSet{};
    uint32_t viewMask{}; bool viewMaskSet{};

    void Reset(uint64_t forList, ID3D12PipelineState* initial, uint32_t gen = 0) {
        *this = Shadow{}; list = forList; valid = true; generation = gen;
        if (initial) { pso = initial; psoSet = true; }
    }
    void SetRoot(ID3D12RootSignature* r) { root = r; rootSet = true; params = {}; }
    void SetView(uint32_t param, uint8_t kind, uint64_t value) {
        if (param >= params.size()) { overflow = true; return; }
        auto& p = params[param]; p.kind = kind; p.value = value; p.wordMask = 0;
    }
    void SetConstants(uint32_t param, uint32_t count, const uint32_t* words, uint32_t offset) {
        if (param >= params.size() || !words || offset + count > 16) { overflow = true; return; }
        auto& p = params[param]; if (p.kind != Constants) { p = {}; p.kind = Constants; }
        for (uint32_t i = 0; i < count; ++i) { p.words[offset + i] = words[i]; p.wordMask |= uint16_t(1u << (offset + i)); }
    }
    void SetVertices(uint32_t start, uint32_t count, const D3D12_VERTEX_BUFFER_VIEW* views) {
        for (uint32_t i = 0; i < count; ++i) {
            const uint32_t slot = start + i; if (slot >= 16) { overflow = true; return; }
            vertices[slot] = views ? views[i] : D3D12_VERTEX_BUFFER_VIEW{};
            vertexMask |= 1u << slot;
        }
    }
    void SetViewports(uint32_t count, const D3D12_VIEWPORT* v) {
        if (count > 16) { overflow = true; return; }
        viewportCount = count; viewportsSet = true; for (uint32_t i = 0; i < count; ++i) viewports[i] = v[i];
    }
    void SetScissors(uint32_t count, const D3D12_RECT* r) {
        if (count > 16) { overflow = true; return; }
        scissorCount = count; scissorsSet = true; for (uint32_t i = 0; i < count; ++i) scissors[i] = r[i];
    }
    void SetHeaps(uint32_t count, ID3D12DescriptorHeap* const* h) {
        if (count > 2) { overflow = true; return; }
        heapCount = count; heapsSet = true; heaps = {}; for (uint32_t i = 0; i < count; ++i) heaps[i] = h[i];
    }
};

// Op categories a stream touched, so restore re-emits only what replay changed.
struct Touched {
    bool pso{}, root{}, params{}, topology{}, index{}, vertices{}, viewports{}, scissors{},
        blend{}, stencil{}, depthBounds{}, heaps{}, rate{}, rateImage{}, viewMask{};
#if CVR_RUST_VRIK
    static Touched Of(const Stream& s) {
        Touched t; detail::cvr_sr_touched(s.Commands().data(), s.Commands().size(), &t); return t;
    }
#else
    static Touched Of(const Stream& s) {
        Touched t;
        for (const auto& c : s.Commands()) switch (c.op) {
            case Op::Pipeline: t.pso = true; break;
            case Op::RootSignature: t.root = t.params = true; break;
            case Op::RootTable: case Op::RootConstant: case Op::RootConstants:
            case Op::RootCbv: case Op::RootSrv: case Op::RootUav: t.params = true; break;
            case Op::Topology: t.topology = true; break;
            case Op::IndexBuffer: t.index = true; break;
            case Op::VertexBuffers: t.vertices = true; break;
            case Op::Viewports: t.viewports = true; break;
            case Op::Scissors: t.scissors = true; break;
            case Op::BlendFactor: t.blend = true; break;
            case Op::StencilRef: t.stencil = true; break;
            case Op::DepthBounds: t.depthBounds = true; break;
            case Op::Heaps: t.heaps = true; break;
            case Op::ShadingRate: t.rate = true; break;
            case Op::ShadingImage: t.rateImage = true; break;
            case Op::ViewInstanceMask: t.viewMask = true; break;
            default: break;
        }
        return t;
    }
#endif
};

// Why a target call fell back to its native consumer.
enum class Fallback : uint32_t {
    None, NoCapture, StaleFrame, CaptureOverflow, CaptureBlocking, CaptureList, Busy, NoContext,
    ShadowInvalid, ShadowOverflow, NoHeap, Camera, Capacity, Ordinal, Leader, Count
};

// Pairs the k-th call of a stage in the target view with the k-th capture of the same stage in the
// source view of the same engine frame.
constexpr uint32_t MaxOrdinals = 8;
#if CVR_RUST_VRIK
inline bool SameFrame(uint32_t captureFrame, uint32_t targetFrame, bool captureComplete) {
    return detail::cvr_sr_check(1, captureFrame, targetFrame, 0, captureComplete);
}

// Replace the source camera descriptor(s) in a table's source list. Returns how many were replaced.
inline uint32_t SubstituteCamera(uint64_t* sources, uint32_t count, uint64_t fromVs, uint64_t toVs,
                                 uint64_t fromPs, uint64_t toPs) {
    return detail::cvr_sr_substitute_camera(sources, count, fromVs, toVs, fromPs, toPs);
}
#else
constexpr bool SameFrame(uint32_t captureFrame, uint32_t targetFrame, bool captureComplete) {
    return captureComplete && captureFrame == targetFrame;
}

// Replace the source camera descriptor(s) in a table's source list. Returns how many were replaced.
inline uint32_t SubstituteCamera(uint64_t* sources, uint32_t count, uint64_t fromVs, uint64_t toVs,
                                 uint64_t fromPs, uint64_t toPs) {
    uint32_t replaced = 0;
    for (uint32_t i = 0; i < count; ++i) {
        if (fromVs && sources[i] == fromVs && toVs) { sources[i] = toVs; ++replaced; }
        else if (fromPs && sources[i] == fromPs && toPs) { sources[i] = toPs; ++replaced; }
    }
    return replaced;
}
#endif

// The descriptors a replayed table may hold that belong to the recording VIEW (census 841-843,
// tools/single_pass_tests/stage_table_diff.py): constant-buffer slot 1 (camera) and slot 0 (view
// buffer), and shader-resource slot 37 (a view texture deferred decals read), each for the vertex
// and pixel stage. The render context's own pass buffers (constant-buffer slots 3, 5, 7) also
// differ between the views but must stay the source's: they describe the source's packets
// (instance ranges), and swapping them broke every draw (842).
struct ViewDescriptors {
    enum : uint32_t { CameraVs, CameraPs, ViewVs, ViewPs, TextureVs, TexturePs, Count };
    std::array<uint64_t, Count> d{};
#if CVR_RUST_VRIK
    bool Contains(uint64_t v) const { return detail::cvr_sr_contains(d.data(), v); }
#else
    bool Contains(uint64_t v) const {
        if (!v) return false;
        for (const auto x : d) if (x == v) return true;
        return false;
    }
#endif
};
// Replaces each source view descriptor with the target's. Returns the number replaced, or -1 when
// an entry belongs to the source view but the target has no counterpart (the table cannot be
// rebuilt for the target).
#if CVR_RUST_VRIK
inline int32_t SubstituteView(uint64_t* sources, uint32_t count, const ViewDescriptors& from, const ViewDescriptors& to) {
    return detail::cvr_sr_substitute_view(sources, count, from.d.data(), to.d.data());
}
static_assert(sizeof(Settings) == 24 && sizeof(Touched) == 15 && sizeof(ViewDescriptors) == 48, "mirrored by stage_replay.rs");
#else
inline int32_t SubstituteView(uint64_t* sources, uint32_t count, const ViewDescriptors& from, const ViewDescriptors& to) {
    int32_t replaced = 0;
    for (uint32_t i = 0; i < count; ++i) {
        if (!sources[i]) continue;
        for (uint32_t k = 0; k < ViewDescriptors::Count; ++k) {
            if (sources[i] != from.d[k]) continue;
            if (!to.d[k]) return -1;
            sources[i] = to.d[k]; ++replaced;
            break;
        }
    }
    return replaced;
}
#endif

// Runtime entry points (src/Render/StageReplay.cpp).
bool Active();
// Mixed into the render-graph cache key at 0x40A1D1 while a graph-builder patch is applied.
uint64_t GraphSalt();
using ConsumerFn = __int64(__fastcall*)(void*, const void*, const void*);
// Wraps one call of the packet consumer 0x334CC0: counts, captures, skips or replays it.
__int64 Consume(int32_t side, uint32_t node, void* renderer, const void* arguments, const void* span, ConsumerFn forward);
// Device CopyDescriptors observer (contents of the tables a captured stream binds).
void Copy(ID3D12Device*, UINT, const D3D12_CPU_DESCRIPTOR_HANDLE*, const UINT*, UINT,
          const D3D12_CPU_DESCRIPTOR_HANDLE*, const UINT*, D3D12_DESCRIPTOR_HEAP_TYPE);

}  // namespace cvr::stereo::stage_replay
