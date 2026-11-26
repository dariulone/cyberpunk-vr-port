#pragma once
#include <cmath>
#include <cstdint>

namespace cvr::held_body_mass {
inline bool SameActor(const void* a, uint32_t proxyA, uint32_t bodyA,
                      const void* b, uint32_t proxyB, uint32_t bodyB) {
    return a == b && proxyA == proxyB && bodyA == bodyB;
}
// The 100kg/20x solver mass is not inventory/carry weight. A completed hold
// publishes the mass captured before its first override; both hands on one
// actor publish the same shared Held::ownMass.
inline bool Original(uint32_t wantedProxy, uint32_t wantedBody,
                     uint32_t heldProxy, uint32_t heldBody, float originalMass,
                     bool active, uint64_t at, uint64_t now, float& out) {
    if (!active || !at || now < at || now - at >= 300 || wantedProxy != heldProxy || wantedBody != heldBody ||
        !std::isfinite(originalMass) || originalMass <= 0.f) return false;
    out = originalMass;
    return true;
}
}
