#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The grasp's natives (side: 0 left, 1 right): the script's parameters read here, the grasp itself in
// src/Anim/GraspPose.cpp or its Rust port (Anim/GraspPose.hpp's cvr_grasp_*).
#if !CVR_RUST_VRIK
// The C++ build's natives; the Rust build's are in rust/crates/cvr-rig/src/vrik/grasp.rs.
#include "Anim/GraspPose.hpp"
#include "Natives/NativeFunctions.hpp"

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/Transform.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>

namespace {
// side, shape, the three half sizes, a position and an orientation: VRGraspSeat's and VRGraspSet's parameters.
struct GraspArgs {
    int32_t side = -1, shape = 0;
    float half[3]{};
    RED4ext::Vector4 p{};
    RED4ext::Quaternion q{};
};
GraspArgs ReadGraspArgs(RED4ext::CStackFrame* aFrame) {
    GraspArgs a;
    RED4ext::GetParameter(aFrame, &a.side);
    RED4ext::GetParameter(aFrame, &a.shape);
    RED4ext::GetParameter(aFrame, &a.half[0]);
    RED4ext::GetParameter(aFrame, &a.half[1]);
    RED4ext::GetParameter(aFrame, &a.half[2]);
    RED4ext::GetParameter(aFrame, &a.p);
    RED4ext::GetParameter(aFrame, &a.q);
    aFrame->code++;
    return a;
}
}  // namespace

// VRGraspSeat(side, shape, hx, hy, hz, position, orientation) -> Transform: where a thing of that shape, at that
// pose in the slot when the hand takes it, lies for its grasp, in the slot's frame. position.W = 1 when seated, 0
// when the palm is not measured yet.
void VRGraspSeat(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, RED4ext::Transform* aOut, int64_t) {
    const GraspArgs a = ReadGraspArgs(aFrame);
    if (!aOut) return;
    aOut->position.X = aOut->position.Y = aOut->position.Z = aOut->position.W = 0.0f;
    aOut->orientation = a.q;
    const float at[3] = {a.p.X, a.p.Y, a.p.Z};
    const float in[4] = {a.q.i, a.q.j, a.q.k, a.q.r};
    float pos[3], rot[4];
    if (!cvr_grasp_seat(a.side, a.shape, a.half, at, in, pos, rot)) return;
    aOut->position.X = pos[0];
    aOut->position.Y = pos[1];
    aOut->position.Z = pos[2];
    aOut->position.W = 1.0f;
    aOut->orientation.i = rot[0];
    aOut->orientation.j = rot[1];
    aOut->orientation.k = rot[2];
    aOut->orientation.r = rot[3];
}

// VRGraspSet(side, shape, hx, hy, hz, position, orientation) -> Bool: the hand holds that thing there (slot frame);
// its fingers are closed on it from the next pass.
void VRGraspSet(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    const GraspArgs a = ReadGraspArgs(aFrame);
    const float at[3] = {a.p.X, a.p.Y, a.p.Z};
    const float in[4] = {a.q.i, a.q.j, a.q.k, a.q.r};
    const bool ok = cvr_grasp_set(a.side, a.shape, a.half, at, in);
    if (aOut) *aOut = ok;
}

// VRGraspPin(side, pinned) -> Bool: whether the slot is held at the rig's reference while this grasp lasts.
void VRGraspPin(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    int32_t side = -1;
    bool pinned = true;
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &pinned);
    aFrame->code++;
    const bool ok = cvr_grasp_pin(side, pinned);
    if (aOut) *aOut = ok;
}

// VRGraspBlend(side, blend) -> Bool: how far the fingers are on the grasp (0 tracking, 1 the grasp).
void VRGraspBlend(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    int32_t side = -1;
    float b = 0.0f;
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &b);
    aFrame->code++;
    const bool ok = cvr_grasp_blend(side, b);
    if (aOut) *aOut = ok;
}

// VRGraspClear(side) -> Bool: the hand holds nothing; its fingers are the tracking's again.
void VRGraspClear(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    int32_t side = -1;
    RED4ext::GetParameter(aFrame, &side);
    aFrame->code++;
    const bool ok = cvr_grasp_clear(side);
    if (aOut) *aOut = ok;
}
#endif
#endif
