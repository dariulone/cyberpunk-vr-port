#pragma once

#include <array>
#include <cstddef>
#include <cstdint>

namespace cvr::scene_velocity {
// 2.5 render-stage descriptor (sub_1410808DC). Keep opaque/padding bytes from
// the engine's entry. The bucket at +20 selects geometry independently of
// the flags at +8 that select the view parameters.
struct Pass {
    uintptr_t callback;
    uint32_t flags;
    uint8_t sortMode;
    uint8_t padding0[3];
    uint32_t stage;
    uint8_t bucket;
    uint8_t padding1[3];
    uint32_t drawArgs[2];
    uint8_t overrideState;
    uint8_t padding2[3];
    int32_t sharedSort;
    uint32_t partition,partitions;
};
static_assert(sizeof(Pass)==48 && offsetof(Pass,stage)==16 && offsetof(Pass,bucket)==20 &&
              offsetof(Pass,overrideState)==32 && offsetof(Pass,sharedSort)==36);

// Late solid/discard nodes each hold one unsplit scene pass. Append the same
// material stage's first-person geometry with the scene's depth and cameras.
inline bool WithObjectVelocity(const Pass& scene,uint32_t solid,uint32_t discard,std::array<Pass,2>& out) {
    if(scene.stage==UINT32_MAX || (scene.stage!=solid && scene.stage!=discard) ||
       scene.flags!=0x11 || scene.bucket!=0 || scene.callback || scene.overrideState ||
       scene.sharedSort!=-1 || scene.partition || scene.partitions ||
       scene.sortMode!=(scene.stage==solid ? 1 : 0))return false;
    out={scene,scene};
    out[1].bucket=2;
    out[1].sortMode=0; // as in the early velbuff draw for this same bucket
    return true;
}
}
