#pragma once
#include <cmath>
#include <cstdint>

namespace cvr::input {
void PublishScannerVision(int state);
int ScannerVision(); // -1 means no recent game-state sample

class ScannerRequest {
    uint64_t started{};
    bool pending{},accepted{};
public:
    void Update(bool& requested,int vision,uint64_t now) {
        if(!requested){pending=accepted=false;started=0;return;}
        if(!pending){pending=true;started=now;accepted=false;}
        if(vision==1)accepted=true;
        // An input request is not proof that the game opened the scanner.
        // Release rejected/stale requests and respect a game-driven exit.
        if((accepted && vision==0) || (vision!=1 && now>=started && now-started>=750)) {
            requested=false;pending=accepted=false;started=0;
        }
    }
};
// Only a squeeze that STARTS at the ear counts. A hand already squeezing -- a shell or a magazine on
// its way from the belt to a gun held close in -- passes within the zone and must not open the scanner.
//
// ONLY A CLEAN NEW SQUEEZE AT THE HEAD (2026-11-06: "with the grip held, bringing it to the head turns the
// scanner on -- only a clean new grip at the head should"). Three ways a held grip still got through:
//   - one threshold: a grip held a little lightly hovers about 0.7, and every dip and return was a new press,
//     the return happening at the ear. Pressed at kPress, released only under kRelease.
//   - Reset forgot the hand: a grip still held when the gesture came back (the wire let go of it, the lobby
//     closed) read as a press made at that moment. After a Reset, and at start, a grip must open first.
//   - "at the ear" was the zone's exit hysteresis too, a quarter second after the hand had left. The caller
//     passes the hand settled IN the zone now.
class ScannerSqueeze {
    bool held{},carried{},primed{};
public:
    static constexpr float kPress=.70f, kRelease=.35f;
    bool Update(float grip,bool atEar) {
        const bool was=held;
        if(!std::isfinite(grip))grip=0;
        held=held ? grip>=kRelease : grip>=kPress;
        if(!held){carried=false;primed=true;return false;}
        if(!was)carried=!primed || !atEar;   // a press counts only after a release, and only at the ear
        return !carried;
    }
    void Reset(){held=carried=primed=false;}
};
// The runtime reports the controller's grip centre, below/outside the ear,
// rather than the point where the fingers touch the headset.
struct ScannerEarZone {
    float x=-0.18f, y=-0.08f, z=0.08f;
    float radius=0.22f, margin=0.04f;
    bool Contains(float px,float py,float pz,bool armed) const {
        if(!std::isfinite(px) || !std::isfinite(py) || !std::isfinite(pz) || px>-.04f)return false;
        const float r0=radius>.02f && radius<=.60f ? radius : .22f;
        const float extra=margin>=0 && margin<=.30f ? margin : .04f;
        const float r=r0+(armed?extra:0);
        const float dx=px-x,dy=py-y,dz=pz-z;
        return dx*dx+dy*dy+dz*dz<=r*r;
    }
};
}
