#pragma once
#include "Runtimes/CyberFingerMenuPolicy.hpp"

namespace cvr::cyberfinger {
// A physical support-hand Grip press, rearmed only by releasing it in the
// melee context. Keep an owned hold suppressed through equipment/UI changes.
class MeleeToggleButton {
    ButtonEdge edge;
    unsigned claimed{};
    int hand=-1;
    uint64_t last{};
public:
    struct Result { bool toggle{};unsigned claimMask{}; };
    Result Step(bool profile,bool allowed,bool leftHanded,bool left,bool right,uint64_t now) {
        if(!profile){*this={};return {};}
        const bool down[]={left,right};
        for(int i=0;i<2;++i)if(!down[i])claimed&=~(1u<<i);
        const int wanted=leftHanded?1:0;
        if(!allowed || hand!=wanted || (last && (now<last || now-last>250)))edge.Reset();
        hand=wanted;last=now;
        bool toggle=false;
        if(allowed){toggle=edge.Step(down[hand]);if(down[hand])claimed|=1u<<hand;}
        return {toggle,claimed};
    }
};
}
