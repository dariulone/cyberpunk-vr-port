// Offline measurement of the stock Cyberpunk 2077 2.5 Overture animation.
// Usage: overture_center <exported base1.anims.glb>. Requires nlohmann/json.
// Read native sample keys, not a blend evaluated at an approximate timestamp.
/* Provenance and reproduction (PowerShell, x64 VS Developer shell; Python with lupa).
   Retail 2.5, exe 3.0.5813403, cp77tools 8.19.0, basegame_4_animation.archive.
   Player weapon-component set, not the NPC or mirrored sets or a player hand clip.
   SHA256 of extracted base1.anims: 9391fcfb732ba830a1c333ceb57cb822f1671921f8a616edbc428e31e669216f
   SHA256 of extracted base1.rig:   f1fac907b11c66145e925e7fdabf1ce3ee6c75cf1ba3ce91f44f2e16a4b10695
   Exported GLB: 423f377da8287f78664488faeef8a29870ccc785fd9ff91fcfb7385ce7043e5d
   A resolver pointed ONLY at this archive produced a byte-identical GLB.

   $r = 'C:/Users/dariulone/Desktop/CyberpunkVRPort'
   $g = 'C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077'
   $w = Join-Path $env:TEMP ('overture-' + [guid]::NewGuid().ToString('N'))
   $dirs = 'resource','rig','export','build' | ForEach-Object { Join-Path $w $_ }; New-Item -ItemType Directory -Path $dirs | Out-Null
   $archive = "$g/archive/pc/content/basegame_4_animation.archive"
   $dir = 'base/animations/weapon/firearms/revolver/malorian_overture'; $stem = 'w_revolver__malorian_overture__base1'
   cp77tools unbundle $archive -r '^base[\\/]animations[\\/]weapon[\\/]firearms[\\/]revolver[\\/]malorian_overture[\\/]w_revolver__malorian_overture__base1\.anims$' -o "$w/resource"
   cp77tools unbundle $archive -r '^base[\\/]weapons[\\/]firearms[\\/]revolver[\\/]malorian_overture[\\/]rig[\\/]w_revolver__malorian_overture__base1\.rig$' -o "$w/rig"
   Get-FileHash "$w/resource/$dir/$stem.anims", "$w/rig/base/weapons/firearms/revolver/malorian_overture/rig/$stem.rig"
   cp77tools export "$w/resource/$dir/$stem.anims" -gp $g -o "$w/export"
   $glb = "$w/export/$stem.anims.glb"; Get-FileHash $glb
   cl /nologo /O2 /EHsc /std:c++17 "/I$r/tools/lefthanded_anims/vendor" "$r/docs/evidence/vr-feel-20261118/overture_center.cpp" "/Fo$w/build/ref.obj" "/Fe$w/build/ref.exe"
   & "$w/build/ref.exe" $glb
   py -3.14 -I "$r/tools/game250_regression_tests/test_crane_rest.py" --measure "$w/build/ref.exe" $glb

   Frame: game RED barrel_plug rigid axes, +X across, +Y forward, +Z up.
   Point: origin of cylinder, NOT origin of magazine_open (which stays put).
   Unrelated closed take: dense idle_break_01 keys 0/26/52, not a bind-pose guess.
   CPVR capture: optionally append the original log path. On the capture machine:
   $log = "$env:TEMP/cpvr-goal-vrfeel/overture-simulator.log"
   py -3.14 -I "$r/tools/game250_regression_tests/test_crane_rest.py" --measure "$w/build/ref.exe" $glb $log
   Lines 24/26/28: gap (hand minus target), fixed simulated hands, seated mag,
   unchanged hold offsets, no spin. Negating the change gives (-36,0,-22) mm
   on weapon entity axes. This measures a mag_slot-based target, not the center
   directly; compare it to the independently computed last column. Overture's
   vrp_mag_slot and vrp_cylinder have zero relative translation on their bones.
   The captured frame pair is overture.jpg alongside this file.
*/
#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <stdexcept>
#include <vector>
using J = nlohmann::json;
using V = std::array<double,3>;
using Q = std::array<double,4>;
static void check(bool ok, const char* why) { if (!ok) throw std::runtime_error(why); }
static V add(V a,V b) {return {a[0]+b[0],a[1]+b[1],a[2]+b[2]};}
static V sub(V a,V b) {return {a[0]-b[0],a[1]-b[1],a[2]-b[2]};}
static V cross(V a,V b) {return {a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]};}
static V mul(double k,V a) {return {k*a[0],k*a[1],k*a[2]};}
static V rotate(Q q,V v) {
    double n=std::sqrt(q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3]);
    check(std::abs(n-1)<0.001,"non-unit rotation"); for(auto& s:q) s/=n;
    V u{q[0],q[1],q[2]}, t=mul(2,cross(u,v)); return add(v,add(mul(q[3],t),cross(u,t)));
}
static V red(V g) {return {g[0],-g[2],g[1]};} // glTF basis -> RED basis, also for the vector part of a quaternion.
struct Pose { V t{}; Q q{0,0,0,1}; V apply(V v) const {return add(t,rotate(q,v));} };
struct Glb {
    J j; std::vector<char> bin;
    explicit Glb(const char* path) {
        std::ifstream f(path,std::ios::binary); f.exceptions(std::ios::badbit|std::ios::failbit);
        auto word=[&]{std::uint32_t v; f.read(reinterpret_cast<char*>(&v),4); return v;};
        check(word()==0x46546c67 && word()==2,"not GLB 2"); word();
        auto len=word(); check(word()==0x4e4f534a,"missing JSON chunk"); std::string text(len,' '); f.read(text.data(),len); j=J::parse(text);
        len=word(); check(word()==0x004e4942,"missing BIN chunk"); bin.resize(len); f.read(bin.data(),len);
    }
    int node(const char* name) const {
        auto& ns=j.at("nodes"); for(int i=0;i<int(ns.size());++i) if(ns.at(i).value("name","")==name) return i;
        throw std::runtime_error(name);
    }
    template<size_t N> std::array<double,N> read(int idx,int frame) const {
        const auto& a=j.at("accessors").at(idx); const auto& v=j.at("bufferViews").at(a.at("bufferView").get<int>());
        check(a.at("componentType")==5126 && a.at("type")== (N==1?"SCALAR":N==3?"VEC3":"VEC4") && !a.contains("sparse") && v.at("buffer")==0,"unexpected accessor");
        check(frame>=0 && frame<a.at("count").get<int>(),"missing key");
        auto start=a.value("byteOffset",size_t(0))+frame*v.value("byteStride",N*sizeof(float));
        check(start+N*4<=v.at("byteLength").get<size_t>(),"bad buffer view"); start+=v.value("byteOffset",size_t(0)); check(start+N*4<=bin.size(),"bad buffer");
        std::array<double,N> out; for(size_t i=0;i<N;++i) {float f; std::memcpy(&f,&bin[start+4*i],4);check(std::isfinite(f),"nonfinite key");out[i]=f;} return out;
    }
    Pose pose(const J& clip,int node,int frame,double& time) const {
        Pose p; int seen=0;
        for(const auto& c:clip.at("channels")) {
            auto& tgt=c.at("target"); if(tgt.at("node")!=node) continue;
            auto& s=clip.at("samplers").at(c.at("sampler").get<int>()); int out=s.at("output"); double t=read<1>(s.at("input"),frame)[0];
            if(time<0) time=t; check(std::abs(time-t)<1e-6,"not synchronous native keys");
            if(tgt.at("path")=="translation") {p.t=read<3>(out,frame);seen|=1;}
            if(tgt.at("path")=="rotation") {p.q=read<4>(out,frame);seen|=2;}
            if(tgt.at("path")=="scale") {check(read<3>(out,frame)==V{1,1,1},"scaled chain");seen|=4;}
        }
        check(seen==7,"not a densely sampled TRS clip"); return p;
    }
};
static void print(V x) {for(double val:x) std::cout<<' '<<std::fixed<<std::setprecision(4)<<1000*val;}
int main(int argc,char** argv) try {
    check(argc==2,"usage: overture_center <base1.anims.glb>"); Glb g(argv[1]);
    int root=g.node("barrel_plug"), barrel=g.node("barrel"), hinge=g.node("magazine_open"), cyl=g.node("cylinder"), slot=g.node("mag_slot");
    for(auto pair:{std::pair{root,barrel}, {barrel,hinge}, {hinge,cyl}, {cyl,slot}}) {
        auto& kids=g.j.at("nodes").at(pair.first).at("children");check(std::find(kids.begin(),kids.end(),pair.second)!=kids.end(),"unexpected parent");
    }
    std::cout<<"clip frame time_s | cylinder.RED_root_mm | delta_from_clip_start_mm | mag_slot_delta_mm\n";
    for(auto name:{"reload","empty_reload","idle_break_01"}) {
        auto& anims=g.j.at("animations"); auto it=std::find_if(anims.begin(),anims.end(),[&](const J& a){return a.at("name")==name;});check(it!=anims.end(),"missing clip");
        const bool idle=it->at("name")=="idle_break_01";V start{},seatStart{};
        for(int f: idle?std::vector<int>{0,26,52}:std::vector<int>{0,7,8,9,10,11,12,30,59,60,61}) {
            double t=-1;auto b=g.pose(*it,barrel,f,t),h=g.pose(*it,hinge,f,t),c=g.pose(*it,cyl,f,t),s=g.pose(*it,slot,f,t);
            // inverse(rootWorld) * cylinderWorld * origin. The root's world TRS cancels.
            V center=red(b.apply(h.apply(c.t))),seat=red(b.apply(h.apply(c.apply(s.t)))); if(f==0) {start=center;seatStart=seat;}
            std::cout<<name<<' '<<f<<' '<<std::fixed<<std::setprecision(6)<<t<<" |";print(center);std::cout<<" |";print(sub(center,start));std::cout<<" |";print(sub(seat,seatStart));std::cout<<'\n';
        }
    }
    return 0;
} catch(const std::exception& e) {std::cerr<<e.what()<<'\n';return 1;}
