#include "Natives/HeldGripPose.hpp"
#include <cstdio>
#include <cstdlib>
#include <limits>
using namespace cvr::held_grip;
static void Check(bool ok,const char* why){if(!ok){std::fprintf(stderr,"FAIL %s\n",why);std::exit(1);}}
static bool Same(const float* a,const float* b){float d=0;for(int k=0;k<4;++k)d+=a[k]*b[k];return std::fabs(d)>0.99999f;}
static void Axis(float degrees,float* q){float t=degrees*3.14159265358979323846f/360; q[0]=std::sin(t);q[1]=q[2]=0;q[3]=std::cos(t);}
int main(){
    unsigned cases=0;
    for(float bodyTurn:{0.f,45.f,90.f,150.f,179.f,-150.f})
      for(float relation:{0.f,37.f,-70.f})
       for(float latestController:{0.f,60.f,170.f,-120.f}){
        float body[4],rel[4],wrist[4],draw[4],pos[3]={1,2,3},seat[3]={.09f,-.04f,.07f};
        Axis(bodyTurn,body);Axis(relation,rel);Axis(latestController,draw);
        float oldGrip[3];Rotate(draw,seat,oldGrip);for(int k=0;k<3;++k)oldGrip[k]+=pos[k];
        Check(WristRotation(body,rel,wrist),"valid measured wrist refused");
        Check(SeatRotation(pos,draw,wrist,seat,1),"seat correction refused");
        float reproduced[4];Multiply(draw,rel,reproduced);Check(Same(body,reproduced),"held part slipped when controller turned");
        float grip[3];Rotate(draw,seat,grip);for(int k=0;k<3;++k)Check(std::fabs(grip[k]+pos[k]-oldGrip[k])<1e-5f,"correction moved grip point");
        ++cases;
    }
    // Two different grips on the same part need different wrist rotations, not a shared average error.
    float body[4],a[4],b[4],wa[4],wb[4],check[4];Axis(150,body);Axis(30,a);Axis(-55,b);
    Check(WristRotation(body,a,wa)&&WristRotation(body,b,wb),"two-hand solve");
    Multiply(wa,a,check);Check(Same(check,body),"first grip slipped");Multiply(wb,b,check);Check(Same(check,body),"second grip slipped");
    const float bad[4]={std::numeric_limits<float>::quiet_NaN(),0,0,1};Check(!WristRotation(bad,a,wa),"NaN accepted");
    Check(std::fabs(PinnedForearmLength(.44f,.21f,.33f,true)-.44f)<1e-6f,"held wrist shortened after load correction");
    Check(std::fabs(PinnedForearmLength(.12f,.21f,.33f,true)-.12f)<1e-6f,"held wrist pushed past grip by minimum length");
    Check(std::fabs(PinnedForearmLength(.44f,.21f,.33f,false)-.33f)<1e-6f,"free arm extension cap lost");
    Check(std::fabs(PinnedForearmLength(.12f,.21f,.33f,false)-.21f)<1e-6f,"free arm minimum lost");
    // A 60-degree controller turn must reach the body, not just keep a frozen wrist aligned to it.
    for(float hz:{45.f,60.f,90.f,120.f})for(float inertia:{.02f,.15f,.8f}) {
        const float dt=1/hz,target=1.04719755f,inertias[3]={inertia,inertia,inertia};
        float angle=0,velocity[3]={},peak=0;
        for(int i=0;i<int(hz*2);++i) {
            const float error[3]={target-angle,0,0};
            DriveAngular(error,velocity,inertias,25.f,dt,40.f,velocity);
            angle+=velocity[0]*dt;peak=std::max(peak,angle);
        }
        Check(std::abs(angle-target)<.01f,"held part did not follow 60-degree wrist turn");
        Check(peak<target+.12f,"held part oscillates past angular target");
        const float error[3]={3,0,0},zero[3]={};float capped[3];
        DriveAngular(error,zero,inertias,25.f,dt,40.f,capped);
        Check(capped[0]<=25.f/inertia*dt+1e-5f,"grip exceeded physical angular acceleration");
        DriveAngular(error,zero,inertias,0,dt,40.f,capped);
        Check(capped[0]==0,"zero torque turned the part");
    }
    std::printf("PASS %u measured body/controller/relative rotations, independent grips, pin bounds, angular following at 45/60/90/120 Hz\n",cases);
}
