#include "Anim/WheelSteering.hpp"
#include "Anim/TrackedWheelSteering.hpp"
#include <cstdlib>
#include <bit>
#include <iostream>
#include <limits>
#include <string>
using namespace cvr::anim;
namespace {
constexpr float pi=3.14159265358979323846f,radius=.19f;
void Check(bool pass,const char* text) { if(!pass){std::cerr<<text<<'\n';std::exit(1);} }
void Near(float x,float y,const char* text) { Check(std::abs(x-y)<.002f,text); }
WheelPoint Rim(float degrees,int side=1,WheelPoint center={}) {
    const float a=-degrees*pi/180;
    return {center.x+side*radius*std::cos(a),center.y+side*radius*std::sin(a)};
}
float Tick(WheelSteering& wheel,int mask,WheelPoint right,WheelPoint left,float max=90,float dead=1.5f) {
    const float value=wheel.Update(uint8_t(mask),right,left,radius,max,dead);
    Check(std::isfinite(value)&&std::abs(value)<=1,"invalid output");return value;
}
void GrabOffset() {
    // Recorded stationary controller targets were below/asymmetric to the
    // animated rim. Neither hand may begin with the old absolute angle offset.
    for(int hand:{1,2}) {
        WheelSteering wheel;WheelPoint right{.178f,.40766f},left{-.24826f,.39796f};
        Near(Tick(wheel,hand,right,left),0,"grab started with steering offset");
        (hand==1 ? right:left).y-=.05f;
        const float out=Tick(wheel,hand,right,left);
        Check(hand==1 ? out>.1f:out<-.1f,"lowering a single hand did not steer");
    }
}
void OneHand() {
    for(int hand:{1,2})for(int direction:{-1,1}) {
        WheelSteering wheel;
        const WheelPoint center{.2f,-.1f};
        Tick(wheel,hand,Rim(0,1,center),Rim(0,-1,center));
        for(int angle=1;angle<=85;++angle) {
            Tick(wheel,hand,Rim(float(direction*angle),1,center),Rim(float(direction*angle),-1,center));
            Near(wheel.Angle(),float(direction*angle),"one-handed angle is not proportional or reverses");
        }
        for(int angle=84;angle>=0;--angle)Tick(wheel,hand,Rim(float(direction*angle),1,center),Rim(float(direction*angle),-1,center));
        Near(wheel.Angle(),0,"single hand did not return to center");
    }
}
void TwoHands() {
    WheelSteering wheel;Tick(wheel,3,Rim(0),Rim(0,-1));
    for(int i=0;i<=120;++i) {
        const float angle=60*std::sin(i*pi/120);
        const WheelPoint translation{.15f*std::sin(i*.05f),.10f*std::cos(i*.08f)};
        Tick(wheel,3,Rim(angle,1,translation),Rim(angle,-1,translation));
        Near(wheel.Angle(),angle,"common hand translation changed two-hand steering");
    }
}
void Handoff() {
    WheelSteering wheel;Tick(wheel,3,Rim(0),Rim(0,-1));
    for(int i=1;i<=40;++i)Tick(wheel,3,Rim(float(i)),Rim(float(i),-1));
    for(int mask:{1,3,2,3}) {
        Tick(wheel,mask,Rim(40),Rim(40,-1));
        Near(wheel.Angle(),40,"adding/releasing a hand changed the turn");
    }
    Tick(wheel,2,Rim(40),Rim(40,-1));
    Tick(wheel,2,Rim(39),Rim(39,-1));
    Near(wheel.Angle(),39,"remaining hand did not react on its next sample");
}
void LockReversal() {
    for(int mask:{1,2,3})for(int direction:{-1,1}) {
        WheelSteering wheel;Tick(wheel,mask,Rim(0),Rim(0,-1));
        for(int angle=1;angle<=210;++angle)Tick(wheel,mask,Rim(float(direction*angle)),Rim(float(direction*angle),-1));
        Near(wheel.Angle(),float(direction*210),"physical travel was discarded beyond full lock");
        for(int angle=209;angle>=0;--angle) {
            const float out=Tick(wheel,mask,Rim(float(direction*angle)),Rim(float(direction*angle),-1));
            Near(wheel.Angle(),float(direction*angle),"overtravel shifted the grab reference");
            Check(out*direction>=0,"return from full lock counter-steered before neutral");
        }
        Near(wheel.Angle(),0,"return after overtravel lost neutral");
        const float correction=Tick(wheel,mask,Rim(float(-direction*4)),Rim(float(-direction*4),-1));
        Check(correction*direction<0,"micro-correction after neutral did not change direction");
    }
}
void TrackingLoss() {
    WheelSteering wheel;Tick(wheel,1,Rim(0),{});Tick(wheel,1,Rim(30),{});
    Near(Tick(wheel,0,{},{}),0,"lost tracking held steering");
    Near(Tick(wheel,1,Rim(30),{}),0,"regrabbing revived old steering");
    const float nan=std::numeric_limits<float>::quiet_NaN();
    Near(Tick(wheel,1,{nan,0},{}),0,"invalid tracking retained steering");
    Tick(wheel,3,Rim(0),Rim(0,-1));Tick(wheel,3,Rim(20),Rim(20,-1));
    Tick(wheel,3,{0,0},{0,0});Near(wheel.Angle(),20,"collapsed span changed angle");
    Tick(wheel,3,Rim(-20),Rim(-20,-1));Near(wheel.Angle(),20,"span recovery jumped");
}
void RateMatrix() {
    for(int hz:{30,45,90,120})for(int mask:{1,2,3})for(float duration:{.25f,.6f,1.5f}) {
        WheelSteering wheel;Tick(wheel,mask,Rim(0),Rim(0,-1));
        const int frames=int(hz*duration);
        for(int i=1;i<=frames;++i) {
            const float angle=75*float(i)/frames;
            Tick(wheel,mask,Rim(angle),Rim(angle,-1));
            Near(wheel.Angle(),angle,"cadence changed steering response");
        }
    }
    WheelSteering wheel;Tick(wheel,3,Rim(0),Rim(0,-1));
    for(int i=0;i<1500;++i) {
        const float angle=.3f*std::sin(float(i)*1.3f);
        Near(Tick(wheel,3,Rim(angle),Rim(angle,-1)),0,"stationary jitter escaped deadzone");
    }
}
void Deadzone() {
    Near(WheelSteering::Output(0,90,0),0,"zero input not neutral");
    Near(WheelSteering::Output(1.5f,90,1.5f),0,"deadzone edge not neutral");
    Near(WheelSteering::Output(2,90,1.5f),.5f/88.5f,"micro-correction is not proportional");
    for(float limit:{30,60,90,120,180}) {
        Near(WheelSteering::Output(limit,limit,20),1,"full lock setting not honored");
        Near(WheelSteering::Output(-limit,limit,20),-1,"left full lock setting not honored");
    }
}
void ExtendedTravel() {
    Near(WheelFullLockDegrees(0),180,"unset limit did not default to half-turn");
    Near(WheelFullLockDegrees(std::numeric_limits<float>::quiet_NaN()),180,"invalid limit not sanitized");
    Near(WheelFullLockForVehicle(180,false),90,"car range changed motorcycle input");
    Near(WheelFullLockForVehicle(120,false),120,"valid legacy bike setting changed");
    for(int mask:{1,2,3})for(int direction:{-1,1}) {
        WheelSteering wheel;
        Tick(wheel,mask,Rim(0),Rim(0,-1),180);
        for(int angle=1;angle<=230;++angle) {
            const float physical=float(direction*angle);
            const float out=Tick(wheel,mask,Rim(physical),Rim(physical,-1),180);
            Near(wheel.Angle(),physical,"visual gain leaked into physical angle");
            if(angle==90)Near(out,direction*88.5f/178.5f,"quarter turn did not give half input");
            if(angle>=180)Near(out,float(direction),"half-turn did not reach full lock");
            if(angle==150) {
                const int other=mask==1?3:1;
                Tick(wheel,other,Rim(physical),Rim(physical,-1),180);
                Near(wheel.Angle(),physical,"extended handoff changed physical angle");
                Tick(wheel,mask,Rim(physical),Rim(physical,-1),180);
            }
        }
        for(int angle=229;angle>=0;--angle) {
            const float out=Tick(wheel,mask,Rim(float(direction*angle)),Rim(float(direction*angle),-1),180);
            if(angle==0)Near(out,0,"overtravel lost neutral at extended range");
        }
    }
    WheelPrediction prediction;uint64_t stamp=1000000;
    for(int i=0;i<4;++i){stamp+=10000;prediction.Push(127.f+i,stamp);}
    const float predicted=prediction.Angle(130,stamp,8,180,1.5f);
    Check(predicted>130 && predicted<=130.5f,"prediction still assumes old full lock");
}
void GamepadRange() {
    for(float inner:{0.f,.1f,.35f,.49f})for(float outer:{.7f,.9f,1.f}) {
        for(float desired:{-1.f,-.2f,-.02f,0.f,.001f,.02f,.2f,1.f}) {
            const float pad=WheelGamepadAxis(desired,inner,outer);
            // Model the game's two endpoint deadzone. Small rotations must not
            // disappear, and neutral must never emit the nonzero inner endpoint.
            const float read=std::copysign(std::clamp((std::abs(pad)-inner)/(outer-inner),0.f,1.f),pad);
            Near(read,desired,"gamepad deadzone changed requested wheel input");
            if(desired==0)Near(pad,0,"neutral emits a nonzero pad axis");
        }
    }
}
WheelTrackingFrame Tracking(uint64_t sequence,float headTime,float angle) {
    WheelTrackingFrame frame;
    frame.headValid=true;frame.validHands=3;frame.sequence=sequence;frame.origin=1;frame.stampUs=1000000+sequence*11111;
    const float yaw=.7f*std::sin(headTime),pitch=.4f*std::cos(headTime*.7f);
    frame.head.orientation=MultiplyQuat({0,std::sin(yaw*.5f),0,std::cos(yaw*.5f)},
                                        {std::sin(pitch*.5f),0,0,std::cos(pitch*.5f)});
    frame.head.position={.12f*std::sin(headTime),1.7f+.1f*std::cos(headTime),.08f*std::sin(headTime*.4f)};
    for(int i=0;i<2;++i) {
        const auto p=Rim(angle,i==0 ? 1:-1,{0,1.4f});
        const XrVector3f world{p.x,p.y,-.4f};
        const XrVector3f relative{world.x-frame.head.position.x,world.y-frame.head.position.y,world.z-frame.head.position.z};
        frame.hands[i]=RotateVector(ConjugateQuat(frame.head.orientation),relative);
    }
    return frame;
}
void TrackedHeadMotion() {
    for(int mask:{1,2,3}) {
        TrackedWheelSteering wheel;
        for(uint64_t i=1;i<=1000;++i) {
            const auto frame=Tracking(i,float(i)*.023f,0);
            Near(wheel.Update(uint8_t(mask),frame,frame.stampUs,radius,90,1.5f),0,
                 "head yaw/pitch/translation turned stationary physical hands");
            Near(wheel.Angle(),0,"head-relative coordinates drifted the physical neutral");
        }
    }
}
void TrackedRoundtrip() {
    for(int mask:{1,2,3})for(int direction:{-1,1}) {
        TrackedWheelSteering wheel;uint64_t sequence=0;
        for(int step=0;step<=420;++step) {
            const float angle=float(direction*(step<=210 ? step:420-step));
            const auto frame=Tracking(++sequence,step*.01f,angle);
            const float out=wheel.Update(uint8_t(mask),frame,frame.stampUs,radius,90,1.5f);
            Near(wheel.Angle(),angle,"raw tracking roundtrip changed the grab origin");
            Check(out*direction>=0,"raw tracking counter-steered on return from overtravel");
        }
        Near(wheel.Angle(),0,"physical neutral was not restored after overtravel");
    }
}
void TrackedLifetime() {
    TrackedWheelSteering wheel;auto frame=Tracking(1,0,0);
    wheel.Update(1,frame,frame.stampUs,radius,90,1.5f);
    frame=Tracking(2,.1f,30);wheel.Update(1,frame,frame.stampUs,radius,90,1.5f);
    Near(wheel.Angle(),30,"tracking setup failed");
    wheel.Update(3,frame,frame.stampUs,radius,90,1.5f);
    Near(wheel.Angle(),30,"same-sequence grip handoff reset the turn");
    Near(wheel.Update(3,frame,frame.stampUs+250001,radius,90,1.5f),0,"stale tracking retained steering");
    frame=Tracking(3,.2f,0);wheel.Update(1,frame,frame.stampUs,radius,90,1.5f);
    frame=Tracking(4,.2f,30);wheel.Update(1,frame,frame.stampUs,radius,90,1.5f);
    frame.origin=2;Near(wheel.Update(1,frame,frame.stampUs,radius,90,1.5f),0,"recenter retained old tracking-space pivot");
    frame=Tracking(5,.3f,0);frame.headValid=false;
    Near(wheel.Update(1,frame,frame.stampUs,radius,90,1.5f),0,"lost head tracking retained steering");
}
void PredictionConstant() {
    for(int hz:{30,45,90,120})for(float speed:{-180.f,-90.f,-45.f,-10.f,10.f,45.f,90.f,180.f}) {
        WheelPrediction prediction;const uint64_t step=1000000/hz;
        for(int i=0;i<4;++i) {
            const float angle=std::copysign(20.f,speed)+speed*float(i*step)*1e-6f;
            const uint64_t stamp=1000000+i*step;prediction.Push(angle,stamp);
            const float lead=prediction.Angle(angle,stamp,8,90,1.5f)-angle;
            if(i<3)Near(lead,0,"prediction started without consistent history");
            else Near(lead,std::clamp(speed*.008f,-.5f,.5f),"constant turn prediction outside expected bound");
        }
    }
}
void PredictionStopReverse() {
    WheelPrediction prediction;uint64_t stamp=1000000;float angle=20;
    for(int i=0;i<8;++i){angle+=.5f;stamp+=10000;prediction.Push(angle,stamp);}
    Check(prediction.Angle(angle,stamp,8,90,1.5f)>angle,"moving setup has no lead");
    stamp+=10000;prediction.Push(angle,stamp);
    Near(prediction.Angle(angle,stamp,8,90,1.5f),angle,"stop retained a prediction tail");
    for(int i=0;i<6;++i){angle+=.5f;stamp+=10000;prediction.Push(angle,stamp);}
    angle-=.5f;stamp+=10000;prediction.Push(angle,stamp);
    Near(prediction.Angle(angle,stamp,8,90,1.5f),angle,"reversal used outgoing velocity");
    for(int i=0;i<4;++i){angle-=.5f;stamp+=10000;prediction.Push(angle,stamp);}
    Check(prediction.Angle(angle,stamp,8,90,1.5f)<angle,"confirmed reverse motion did not predict");
    prediction.Reset();
    for(int i=0;i<20;++i){stamp+=10000;prediction.Push(0,stamp);}
    for(int i=0;i<20;++i){stamp+=10000;prediction.Push(4,stamp);
        Near(prediction.Angle(4,stamp,8,90,1.5f),4,"a step from rest gained unconfirmed overshoot");}
}
void PredictionBoundaries() {
    for(float angle:{-210.f,-90.f,-89.9f,-3.f,-1.51f,-1.5f,-.1f,0.f,.1f,1.5f,1.51f,3.f,89.9f,90.f,210.f})
    for(int direction:{-1,1}) {
        WheelPrediction prediction;uint64_t stamp=1000000;
        for(int i=0;i<4;++i){stamp+=10000;prediction.Push(angle-float(direction*(3-i)),stamp);}
        const float result=prediction.Angle(angle,stamp,8,90,1.5f);
        Check(std::abs(result-angle)<=.50001f,"prediction exceeded half a degree");
        if(std::abs(angle)<=1.5f || std::abs(angle)>=90)Near(result,angle,"prediction altered neutral or full-lock saturation");
        else Check(result*angle>0 && std::abs(result)>=1.5f && std::abs(result)<=90,
                   "prediction crossed neutral/deadzone/full lock");
    }
}
void PredictionFreshness() {
    WheelPrediction prediction;uint64_t stamp=1000000;
    for(int i=0;i<4;++i){stamp+=10000;prediction.Push(20.f+i,stamp);}
    const float first=prediction.Angle(23,stamp,8,90,1.5f);
    for(int i=1;i<=20;++i)Near(prediction.Angle(23,stamp+i*1000,8,90,1.5f),first,"polling extrapolated the same pose farther");
    Near(prediction.Angle(23,stamp+60001,8,90,1.5f),23,"stale history retained prediction");
    Near(prediction.Angle(23,stamp-1,8,90,1.5f),23,"future-dated history was accepted");
    Near(prediction.Angle(23,stamp,0,90,1.5f),23,"disabled prediction changed steering");
    Near(prediction.Angle(23,stamp,100,90,1.5f),first,"horizon exceeded its hard cap");
    prediction.Push(24,stamp+100000);Near(prediction.Angle(24,stamp+100000,8,90,1.5f),24,"publication gap reused old history");
}
void PredictionTracking() {
    for(int mask:{1,2,3})for(int direction:{-1,1}) {
        TrackedWheelSteering direct,predicted,disabled;uint64_t sequence=0;
        for(int step=0;step<=420;++step) {
            const float angle=float(direction*(step<=210 ? step:420-step));
            const auto frame=Tracking(++sequence,step*.01f,angle);
            const float reference=direct.Update(uint8_t(mask),frame,frame.stampUs,radius,90,1.5f);
            const float value=predicted.Update(uint8_t(mask),frame,frame.stampUs,radius,90,1.5f,8);
            Near(predicted.Angle(),direct.Angle(),"prediction fed back into physical angle");
            Check(value*direction>=0,"prediction counter-steered during a physical return");
            if(step==0 || step==420)Near(value,0,"predicted roundtrip missed neutral");
            Check(std::bit_cast<uint32_t>(disabled.Update(uint8_t(mask),frame,frame.stampUs,radius,90,1.5f,0))==std::bit_cast<uint32_t>(reference),
                  "disabled prediction changed the direct input bits");
        }
    }
    TrackedWheelSteering wheel;auto frame=Tracking(1,0,0);
    wheel.Update(3,frame,frame.stampUs,radius,90,1.5f,8);
    for(int i=2;i<24;++i){frame=Tracking(i,.1f*i,float(i));wheel.Update(3,frame,frame.stampUs,radius,90,1.5f,8);}
    float raw=WheelSteering::Output(wheel.Angle(),90,1.5f);
    Check(wheel.Update(3,frame,frame.stampUs,radius,90,1.5f,8)>raw,"tracked prediction setup failed");
    Near(wheel.Update(3,frame,frame.stampUs,radius,90,1.5f,0),raw,"disabling prediction retained the lead");
    Near(wheel.Update(3,frame,frame.stampUs,radius,90,1.5f,8),raw,"enabling prediction reused pre-toggle history");
    for(int i=24;i<30;++i){frame=Tracking(i,.1f*i,float(i));wheel.Update(3,frame,frame.stampUs,radius,90,1.5f,8);}
    raw=WheelSteering::Output(wheel.Angle(),90,1.5f);
    Check(wheel.Update(3,frame,frame.stampUs,radius,90,1.5f,8)>raw,"prediction did not rearm after a toggle");
    Near(wheel.Update(1,frame,frame.stampUs,radius,90,1.5f,8),raw,"grip handoff carried prediction velocity");
    frame.origin=2;Near(wheel.Update(1,frame,frame.stampUs,radius,90,1.5f,8),0,"recenter carried predicted input");
}
void PredictionJitter() {
    for(float centre:{0.f,20.f,-20.f}) {
        WheelPrediction prediction;float maximum=0;
        for(int i=0;i<1500;++i) {
            const float angle=centre+.3f*std::sin(i*1.0471f);
            const uint64_t stamp=1000000+i*11111;prediction.Push(angle,stamp);
            const float result=prediction.Angle(angle,stamp,8,90,1.5f);
            maximum=std::max(maximum,std::abs(result-angle));
            if(centre==0)Near(WheelSteering::Output(result,90,1.5f),0,"neutral jitter triggered steering");
        }
        Check(maximum<.15f,"fixed-position jitter gained excessive prediction");
    }
}
void PredictionModel() {
    // This is a numerical continuing-motion comparison, not measured hardware
    // latency: apply a fixed 44ms plant delay to the unfiltered/predicted input.
    for(int hz:{30,45,90,120}) {
        WheelPrediction prediction;double directError=0,predictedError=0;int count=0;
        const auto motion=[](double t){return float(30+15*std::sin(2*3.141592653589793*.4*t));};
        for(int i=0;i<8*hz;++i) {
            const double t=double(i)/hz;const auto stamp=uint64_t(1000000+t*1000000);
            const float angle=motion(t);prediction.Push(angle,stamp);
            const float predicted=prediction.Angle(angle,stamp,8,90,1.5f);
            if(i>hz) {
                const float target=motion(t+.044);
                directError+=(angle-target)*(angle-target);predictedError+=(predicted-target)*(predicted-target);++count;
            }
        }
        Check(predictedError<directError*.95,"conservative lead did not help continuing-motion model");
        std::cout<<hz<<"Hz modeled RMS degrees: direct="<<std::sqrt(directError/count)
                 <<" predicted="<<std::sqrt(predictedError/count)<<'\n';
    }
}
}
// Held at the top, the hub is below the hand: a pull to the right is a turn to the right, as much as at 3 o'clock going down.
void OneHandTop() {
    cvr::anim::WheelSteering top,side;
    const float r=.18f,pi=3.14159265f,a=30/57.295779513f;
    top.Update(1,{0,r},{},r,180,0,pi/2);
    side.Update(1,{r,0},{},r,180,0,0);
    const float turnedTop=top.Update(1,{r*std::sin(a),r*std::cos(a)},{},r,180,0,pi/2);
    const float turnedSide=side.Update(1,{r*std::cos(a),-r*std::sin(a)},{},r,180,0,0);
    if(std::abs(top.Angle()-30)>.01f || std::abs(side.Angle()-30)>.01f || std::abs(turnedTop-turnedSide)>.001f) {
        std::cerr<<"top "<<top.Angle()<<" side "<<side.Angle()<<'\n';std::exit(1);
    }
    // a sideways pull at the top is a full tangent: nothing is lost to a hub assumed beside the hand
    cvr::anim::WheelSteering wrong;
    wrong.Update(1,{0,r},{},r,180,0);
    wrong.Update(1,{r*std::sin(a),r*std::cos(a)},{},r,180,0);
    if(!(std::abs(wrong.Angle())<25)) {std::cerr<<"the old hub read it as "<<wrong.Angle()<<'\n';std::exit(1);}
    // the old assumption, kept when no rim angle is given: left hand at 9 o'clock
    cvr::anim::WheelSteering left;
    left.Update(2,{},{-r,0},r,180,0);
    left.Update(2,{},{-r*std::cos(a),r*std::sin(a)},r,180,0);
    if(std::abs(left.Angle()-30)>.01f){std::cerr<<"left "<<left.Angle()<<'\n';std::exit(1);}
}

