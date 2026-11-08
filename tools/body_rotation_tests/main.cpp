#include "Runtimes/TrackedBodyYaw.hpp"
#include "Runtimes/BodyYawFollower.hpp"
#include <array>
#include <iostream>
#include <random>
#include <stdexcept>
#include <string>
#include <limits>
#include <chrono>
using namespace cvr::body;
constexpr float pi=3.14159265359f,deg=pi/180;
void Check(bool value,const std::string& message){if(!value)throw std::runtime_error(message);}
float Error(float a,float b){return std::abs(std::remainder(a-b,2*pi))/deg;}
XrQuaternionf Yaw(float a){return {0,std::sin(a/2),0,std::cos(a/2)};}
struct Pose {XrVector3f hand[2];const char* name;XrQuaternionf rotation[2]{{0,0,0,1},{0,0,0,1}};};
const std::array<Pose,10> poses{{
    {{{-.24f,-.65f,0},{.24f,-.65f,0}},"rest"},
    {{{-.25f,-.45f,-.2f},{.25f,-.45f,-.2f}},"low_ready"},
    {{{-.10f,-.24f,-.55f},{.06f,-.24f,-.35f}},"rifle"},
    {{{-.025f,-.2f,-.45f},{.025f,-.2f,-.45f}},"two_handed_pistol"},
    {{{-.32f,-.25f,-.42f},{.24f,-.28f,-.4f}},"boxing_guard"},
    {{{.20f,-.35f,-.30f},{-.20f,-.35f,-.3f}},"crossed"},
    {{{-.28f,.2f,-.2f},{.28f,.2f,-.2f}},"hands_high"},
    {{{-.24f,-.65f,0},{.15f,-.20f,-.45f}},"right_pistol",{{-.70710678f,0,0,.70710678f},{0,0,0,1}}},
    {{{-.15f,-.20f,-.45f},{.24f,-.65f,0}},"left_pistol",{{0,0,0,1},{-.70710678f,0,0,.70710678f}}},
    {{{-.24f,-.65f,0},{.25f,.20f,-.2f}},"one_hand_high"},
}};
TrackingFrame Frame(const Pose& pose,float body,float head,uint64_t seq,uint64_t stamp,float pitch=0,float roll=0,float scale=1,float neckBack=.08f,float neckDrop=.1f){
    const XrQuaternionf px{std::sin(pitch/2),0,0,std::cos(pitch/2)},rz{0,0,std::sin(roll/2),std::cos(roll/2)};
    TrackingFrame out;out.head=MultiplyQuat(MultiplyQuat(Yaw(head),px),rz);
    out.valid=out.tracked[0]=out.tracked[1]=true;out.sequence=seq;out.stampUs=stamp;out.origin=1;
    const auto lever=RotateVector(out.head,{0,-neckDrop,neckBack});
    out.headPosition={-lever.x,-lever.y,-lever.z};
    for(int h=0;h<2;++h){auto p=pose.hand[h];p.x*=scale;p.y*=scale;p.z*=scale;p=RotateVector(Yaw(body),p);
        out.hands[h]=RotateVector(ConjugateQuat(out.head),{p.x+lever.x,p.y+lever.y,p.z+lever.z});
        out.rotations[h]=MultiplyQuat(ConjugateQuat(out.head),MultiplyQuat(Yaw(body),pose.rotation[h]));}
    return out;
}
struct Run {
    TrackedBodyYaw tracker;uint64_t seq=0,time=1000000;float hz;YawEstimate last{};
    explicit Run(float rate):hz(rate){}
    YawEstimate Step(const Pose& pose,float body,float head,float pitch=0,float roll=0,float scale=1){
        time+=uint64_t(1000000/hz);auto frame=Frame(pose,body,head,++seq,time,pitch,roll,scale);last=tracker.Update(frame);return last;
    }
    void Warm(const Pose& pose,float scale=1){for(int i=0;i<int(hz*.5f);++i)Step(pose,0,0,0,0,scale);}
};
void Turns(){int count=0;float worst=0;
    for(const auto& pose:poses)for(float hz:{30.f,45.f,72.f,90.f,120.f,144.f})for(float speed:{2.f,10.f,45.f,90.f,180.f,360.f})for(float sign:{-1.f,1.f}){
        Run r(hz);r.Warm(pose);const float duration=std::max(.5f,90.f/speed);float goal=0;
        for(int i=1;i<=int(duration*hz);++i){goal=sign*speed*deg*i/hz;r.Step(pose,goal,goal);}
        for(int i=0;i<int(hz*.25f);++i)r.Step(pose,goal,goal);
        const float error=Error(r.last.yaw,goal);worst=std::max(worst,error);++count;
        Check(error<2.0f,std::string("missed turn ")+pose.name+" hz="+std::to_string(hz)+" speed="+std::to_string(sign*speed)+" error="+std::to_string(error));
    }std::cout<<count<<" turn trajectories; worst settled error "<<worst<<" deg\n";
}
// THE HEAD LEADS, THE TORSO FOLLOWS (2026-11-07). A person turns the head first and the torso after it: the head
// reaches the new heading while the torso is still on its way. The estimator re-anchors after 60 degrees of accepted
// turn, and the rest of the torso's turn -- with the head already still -- was refused as a hand gesture: in game the
// body stopped at 62-65 degrees of a 90-degree turn and stayed there while the head was down. Also the same with the
// head pitched down, and a head lead of 0-40 % of the turn (up to 75 degrees ahead of the torso).
void HeadLedTurns(){int count=0;float worst=0;
    for(const auto& pose:poses)for(float hz:{45.f,90.f,144.f})for(float goal:{40.f,90.f,120.f,180.f})for(float lead:{0.f,.2f,.4f})
    for(float pitch:{0.f,-30.f})for(float sign:{-1.f,1.f}){
        Run r(hz);r.Warm(pose);const float seconds=std::max(.6f,goal/90.f*1.2f);const int n=int(seconds*hz);
        auto ease=[](float u){u=std::clamp(u,0.0f,1.0f);return u*u*(3-2*u);};
        // a neck turns the head at most ~80 degrees off the torso: no lead past that
        float gap=0;for(int i=1;i<=n;++i){const float u=float(i)/n;gap=std::max(gap,goal*(ease(u/(1-lead))-ease((u-lead)/(1-lead))));}
        if(gap>75)continue;
        float body=0,head=0;
        for(int i=1;i<=n;++i){const float u=float(i)/n;
            body=sign*goal*deg*ease((u-lead)/(1-lead));head=sign*goal*deg*ease(u/(1-lead));r.Step(pose,body,head,pitch*deg);}
        for(int i=0;i<int(hz*.5f);++i)r.Step(pose,body,head,pitch*deg);
        float error=Error(r.last.yaw,body);worst=std::max(worst,error);++count;
        Check(error<2.0f,std::string("head-led turn lost ")+pose.name+" hz="+std::to_string(hz)+" goal="+std::to_string(sign*goal)+
              " lead="+std::to_string(lead)+" pitch="+std::to_string(pitch)+" error="+std::to_string(error));
        // held, then back the same way, the head leading again
        for(int i=0;i<int(hz*2);++i)r.Step(pose,body,head,pitch*deg);
        for(int i=1;i<=n;++i){const float u=float(i)/n;
            body=sign*goal*deg*(1-ease((u-lead)/(1-lead)));head=sign*goal*deg*(1-ease(u/(1-lead)));r.Step(pose,body,head,pitch*deg);}
        for(int i=0;i<int(hz*.5f);++i)r.Step(pose,body,head,pitch*deg);
        error=Error(r.last.yaw,0);worst=std::max(worst,error);
        Check(error<2.0f,std::string("head-led return lost ")+pose.name+" hz="+std::to_string(hz)+" goal="+std::to_string(sign*goal)+
              " lead="+std::to_string(lead)+" pitch="+std::to_string(pitch)+" error="+std::to_string(error));
    }std::cout<<count<<" head-led turn trajectories; worst settled error "<<worst<<" deg\n";
}
void Jitter(TrackingFrame& f,std::mt19937& rng,float mm);
// The same out and back with device noise and a 30 % head lead, as the simulator ran it (2026-11-07: the return turn
// started late and then overshot the torso by 13 degrees, and stayed so while the head was down).
void HeadLedNoisy(){int count=0;float worst=0;std::mt19937 rng(110711);
    for(const auto& pose:poses)for(float mm:{.5f,2.f})for(float goal:{60.f,90.f,120.f})for(float pitch:{0.f,-30.f})for(float sign:{-1.f,1.f}){
        const float hz=90,lead=.3f;
        Run r(hz);for(int i=0;i<int(hz*.5f);++i){auto f=Frame(pose,0,0,++r.seq,r.time+=uint64_t(1000000/hz),pitch*deg);Jitter(f,rng,mm);r.last=r.tracker.Update(f);}
        const float seconds=std::max(.6f,goal/90.f*1.2f);const int n=int(seconds*hz);
        auto ease=[](float u){u=std::clamp(u,0.0f,1.0f);return u*u*(3-2*u);};
        float body=0,head=0;
        auto step=[&](){auto f=Frame(pose,body,head,++r.seq,r.time+=uint64_t(1000000/hz),pitch*deg);Jitter(f,rng,mm);r.last=r.tracker.Update(f);};
        for(int i=1;i<=n;++i){const float u=float(i)/n;body=sign*goal*deg*ease((u-lead)/(1-lead));head=sign*goal*deg*ease(u/(1-lead));step();}
        for(int i=0;i<int(hz*2);++i)step();
        float error=Error(r.last.yaw,body);worst=std::max(worst,error);
        Check(error<3.0f,std::string("noisy head-led turn lost ")+pose.name+" mm="+std::to_string(mm)+" goal="+std::to_string(sign*goal)+
              " pitch="+std::to_string(pitch)+" error="+std::to_string(error));
        float peak=0;
        for(int i=1;i<=n;++i){const float u=float(i)/n;body=sign*goal*deg*(1-ease((u-lead)/(1-lead)));head=sign*goal*deg*(1-ease(u/(1-lead)));step();
            peak=std::max(peak,Error(r.last.yaw,body));}
        for(int i=0;i<int(hz*.5f);++i)step();
        error=Error(r.last.yaw,0);worst=std::max(worst,error);++count;
        Check(error<3.0f,std::string("noisy head-led return lost ")+pose.name+" mm="+std::to_string(mm)+" goal="+std::to_string(sign*goal)+
              " pitch="+std::to_string(pitch)+" error="+std::to_string(error)+" peak="+std::to_string(peak));
    }std::cout<<count<<" noisy head-led out-and-back turns; worst settled error "<<worst<<" deg\n";
}
void HeadOnly(){float worst=0;int count=0;
    for(const auto& pose:poses)for(float hz:{30.f,60.f,90.f,144.f})for(float pitch:{-89.f,-60.f,0.f,60.f,89.f}){
        Run r(hz);r.Warm(pose);
        for(int i=1;i<int(hz*6);++i){const float t=i/hz,head=100*deg*std::sin(t*1.5f);auto out=r.Step(pose,0,head,pitch*deg*std::sin(t),35*deg*std::sin(t*.7f));worst=std::max(worst,Error(out.yaw,0));}
        ++count;Check(Error(r.last.yaw,0)<1.0f,std::string("head-only rotation moved torso: ")+pose.name);
    }Check(worst<1.0f,"head/pitch/roll produced a false turn");std::cout<<count<<" head-only trajectories; peak false yaw "<<worst<<" deg\n";
}
void Gestures(){float worst=0;int count=0;
    for(float hz:{30.f,45.f,90.f,144.f})for(int kind=0;kind<10;++kind){Run r(hz);r.Warm(poses[0]);
        for(int i=1;i<int(hz*20);++i){float t=i/hz,w=.5f-.5f*std::cos(t*2);Pose p=poses[0];
            if(kind==0)p.hand[1].z-=.55f*w;
            if(kind==1){p.hand[1].x-=.6f*w;p.hand[1].y+=.4f*w;}
            if(kind==2){p.hand[0].y+=.6f*w;p.hand[1].z-=.3f*w;}
            if(kind==3){p.hand[0].z-=.2f*std::sin(t*4);p.hand[1].z+=.2f*std::sin(t*4);}
            if(kind==4){p.hand[0].y+=.8f*w;p.hand[1].y+=.8f*w;}
            if(kind==5){p.hand[0].x+=.42f*w;p.hand[1].x-=.42f*w;}
            if(kind==6){p.hand[0].z-=.45f*w;p.hand[1].z-=.45f*w;}
            if(kind==7){p.hand[0].x-=.35f*w;p.hand[1].x+=.35f*w;}
            if(kind==8){p.hand[0].z-=.2f*std::sin(t*4);p.hand[1].z+=.2f*std::sin(t*4);}
            if(kind==9){p.hand[1].x-=.5f*w;p.hand[1].y+=.4f*w;p.rotation[1]=Yaw(40*deg*w);}
            auto out=r.Step(p,0,(kind==1 || kind==9)?40*deg*w:(kind==8?60*deg*std::sin(t*.6f):0));worst=std::max(worst,Error(out.yaw,0));
        }++count;Check(Error(r.last.yaw,0)<2,"gesture drift kind="+std::to_string(kind)+" error="+std::to_string(Error(r.last.yaw,0)));
    }Check(worst<3,"gesture falsely rotated body peak="+std::to_string(worst));std::cout<<count<<" repeated gesture traces; peak false yaw "<<worst<<" deg\n";
}
void Tracking(){Run r(90);r.Warm(poses[0]);for(int i=0;i<90;++i)r.Step(poses[0],i*deg,i*deg);
    auto f=Frame(poses[0],90*deg,90*deg,++r.seq,r.time+=11111);const auto before=r.last.yaw;
    f.tracked[0]=false;Check(!r.tracker.Update(f).valid,"untracked controller accepted");
    for(int i=0;i<90;++i){f.stampUs+=11111;++f.sequence;auto out=r.tracker.Update(f);Check(Error(out.yaw,before)<.001f,"tracking loss changed yaw");}
    f.tracked[0]=true;auto recovered=r.tracker.Update(f);Check(Error(recovered.yaw,before)<.001f,"reacquisition snapped yaw");
    f.origin=2;f.head=Yaw(0);f=Frame(poses[0],0,0,++f.sequence,f.stampUs+=11111);f.origin=2;
    Check(Error(r.tracker.Update(f).yaw,0)<.001f,"recenter carried the old body offset");
    f.head.x=std::numeric_limits<float>::quiet_NaN();Check(!r.tracker.Update(f).valid,"NaN head accepted");
    f.head=Yaw(0);f.hands[0].x=INFINITY;Check(!r.tracker.Update(f).valid,"infinite controller accepted");
}
void Cadence(){for(float hz:{20.f,30.f,45.f,72.f,90.f,120.f,144.f}){Run r(hz);r.Warm(poses[2]);
    for(int i=0;i<int(hz*4);++i){auto f=Frame(poses[2],i/hz*deg*90,i/hz*deg*90,++r.seq,r.time+=uint64_t(1000000/hz));auto out=r.tracker.Update(f);
        for(int j=0;j<4;++j)Check(Error(r.tracker.Update(f).yaw,out.yaw)<.0001f,"duplicate frame advanced estimator");}
    } }
