#pragma once
#include <cstdint>

// Diagnostic-only ceiling probe for single-pass stereo. It answers one question with live GPU time:
// how much of one eye's native geometry workload is NOT pixel work, i.e. the part a view-instanced
// draw could share between the eyes. Two reversible actions apply to native draws recorded inside
// one selected render-graph node (RenderElements by default) for the selected eye(s):
//   EmptyScissor - forward the draw with a zero-area scissor: IA/VS/primitive setup still execute,
//                  rasterization/PS/ROP do not. Remaining node time bounds the shareable work.
//   Skip         - do not forward the draw at all: the node's whole geometry cost for that eye.
// Both corrupt the selected eye's image while active. The control defaults to zero and is never set
// by settings; only the live measurement script writes it and restores it to zero.
namespace cvr::stereo::geometry_cost {

enum class Action : uint8_t { Forward = 0, EmptyScissor = 1, Skip = 2 };

// Control word layout: bits 0..1 action (0 off, 1 empty scissor, 2 skip, 3 invalid -> off),
// bit 4 selects side 0 (MAIN), bit 5 selects side 1 (VRCAM). Engine stereo draws both eyes in side 0, each
// into its half of one side-by-side target: bit 6 restricts the action to draws whose viewport starts at x = 0
// (the left eye), bit 7 to draws whose viewport starts at its own width (the right half). Other bits must be zero.
constexpr uint32_t kActionMask = 0x3u;
constexpr uint32_t kSide0 = 0x10u;
constexpr uint32_t kSide1 = 0x20u;
constexpr uint32_t kLeftHalf = 0x40u;
constexpr uint32_t kRightHalf = 0x80u;
constexpr uint32_t kKnownBits = kActionMask | kSide0 | kSide1 | kLeftHalf | kRightHalf;

// The eye bits against the draw's viewport (x = TopLeftX, width = Width, rounded); unknown viewport -> no.
constexpr bool EyeAllows(uint32_t control, bool known, uint32_t x, uint32_t width) {
    if (!(control & (kLeftHalf | kRightHalf))) return true;
    if (!known) return false;
    const bool right = x > 0 && (x > width ? x - width : width - x) <= 1;
    const bool left = x == 0;
    return ((control & kLeftHalf) && left) || ((control & kRightHalf) && right);
}
// RenderElements work function RVA (src/Stereo/NodeNames.inc), the geometry passes of both eyes.
constexpr uint32_t kRenderElementsNode = 0x33DDA4u;

constexpr Action Decide(uint32_t control, uint32_t nodeFilter, uint32_t node, int side) {
    if (!control || (control & ~kKnownBits)) return Action::Forward;
    const uint32_t action = control & kActionMask;
    if (action == 0 || action == 3) return Action::Forward;
    if (side != 0 && side != 1) return Action::Forward;
    if (!(control & (side ? kSide1 : kSide0))) return Action::Forward;
    const uint32_t wanted = nodeFilter ? nodeFilter : kRenderElementsNode;
    if (!node || node != wanted) return Action::Forward;
    return static_cast<Action>(action);
}

// The engine's last scissor on one recording thread. A draw-time override must restore exactly what
// the engine set, not a guess; an unknown list falls back to the widest legal rectangle so geometry
// after the probe is never clipped by our override.
struct ScissorState {
    static constexpr unsigned kMax = 16;
    const void* list{};
    unsigned count{};
    int32_t rects[kMax][4]{};

    void Track(const void* owner, unsigned n, const int32_t (*values)[4]) {
        list = owner;
        count = n > kMax ? kMax : n;
        for (unsigned i = 0; i < count; ++i)
            for (unsigned j = 0; j < 4; ++j) rects[i][j] = values[i][j];
    }
    void Forget(const void* owner) {
        if (list == owner) { list = nullptr; count = 0; }
    }
    // Returns the number of rectangles to restore into out; 0 means "use the widest rectangle".
    unsigned Restore(const void* owner, int32_t (*out)[4]) const {
        if (owner != list || !count) return 0;
        for (unsigned i = 0; i < count; ++i)
            for (unsigned j = 0; j < 4; ++j) out[i][j] = rects[i][j];
        return count;
    }
};

}  // namespace cvr::stereo::geometry_cost
