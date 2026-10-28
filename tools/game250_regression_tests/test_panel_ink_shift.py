"""Exercise the production InkShift function with flat-panel/gameplay transitions."""
from pathlib import Path
import subprocess
import tempfile
s=(Path(__file__).resolve().parents[2]/'src/Hooks/WorldMarkers.cpp').read_text()
start=s.index('bool InkShift(');end=s.index('{',start)+1;depth=1
while depth:
    depth+=(s[end]=='{')-(s[end]=='}');end+=1
code=r'''
#include <atomic>
#include <cmath>
#include <cassert>
std::atomic<int> CyberpunkVR_AsymHudShift{1};
int menu=0,rect=0,g_launcherWidth=3000,g_launcherHeight=3000;
bool panel=false;
int GetMenuMode(){return menu;} int GetMenuRectMode(){return rect;}
namespace cvr::projection {
struct ViewWindow {float halfX=1,halfY=1;};
bool ViewWindowOf(int,ViewWindow*){return true;}
}
namespace cvr::engine_stereo {
bool OnePicture(){return panel;} bool UiOneEye(){return true;}
}
bool EngineEyeWindow(int,cvr::projection::ViewWindow&){return true;}
'''+s[start:end]+r'''
int main(){
 cvr::projection::ViewWindow w;float squeeze=1;
 assert(InkShift(0,0,3000,3000,w,squeeze)); // gameplay still gets HUD correction
 panel=true;assert(!InkShift(0,0,3000,3000,w,squeeze)); // loading, native menu mode already zero
 panel=false;menu=1;assert(!InkShift(0,0,3000,3000,w,squeeze));
 menu=0;rect=1;assert(!InkShift(0,0,3000,3000,w,squeeze));
 rect=0;assert(InkShift(0,1,3000,3000,w,squeeze)); // resume gameplay
}
'''
with tempfile.TemporaryDirectory() as d:
    cpp=Path(d)/'test.cpp';exe=Path(d)/'test';cpp.write_text(code)
    subprocess.run(['g++','-std=c++17',str(cpp),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)
print('Production InkShift: loading/frontend/forced panels bypass correction; gameplay retains it: PASS')
