#pragma once
#include <cstdint>

namespace cvr::framegen::refresh {
struct Observation {int64_t periodNs{};uint64_t observedMs{};};
inline bool Credible(int64_t ns){return ns>=4000000&&ns<=40000000;}
// Only a positively identified SteamVR session with the package enabled uses this correction.
// Missing/stale readings preserve the original runtime-based behavior. Never
// infer physical refresh by dividing a throttled interval or pinning a minimum.
inline int64_t Select(bool steamvr,int64_t raw,Observation physical,uint64_t nowMs){
    if(steamvr&&Credible(physical.periodNs)&&physical.observedMs&&
        nowMs>=physical.observedMs&&nowMs-physical.observedMs<=6000)return physical.periodNs;
    return raw;
}
// Latch the complete package once at XR initialization. Saved UI edits take effect
// on the next initialization; every timing/queue consumer uses Targeted().
void Start(bool steamvr,bool packageEnabled=true);
void Stop();
void Submitted();
int64_t Resolve(int64_t raw);
Observation Read();
bool Targeted();
}
