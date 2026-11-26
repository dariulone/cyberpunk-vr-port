#include "Natives/HandReferenceFrame.hpp"
#include <cstdlib>
#include <iostream>
#include <limits>
using namespace cvr::handspace;
static void Check(bool ok,const char* why){if(!ok){std::cerr<<why<<'\n';std::exit(1);}}
static void Near(Vector a,Vector b,const char* why,float e=3e-4f){Check(LengthSquared(a-b)<e*e,why);}
static Rotation Yaw(float angle){return {0,0,std::sin(angle*.5f),std::cos(angle*.5f)};}
int main(){
    const Vector worldEye{-1400,1250,25},head{.13f,-.07f,.04f};
    const auto tracking=Make(worldEye,Yaw(.4f),1,7);
    const Rotation headTurn=Multiply(Yaw(.7f),Rotation{std::sin(.2f),0,0,std::cos(.2f)});
    // Changing the avatar's yaw changes only its coordinate system. Both
    // representations of one rendered view must yield one tracking reference.
    for(float bodyYaw:{-1.2f,0.f,.3f,1.4f}){
        const auto entity=Make({-1400,1250,23},Yaw(bodyYaw),1,7);
        const auto inverse=Conjugate(entity.rotation);
        const auto headWorld=Rotate(tracking.rotation,head);
        const auto viewModel=Rotate(inverse,tracking.position+headWorld-entity.position);
        const auto viewRotation=Multiply(inverse,Multiply(tracking.rotation,headTurn));
        const auto reference=FromView(entity,viewModel,viewRotation,Rotate(inverse,headWorld),headTurn);
        Check(reference.valid,"view reference invalid");
        Near(reference.position,tracking.position,"avatar yaw moved the tracking origin");
        Near(Point(reference,{.25f,.5f,-.4f}),Point(tracking,{.25f,.5f,-.4f}),"avatar/head yaw leaked into controller frame");
    }
    // A stick turn occurs around the eye, not around the feet. Contacts on the
    // other hand retain their local separation through every rigid rig move.
    Reference previous=tracking;
    Vector left=Point(previous,{-.03f,.5f,-.3f}),right=Point(previous,{.03f,.5f,-.3f});
    Vector velocity=Rotate(previous.rotation,{.2f,.1f,0});
    for(int i=1;i<=120;++i){
        const auto turn=Yaw(.4f+float(i)*.017f);
        const Vector eye=worldEye+Vector{float(i)*.025f,0,float(i)*.004f};
        const auto next=Make(eye-Rotate(turn,head),turn,1,7);
        const auto move=Between(previous,next);
        Check(move.valid,"walking/lift/turn motion rejected");
        const Vector localLeft=Rotate(Conjugate(previous.rotation),left-previous.position);
        const Vector localRight=Rotate(Conjugate(previous.rotation),right-previous.position);
        const auto beforeSpeed=LengthSquared(velocity);
        left=move.Point(left);right=move.Point(right);velocity=move.Direction(velocity);
        Near(left,Point(next,localLeft),"left contact changed in moving rig");
        Near(right,Point(next,localRight),"right contact changed in moving rig");
        Check(std::abs(LengthSquared(velocity)-beforeSpeed)<1e-5f,"rig turn changed kinetic speed");
        previous=next;
    }
    Near(right-left,Rotate(previous.rotation,{.06f,0,0}),"contact separation accumulated drift",.003f);
    auto recentered=previous;recentered.origin++;
    Check(!Between(previous,recentered).valid,"recenter treated as physical movement");
    auto replaced=previous;replaced.owner++;
    Check(!Between(previous,replaced).valid,"new avatar inherited old contacts");
    auto teleport=previous;teleport.position.x+=5;
    Check(!Between(previous,teleport).valid,"teleport transported stale contact");
    Check(!Make({std::numeric_limits<float>::quiet_NaN(),0,0},{},1,7).valid,"NaN frame accepted");
    Check(!Make({},Rotation{0,0,0,0},1,7).valid,"zero quaternion accepted");
    std::cout<<"hand reference: torso/HMD invariance, running, lift, stick turn, recenter and teleport PASS\n";
}