void Noise(){std::mt19937 rng(7142);std::normal_distribution<float> noise(0,.002f);float worst=0;
    for(const auto& pose:poses)for(float hz:{30.f,45.f,90.f,144.f}){Run r(hz);r.Warm(pose);
        for(int i=0;i<int(hz*60);++i){auto f=Frame(pose,0,0,++r.seq,r.time+=uint64_t(1000000/hz));for(auto& p:f.hands){p.x+=noise(rng);p.y+=noise(rng);p.z+=noise(rng);}auto out=r.tracker.Update(f);worst=std::max(worst,Error(out.yaw,0));}
    }Check(worst<2,"idle jitter drift="+std::to_string(worst));std::cout<<poses.size()*4<<" minutes simulated idle; peak false yaw "<<worst<<" deg\n";
}
void Jitter(TrackingFrame& f,std::mt19937& rng,float mm){
    std::normal_distribution<float> p(0,mm*.001f),a(0,.15f*deg);
    const auto oldHead=f.head;
    const float pitch=a(rng);
    f.head=MultiplyQuat(MultiplyQuat(Yaw(a(rng)),XrQuaternionf{std::sin(pitch/2),0,0,std::cos(pitch/2)}),oldHead);
    const XrVector3f headNoise{p(rng),p(rng),p(rng)};
    f.headPosition.x+=headNoise.x;f.headPosition.y+=headNoise.y;f.headPosition.z+=headNoise.z;
    for(int h=0;h<2;++h){auto world=RotateVector(oldHead,f.hands[h]);world.x+=p(rng)-headNoise.x;world.y+=p(rng)-headNoise.y;world.z+=p(rng)-headNoise.z;
        const auto worldQ=MultiplyQuat(Yaw(a(rng)),MultiplyQuat(oldHead,f.rotations[h]));
        f.hands[h]=RotateVector(ConjugateQuat(f.head),world);f.rotations[h]=MultiplyQuat(ConjugateQuat(f.head),worldQ);}
}
void NoisyTurns(){float worst=0;int count=0;std::mt19937 rng(8641);
    for(const auto& pose:poses)for(float hz:{30.f,45.f,90.f,144.f})for(float speed:{-180.f,-45.f,-2.f,2.f,45.f,180.f})for(float mm:{1.f,3.f}){
        Run r(hz);r.Warm(pose);float goal=0;
        for(int i=0;i<int(hz*5);++i){goal=i/hz*speed*deg;auto f=Frame(pose,goal,goal,++r.seq,r.time+=uint64_t(1000000/hz));Jitter(f,rng,mm);r.last=r.tracker.Update(f);}
        for(int i=0;i<int(hz*.5f);++i){auto f=Frame(pose,goal,goal,++r.seq,r.time+=uint64_t(1000000/hz));Jitter(f,rng,mm);r.last=r.tracker.Update(f);}
        const float error=Error(r.last.yaw,goal);worst=std::max(worst,error);++count;
        Check(error<3,std::string("noisy turn lost: ")+pose.name+" hz="+std::to_string(hz)+" speed="+std::to_string(speed)+" error="+std::to_string(error));
    }std::cout<<count<<" independently noisy device trajectories; worst error "<<worst<<" deg\n";
}
void MixedMotion(){float worst=0,peak=0;
    for(float hz:{30.f,45.f,90.f,144.f})for(float speed:{-90.f,-15.f,15.f,90.f})for(int kind=0;kind<5;++kind){Run r(hz);r.Warm(poses[0]);float goal=0;
        for(int i=0;i<int(hz*8);++i){const float t=i/hz;goal=speed*deg*t;Pose p=poses[0];
            if(kind==0 || kind==1){p.hand[0].z-=.2f*std::sin(t*4);p.hand[1].z+=.2f*std::sin(t*4);}
            if(kind==2){p.hand[1].y+=.7f*(.5f-.5f*std::cos(t*3));p.hand[1].z-=.2f;}
            const float head=kind==3?0:goal;
            const auto out=r.Step(p,goal,head,kind==4?85*deg:0);peak=std::max(peak,Error(out.yaw,goal));
        }
        // Keep the final actual pose while settling, including a counterturned head.
        for(int i=0;i<int(hz*.5f);++i)r.Step(poses[0],goal,kind==3?0:goal,kind==4?85*deg:0);
        const float error=Error(r.last.yaw,goal);worst=std::max(worst,error);
        Check(error<3,"mixed body motion missed kind="+std::to_string(kind)+" hz="+std::to_string(hz)+" speed="+std::to_string(speed)+" error="+std::to_string(error));
    }std::cout<<"80 mixed gesture/walking/counterlook/pitch trajectories; worst error "<<worst<<" deg; moving peak "<<peak<<" deg\n";
}
void Lifecycle(){Run r(90);r.Warm(poses[0]);auto f=Frame(poses[0],20*deg,20*deg,++r.seq,r.time+=11111);r.tracker.Update(f);
    const auto old=f;f.sequence+=5;f.stampUs+=55555;const auto latest=r.tracker.Update(f);
    auto stale=old;stale.stampUs=f.stampUs+1;stale.head=Yaw(-170*deg);
    Check(Error(r.tracker.Update(stale).yaw,latest.yaw)<.001f,"old sequence accepted with a new timestamp");
    r.tracker.Reset(.3f);f=Frame(poses[2],0,0,1,1000000);Check(Error(r.tracker.Update(f).yaw,.3f)<.001f,"mode switch discarded existing heading");
    for(int i=0;i<8;++i){f.stampUs+=11111;++f.sequence;f.head.x=-f.head.x;f.head.y=-f.head.y;f.head.z=-f.head.z;f.head.w=-f.head.w;
        for(auto& q:f.rotations){q.x=-q.x;q.y=-q.y;q.z=-q.z;q.w=-q.w;}Check(std::isfinite(r.tracker.Update(f).yaw),"quaternion sign flip invalidated tracking");}
    cvr::roomscale::BodyYawFollower follow;follow.SetEnabled(true);follow.Step(0,0,1000000,1);float total=0;
    for(int i=1;i<300;++i){const float target=.6f*std::sin(float(i)/100);const float step=follow.Step(target,0,1000000+i*11111,1);total+=step;
        Check(Error(follow.Offset(),target)<.001f,"cone-free follower held estimated body yaw");
        // Engine heading gains the step, camera subtracts precisely that offset.
        Check(Error(total-follow.Offset(),0)<.001f,"body turn leaked into view heading");}
}
void NoisyIdle(){float worst=0;int count=0;
    for(int seed=0;seed<8;++seed)for(const auto& pose:poses)for(float hz:{30.f,90.f,144.f}){
        Run r(hz);std::mt19937 rng(5301+seed);float localPeak=0;
        for(int i=0;i<int(hz*30);++i){const float head=60*deg*std::sin(i/hz*.4f);auto f=Frame(pose,0,head,++r.seq,r.time+=uint64_t(1000000/hz));Jitter(f,rng,3);r.last=r.tracker.Update(f);localPeak=std::max(localPeak,Error(r.last.yaw,0));}
        worst=std::max(worst,localPeak);++count;
        Check(localPeak<2,std::string("noisy independent head glance drift: ")+pose.name+" seed="+std::to_string(seed)+" peak="+std::to_string(localPeak));
    }std::cout<<count<<" 30-second noisy head-glance traces; peak false yaw "<<worst<<" deg\n";
}
void WristOnly(){float worst=0;
    for(const auto& pose:poses)for(int handMode=0;handMode<3;++handMode){Run r(90);r.Warm(pose);
        for(int i=0;i<1800;++i){const float yaw=75*deg*std::sin(i/90.f*2);auto f=Frame(pose,0,yaw,++r.seq,r.time+=11111);
            for(int h=0;h<2;++h)if(handMode==2 || h==handMode)f.rotations[h]=MultiplyQuat(ConjugateQuat(f.head),Yaw(yaw));
            const auto out=r.tracker.Update(f);worst=std::max(worst,Error(out.yaw,0));}
    }Check(worst<1,"head + wrist gesture with stationary hands rotated body");std::cout<<poses.size()*3<<" wrist-only traces; peak false yaw "<<worst<<" deg\n";
}
void NoisyGestures(){float worst=0;int count=0,failed=0;std::mt19937 rng(92413);
    for(const auto& pose:poses)for(float hz:{45.f,90.f,144.f})for(float mm:{1.f,3.f})for(int kind=0;kind<3;++kind){
        Run r(hz);r.Warm(pose);float peak=0;
        for(int i=0;i<int(hz*12);++i){const float t=i/hz,w=std::sin(t*1.5f);Pose p=pose;
            if(kind==0){p.hand[1].x-=.25f*w;p.hand[1].y+=.2f*w;p.rotation[1]=MultiplyQuat(Yaw(40*deg*w),p.rotation[1]);}
            if(kind==1)for(auto& q:p.rotation)q=MultiplyQuat(Yaw(55*deg*w),q);
            if(kind==2){p.hand[0].z-=.15f*std::sin(t*4);p.hand[1].z+=.15f*std::sin(t*4);}
            auto f=Frame(p,0,70*deg*w,++r.seq,r.time+=uint64_t(1000000/hz));Jitter(f,rng,mm);
            r.last=r.tracker.Update(f);peak=std::max(peak,Error(r.last.yaw,0));
        }
        worst=std::max(worst,peak);++count;
        if(peak>=3){++failed;std::cout<<"false turn "<<pose.name<<" kind="<<kind<<" hz="<<hz<<" mm="<<mm<<" peak="<<peak<<" deg\n";}
    }
    std::cout<<count<<" combined noise/head/gesture traces; peak false yaw "<<worst<<" deg; failed "<<failed<<'\n';
    Check(failed==0,"combined noisy gesture produced a false torso turn");
}
void ReturnTurns(){float worst=0;int count=0,failed=0;std::mt19937 rng(140224);
    for(const auto& pose:poses)for(float hz:{30.f,45.f,90.f,144.f})for(float speed:{5.f,90.f,360.f})for(float sign:{-1.f,1.f})for(float mm:{1.f,3.f}){
        Run r(hz);r.Warm(pose);const float goal=sign*(speed==5?10:speed==90?60:90)*deg;const int steps=std::max(2,int(std::abs(goal)/(speed*deg)*hz));
        for(int side=0;side<2;++side){
            for(int i=1;i<=steps;++i){const float yaw=goal*(side?1-float(i)/steps:float(i)/steps);auto f=Frame(pose,yaw,yaw,++r.seq,r.time+=uint64_t(1000000/hz));Jitter(f,rng,mm);r.last=r.tracker.Update(f);}
            for(int i=0;i<int(hz*.75f);++i)r.Step(pose,side?0:goal,side?0:goal);
        }
        float error=Error(r.last.yaw,0);worst=std::max(worst,error);++count;
        if(error>=1){++failed;if(failed<=12)std::cout<<"return lost "<<pose.name<<" hz="<<hz<<" speed="<<sign*speed<<" mm="<<mm<<" error="<<error<<" deg\n";}
    }
    std::cout<<count<<" noisy out-and-back turns; worst final error "<<worst<<" deg; failed "<<failed<<'\n';
    Check(failed==0,"return to the reference pose left a body offset");
}
void Startup(){for(float head:{-120.f,-95.f,-90.f,0.f,90.f,95.f,120.f})for(int cross=0;cross<2;++cross){Run r(90);auto pose=poses[0];if(cross)std::swap(pose.hand[0],pose.hand[1]);
        for(int i=0;i<180;++i)r.Step(pose,0,head*deg);
        Check(Error(r.last.yaw,0)<.01f,"startup head/crossed hands flipped torso");}
    Run r(90);r.Warm(poses[0]);auto f=Frame(poses[0],0,0,1000,r.time+11111);f.origin=2;r.tracker.Update(f);
    f.origin=1;f.sequence=1001;f.stampUs+=11111;f.head=Yaw(1);Check(!r.tracker.Update(f).valid,"retired origin reactivated");
}
void Freshness(){auto f=Frame(poses[0],0,0,7,1000000);
    Check(TrackingFrameFresh(f,1,1100000),"recent coherent frame rejected");
    Check(!TrackingFrameFresh(f,2,1100000),"retired tracking space accepted");
    Check(!TrackingFrameFresh(f,1,999999),"future frame accepted");
    Check(!TrackingFrameFresh(f,1,1150001),"stale frame accepted");
    f.valid=false;Check(!TrackingFrameFresh(f,1,1100000),"invalid publication accepted");
    for(float pole:{-90.f,90.f}){Run r(90);r.Warm(poses[0]);
        for(int i=0;i<180;++i)r.Step(poses[0],i*deg,i*deg,pole*deg);
        for(int i=0;i<45;++i)r.Step(poses[0],179*deg,179*deg,pole*deg);
        Check(Error(r.last.yaw,179*deg)<3,"pitch pole blocked coherent torso rotation");}
}
void Baselines(){const auto& rifle=poses[2];
    const float lineYaw=std::atan2(-(rifle.hand[1].z-rifle.hand[0].z),rifle.hand[1].x-rifle.hand[0].x);
    Check(Error(lineYaw,0)>40,"hand-line baseline no longer exposes rifle pose bias");
    Run r(90);r.Warm(rifle);for(int i=0;i<90;++i)r.Step(rifle,0,90*deg);
    Check(Error(r.last.yaw,0)<1,"estimator repeated the head-only baseline error");
    std::cout<<"reference head-only error 90 deg; reference rifle hand-line bias "<<Error(lineYaw,0)<<" deg; estimator "<<Error(r.last.yaw,0)<<" deg\n";
}
void NeckLever(){float worst=0;int count=0;
    for(const auto& pose:poses)for(float hz:{30.f,90.f,144.f})for(float back:{.03f,.05f,.08f,.14f,.18f})for(float drop:{.05f,.1f,.16f}){Run r(hz);
        for(int i=0;i<int(hz*.5f);++i)r.tracker.Update(Frame(pose,0,0,++r.seq,r.time+=uint64_t(1000000/hz),0,0,1,back,drop));
        for(int i=0;i<int(hz*8);++i){const float t=i/hz;const auto out=r.tracker.Update(Frame(pose,0,95*deg*std::sin(t*1.3f),++r.seq,r.time+=uint64_t(1000000/hz),65*deg*std::sin(t*1.7f),30*deg*std::sin(t*.7f),1,back,drop));
            worst=std::max(worst,Error(out.yaw,0));Check(Error(out.yaw,0)<.05f,std::string("neck lever mismatch falsely turned body: ")+pose.name+" peak="+std::to_string(Error(out.yaw,0)));}
        ++count;
    }std::cout<<count<<" neck/eye geometry trajectories; peak false yaw "<<worst<<" deg\n";
}
void SmallTurns(){for(const auto& pose:poses)for(float angle:{-.05f,-.2f,-1.f,.05f,.2f,1.f}){Run r(90);r.Warm(pose);
        for(int i=0;i<90;++i)r.Step(pose,angle*deg,angle*deg);
        Check(Error(r.last.yaw,angle*deg)<.02f,"small physical turn hidden behind an angular cone");}
    std::cout<<poses.size()*6<<" small turns down to0.05 deg without a free-look cone\n";
}
void Discontinuities(){Run r(90);r.Warm(poses[0]);
    auto f=Frame(poses[0],pi,pi,++r.seq,r.time+=11111);const auto invalid=r.tracker.Update(f);
    // (a whole-player turn is held for a sample and carried only when the next confirms it: Update)
    Check(Error(invalid.yaw,0)<.001f && invalid.confidence==0,"one-frame whole-tracking yaw jump was injected");
    for(int i=0;i<90;++i)r.Step(poses[0],0,0);
    Check(Error(r.last.yaw,0)<.01f,"outlier left a permanent offset");
    r.tracker.Reset();uint64_t stamp=1000000;uint32_t seq=0xffffff00;
    for(int i=0;i<140;++i){seq+=2;f=Frame(poses[0],0,0,seq,stamp+=11111);r.tracker.Update(f);}
    for(int i=0;i<90;++i){seq+=2;f=Frame(poses[0],float(i)*deg,float(i)*deg,seq,stamp+=11111);r.last=r.tracker.Update(f);}
    for(int i=0;i<45;++i){seq+=2;f=Frame(poses[0],89*deg,89*deg,seq,stamp+=11111);r.last=r.tracker.Update(f);}
    Check(Error(r.last.yaw,89*deg)<2,"native sequence wrap froze body rotation");
}
void Benchmark(){std::array<TrackingFrame,720> data;
    for(size_t i=0;i<data.size();++i)data[i]=Frame(poses[2],float(i)*deg*.5f,float(i)*deg*.5f,i+1,1000000+i*11111);
    constexpr int iterations=500000;TrackedBodyYaw tracker;float checksum=0;
    const auto start=std::chrono::steady_clock::now();
    for(int i=0;i<iterations;++i){auto f=data[static_cast<size_t>(i)%data.size()];f.sequence=i+1;f.stampUs=1000000+uint64_t(i)*11111;checksum+=tracker.Update(f).yaw;}
    const double ns=std::chrono::duration<double,std::nano>(std::chrono::steady_clock::now()-start).count()/iterations;
    std::cout<<"estimator mean "<<ns<<" ns/update; iterations "<<iterations<<"; checksum "<<checksum<<'\n';
}
void Invariants(){for(const auto& pose:poses)for(float scale:{.7f,1.f,1.3f}){Run r(90);r.Warm(pose,scale);
        for(int i=0;i<900;++i){auto out=r.Step(pose,i*deg,i*deg,0,0,scale);Check(std::isfinite(out.yaw),"nonfinite yaw during multiple revolutions");}
        for(int i=0;i<45;++i)r.Step(pose,899*deg,899*deg,0,0,scale);
        Check(Error(r.last.yaw,899*deg)<3,std::string("multiple turn/size drift: ")+pose.name);}
}
#include "hybrid_cases.hpp"
#include "dynamics_cases.hpp"
int main(int argc,char** argv)try{Check(argc==2,"case required");std::string name=argv[1];
    if(name=="prediction_noise_metrics"){PredictionNoiseMetrics();return 0;}
    if(name=="prediction_held_samples"){PredictionHeldSamples();return 0;}
    if(name=="dynamics_metrics"){DynamicsMetrics();return 0;}
    if(name=="prediction_steady"){PredictionSteady();return 0;}
    if(name=="prediction_stops"){PredictionStops();return 0;}
    if(name=="prediction_lifecycle"){PredictionLifecycle();return 0;}
    if(name=="prediction_noisy_rebase"){PredictionNoisyRebase();return 0;}
    if(name=="prediction_cadence"){PredictionCadence();return 0;}
    if(name=="prediction_acceleration"){PredictionAcceleration();return 0;}
    if(name=="hybrid_policy")HybridPolicy();else if(name=="hybrid_transitions")HybridTransitions();
    else if(name=="hybrid_boundary")HybridBoundary();else if(name=="hybrid_recovery")HybridRecovery();
    else if(name=="hybrid_posture")HybridPosture();
    else if(name=="hybrid_low_glance")HybridLowGlance();else if(name=="hybrid_neck_limit")HybridNeckLimit();else if(name=="hybrid_fast_turn_down")HybridFastTurnDown();
    else if(name=="hybrid_glance_then_down")HybridGlanceThenDown();else if(name=="hybrid_missed_turn_heals")HybridMissedTurnHeals();else if(name=="hybrid_freeze_turn")HybridFreezeTurn();else if(name=="hybrid_down_glance_then_turn")HybridDownGlanceThenTurn();else if(name=="hybrid_hands_change_then_turn")HybridHandsChangeThenTurn();else if(name=="head_led_turn")HeadLedTurn();
    else {
    if(name=="noisy_gestures"){NoisyGestures();std::cout<<"PASS "<<name<<'\n';return 0;}
    if(name=="return_turns"){ReturnTurns();std::cout<<"PASS "<<name<<'\n';return 0;}
    if(name=="turns")Turns();else if(name=="head_led_turns")HeadLedTurns();else if(name=="head_led_noisy")HeadLedNoisy();else if(name=="head_only")HeadOnly();else if(name=="gestures")Gestures();else if(name=="tracking")Tracking();else if(name=="cadence")Cadence();else if(name=="noise")Noise();else if(name=="invariants")Invariants();else if(name=="noisy_turns")NoisyTurns();else if(name=="mixed_motion")MixedMotion();else if(name=="lifecycle")Lifecycle();else if(name=="noisy_idle")NoisyIdle();else if(name=="wrist_only")WristOnly();else if(name=="startup")Startup();else if(name=="source_freshness")Freshness();else if(name=="baselines")Baselines();else if(name=="neck_lever")NeckLever();else if(name=="small_turns")SmallTurns();else if(name=="discontinuities")Discontinuities();else if(name=="benchmark")Benchmark();else Check(false,"unknown case");
    }
    std::cout<<"PASS "<<name<<'\n';return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
