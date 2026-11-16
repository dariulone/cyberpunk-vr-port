// Generate nearby support holds for the two-hand grip. Numerical work stays in C++.
// Pistol files are never input. Existing files are kept as they are: a hold tuned by hand is never overwritten.
//
// LONG GUNS, from the primary fore-end hold (CyberpunkVR_TwoHandGrip_<gun>.ini, left wrist in the right wrist's
// frame), the gun's frame in the right wrist's (the median of the idle frames of <gun>_one_a.lua that match the hold:
// r = the right wrist, wp/wq = the weapon) and the gun's own boxes (weapon_shapes.lua: origin at the grip, +Y along
// the barrel, +Z up, fitted to the game's vertices):
//   @z_rear / @z_forward -- the primary hold 35 mm back and forward along the barrel.
//   @trigger -- the support hand under the receiver just ahead of the trigger guard. The primary hold's own relation to
//     the gun moved there: the palm (the left hand's collision capsule, capsules.lua) as far below the gun's underside
//     and as far from its centre line as it is on the fore-end, the wrist turned as on the fore-end (palm up under the
//     gun), the four fingers opened by how much wider the gun is there. A palm at least 9 cm ahead of the gun hand's.
//     Not where something hangs below the receiver there (a magazine well: the gun's @mag hold is that grip).
// TWO-HANDED BLADES, along the handle's real axis: the katana's frame in the right hand bone's, measured in game
// (tools/hand_tests/melee_frames.txt), and its handle from its mesh (weapon_shapes_melee.lua: the guard at z = 0, the
// handle down -Z to the pommel). @hNNN: the primary hold slid along that axis, its palm NNN mm below the guard, every
// 25 mm from where it touches the gun hand down to a palm's half-width short of the pommel.
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <sstream>
#include <string>
#include <vector>
namespace fs=std::filesystem;
using V=std::array<double,3>;using Q=std::array<double,4>;
V add(V a,V b){for(int k=0;k<3;++k)a[k]+=b[k];return a;}
V sub(V a,V b){for(int k=0;k<3;++k)a[k]-=b[k];return a;}
V scale(V a,double s){for(auto& x:a)x*=s;return a;}
double dot(V a,V b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
double length(V a){return std::sqrt(dot(a,a));}
Q norm(Q a){double n=0;for(double x:a)n+=x*x;n=std::sqrt(n);if(n<1e-6)throw std::runtime_error("invalid quaternion");for(auto& x:a)x/=n;return a;}
Q conj(Q a){a[0]=-a[0];a[1]=-a[1];a[2]=-a[2];return a;}
Q mul(Q a,Q b){return {a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1],a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0],a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3],a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]};}
V rotate(Q q,V p){q=norm(q);V t{2*(q[1]*p[2]-q[2]*p[1]),2*(q[2]*p[0]-q[0]*p[2]),2*(q[0]*p[1]-q[1]*p[0])};return add(p,add(scale(t,q[3]),V{q[1]*t[2]-q[2]*t[1],q[2]*t[0]-q[0]*t[2],q[0]*t[1]-q[1]*t[0]}));}
std::vector<double> field(const std::string& line,const std::string& key){const auto p=line.find(key+"={");if(p==std::string::npos)return {};const auto e=line.find('}',p);std::string t=line.substr(p+key.size()+2,e-p-key.size()-2);std::replace(t.begin(),t.end(),',',' ');std::istringstream s(t);std::vector<double> v;double x;while(s>>x)v.push_back(x);return v;}
double median(std::vector<double> v){std::sort(v.begin(),v.end());return v.empty()?0:v[v.size()/2];}

// The hands' collision capsules' centres in their bone's frame (capsules.lua vrp_hand_l / vrp_hand_r): the palm.
const V kPalmLeft{0.0407,0.0140,0.0050},kPalmRight{-0.0407,-0.0140,-0.0050};

struct Pose{V p{};Q q{};std::string fingers;};
Pose readPose(const fs::path& path){std::ifstream f(path);if(!f)throw std::runtime_error("missing "+path.string());Pose p;std::string line;bool have=false;while(std::getline(f,line)){if(line.rfind("W ",0)==0){std::istringstream s(line.substr(2));for(auto& x:p.p)s>>x;for(auto& x:p.q)s>>x;have=bool(s);}else if(line.rfind("F ",0)==0)p.fingers+=line+'\n';}if(!have||p.fingers.empty())throw std::runtime_error("invalid hold");return p;}

