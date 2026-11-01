from pathlib import Path
import subprocess,tempfile
p=(Path(__file__).resolve().parents[2]/'src/Overlay/OverlayDebugDraw.cpp');s=p.read_text();start=s.index('                const int mainEye',s.index('        if (dl) {',s.index('void DrawBarrelCrosshair()')));end=s.index('            } else {',start);body=s[start:end]
code=r'''
#include <vector>
#include <cassert>
#include <cmath>
struct ImVec2 {float x,y;ImVec2(float a=0,float b=0):x(a),y(b){}};
#define IM_COL32(a,b,c,d) 0
struct Draw {std::vector<ImVec2> dots,starts,ends;void PushClipRect(ImVec2 a,ImVec2 b,bool){starts.push_back(a);ends.push_back(b);}void PopClipRect(){}void AddCircleFilled(ImVec2 p,float,int){dots.push_back(p);}};
int CyberpunkVR_MainIsRightEye=1;
float CyberpunkVR_BarrelDotNdcX=.25f,CyberpunkVR_BarrelDotNdcX2=-.25f,CyberpunkVR_BarrelDotNdcY=.2f;
struct AsymEyeWindows{int eye[2]{0,1};};
namespace cvr::projection{bool Active(){return true;}float MapNdcY(float x,int,int){return x+.1f;}}
struct OpenXRManager{static OpenXRManager& Get(){static OpenXRManager m;return m;}bool GetAsymmetricEyeWindows(AsymEyeWindows*){return true;}};
Draw run(int main){CyberpunkVR_MainIsRightEye=main;Draw d;auto* dl=&d;const ImVec2 displaySize(3144,3000),scRight(0,1200);bool worldOk=true;float rad=3;
'''+body+r'''
return d;}
int main(){for(int main=0;main<2;++main){auto d=run(main);assert(d.dots.size()==2);for(int eye=0;eye<2;++eye){assert(d.starts[eye].x==eye*3144);assert(d.ends[eye].x==(eye+1)*3144);float local=d.dots[eye].x-eye*3144;assert(std::fabs(local-(eye==main?1965.f:1179.f))<.01f);assert(std::fabs(d.dots[eye].y-(eye==main?1200.f:1050.f))<.01f);}}}
'''
with tempfile.TemporaryDirectory() as d:
 cpp=Path(d)/'test.cpp';exe=Path(d)/'test';cpp.write_text(code)
 subprocess.run(['g++','-std=c++17',str(cpp),'-o',str(exe)],check=True);subprocess.run([str(exe)],check=True)
print('Actual SBS draw block: both main-eye assignments, separate viewports/clips, X/Y eye coordinates: PASS')
