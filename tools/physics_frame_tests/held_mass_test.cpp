#include "Natives/HeldBodyMass.hpp"
#include <array>
#include <cstdlib>
#include <iostream>
#include <limits>
using cvr::held_body_mass::Original;
static void Check(bool ok,const char* why){if(!ok){std::cerr<<why<<'\n';std::exit(1);}}
struct Snapshot {uint32_t proxy,body;float mass;bool on;uint64_t at;};
static float Measure(uint32_t proxy,uint32_t body,float solverMass,const std::array<Snapshot,2>& hands,uint64_t now){
 for(const auto&s:hands)if(Original(proxy,body,s.proxy,s.body,s.mass,s.on,s.at,now,solverMass))break;
 return solverMass;
}
int main(){
 const auto* actor=reinterpret_cast<void*>(uintptr_t(0x10000));
 Check(cvr::held_body_mass::SameActor(actor,68334,18,actor,68334,18),"same native body not shared by both hands");
 Check(!cvr::held_body_mass::SameActor(actor,68335,18,actor,68334,18),"recycled actor pointer inherited old mass ownership");
 Check(!cvr::held_body_mass::SameActor(actor,68334,19,actor,68334,18),"new body index reused old original mass");
 const std::array<Snapshot,2> same{{{68334,18,5,true,1000},{68334,18,5,true,1000}}};
 Check(Measure(68334,18,100,same,1016)==5,"same-body second hand published overridden mass");
 auto firstReleased=same;firstReleased[0].on=false;
 Check(Measure(68334,18,100,firstReleased,1016)==5,"remaining hand lost the original mass after first hand released");
 Check(Measure(68334,7,5,same,1016)==5,"reference body inherited another body's mass");
 // The live reselection was ref7 -> native18. Re-measuring from anchor18
 // after HeldTake must give the same 19*5kg ragdoll, not 18*5+100kg.
 float before=0,after=0;
 for(uint32_t body=0;body<19;++body){before+=5;after+=Measure(68334,body,body==18?100.f:5.f,same,1016);}
 Check(before==95&&after==before,"anchor change inflated whole-corpse weight after native mass override");
 auto different=same;different[0]={68334,7,5,true,1000};
 after=0;for(uint32_t body=0;body<19;++body)after+=Measure(68334,body,body==7||body==18?100.f:5.f,different,1016);
 Check(after==95,"two different held parts inflated ragdoll mass");
 Check(Measure(68335,18,12,same,1016)==12,"foreign proxy matched original held mass");
 Check(Measure(68334,18,100,same,1300)==100,"stale snapshot overrode measured mass");
 Check(Measure(68334,18,100,same,999)==100,"future snapshot accepted");
 auto inactive=same;for(auto&s:inactive)s.on=false;
 Check(Measure(68334,18,7,inactive,1016)==7,"released hold changed measured mass");
 auto invalid=same;for(auto&s:invalid)s.mass=std::numeric_limits<float>::quiet_NaN();
 Check(Measure(68334,18,7,invalid,1016)==7,"non-finite original mass accepted");
 std::cout<<"held mass: resolved anchor, shared/different two-hand actors, override normalization, freshness and identity PASS\n";
}
