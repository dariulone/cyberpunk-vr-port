"""The person's solver, cut out of NpcArmGrip.cpp as it is, on a skeleton with the shape of man_base."""
import os, subprocess, tempfile
HERE = os.path.dirname(os.path.abspath(__file__))
R = os.path.abspath(os.path.join(HERE, '..', '..')).replace(os.sep, '/') + '/'
VCVARS = os.environ.get('VCVARS64', r'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat')
WORK = tempfile.mkdtemp(prefix='grip_test')
s = open(R + 'src/Anim/NpcArmGrip.cpp', encoding='utf8').read().replace('\r\n', '\n')
a = s.index('constexpr float kBlendIn')
b = s.index('void ApplyInner(void* a2)')
solver = s[a:b].replace('SRWLOCK lock=SRWLOCK_INIT;\n', '').replace('volatile LONG anyOn=0;\n', '')
rig = open(R + 'src/Anim/CharacterRig.cpp', encoding='utf8').read().replace('\r\n', '\n')
def fn(name):
    i = rig.index('void %s(' % name)
    j = rig.index('\n}\n', i)
    return rig[i:j + 3]
main = r'''
#include <cstdio>
#include <cstdint>
#include <cmath>
#include <cstring>
#include <algorithm>
constexpr int VRIK_MAX_BONES=800,VRIK_TRANS_OFF=0,VRIK_ROT_OFF=16;
''' + fn('VRIK_QuatMul') + fn('VRIK_QuatNorm') + fn('VRIK_QuatRotateVec') + solver + r'''
enum {Root,Hips,Spine,Spine1,Spine2,Spine3,Neck,Neck1,Head,LS,LA,LF,LH,LU,LL,LFoot,N};
static uint8_t buf[N*48];
static void Reset(Grip& g){
    static const struct {int parent;float x,y,z;} b[N]={{-1,0,0,0},{Root,0,0,1},{Hips,0,0,.1f},{Spine,0,0,.1f},{Spine1,0,0,.1f},{Spine2,0,0,.1f},{Spine3,0,0,.12f},{Neck,0,0,.05f},{Neck1,0,0,.08f},
        {Spine3,.05f,0,.08f},{LS,.12f,0,0},{LA,.3f,-.02f,-.02f},{LF,.27f,.02f,0},{Hips,.1f,0,-.05f},{LU,0,.02f,-.45f},{LL,0,-.02f,-.45f}};
    std::memset(buf,0,sizeof(buf));
    for(int i=0;i<N;++i){LocalT(buf,i)[0]=b[i].x;LocalT(buf,i)[1]=b[i].y;LocalT(buf,i)[2]=b[i].z;LocalR(buf,i)[3]=1;g.parents[i]=int16_t(b[i].parent);}
    g.turn[0]=g.turn[1]=g.turn[2]=0;g.turn[3]=1;g.seeded=false;g.miss=0;g.jointCount=0;g.upper=g.mid=g.end=-1;g.chest=g.watch=-1;std::memset(g.done,0,sizeof(g.done));g.doneNext=0;g.again=0;
}
static void Add(Grip& g,int b,float s,float m){g.joints[g.jointCount++]=Joint{b,s,m};}
static void SetArm(Grip& g){g.part=ArmL;g.upper=LA;g.mid=LF;g.end=LH;Add(g,Spine,.08f,.08f);Add(g,Spine1,.12f,.12f);Add(g,Spine2,.18f,.15f);Add(g,Spine3,.25f,.2f);Add(g,LS,.6f,.45f);}
static void SetLeg(Grip& g){g.part=LegL;g.upper=LU;g.mid=LL;g.end=LFoot;}
static void SetHead(Grip& g){g.part=PartHead;g.end=Head;g.chest=Spine3;Add(g,Spine,.02f,.02f);Add(g,Spine1,.03f,.03f);Add(g,Spine2,.05f,.05f);Add(g,Spine3,.08f,.07f);Add(g,Neck,.5f,.25f);Add(g,Neck1,.7f,.3f);}
static void SetChest(Grip& g){g.part=PartChest;g.end=Spine3;g.watch=Spine2;Add(g,Spine,.3f,.3f);Add(g,Spine1,.5f,.35f);Add(g,Spine2,.7f,.4f);}
static float Case(const char* name,void(*set)(Grip&),float dx,float dy,float dz,float weight=1.f,const float* turn=nullptr){
    static Grip g;
    Reset(g);set(g);
    Tr was;ModelOf(g,buf,g.end,was);
    // seeded on a first pass at the slot's spot, 2 cm off the bone
    g.target[0]=was.p[0]+.02f;g.target[1]=was.p[1];g.target[2]=was.p[2];
    Solve(g,buf,0.f);
    if(turn)std::copy_n(turn,4,g.turn);
    g.target[0]+=dx;g.target[1]+=dy;g.target[2]+=dz;
    float biggestSeed=0;
    Solve(g,buf,weight);
    Tr now;ModelOf(g,buf,g.end,now);
    const float want[3]={was.p[0]+dx,was.p[1]+dy,was.p[2]+dz};
    float hips=0;Tr h;ModelOf(g,buf,Hips,h);hips=std::fabs(h.p[2]-1.f);
    float q=0;for(int i=0;i<N;++i){const float* r=LocalR(buf,i);const float l=std::sqrt(r[0]*r[0]+r[1]*r[1]+r[2]*r[2]+r[3]*r[3]);q=std::max(q,std::fabs(l-1.f));}
    Tr sp;ModelOf(g,buf,Spine3,sp);
    printf("%-26s want %.3f %.3f %.3f now %.3f %.3f %.3f off %.3f miss %.3f; hips moved %.3f, chest moved %.3f, quat error %.5f, end turn w %.3f\n",name,want[0],want[1],want[2],now.p[0],now.p[1],now.p[2],Apart(want,now.p),g.miss,hips,std::sqrt((sp.p[0])*(sp.p[0])+sp.p[1]*sp.p[1]+(sp.p[2]-1.4f)*(sp.p[2]-1.4f)),q,now.q[3]);
    (void)biggestSeed;
    return Apart(want,now.p);
}
// The degrees a head has come from how it started, after that many passes: once = through SolveOnce, as the hook does; otherwise a pose that keeps what was written and is solved again regardless.
static float HeadAfter(const char* name,const float* turn,int passes,bool once,bool freshPose=false){
    static Grip g;
    static uint8_t start[N*48];
    Reset(g);SetHead(g);
    std::memcpy(start,buf,sizeof(buf));
    Tr was;ModelOf(g,buf,Head,was);
    g.target[0]=was.p[0];g.target[1]=was.p[1];g.target[2]=was.p[2];
    Solve(g,buf,0.f);
    std::copy_n(turn,4,g.turn);
    for(int f=0;f<3;++f){
        if(freshPose)std::memcpy(buf,start,sizeof(buf));
        for(int i=0;i<passes;++i){if(once)SolveOnce(g,buf,1.f);else Solve(g,buf,1.f);}
    }
    Tr now;ModelOf(g,buf,Head,now);
    const float deg=AngleBetween(was.q,now.q)*57.2958f;
    printf("%-44s %6.1f degrees, %u passes skipped\n",name,deg,g.again);
    return deg;
}
int main(){
    const float t30[4]={0,0,std::sin(.26f),std::cos(.26f)};
    int bad=0;
    bad+=Case("still arm",SetArm,0,0,0)>.002f;
    bad+=Case("arm forward 20 cm",SetArm,-.1f,.2f,0)>.002f;
    bad+=Case("arm up 30 cm",SetArm,-.2f,0,.3f)>.002f;
    Case("arm pulled 30 cm past",SetArm,.3f,0,0);
    Case("arm pulled 60 cm past",SetArm,.6f,0,0);
    bad+=Case("leg forward 30 cm",SetLeg,0,.3f,.1f)>.002f;
    Case("leg pulled down 40 cm",SetLeg,0,0,-.4f);
    bad+=Case("still head",SetHead,0,0,0)>.002f;
    bad+=Case("head forward 8 cm",SetHead,0,.08f,0)>.03f;
    Case("head aside 15 cm",SetHead,.15f,0,0);
    Case("head pulled 50 cm",SetHead,0,.5f,0);
    Case("head turned 30 deg",SetHead,0,0,0,1.f,t30);
    const float t150[4]={0,0,std::sin(1.3f),std::cos(1.3f)};
    Case("head turned 150 deg about neck",SetHead,0,0,0,1.f,t150);
    const float n150[4]={std::sin(1.3f),0,0,std::cos(1.3f)};
    Case("head nodded 150 deg",SetHead,0,0,0,1.f,n150);
    const float r150[4]={0,std::sin(1.3f),0,std::cos(1.3f)};
    Case("head rolled 150 deg",SetHead,0,0,0,1.f,r150);
    Case("head down 20 cm",SetHead,0,0,-.2f);
    Case("head down 50 cm",SetHead,0,.05f,-.5f);
    Case("head down 80 cm",SetHead,.02f,0,-.8f);
    Case("head behind 30 cm",SetHead,0,-.3f,0);
    Case("chest down 40 cm",SetChest,0,.05f,-.4f);
    bad+=Case("chest forward 10 cm",SetChest,0,.1f,0)>.03f;
    Case("chest pulled 50 cm",SetChest,0,.5f,0);
    Case("arm half blended",SetArm,-.1f,.2f,0,.5f);
    const float t60[4]={0,0,std::sin(.52f),std::cos(.52f)};
    const float t170[4]={0,0,std::sin(1.48f),std::cos(1.48f)};
    const float n170[4]={std::sin(1.48f),0,0,std::cos(1.48f)};
    bad+=std::fabs(HeadAfter("hand 60 deg, 1 pass",t60,1,true)-42.f)>3.f;
    bad+=std::fabs(HeadAfter("hand 60 deg, same pose 4 times, once",t60,4,true)-42.f)>3.f;
    bad+=HeadAfter("hand 170 deg, same pose 4 times, once",t170,4,true)>48.f;
    bad+=HeadAfter("hand 170 deg, solved 4 times regardless",t170,4,false)>48.f;
    bad+=HeadAfter("hand nod 170 deg, solved 4 times regardless",n170,4,false)>28.f;
    bad+=HeadAfter("hand 170 deg, new pose each frame, 4 passes",t170,4,true,true)>48.f;
    printf("bad %d\n",bad);
    return bad;
}
'''
open(os.path.join(WORK, 'grip_test.cpp'), 'w', encoding='utf8').write(main)
bat = os.path.join(WORK, 'grip_test.bat')
open(bat, 'w').write('@echo off\r\ncall "%s" >nul\r\ncd /d "%s"\r\ncl /nologo /EHsc /std:c++17 /O1 grip_test.cpp /Fe:grip_test.exe >grip_build.txt 2>&1\r\nif errorlevel 1 (type grip_build.txt & exit /b 1)\r\n' % (VCVARS, WORK))
r = subprocess.run(['cmd', '/c', bat], capture_output=True, text=True, errors='replace')
if r.returncode:
    raise SystemExit(r.stdout[-5000:] + r.stderr[-1000:])
# Run directly: under cmd's pipe the output of the harness was lost.
raise SystemExit(subprocess.run([os.path.join(WORK, 'grip_test.exe')]).returncode)