// The gun in the right wrist's frame, and its barrel axis there, from the take's idle frames that match the hold.
struct GunFrame {V axis{};V pos{};Q q{};};
GunFrame frameFromTake(const fs::path& path,const Pose& p){
    std::ifstream file(path);if(!file)throw std::runtime_error("missing take "+path.string());
    struct Row{double error;V axis,pos;Q q;};std::vector<Row> rows;std::string line;double best=1e9;
    while(std::getline(file,line)){
        if(line.rfind("{t=",0)!=0)continue;if(std::stod(line.substr(3))<.05)continue;
        auto l=field(line,"l"),r=field(line,"r"),w=field(line,"wq"),wp=field(line,"wp");
        if(l.size()<7||r.size()<7||w.size()<4||wp.size()<3)continue;
        Q rq{r[3],r[4],r[5],r[6]},wq{w[0],w[1],w[2],w[3]};V rp{r[0],r[1],r[2]};
        V off=rotate(conj(rq),sub({l[0],l[1],l[2]},rp));const double error=length(sub(off,p.p));
        Q gq=norm(mul(conj(rq),wq));if(gq[3]<0)for(auto& x:gq)x=-x;
        rows.push_back({error,rotate(gq,{0,1,0}),rotate(conj(rq),sub({wp[0],wp[1],wp[2]},rp)),gq});best=std::min(best,error);
    }
    if(best>.025)throw std::runtime_error("take does not match support pose (m): "+std::to_string(best));
    GunFrame g{};std::array<std::vector<double>,10> c;unsigned count=0;
    for(auto& row:rows)if(row.error<=best+.003){
        g.axis=add(g.axis,row.axis);++count;
        for(int k=0;k<3;++k)c[k].push_back(row.pos[k]);for(int k=0;k<4;++k)c[3+k].push_back(row.q[k]);
    }
    const auto n=length(g.axis);if(n<count*.98||!count)throw std::runtime_error("unstable weapon axis");
    g.axis=scale(g.axis,1/n);
    for(int k=0;k<3;++k)g.pos[k]=median(c[k]);for(int k=0;k<4;++k)g.q[k]=median(c[3+k]);g.q=norm(g.q);
    return g;
}

// weapon_shapes*.lua: { minX, maxX, minY, maxY, minZ, maxZ } per box, by family name.
using Box=std::array<double,6>;
std::map<std::string,std::vector<Box>> readShapes(const fs::path& path){
    std::ifstream f(path);if(!f)throw std::runtime_error("missing "+path.string());
    std::map<std::string,std::vector<Box>> out;std::string line,cur;
    while(std::getline(f,line)){
        const auto eq=line.find(" = {");
        if(eq!=std::string::npos){auto s=line.find_first_not_of(' ');cur=line.substr(s,eq-s);continue;}
        const auto b=line.find('{');if(b==std::string::npos||cur.empty())continue;
        std::string t=line.substr(b+1,line.find('}')-b-1);std::replace(t.begin(),t.end(),',',' ');
        std::istringstream s(t);Box x{};for(auto& v:x)s>>v;if(s)out[cur].push_back(x);
    }
    return out;
}
// The gun's underside and half-width across y = at (the boxes spanning it); false where nothing spans it.
bool sectionAt(const std::vector<Box>& boxes,double at,double& bottom,double& centreX,double& halfWidth){
    bool any=false;double lo=1e9,xl=1e9,xh=-1e9;
    for(const auto& b:boxes)if(b[2]<=at&&at<=b[3]){any=true;lo=std::min(lo,b[4]);xl=std::min(xl,b[0]);xh=std::max(xh,b[1]);}
    if(!any)return false;bottom=lo;centreX=.5*(xl+xh);halfWidth=.5*(xh-xl);return true;
}

