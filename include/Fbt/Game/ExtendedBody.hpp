#pragma once
#include "Fbt/FbtIk.hpp"
#include <algorithm>
#include <utility>

namespace cvr::fbt::extended {
using namespace wrpuppets::fbt;
inline bool Finite(Vec3 p) { return std::isfinite(p.x) && std::isfinite(p.y) && std::isfinite(p.z); }
inline bool Finite(Quat q) { return std::isfinite(q.x) && std::isfinite(q.y) && std::isfinite(q.z) && std::isfinite(q.w); }

// Rotate a solved limb's bending plane toward an anatomical joint from SlimeVR.
// The wrist/ankle stays at the original IK goal, with the original orientation;
// bone lengths and the shoulder/hip position stay unchanged. A straight limb or
// a pole on its axis has no defined plane, so keep the ordinary solver's result.
inline bool ApplyPole(const Skeleton& sk, Pose& pose, int upper, int middle, int end, Vec3 pole) {
    if (upper < 0 || middle < 0 || end < 0 || upper >= sk.Count() || middle >= sk.Count() || end >= sk.Count() ||
        sk.BoneAt(middle).parent != upper || sk.BoneAt(end).parent != middle || !Finite(pole)) return false;
    const Vec3 root = pose.modelPos[upper], tip = pose.modelPos[end];
    if (!Finite(root) || !Finite(tip) || Length(tip - root) < 1e-4f) return false;
    const Vec3 axis = Normalize(tip - root);
    auto project = [&](Vec3 p) { const Vec3 v = p - root; return v - axis * Dot(v, axis); };
    const Vec3 from = project(pose.modelPos[middle]), to = project(pole);
    if (Length(from) < 0.005f || Length(to) < 0.005f) return false;
    const Vec3 a = Normalize(from), b = Normalize(to);
    const float angle = std::atan2(Dot(axis, Cross(a, b)), std::clamp(Dot(a, b), -1.0f, 1.0f));
    const Quat roll = AxisAngle(axis, angle), endRot = pose.modelRot[end];
    SetModel(sk, pose, upper, root, Normalize(roll * pose.modelRot[upper]));
    sk.Fk(pose);
    // The lower bone inherits the same roll. Restore only the end's orientation.
    SetModel(sk, pose, end, pose.modelPos[end], endRot);
    sk.Fk(pose);
    return true;
}

inline bool MakeChain(const Skeleton& sk, int root, int end, Chain& chain) {
    chain = {};
    if (root < 0 || end < 0 || root >= sk.Count() || end >= sk.Count() || root == end) return false;
    int reverse[16], count = 0;
    for (int b = end; b >= 0 && count < 16; b = sk.BoneAt(b).parent) {
        reverse[count++] = b;
        if (b == root) break;
    }
    if (reverse[count - 1] != root) return false;
    chain.count = count;
    for (int i = 0; i < count; ++i) chain.bones[i] = reverse[count - i - 1];
    for (int i = 0; i + 1 < count; ++i) {
        chain.length[i] = Length(sk.BoneAt(chain.bones[i + 1]).refLocalPos);
        chain.total += chain.length[i];
    }
    return chain.total > 1e-4f;
}

// Chest position divides the spine into two chains. The head remains the final
// target; unreachable/noisy chest targets fall back to the existing head solve.
inline bool ApplyChest(const Skeleton& sk, Pose& pose, const Chain& lower, const Chain& upper,
                       const Goal& chest, const Goal& head) {
    if (!chest.active || !head.active || lower.count < 2 || upper.count < 2 ||
        !Finite(chest.pos) || !Finite(head.pos) || !Finite(chest.rot) || !Finite(head.rot)) return false;
    Pose candidate = pose;
    float chestError = 0, headError = 0;
    SolveLimbFabrik(sk, candidate, lower, chest.pos, chest.rot, 40, 1e-4f, &chestError);
    SolveLimbFabrik(sk, candidate, upper, head.pos, head.rot, 40, 1e-4f, &headError);
    if (!std::isfinite(chestError) || !std::isfinite(headError) || chestError > 0.025f || headError > 0.005f) return false;
    pose = std::move(candidate);
    return true;
}
}
