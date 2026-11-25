#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>

namespace cvr::cyberfinger {
struct GuardHand {
    float x{}, y{}, z{};
    bool closed{};
    bool Raised() const {
        return y>=-.42f && y<=.20f && z<=.04f && z>=-.95f && std::abs(x)<=.62f;
    }
};

class FistFightGesture {
    struct Stroke { float startZ{}, otherZ{}; uint64_t start{}; bool extended{}; };
    Stroke stroke[2]{};
    uint64_t last{}, origin{}, firstAt{};
    int first=-1;
    bool spent{};
public:
    void Reset() { *this={}; }
    bool Step(const GuardHand (&hands)[2],uint64_t stamp,uint64_t serial) {
        if (origin!=serial || stamp<last || stamp-last>120000) Reset();
        if (!stamp || stamp==last) return false;
        last=stamp; origin=serial;
        for (const auto& h:hands) if (!h.Raised() || !h.closed) { Reset(); return false; }
        if (spent) return false;
        if (first>=0 && stamp-firstAt>1000000) first=-1;
        bool hit[2]{};
        for (int i=0;i<2;++i) {
            auto& s=stroke[i];
            const float z=hands[i].z;
            if (s.extended) {
                if (z>s.startZ-.055f) s={z,hands[1-i].z,stamp,false};
                continue;
            }
            if (!s.start || stamp-s.start>260000 || z>s.startZ) s={z,hands[1-i].z,stamp,false};
            const float travel=s.startZ-z;
            const auto elapsed=stamp-s.start;
            const float otherTravel=s.otherZ-hands[1-i].z;
            if (elapsed>=35000 && travel>=.13f && travel-otherTravel>=.085f && travel>=float(elapsed)*.00000075f) {
                s.extended=true; hit[i]=true;
            }
        }
        if (hit[0]==hit[1]) return false;
        const int side=hit[1]?1:0;
        if (first>=0 && side!=first) { spent=true;return true; }
        first=side; firstAt=stamp;
        stroke[1-side]={hands[1-side].z,hands[side].z,stamp,false};
        return false;
    }
};

// The weapon Trigger is owned from press through release. Any overlap with
// the other Trigger vetoes draw for the entire gesture, in either release order.
class FistTriggerChord {
    bool armed{}, pending{}, chord{}, leftWeapon{};
    uint64_t last{};
public:
    struct Result { bool draw{}, fists{}; };
    Result Step(bool eligible,bool raised,bool punch,bool leftHanded,float& left,float& right,uint64_t now) {
        const bool both=left>=.85f && right>=.85f;
        const bool neutral=left<=.20f && right<=.20f;
        float& weapon=leftHanded?left:right;
        Result result;
        if (!eligible || leftWeapon!=leftHanded || now-last>200) {
            if (pending) chord=true;
            pending=armed=false;
        }
        leftWeapon=leftHanded;last=now;
        if (chord) {
            left=right=0;
            if (neutral) { chord=false;armed=eligible; }
            return result;
        }
        if (!eligible) return result;
        if (punch || (armed && both)) {
            result.fists=punch || raised;
            chord=!neutral;pending=armed=false;
            left=right=0;
            return result;
        }
        if (pending && weapon<=.20f) {
            result.draw=true;pending=false;armed=neutral;
        } else if (armed && weapon>=.85f) {
            pending=true;
        }
        if (pending) weapon=0;
        if (neutral) armed=true;
        return result;
    }
};
}
