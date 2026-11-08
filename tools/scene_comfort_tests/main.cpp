// Scene camera comfort (include/Camera/SceneCameraComfort.hpp): synthetic scene camera tracks -- handheld sway,
// slow and whip pans, cuts by turn and by jump, a montage of short shots -- at several frame rates.
#include "Camera/SceneCameraComfort.hpp"
#include <cstdio>
#include <stdexcept>
#include <vector>
using namespace cvr::camera;
using Quat=SceneCameraComfort::Quat;

static void Check(bool ok,const char* why) { if(!ok)throw std::runtime_error(why); }

// yaw about z (up), pitch about x (right), roll about y (forward): the game's camera axes.
static Quat Euler(float yawDeg,float pitchDeg,float rollDeg) {
    const float k=.00872664626f;
    const Quat y{0,0,std::sin(yawDeg*k),std::cos(yawDeg*k)},p{std::sin(pitchDeg*k),0,0,std::cos(pitchDeg*k)},
        r{0,std::sin(rollDeg*k),0,std::cos(rollDeg*k)};
    auto mul=[](const Quat& a,const Quat& b)->Quat {
        return {a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1],a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0],
                a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3],a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]};
    };
    return mul(mul(y,p),r);
}
static float Ang(const Quat& a,const Quat& b) { return SceneCameraComfort::AngleDeg(a,b); }

static void HandheldSwayIsAbsorbed() {
    for(int fps:{30,45,60,90,144}) {
        SceneCameraComfort f;SceneComfortSettings s;const float dt=1.0f/fps;
        Quat first{};float moved=0;
        for(int i=0;i<fps*10;++i) {
            const float t=i*dt;
            const Quat scene=Euler(9*std::sin(t*3.1f),6*std::sin(t*4.7f+1),4*std::sin(t*2.3f+2));
            const float pos[3]={.05f*std::sin(t*2),.03f*std::sin(t*3),1.6f};
            const Quat view=f.Step(scene,pos,dt,s);
            if(i==0)first=view;
            moved=std::max(moved,Ang(view,first));
            Check(f.LagDeg()<=s.deadDeg+.01f,"sway inside the dead zone was followed");
        }
        Check(moved<.01f,"handheld sway within 30 deg turned the view");
        Check(f.Cuts()==0,"sway was taken for a cut");
    }
}

static void SlowAndLongPans() {
    for(int fps:{30,60,90,144}) {
        SceneCameraComfort f;SceneComfortSettings s;const float dt=1.0f/fps;
        // 25 deg over 2 s: never reaches the edge
        Quat start{};
        for(int i=0;i<=fps*2;++i) {
            const Quat v=f.Step(Euler(25.0f*i/(fps*2),0,0),nullptr,dt,s);
            if(i==0)start=v;
        }
        Check(Ang(f.Step(Euler(25,0,0),nullptr,dt,s),start)<.01f,"a 25 deg pan moved the view");
        // a 120 deg pan at 30 deg/s: follows past the edge, smooth, gap bounded
        f.Reset();
        float prevStep=0,maxAccel=0,maxLag=0;Quat prev=f.Step(Euler(0,0,0),nullptr,dt,s);
        for(int i=1;i<=fps*4;++i) {
            const Quat v=f.Step(Euler(30.0f*i/fps,0,0),nullptr,dt,s);
            const float stepDeg=Ang(v,prev);prev=v;
            maxAccel=std::max(maxAccel,std::fabs(stepDeg-prevStep)/dt/dt);prevStep=stepDeg;
            maxLag=std::max(maxLag,f.LagDeg());
        }
        Check(maxLag<=s.deadDeg+s.maxExtraDeg+.01f,"pan lag exceeded the cap");
        Check(maxLag>s.deadDeg,"pan never crossed the dead zone");
        // the view's own speed ramps rather than jumping to the pan rate (deg/s^2)
        Check(maxAccel<400.0f,"the view started following with a jolt");
        // steady pan: the view turns at the scene's rate
        Check(std::fabs(prevStep/dt-30.0f)<2.0f,"the view does not keep up with a steady pan");
        std::printf("fps %3d  pan 30 deg/s: lag %.1f deg, peak view acceleration %.0f deg/s^2\n",fps,maxLag,maxAccel);
    }
}

static void WhipPanIsCapped() {
    SceneCameraComfort f;SceneComfortSettings s;const float dt=1.0f/60;
    f.Step(Euler(0,0,0),nullptr,dt,s);
    float maxLag=0;
    for(int i=1;i<=27;++i) {   // 180 deg in 0.45 s, 6.7 deg a frame: a pan, not a cut
        f.Step(Euler(180.0f*i/27,0,0),nullptr,dt,s);
        maxLag=std::max(maxLag,f.LagDeg());
    }
    Check(f.Cuts()==0,"a whip pan was taken for a cut");
    Check(maxLag<=s.deadDeg+s.maxExtraDeg+.01f,"a whip pan left the subject behind");
}

