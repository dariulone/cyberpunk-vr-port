#pragma once
#include "Natives/HeldGripPose.hpp"
#include <cstdint>
#include <limits>

namespace cvr::held_grip_contact {
constexpr uint64_t kInvalid = std::numeric_limits<uint64_t>::max();
constexpr uint64_t kMaximumMicrometres = 1000000000000ULL;

struct Fetch {
    const void* scene{};
    uint64_t generation{};
    bool valid{};
};
struct StepGate {
    const void* scene{};
    uint64_t generation{};
    bool prepared{},consumed{true};
    // Run before BeforeSimulate's early returns. Other scenes must not invalidate
    // an outstanding world step, but a failed preparation of this scene must.
    void Begin(const void* incoming,const void* world) {
        if(!incoming || (incoming!=world && incoming!=scene))return;
        scene=incoming;++generation;if(!generation)++generation;
        prepared=false;consumed=false;
    }
    void Prepared(const void* incoming) {
        if(incoming==scene && generation)prepared=true;
    }
    // Consume before AfterFetch's early returns too: a duplicate/failed fetch
    // cannot later relabel the old contact target with a fresh publication time.
    Fetch Take(const void* incoming) {
        if(incoming!=scene || !incoming)return {incoming,0,false};
        const bool valid=prepared&&!consumed;
        consumed=true;
        return {incoming,generation,valid};
    }
};

inline bool WorldPoint(const float* actorPosition, const float* actorRotation, const float* localPoint,
                       const float* sceneOrigin, float* world) {
    float q[4],offset[3];std::copy_n(actorRotation,4,q);
    if(!cvr::held_grip::Normalize(q))return false;
    cvr::held_grip::Rotate(q,localPoint,offset);
    for(int k=0;k<3;++k){world[k]=actorPosition[k]+offset[k]+sceneOrigin[k];if(!std::isfinite(world[k]))return false;}
    return true;
}

// Both points refer to the same completed physics step. The target is retained
// in world space across an origin rebase. No Lua-reference-body point is used.
inline uint64_t ErrorMicrometres(const float* actorPosition,const float* actorRotation,const float* actualLocalGrip,
                                 const float* currentOrigin,const float* targetWorld,bool targetValid) {
    float actual[3];
    if(!targetValid||!WorldPoint(actorPosition,actorRotation,actualLocalGrip,currentOrigin,actual))return kInvalid;
    double squared=0;
    for(int k=0;k<3;++k){const double d=double(actual[k])-targetWorld[k];squared+=d*d;}
    if(!std::isfinite(squared))return kInvalid;
    const double micrometres=std::sqrt(squared)*1000000.0;
    return static_cast<uint64_t>(std::min(micrometres,double(kMaximumMicrometres))+.5);
}
}
