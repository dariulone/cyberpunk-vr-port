#include "Hooks/CyberFingerMelee.hpp"
#include <stdexcept>
#include <iostream>
void Check(bool value,const char* why){if(!value)throw std::runtime_error(why);}
int main()try{
    for(bool leftHanded:{false,true}) {
        cvr::cyberfinger::MeleeToggleButton input;
        uint64_t time=1;
        auto step=[&](bool enabled,bool allowed,bool support,bool weapon=false){
            return input.Step(enabled,allowed,leftHanded,leftHanded?weapon:support,leftHanded?support:weapon,time++);};
        const unsigned mask=leftHanded?2:1;
        auto state=step(true,true,true);Check(!state.toggle && state.claimMask==mask,"held Grip on entry toggled or leaked");
        step(true,true,false);state=step(true,true,true);
        Check(state.toggle && state.claimMask==mask,"support Grip failed to toggle");
        Check(!step(true,true,true).toggle,"held Grip repeated toggle");
        state=step(true,false,true);Check(!state.toggle && state.claimMask==mask,"equipment change leaked an owned hold");
        Check(!step(true,true,true).toggle,"returning from a menu repeated held toggle");
        step(true,true,false);Check(step(true,true,true).toggle,"release did not rearm");
        time+=300;Check(!step(true,true,true).toggle,"stale input replayed held Grip");
        step(true,true,false);state=step(true,true,false,true);
        Check(!state.toggle && state.claimMask==0,"weapon-hand Grip toggled melee");
        step(true,false,false);Check(!step(true,false,true).toggle,"ineligible context toggled melee");
        step(true,true,false);step(true,true,true);state=step(false,true,true);
        Check(!state.toggle && state.claimMask==0,"disabled CF profile claimed input");
    }
    std::cout<<"CyberFinger melee input ownership passed\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
