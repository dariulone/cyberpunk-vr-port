from pathlib import Path
import subprocess
import tempfile
s=(Path(__file__).resolve().parents[2] / 'src/Stereo/EngineStereoSdk.cpp').read_text()
def extract(name):
 start=s.index('void '+name+'()');end=s.index('{',start)+1;depth=1
 while depth:
  depth += (s[end]=='{')-(s[end]=='}');end+=1
 return s[start:end]
code='''#include <atomic>
#include <cassert>
std::atomic<int> CyberpunkVR_EngineStereoMainMenus{},CyberpunkVR_EngineStereoArmed{};
std::atomic<bool> g_armed{};
bool enabled=true;int writes=0;
bool Enabled(){return enabled;}
void WriteCvars(bool b){assert(b);++writes;}
void Log(const char*,...){}
'''+extract('NoteMainMenu')+extract('NoteMainMenuClosed')+'''
int main(){
 NoteMainMenu();assert(!g_armed && writes==0);
 NoteMainMenu();NoteMainMenuClosed();assert(!g_armed && writes==0);
 NoteMainMenuClosed();assert(g_armed && writes==1 && CyberpunkVR_EngineStereoMainMenus==0);
 NoteMainMenuClosed();assert(writes==1 && CyberpunkVR_EngineStereoMainMenus==0);
 NoteMainMenu();NoteMainMenuClosed();assert(writes==1);
 enabled=false;NoteMainMenu();NoteMainMenuClosed();assert(writes==1 && CyberpunkVR_EngineStereoMainMenus==0);
}
'''
with tempfile.TemporaryDirectory() as d:
    cpp=Path(d)/'test.cpp';exe=Path(d)/'test';cpp.write_text(code)
    subprocess.run(['g++','-std=c++17',str(cpp),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)
print('Production frontend callbacks: nested lifetimes, no premature arming, no duplicate activation, no underflow: PASS')
