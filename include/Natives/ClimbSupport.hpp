#pragma once
#include "Natives/HandReferenceFrame.hpp"
#include <initializer_list>

namespace cvr::climb_support {
using handspace::Vector;
enum class Feature { None, Top, Edge };
// A shape index is local to one physics-thread query of one static snapshot.
struct Hit { Vector point, normal; int shape; bool close; };
constexpr int kMaxHits = 24;
constexpr float kSeamRadius = .012f; // allow a small collision seam between adjacent pieces
constexpr float kCapRadius = .016f;  // small pad just inside the side the hand touches
inline float Dot(Vector a, Vector b) { return a.x*b.x + a.y*b.y + a.z*b.z; }

// The callback tests a small ball at the intersection of the two measured planes
// against BOTH shapes. Nearby AABBs alone do not establish a common corner.
template<class SharesSeam, class HasCap>
Feature Classify(const Hit* hits, int count, SharesSeam sharesSeam, HasCap hasCap) {
    for (int i = 0; i < count; ++i) {
        if (hits[i].normal.z < .45f) continue;
        for (int j = 0; j < count; ++j)
            if (hits[j].close && hits[i].shape == hits[j].shape) return Feature::Top;
    }
    for (int i = 0; i < count; ++i) for (int j = i + 1; j < count; ++j) {
        const Hit &a = hits[i], &b = hits[j];
        if (!a.close && !b.close) continue;
        const Vector d = b.point - a.point;
        const float c = Dot(a.normal, b.normal);
        if (c >= .77f || LengthSquared(d) > .18f*.18f) continue;
        const float ab = Dot(d, a.normal), ba = -Dot(d, b.normal);
        // Exterior: one point wraps behind the other plane, neither is clearly in front.
        if ((ab >= -.015f && ba >= -.015f) || ab > .012f || ba > .012f) continue;
        if (a.shape == b.shape) return Feature::Edge; // includes opposite sides of a narrow rail
        if (c < -.95f) continue; // facing sheets have no well-defined local seam
        // Project the midpoint onto both planes. Normals are unit MTD directions.
        const float u = -.5f * ab, v = -.5f * ba, inv = 1.f / (1.f - c*c);
        const Vector seam = a.point + d*.5f + a.normal*((u-c*v)*inv) + b.normal*((v-c*u)*inv);
        if (LengthSquared(seam-a.point) <= .12f*.12f && LengthSquared(seam-b.point) <= .12f*.12f &&
            sharesSeam(a.shape, b.shape, seam)) return Feature::Edge;
    }
    // A finger over a rail can curl in the drawn pose while its physical capsule is
    // still on the near face. Look just behind the highest close side contact, on that
    // same shape only. Two pad heights cover a lip up to ~4.5 cm; never scan the scene.
    const Hit* highest = nullptr;
    for (int i = 0; i < count; ++i) {
        const auto& h = hits[i];
        if (h.close && std::fabs(h.normal.z) < .35f && (!highest || h.point.z > highest->point.z)) highest = &h;
    }
    if (highest) {
        const Vector inside = highest->point - highest->normal * .009f;
        for (float rise : {.020f, .045f})
            if (hasCap(highest->shape, inside + Vector{0,0,rise})) return Feature::Top;
    }
    return Feature::None;
}
} // namespace cvr::climb_support