// The four fingers' curl joints (Index1..Pinky3: parent-local, identity = straight) turned `t` of the way toward
// straight; the thumb and the in-hand bones as they are.
std::string openFingers(const std::string& fingers,double t){
    std::istringstream in(fingers);std::ostringstream out;out<<std::setprecision(9);std::string line;
    while(std::getline(in,line)){
        std::istringstream s(line);std::string tag,bone;Q q{};s>>tag>>bone;for(auto& x:q)s>>x;
        const bool curl=bone.find("InHand")==std::string::npos && bone.find("Thumb")==std::string::npos &&
            (bone.find("Index")!=std::string::npos || bone.find("Middle")!=std::string::npos ||
             bone.find("Ring")!=std::string::npos || bone.find("Pinky")!=std::string::npos);
        if(!s || !curl || t<=0){out<<line<<'\n';continue;}
        const double sign=q[3]<0?-1:1;
        Q r{q[0]*(1-t),q[1]*(1-t),q[2]*(1-t),q[3]*(1-t)+t*sign};r=norm(r);
        out<<"F "<<bone;for(double x:r)out<<' '<<x;out<<'\n';
    }
    return out.str();
}
// false: kept (the file exists)
bool write(const fs::path& path,const std::string& note,V position,Q rotation,const std::string& fingers){
    if(fs::exists(path))return false;
    std::ofstream f(path);
    f<<"# CyberpunkVR two-hand support pose v1\n# "<<note<<"\n# Generated by tools/hand_tests/generate_support_points.cpp; validate mesh clearance in game.\nW"<<std::setprecision(9);
    for(auto x:position)f<<' '<<x;for(auto x:rotation)f<<' '<<x;f<<'\n'<<fingers;
    if(!f)throw std::runtime_error("write failed");
    return true;
}