static void CutsSnap() {
    SceneCameraComfort f;SceneComfortSettings s;const float dt=1.0f/60;
    const float p0[3]={0,0,0},p1[3]={3,1,0},p2[3]={3.1f,1,0};
    f.Step(Euler(0,0,0),p0,dt,s);
    Quat v=f.Step(Euler(20,0,0),p0,dt,s);
    Check(Ang(v,Euler(0,0,0))<.01f,"setup: a 20 deg turn was followed");
    v=f.Step(Euler(110,-10,0),p0,dt,s);   // 90 deg in one frame
    Check(Ang(v,Euler(110,-10,0))<.01f && f.Cuts()==1,"a cut by turn was eased instead of taken");
    v=f.Step(Euler(120,-10,0),p1,dt,s);   // 10 deg and 3 m: a new shot
    Check(Ang(v,Euler(120,-10,0))<.01f && f.Cuts()==2,"a cut by jump was eased instead of taken");
    v=f.Step(Euler(125,-10,0),p2,dt,s);   // 10 cm on: same shot
    Check(Ang(v,Euler(120,-10,0))<.01f && f.Cuts()==2,"a dolly was taken for a cut");
}

static void OffIsPassThrough() {
    SceneCameraComfort f;SceneComfortSettings s;s.deadDeg=0;
    for(int i=0;i<100;++i) {
        const Quat scene=Euler(i*3.0f,std::sin(i*.1f)*10,0);
        Check(Ang(f.Step(scene,nullptr,1.0f/60,s),scene)<.001f,"switched off, the filter changed the camera");
    }
}

static void FrameRateIndependent() {
    float lagAt[4]{};int k=0;
    for(int fps:{30,60,90,144}) {
        SceneCameraComfort f;SceneComfortSettings s;const float dt=1.0f/fps;
        for(int i=0;i<=fps*2;++i)f.Step(Euler(40.0f*i/fps,0,0),nullptr,dt,s);   // 80 deg at 40 deg/s
        lagAt[k++]=f.LagDeg();
    }
    for(int i=1;i<4;++i)Check(std::fabs(lagAt[i]-lagAt[0])<1.0f,"the catch-up depends on the frame rate");
}

static void YawThroughBehind() {
    SceneCameraComfort f;SceneComfortSettings s;const float dt=1.0f/60,d=.01745329252f;
    float y=f.StepYaw(170*d,dt,s);
    for(int i=1;i<=120;++i)y=f.StepYaw((170+i*.5f)*d,dt,s);   // to 230 deg, through 180
    // the scene ended 60 deg on; the view is within the dead zone of it
    float diff=std::remainder(y-230*d,6.28318531f)/d;
    Check(std::fabs(diff)<=s.deadDeg+s.maxExtraDeg+.01f && std::fabs(diff)>=s.deadDeg-.5f,"yaw wrapped wrongly through 180 deg");
}

// The montage's shape: shots of 0.5-1.7 s, each with a pan and sway, a cut between them.
static void MontageShape() {
    SceneCameraComfort f;SceneComfortSettings s;const float dt=1.0f/60;
    const float shots[][4]={{0,.97f,15,0},{80,.47f,40,5},{-30,1.07f,-25,0},{150,.67f,60,-10},{20,.83f,10,0},
                            {-100,.57f,-35,8},{60,1.63f,45,0},{-170,1.23f,-20,-5},{10,1.67f,30,0}};
    float sceneSpeed=0,viewSpeed=0,sceneTurn=0,viewTurn=0;Quat prevScene{},prevView{};bool have=false;int expectedCuts=0;
    float pos[3]={0,0,0};
    for(const auto& shot:shots) {
        const int n=int(shot[1]/dt);
        for(int i=0;i<n;++i) {
            const float t=i*dt;
            const Quat scene=Euler(shot[0]+shot[2]*t/shot[1]+3*std::sin(t*5),shot[3]+2*std::sin(t*4),1.5f*std::sin(t*3));
            const Quat view=f.Step(scene,pos,dt,s);
            if(have && i>0) {
                sceneSpeed=std::max(sceneSpeed,Ang(scene,prevScene)/dt);
                viewSpeed=std::max(viewSpeed,Ang(view,prevView)/dt);
                sceneTurn+=Ang(scene,prevScene);viewTurn+=Ang(view,prevView);
            }
            prevScene=scene;prevView=view;have=true;
        }
        pos[0]+=5;++expectedCuts;
    }
    Check(int(f.Cuts())==expectedCuts-1,"montage cuts were not all taken as cuts");
    std::printf("montage: in-shot turning %.0f deg scene, %.0f deg view; peak rate %.0f deg/s scene, %.0f deg/s view\n",
                sceneTurn,viewTurn,sceneSpeed,viewSpeed);
    Check(viewTurn<sceneTurn*.5f,"the montage's in-shot turning was not reduced");
    Check(viewSpeed<=sceneSpeed+.5f,"the view turned faster than the scene camera");
}

int main() try {
    HandheldSwayIsAbsorbed();
    SlowAndLongPans();
    WhipPanIsCapped();
    CutsSnap();
    OffIsPassThrough();
    FrameRateIndependent();
    YawThroughBehind();
    MontageShape();
    std::puts("scene camera comfort: all passed");
    return 0;
} catch(const std::exception& e) {
    std::fprintf(stderr,"FAILED: %s\n",e.what());
    return 1;
}
