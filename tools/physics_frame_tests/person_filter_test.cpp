#include "Natives/PersonCollisionFilter.hpp"
#include <cstdlib>
#include <iostream>
using namespace cvr::physhands;
void Check(bool value,const char* message){if(!value){std::cerr<<message<<'\n';std::exit(1);}}
bool Pair(uint64_t group,uint64_t mask,const PersonCollisionFilter& p){
    const uint64_t pg=p.words[0]|uint64_t(p.words[1])<<32,pm=p.words[2]|uint64_t(p.words[3])<<32;
    return (group&pm)||(pg&mask);
}
int main(){
    constexpr uint64_t common=1ull<<60,left=1ull<<54,right=1ull<<53,contactL=1ull<<58,contactR=1ull<<57;
    for(uint8_t held=0;held<4;++held){
        const uint8_t wanted=uint8_t(3&~held);const auto p=FilterPersonHands(common,left,right,wanted);
        for(int side=0;side<2;++side)for(uint64_t handMask:{0ull,0x5c004ull,contactL|contactR}){
            const bool expected=(wanted>>side)&1,actual=Pair(common|(side?right:left),handMask,p);
            Check(actual==expected,"one hand's grip changed the other hand's physical contacts");
            Check(actual==bool((p.hands>>side)&1),"query eligibility disagrees with the physical pair filter");
        }
        Check(!Pair(0x5c004,0,p),"NPC capsule started colliding with the static world");
        Check(!Pair(contactL,0,p)&&!Pair(contactR,0,p),"hand/weapon contact identities were reused for NPC filtering");
    }
    for(uint8_t wanted=0;wanted<4;++wanted){
        const auto p=FilterPersonHands(common,0,0,wanted);
        for(uint64_t group:{common,common|left,common|right})Check(Pair(group,0,p)==bool(p.hands),"fallback simulation and query disagree");
        Check(p.hands==(wanted==3?3:0),"fallback pretended to isolate a hand without distinct identities");
    }
    const auto oldPhysical=FilterPersonHands(common,0,0,1);
    Check(!Pair(common|left,0,oldPhysical),"old shared filter no longer exposes loss of the nonholding hand");
    Check(!oldPhysical.hands,"disabled collision remained a query obstacle");
    Check(FilterPersonHands(common,left,left,1).hands==0,"aliased identities were accepted");
    Check(FilterPersonHands(0,0,0,3).hands==0,"empty physical filter claimed contacts");
    std::cout<<"NPC contacts: independent hands, physical/query agreement, distinct weapon masks and fallback PASS\n";
}