int main(int argc,char** argv)try{
 if(argc!=3)throw std::runtime_error("generate_support_points repo output-dir");fs::path root=argv[1],out=argv[2];fs::create_directories(out);
 const std::vector<std::string> guns={"achilles","ajax","ashura","carnage","copperhead","crusher","defender","dian","grad","guillotine","hercules","igla","kolac","kyubi","masamune","nekomata","osprey","pozhar","pulsar","rasetsu","saratoga","senkoh","shingen","sidewinder","sor22","tactician","testera","warden","zhuo"};
 const auto shapes=readShapes(root/"mods/cet/CyberpunkVRPort_HandCollision/weapon_shapes.lua");
 const auto meleeShapes=readShapes(root/"mods/cet/CyberpunkVRPort_HandCollision/weapon_shapes_melee.lua");
 // Under the receiver: the palm centre this far ahead of the grip origin at least, and 9 cm ahead of the gun hand's.
 constexpr double kTriggerAhead=.11,kPalmsApart=.09,kHanging=.05,kStep=.025;
 unsigned made=0,kept=0;
 const auto count=[&](bool fresh){fresh?++made:++kept;};
 for(const auto& gun:guns){
     const auto p=readPose(root/"mods/config"/("CyberpunkVR_TwoHandGrip_"+gun+".ini"));
     const auto g=frameFromTake(root/"build/shotgun-reload-20261019/takes"/(gun+"_one_a.lua"),p);
     for(int sign:{-1,1}){
         const std::string name=gun+(sign<0?"@z_rear":"@z_forward");
         count(write(out/("CyberpunkVR_TwoHandGrip_"+name+".ini"),name+": "+std::to_string(int(sign*35))+
             " mm along "+gun+"_one_a measured barrel axis; fingers and wrist rotation from the primary hold.",
             add(p.p,scale(g.axis,.035*sign)),p.q,p.fingers));
     }
     std::string family;
     for(const auto& [key,boxes]:shapes){const std::string tail="_"+gun;if(key.size()>tail.size()&&key.compare(key.size()-tail.size(),tail.size(),tail)==0)family=key;}
     if(family.empty()){std::cout<<gun<<": no shape, no @trigger\n";continue;}
     const auto& boxes=shapes.at(family);
     // Everything in the gun's frame: the support wrist and palm, the gun hand's palm.
     const Q toGun=conj(g.q);
     const V wrist=rotate(toGun,sub(p.p,g.pos));const Q turn=norm(mul(toGun,p.q));
     const V palm=add(wrist,rotate(turn,kPalmLeft));
     const V gunPalm=rotate(toGun,sub(rotate({0,0,0,1},kPalmRight),g.pos));
     double foreBottom,foreX,foreHalf;
     if(!sectionAt(boxes,palm[1],foreBottom,foreX,foreHalf)){std::cout<<gun<<": no fore-end under the hold\n";continue;}
     const double at=std::max(kTriggerAhead,gunPalm[1]+kPalmsApart);
     double bottom,centreX,half;
     if(palm[1]-at<.06 || !sectionAt(boxes,at,bottom,centreX,half)){std::cout<<gun<<": the fore-end hold is already there\n";continue;}
     // Something hanging below the receiver there (a magazine): its underside far below the fore-end's. The gun's @mag
     // hold is that grip. Nor where the boxes leave the underside open (it lies above the fore-end's).
     if(bottom<foreBottom-kHanging){std::cout<<gun<<": a magazine ahead of the trigger, no @trigger\n";continue;}
     if(bottom>foreBottom+.03){std::cout<<gun<<": no underside ahead of the trigger, no @trigger\n";continue;}
     const V movedPalm{centreX+(palm[0]-foreX),at,bottom+(palm[2]-foreBottom)};
     const V movedWrist=add(wrist,sub(movedPalm,palm));
     const V position=add(g.pos,rotate(g.q,movedWrist));
     const double open=std::clamp((half-foreHalf)/.03,0.0,.5);
     char note[320];std::snprintf(note,sizeof(note),"%s@trigger: under the receiver, the palm %.0f mm ahead of the grip origin (the fore-end hold's %.0f mm), "
         "%.0f mm under the gun as there; wrist turn from the primary hold, fingers opened %.0f%% (the gun %.0f mm wide there, %.0f at the fore-end).",
         gun.c_str(),at*1000,palm[1]*1000,(foreBottom-palm[2])*1000,open*100,half*2000,foreHalf*2000);
     count(write(out/("CyberpunkVR_TwoHandGrip_"+gun+"@trigger.ini"),note,position,p.q,openFingers(p.fingers,open)));
     std::cout<<gun<<" @trigger at "<<at<<" (fore-end palm "<<palm[1]<<", gun palm "<<gunPalm[1]<<"), underside "<<bottom<<" vs "<<foreBottom<<", open "<<open<<"\n";
 }
 // Two-handed blades along the handle.
 std::ifstream frames(root/"tools/hand_tests/melee_frames.txt");std::string line;
 while(std::getline(frames,line)){
     if(line.empty()||line[0]=='#')continue;
     std::istringstream s(line);std::string weapon,family;V pos{};Q q{};s>>weapon>>family;for(auto& x:pos)s>>x;for(auto& x:q)s>>x;
     if(!s)throw std::runtime_error("bad melee frame: "+line);
     const auto it=meleeShapes.find(family);if(it==meleeShapes.end())throw std::runtime_error("no shape for "+family);
     double pommel=0;for(const auto& b:it->second)pommel=std::min(pommel,b[4]);
     const auto p=readPose(root/"mods/config"/("CyberpunkVR_TwoHandGrip_"+weapon+".ini"));
     q=norm(q);const V down=rotate(q,{0,0,-1});
     const auto along=[&](V x){return dot(sub(x,pos),down);};   // metres below the guard
     const double palm=along(add(p.p,rotate(p.q,kPalmLeft))),gunPalm=along(kPalmRight);
     const double least=std::max(palm-kStep*4,gunPalm+kPalmsApart),most=-pommel-.045;
     const auto offAxis=[&](V x){const V r=sub(x,pos);return length(sub(r,scale(down,dot(r,down))));};
     std::cout<<weapon<<": handle 0.."<<-pommel<<" m, gun palm "<<gunPalm<<" ("<<offAxis(kPalmRight)<<" off the axis), primary palm "<<palm
         <<" ("<<offAxis(add(p.p,rotate(p.q,kPalmLeft)))<<" off), holds "<<least<<".."<<most<<"\n";
     for(double at=least;at<=most+1e-6;at+=kStep){
         if(std::abs(at-palm)<.01)continue;   // the primary hold itself
         const int mm=int(std::lround(at*1000));
         char name[64];std::snprintf(name,sizeof(name),"%s@h%03d",weapon.c_str(),mm);
         count(write(out/("CyberpunkVR_TwoHandGrip_"+std::string(name)+".ini"),std::string(name)+": the palm "+std::to_string(mm)+
             " mm below the guard along the handle (the primary hold's "+std::to_string(int(std::lround(palm*1000)))+
             "); the handle's axis from the "+family+" frame measured in the right hand; fingers and wrist from the primary hold.",
             add(p.p,scale(down,at-palm)),p.q,p.fingers));
     }
 }
 std::cout<<"Generated "<<made<<" support points, kept "<<kept<<"; no pistol input\n";return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
