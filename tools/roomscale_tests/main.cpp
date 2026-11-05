#include "Runtimes/RoomscaleMovement.hpp"
#include "Hooks/RoomscaleEligibility.hpp"
#include "Hooks/TurnInput.hpp"
#include "Hooks/VehicleButtons.hpp"
#include "Runtimes/RoomscaleTracking.hpp"
#include "Camera/EyeCentreLedger.hpp"
#include "Camera/AnchorTranslation.hpp"
#include "Camera/RenderedPoseHistory.hpp"
#include "Camera/NativeCameraPair.hpp"
#include "Camera/VehicleStereoHeading.hpp"
#include "Camera/SceneCameraHeading.hpp"
#include "Camera/HandViewFrame.hpp"
#include "Runtimes/HandPublication.hpp"
#include "Runtimes/FrameAim.hpp"
#include "Runtimes/TrackingFilter.hpp"
#include "Runtimes/TrackingReset.hpp"
#include "Runtimes/LookDownCone.hpp"
#include "Runtimes/BodyYawFollower.hpp"
#include "Anim/ScenePolicy.hpp"
#include <cstdlib>
#include <iostream>
#include <limits>
#include <string>
#include <atomic>
#include <thread>
#include <vector>
#include <random>

using namespace cvr::roomscale;
namespace {
struct PoseLabel {
    float posX{},posY{},posZ{},oriX{},oriY{},oriZ{},oriW{1};
    bool valid{true};
    uint64_t originSerial{1};
};
constexpr float pi = 3.14159265358979323846f;
void Check(bool value, const char* message) {
    if (!value) { std::cerr << "FAIL: " << message << '\n'; std::exit(1); }
}
void Near(float value, float expected, const char* message, float tolerance = 0.00003f) {
    Check(std::isfinite(value) && std::abs(value - expected) <= tolerance, message);
}
void Near(Vec2 value, Vec2 expected, const char* message, float tolerance = 0.00003f) {
    Near(value.x, expected.x, message, tolerance); Near(value.y, expected.y, message, tolerance);
}
Sample Pose(uint64_t seq, float x, float y = 0, uint64_t time = 0, uint64_t origin = 1) {
    return {{x, y}, seq, origin, time ? time : seq*10000, true};
}
Step Tick(Movement& m, Sample p, float yaw = 0, bool allowed = true, float scale = 1, float dt = 0.01f) {
    auto step = m.Begin(p, p.stampUs, dt, scale, yaw, allowed); m.Complete(step); return step;
}
void VehicleStereoHeading() {
    const float entity[4]={0,0,0,1};
    // Read-only paused capture, PID28248: the old independent yaw correction
    // reproduces MAIN exactly while VRCAM retains the shared composition.
    const float nativeMain[4]={-.020116329f,.004417845f,-.887486875f,.460372448f};
    const XrQuaternionf liveShared{-.038748689f,.062863640f,-.984293222f,.160354912f};
    const XrQuaternionf liveMain{-.054560460f,.049763985f,-.902089536f,.425183862f};
    const float sharedYaw=-2.742873430252075f;
    const float mainYaw=-125.1890127435023f*pi/180;
    auto yawQuat=[](float yaw) { return XrQuaternionf{0,0,std::sin(yaw*.5f),std::cos(yaw*.5f)}; };
    auto angle=[](const XrQuaternionf& a,const XrQuaternionf& b) {
        const double dot=double(a.x)*b.x+double(a.y)*b.y+double(a.z)*b.z+double(a.w)*b.w;
        const double aa=double(a.x)*a.x+double(a.y)*a.y+double(a.z)*a.z+double(a.w)*a.w;
        const double bb=double(b.x)*b.x+double(b.y)*b.y+double(b.z)*b.z+double(b.w)*b.w;
        return float(2*std::acos(std::min(1.0,std::abs(dot)/std::sqrt(aa*bb)))*180/pi);
    };
    const auto oldMain=MultiplyQuat(yawQuat(mainYaw-sharedYaw),liveShared);
    Near(angle(oldMain,liveMain),0,"captured vehicle split is not explained by per-eye yaw catch-up",.0001f);
    Near(angle(oldMain,liveShared),31.96606128f,"captured stereo divergence changed",.0001f);

    const auto mappedHead=MultiplyQuat(yawQuat(-sharedYaw),liveShared);
    cvr::camera::VehicleStereoHeading bases[2];
    for(auto& base:bases) {
        base.SetSource(10,20);
        Check(base.Observe(20,nativeMain,entity),"current MAIN native heading rejected");
    }
    float maxSplit=0,maxOrderError=0;
    for(uint64_t epoch=1;epoch<=64;++epoch) {
        const auto head=MultiplyQuat(yawQuat(float(epoch)*.02f),mappedHead);
        const auto expected=MultiplyQuat(yawQuat(mainYaw),head);
        XrQuaternionf results[2][2]{};
        for(int order=0;order<2;++order)for(int call=0;call<2;++call) {
            const bool main=(order==call);
            // Vehicle camera attachments can disagree, and that difference
            // can change on entering or leaving window combat.
            const auto secondary=yawQuat(sharedYaw+float(epoch)*.013f);
            const float secondaryQ[4]={secondary.x,secondary.y,secondary.z,secondary.w};
            const bool accepted=bases[order].Observe(main ? 20 : 30,main ? nativeMain : secondaryQ,entity);
            Check(accepted==main,"secondary camera replaced the native MAIN source");
            const float yaw=bases[order].ForFrame(epoch,entity,.8f,0).yaw;
            Near(yaw,mainYaw,"vehicle view adopted the secondary attachment heading");
            results[order][call]=MultiplyQuat(yawQuat(yaw),head);
            Near(angle(results[order][call],expected),0,"vehicle camera composed on the wrong heading",.0001f);
        }
        maxSplit=std::max(maxSplit,angle(results[0][0],results[0][1]));
        maxOrderError=std::max(maxOrderError,angle(results[0][0],results[1][0]));
        const auto rightA=RotateVector(results[0][0],{1,0,0});
        const auto rightB=RotateVector(results[0][1],{1,0,0});
        Near(rightA.x,rightB.x,"eyes have different IPD axes");
        Near(rightA.y,rightB.y,"eyes have different IPD axes");
        Near(rightA.z,rightB.z,"eyes have different IPD axes");
    }
    Check(maxSplit<.0001f && maxOrderError<.0001f,"callback order split or alternated the vehicle view");
    std::cout<<"captured_old_split_deg="<<angle(oldMain,liveShared)
        <<" corrected_pair_deg="<<maxSplit<<" callback_order_error_deg="<<maxOrderError<<'\n';
}
void VehicleHeadingIdentity() {
    const float entity[4]={0,0,0,1};
    cvr::camera::VehicleStereoHeading base;
    auto nearYaw=[](float yaw,float expected,const char* message) {
        Near(std::remainder(yaw-expected,2*pi),0,message);
    };
    const float quarterTurn[4]={0,0,std::sin(pi/4),std::cos(pi/4)};
    const float opposite[4]={0,0,1,0};
    const float vertical[4]={std::sin(pi/4),0,0,std::cos(pi/4)};
    const float zero[4]={};
    const float bad[4]={0,0,std::numeric_limits<float>::quiet_NaN(),1};
    const float inf[4]={0,0,0,std::numeric_limits<float>::infinity()};
    base.SetSource(10,20);
    Check(!base.Observe(30,opposite,entity),"VRCAM initialized an unknown MAIN heading");
    Near(base.ForFrame(1,entity,.2f,.5f).yaw,.2f,"bootstrap must use the already adjusted common heading");
    Check(base.Observe(20,quarterTurn,entity),"MAIN publication rejected");
    Near(base.ForFrame(1,entity,.8f,.5f).yaw,.2f,"MAIN arriving between eyes changed the active frame");
    Near(base.ForFrame(2,entity,.8f,.5f).yaw,pi/2+.5f,"next epoch did not adopt MAIN and its offset once");
    Check(base.Observe(20,opposite,entity),"new MAIN heading rejected");
    Near(base.ForFrame(2,entity,.8f,.7f).yaw,pi/2+.5f,"shared epoch rebased only the second eye");
    nearYaw(base.ForFrame(3,entity,.8f,0).yaw,pi,"next epoch retained the previous vehicle heading");
    Check(!base.Observe(20,vertical,entity),"vertical forward invented a yaw");
    Check(!base.Observe(20,zero,entity) && !base.Observe(20,bad,entity) && !base.Observe(20,inf,entity),"invalid native quaternion accepted");
    nearYaw(base.ForFrame(4,entity,.8f,0).yaw,pi,"invalid input discarded the last valid MAIN heading");
    base.SetSource(11,20);
    Near(base.ForFrame(4,entity,.3f,.5f).yaw,.3f,"new player reused the previous player's heading or epoch");
    Check(!base.Observe(20,vertical,entity),"vertical new MAIN acquired an invented heading");
    Near(base.ForFrame(5,entity,.4f,.5f).yaw,.4f,"new player inherited held yaw from old MAIN");
    Check(base.Observe(20,quarterTurn,entity),"replacement player MAIN rejected");
    base.SetSource(11,21);
    Check(!base.Observe(20,opposite,entity),"old MAIN was accepted after component rebind");
    Near(base.ForFrame(5,entity,.6f,.5f).yaw,.6f,"replacement component reused stale heading");
    Check(base.Observe(21,quarterTurn,entity),"new MAIN component rejected");
    Near(base.ForFrame(0,entity,.8f,0).yaw,pi/2,"startup epoch froze the camera");
    Check(base.Observe(21,opposite,entity),"startup heading update rejected");
    nearYaw(base.ForFrame(0,entity,.8f,0).yaw,pi,"unknown epoch latched forever");
    base.SetSource(0,21);
    Check(!base.Observe(21,opposite,entity),"unowned MAIN heading accepted");
    Near(base.ForFrame(6,entity,.7f,.5f).yaw,.7f,"unowned source leaked a previous vehicle heading");
}
void SceneCameraHeading() {
    using namespace cvr::camera;
    for(int tier:{2,3,4,5})Check(UseNativeMainHeading(false,tier,false,false),"scripted scene still uses body yaw");
    for(int tier:{0,1,6})Check(!UseNativeMainHeading(false,tier,false,false),"ordinary gameplay acquired scene base");
    Check(UseNativeMainHeading(true,1,false,false),"vehicle MAIN base disabled");
    Check(!UseNativeMainHeading(false,4,true,false) && !UseNativeMainHeading(false,4,false,true),"scene base overrode device/braindance camera");
    auto qarray=[](XrQuaternionf q) { return std::array<float,4>{q.x,q.y,q.z,q.w}; };
    auto yaw=[](XrQuaternionf q) { return std::atan2(2*(q.z*q.w-q.x*q.y),1-2*(q.x*q.x+q.z*q.z)); };
    // Frozen Tier4 snapshot, PID8136. Native MAIN differs from the player body.
    const XrQuaternionf owner{0,0,.9646251798f,.26362514496f};
    const XrQuaternionf main{-.10268808156f,-.2866577208f,.86849993467f,.39114004374f};
    const XrQuaternionf head{.30956453085f,.20560458302f,-.01862309501f,.92819708586f};
    const float realign=.27175703645f;
    const auto local=MultiplyQuat(ConjugateQuat(owner),main);
    const float oldError=std::abs(std::remainder(yaw(owner)-yaw(main),2*pi));
    Check(oldError>.30f,"fixture no longer exposes scene/body yaw mismatch");
    cvr::camera::VehicleStereoHeading selectors[2];
    const auto ownerValues=qarray(owner),mainValues=qarray(main);
    for(auto& s:selectors) { s.SetSource(10,20);Check(s.Observe(20,mainValues.data(),ownerValues.data()),"native scene MAIN rejected"); }
    float maxError=0;
    for(int frame=0;frame<90;++frame) {
        const float angle=.02f*frame;
        const auto current=MultiplyQuat({0,0,std::sin(angle/2),std::cos(angle/2)},owner);
        const auto native=MultiplyQuat(current,local);const auto currentValues=qarray(current),nativeValues=qarray(native);
        const auto mappedHead=XrQuaternionf{head.x,-head.z,head.y,head.w};const auto headValues=qarray(head);
        const float expected=yaw(native)-realign;
        for(int order=0;order<2;++order)for(int eye=0;eye<2;++eye) {
            const int view=(eye+order)%2;
            if(view==0)selectors[order].Observe(20,nativeValues.data(),currentValues.data());
            else Check(!selectors[order].Observe(30,ownerValues.data(),currentValues.data()),"VRCAM supplied scene heading");
            const auto selected=selectors[order].ForFrame(1+frame/3,currentValues.data(),0,-realign);
            const float error=std::abs(std::remainder(selected.yaw-expected,2*pi));maxError=std::max(maxError,error);
            Near(error,0,"scene composition kept body yaw or split MAIN/VRCAM");
            const auto composed=MultiplyQuat({0,0,std::sin(selected.yaw/2),std::cos(selected.yaw/2)},mappedHead);
            const auto composedValues=qarray(composed);float recovered{};
            Check(CompositionBaseYaw(composedValues.data(),headValues.data(),&recovered),"shared scene composition could not supply its own base");
            Near(std::remainder(recovered-selected.yaw,2*pi),0,"Locate publication disagrees with the writer's scene yaw");
        }
    }
    std::cout<<"captured_scene_body_yaw_error_deg="<<oldError*180/pi<<" corrected_max_error_deg="<<maxError*180/pi<<'\n';
}
void VehicleWorldHeading() {
    using Heading=cvr::camera::VehicleStereoHeading;
    auto yawQuat=[](float yaw) { return XrQuaternionf{0,0,std::sin(yaw*.5f),std::cos(yaw*.5f)}; };
    auto values=[](XrQuaternionf q) { return std::array<float,4>{q.x,q.y,q.z,q.w}; };
    auto yaw=[](XrQuaternionf q) {
        return std::atan2(2*(q.z*q.w-q.x*q.y),1-2*(q.x*q.x+q.z*q.z));
    };
    const auto local=MultiplyQuat(yawQuat(2.1f),{std::sin(.08f),0,0,std::cos(.08f)});
    const auto localQ=values(local);
    const float identity[4]={0,0,0,1};
    Heading headings[2];
    for(auto& h:headings) { h.SetSource(10,20);Check(h.Observe(20,localQ.data(),identity),"initial MAIN pair rejected"); }
    uint64_t previousEpoch=0;
    float oldHeld=0,maxOldLag=0,maxNewLag=0,maxPairError=0;
    int worldRecomposes=0;
    for(int frame=0;frame<180;++frame) {
        // Three moving vehicle frames share one HMD sample. The world yaw must
        // still advance on all three; the eyes must agree in either call order.
        const uint64_t epoch=100+frame/3;
        const float pitch=.05f*std::sin(float(frame)*.07f);
        const auto owner=MultiplyQuat(yawQuat(float(frame)*.025f),{std::sin(pitch*.5f),0,0,std::cos(pitch*.5f)});
        const auto ownerQ=values(owner),mainQ=values(MultiplyQuat(owner,local));
        const float expected=yaw(MultiplyQuat(owner,local));
        if(epoch!=previousEpoch)oldHeld=expected;
        maxOldLag=std::max(maxOldLag,std::abs(std::remainder(oldHeld-expected,2*pi)));
        float result[2][2]{};
        for(int order=0;order<2;++order)for(int call=0;call<2;++call) {
            const bool main=order==call;
            const auto secondary=values(yawQuat(-.6f+float(frame)*.013f));
            Check(headings[order].Observe(main ? 20 : 30,main ? mainQ.data() : secondary.data(),ownerQ.data())==main,
                "secondary heading entered vehicle frame transport");
            const auto chosen=headings[order].ForFrame(epoch,ownerQ.data(),-.9f,0);
            Check(chosen.changed==(call==0),"world update did not recompose once for the camera pair");
            if(order==0 && call==0 && epoch==previousEpoch && chosen.changed)++worldRecomposes;
            result[order][call]=chosen.yaw;
            const float error=std::abs(std::remainder(chosen.yaw-expected,2*pi));
            maxNewLag=std::max(maxNewLag,error);
            Check(error<.000003f,"vehicle yaw held or lagged while HMD sample was reused");
            const auto delta=cvr::camera::ComposeAnchorTranslation({.1f,.2f,.04f},{.02f,.15f,.1f},{},chosen.yaw,0);
            const auto relative=cvr::camera::RotateAnchorYaw(delta,-expected);
            Near(relative.x,.12f,"vehicle turn moved the camera offset in its own frame");
            Near(relative.y,.35f,"vehicle turn delayed camera translation behind orientation");
        }
        maxPairError=std::max(maxPairError,std::abs(std::remainder(result[0][0]-result[1][0],2*pi)));
        Near(result[0][0],result[0][1],"vehicle owner update split MAIN and VRCAM");
        auto oppositeSign=ownerQ;for(auto& v:oppositeSign)v=-v;
        Check(!headings[0].ForFrame(epoch,oppositeSign.data(),0,0).changed,"quaternion sign created a false world frame");
        const auto unavailable=headings[0].ForFrame(epoch,nullptr,-2,0);
        Check(!unavailable.changed,"temporary owner-read failure changed only one eye");
        Near(unavailable.yaw,result[0][0],"missing owner replaced the paired heading with a fallback");
        previousEpoch=epoch;
    }
    Check(worldRecomposes==120 && maxOldLag>.04f && maxPairError<.000003f,"moving-world regression was not exercised");
    std::cout<<"old_world_lag_deg="<<maxOldLag*180/pi<<" new_world_lag_deg="<<maxNewLag*180/pi
        <<" pair_order_error_deg="<<maxPairError*180/pi<<" reused_head_world_updates="<<worldRecomposes<<'\n';
}
void TakeoverWorldHeading() {
    using Heading=cvr::camera::VehicleStereoHeading;
    auto rotation=[](float angle) {return XrQuaternionf{0,0,std::sin(angle*.5f),std::cos(angle*.5f)};};
    auto values=[](XrQuaternionf q) {return std::array<float,4>{q.x,q.y,q.z,q.w};};
    auto yaw=[](XrQuaternionf q) {return std::atan2(2*(q.z*q.w-q.x*q.y),1-2*(q.x*q.x+q.z*q.z));};
    constexpr uintptr_t entity=10849869, lens=40, playerMain=20, vrcam=30;
    const float identity[4]={0,0,0,1};
    const auto local=MultiplyQuat(rotation(-.09f),{std::sin(-.11f),0,0,std::cos(-.11f)});
    const auto localQ=values(local);
    float maxError=0,maxOldLag=0;
    for(float speed:{0.f,.1f,.8f,2.5f}) for(int order=0;order<3;++order) {
        Heading source;source.SetSource(entity,lens);
        Check(source.Observe(lens,localQ.data(),identity),"takeover lens not accepted");
        float oldYaw=0;
        for(int frame=0;frame<360;++frame) {
            const float turn=frame*speed/90.f + .003f*std::sin(frame*.7f);
            const auto owner=MultiplyQuat(rotation(turn),{std::sin(.025f*std::sin(frame*.09f)),0,0,std::cos(.025f*std::sin(frame*.09f))});
            const auto ownerQ=values(owner), nativeQ=values(MultiplyQuat(owner,local));
            const float expected=yaw(MultiplyQuat(owner,local));
            // The HMD is fixed for four moving world frames, including small
            // alternating steering corrections. Both player cameras have a
            // different attachment and must never become this lens's source.
            const uint64_t epoch=1+frame/4;
            if(frame%4==0)oldYaw=expected;
            maxOldLag=std::max(maxOldLag,std::abs(std::remainder(oldYaw-expected,2*pi)));
            float first=0;
            for(int call=0;call<3;++call) {
                const uintptr_t camera=call==order ? lens : (call<order ? playerMain : vrcam);
                Check(source.Observe(camera,nativeQ.data(),ownerQ.data())==(camera==lens),"player attachment replaced takeover aim");
                const auto selected=source.ForFrame(epoch,ownerQ.data(),-2.f,0);
                Check(selected.changed==(call==0),"takeover world turn was held or applied per eye");
                if(call==0)first=selected.yaw;else Near(selected.yaw,first,"takeover callbacks disagree about yaw");
                maxError=std::max(maxError,std::abs(std::remainder(selected.yaw-expected,2*pi)));
            }
            const auto missing=source.ForFrame(epoch,nullptr,2.f,0);
            Check(!missing.changed,"missing takeover owner changed a shared frame");Near(missing.yaw,first,"missing owner lost takeover yaw");
        }
        source.SetSource(entity+1,lens+1);
        Near(source.ForFrame(90,identity,1.2f,0).yaw,1.2f,"switching camera retained old lens yaw");
        Check(!source.Observe(lens,localQ.data(),identity),"old lens accepted after switching device");
        source.SetSource(0,0);
        Near(source.ForFrame(90,identity,-.8f,0).yaw,-.8f,"takeover exit retained world heading");
    }
    Check(maxError<.00001f && maxOldLag>.08f,"takeover yaw regression was not reproduced and corrected");
    std::cout<<"takeover_old_lag_deg="<<maxOldLag*180/pi<<" transported_error_deg="<<maxError*180/pi<<'\n';
}
void LadderTurnInput() {
    using namespace cvr::input;
    Check(NativeOwnsTurn(false,0,true),"ladder still allows snap turn");
    Check(!NativeOwnsTurn(false,0,false),"leaving ladder disabled chosen on-foot turn mode");
    Check(NativeOwnsTurn(false,2,false) && NativeOwnsTurn(true,0,false),"vehicle turn policy regressed");
    for(float direction:{-1.0f,1.0f}) {
        int armed=0;
        Check(RouteTurn(direction,true,armed,30,.9f,.5f).snapDegrees!=0,"control snap did not fire");
        for(float axis:{direction*.4f,direction*.8f,direction}) {
            const auto turn=RouteTurn(axis,!NativeOwnsTurn(false,0,true),armed,30,.9f,.5f);
            Near(turn.axis,axis,"native ladder turning lost analog axis");
            Check(turn.clearPending && turn.snapDegrees==0,"ladder retained a queued/new snap");
        }
        Check(RouteTurn(direction,!NativeOwnsTurn(false,0,false),armed,30,.9f,.5f).snapDegrees==0,"held stick snapped immediately on ladder exit");
        RouteTurn(0,true,armed,30,.9f,.5f);
        Check(RouteTurn(direction,true,armed,30,.9f,.5f).snapDegrees!=0,"release after ladder exit did not rearm snap");
    }
}
void VehicleTurnInput() {
    using namespace cvr::input;
    for(int state:{1,2,3,4,5,6,7})Check(VehicleOwnsTurn(true,state),"mounted state still allows on-foot snap");
    Check(VehicleOwnsTurn(false,2),"window Combat before mounted refresh permits a snap");
    Check(!VehicleOwnsTurn(false,0),"ordinary on-foot snap was disabled");
    int armed=0;
    auto turn=RouteTurn(1,true,armed,30,.9f,.5f);
    Near(turn.snapDegrees,-30,"on-foot right snap changed direction or angle");
    Near(RouteTurn(1,true,armed,30,.9f,.5f).snapDegrees,0,"held stick repeats on-foot snap");
    float pending=30; // PID10700 had this unconsumed delta while mounted.
    for(float axis:{1.0f,.7f,0.0f,-.3f,-1.0f}) {
        turn=RouteTurn(axis,!VehicleOwnsTurn(true,2),armed,30,.9f,.5f);
        Near(turn.axis,axis,"Combat did not pass the continuous turn axis unchanged");
        Near(turn.snapDegrees,0,"Combat emitted an instant yaw delta");
        if(turn.clearPending)pending=0;
        Near(pending,0,"queued snap survived native vehicle look");
    }
    Near(RouteTurn(-1,true,armed,30,.9f,.5f).snapDegrees,0,"leaving the vehicle fired a held stick");
    RouteTurn(0,true,armed,30,.9f,.5f);
    Near(RouteTurn(-1,true,armed,30,.9f,.5f).snapDegrees,30,"snap did not rearm after vehicle exit and release");
    turn=RouteTurn(.4f,false,armed,30,.9f,.5f);
    Near(turn.axis,.4f,"ordinary smooth look was modified");
    Check(turn.clearPending,"turn-mode change retained a pending snap");
}
void VehicleReloadInput() {
    using namespace cvr::input;
    const auto combat=VehicleButtonPolicy(true,2),seat=VehicleButtonPolicy(true,3);
    Check((uint16_t(0x4000)&~combat.owned)==0x4000,"Combat swallowed vanilla X reload");
    Check(!combat.exitOnX && !combat.confirmOnA,"Combat remapped reload to exit or A to reload");
    Check((combat.owned&0x2000)!=0,"Combat exposed accidental raw B exit");
    Check(VehicleButtonPolicy(false,2).owned==combat.owned,"late mounted flag broke Combat reload");
    Check((uint16_t(0x4000)&~seat.owned)==0 && seat.exitOnX && seat.confirmOnA,"ordinary seat lost its mappings");
    Check(VehicleButtonPolicy(true,1).owned==seat.owned,"driving mappings changed");
    Check(VehicleButtonPolicy(false,0).owned==0,"on-foot face buttons were claimed");
    VehicleExitHold hold;
    Check(!hold.Update(seat.exitOnX,true,100,400),"seat exits immediately on X press");
    Check(!hold.Update(combat.exitOnX,true,600,400),"entering Combat emitted an old exit hold");
    Check(!hold.Update(combat.exitOnX,true,1600,400),"holding reload exited window Combat");
    Check(!hold.Update(seat.exitOnX,true,1700,400),"Combat reload hold carried into ordinary seat exit");
    Check(hold.Update(seat.exitOnX,true,2100,400),"ordinary seat X hold stopped working");
    Check(!hold.Update(true,false,2200,400),"X release left exit held");
}
// The body follows the head CONTINUOUSLY: a slow head turn is a slow body turn, every frame, never a staircase of
// catch-ups; the turn rate never jumps; after the turn the body comes round under the head and never past it.
void FreeLookZone() {
    const float radius=10*pi/180,dt=1.f/90;
    {
        BodyFollowZone follow;
        Near(follow.Step(radius*.9f,radius,dt),0,"small glance turned body");
        Check(!follow.Following(),"a glance inside the cone started a turn");
    }
    {   // a slow head turn, 30 deg/s for three seconds, then the head stops
        BodyFollowZone follow;
        float head=0,body=0,prev=0,jerk=0,lag=0;
        int still=0,first=-1;
        for(int i=0;i<270;++i) {
            head+=30*pi/180*dt;
            const float step=follow.Step(head-body,radius,dt);
            body+=step;
            if(first<0 && step!=0)first=i;
            if(first>=0 && i>first+45 && std::abs(step)<1e-7f)++still;
            if(first>=0)jerk=std::max(jerk,std::abs(step-prev));
            prev=step;lag=std::max(lag,head-body);
        }
        Check(first>=0,"slow head turn never turned the body");
        Check(still==0,"slow head turn followed in steps (the staircase)");
        Near(prev/dt*180/pi,30,"body did not turn at the head's speed",1.5f);
        Check(lag<radius+2*pi/180,"body fell far behind the cone during a slow turn");
        Check(jerk*180/pi<.05f,"body turn rate jumped between frames");
        float closest=head-body;
        for(int i=0;i<360;++i) {
            body+=follow.Step(head-body,radius,dt);
            Check(head-body>=-1e-6f,"body turned past the head");
            closest=std::min(closest,head-body);
        }
        Check(head-body<1.6f*pi/180,"body did not come round under the head after the turn");
        Near(follow.Step(.5f*pi/180,radius,dt),0,"half a degree of tremor moved the body after the turn");
    }
    {   // a fast head turn (90 degrees in a sixth of a second): fast but capped, and never past the head
        BodyFollowZone follow;
        float head=0,body=0,top=0;
        for(int i=0;i<360;++i) {
            if(i<15)head-=6*pi/180;
            const float step=follow.Step(head-body,radius,dt);
            body+=step;top=std::max(top,std::abs(step)/dt);
            Check(body-head>=-1e-6f,"fast turn carried the body past the head");
        }
        Check(top<=BodyFollowZone::kMaxRate*1.0001f,"fast turn exceeded the rate cap");
        Check(body-head<1.6f*pi/180,"fast turn did not settle under the head");
    }
    BodyFollowZone follow;
    follow.Step(30*pi/180,radius,dt);
    follow.Reset();
    Near(follow.Step(radius*.5f,radius,dt),0,"reset retained a pending turn");
}
void BodyBendTracking() {
    auto observed=[](float forward,float drop,float pitch) {
        cvr::body::BendTracker t;
        const XrQuaternionf q{-std::sin(pitch*.5f),0,0,std::cos(pitch*.5f)};
        const auto optical=RotateVector(q,{0,.08f,-.15f});
        return t.Update({optical.x,optical.y-.08f-drop,optical.z+.15f-forward},{q.x,q.y,q.z,q.w},1);
    };
    Near(observed(.045f,.006f,20*pi/180).angle,0,"small torso movement still crosses the bend threshold");
    Near(observed(.22f,.04f,35*pi/180).angle,0,"lean below the doubled threshold bends the body");
    Check(observed(.30f,.09f,35*pi/180).angle>10*pi/180,"raised threshold suppresses an intentional bend");
    cvr::body::BendTracker tracker;
    auto head=[](float angle,float walk=0.0f) {
        const XrQuaternionf q{-std::sin(angle*.5f),0,0,std::cos(angle*.5f)};
        const auto optical=RotateVector(q,{0,.08f,-.15f});
        return std::pair{std::array<float,3>{optical.x,optical.y-.08f-cvr::body::BendReach*(1-std::cos(angle)),
            optical.z+.15f-cvr::body::BendReach*std::sin(angle)-walk},std::array<float,4>{q.x,q.y,q.z,q.w}};
    };
    // Live PID30744 exposed an onset/return defect: fading the classified
    // hinge together with the visible torso sent up to 8.7cm into CCT. Exercise
    // the complete tracker -> movement hand-off, not just its settled angle.
    cvr::body::BendTracker movementTracker;Movement movement;Vec2 body{};float maxTravel=0;
    for(int i=0;i<=180;++i) {
        const float angle=.01f*float(i<=90 ? i:180-i);
        const auto [p,q]=head(angle);const auto bend=movementTracker.Update(p,q,1);
        const auto sample=Pose(i+1,p[0]-bend.right,-p[2]-bend.forward);
        const auto step=movement.Begin(sample,sample.stampUs,.02f,1,0,true,.05f);
        movement.Complete(step);body=body+step.world;
        maxTravel=std::max(maxTravel,std::hypot(body.x,body.y));
        Near(body+Vec2{p[0],-p[2]}-movement.Consumed(1),{p[0],-p[2]},"bend accounting altered optical camera motion");
    }
    std::cout<<"bend_cct_peak_mm="<<maxTravel*1000<<'\n';
    Check(maxTravel<.0001f,"pure bend onset/return made CCT walk");
    cvr::body::BendSample pose{};
    for(int i=0;i<=90;++i) { const auto [p,q]=head(float(i)*.01f);pose=tracker.Update(p,q,1); }
    Near(pose.angle,.9f,"physical hinge not recovered from neck arc",.001f);
    Near(pose.forward,cvr::body::BendReach*std::sin(.9f),"hinge does not reserve head translation",.001f);
    Near(LookDownFreeLookCone(50,10,30,pose.angle),60,"bending did not widen cone to sixty degrees");
    const float held=pose.forward;
    for(int i=0;i<=100;++i) {
        const auto [p,q]=head(.9f,float(i)*.01f);pose=tracker.Update(p,q,1);
        Near(pose.forward,held,"walking while bent grows the hinge");
    }
    for(int i=90;i>=0;--i) { const auto [p,q]=head(float(i)*.01f,1);pose=tracker.Update(p,q,1); }
    Near(pose.angle,0,"standing up retained a bend",.001f);
    Near(pose.forward,0,"standing up retained consumed hinge displacement",.001f);
    Near(LookDownFreeLookCone(0,10,30,pose.angle),10,"standing up retained wide cone");
    for(int mode=0;mode<3;++mode) {
        cvr::body::BendTracker onlyHead;
        for(int i=0;i<=90;++i) {
            const float a=float(i)*.01f;
            const XrQuaternionf q{-std::sin(a*.5f),0,0,std::cos(a*.5f)};
            const auto opt=RotateVector(q,{0,.08f,-.15f});
            const std::array<float,3> position=mode==0 ? std::array<float,3>{0,0,0}:
                mode==1 ? std::array<float,3>{opt.x,opt.y-.08f,opt.z+.15f}:std::array<float,3>{0,-a*.5f,0};
            const std::array<float,4> rotation=mode==2 ? std::array<float,4>{0,0,0,1}:std::array<float,4>{q.x,q.y,q.z,q.w};
            const auto b=onlyHead.Update(position,rotation,1);
            Near(b.angle,0,"head rotation or vertical crouch created body bend");
        }
    }
    const auto [p,q]=head(.8f);tracker.Update(p,q,1);
    pose=tracker.Update({0,0,0},{0,0,0,1},2);
    Near(pose.angle,0,"recenter retained old hinge");
    for(float angle:{.0872664f,.0872665f,.4363322f,.4363324f}) {
        const float cone=LookDownFreeLookCone(45,10,30,angle);
        Check(cone>=30 && cone<=60,"bending cone outside requested range");
    }
    BodyFollowZone follow;
    Check(follow.Step(40*pi/180,10*pi/180,.01f)>0,"turn setup failed");
    Near(follow.Step(40*pi/180,60*pi/180,.01f),0,"bending did not release existing body turn");
}
void FloorReachTracking() {
    cvr::body::BendTracker tracker;Movement movement;Vec2 body{};
    cvr::body::BendSample bend{};uint64_t sequence=0;
    for(int i=0;i<=180;++i) {
        const float t=float(i<=90 ? i:180-i)/90;
        const float angle=65*pi/180*t;
        const XrQuaternionf q{-std::sin(angle*.5f),0,0,std::cos(angle*.5f)};
        const auto opt=RotateVector(q,{0,.08f,-.15f});
        // A low reach with hips retreating: substantial lowering but only
        // 15cm forward neck motion. Arc-only recognition under-bent this pose.
        const std::array<float,3> p{opt.x,-.8f*t+opt.y-.08f,-.15f*t+opt.z+.15f};
        bend=tracker.Update(p,{q.x,q.y,q.z,q.w},1);
        const auto sample=Pose(++sequence,p[0]-bend.right,-p[2]-bend.forward);
        const auto step=movement.Begin(sample,sample.stampUs,.02f,1,0,true,.05f);
        movement.Complete(step);body=body+step.world;
        Check(std::hypot(body.x,body.y)<.0001f,"floor-reach cue manufactured a walking request");
        if(i==90) {
            Check(bend.angle>60*pi/180,"low hand-reaching posture remained a shallow bend");
            Near(LookDownFreeLookCone(65,20,45,bend.angle),60,"deep floor reach lost sixty-degree cone");
            Check(bend.forward<=.151f,"inferred torso angle replaced measured planar movement");
        }
    }
    Near(bend.angle,0,"floor reach did not release on standing up");
}
void LookDownCone() {
    auto orientation=[](float yaw,float pitch,float roll) {
        const float half=pi/360;
        const XrQuaternionf y{0,std::sin(yaw*half),0,std::cos(yaw*half)};
        const XrQuaternionf p{std::sin(pitch*half),0,0,std::cos(pitch*half)};
        const XrQuaternionf r{0,0,std::sin(roll*half),std::cos(roll*half)};
        return MultiplyQuat(MultiplyQuat(y,p),r);
    };
    for(float yaw:{-135.0f,0.0f,80.0f})for(float roll:{-25.0f,0.0f,40.0f}) {
        for(float pitch:{-70.0f,-30.0f,-20.0f,-5.0f,0.0f,35.0f}) {
            const auto q=orientation(yaw,pitch,roll);
            const float down=HeadDownDegrees(q.x,q.y,q.z,q.w);
            Near(down,-pitch,"downward pitch depends on yaw/roll",.0001f);
            Near(HeadDownDegrees(-q.x,-q.y,-q.z,-q.w),down,"quaternion sign changed down cone",.0001f);
            const float cone=LookDownFreeLookCone(down,10,30);
            if(pitch<=-30)Near(cone,30,"chest reach did not receive thirty-degree cone");
            else if(pitch==-20)Near(cone,20,"look-down cone transition is not gradual",.0001f);
            else if(pitch>=-5)Near(cone,10,"normal/upward gaze widened the cone");
        }
    }
    Near(LookDownFreeLookCone(45,45,30),45,"looking down narrowed normal cone");
    Near(LookDownFreeLookCone(45,10,55),55,"look-down slider value ignored");
    Near(LookDownFreeLookCone(45,10,10),10,"equal cones did not disable widening");
    Check(std::abs(LookDownFreeLookCone(10.001f,10,30)-LookDownFreeLookCone(9.999f,10,30))<.0001f,"cone jumps at tilt threshold");
    BodyFollowZone follow;
    Check(follow.Step(20*pi/180,10*pi/180,.01f)>0 && follow.Following(),"test did not start a body turn");
    Near(follow.Step(20*pi/180,30*pi/180,.01f),0,"old body turn continues while head is inside widened cone");
    Check(!follow.Following(),"look-down cone did not release old turn");
    Near(follow.Step(29*pi/180,30*pi/180,.01f),0,"body moved while reaching across chest belt");
    Check(follow.Step(20*pi/180,10*pi/180,.01f)>0,"normal body follow failed to resume after looking up");
}
void MovementFreeZone() {
    Movement m;uint64_t seq=0;Vec2 entity{};
    auto tick=[&](float x,float y=0.0f) {
        auto pose=Pose(++seq,x,y);const auto step=m.Begin(pose,pose.stampUs,.01f,1,0,true,.10f);
        entity=entity+step.world;m.Complete(step);
        Near(entity+pose.head-m.Consumed(1),pose.head,"body free zone delayed or jumped camera");return step;
    };
    tick(0);Near(tick(.01f).world,{},"one centimetre moved body");
    Near(tick(.09f).world,{},"movement inside free zone moved body");
    const auto first=tick(.11f);Check(first.world.x>0 && first.world.x<.11f,"roomscale catch-up is not smooth");
    for(int i=0;i<60;++i)tick(.11f);
    Near(entity,{.11f,0},"body never reached new centre");
    Near(tick(.12f).world,{},"one centimetre moved body after catch-up");
    Near(tick(.12f,.01f).world,{},"diagonal small lean moved body");
    // A follow the controller carried out is not requested again.
    Movement carried;seq=0;
    for(int i=0;i<60;++i) {
        auto pose=Pose(++seq,i ? .2f:0);auto step=carried.Begin(pose,pose.stampUs,.01f,1,0,true,.10f);carried.Complete(step);
    }
    Near(carried.Consumed(1),{.2f,0},"carried follow leaves movement debt");
    auto pose=Pose(++seq,.2f);Near(carried.Begin(pose,pose.stampUs,.01f,1,0,true,.10f).world,{},"stationary head replays a carried step");
    Movement held;auto initial=Pose(1,0);held.Complete(held.Begin(initial,initial.stampUs,.01f,1,0,true,.10f));
    auto same=Pose(2,.13f);
    for(int i=0;i<40;++i)held.Complete(held.Begin(same,same.stampUs+i*5000,.005f,1,0,true,.10f));
    for(int i=0;i<30;++i) { same=Pose(3+i,.13f,0,220000+i*10000);held.Complete(held.Begin(same,same.stampUs,.01f,1,0,true,.10f)); }
    Near(held.Consumed(1),{.13f,0},"body follow stalls between repeated head packets");
}
void MovementSoftOnset() {
    Movement m;uint64_t sequence=0;Vec2 travelled{};float peak=0;
    auto tick=[&](float x) {
        auto pose=Pose(++sequence,x,0,sequence*20000);
        auto step=m.Begin(pose,pose.stampUs,.02f,1,0,true,.05f);m.Complete(step);
        travelled=travelled+step.world;
        Near(travelled+pose.head-m.Consumed(1),pose.head,"translation damping changed camera tracking");
        peak=std::max(peak,std::abs(step.world.x));return step.world.x;
    };
    tick(0);Near(tick(.04f),0,"small lean left free zone");
    const float first=tick(.052f),second=tick(.052f);
    Check(first>0 && first<.004f,"free-zone crossing starts with a visible body jump");
    Check(second>first,"body did not accelerate from rest");
    for(int i=0;i<40;++i)tick(.052f);
    Near(travelled,{.052f,0},"smooth translation failed to settle");
    Check(peak<.008f,"translation still contains a large single-step catch-up");
    Near(tick(.06f),0,"translation free zone did not recover after settling");
    for(int i=0;i<50;++i) {
        tick(-.01f);Check(travelled.x>=-.01001f,"body overshot reversed target");
    }
    Near(travelled,{-.01f,0},"reversed translation did not settle");
    std::cout<<"first_step_mm="<<first*1000<<" peak_step_mm="<<peak*1000<<'\n';
}

void Cadence() {
    // Multiple render rates, physics slower/faster than XR, skipped samples.
    for (int xrHz : {72, 90, 120}) for (int gameHz : {40, 60, 90, 144}) {
        Movement m;
        Vec2 sum{};
        const int seconds = 4;
        for (int tick = 0; tick <= gameHz*seconds; ++tick) {
            const int xrTick = tick*xrHz/gameHz;
            const float x = static_cast<float>(xrTick) / xrHz * 0.35f;
            Sample p = Pose(xrTick+1, x, -x*0.5f,
                            1000000 + static_cast<uint64_t>(xrTick)*1000000/xrHz);
            const auto step = m.Begin(p, 1000000 + static_cast<uint64_t>(tick)*1000000/gameHz,
                                      1.0f/gameHz, 1, 0, true);
            sum = sum + step.world; m.Complete(step);
            m.Complete(step); // duplicate completion must never double consumption
        }
        Near(sum, {1.4f, -0.7f}, "cadence-independent distance");
        Near(m.Consumed(1), sum, "consumption once per physical sample");
    }
}

void Camera() {
    for (float yaw : {0.0f, pi/2, pi, -2.4f}) for (float scale : {0.5f, 1.0f, 2.0f}) {
        Movement m; Vec2 entity{}, nativeTotal{};
        Tick(m, Pose(1, 0, 0), yaw, true, scale);
        for (int i = 1; i <= 180; ++i) {
            const Vec2 head{0.002f*i, -0.001f*i};
            const Vec2 game{0.003f, 0.001f}; // simultaneous WASD
            auto step = Tick(m, Pose(i+1, head.x, head.y), yaw, true, scale);
            entity = entity + game + step.world; nativeTotal = nativeTotal + game;
            // Independent analytic expected view: game travel + physical head travel.
            const auto view = entity + Rotate((head - m.Consumed(1))*scale, yaw);
            Near(view, nativeTotal + Rotate(head*scale, yaw), "camera must not double roomscale");
            // A newer render pose is only a residual, not another physics move.
            const Vec2 fresh = head + Vec2{0.0005f, 0.001f};
            Near(entity + Rotate((fresh - m.Consumed(1))*scale, yaw),
                 nativeTotal + Rotate(fresh*scale, yaw), "render-time head residual");
        }
    }
}

void Collision() {
    constexpr float lead = 0.3f;
    const Movement::LeadLimit limit{{}, lead};
    {
        // Wall accepts 2 cm of the requested 10 cm: the view keeps the headset's
        // 8 cm lead over the capsule instead of being held on it.
        Movement m; Tick(m, Pose(1, 0));
        auto p = Pose(2, 0.1f);
        const auto step = m.Begin(p, p.stampUs, .01f, 1, 0, true);
        Near(step.world, {0.1f, 0}, "physics receives whole displacement, not an eased target");
        m.Complete(step, {0.02f, 0}, limit);
        const Vec2 entity{0.02f, 0};
        Near(entity + (Vec2{0.1f, 0} - m.Consumed(1)), {0.1f, 0}, "wall held the view on the capsule");
        Near(m.Pending(), {0.08f, 0}, "refused distance is not the view's lead");
    }
    // A wall 0 m in front of the capsule, the follow's free zone 10 cm, the head walked in.
    for (float yaw : {0.0f, 1.1f}) for (float scale : {1.0f, 1.6f}) {
        Movement m; Vec2 entity{}; uint64_t seq = 0; float peak = 0;
        auto tick = [&](Vec2 head, bool wall) {
            auto p = Pose(++seq, head.x, head.y);
            const auto step = m.Begin(p, p.stampUs, .011f, scale, yaw, true, .10f);
            // The wall's normal is the tracking +x axis, in world axes.
            const Vec2 normal = Rotate({1, 0}, yaw);
            Vec2 accepted = step.world;
            const float into = Dot(accepted, normal);
            if (wall && into > 0) accepted = accepted - normal * into;
            entity = entity + accepted;
            m.Complete(step, accepted, limit);
            const Vec2 view = entity + Rotate((head - m.Consumed(1)) * scale, yaw);
            peak = std::max(peak, std::sqrt(Dot(step.world, step.world)));
            return view;
        };
        tick({}, true);
        Vec2 view{};
        for (int i = 1; i <= 15; ++i) view = tick({0.01f * i, 0}, true);
        Near(view, Rotate({0.15f * scale, 0}, yaw), "view did not lead the blocked capsule", .0002f);
        Near(entity, {}, "wall let the capsule through", .0002f);
        for (int i = 0; i < 200; ++i) view = tick({0.15f, 0}, true);
        Check(peak < .05f * scale, "a stationary headset gained speed into the wall");
        Near(view, Rotate({0.15f * scale, 0}, yaw), "a stationary headset drifted at the wall", .0002f);
        // Walked on past the cap: the view stops at the cap, no debt.
        for (int i = 16; i <= 60; ++i) view = tick({0.01f * i, 0}, true);
        Near(std::sqrt(Dot(view, view)), lead, "view went past the capsule's free footprint", .0002f);
        // Stepping back moves the view at once.
        const Vec2 before = view;
        view = tick({0.55f, 0}, true);
        Near(view - before, Rotate({-0.05f * scale, 0}, yaw), "step away from wall responds immediately", .0002f);
        // Along the wall the body follows smoothly; into it, it stays.
        peak = 0;
        for (int i = 1; i <= 30; ++i) view = tick({0.55f, 0.01f * i}, true);
        for (int i = 0; i < 100; ++i) view = tick({0.55f, 0.3f}, true);
        Check(peak < .05f * scale, "the body was thrown along the wall");
        Near(Dot(entity, Rotate({0, 1}, yaw)), 0.3f * scale, "body did not follow along the wall", .002f);
        // The wall gone, the body closes up under the head; the view stays put.
        const Vec2 held = view;
        for (int i = 0; i < 200; ++i) view = tick({0.55f, 0.3f}, false);
        Near(view, held, "the view moved when the capsule caught up", .0002f);
        Near(m.Pending(), {}, "the capsule did not close up under the head", .0002f);
    }
    {
        // A suspension keeps the lead for the body to follow; it is not baked into the view.
        Movement m; Tick(m, Pose(1, 0));
        auto p = Pose(2, 0.2f);
        auto step = m.Begin(p, p.stampUs, .01f, 1, 0, true);
        m.Complete(step, {}, limit);
        m.Suspend();
        Near(m.Pending(), {0.2f, 0}, "suspension dropped the head's lead");
        Tick(m, Pose(3, 0.2f));
        Near(Tick(m, Pose(4, 0.2f)).world, {0.2f, 0}, "body did not follow the kept lead");
    }
    {
        // The eye already sits 13 cm ahead of the capsule's axis: the lead toward the
        // wall ends 13 cm sooner, and an eye placed past the limit without any lead
        // (a recenter offset) is not pulled back by it.
        Movement m; Tick(m, Pose(1, 0));
        const Movement::LeadLimit ahead{{0.13f, 0}, lead};
        auto p = Pose(2, 0.4f);
        auto step = m.Begin(p, p.stampUs, .01f, 1, 0, true);
        m.Complete(step, {}, ahead);
        Near(Vec2{0.4f, 0} - m.Consumed(1), {0.17f, 0}, "eye base ignored by the lead limit");
        Movement far; Tick(far, Pose(1, 0));
        const Movement::LeadLimit beyond{{0.5f, 0}, lead};
        p = Pose(2, 0.01f);
        step = far.Begin(p, p.stampUs, .01f, 1, 0, true);
        far.Complete(step, {}, beyond);
        Near(Vec2{0.01f, 0} - far.Consumed(1), {}, "the limit pulled a view that already sat past it", .0001f);
        p = Pose(3, -0.1f);
        step = far.Begin(p, p.stampUs, .01f, 1, 0, true);
        far.Complete(step, step.world, beyond);
        Near(Vec2{-0.1f, 0} - far.Consumed(1), {}, "a step back toward the capsule was refused");
    }
    // The 0.25 m capsule as shipped: the eye may lead its axis by 0.20 m and already sits 0.13 m ahead of it, the
    // follow's free zone 10 cm. In the open a walk straight ahead is walked; the lead limit is no wall (2026-11-04:
    // the 7 cm left of the limit never reached the free zone, the step went into the view and the body stood).
    const Movement::LeadLimit shipped{{0.13f, 0}, 0.20f};
    {
        Movement m; Vec2 entity{}; uint64_t seq = 0;
        for (int i = 0; i <= 300; ++i) {
            auto p = Pose(++seq, std::min(0.005f * i, 1.0f));
            const auto step = m.Begin(p, p.stampUs, .011f, 1, 0, true, .10f);
            entity = entity + step.world;
            m.Complete(step, step.world, shipped);
        }
        Near(entity, {1.0f, 0}, "an open walk straight ahead was not walked", .002f);
        Near(m.Consumed(1), {1.0f, 0}, "an open walk was consumed into the view", .002f);
    }
    // ...and walked into a wall with the follow's lag on it, the view stops where it is and eases back inside the
    // limit; it is not thrown back.
    {
        Movement m; Vec2 entity{}; uint64_t seq = 0;
        float last = -1, worstBack = 0, eye = 0;
        for (int i = 0; i <= 400; ++i) {
            const float head = std::min(0.005f * i, 1.2f);
            auto p = Pose(++seq, head);
            const auto step = m.Begin(p, p.stampUs, .011f, 1, 0, true, .10f);
            Vec2 accepted = step.world;
            if (entity.x + accepted.x > 0.5f) accepted.x = std::max(0.0f, 0.5f - entity.x);
            entity = entity + accepted;
            m.Complete(step, accepted, shipped);
            eye = entity.x + 0.13f + (head - m.Consumed(1).x);
            if (last >= 0) worstBack = std::max(worstBack, last - eye);
            last = eye;
        }
        Check(worstBack <= Movement::kLeadEaseBack * .011f + .0002f, "the view was thrown back at the wall");
        Check(eye <= 0.5f + 0.20f + .001f, "the view stayed past the capsule's footprint at the wall");
        Near(entity, {0.5f, 0}, "the wall let the capsule through", .0002f);
    }
    // A loss of a hundredth of a millimetre (the solver's rounding) withholds that much, not the step.
    Near(AcceptedDisplacement({}, {0.0096f, 0}, {0.00955f, 0}), {0.00955f, 0}, "rounding withheld the step");
    Near(AcceptedDisplacement({}, {0.0096f, 0.001f}, {0.0096f, 0.00099f}), {0.0096f, 0.00099f},
         "a sideways rounding withheld the forward step");
}

void Feedback() {
    // A physical step must not become the next solver's previous velocity.
    const Vec2 physical{0.04f, 0.015f}; constexpr float dt = 0.02f;
    Near(PhysicalVelocity({}, physical, physical*(1/dt), dt), physical*(1/dt), "physical-only free velocity");
    Near(PhysicalVelocity({}, physical, {}, dt), {}, "blocked physical-only feedback");
    const Vec2 native{0.06f, 0.01f};
    const auto full = (native + physical)*(1/dt);
    Near(full - PhysicalVelocity(native, physical, full, dt), native*(1/dt), "WASD velocity retained");
    // Wall normal X: native and physical Y slide, their blocked X stays zero.
    const Vec2 slide{0, (native.y + physical.y)/dt};
    Near(slide - PhysicalVelocity(native, physical, slide, dt), {0, native.y/dt}, "wall-slide attribution");
    // Exactly opposing WASD/physical motion: no artificial momentum next frame.
    Near(Vec2{} - PhysicalVelocity(native, native*(-1), {}, dt), native*(1/dt), "opposing inputs");
    Near(PhysicalVelocity(native, {}, native*(1/dt), dt), {}, "native-only velocity untouched");

    Vec2 velocity{}, feedback{}, position{};
    for (int i = 0; i < 100; ++i) {
        // Mock the native friction/velocity-feedback loop, not just the arithmetic call.
        const Vec2 requestedNative = (velocity - feedback) * (dt*0.8f);
        const Vec2 room = i == 0 ? physical : Vec2{};
        const Vec2 applied = requestedNative + room;
        position = position + applied; velocity = applied*(1/dt);
        feedback = PhysicalVelocity(requestedNative, room, velocity, dt);
    }
    Near(position, physical, "a stopped headset never leaves an inertial tail");
    Near(velocity, {}, "velocity stops after physical step");
}

void Transitions() {
    Movement m; Tick(m, Pose(1, 0)); Tick(m, Pose(2, 0.1f));
    Near(Tick(m, Pose(3, 0.3f), 0, false).world, {}, "scripted movement suspended");
    Near(Tick(m, Pose(4, 0.4f)).world, {}, "resume seeds instead of replaying inactive movement");
    Near(Tick(m, Pose(5, 0.42f)).world, {0.02f, 0}, "resume subsequent live step");
    Near(m.Consumed(1), {0.12f, 0}, "toggle preserves existing camera correction");
    Near(Tick(m, Pose(6, 0, 0, 60000, 2)).world, {}, "recenter does not move capsule");
    Near(m.Consumed(1), {}, "old origin correction is unusable after recenter");
    Near(m.Consumed(2), {}, "new origin starts zero");
    Near(Tick(m, Pose(7, 0.02f, 0, 70000, 2)).world, {0.02f, 0}, "new origin moves normally");
    Near(Tick(m, Pose(8, 1, 0, 1000000, 2)).world, {}, "long paused interval never becomes movement");
    Near(Tick(m, Pose(9, 1.01f, 0, 1010000, 2)).world, {0.01f, 0}, "after pause");
    m.Reset();
    Near(Tick(m, Pose(10, 5)).world, {}, "new player establishes fresh baseline");
}

void Invalid() {
    Movement m; Tick(m, Pose(1, 0)); Tick(m, Pose(2, 0.1f));
    auto p = Pose(3, 0.2f); p.valid = false;
    Near(Tick(m, p).world, {}, "tracking loss");
    Near(Tick(m, Pose(4, 4.0f)).world, {}, "tracking recovery never replays missing motion");
    const auto old = m.Consumed(1);
    auto jump = Tick(m, Pose(5, 8.0f));
    Check(jump.reason == Reason::Discontinuity, "discontinuity detected");
    Near(jump.world, {}, "no capsule teleport on tracking jump");
    Near(Vec2{8,0} - m.Consumed(1), Vec2{4,0} - old, "no camera teleport on tracking jump");
    p = Pose(6, 8.1f); p.head.x = std::numeric_limits<float>::quiet_NaN();
    Near(Tick(m, p).world, {}, "NaN input rejected");
    p = Pose(7, 8.1f);
    Near(m.Begin(p, p.stampUs + Movement::MaxAgeUs + 1, .01f, 1, 0, true).world, {}, "stale input rejected");
    Near(Tick(m, Pose(8, 8.1f, 0, 330000)).world, {}, "stale recovery seeded");
    // An older pose arriving at a NEWER physics time, not a backwards QPC clock.
    const auto oldPose = m.Begin(Pose(7, 0, 0, 320000), 335000, .01f, 1, 0, true);
    Near(oldPose.world, {}, "out-of-order input cannot rewind baseline");
    m.Complete(oldPose);
    Near(Tick(m, Pose(9, 8.12f, 0, 340000)).world, {.02f, 0}, "valid delta after out-of-order sample");
    Near(Tick(m, Pose(10, 8.2f, 0, 350000), 0, true, 1, 0).world, {}, "zero timestep rejected");
}

void Yaw() {
    for (float yaw : {-2.8f, -pi/4, 0.0f, pi/4, 2.8f}) for (float pitch : {-1.3f, 0.0f, 1.3f}) {
        // Quaternion Y(yaw)*X(pitch), independently constructed.
        const float cy = std::cos(yaw/2), sy = std::sin(yaw/2);
        const float cp = std::cos(pitch/2), sp = std::sin(pitch/2);
        float heading{};
        Check(HeadYaw(cy*sp, sy*cp, -sy*sp, cy*cp, &heading), "valid pitched head");
        Near(heading, yaw, "pitch must not rotate the body");
    }
    float singular{};
    Check(!HeadYaw(std::sin(pi/4), 0, 0, std::cos(pi/4), &singular), "looking vertically holds body heading");
    Movement m; Tick(m, Pose(1, 0));
    Near(Tick(m, Pose(2, 0), TrackingYaw(pi, pi)).world, {}, "physical 180 turn cannot translate capsule");
    auto step = Tick(m, Pose(3, 0, -0.05f), TrackingYaw(pi, pi));
    Near(step.world, {0, -0.05f}, "tracking axes do not rotate with body realignment");
    Near(Rotate(step.world, -pi), {0, 0.05f}, "native animation direction is forward after physical 180");
    Near(Tick(m, Pose(4, 0, -0.1f), TrackingYaw(pi + pi/2, pi)).world,
         {0.05f, 0}, "snap turn changes tracking-to-world basis once");
    Near(TrackingYaw(-pi + .02f, pi - .02f), .04f, "wrap at 180 degrees");
}

void Gameplay() {
    // PID21108: the border checkpoint uses Tier2/3 with a valid enabled CCT.
    const auto allowed=[](int tier,int mode=4,bool headAim=false,bool vehicle=false,int suspend=3) {
        return IsVrikBodyMovementAllowed(mode,headAim,vehicle,tier,suspend);
    };
    for (int tier : {1,2,3}) {
        Check(allowed(tier), "active VRIK in a quest tier must permit physical movement");
        Movement movement;
        Tick(movement,Pose(1,0),0,allowed(tier));
        Near(Tick(movement,Pose(2,.02f),0,allowed(tier)).world,
             {.02f,0},"quest tier dropped physical displacement");
    }
    for (int tier : {-1,0,4,5,6})
        Check(!allowed(tier), "default VRIK suspension/invalid tier must gate movement");
    Check(!allowed(1,0), "VRIK off must disable physical body motion even in full gameplay");
    Check(!allowed(3,4,true), "head aim suspends controller VRIK and physical body motion");
    for(int tier=1;tier<=5;++tier)
        Check(!allowed(tier,4,false,true,-1), "vehicles must block body motion even with VRIK Never suspend");
    Check(!allowed(2,4,false,false,1), "Tier2+ VRIK suspend setting must also suspend body motion");
    Check(allowed(4,4,false,false,-1) && allowed(5,4,false,false,-1),
          "Never suspend must not leave a separate hard-coded cinematic movement gate");

    Movement transition;
    Tick(transition,Pose(1,0),0,allowed(2));
    Near(Tick(transition,Pose(2,.1f),0,allowed(4)).world,
         {0,0},"cinematic entry must stop physical movement");
    Near(Tick(transition,Pose(3,.3f),0,allowed(3)).world,
         {0,0},"return to limited gameplay must not repay cinematic head motion");
    Near(Tick(transition,Pose(4,.32f),0,allowed(3)).world,
         {.02f,0},"limited gameplay must resume from its fresh baseline");
    Near(Tick(transition,Pose(5,.42f),0,allowed(3,0)).world,
         {0,0},"turning VRIK off must immediately stop physical movement");
    Near(Tick(transition,Pose(6,.52f),0,allowed(3)).world,
         {0,0},"turning VRIK back on must not replay disabled motion");
}

void SceneSuspend() {
    using cvr::anim::ShouldSuspendVrik;
    // UI value 3 is labelled Tier4+, while the live prologue reports raw Tier3=3.
    Check(!ShouldSuspendVrik(3,3), "default must keep VRIK in limited prologue gameplay");
    Check(ShouldSuspendVrik(4,3), "default must suspend at the actual Tier4 boundary");
    Check(ShouldSuspendVrik(5,3), "default must also suspend full cinematics");
    Check(!ShouldSuspendVrik(4,4), "Tier5-only setting must preserve Tier4 VRIK");
    Check(ShouldSuspendVrik(5,4), "Tier5-only setting must suspend Tier5");
    Check(!ShouldSuspendVrik(1,1), "Tier2+ setting must preserve full gameplay");
    Check(ShouldSuspendVrik(2,1), "Tier2+ setting must suspend staged gameplay");
    Check(!ShouldSuspendVrik(2,2) && ShouldSuspendVrik(3,2), "Tier3+ label must match engine boundary");
    for(int tier=1;tier<=5;++tier) {
        Check(!ShouldSuspendVrik(tier,-1), "Never must preserve all tiers");
        Check(!ShouldSuspendVrik(tier,0), "uninitialized setting must not suspend VRIK");
    }
    Check(!ShouldSuspendVrik(0,3), "Undefined is not a cinematic");
    Check(ShouldSuspendVrik(3,false,2,-1), "vehicle Never setting disabled on-foot suspension");
    Check(!ShouldSuspendVrik(3,true,2,-1), "on-foot threshold leaked into the vehicle");
    Check(!ShouldSuspendVrik(3,false,-1,2), "vehicle threshold leaked into on-foot gameplay");
    Check(ShouldSuspendVrik(3,true,-1,2), "vehicle threshold did not suspend mounted VRIK");
    Check(!ShouldSuspendVrik(4,false,4,3) && ShouldSuspendVrik(4,true,4,3),
          "entering a vehicle did not switch to its own tier boundary");
    using cvr::anim::ResolveVehicleVrikSuspendTier;
    Check(ResolveVehicleVrikSuspendTier(2,3,false)==2, "old INI must preserve its previous mounted threshold");
    Check(ResolveVehicleVrikSuspendTier(2,-1,true)==-1, "explicit vehicle Never must not inherit on-foot setting");
    Check(ResolveVehicleVrikSuspendTier(-1,4,true)==4, "explicit vehicle threshold was coupled to on-foot Never");
}

void Tracking() {
    constexpr XrSpaceLocationFlags flags = XR_SPACE_LOCATION_POSITION_VALID_BIT |
        XR_SPACE_LOCATION_ORIENTATION_VALID_BIT | XR_SPACE_LOCATION_POSITION_TRACKED_BIT |
        XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT;
    Check(HeadTrackingUsable(XR_SESSION_STATE_VISIBLE, flags), "background visible head tracking is usable");
    Check(HeadTrackingUsable(XR_SESSION_STATE_FOCUSED, flags), "focused head tracking is usable");
    for (auto state : {XR_SESSION_STATE_UNKNOWN, XR_SESSION_STATE_IDLE, XR_SESSION_STATE_READY,
                       XR_SESSION_STATE_SYNCHRONIZED, XR_SESSION_STATE_STOPPING,
                       XR_SESSION_STATE_LOSS_PENDING, XR_SESSION_STATE_EXITING})
        Check(!HeadTrackingUsable(state, flags), "non-visible session stays gated");
    for (auto bit : {XR_SPACE_LOCATION_POSITION_VALID_BIT, XR_SPACE_LOCATION_ORIENTATION_VALID_BIT,
                    XR_SPACE_LOCATION_POSITION_TRACKED_BIT, XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT}) {
        Check(!HeadTrackingUsable(XR_SESSION_STATE_VISIBLE, flags & ~bit), "incomplete head tracking rejected");
        Check(!HeadTrackingUsable(XR_SESSION_STATE_FOCUSED, flags & ~bit), "focus cannot override tracking loss");
    }
}

void EyeCentre() {
    using Ledger = cvr::camera::EyeCentreLedger;
    Ledger ledger;
    const Ledger::Position entity{45987168,-318289856,23864768};
    const Ledger::Position centre{entity[0],entity[1],entity[2]+209715};
    Ledger::Position out{};
    Check(!ledger.Read(centre,out), "unpublished position must not match");
    for (float yaw : {0.0f, .4f, 1.2f, pi, -pi/2}) {
        const auto eyeDelta = Rotate({.032f,0},yaw);
        Ledger::Position eye{centre[0]+int(eyeDelta.x*131072), centre[1]+int(eyeDelta.y*131072),centre[2]};
        ledger.Publish(eye,centre);
        Check(ledger.Read(eye,out) && out==centre, "VRIK receives centre instead of the rotated eye");
        const Vec2 bodyGoal{float(out[0]-entity[0])/131072,float(out[1]-entity[1])/131072};
        Near(bodyGoal, {}, "IPD must not become hips translation on HMD yaw");
    }
    const Ledger::Position unknown{1,2,3};
    Check(!ledger.Read(unknown,out), "unrelated serialized camera must not match");
    const Ledger::Position sameEye{4,5,6};
    ledger.Publish(sameEye,{7,8,9}); ledger.Publish(sameEye,{10,11,12});
    Check(ledger.Read(sameEye,out) && out==Ledger::Position{10,11,12}, "latest exact publication wins");
    for (int i=0;i<40;++i) ledger.Publish({100+i,0,0},{100+i,0,0});
    Check(!ledger.Read(sameEye,out), "old camera generations leave bounded history");
}

void RuntimeReset() {
    TrackingResetGate gate;
    Movement m; Tick(m,Pose(1,0)); Tick(m,Pose(2,.13f));
    const Vec2 body{.13f,0};
    const auto locatedBeforeReset=gate.Serial();
    gate.Begin();
    Check(gate.Pending() && !gate.Ready(gate.Serial()), "no pose publish while runtime mutates its origin");
    Sample rawJump=Pose(3,0); rawJump.valid=!gate.Pending();
    Near(Tick(m,rawJump).world, {}, "small runtime reset must not be mistaken for physical return step");
    gate.End();
    Check(gate.Pending() && !gate.Ready(locatedBeforeReset), "pose located before reset must be discarded");
    const auto rebasedAt=gate.Serial();
    Check(gate.Ready(rebasedAt), "post-reset pose may establish a new base");
    gate.Applied(rebasedAt);
    Check(!gate.Pending(), "new coherent base releases movement gate");
    auto first=Tick(m,Pose(4,0,0,40000,2));
    Near(first.world, {}, "origin transition emits no displacement");
    Near(m.Consumed(2), {}, "new tracking span starts at zero");
    Near(body+first.world, body, "capsule remains at the travelled location");
    gate.Begin();
    gate.Applied(rebasedAt);
    Check(gate.Pending(), "completion of an older pose cannot erase a newer pending reset");
    gate.End();
}

void BodySources() {
    using Ledger = cvr::camera::EyeCentreLedger;
    Ledger ledger;
    const Ledger::Position entity{45987168,-318289856,23864768};
    const Ledger::Position centre{entity[0],entity[1],entity[2]+209715};
    const auto expected=Ledger::World(centre);
    Ledger::WorldPosition lastGood=expected;
    // Native-centred snapshots alternate with Lua float snapshots sampled from
    // MAIN's eye. Camera/physics rates need not agree and some publications miss.
    for (int frame=0;frame<180;++frame) {
        const float yaw=float(frame)*pi/90;
        const auto offset=Rotate({.032f,0},yaw);
        const Ledger::Position eye{centre[0]+int(offset.x*131072),centre[1]+int(offset.y*131072),centre[2]};
        ledger.Publish(eye,centre);
        Ledger::Position native{};
        Check(ledger.Read(eye,native),"native centre match");
        const auto nativePair=Ledger::World(native);
        Ledger::WorldPosition lua{};
        Check(ledger.ReadWorld(Ledger::World(eye),lua),"Lua float eye must resolve to the same centre");
        Check(lua==nativePair && lua==expected,"native/Lua fallback cannot alternate by IPD");
        const auto selected=frame%2 ? lua : nativePair;
        Check(selected==lastGood,"source switching must not shift the body anchor");
        lastGood=selected;
        Ledger::WorldPosition missing{};
        Check(!ledger.ReadWorld({1000.123f,2222.75f,-47.0f},missing),"unmatched input cannot masquerade as centred");
        Check(lastGood==expected,"failed reads leave only the complete prior centred snapshot");
    }
    Ledger alreadyCentred;
    alreadyCentred.Publish({centre[0]+4194,centre[1],centre[2]},centre);
    Ledger::WorldPosition value{};
    Check(alreadyCentred.ReadWorld(expected,value) && value==expected,"already-centred publication is not corrected twice");
    Ledger ambiguous;
    const Ledger::Position eyeA{600000000,0,0},eyeB{600000001,0,0};
    Check(Ledger::World(eyeA)==Ledger::World(eyeB),"fixture requires float quantization alias");
    ambiguous.Publish(eyeA,{599995000,0,0});
    ambiguous.Publish(eyeB,{599985000,0,0});
    Check(!ambiguous.ReadWorld(Ledger::World(eyeA),value),"ambiguous float eye cannot select an unrelated centre");
}

void AnchorFrames() {
    using namespace cvr::camera;
    const AnchorVector eyeFromHead{-.02f,.10f,.15f};
    // No head translation, 180-degree physical turn. The model's forward
    // offset must still be forward in model space, not behind the avatar.
    const auto reversed=ComposeAnchorTranslation({}, {}, eyeFromHead, 0, pi);
    Near(reversed.x,.02f,"180 HMD: anatomical camera right offset rotates with body");
    Near(reversed.y,-.10f,"180 HMD: anatomical camera stays in front of the turned head");
    Near(reversed.z,.15f,"yaw preserves eye height");
    for(float base : {0.0f,1.3749924f,-2.1f}) {
        for(float turn : {-pi,-.34906585f,0.0f,1.3962634f,pi}) {
            const float entity=base+turn;
            const auto mouse=ComposeAnchorTranslation({}, {}, eyeFromHead,entity,entity);
            const auto hmd=ComposeAnchorTranslation({}, {}, eyeFromHead,base,entity);
            Near(Vec2{hmd.x,hmd.y},{mouse.x,mouse.y},"same body orientation has same anatomical offset for mouse/HMD");
            const auto inBody=RotateAnchorYaw(hmd,-entity);
            Near(Vec2{inBody.x,inBody.y},{-.02f,.10f},"camera/head relationship survives realign and wrap");
            const AnchorVector physical{.31f,-.12f,.08f};
            const auto translated=ComposeAnchorTranslation(physical,{},eyeFromHead,base,entity);
            const auto physicalOnly=RotateAnchorYaw(physical,base);
            Near(Vec2{translated.x-hmd.x,translated.y-hmd.y},{physicalOnly.x,physicalOnly.y},
                 "physical translation must not rotate with body realign");
            Near(translated.z-hmd.z,.08f,"physical vertical displacement unchanged");
        }
    }
    // PID13216 captured model-space bake at the 180-degree endpoint.
    const AnchorVector measuredBake{-.0209936071f,.115874425f,.265473366f};
    const auto corrected=ComposeAnchorTranslation({}, {}, measuredBake,1.3749907f,-1.8538683f);
    const auto correctedLocal=RotateAnchorYaw(corrected,1.8538683f);
    Near(correctedLocal.y,.115874425f,"captured 180 endpoint retains forward model bake");
    // With no model offsets (mounted/deferred camera) the original tracking
    // offset and physical displacement remain on their original path.
    const auto mounted=ComposeAnchorTranslation({.2f,.1f,-.1f},{.014f,-.013f,-.005f},{},pi/2,0);
    Near(Vec2{mounted.x,mounted.y},{-.087f,.214f},"zero-model-offset vehicle path");
    Near(mounted.z,-.105f,"vehicle height unchanged");
}

void ManualAnchorFrames() {
    using namespace cvr::camera;
    const AnchorVector manual{-.012f,.174f,-.175f},baked{-.0012f,.0102f,.2655f};
    const AnchorVector physical{.31f,-.12f,.08f};
    for (float base : {0.0f,1.37f,-2.1f}) {
        for (float turn : {-pi,-pi/12,0.0f,.7f,pi}) {
            const float entity=base+turn;
            auto recipe=MakeAnchorRecipe(manual,{},baked,false,1,base,entity);
            const auto camera=ComposeAnchorTranslation({},recipe.trackingOffset,recipe.modelOffset,base,entity);
            const auto local=RotateAnchorYaw(camera,-entity);
            Near(Vec2{local.x,local.y},{manual.x+baked.x,manual.y+baked.y},
                 "manual calibration stays attached to body during physical turns");
            Near(local.z,manual.z+baked.z,"manual height is applied once");
            const auto moved=ComposeAnchorTranslation(physical,recipe.trackingOffset,recipe.modelOffset,base,entity);
            const auto expected=RotateAnchorYaw(physical,base);
            Near(Vec2{moved.x-camera.x,moved.y-camera.y},{expected.x,expected.y},
                 "manual calibration does not rotate physical room movement");
            recipe=MakeAnchorRecipe(manual,{},baked,false,1,entity,entity);
            const auto mouse=ComposeAnchorTranslation({},recipe.trackingOffset,recipe.modelOffset,entity,entity);
            Near(Vec2{camera.x,camera.y},{mouse.x,mouse.y},"nonzero manual offsets agree for mouse and HMD yaw");
        }
    }
    const AnchorVector seat{.026f,-.187f,.17f};
    const auto mounted=MakeAnchorRecipe(manual,seat,{},true,1,pi/2,0);
    const auto position=ComposeAnchorTranslation({.2f,.1f,-.1f},mounted.trackingOffset,mounted.modelOffset,pi/2,0);
    Near(Vec2{position.x,position.y},{-.087f,.214f},"seated manual and vehicle offsets retain view basis");
    Near(position.z,-.105f,"seated height unchanged");
    const auto combatSeat=ActiveVehicleCameraOffset(seat,true);
    const auto combat=MakeAnchorRecipe(manual,combatSeat,{},true,1,pi/2,0);
    const auto combatPosition=ComposeAnchorTranslation(physical,combat.trackingOffset,combat.modelOffset,pi/2,0);
    const auto manualOnly=RotateAnchorYaw({physical.x+manual.x,physical.y+manual.y,physical.z+manual.z},pi/2);
    Near(Vec2{combatPosition.x,combatPosition.y},{manualOnly.x,manualOnly.y},
         "passenger combat must retain ordinary Head offsets and omit Car offsets");
    Near(combatPosition.z,manualOnly.z,"passenger combat lost ordinary Head Z or retained Car Z");
    const auto ordinarySeat=ActiveVehicleCameraOffset(seat,false);
    Near(Vec2{ordinarySeat.x,ordinarySeat.y},{seat.x,seat.y},"ordinary vehicle lost saved car offsets");
    Near(ordinarySeat.z,seat.z,"ordinary vehicle lost saved car height");
}

void RenderedPoseMatching() {
    using namespace cvr::camera;
    RenderedPoseHistory<PoseLabel,32> history;
    const float q[4]={0,0,0,1};
    const int32_t start[3]={100000,200000,300000};
    PoseLabel pose{},out{};
    uint32_t age{},ties{};
    for (int frame=0;frame<12;++frame) {
        const int32_t position[3]={start[0]+frame*1000,start[1],start[2]};
        pose.posX=frame*.08f;
        history.Push(1,q,position,pose);
        PoseLabel otherEye=pose; otherEye.posX+=.01f;
        history.Push(2,q,position,otherEye);
    }
    // Identical rotations across a fast translation: render an older frame,
    // repeat it, skip forward, then select the other eye independently.
    for (int frame : {2,2,5,8,11}) {
        const int32_t position[3]={start[0]+frame*1000,start[1],start[2]};
        Check(history.Find(1,q,position,1,&out,&age,&ties)==RenderedPoseMatch::Exact,
              "translation-only render frame has a complete camera match");
        Near(out.posX,frame*.08f,"render pose follows pixels despite repeats and skips");
        Check(ties==1,"eye identity rejects another eye at the same position");
        Check(history.Find(2,q,position,1,&out,&age,&ties)==RenderedPoseMatch::Exact,"second eye matches its own label");
        Near(out.posX,frame*.08f+.01f,"second eye cannot borrow main's XR label");
    }
    const int32_t last[3]={111000,200000,300000};
    Check(history.Find(1,q,last,2,&out,&age,&ties)==RenderedPoseMatch::Missing,
          "recenter rejects labels from an earlier origin");
    Check(history.Find(1,q,start,1,&out,&age,&ties)==RenderedPoseMatch::Exact,"initial pose retained");
    pose.posX=0; history.Push(1,q,start,pose);
    Check(history.Find(1,q,start,1,&out,&age,&ties)==RenderedPoseMatch::Exact && ties==2,
          "repeated camera and equivalent head pose are unambiguous");
    pose.posX=.2f; history.Push(1,q,start,pose);
    Check(history.Find(1,q,start,1,&out,&age,&ties)==RenderedPoseMatch::Ambiguous,
          "blocked movement must not disguise different head positions as one pose");
    pose.originSerial=2; history.Push(1,q,start,pose);
    Check(history.Find(1,q,start,2,&out,&age,&ties)==RenderedPoseMatch::Exact,
          "new origin ignores otherwise identical obsolete transforms");
    for (int i=0;i<40;++i) {
        const int32_t position[3]={400000+i,0,0};
        history.Push(1,q,position,pose);
    }
    Check(history.Find(1,q,start,2,&out,&age,&ties)==RenderedPoseMatch::Missing,
          "overwritten history cannot return an expired camera record");
    pose.posX=std::numeric_limits<float>::quiet_NaN();
    history.Push(1,q,start,pose);
    Check(history.Find(1,q,start,2,&out,&age,&ties)==RenderedPoseMatch::Missing,
          "nonfinite labels cannot enter the rendered camera history");
}

void HandPublication() {
    cvr::tracking::HandPublicationGate gate;
    Check(!gate.NativeOwns(1000000,1),"Present supplies startup hands before native movement");
    gate.PublishedNative(1000000,1);
    Check(gate.NativeOwns(1000000,1),"Present cannot overwrite a just-published native packet");
    Check(gate.NativeOwns(1021000,1),"separate render cadence retains the physical tick's hands");
    Check(!gate.NativeOwns(1021000,1,false),"vehicle/menu/disabled roomscale immediately restore Present publication");
    Check(!gate.NativeOwns(999999,1),"clock rollback does not suppress hand publication");
    Check(!gate.NativeOwns(1010000,2),"recenter releases obsolete native ownership");
    Check(gate.NativeOwns(1250000,1),"ownership remains bounded by its original timestamp");
    Check(!gate.NativeOwns(1250001,1),"loading or missing CCT ticks restore the Present path");
    gate.PublishedNative(1300000,2);
    Check(gate.NativeOwns(1320000,2),"fresh origin can resume native hand publication");
    Check(!gate.NativeOwns(1320000,1),"old tracking generation cannot suppress a publisher");
}

void HandFilter() {
    using cvr::tracking::ExponentialFollow;
    float uniform=0,irregular=0;
    const float target=.25f;
    for (int i=0;i<20;++i) uniform+=(target-uniform)*ExponentialFollow(40,.01f);
    // Same200ms split between 17..49ms frames, including the old25ms snap threshold.
    for (float dt : {.017f,.049f,.023f,.031f,.026f,.019f,.035f})
        irregular+=(target-irregular)*ExponentialFollow(40,dt);
    Near(uniform,irregular,"hand filtering is independent of partitioning frame time",.000001f);
    Near(irregular,target*(1-std::exp(-8.0f)),"configured rate is the continuous-time response",.000001f);
    Near(ExponentialFollow(40,.025f),1-std::exp(-1.0f),"25ms frame must not snap a melee hand to its target");
    Check(ExponentialFollow(40,.049f)<1,"long native frame retains a bounded gradual response");
    Near(ExponentialFollow(40,0),0,"no elapsed time introduces no filter movement");
    Near(ExponentialFollow(40,-.1f),0,"backwards clock introduces no filter movement");
    Near(ExponentialFollow(std::numeric_limits<float>::quiet_NaN(),.01f),0,"invalid rate cannot poison hand pose");
    Check(ExponentialFollow(40,.000001f)>0,"small intervals do not lose precision to cancellation");
}

void FilterOriginReset() {
    using cvr::camera::AnchorVector;
    using cvr::tracking::RebaseTrackingFilter;
    using Quat=std::array<float,4>;
    bool initialized=false; uint64_t origin=0,lastUs=0;
    AnchorVector position{};Quat orientation{};
    const AnchorVector oldPosition{-.1493578f,0,.05052143f};
    const Quat oldOrientation{0,.2f,0,.9797959f},identity{0,0,0,1};
    Check(RebaseTrackingFilter(2,1000000,oldPosition,oldOrientation,initialized,origin,lastUs,position,orientation),
          "cold filter adopts the first tracking reference");
    Check(!RebaseTrackingFilter(2,1010000,AnchorVector{},identity,initialized,origin,lastUs,position,orientation),
          "ordinary tracking samples retain the filter history");
    Movement movement;
    Tick(movement,Pose(1,oldPosition.x,-oldPosition.z,1000000,2));
    Check(RebaseTrackingFilter(3,1020000,AnchorVector{},identity,initialized,origin,lastUs,position,orientation),
          "runtime reset replaces stale coordinates instead of interpolating between origins");
    Check(orientation==identity && lastUs==1020000,"rotation and filter clock reset with position");
    Vec2 falseTravel{};
    for (uint64_t i=2;i<=24;++i) {
        // Same stationary head after Reset View. Without replacing the old
        // filter state its tail becomes149mm of spurious native movement.
        position.x+=(0-position.x)*cvr::tracking::ExponentialFollow(30,.02f);
        position.z+=(0-position.z)*cvr::tracking::ExponentialFollow(30,.02f);
        falseTravel=falseTravel+Tick(movement,Pose(i,position.x,-position.z,1000000+i*20000,3)).world;
    }
    Near(falseTravel,{},"reset filter tail cannot move the native body");
    Near(movement.Consumed(3),{},"reset does not leave a hidden camera/body offset in the ledger");
}

void MovementFrame() {
    Movement movement;
    const auto seed=Pose(1,0,0,1000000);
    Tick(movement,MovementFrameSample(seed,seed));
    Vec2 entity{},viewHead{};
    float worstRelativeError=0;
    // A rapid80cm movement, with the existing camera low-pass and irregular
    // engine cadence. Raw XR keeps advancing independently between CCT calls.
    for (int i=1;i<=180;++i) {
        const float dt=i%3==0 ? .021f : .014f;
        const float target=i<30 ? .8f*i/30 : i<65 ? .8f : i<95 ? .8f*(95-i)/30 : 0;
        viewHead.x+=(target-viewHead.x)*(1-std::exp(-30*dt));
        const uint64_t time=1000000+i*20000;
        Sample tracking=Pose(100+i*2,target,0,time);
        Sample frame=Pose(1+i,viewHead.x,0,time-1000);
        const auto selected=MovementFrameSample(tracking,frame);
        auto step=movement.Begin(selected,time,dt,1,0,true);movement.Complete(step);
        entity=entity+step.world;
        worstRelativeError=std::max(worstRelativeError,std::abs(entity.x-viewHead.x));
        // A second physics call in the same camera frame must not consume the
        // newer runtime pose. There is still only one visible sample this frame.
        tracking.sequence++;tracking.head.x+=.007f;tracking.stampUs+=1000;
        auto duplicate=movement.Begin(MovementFrameSample(tracking,frame),time+1000,dt,1,0,true);
        Near(duplicate.world,{},"new XR cycle cannot add a second movement to one camera frame");
        movement.Complete(duplicate);
    }
    Near(worstRelativeError,0,"physics/body and rendered head use the same moving sample");
    Near(entity,{},"published camera trajectory returns without movement debt");
    Sample raw=Pose(500,.8f,0,5000000),frame=Pose(12,.7f,0,4990000);
    auto selected=MovementFrameSample(raw,frame);
    Near(selected.head,frame.head,"movement uses the frame position, not fresh raw pose");
    Check(selected.sequence==frame.sequence,"deduplicate using frame publication identity");
    Check(selected.stampUs==frame.stampUs,"re-reading a cached frame cannot make it fresh");
    raw.valid=false;
    Check(!MovementFrameSample(raw,frame).valid,"cached render pose cannot override loss of tracking");
    raw.valid=true;frame.origin=raw.origin+1;
    Check(!MovementFrameSample(raw,frame).valid,"mismatched recenter generations are not movement");
    frame.origin=raw.origin;raw.stampUs=4000000;
    Check(MovementFrameSample(raw,frame).stampUs==raw.stampUs,"fresh view cannot mask stale tracking");
    frame.valid=false;
    Check(!MovementFrameSample(raw,frame).valid,"invalid camera latch is rejected");
    Movement jumpGuard;
    Tick(jumpGuard,MovementFrameSample(Pose(1,0),Pose(1,0)));
    float filteredJump=0;Vec2 jumpTravel{};
    for(int i=2;i<60;++i) {
        filteredJump+=(3-filteredJump)*.35f;
        auto selectedJump=MovementFrameSample(Pose(i,3),Pose(i,filteredJump));
        jumpTravel=jumpTravel+Tick(jumpGuard,selectedJump).world;
    }
    Near(jumpTravel,{},"view smoothing cannot turn a raw tracking teleport into movement",.0011f);
    Near(jumpGuard.Consumed(1),{3,0},"tracking-jump camera span is rebased",.0011f);
    raw.head.x=std::numeric_limits<float>::quiet_NaN();frame.valid=true;
    Check(!MovementFrameSample(raw,frame).valid,"invalid raw tracking cannot hide behind a valid cached view");
    Movement missingFrame;
    Tick(missingFrame,MovementFrameSample(Pose(1,0),Pose(1,0)));
    Tick(missingFrame,MovementFrameSample(Pose(2,.2f),Pose(2,.2f)));
    auto failedRead=MovementFrameSample(Pose(3,.2f),Sample{});
    Tick(missingFrame,failedRead);
    Near(missingFrame.Consumed(1),{.2f,0},"frame read failure cannot reset the existing camera consumption ledger");
}

void BodyCameraBase() {
    using Ledger=cvr::camera::EyeCentreLedger;
    using namespace cvr::camera;
    Ledger ledger;
    const Ledger::Position base{45987168,-318289856,23864768};
    const Ledger::Position centreA{base[0]+1000,base[1]-2000,base[2]+3000};
    const Ledger::Position eyeA{centreA[0]-4194,centreA[1],centreA[2]};
    ledger.Publish(eyeA,centreA,base);
    // Camera B has newer head motion or a just-clicked bake. The body read of
    // camera A must retain A's original base, not subtract B's latest offset.
    const Ledger::Position centreB{base[0]+9000,base[1]+26000,base[2]+1000};
    ledger.Publish({centreB[0]-4194,centreB[1],centreB[2]},centreB,base);
    Ledger::Position centre{},body{};
    Check(ledger.Read(eyeA,centre,&body) && centre==centreA && body==base,
          "body base travels with its camera sample instead of a later delta");
    Ledger::WorldPosition worldCentre{},worldBase{};
    Check(ledger.ReadWorld(Ledger::World(eyeA),worldCentre,&worldBase) && worldBase==Ledger::World(base),
          "Lua fallback uses the same base as native publication");
    const AnchorVector target{0,0,1.6f},nativeView{.24f,.31f,1.6f};
    AnchorVector bake{};
    for(int press=0;press<5;++press) {
        const AnchorVector shown{nativeView.x+bake.x,nativeView.y+bake.y,nativeView.z};
        bake=AbsoluteHorizontalCameraBake(bake,target,shown);
        Near(Vec2{nativeView.x+bake.x,nativeView.y+bake.y},{target.x,target.y},"repeated Bake keeps the camera target");
    }
    Near(Vec2{bake.x,bake.y},{-.24f,-.31f},"Bake is an absolute replacement, not a toggling residual");
    Near(bake.z,0,"horizontal calibration keeps native height");
    Ledger ambiguous;
    ambiguous.Publish(eyeA,centreA,base);
    auto movedBase=base; movedBase[0]+=1000;
    ambiguous.Publish(eyeA,centreA,movedBase);
    Check(!ambiguous.ReadWorld(Ledger::World(eyeA),worldCentre,&worldBase),
          "same eye and different body bases cannot produce a guessed Lua anchor");
}
void FrameAimPublication() {
    cvr::tracking::FrameAim aim;
    const auto empty=aim.Read();
    Check(!empty.epoch && !empty.time && !empty.stampUs,"unpublished aim is not empty");
    std::atomic<bool> start{false},done{false},failed{false};
    std::atomic<uint64_t> reads{0};
    std::vector<std::thread> consumers;
    for(int k=0;k<3;++k)consumers.emplace_back([&] {
        while(!start.load())std::this_thread::yield();
        uint64_t previous=0;
        do {
            const auto value=aim.Read();
            if(value.time!=int64_t(value.epoch)*100003 || value.stampUs!=value.epoch*17 || value.epoch<previous)
                failed.store(true);
            previous=value.epoch;++reads;
        } while(!done.load());
    });
    start.store(true);
    for(uint64_t epoch=1;epoch<=100000;++epoch) {
        aim.Publish(int64_t(epoch)*100003,epoch*17);
        if(epoch%64==0)std::this_thread::yield();
    }
    done.store(true);
    for(auto& thread:consumers)thread.join();
    Check(!failed.load(),"pose aim combined fields from different publications");
    Check(aim.Read().epoch==100000 && reads.load()>=3,"aim concurrency probe did not complete");
}
void EyeFloatAlias() {
    using L=cvr::camera::EyeCentreLedger;
    L ledger;
    const L::Position base{46266052,-322021088,23955425};
    ledger.Publish({46279060,-322035670,23984654},{46280997,-322031951,23984654},base);
    ledger.Publish({46279060,-322035670,23984654},{46280997,-322031950,23984654},base);
    const L::Position newest{46280998,-322031949,23984654};
    ledger.Publish({46279061,-322035669,23984654},newest,base);
    const L::WorldPosition query{353.0812072753906f,-2456.937255859375f,182.98838806152344f};
    L::WorldPosition centre{},body{};
    Check(ledger.ReadWorld(query,centre,&body),"live float alias must not expire the Lua camera");
    Check(centre==L::World(newest) && body==L::World(base),"float alias must retain the newest complete record");
    auto different=newest;different[0]+=32;
    ledger.Publish({46279061,-322035669,23984654},different,base);
    Check(!ledger.ReadWorld(query,centre,&body),"genuinely different centres cannot be merged by rounding tolerance");
    L movedBody;auto otherBase=base;otherBase[0]+=32;
    movedBody.Publish({46279060,-322035670,23984654},newest,base);
    movedBody.Publish({46279061,-322035669,23984654},newest,otherBase);
    Check(!movedBody.ReadWorld(query,centre,&body),"float centre tolerance cannot merge different body frames");
    // PID14588, 21:14: camera publication stopped for 610-750ms at rest.
    // Camera and body translated together by <=2 fixed-point units in Z, but
    // their independently rounded float coordinates did not agree exactly.
    L seated;
    seated.Publish({102588179,-277197450,22767589},{102584036,-277196802,22767589},{102583587,-277197129,22766803});
    seated.Publish({102588180,-277197450,22767588},{102584037,-277196802,22767588},{102583588,-277197129,22766802});
    const L::Position latestCentre{102584037,-277196802,22767587},latestBase{102583588,-277197129,22766801};
    seated.Publish({102588180,-277197450,22767587},latestCentre,latestBase);
    const L::WorldPosition seatedQuery{782.6856689453125f,-2114.8486328125f,173.70291137695312f};
    Check(seated.ReadWorld(seatedQuery,centre,&body),"identical relative seated frames were rejected due to float aliasing");
    Check(centre==L::World(latestCentre)&&body==L::World(latestBase),"seated alias did not preserve the newest complete record");
    auto alteredBase=latestBase;alteredBase[2]+=2;
    seated.Publish({102588180,-277197450,22767587},latestCentre,alteredBase);
    Check(!seated.ReadWorld(seatedQuery,centre,&body),"a changing head-to-body offset was incorrectly accepted");
    L roundedRelative;
    roundedRelative.Publish({102535866,-277197565,22767589},{102531730,-277196874,22767589},{102531278,-277197197,22766803});
    roundedRelative.Publish({102535865,-277197566,22767587},{102531729,-277196874,22767587},{102531277,-277197196,22766801});
    const L::WorldPosition roundQuery{782.2865600585938f,-2114.849609375f,173.70291137695312f};
    Check(roundedRelative.ReadWorld(roundQuery,centre,&body),"one fixed-point unit of relative rounding starved mounted publication");
}
void NativeCameraPair() {
    using namespace cvr::camera;
    const float q[4]={0,0,-.980793178f,.195050582f};
    const FixedPosition entity{46297284,-322011232,23745930};
    const FixedPosition base{46297284,-322011232,23955645};
    const FixedPosition centre{46303296,-322028702,23984874};
    const auto frame=MakeCameraEntitySample(centre,base,entity,q,q);
    Check(frame.valid,"verified camera owner pair rejected");
    Near(frame.bodyMinusEntity[0],0,"paired root X");Near(frame.bodyMinusEntity[1],0,"paired root Y");
    Near(frame.bodyMinusEntity[2],209715.0f/131072.0f,"native relative height must not subtract rounded world floats",1e-7f);
    const auto state=CameraPacketStatus(10,2,1000,10,2,1020,true,false);
    Check(state==CameraPacketState::Valid,"complete camera write must not depend on a Present counter");
    Check(CameraPacketStatus(10,2,1000,11,2,1020,true,false)==CameraPacketState::Invalidated,"player replacement accepted stale camera");
    Check(CameraPacketStatus(10,2,1000,10,3,1020,true,false)==CameraPacketState::Invalidated,"recenter accepted old origin");
    Check(CameraPacketStatus(10,2,1000,10,2,1251,true,false)==CameraPacketState::Unavailable,"stale camera accepted");
    Check(CameraPacketStatus(10,2,1000,10,2,1020,false,true)==CameraPacketState::Unavailable,"concurrent owner write was treated as detachment");
    auto displaced=centre;displaced[0]+=10*131072;
    Check(!MakeCameraEntitySample(displaced,base,entity,q,q).valid,"foreign detached camera accepted");
}
void RenderRoundoff() {
    using namespace cvr::camera;
    const float written[4]={0,0,-.9883676767349243f,.1520833671092987f};
    const float rendered[4]={-9.236703135684365e-7f,1.5223009768305928e-6f,-.9883673191070557f,.1520863026380539f};
    Check(SameRenderedRotation(written,rendered),"actual VRCAM matrix round-trip lost the rendered pose");
    const float negated[4]={-rendered[0],-rendered[1],-rendered[2],-rendered[3]};
    Check(SameRenderedRotation(written,negated),"quaternion sign changed the camera identity");
    // PID15084: VRCAM matrix round trip at -40deg pitch, exact fixed-point eye.
    const float pitchedWrite[4]={-.06671655178070068f,.3354438841342926f,-.9216427803039551f,.18330585956573486f};
    const float pitchedRead[4]={-.06671430170536041f,.3354325592517853f,-.9216468334197998f,.18330666422843933f};
    Check(SameRenderedRotation(pitchedWrite,pitchedRead),"pitched VRCAM round trip lost its image label");
    const float moved[4]={.001f,0,written[2],written[3]};
    Check(!SameRenderedRotation(written,moved),"real rotation accepted as numerical roundoff");
    const float bad[4]={};Check(!SameRenderedRotation(written,bad),"zero quaternion accepted");
}
void HandViewSample() {
    using namespace cvr::camera;
    const AnchorRecipe recipe=MakeAnchorRecipe({.012f,.034f,-.05f},{},{-.02f,.1f,.23f},false,1,.2f,.7f);
    const float bodyYaw=.7f,trackingYaw=.2f;
    const XrPosef controller{{0,0,0,1},{.23f,-.30f,-.28f}};
    for(float x:{-.4f,0.0f,.3f})for(float yaw:{-.8f,0.0f,.6f}) {
        const XrPosef head{{0,std::sin(yaw*.5f),0,std::cos(yaw*.5f)},{x,.12f,x*.5f}};
        const auto local=RelativePose(head,controller);
        const auto frame=BuildHandViewFrame({0,0,1.6f},recipe,
            {head.position.x,head.position.y,head.position.z},head.orientation,bodyYaw,trackingYaw,{0,0,0,1});
        const auto offset=RotateVector(frame.rotation,{local.position.x,-local.position.z,local.position.y});
        const auto expected=RotateAnchorYaw({controller.position.x,-controller.position.z,controller.position.y},trackingYaw-bodyYaw);
        Near(frame.position.x+offset.x,expected.x+recipe.modelOffset.x,"hand sample translation/orientation must cancel together",1e-6f);
        Near(frame.position.y+offset.y,expected.y+recipe.modelOffset.y,"head rotation must not move a stationary controller",1e-6f);
        Near(frame.position.z+offset.z,1.6f+expected.z+recipe.modelOffset.z,"current hand head height must replace previous camera height",1e-6f);
    }
}
void RenderQuantizedLabels() {
    using namespace cvr::camera;
    RenderedPoseHistory<PoseLabel> history;
    const float q[4]={0,0,-.9807894229888916f,.1950695514678955f};
    const int32_t position[3]={46305623,-322027560,23984874};
    PoseLabel first{};first.originSerial=2;
    first.posX=.00022154403268359601f;
    auto latest=first;latest.posX=4.242219910858408e-13f;
    history.Push(2,q,position,first);history.Push(2,q,position,latest);
    PoseLabel result{};uint32_t age=99,ties=0;
    Check(history.Find(2,q,position,2,&result,&age,&ties)==RenderedPoseMatch::Exact,
          "one world-float quantum made equivalent image labels ambiguous");
    Check(result.posX==latest.posX && age==0 && ties==2,"quantized labels lost newest complete sample");
    Check(history.Find(2,q,position,3,&result,&age,&ties)==RenderedPoseMatch::Missing,"quantization bypassed tracking origin");
    Check(history.Find(1,q,position,2,&result,&age,&ties)==RenderedPoseMatch::Missing,"quantization bypassed eye identity");
    auto different=latest;different.posX=.001f;history.Push(2,q,position,different);
    Check(history.Find(2,q,position,2,&result,&age,&ties)==RenderedPoseMatch::Ambiguous,"real head displacement was merged with old pixels");
    RenderedPoseHistory<PoseLabel> nearOrigin;
    const int32_t zero[3]={0,0,0};nearOrigin.Push(2,q,zero,first);nearOrigin.Push(2,q,zero,latest);
    Check(nearOrigin.Find(2,q,zero,2,&result,&age,&ties)==RenderedPoseMatch::Ambiguous,"world-float tolerance ignored coordinate precision");
    RenderedPoseHistory<PoseLabel> diagonal;
    const int32_t distant[3]={2000000000,-2000000000,2000000000};
    diagonal.Push(2,q,distant,latest);different=latest;different.posX=.0002f;different.posY=.0002f;
    diagonal.Push(2,q,distant,different);
    Check(diagonal.Find(2,q,distant,2,&result,&age,&ties)==RenderedPoseMatch::Ambiguous,"vector error exceeded the quarter-millimetre cap");
}
}
void BodyFollowSuspend() {
    BodyYawFollower follower;
    follower.SetEnabled(IsVrikBodyMovementAllowed(4,false,false,1,2));
    uint64_t stamp=1000000;
    follower.Step(-.68414277f,.08726646f,stamp,1);
    // the body comes round under the head over a couple of seconds, to within the tremor it ignores
    for(int i=0;i<450;++i)follower.Step(-.68414277f,.08726646f,stamp+=11111,1);
    Near(follower.Offset(),-.68414277f,"baseline body yaw was not acquired",1.6f*pi/180);
    // A menu pauses injection without handing the heading to another camera.
    const float before=follower.Offset();
    follower.SetEnabled(true);
    Near(follower.Offset(),before,"menu discarded the on-foot heading");
    // Refresh happens from camera consumers too: no heading callback required.
    follower.SetEnabled(IsVrikBodyMovementAllowed(4,false,false,4,2));
    Near(follower.Offset(),0,"suspended scene retained the -39.2 degree offset");
    Near(follower.Step(1,.08726646f,stamp+=11111,1),0,"suspended follower injected a turn");
    follower.SetEnabled(IsVrikBodyMovementAllowed(4,false,false,1,2));
    Near(follower.Step(.02f,.08726646f,stamp+=11111,1),0,"resume retained an active catch-up");
    Check(!IsVrikBodyMovementAllowed(4,false,true,1,-1),"vehicle enabled physical body motion");
    Check(!IsVrikBodyMovementAllowed(0,false,false,1,-1),"disabled VRIK enabled physical body motion");
    follower.Step(.8f,.08726646f,stamp+=11111,1);
    follower.Step(.8f,.08726646f,stamp+=11111,2);
    Near(follower.Offset(),0,"recenter retained the previous origin's offset");
}
void BodyBendNoise() {
    float minimum=1000;int failed=0;
    for(float hz:{30.f,45.f,90.f,144.f})for(unsigned seed=1;seed<=10;++seed) {
        cvr::body::BendTracker tracker;std::mt19937 rng(seed);
        std::normal_distribution<float> noise(0,.002f);cvr::body::BendSample result{};
        tracker.Update({0,0,0},{0,0,0,1},1);
        for(int i=0;i<int(hz*1.2f);++i) {
            const float u=std::min(1.f,float(i+1)/(hz*1.2f));
            const float angle=40*pi/180*(u*u*(3-2*u));
            const std::array<float,3> position{noise(rng),.63f*(std::cos(angle)-1)+noise(rng),-.63f*std::sin(angle)+noise(rng)};
            for(int repeated=0;repeated<3;++repeated)result=tracker.Update(position,{0,0,0,1},1);
        }
        // Keep the true endpoint, with independent tracking noise still active.
        for(int i=0;i<int(hz*.3f);++i)
            result=tracker.Update({noise(rng),.63f*(std::cos(40*pi/180)-1)+noise(rng),-.63f*std::sin(40*pi/180)+noise(rng)},
                                  {0,0,0,1},1);
        minimum=std::min(minimum,result.angle*180/pi);
        failed+=result.angle<5*pi/180;
    }
    std::cout<<"noisy hinge: minimum "<<minimum<<" degrees, failed "<<failed<<"/40\n";
    Check(failed==0,"2mm tracking noise hid a deliberate 40-degree level-head hinge");
    float falseBend=0;
    for(float drop:{0.f,.35f,.7f})for(float hz:{30.f,90.f,144.f})for(unsigned seed=1;seed<=10;++seed) {
        cvr::body::BendTracker tracker;std::mt19937 rng(914+seed);
        std::normal_distribution<float> noise(0,.002f);
        tracker.Update({0,-drop,0},{0,0,0,1},1);
        for(int i=0;i<int(hz*30);++i) {
            const float t=i/hz;
            const auto result=tracker.Update({noise(rng),-drop+noise(rng),-.5f*t+noise(rng)}, {0,0,0,1},1);
            falseBend=std::max(falseBend,result.angle*180/pi);
        }
    }
    std::cout<<"noisy flat/crouched walk: false bend "<<falseBend<<" degrees\n";
    Check(falseBend<5,"noisy walking/crouching created a false Hybrid posture cue");
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    const std::string name = argv[1];
    if(name=="body_follow_suspend") { BodyFollowSuspend();return 0; }
    if(name=="vehicle_reload_input") { VehicleReloadInput();return 0; }
    if(name=="vehicle_world_heading") { VehicleWorldHeading();return 0; }
    if(name=="takeover_world_heading") { TakeoverWorldHeading();return 0; }
    if(name=="scene_camera_heading") { SceneCameraHeading();return 0; }
    if(name=="vehicle_turn_input") { VehicleTurnInput();return 0; }
    if(name=="ladder_turn_input") { LadderTurnInput();return 0; }
    if(name=="vehicle_stereo_heading") { VehicleStereoHeading();return 0; }
    if(name=="vehicle_heading_identity") { VehicleHeadingIdentity();return 0; }
    if(name=="free_look_zone") { FreeLookZone();return 0; }
    if(name=="scene_suspend") { SceneSuspend();return 0; }
    if(name=="look_down_cone") { LookDownCone();return 0; }
    if(name=="body_bend_tracking") { BodyBendTracking();return 0; }
    if(name=="floor_reach_tracking") { FloorReachTracking();return 0; }
    if(name=="movement_free_zone") { MovementFreeZone();return 0; }
    if(name=="movement_soft_onset") { MovementSoftOnset();return 0; }
    if(name=="frame_aim") { FrameAimPublication();return 0; }
    if(name=="render_quantized_labels") { RenderQuantizedLabels();return 0; }
    if(name=="eye_float_alias") { EyeFloatAlias();return 0; }
    if(name=="native_camera_pair") { NativeCameraPair();return 0; }
    if(name=="render_roundoff") { RenderRoundoff();return 0; }
    if(name=="body_bend_noise") { BodyBendNoise();return 0; }
    if(name=="hand_view_sample") { HandViewSample();return 0; }
    if (name == "cadence") Cadence(); else if (name == "camera") Camera();
    else if (name == "collision") Collision(); else if (name == "feedback") Feedback();
    else if (name == "transitions") Transitions(); else if (name == "invalid") Invalid();
    else if (name == "yaw") Yaw(); else if (name == "gameplay") Gameplay();
    else if (name == "tracking") Tracking();
    else if (name == "eye_centre") EyeCentre();
    else if (name == "body_sources") BodySources();
    else if (name == "anchor_frames") AnchorFrames();
    else if (name == "manual_anchor_frames") ManualAnchorFrames();
    else if (name == "rendered_pose") RenderedPoseMatching();
    else if (name == "hand_publication") HandPublication();
    else if (name == "hand_filter") HandFilter();
    else if (name == "filter_origin_reset") FilterOriginReset();
    else if (name == "movement_frame") MovementFrame();
    else if (name == "body_camera_base") BodyCameraBase();
    else if (name == "runtime_reset") RuntimeReset(); else return 2;
    std::cout << "PASS " << name << '\n';
}
