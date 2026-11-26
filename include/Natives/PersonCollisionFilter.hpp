#pragma once
#include <array>
#include <cstdint>

namespace cvr::physhands {
struct PersonCollisionFilter {
    std::array<uint32_t,4> words{};
    uint8_t hands{};
};
// The game accepts a pair when either group's bits occur in the other's mask.
// NPC capsules use dedicated hand identity bits, separate from hand/weapon
// contact bits, so holding with one hand does not disable the other hand.
constexpr PersonCollisionFilter FilterPersonHands(uint64_t common,uint64_t left,uint64_t right,uint8_t requested){
    requested&=3;
    if(left&&right&&!(left&right)&&!(common&(left|right))){
        const uint64_t mask=(requested&1?left:0)|(requested&2?right:0);
        return {{{0,0,uint32_t(mask),uint32_t(mask>>32)}},requested};
    }
    // Without distinct identities a partial pair cannot be expressed. Mirror
    // the actual conservative physical filter in the query mask as well.
    if(common&&requested==3)return {{{uint32_t(common),uint32_t(common>>32),uint32_t(common),uint32_t(common>>32)}},3};
    return {};
}
}
