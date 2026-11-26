#include "Anim/BladeContactState.hpp"
#include <algorithm>
#include <cmath>

namespace cvr::blade {
namespace {
constexpr float kEpsilon = 1.0e-6f;

float Dot(const float* left, const float* right) {
    return left[0] * right[0] + left[1] * right[1] + left[2] * right[2];
}

float Length(const float* vector) {
    return std::sqrt(Dot(vector, vector));
}

bool Normalized(const float* source, float* destination) {
    const float length = Length(source);
    if (!std::isfinite(length) || length <= kEpsilon) return false;
    for (int index = 0; index < 3; ++index) destination[index] = source[index] / length;
    return true;
}

float MaterialResistance(Material material) {
    switch (material) {
    case Material::Flesh: return 1.0f;
    case Material::Cyberware: return 2.6f;
    case Material::Metal: return 4.5f;
    case Material::Armor: return 6.0f;
    case Material::Unknown: return 3.0f;
    }
    return 3.0f;
}
}

bool InsideWeaponBox(const float point[3], const float halfExtents[3]) {
    float gapSquared = 0;
    for (int i = 0; i < 3; ++i) {
        if (!std::isfinite(point[i]) || !std::isfinite(halfExtents[i]) || halfExtents[i] < 0) return false;
        const float outside = std::max(0.f, std::abs(point[i]) - halfExtents[i]);
        gapSquared += outside * outside;
    }
    return gapSquared <= .035f * .035f;
}

bool SelectRailAxis(const float shaft[3],const float* cuttingEdge,float axis[3]) {
    if(!Normalized(shaft,axis)) return false;
    if(!cuttingEdge) return true;
    float edge[3];
    if(!Normalized(cuttingEdge,edge)) return false;
    const float axial=Dot(edge,axis);
    if(std::abs(axial)>.12f) return false;
    for(int i=0;i<3;++i) edge[i]-=axis[i]*axial;
    return Normalized(edge,axis);
}

float ShaftPoint(const float hand[3],const float body[3],const float shaft[3],float point[3]) {
    const float delta[]{body[0]-hand[0],body[1]-hand[1],body[2]-hand[2]};
    const float slide=Dot(delta,shaft);
    for(int i=0;i<3;++i) point[i]=hand[i]+slide*shaft[i];
    return slide;
}

bool DampSlide(float handVelocity[3],float bodyVelocity[3],const float axis[3],
               float handMass,float bodyMass,float dampingRate,float step,bool kinematicBody) {
    if(!(handMass>0 && std::isfinite(handMass) && (kinematicBody || (bodyMass>0 && std::isfinite(handMass+bodyMass))) &&
         dampingRate>0 && std::isfinite(dampingRate) && step>0 && step<=.1f)) return false;
    float direction[3];
    if(!Normalized(axis,direction)) return false;
    const float relative=Dot(handVelocity,direction)-Dot(bodyVelocity,direction);
    if(!std::isfinite(relative) || std::abs(relative)<1e-4f) return false;
    const float reduction=relative*(-std::expm1(-std::min(dampingRate,60.f)*step));
    const float handShare=kinematicBody ? 1.f : bodyMass/(handMass+bodyMass);
    for(int i=0;i<3;++i) {
        handVelocity[i]-=direction[i]*reduction*handShare;
        if(!kinematicBody) bodyVelocity[i]+=direction[i]*reduction*(1.f-handShare);
    }
    return true;
}

static bool ProjectEdge(const float shaft[3],const float input[3],float edge[3]) {
    const float axial=Dot(input,shaft);
    for(int i=0;i<3;++i) edge[i]=input[i]-axial*shaft[i];
    return Normalized(edge,edge);
}

bool HasContactEdge(const float a[3],const float b[3],const float edge[3]) {
    float shaft[]{b[0]-a[0],b[1]-a[1],b[2]-a[2]},projected[3];
    return Normalized(shaft,shaft) && ProjectEdge(shaft,edge,projected);
}

ContactMetrics EvaluateContact(const ContactInput& input) {
    ContactMetrics result{};
    float bladeDirection[3]{};
    float surfaceNormal[3]{};
    float edgeDirection[3]{};
    bladeDirection[0] = input.bladeEnd[0] - input.bladeStart[0];
    bladeDirection[1] = input.bladeEnd[1] - input.bladeStart[1];
    bladeDirection[2] = input.bladeEnd[2] - input.bladeStart[2];
    const float reach = Length(bladeDirection);
    if (reach > 1.5f || !Normalized(bladeDirection, bladeDirection)) return result;
    if (!Normalized(input.surfaceNormal, surfaceNormal)) return result;
    if (!ProjectEdge(bladeDirection,input.edgeDirection,edgeDirection)) return result;
    auto project = [&](const float* point, float& distance) {
        float delta[3];
        for (int index = 0; index < 3; ++index) delta[index] = point[index] - input.bladeStart[index];
        distance = Dot(delta, bladeDirection);
        for (int index = 0; index < 3; ++index) delta[index] -= distance * bladeDirection[index];
        return std::isfinite(distance) && Dot(delta, delta) <= 0.000025f;
    };
    float enterAt{}, exitAt{};
    if (!project(input.entry, enterAt) || !project(input.exit, exitAt) || exitAt < enterAt) return result;
    const float depth = std::min(reach, exitAt) - std::max(0.f, enterAt);
    if (!(depth > kEpsilon)) return result;

    float motion[3] = {
        input.motionEnd[0] - input.motionStart[0],
        input.motionEnd[1] - input.motionStart[1],
        input.motionEnd[2] - input.motionStart[2],
    };
    if (!std::isfinite(Dot(motion, motion))) return result;
    const bool moving = Normalized(motion, motion);

    result.penetrationDepth = depth;
    result.centerFraction = (std::max(0.f,enterAt)+depth*.5f)/reach;
    result.incidence = std::clamp(std::fabs(Dot(bladeDirection, surfaceNormal)), 0.0f, 1.0f);
    result.edgeAlignment = moving ? std::clamp(Dot(motion, edgeDirection), 0.0f, 1.0f) : 0.f;
    const float depthFactor = 1.0f + std::clamp(depth / 0.12f, 0.0f, 4.0f);
    const float angleFactor = 0.35f + 0.65f * result.incidence;
    result.resistance = MaterialResistance(input.material) * depthFactor * angleFactor;
    result.valid = std::isfinite(result.resistance);
    return result;
}

ContactMetrics EvaluateCapsule(const ContactInput& input, const float center[3], const float axis[3],
                               float radius, float halfHeight) {
    if (!(radius > 0 && radius <= 1 && halfHeight >= 0 && halfHeight <= 1)) return {};
    float direction[3], origin[3], up[3];
    for (int index = 0; index < 3; ++index) {
        direction[index] = input.bladeEnd[index] - input.bladeStart[index];
        origin[index] = input.bladeStart[index] - center[index];
    }
    if (!Normalized(direction, direction) || !Normalized(axis, up) || !std::isfinite(Dot(origin,origin))) return {};
    const float axialSpeed = Dot(direction, up), axialOrigin = Dot(origin, up);
    float first = INFINITY, last = -INFINITY;
    auto intersect = [&](float shift, int cap) {
        float local[3], slope[3];
        for (int index = 0; index < 3; ++index) {
            local[index] = origin[index] - up[index] * (cap ? shift : axialOrigin);
            slope[index] = direction[index] - (cap ? 0.f : axialSpeed * up[index]);
        }
        const float squared = Dot(slope,slope), linear = Dot(slope,local);
        const float discriminant = linear*linear - squared*(Dot(local,local) - radius*radius);
        if (squared < 1e-10f || !(discriminant >= 0)) return;
        const float root = std::sqrt(discriminant);
        for (float dist : {(-linear-root)/squared, (-linear+root)/squared}) {
            const float along = axialOrigin + dist*axialSpeed;
            if (cap ? cap*(along-shift) < -1e-5f : std::fabs(along) > halfHeight+1e-5f) continue;
            first = std::min(first,dist); last = std::max(last,dist);
        }
    };
    intersect(0,0); intersect(-halfHeight,-1); intersect(halfHeight,1);
    if (!(first < last)) return {};
    ContactInput measured = input;
    const float closest = std::clamp(axialOrigin + first*axialSpeed, -halfHeight, halfHeight);
    for (int index = 0; index < 3; ++index) {
        measured.entry[index] = input.bladeStart[index] + first*direction[index];
        measured.exit[index] = input.bladeStart[index] + last*direction[index];
        measured.surfaceNormal[index] = origin[index] + first*direction[index] - closest*up[index];
    }
    return EvaluateContact(measured);
}

bool CapsuleLookup::Consider(const std::array<float,8>& candidate, bool exactHandle) {
    ambiguous=!exactHandle && (ambiguous || (found && geometry!=candidate));
    geometry=candidate;
    found=true;
    return exactHandle;
}

SweepContact SweepCapsule(const float oldA[3], const float oldB[3], const float a[3], const float b[3],
                          const float center[3], const float up[3], float radius, float halfHeight) {
    float da[3], db[3];
    for (int i=0;i<3;++i) { da[i]=a[i]-oldA[i]; db[i]=b[i]-oldB[i]; }
    const float travel=std::max(Length(da),Length(db));
    if (!(travel<=4.f && radius>0.f && radius<=1.f)) return {};
    for (int i=0;i<3;++i) {
        const float bound=halfHeight+radius;
        if (std::min({oldA[i],oldB[i],a[i],b[i]})>center[i]+bound ||
            std::max({oldA[i],oldB[i],a[i],b[i]})<center[i]-bound) return {};
    }
    const int steps=std::clamp(static_cast<int>(std::ceil(travel/std::max(.005f,radius*.25f))),1,256);
    ContactInput input{};
    for (int step=0;step<=steps;++step) {
        float direction[3];
        const float t=static_cast<float>(step)/steps;
        for (int i=0;i<3;++i) {
            input.bladeStart[i]=oldA[i]+t*da[i]; input.bladeEnd[i]=oldB[i]+t*db[i];
            direction[i]=input.bladeEnd[i]-input.bladeStart[i];
        }
        // Only the finite interval is needed. Supply a non-axial basis for
        // the contact metric's edge projection, including vertical weapons.
        input.edgeDirection[0]=std::abs(direction[0])<std::abs(direction[2]) ? 1.f : 0.f;
        input.edgeDirection[2]=1.f-input.edgeDirection[0];
        const auto hit=EvaluateCapsule(input,center,up,radius,halfHeight);
        if (hit.valid) return {t,hit.centerFraction,hit.penetrationDepth};
    }
    return {};
}

}
