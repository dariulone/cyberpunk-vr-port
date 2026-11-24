#include "Hooks/CyberFingerMelee.hpp"
#include "Hooks/CyberFingerKatana.hpp"
#include "Hooks/CyberFingerFistFight.hpp"
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
    using namespace cvr::cyberfinger;
    for (bool leftWeapon : {false,true}) {
        FistTriggerChord draw;
        uint64_t t=10;
        auto step=[&](bool weapon,bool other,bool raised=true,bool allowed=true) {
            float l=(leftWeapon?weapon:other)?1.f:0.f, r=(leftWeapon?other:weapon)?1.f:0.f;
            return draw.Step(allowed,raised,false,leftWeapon,l,r,t++);
        };
        step(false,false);
        Check(!step(true,false).draw,"press drew a gun");
        t+=140;Check(!step(true,false).draw && step(false,false).draw,"single Trigger must draw exactly on release");
        Check(!step(false,false).draw,"release repeated weapon draw");
        for (bool otherFirst:{false,true}) for (bool releaseOtherFirst:{false,true}) {
            step(!otherFirst,otherFirst);
            for(int i=0;i<400;++i) step(!otherFirst,otherFirst);
            auto both=step(true,true);
            Check(both.fists && !both.draw,"delayed overlapping Triggers drew a gun or missed fists");
            auto firstUp=step(releaseOtherFirst,!releaseOtherFirst);
            Check(!firstUp.draw && !firstUp.fists && !step(false,false).draw,"a chord release equipped a weapon");
        }
        step(true,false);auto overlap=step(true,true,false);
        Check(!overlap.draw && !overlap.fists && !step(false,false).draw,"lowered hands leaked a draw");
        step(true,false);step(true,false,true,false);
        Check(!step(false,false).draw,"leaving gameplay committed a pending draw");
        KatanaGrip grip;
        grip.Offer(71,0,40);grip.Input(true,false,false,40);
        Check(grip.Input(true,true,true,41).button && grip.Offer(71,0,42),"nearby hilt missed a tap");
        Check(grip.Input(true,true,true,43).button && !grip.Offer(71,0,44),"held Grip acted twice or leaked");
        Check(grip.Input(true,false,true,45).gesture,"release leaked a held finger gesture");
        grip.Input(true,false,false,46);grip.Offer(0,98,46);grip.Pose(true,false,46);
        grip.Input(true,true,false,47);
        Check(grip.Pose(true,false,47)==KatanaGrip::Support::Hold,"lower hilt did not engage");
        grip.Input(true,false,false,48);
        Check(grip.Pose(false,true,48)==KatanaGrip::Support::Hold,"button release dropped support");
        grip.Input(true,true,false,49);
        Check(grip.Pose(false,true,49)==KatanaGrip::Support::Release,"second tap did not release");
        grip.Input(true,false,false,50);grip.Offer(99,0,50);grip.Input(true,true,false,51);
        Check(!grip.Offer(100,0,51),"wheel swap redirected a hilt press");
        grip.Input(true,false,false,1000);
        Check(!grip.Input(true,true,false,1001).button,"expired hilt stole an ordinary Grip press");
    }
    for(int first:{0,1}) {
        FistFightGesture gesture;
        GuardHand hands[2]={{-.2f,-.2f,-.25f,true},{.2f,-.2f,-.25f,true}};
        uint64_t t=1000000;
        auto poll=[&](){t+=60000;return gesture.Step(hands,t,1);};
        Check(!poll(),"raised hands alone equipped fists");
        hands[first].z=-.4f;Check(!poll(),"one punch equipped fists");
        hands[first].z=-.25f;poll();
        hands[first].z=-.4f;Check(!poll(),"two punches with the same hand equipped fists");
        hands[1-first].z=-.4f;Check(poll(),"alternating punches failed to equip fists");
        Check(!poll(),"pair repeated without rearming");
        hands[0].closed=false;poll();hands[0].closed=true;
        hands[0].z=hands[1].z=-.25f;poll();
        hands[0].z=hands[1].z=-.4f;Check(!poll(),"simultaneous hand movement counted as two punches");
    }
    std::cout<<"CyberFinger melee input ownership passed\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
