#pragma once
#include <cmath>
#include <cstdint>

namespace cvr::input {
struct CyberwareChordResult {bool claimed{},fire{};};
class CyberwareHold {
    uint64_t last{},since{};
    bool grip{},blocked{},claimed{},fired{};
public:
    CyberwareChordResult Step(bool l3,float value,bool r3,bool allowed,uint64_t now) {
        grip=std::isfinite(value) && (grip ? value>.4f:value>=.7f);
        const bool interrupted=!last || now<last || now-last>250;
        last=now;
        if(!l3 && !grip) {since=0;blocked=claimed=fired=false;return {};}
        if(!allowed || r3 || interrupted) {since=0;blocked=true;return {claimed,false};}
        if(!l3 || !grip) {since=0;if(claimed)blocked=true;return {claimed,false};}
        if(blocked)return {claimed,false};
        claimed=true;
        if(!since)since=now;
        if(!fired && now-since>=500) {fired=true;return {true,true};}
        return {true,false};
    }
};
// Called with raw XR buttons before deferred L3/D-pad and grip consumers.
bool UpdateCyberwareChord(bool l3,float grip,bool r3,bool inputActive);
// Called by the game's input poll. Delivers a single direct native-action key
// only while this process owns the foreground; never types into another app.
// With the window behind it leaves the activation to the script side instead.
void DispatchCyberwareChord();
// True once for a chord that fired while the game window was not in front (VRCyberwareChordTake).
bool TakeCyberwareChord();
}