int main(int argc,char** argv) {
    if(argc!=2)return 2;const std::string name=argv[1];
    if(name=="grab_offset")GrabOffset();else if(name=="one_hand")OneHand();else if(name=="one_hand_top")OneHandTop();
    else if(name=="two_hands")TwoHands();else if(name=="handoff")Handoff();
    else if(name=="lock_reversal")LockReversal();else if(name=="tracking_loss")TrackingLoss();
    else if(name=="rate_matrix")RateMatrix();else if(name=="deadzone")Deadzone();
    else if(name=="extended_travel")ExtendedTravel();
    else if(name=="gamepad_range")GamepadRange();else if(name=="tracked_head_motion")TrackedHeadMotion();
    else if(name=="tracked_roundtrip")TrackedRoundtrip();else if(name=="tracked_lifetime")TrackedLifetime();
    else if(name=="prediction_constant")PredictionConstant();else if(name=="prediction_stop_reverse")PredictionStopReverse();
    else if(name=="prediction_boundaries")PredictionBoundaries();else if(name=="prediction_freshness")PredictionFreshness();
    else if(name=="prediction_tracking")PredictionTracking();else if(name=="prediction_jitter")PredictionJitter();
    else if(name=="prediction_model")PredictionModel();else return 2;
    std::cout<<"PASS "<<name<<'\n';
}
