#include "Natives/HeldGripPose.hpp"
#include <array>
#include <cstdlib>
#include <iostream>
#include <limits>
using namespace cvr::held_grip;
static void Check(bool ok,const char* why){if(!ok){std::cerr<<why<<'\n';std::exit(1);}}
static std::array<float,4> Yaw(float degrees){const float h=degrees*.00872664626f;return {0,0,std::sin(h),std::cos(h)};}
int main(){
    const float wrist[3]{-1042.f,1337.f,6.6f},relationP[3]{.08f,-.06f,.04f},grip[3]{.03f,.1f,-.02f};
    for(float visual:{-170.f,-65.f,0.f,70.f,179.f})for(float wanted:{-160.f,-40.f,45.f,150.f})for(float relation:{-35.f,0.f,80.f}){
        const auto draw=Yaw(visual),control=Yaw(wanted),bodyQ=Yaw(relation);
        float outP[3],outQ[4],local[3],before[3],after[3],expected[4];
        Check(DriveTarget(wrist,draw.data(),control.data(),relationP,bodyQ.data(),grip,outP,outQ),"valid drive rejected");
        Rotate(bodyQ.data(),grip,local);for(int k=0;k<3;++k)local[k]+=relationP[k];Rotate(draw.data(),local,before);
        Rotate(outQ,grip,after);for(int k=0;k<3;++k)Check(std::abs((outP[k]+after[k])-(wrist[k]+before[k]))<.00025f,"visual grip point moved when angular drive changed");
        Multiply(control.data(),bodyQ.data(),expected);float dot=0;for(int k=0;k<4;++k)dot+=outQ[k]*expected[k];Check(std::abs(dot)>.99999f,"load-corrected wrist cancelled controller torque");
    }
    auto still=Yaw(0),turn=Yaw(90);float p[3],q[4];
    Check(DriveTarget(wrist,still.data(),turn.data(),relationP,still.data(),grip,p,q),"held rotation test");
    Check(std::abs(q[2])>.7f,"stationary visual wrist swallowed a ninety-degree controller turn");
    const float origin[3]{-1024,1024,256};float localWrist[3],localP[3],localQ[4];
    for(int k=0;k<3;++k)localWrist[k]=wrist[k]-origin[k];
    Check(DriveTarget(localWrist,still.data(),turn.data(),relationP,still.data(),grip,localP,localQ),"rebased drive rejected");
    for(int k=0;k<3;++k)Check(std::abs(localP[k]+origin[k]-p[k])<.00025f,"scene origin rebase moved held grip");
    turn[0]=std::numeric_limits<float>::quiet_NaN();
    Check(!DriveTarget(wrist,still.data(),turn.data(),relationP,still.data(),grip,p,q),"NaN control accepted");
    // Both wrists can stay level while their grip positions turn the held body.
    // The previous average-of-wrists solution left it horizontal in these cases.
    const float aGrip[3]{-.2f,.03f,.02f},bGrip[3]{.2f,.03f,.02f};
    for(float angle:{-179.f,-90.f,-30.f,0.f,40.f,90.f,179.f,180.f}) {
        const auto expected=Yaw(angle);
        float ga[3],gb[3],ap[3],bp[3],centre[3],outP[3],outQ[4];
        Rotate(expected.data(),aGrip,ga);Rotate(expected.data(),bGrip,gb);
        for(int k=0;k<3;++k){ga[k]+=wrist[k];gb[k]+=wrist[k];ap[k]=ga[k]-aGrip[k];bp[k]=gb[k]-bGrip[k];}
        Check(TwoHandTarget(ap,still.data(),aGrip,bp,still.data(),bGrip,outP,outQ,centre),"two-hand target rejected");
        float gotA[3],gotB[3];Rotate(outQ,aGrip,gotA);Rotate(outQ,bGrip,gotB);
        for(int k=0;k<3;++k) {
            Check(std::abs(outP[k]+gotA[k]-ga[k])<.0005f,"first grip missed when hands turned body");
            Check(std::abs(outP[k]+gotB[k]-gb[k])<.0005f,"second grip missed when hands turned body");
        }
        float swappedP[3],swappedQ[4],swappedCentre[3];
        Check(TwoHandTarget(bp,still.data(),bGrip,ap,still.data(),aGrip,swappedP,swappedQ,swappedCentre),"swapped hands rejected");
        for(int k=0;k<3;++k)Check(std::abs(swappedP[k]-outP[k])<.0005f,"hand order moved body");
        float d=0;for(int k=0;k<4;++k)d+=swappedQ[k]*outQ[k];Check(std::abs(d)>.99999f,"hand order changed body turn");
    }
    // Unequal hand separation cannot stretch a rigid body: share the error
    // equally, retain its midpoint, and still align the two grip directions.
    const float ap[3]{.2f,-.4f,0},bp[3]{-.2f,.4f,0};float centre[3];
    Check(TwoHandTarget(ap,still.data(),aGrip,bp,still.data(),bGrip,p,q,centre),"stretched hand span rejected");
    float ca[3],cb[3];Rotate(q,aGrip,ca);Rotate(q,bGrip,cb);
    Check(std::abs(cb[0]-ca[0])<1e-6f && std::abs(cb[1]-ca[1]-.4f)<1e-6f,"two-hand span changed rigid body length");
    for(int k=0;k<3;++k)Check(std::abs(p[k]+.5f*(ca[k]+cb[k])-.5f*(ap[k]+aGrip[k]+bp[k]+bGrip[k]))<1e-6f,"unequal span lost midpoint");
    const float zero[3]{},same[3]{.05f,0,0};const auto roll=std::array<float,4>{.5f,0,0,.866025404f};
    Check(TwoHandTarget(zero,roll.data(),same,zero,roll.data(),same,p,q,centre),"coincident grips rejected");
    Check(std::abs(q[0]-.5f)<1e-6f,"coincident grips lost wrist twist");
    const float reversed[4]{-.5f,0,0,-.866025404f};
    Check(TwoHandTarget(zero,roll.data(),same,zero,reversed,same,p,q,centre),"antipodal quaternion rejected");
    Check(std::abs(q[0]-.5f)<1e-6f,"antipodal quaternion lost wrist twist");
    // Vertical lift with both wrists rolled 60 degrees: the grip line supplies
    // swing, while their roll around that line remains in the result.
    const float gaLocal[3]{-.2f,0,0},gbLocal[3]{.2f,0,0};
    const float liftedA[3]{.2f,0,-.2f},liftedB[3]{-.2f,0,.2f};
    Check(TwoHandTarget(liftedA,roll.data(),gaLocal,liftedB,roll.data(),gbLocal,p,q,centre),"vertical two-hand lift rejected");
    const float axis[3]{1,0,0},across[3]{0,1,0};float liftedAxis[3],liftedAcross[3];
    Rotate(q,axis,liftedAxis);Rotate(q,across,liftedAcross);
    Check(std::abs(liftedAxis[0])<1e-6f && std::abs(liftedAxis[2]-1)<1e-6f,"vertical lift did not swing body");
    Check(std::abs(liftedAcross[0]+.866025404f)<1e-6f && std::abs(liftedAcross[1]-.5f)<1e-6f,"vertical lift discarded wrist twist");
    const float badP[3]{std::numeric_limits<float>::infinity(),0,0};
    Check(!TwoHandTarget(badP,still.data(),aGrip,bp,still.data(),bGrip,p,q,centre),"infinite two-hand position accepted");
    std::cout<<"held drive: independent controller rotation, fixed reachable grip point, world translation and invalid input PASS\n";
    std::cout<<"two-hand body: grip-line rotation, hand order, half turn, fixed length, midpoint, coincident grips and invalid input PASS\n";
}
