#pragma once
#include <cstdio>
#include <cstdlib>
#include "Runtimes/HybridBodyYaw.hpp"
#include "Runtimes/HeadLedBodyTurn.hpp"
#include "Runtimes/LookDownCone.hpp"
#include "Core/LiveControls.hpp"

struct HybridRun {
    TrackedBodyYaw tracker;
    HybridBodyYaw policy;
    cvr::roomscale::BodyYawFollower follower;
    uint64_t seq=0,time=1000000,origin=1;
    float hz,total=0,maxStep=0;
    HybridCommand last{};
    bool trace=false;
    explicit HybridRun(float rate):hz(rate){tracker.Reset(0,false);follower.SetEnabled(true);}
    float Step(const Pose& pose,float body,float head,float down=0,float bend=0,bool hands=true){
        const auto before=follower.Offset();
        auto f=Frame(pose,body,head,++seq,time+=uint64_t(1000000/hz),-down*deg);
        f.origin=origin;f.tracked[0]=hands;
        auto estimate=tracker.Update(f);
        last=policy.Step({head,down,bend*deg,before,10,estimate,true,estimate.valid&&tracker.Ready(),time,origin});
        const float delta=follower.Step(last.targetYaw,0,time,origin);total+=delta;
        if(trace){const auto& t=tracker.trace;
            std::printf("seq %3llu pose %-10s torso %7.1f head %7.1f down %4.1f body %7.1f est %7.1f w %.2f | d %6.1f wM %6.1f hD %6.1f lead %6.1f acc %6.1f ev %.3f res %.3f bits %08x\n",
                (unsigned long long)seq,pose.name,body/deg,head/deg,down,follower.Offset()/deg,estimate.yaw/deg,last.trackedWeight,
                t.delta/deg,t.wristMean/deg,t.headDelta/deg,t.headLead/deg,t.accepted/deg,t.evidence,t.residual,t.bits);}
        maxStep=std::max(maxStep,std::abs(delta)/deg);
        Check(Error(total-follower.Offset(),0)<.002f,"hybrid rotation leaked into view cancellation");
        return delta;
    }
    void Hold(const Pose& pose,float body,float head,float down=0,float bend=0,float seconds=.5f){
        for(int i=0;i<int(hz*seconds);++i)Step(pose,body,head,down,bend);
    }
};
void HybridPolicy(){
    LiveControls defaults{};
    Check(defaults.xrHeadLedBodyRotation==1 && defaults.xrBodyRotationMode==static_cast<int>(RotationMode::HeadLed) &&
          defaults.xrBodyFreeLookDeg==10,"the head-led turn is not the default");
    Check(RotationModeFromFlags(0,0,1)==RotationMode::Hybrid,"hybrid requires legacy master");
    Check(RotationModeFromFlags(1,0,1,1)==RotationMode::HeadLed && RotationModeFromFlags(1,1,0,1)==RotationMode::Tracked,
          "head-led flag priority");
    Check(RotationModeFromFlags(1,1,1)==RotationMode::Tracked,"explicit tracked-only request lost priority");
    Check(RotationModeFromFlags(1,0,0)==RotationMode::HeadCone && RotationModeFromFlags(0,0,0)==RotationMode::Off,"legacy/off modes lost");
    HybridRun r(90);r.Hold(poses[0],0,0);
    for(float head:{-9.f,9.f}){r.Hold(poses[0],0,head*deg);Check(Error(r.follower.Offset(),0)<.01f,"head moved body inside 10 deg cone");}
    r.Hold(poses[0],0,25*deg,0,0,1);Check(Error(r.follower.Offset(),25*deg)<.05f,"upright cone did not follow head");
    // the head turned alone (the torso, the hands, at 0): looking down the body goes back to the hands, easing in with
    // the hands' share (2026-11-07: it used to stay where the head had looked -- the head alone is no torso)
    const float prior=r.follower.Offset();const float first=r.Step(poses[0],0,25*deg,10);
    Check(r.last.tracked && r.last.coneDegrees==0 && std::abs(first)<.2f*std::abs(prior),"look-down handover jumped or retained a cone");
    // (a head within the neck limit of the body: past it the body is pulled along, see HybridNeckLimit)
    r.Hold(poses[0],0,-35*deg,45);Check(Error(r.follower.Offset(),0)<.1f,"hybrid looked down but still followed head yaw");
    const float delta=r.Step(poses[0],0,-35*deg,8);
    Check(!r.last.tracked && std::abs(delta)<1*deg,"standing up jumped at handover");
    r.Hold(poses[0],0,-35*deg,8,0,.1f);Check(r.last.trackedWeight>.5f && r.last.trackedWeight<1,"standing up did not fade the hands out");
    r.Hold(poses[0],0,-35*deg,0,0,1.5f);Check(Error(r.follower.Offset(),-35*deg)<=10.01f,"head cone did not resume");
    Check(r.last.trackedWeight==0 && r.last.coneDegrees==10,"hands' share did not fade out fully");
    Check(r.maxStep<=720/90.f+.001f,"hybrid head handover exceeded its angular-rate limit");
}
void HybridTransitions(){int count=0;float worst=0;
    for(const auto& pose:poses)for(float hz:{30.f,45.f,90.f,144.f})for(float sign:{-1.f,1.f})for(int byBend=0;byBend<2;++byBend){
        // turned 45 degrees, torso and head together (the head alone is no torso: HybridGlanceThenDown)
        const float t0=sign*45*deg;
        HybridRun r(hz);r.Hold(pose,0,0,0,0,.5f);
        for(int i=1;i<=int(hz);++i)r.Step(pose,t0*i/hz,t0*i/hz);
        r.Hold(pose,t0,t0,0,0,1.5f);
        const float entry=r.follower.Offset();
        Check(std::abs(r.Step(pose,t0,t0,byBend?0:30,byBend?15:0))<1*deg,"source switch jumped");
        r.Hold(pose,t0,sign*80*deg,byBend?0:45,byBend?15:0);
        // (within 1 degree: the hands' estimate of the 45 degree turn is trimmed to the body over AgreeTau, not at once)
        Check(Error(r.follower.Offset(),entry)<1.0f,"independent head turn changed hybrid tracked heading");
        for(int i=1;i<=int(hz);++i){float turn=sign*35*deg*i/hz;r.Step(pose,t0+turn,sign*80*deg+turn,byBend?0:45,byBend?15:0);}
        r.Hold(pose,t0+sign*35*deg,sign*115*deg,byBend?0:45,byBend?15:0);
        const float error=Error(r.follower.Offset(),entry+sign*35*deg);worst=std::max(worst,error);
        Check(error<1,"coherent torso turn was lost after hybrid handover");
        Check(std::abs(r.Step(pose,t0+sign*35*deg,sign*115*deg,0,0))<1*deg,"tracked-to-head handover jumped");
        r.Hold(pose,t0+sign*35*deg,sign*115*deg,0,0,1.5f);
        Check(Error(r.follower.Offset(),sign*115*deg)<=10.01f,"head mode did not recover after hybrid");
        Check(r.maxStep<=720/hz+.01f,"hybrid source switch exceeded angular-rate limit");++count;
    }
    std::cout<<count<<" hybrid head/bend/hand-pose transitions; worst tracked turn error "<<worst<<" deg\n";
}
void HybridBoundary(){int changes=0;HybridRun r(90);r.Hold(poses[0],0,0);bool prior=false;
    auto feed=[&](float down,float bend){r.Step(poses[0],0,0,down,bend);if(r.last.tracked!=prior){++changes;prior=r.last.tracked;}};
    for(int i=0;i<1000;++i)feed(9+.8f*std::sin(float(i)),0);
    Check(changes==0,"sub-threshold pitch activated hybrid");feed(10,0);
    for(int i=0;i<1000;++i)feed(9+.8f*std::sin(float(i)),0);
    Check(changes==1 && r.last.tracked,"pitch boundary chattered");feed(8,0);Check(changes==2,"pitch release threshold ignored");
    feed(0,5);for(int i=0;i<1000;++i)feed(0,4+.8f*std::sin(float(i)));
    Check(changes==3 && r.last.tracked,"bend boundary chattered");feed(11,2);Check(changes==3,"clearing one cue cancelled another active cue");
    feed(0,3);Check(changes==4,"bend release threshold ignored");
    std::cout<<"3000 boundary-jitter samples; exactly four expected phase changes\n";
}
void HybridRecovery(){
    HybridRun r(90);r.Hold(poses[0],0,0,0,0,.5f);
    for(int i=1;i<=90;++i)r.Step(poses[0],40*deg*i/90,40*deg*i/90);   // torso and head turned 40 together
    r.Hold(poses[0],40*deg,40*deg,0,0,1);
    r.Hold(poses[0],40*deg,40*deg,35);const float before=r.follower.Offset();
    for(int i=0;i<90;++i)r.Step(poses[0],70*deg,100*deg,35,0,false);
    Check(Error(before,r.follower.Offset())<.001f,"missing controller fell back to head yaw");
    r.Hold(poses[0],70*deg,100*deg,35,0,1);
    Check(Error(before,r.follower.Offset())<.001f,"tracking reacquisition realigned the body absolutely");
    for(int i=1;i<=90;++i)r.Step(poses[0],(70+20*i/90.f)*deg,(100+20*i/90.f)*deg,35);
    r.Hold(poses[0],90*deg,120*deg,35);
    Check(Error(r.follower.Offset(),before+20*deg)<.2f,"relative turn after recovery lost");
    const auto old=r.policy.Step({0,30,0,r.follower.Offset(),10,{},true,false,r.time,r.origin});
    Check(Error(old.targetYaw,r.follower.Offset())<.001f,"duplicate stamp changed heading");
    r.policy.Suspend();r.tracker.Suspend();r.Hold(poses[0],-40*deg,20*deg,35);
    Check(Error(r.follower.Offset(),before+20*deg)<.2f,"suspend/resume lost alignment");
    HybridBodyYaw p;HybridInput in{0,30,0,.8f,10,{.5f,1,true},true,true,1000,1};p.Step(in);
    in.origin=2;in.stampUs=2000;in.estimate={0,1,true};
    Check(Error(p.Step(in).targetYaw,0)<.001f,"origin reset retained the old correction");
    in.headDownDegrees=std::numeric_limits<float>::quiet_NaN();in.bodyOffset=.2f;in.stampUs=3000;
    Check(Error(p.Step(in).targetYaw,.2f)<.001f,"invalid posture did not hold body");
}
// A glance to the side that lowers the head on its way (a look at the hip): the cone drags the body after the head
// before the head is low enough, then the hands take over. The drag must not become the hands' heading.
void HybridLowGlance(){float worst=0;int count=0;
    for(const auto& pose:poses)for(float hz:{45.f,90.f,144.f})for(float sign:{-1.f,1.f}){
        HybridRun r(hz);r.Hold(pose,0,0,0,0,1);
        const int n=int(hz*1.2f);
        for(int i=1;i<=n;++i){const float u=float(i)/n;r.Step(pose,0,sign*45*deg*std::sin(3.14159265f*.5f*std::min(1.f,u*2)),30*(u*u*(3-2*u)));}
        r.Hold(pose,0,sign*45*deg,30,0,1.5f);
        const float error=Error(r.follower.Offset(),0);worst=std::max(worst,error);++count;
        Check(error<2,std::string("a lowering glance left the body turned: ")+pose.name+" "+std::to_string(error));
        Check(r.maxStep<=720/hz+.01f,"low glance exceeded the angular-rate limit");
    }std::cout<<count<<" lowering glances; worst body left off the torso "<<worst<<" deg\n";
}
// Whatever the sources say, the head never stays past the neck limit off the body.
void HybridNeckLimit(){
    HybridRun r(90);r.Hold(poses[0],0,0,30,0,1);
    for(int i=0;i<90;++i)r.Step(poses[0],0,-110*deg*i/89,30,0,false); // hands lost, the head turns far
    r.Hold(poses[0],0,-110*deg,30,0,1);
    Check(Error(r.follower.Offset(),-110*deg)<=65.5f,"head left past the neck limit");
    Check(Error(r.follower.Offset(),-110*deg)>=64.0f,"neck limit pulled the body further than the limit");
    r.Hold(poses[0],0,-110*deg,30,0,1);
    const float held=r.follower.Offset();
    r.Hold(poses[0],0,-110*deg,30,0,1); // hands back (still): realigned to the body as it stands
    Check(Error(r.follower.Offset(),held)<.5f,"hands pulled back against the neck limit");
    std::cout<<"neck limit holds the head within "<<Error(r.follower.Offset(),-110*deg)<<" deg\n";
    // a rubber band: hands still at 0 and tracked, the head out to -110 and back -- the body comes back with it
    HybridRun b(90);b.Hold(poses[0],0,0,30,0,1);
    for(int i=0;i<90;++i)b.Step(poses[0],0,-110*deg*i/89,30);
    b.Hold(poses[0],0,-110*deg,30,0,1);
    Check(Error(b.follower.Offset(),-110*deg)<=65.5f,"head left past the neck limit with the hands still");
    for(int i=0;i<90;++i)b.Step(poses[0],0,-110*deg*(1-i/89.f),30);
    b.Hold(poses[0],0,0,30,0,1.5f);
    Check(Error(b.follower.Offset(),0)<1,"the neck limit's pull stayed after the head came back");
}
// Looking down, turned round fast and back, the controllers turning with the head (the simulator's legacy hands), the
// body must end where the torso is (2026-11-07: the body 40 degrees right of the head). Faster than a person turns
// (0.12 s for 180 degrees; the simulator's mouse) the tracker took the head and wrists turning as one for a tracking
// fault and suspended: the turn was lost and the body stayed at the neck limit, 65 degrees off.
void HybridFastTurnDown(){float worst=0;int count=0;
    for(const auto& pose:poses)for(float seconds:{.12f,.2f,.35f,.6f,1.f})for(float sign:{-1.f,1.f}){
        HybridRun r(90);r.Hold(pose,0,0,45,0,1);
        const int n=int(90*seconds);
        auto ease=[](float u){return u*u*(3-2*u);};
        for(int i=1;i<=n;++i){const float a=sign*180*deg*ease(float(i)/n);r.Step(pose,a,a,45);}
        r.Hold(pose,sign*180*deg,sign*180*deg,45,0,1.5f);
        float error=Error(r.follower.Offset(),sign*180*deg);worst=std::max(worst,error);
        Check(error<2,std::string("fast turn round looking down left the body off: ")+pose.name+" "+std::to_string(error));
        for(int i=1;i<=n;++i){const float a=sign*180*deg*(1-ease(float(i)/n));r.Step(pose,a,a,45);}
        r.Hold(pose,0,0,45,0,1.5f);
        error=Error(r.follower.Offset(),0);worst=std::max(worst,error);++count;
        Check(error<2,std::string("fast turn back looking down left the body off: ")+pose.name+" "+std::to_string(error));
    }
    // flicks: 180 degrees in three samples and back (the simulator's mouse turns the player as one)
    for(const auto& pose:poses)for(float sign:{-1.f,1.f}){
        HybridRun r(90);r.Hold(pose,0,0,45,0,1);
        for(int i=1;i<=3;++i){const float a=sign*60*deg*i;r.Step(pose,a,a,45);}
        r.Hold(pose,sign*180*deg,sign*180*deg,45,0,1.5f);
        float error=Error(r.follower.Offset(),sign*180*deg);worst=std::max(worst,error);
        Check(error<2,std::string("a flick round looking down left the body off: ")+pose.name+" "+std::to_string(error));
        for(int i=1;i<=3;++i){const float a=sign*60*deg*(3-i);r.Step(pose,a,a,45);}
        r.Hold(pose,0,0,45,0,1.5f);
        error=Error(r.follower.Offset(),0);worst=std::max(worst,error);++count;
        Check(error<2,std::string("a flick back looking down left the body off: ")+pose.name+" "+std::to_string(error));
    }
    // one sample turned and the next back is an outlier: the tracker does not carry it, and the neck limit's one step
    // toward the outlying head (720 deg/s for a sample) lets go again
    for(const auto& pose:poses){
        HybridRun r(90);r.Hold(pose,0,0,45,0,1);
        r.Step(pose,pi,pi,45);
        float most=0;
        for(int i=0;i<180;++i){r.Step(pose,0,0,45);most=std::max(most,Error(r.follower.Offset(),0));}
        Check(most<=720/90.f+.01f,std::string("a one-sample outlier turned the body more than one step: ")+pose.name);
        Check(Error(r.follower.Offset(),0)<.2f,std::string("a one-sample outlier left the body turned: ")+pose.name);
    }
    std::cout<<count<<" fast turns and flicks round and back looking down; worst body off the torso "<<worst<<" deg\n";
}
// THE HEAD ALONE IS NO TORSO (2026-11-07).
// Upright, the head turns on its own and holds (the cone has the body follow it), then looks down: the body goes to
// the hands -- the torso never turned. The alignment used to be refitted to the cone's body and the body stayed where
// the head had looked.
void HybridGlanceThenDown(){float worst=0;int count=0;
    for(const auto& pose:poses)for(float hz:{45.f,90.f})for(float sign:{-1.f,1.f})for(float glance:{30.f,50.f,62.f}){
        HybridRun r(hz);r.Hold(pose,0,0,0,0,1);
        const int n=int(hz*.6f);
        for(int i=1;i<=n;++i)r.Step(pose,0,sign*glance*deg*i/n);
        r.Hold(pose,0,sign*glance*deg,0,0,2.0f);
        Check(Error(r.follower.Offset(),sign*glance*deg)<=10.01f,"upright cone did not follow the head alone");
        for(int i=1;i<=n;++i)r.Step(pose,0,sign*glance*deg,35.f*i/n);
        r.Hold(pose,0,sign*glance*deg,35,0,1.5f);
        const float error=Error(r.follower.Offset(),0);worst=std::max(worst,error);++count;
        Check(error<3,std::string("a look round, then down, left the body off the hands: ")+pose.name+" "+std::to_string(error));
        // up again and the same glance back: the hands' heading is still the torso's
        for(int i=1;i<=n;++i)r.Step(pose,0,sign*glance*deg,35.f*(1-float(i)/n));
        for(int i=1;i<=n;++i)r.Step(pose,0,sign*glance*deg*(1-float(i)/n));
        r.Hold(pose,0,0,0,0,1.5f);
        r.Hold(pose,0,0,35,0,1.5f);
        Check(Error(r.follower.Offset(),0)<3,std::string("the hands' heading drifted after a glance: ")+pose.name);
        Check(r.maxStep<=720/hz+.01f,"glance then down exceeded the angular-rate limit");
    }
    std::cout<<count<<" looks round then down; worst body off the hands "<<worst<<" deg\n";
}
// The hands' estimate missed a turn (the torso and head turned, the hands' sample did not): upright, the head held
// farther off the hands than any neck turns takes the body as the torso, and looking down keeps it there.
void HybridMissedTurnHeals(){
    for(float sign:{-1.f,1.f}){
        HybridRun r(90);r.Hold(poses[0],0,0,0,0,1);
        for(int i=1;i<=90;++i)r.Step(poses[0],0,sign*120*deg*i/90);
        r.Hold(poses[0],0,sign*120*deg,0,0,1.5f);
        for(int i=1;i<=30;++i)r.Step(poses[0],0,sign*120*deg,35.f*i/30);
        r.Hold(poses[0],0,sign*120*deg,35,0,1.5f);
        const float error=Error(r.follower.Offset(),sign*120*deg);
        Check(error<=10.5f,"a turn the hands missed was not healed upright: "+std::to_string(error));
        std::cout<<"missed turn of 120 deg: looking down the body stands "<<error<<" deg off the head\n";
    }
}
// A FREEZE IN A TURN: looking down, torso and head turn together, and the game stops for 0.15-0.9 s in the middle of
// it (no samples; the player keeps turning). The body must end at the torso.
void HybridFreezeTurn(){float worst=0;int count=0;
    for(const auto& pose:poses)for(float hz:{45.f,90.f})for(float freeze:{.15f,.4f,.9f})for(float at:{.1f,.5f,.8f})
    for(float sign:{-1.f,1.f}){
        HybridRun r(hz);r.Hold(pose,0,0,40,0,1);
        const float seconds=.8f,turn=sign*110*deg;
        auto ease=[](float u){return u*u*(3-2*u);};
        bool frozen=false;float t=0;
        while(t<seconds){
            const float a=turn*ease(std::min(1.0f,t/seconds));
            if(!frozen && t>=at*seconds){frozen=true;const float gap=freeze;t+=gap;r.time+=uint64_t(gap*1e6f);continue;}
            r.Step(pose,a,a,40);t+=1/hz;
        }
        r.Hold(pose,turn,turn,40,0,1.5f);
        const float error=Error(r.follower.Offset(),turn);worst=std::max(worst,error);++count;
        Check(error<3,std::string("a freeze in a turn looking down left the body off: ")+pose.name+" "+std::to_string(error)+
              " freeze "+std::to_string(freeze)+" at "+std::to_string(at));
    }
    // the head turns alone looking down and the game freezes during it; then the torso turns to where the head looks
    for(const auto& pose:poses)for(float hz:{45.f,90.f})for(float freeze:{.25f,.6f})for(float at:{.5f,.92f})for(float sign:{-1.f,1.f}){
        HybridRun r(hz);r.Hold(pose,0,0,40,0,1);
        const float head=sign*60*deg;
        auto ease=[](float u){return u*u*(3-2*u);};
        const int n=int(hz*.5f);
        for(int i=1;i<=n;++i){
            if(i==int(n*at)){r.time+=uint64_t(freeze*1e6f);}
            r.Step(pose,0,head*ease(float(i)/n),40);
        }
        r.Hold(pose,0,head,40,0,.5f);
        for(int i=1;i<=n;++i)r.Step(pose,head*ease(float(i)/n),head,40);
        r.Hold(pose,head,head,40,0,1.5f);
        const float error=Error(r.follower.Offset(),head);worst=std::max(worst,error);++count;
        Check(error<3,std::string("a freeze in a look round, then the torso after the head, left the body off: ")+pose.name+" "+
              std::to_string(error)+" freeze "+std::to_string(freeze));
    }
    std::cout<<count<<" freezes in turns looking down; worst body off the torso "<<worst<<" deg\n";
}
// A LOOK ROUND LOOKING DOWN, THEN A TURN (simulator, random turns, 2026-11-07): looking down the head turned alone
// 60 degrees and held, came up (the cone took the body after it), went back over the torso, down again, and the torso
// turned 106 degrees fast with the head leading. The hands' estimate took 42 of the 106 and the body stood at the neck
// limit, 65 degrees off a torso the head was right over.
void HybridDownGlanceThenTurn(){float worst=0;int count=0;std::string where;
    auto ease=[](float u){u=std::clamp(u,0.0f,1.0f);return u*u*(3-2*u);};
    for(const auto& pose:poses)for(float hz:{45.f,90.f})for(float sign:{-1.f,1.f})for(float glance:{40.f,60.f})
    for(float goal:{80.f,106.f,140.f})for(float seconds:{.35f,.8f}){
        HybridRun r(hz);r.Hold(pose,0,0,0,0,1);
        auto ramp=[&](float s0,float s1,float t0,float t1,float h0,float h1,float d0,float d1){
            const int n=std::max(1,int(hz*s1));
            for(int i=1;i<=n;++i){const float u=float(i)/n;r.Step(pose,t0+(t1-t0)*ease(u),h0+(h1-h0)*ease(u),d0+(d1-d0)*ease(u));}
            (void)s0;
        };
        const float g=sign*glance*deg;
        ramp(0,.3f,0,0,0,0,0,30);r.Hold(pose,0,0,30,0,.5f);
        ramp(0,.5f,0,0,0,g,30,30);r.Hold(pose,0,g,30,0,1);
        ramp(0,.4f,0,0,g,g,30,0);r.Hold(pose,0,g,0,0,.3f);
        ramp(0,.3f,0,0,g,0,0,0);
        ramp(0,.3f,0,0,0,0,0,32);
        const float turn=-sign*goal*deg;
        const int n=int(hz*seconds);
        for(int i=1;i<=n;++i){const float u=float(i)/n;r.Step(pose,turn*ease((u-.2f)/.8f),turn*ease(u/.8f),32);}
        r.Hold(pose,turn,turn,32,0,1);
        const float error=Error(r.follower.Offset(),turn);++count;
        if(error>worst){worst=error;where=std::string(pose.name)+" hz "+std::to_string(int(hz))+" glance "+std::to_string(int(sign*glance))+
            " turn "+std::to_string(int(-sign*goal))+" in "+std::to_string(seconds)+" s";}
    }
    std::cout<<count<<" looks round looking down, then turns; worst body off the torso "<<worst<<" deg ("<<where<<")\n";
    Check(worst<3,"a look round looking down lost part of the next turn: "+where);
}
// HANDS CHANGED, THEN A TURN (2026-11-07).
// Looking down, the head turns alone and holds; the hands change their hold (lowered, raised to a gun) while it is
// turned; the head comes back over the torso and the torso and head turn together. The body must end at the torso.
void HybridHandsChangeThenTurn(){float worst=0;int count=0;std::string where;
    auto ease=[](float u){u=std::clamp(u,0.0f,1.0f);return u*u*(3-2*u);};
    const int pairs[][2]{{2,0},{0,2},{3,1},{1,4},{4,0},{0,3}};
    for(const auto& pr:pairs)for(float hz:{45.f,90.f})for(float sign:{-1.f,1.f})for(float glance:{35.f,60.f})
    for(float goal:{70.f,106.f,150.f})for(float seconds:{.35f,.9f})for(int upright=0;upright<2;++upright)for(float dir:{-1.f,1.f}){
        const Pose& a=poses[pr[0]];const Pose& b=poses[pr[1]];
        HybridRun r(hz);r.Hold(a,0,0,0,0,1);
        r.trace=std::getenv("HYBRID_TRACE")!=nullptr && count==std::atoi(std::getenv("HYBRID_TRACE"));
        const float down=upright?0.0f:35.0f,g=sign*glance*deg;
        auto ramp=[&](const Pose& pose,float seconds_,float t0,float t1,float h0,float h1,float d0,float d1){
            const int n=std::max(1,int(hz*seconds_));
            for(int i=1;i<=n;++i){const float u=float(i)/n;r.Step(pose,t0+(t1-t0)*ease(u),h0+(h1-h0)*ease(u),d0+(d1-d0)*ease(u));}
        };
        ramp(a,.3f,0,0,0,0,0,35);r.Hold(a,0,0,35,0,.5f);
        ramp(a,.4f,0,0,0,g,35,down);r.Hold(a,0,g,down,0,.6f);
        r.Hold(b,0,g,down,0,.8f);                         // the hands change while the head is turned
        ramp(b,.4f,0,0,g,0,down,35);r.Hold(b,0,0,35,0,.4f);
        // the turn away from where the head looked, or toward it (the head first comes back past where it was when
        // the hands changed: a reference taken then has the head turned)
        const float turn=dir*sign*goal*deg;
        const int n=int(hz*seconds);
        for(int i=1;i<=n;++i){const float u=float(i)/n;r.Step(b,turn*ease((u-.2f)/.8f),turn*ease(u/.8f),35);}
        r.Hold(b,turn,turn,35,0,1);
        const float error=Error(r.follower.Offset(),turn);++count;
        if(error>worst){worst=error;where=std::string(a.name)+" -> "+b.name+" hz "+std::to_string(int(hz))+" glance "+
            std::to_string(int(sign*glance))+(upright?" upright":" down")+" turn "+std::to_string(int(dir*sign*goal))+" in "+
            std::to_string(seconds)+" s";}
    }
    std::cout<<count<<" hand changes with the head turned, then turns; worst body off the torso "<<worst<<" deg ("<<where<<")\n";
    Check(worst<3,"a hand change with the head turned lost part of the next turn: "+where);
}
// THE HEAD-LED TURN, TIGHT (Runtimes/HeadLedBodyTurn.hpp kTightBodyTurnConfig). Upright: the body follows the head
// within 1 degree, never faster than 150 deg/s. Looking down (pitch below -30): the body stays while the head is
// within 70 degrees of where it last re-aimed, then turns at 150 deg/s to where the head was then. Through 0/360.
void HeadLedTurn(){
    using cvr::body::HeadLedBodyTurn;using cvr::body::TurnDeltaAngle;
    for(float hz:{45.f,90.f,144.f})for(float sign:{-1.f,1.f}){
        const float dt=1/hz;
        HeadLedBodyTurn m;m.Seed(10);
        float head=10,most=0,prev=10;
        for(int i=0;i<int(hz*.3f);++i){head+=sign*90*dt/.3f;const float b=m.Step(head,0,dt);most=std::max(most,std::fabs(TurnDeltaAngle(prev,b))/dt);prev=b;}
        for(int i=0;i<int(hz*1.0f);++i){const float b=m.Step(head,0,dt);most=std::max(most,std::fabs(TurnDeltaAngle(prev,b))/dt);prev=b;}
        Check(std::fabs(TurnDeltaAngle(prev,head))<=1.001f,"upright: the body did not end within 1 degree of the head");
        Check(most<=150.01f,"upright: the body turned faster than 150 deg/s: "+std::to_string(most));
        Check(most>=149.0f,"upright: the body did not turn at its 150 deg/s: "+std::to_string(most));
        // looking down, the head turns 60 and holds: the body stays
        const float base=prev;
        for(int i=0;i<int(hz*.5f);++i){head+=sign*60*dt/.5f;prev=m.Step(head,-45,dt);}
        for(int i=0;i<int(hz*1.0f);++i)prev=m.Step(head,-45,dt);
        Check(std::fabs(TurnDeltaAngle(base,prev))<.001f,"looking down: a 60 degree look round turned the body");
        // past 70 the body turns to where the head was when it crossed, at 150 deg/s
        float crossed=0;bool seen=false;
        for(int i=0;i<int(hz*.3f);++i){
            head+=sign*30*dt/.3f;prev=m.Step(head,-45,dt);
            if(!seen && std::fabs(TurnDeltaAngle(base,prev))>1e-4f){seen=true;crossed=head;}
        }
        for(int i=0;i<int(hz*1.5f);++i)prev=m.Step(head,-45,dt);
        Check(seen,"looking down: a 90 degree look round did not turn the body");
        Check(std::fabs(TurnDeltaAngle(prev,crossed))<.01f,"looking down: the body did not stop where the head was when it crossed 70");
        // (the re-aim point is within the upright 1 degree of the body, and the head moves up to 2.3 degrees a frame)
        Check(std::fabs(TurnDeltaAngle(base,crossed))>69 && std::fabs(TurnDeltaAngle(base,crossed))<73.5f,"the crossing was not at 70 degrees: "+std::to_string(TurnDeltaAngle(base,crossed)));
    }
    // through 0/360: 350 -> 10 turns 20 degrees the short way
    HeadLedBodyTurn m;m.Seed(350);float b=350;
    for(int i=0;i<90;++i)b=m.Step(10,0,1/90.f);
    Check(std::fabs(TurnDeltaAngle(b,10))<1.001f,"the turn did not go through 0");
    std::cout<<"head-led turn: upright follow <= 150 deg/s to within 1 deg; looking down holds to 70\n";
    // the port's: a 10 degree upright cone -- an 8 degree look leaves the body, a 30 degree turn brings it under the head
    {
        using cvr::body::kDefaultBodyTurnConfig;
        HeadLedBodyTurn p;p.Seed(0);float body=0;
        for(int i=0;i<90;++i)body=p.Step(8.0f*std::min(1.0f,i/30.0f),0,1/90.f,kDefaultBodyTurnConfig);
        Check(std::fabs(TurnDeltaAngle(0,body))<1e-4f,"port cone: an 8 degree look turned the body");
        for(int i=0;i<90;++i)body=p.Step(30.0f,0,1/90.f,kDefaultBodyTurnConfig);
        Check(std::fabs(TurnDeltaAngle(body,30.0f))<.01f,"port cone: a 30 degree turn did not end under the head");
        // a gradual 30 degree turn (0.67 degree a frame, under the cone every frame) ends under the head too
        HeadLedBodyTurn g;g.Seed(0);float gb=0;
        for(int i=0;i<=45;++i)gb=g.Step(30.0f*i/45.0f,0,1/90.f,kDefaultBodyTurnConfig);
        for(int i=0;i<90;++i)gb=g.Step(30.0f,0,1/90.f,kDefaultBodyTurnConfig);
        Check(std::fabs(TurnDeltaAngle(gb,30.0f))<.01f,"port cone: a gradual 30 degree turn stopped short of the head");
        // and the cone is centred there: an 8 degree look from it leaves the body
        for(int i=0;i<90;++i)gb=g.Step(38.0f,0,1/90.f,kDefaultBodyTurnConfig);
        Check(std::fabs(TurnDeltaAngle(gb,30.0f))<.01f,"port cone: the cone was not centred under the head after the turn");
    }
}
void HybridPosture(){
    cvr::body::BendTracker bend;HybridBodyYaw hybrid;bool activated=false;uint64_t stamp=1000000;
    for(int i=0;i<90;++i){float angle=45*deg*i/89;std::array<float,3> position{0,BendReach*(std::cos(angle)-1),-BendReach*std::sin(angle)};
        const auto measured=bend.Update(position,{0,0,0,1},1);
        const auto cmd=hybrid.Step({0,0,measured.angle,0,10,{0,1,true},true,true,stamp+=11111,1});
        activated|=cmd.tracked;
    }Check(activated,"physical lean with level HMD did not select tracking");
    for(int i=0;i<90;++i){float angle=45*deg*(1-i/89.f);auto measured=bend.Update({0,BendReach*(std::cos(angle)-1),-BendReach*std::sin(angle)},{0,0,0,1},1);
        hybrid.Step({0,0,measured.angle,0,10,{0,1,true},true,true,stamp+=11111,1});}
    Check(!hybrid.Tracked(),"standing up did not restore normal head cone");
    TrackedBodyYaw relative;relative.Reset(50*deg,false);
    for(int i=0;i<90;++i){auto f=Frame(poses[0],-60*deg,-60*deg,i+1,1000000+i*11111);Check(Error(relative.Update(f).yaw,50*deg)<.001f,"relative acquisition snapped toward low hands");}
    Check(relative.Ready(),"relative tracker never warmed");
}
