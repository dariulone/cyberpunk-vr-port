#pragma once

#include <cstdint>
#include <array>

namespace cvr::blade {

enum class Material : int32_t {
    Flesh = 0,
    Cyberware = 1,
    Metal = 2,
    Armor = 3,
    Unknown = 4,
};

struct ContactInput {
    float entry[3]{};
    float exit[3]{};
    float bladeStart[3]{};
    float bladeEnd[3]{};
    float motionStart[3]{};
    float motionEnd[3]{};
    float surfaceNormal[3]{};
    float edgeDirection[3]{};
    Material material{Material::Unknown};
};

struct ContactMetrics {
    float penetrationDepth{};
    float centerFraction{};
    float incidence{};
    float edgeAlignment{};
    float resistance{};
    bool valid{};
};

ContactMetrics EvaluateContact(const ContactInput& input);
bool InsideWeaponBox(const float point[3], const float halfExtents[3]);
bool SelectRailAxis(const float shaft[3],const float* cuttingEdge,float axis[3]);
float ShaftPoint(const float hand[3],const float body[3],const float shaft[3],float point[3]);
// D6 enum values: Locked = 0, Free = 2. The frame builder maps local X
// onto either the shaft or the authored cutting edge.
template<class Set,class Read>
bool ConfigureRailTranslations(bool hold,Set set,Read read) {
    const int x=hold ? 0 : 2;
    set(0,x);set(1,0);set(2,0);
    return read(0)==x && read(1)==0 && read(2)==0;
}
// Damp relative velocity on D6's free axis at the supplied rate (per second).
// A kinematic target contributes its animation velocity and receives no impulse.
bool DampSlide(float handVelocity[3],float bodyVelocity[3],const float axis[3],
               float handMass,float bodyMass,float dampingRate,float step,bool kinematicBody=false);
ContactMetrics EvaluateCapsule(const ContactInput& input, const float center[3], const float axis[3],
                               float radius, float halfHeight);
struct SweepContact { float time{}, fraction{}, depth{}; };
// Prefer a matching hit-data handle. If the engine cloned it, all named
// candidates must agree on the same local capsule.
struct CapsuleLookup {
    std::array<float,8> geometry{};
    bool found{}, ambiguous{};
    bool Consider(const std::array<float,8>& candidate, bool exactHandle);
};
inline bool UsesShapeResource(bool useResourceData, bool hasLocalShapes) {
    return useResourceData || !hasLocalShapes;
}
SweepContact SweepCapsule(const float oldA[3], const float oldB[3], const float a[3], const float b[3],
                          const float center[3], const float up[3], float radius, float halfHeight);
bool HasContactEdge(const float a[3],const float b[3],const float edge[3]);

// A hit callback identifies a shape by name and user data, without its slot.
// A cloned data handle is usable only when every same-name candidate identifies
// the observed container. An exact data handle takes precedence over clones.
struct CallbackShapeMatch {
    bool observed{}, other{}, exactObserved{}, exactOther{};
    void Consider(bool sameContainer, bool exactData) {
        if (sameContainer) { observed=true;exactObserved|=exactData; }
        else { other=true;exactOther|=exactData; }
    }
    bool Matches() const { return !exactOther && (exactObserved || (observed && !other)); }
};
void RegisterBladeContactStateNative();

}
