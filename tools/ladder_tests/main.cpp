#include "Runtimes/LadderClimbing.hpp"
#include "Anim/LadderFingerTuning.hpp"
#include "Anim/LadderRungWrist.hpp"
#include <cstdlib>
#include <iostream>
#include <string>
using namespace cvr::ladder;
void Check(bool ok,const char* text) { if(!ok) { std::cerr<<"FAIL: "<<text<<'\n';std::exit(1); } }
Geometry GeometryAtOrigin() { Geometry g;g.normal={0,-1,0};g.up={0,0,1};g.height=8;g.identity=1;Check(g.Prepare(),"geometry rejected");return g; }
struct Rig {
    Geometry geometry=GeometryAtOrigin();Climber climb;Frame f{};
    Rig() {
        f.valid=true;f.handValid[0]=f.handValid[1]=true;f.sequence=1;f.origin=1;f.stamp=1000000;f.height=1;
        f.world[0]={-.3f,-.025f,2};f.world[1]={.3f,-.025f,2.4f};
        f.tracking[0]={-.3f,0,1.2f};f.tracking[1]={.3f,0,1.6f};Tick();
    }
    float Tick(bool allowed=true,uint64_t dt=20000) { ++f.sequence;f.stamp+=dt;return climb.Update(geometry,f,f.stamp,allowed); }
    void Grab(bool both=false) { f.grip[0]=1;if(both)f.grip[1]=1;Check(Tick()==0,"grip press itself moved the body");Check(climb.Held(0),"rail grip did not attach"); }
};
void GeometryTest() {
    const auto g=GeometryAtOrigin();
    const auto rail=Closest(g,{.3f,-.08f,2.17f});Check(rail.kind==1 && rail.distance<.06f,"side rail is not grabbable between rungs");
    const auto rung=Closest(g,{0,-.06f,2.4f});Check(rung.kind==2 && rung.distance<.04f,"rung centre is not grabbable");
    Check(Closest(g,{0,-.025f,2.2f}).distance>.11f,"air between rungs counted as a contact");
    Check(Closest(g,{.8f,-.025f,2.4f}).distance>.11f,"grip outside ladder width accepted");
    Geometry shifted=g;shifted.position={-1862.615f,-2354.831f,19.754f};shifted.normal={.7156f,-.6985f,0};shifted.Prepare();
    const auto world=shifted.position+shifted.up*4.4f+shifted.normal*.025f;
    Check(Closest(shifted,world).distance<.001f,"rotated world ladder lost rung spacing");
}
void Directions() {
    Rig up;up.Grab();up.f.tracking[0].z-=.1f;Check(up.Tick()>0,"downward pull did not climb up");
    Rig down;down.Grab();down.f.tracking[0].z+=.1f;Check(down.Tick()<0,"upward push did not climb down");
    Rig idle;idle.Grab();for(int i=0;i<100;++i)Check(idle.Tick()==0,"stationary gripped hand moved the body");
    Rig rung;rung.f.world[0]={0,-.025f,2.4f};rung.Grab();Check(rung.climb.Kind(0)==2,"rung grip became side grip");
}
void WristLock() {
    Rig r;r.Grab(true);Vec point{};Rotation wrist{std::sin(.3f),0,0,std::cos(.3f)};
    const Rotation body{0,0,std::sin(.7f),std::cos(.7f)};
    const Rotation firstWorld=GripRotation(r.geometry,Closest(r.geometry,r.f.world[0]),0);
    const Vec origin{2,-3,1};
    Check(r.climb.Constrain(0,origin,body,point,wrist),"first grip did not lock wrist");
    for(float yaw:{-.9f,0.0f,1.6f}) {
        const Rotation turned{0,0,std::sin(yaw/2),std::cos(yaw/2)};
        wrist={0,std::sin(.8f),0,std::cos(.8f)};
        Check(r.climb.Constrain(0,origin,turned,point,wrist),"held grip lost wrist lock");
        const auto world=turned*wrist;
        Check(std::abs(world.x*firstWorld.x+world.y*firstWorld.y+world.z*firstWorld.z+world.w*firstWorld.w)>.99999f,
            "controller/body rotation rotated the gripped wrist in world space");
        Check(Length(turned.Rotate(point+wrist.Rotate(PalmOffset(0)))+origin-r.climb.Anchor(0))<.00001f,"body frame moved fixed palm contact");
    }
    Rotation right{};Check(r.climb.Constrain(1,{},Rotation{},point,right),"other hand did not lock independently");
    const auto expectedRight=GripRotation(r.geometry,Closest(r.geometry,r.f.world[1]),1);
    Check(std::abs(right.w-expectedRight.w)<.00001f,"other hand inherited left wrist rotation");
    r.f.grip[0]=0;r.Tick();Check(!r.climb.Constrain(0,{},Rotation{},point,wrist),"release retained wrist lock");
    r.f.world[0]={0,-.025f,2.4f};r.Grab();wrist={};r.climb.Constrain(0,{},Rotation{},point,wrist);
    const auto expectedRung=GripRotation(r.geometry,Closest(r.geometry,r.f.world[0]),0);
    Check(std::abs(wrist.w-expectedRung.w)<.00001f,"rung regrip reused old side wrist rotation");
}
void RungWrist() {
    const auto geometry=GeometryAtOrigin();const auto contact=Closest(geometry,{0,-.025f,2.4f});
    for(int side=0;side<2;++side)for(float yaw:{0.0f,.9f,-1.8f})for(float pitch:{0.0f,.45f}) {
        const Rotation body=Rotation{0,0,std::sin(yaw/2),std::cos(yaw/2)}*
            Rotation{std::sin(pitch/2),0,0,std::cos(pitch/2)};
        const auto original=GripRotation(geometry,contact,side);
        const auto oldWrist=contact.point-original.Rotate(PalmOffset(side));
        Vec wrist=body.Inverse().Rotate(oldWrist);Rotation rotation=body.Inverse()*original;
        CorrectRungWrist(side,body.Inverse().Rotate(geometry.right),body.Inverse().Rotate(geometry.up),wrist,rotation);
        const Vec world=body.Rotate(wrist),palm=world+(body*rotation).Rotate(PalmOffset(side));
        Check(Length(palm-(contact.point-geometry.up*.02f))<.00001f,"2cm rung calibration changed with hand/body orientation");
        const Vec previousWrist=contact.point-(body*rotation).Rotate(PalmOffset(side));
        Check(Length(world-previousWrist+geometry.up*.02f)<.00001f,"wrist was not lowered exactly2cm from approved angled pose");
        const auto q=body*rotation;
        const float dot=q.x*original.x+q.y*original.y+q.z*original.z+q.w*original.w;
        Check(std::abs(2*std::acos(std::clamp(std::abs(dot),0.0f,1.0f))*57.2957795f-25)<.001f,"rung correction differs between hands/body headings");
    }
}
void TopExit() {
    Rig r;r.geometry.height=17.2f;r.geometry.topStep=-.4f;r.geometry.Prepare();
    r.f.autoFinish=false; // manual top-rail climb remains usable with assistance off
    Check(Closest(r.geometry,{0,-.025f,17.6f}).distance<.001f,"last rung above exit plane is missing");
    const Vec origin{0,.112f,17.6f};
    Check(r.geometry.SetTopRails(origin,{1,0,0},{0,-1,0},{0,0,1}),"real finisher transform rejected");
    const Vec hand=origin+Vec{.4023f,-.1485f,.50f};
    Check(Closest(r.geometry,hand).kind==3 && Closest(r.geometry,hand).distance<.001f,"upper finisher rail cannot be gripped");
    Check(Closest(r.geometry,origin+Vec{0,0,.50f}).distance>.11f,"air between upper rails accepted as a rung");
    Check(!r.geometry.SetTopRails(origin+Vec{0,0,3},{1,0,0},{0,-1,0},{0,0,1}),"different ladder finisher accepted");
    r.f.height=16.95f;r.f.world[0]=hand;r.Tick();r.Grab();
    const float start=r.f.height;
    for(int frame=0;frame<130;++frame) {
        if(frame<32)r.f.tracking[0].z-=.01f;
        const float axis=r.Tick();r.f.height+=axis*1.5f*.02f;
        r.f.world[0]=hand+Vec{0,0,r.f.height-start};
    }
    Check(r.f.height>r.geometry.height+.05f,"upper rail pull never reached native exit height");
    Check(r.Tick(false)==0 && !r.climb.Held(0),"native ladder exit retained grip input");
}
void AutoFinish() {
    auto nearTop=[](Rig& r) { r.f.height=r.geometry.height-.6f;r.Tick();r.Grab(); };
    auto pull=[](Rig& r) { for(int i=0;i<7;++i) { r.f.tracking[0].z-=.01f;r.Tick(); } };
    Rig idle;nearTop(idle);for(int i=0;i<60;++i)Check(idle.Tick()==0 && !idle.climb.Finishing(),"idle grip near top started auto climb");
    idle.f.tracking[0].z+=.08f;Check(idle.Tick()<0 && !idle.climb.Finishing(),"descending grip started auto climb");
    Rig up;nearTop(up);pull(up);Check(up.climb.Finishing(),"last upward pull did not arm finish");
    up.f.grip[0]=0;
    while(up.f.height<up.geometry.height+.03f) { const float axis=up.Tick();Check(axis>0,"releasing hands stopped the last climb");up.f.height+=axis*1.5f*.02f; }
    Check(up.Tick(false)==0 && !up.climb.Finishing(),"native exit retained automatic input");
    Rig cancel;nearTop(cancel);pull(cancel);cancel.f.tracking[0].z+=.04f;
    Check(cancel.Tick()<0 && !cancel.climb.Finishing(),"upward hand push did not cancel finish into descent");
    Rig stall;nearTop(stall);pull(stall);stall.f.grip[0]=0;
    for(int i=0;i<70;++i)stall.Tick();Check(stall.Tick()==0 && !stall.climb.Finishing(),"blocked native climb never timed out");
    // A ladder whose floor is 0.4 m above its exit plane (verticalStepTop -0.4, the generic finisher ladders): the
    // last climb carries V all the way onto it, through a pause while the exit moves V forward.
    Rig step;step.geometry.topStep=-.4f;step.geometry.Prepare();nearTop(step);pull(step);
    Check(step.climb.Finishing(),"last upward pull did not arm finish on a stepped top");
    step.f.grip[0]=0;
    int paused=0;
    for(int i=0;i<200 && step.f.height<step.geometry.height+.42f;++i) {
        const float axis=step.Tick();Check(axis>0,"finish stopped short of a raised top floor");
        if(step.f.height>step.geometry.height+.2f && paused<45) { ++paused;continue; }   // the exit moves V forward
        step.f.height+=axis*1.5f*.02f;
    }
    Check(step.f.height>=step.geometry.height+.42f,"V never reached the raised top floor");
    Check(step.Tick(false)==0 && !step.climb.Finishing(),"native exit retained automatic input on a stepped top");
    Rig lost;nearTop(lost);pull(lost);lost.f.handValid[0]=lost.f.handValid[1]=false;
    Check(lost.Tick()==0 && !lost.climb.Finishing(),"tracking loss kept automatic movement");
    Rig disabled;nearTop(disabled);disabled.f.autoFinish=false;pull(disabled);disabled.f.grip[0]=0;
    Check(disabled.Tick()==0 && !disabled.climb.Finishing(),"disabled auto finish ignored release");
    Rig far;far.Grab();pull(far);Check(!far.climb.Finishing(),"ordinary ladder stroke started final climb");
    Rig metre;metre.f.height=metre.geometry.height-.95f;metre.Tick();metre.Grab();pull(metre);
    Check(metre.climb.Finishing(),"100 cm default did not allow a pull at 95 cm");
    Rig outside;outside.f.height=outside.geometry.height-1.05f;outside.Tick();outside.Grab();pull(outside);
    Check(!outside.climb.Finishing(),"automatic finish started outside the metre zone");
}
void FingerSettings() {
    int count=0;
    for(int h=0;h<2;++h)for(int f=0;f<5;++f)for(int c=0;c<AdjustmentCount;++c)if(HasAdjustment(f,c)) {
        char line[128];std::snprintf(line,sizeof(line),"xr_ladder_rung_%s_%s_%s=%.3f",HandNames[h],FingerNames[f],AdjustmentNames[c],-13.25f);
        int hand,finger,channel;float degrees;
        Check(ParseFingerAdjustment(line,hand,finger,channel,degrees),"saved finger control could not be parsed");
        Check(hand==h && finger==f && channel==c && degrees==-13.25f,"saved control changed hand/finger/joint");++count;
    }
    int h,f,c;float value;
    Check(count==58,"finger adjustment count changed");
    Check(!ParseFingerAdjustment("xr_ladder_rung_index=1.2",h,f,c,value),"legacy curl key mistaken for joint tuning");
    Check(!ParseFingerAdjustment("xr_ladder_rung_left_thumb_tip=5",h,f,c,value),"nonexistent third thumb joint accepted");
    Check(ParseFingerAdjustment("xr_ladder_rung_right_index_base = 900",h,f,c,value) && value==60,"joint offset not bounded");
    Check(ParseFingerAdjustment("xr_ladder_rung_left_thumb_roll=nan",h,f,c,value) && value==0,"invalid slider value reached skeleton");
}
void TwoHands() {
    Rig one,two;one.Grab();two.Grab(true);
    one.f.tracking[0].z-=.08f;two.f.tracking[0].z-=.08f;two.f.tracking[1].z-=.08f;
    Check(std::abs(one.Tick()-two.Tick())<1e-6f,"two hands doubled climbing speed");
    const auto before=two.climb.Debt();
    for(int i=0;i<100;++i)two.climb.Update(two.geometry,two.f,two.f.stamp+i,true);
    Check(two.climb.Debt()==before,"repeated hand packet accumulated displacement");
}
void Release() {
    Rig r;r.Grab();r.f.tracking[0].z-=.1f;Check(r.Tick()>0,"pull missing");
    r.f.grip[0]=0;Check(r.Tick()==0 && !r.climb.Held(0) && r.climb.Debt()==0,"release retained movement debt");
    r.f.world[0]={2,0,2};r.f.grip[0]=1;r.Tick();Check(!r.climb.Held(0),"grip in empty space attached");
    r.f.world[0]={-.3f,-.025f,2};r.Tick();Check(!r.climb.Held(0),"held button silently reattached after missing ladder");
    r.f.grip[0]=0;r.Tick();r.Grab();
}
void Discontinuities() {
    Rig r;r.Grab();r.f.tracking[0].z-=.1f;r.Tick();r.f.origin++;
    Check(r.Tick()==0 && !r.climb.Held(0),"recenter retained a grip");
    r.f.grip[0]=0;r.Tick();r.Grab();r.geometry.identity++;
    Check(r.Tick()==0 && !r.climb.Held(0),"different ladder retained grip");
    r.f.grip[0]=0;r.Tick();r.Grab();r.f.handValid[0]=false;
    Check(r.Tick()==0 && !r.climb.Held(0),"tracking loss retained movement");
    r.f.handValid[0]=true;r.f.grip[0]=0;r.Tick();r.Grab();
    Check(r.climb.Update(r.geometry,r.f,r.f.stamp+250001,true)==0,"stale tracking kept climbing");
    r.f.grip[0]=0;r.Tick();r.Grab();Check(r.Tick(false)==0,"menu/exit did not release input");
}
void ClosedLoop() {
    for(float speed:{.7f,1.5f,3.0f})for(int sign:{-1,1}) {
        Rig r;r.Grab();const float start=r.f.height;
        for(int frame=0;frame<150;++frame) {
            if(frame<20)r.f.tracking[0].z-=sign*.01f;
            const float axis=r.Tick();r.f.height+=axis*speed*.02f;
            Check(std::abs(r.f.height-start)<=.201f,"native response overshot requested hand travel");
        }
        Check(std::abs(r.f.height-(start+sign*.20f))<.003f,"body did not catch up to the signed hand displacement");
        r.f.grip[0]=0;Check(r.Tick()==0,"controller did not stop after release");
    }
}
void State() {
    for(int s:{10,11,12})Check(OnLadder(s)&&Fresh(1,1,1000,1200,s),"native ladder state excluded");
    for(int s:{0,8,9,13,14})Check(!OnLadder(s),"jump/vault/fall mistaken for attached ladder");
    Check(!Fresh(1,2,1000,1200,10)&&!Fresh(1,1,1000,251001,10),"stale/other player ladder state accepted");
}
// The 10 m ladder of 2026-11-05 (b_h400, b_h400, a_h200, a finisher rail at 10.0): its description says 9.1 / -0.3,
// so the last rungs (9.6, 10.0) and the rail were out of reach until its modules gave the top.
void MeasuredTop() {
    Geometry g=GeometryAtOrigin();g.height=9.1f;g.topStep=-.3f;Check(g.Prepare(),"geometry rejected");
    Check(std::abs(g.PhysicalTop()-9.4f)<.001f,"description top changed");
    Check(Closest(g,{0,-.025f,10.0f}).distance>.11f && Closest(g,{0,-.025f,9.6f}).distance>.11f,
        "test premise: the last rungs were already reachable");
    const Vec rail{0,.112f,10.0f};
    Check(!g.SetTopRails(rail,{1,0,0},{0,-1,0},{0,0,1}),"test premise: the rail was already accepted");
    Check(!g.SetMeasuredTop(14.5f) && !g.SetMeasuredTop(-1) && !g.SetMeasuredTop(std::nanf("")),"implausible top accepted");
    Check(g.SetMeasuredTop(10.0f) && std::abs(g.PhysicalTop()-10.0f)<.001f,"measured top refused");
    for(float z:{9.6f,10.0f}) {
        const auto rung=Closest(g,{0,-.03f,z});
        Check(rung.kind==2 && rung.distance<.01f,"a last rung below the measured top cannot be gripped");
    }
    Check(Closest(g,{.3f,-.03f,9.9f}).kind==1 && Closest(g,{.3f,-.03f,9.9f}).distance<.01f,"rail top cut short");
    Check(g.SetTopRails(rail,{1,0,0},{0,-1,0},{0,0,1}),"finisher at the measured top refused");
    Check(Closest(g,{0,-.025f,10.4f}).distance>.11f,"a rung above the ladder's top accepted");
}
void MasterGate() {
    Check(!GripEnabled(false,true),"master off left ladder grip enabled");
    Check(!GripEnabled(true,false),"ladder-specific off ignored");
    Check(GripEnabled(true,true),"enabled ladder grip rejected");
    Rig r;r.Grab();r.f.tracking[0].z-=.1f;Check(r.Tick()>0,"test has no pull");
    Check(r.Tick(GripEnabled(false,true))==0,"master off retained climb axis");
    Check(!r.climb.Held(0),"master off retained hand lock");
    Check(r.Tick(GripEnabled(true,true))==0 && !r.climb.Held(0),"reenabled with held grip auto-attached");
    r.f.grip[0]=0;r.Tick();r.Grab();
}
int main(int argc,char** argv) {
    const std::string test=argc>1?argv[1]:"";
    if(test=="geometry")GeometryTest();else if(test=="top_exit")TopExit();else if(test=="wrist_lock")WristLock();
    else if(test=="measured_top")MeasuredTop();
    else if(test=="rung_wrist")RungWrist();
    else if(test=="auto_finish")AutoFinish();else if(test=="finger_settings")FingerSettings();
    else if(test=="directions")Directions();else if(test=="two_hands")TwoHands();
    else if(test=="release")Release();else if(test=="discontinuities")Discontinuities();else if(test=="closed_loop")ClosedLoop();
    else if(test=="master_gate")MasterGate();
    else if(test=="state")State();else return 2;
    std::cout<<"PASS "<<test<<'\n';return 0;
}
