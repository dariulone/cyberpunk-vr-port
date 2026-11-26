"""Extract the production hold parser/selection into a C++ fixture. All pose arithmetic runs in that fixture.
Usage: support_selection_test.py OUTPUT.cpp
"""
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[2]
s=(ROOT/'src/Anim/TwoHandGrip.cpp').read_text(encoding='utf-8')
def between(start,end):
 a=s.index(start);return s[a:s.index(end,a)]
# SelectHold includes the exact production name ordering, aliases and magazine-index mapping.
parts=[between('struct Hold {','// The support point, recomputed'),
       between('Hold* FindHold(','// THE HOLD AS THE WEAPON HAND HAS IT.')]
text=r'''#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>
char CyberpunkVR_WeaponName[64]{};
int CyberpunkVR_DebugTwoHandHoldIdx=-1,CyberpunkVR_DebugTwoHandHave=0,CyberpunkVR_TwoHandActive=0;
void VRIK_QuatNorm(float* q){float n=0;for(int i=0;i<4;++i)n+=q[i]*q[i];n=std::sqrt(n);for(int i=0;i<4;++i)q[i]/=n;}
const char* WeaponKey(){return CyberpunkVR_WeaponName;}
'''+''.join(parts)+r'''
void check(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
int main(int argc,char** argv)try{
 check(argc==2,"grips folder required");
 for(const auto& e:std::filesystem::directory_iterator(argv[1])){
  auto n=e.path().filename().string();const std::string prefix="CyberpunkVR_TwoHandGrip_";
  if(n.rfind(prefix,0)!=0 || e.path().extension()!=".ini")continue;
  n=n.substr(prefix.size(),n.size()-prefix.size()-4);auto* hold=AddHold(n.c_str());check(hold,"hold capacity exceeded");
  FILE* f=nullptr;fopen_s(&f,e.path().string().c_str(),"r");check(f,"pose open failed");ParseInto(hold,f);fclose(f);
 }
 unsigned weapons=0,points=0;
 for(int i=0;i<g_holdCount;++i){
  if(std::strchr(g_holds[i].weapon,'@') || !g_holds[i].fingerCount)continue;
  strcpy_s(CyberpunkVR_WeaponName,g_holds[i].weapon);SelectHold();++weapons;
  for(int j=0;j<g_candN;++j){
   float best=1e9f;int nearest=-1;
   for(int k=0;k<g_candN;++k){float d=0;for(int axis=0;axis<3;++axis){const float x=g_cand[k]->off[axis]-g_cand[j]->off[axis];d+=x*x;}if(d<best){best=d;nearest=k;}}
   check(nearest==j,"support candidate unreachable or duplicate");++points;
   if(std::strstr(g_cand[j]->weapon,"@mag"))check(j==1,"magazine suppression no longer names slot one");
  }
  const std::string forward=std::string(CyberpunkVR_WeaponName)+"@z_forward";
  if(FindHold(forward.c_str()))check(g_candN>=3,"generated alternate not selected");
 }
 std::cout<<"PASS production parser/selector: "<<g_holdCount<<" loaded, "<<weapons<<" primary weapons, "<<points<<" reachable candidates\n";return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
'''
Path(sys.argv[1]).write_text(text,encoding='utf-8')
print('Generated C++ production selection fixture')
