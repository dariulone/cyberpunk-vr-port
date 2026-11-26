// Offline, deliberately narrow decoder for the native two-frame Osprey ADS pose.
#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <vector>
using J=nlohmann::json;using V=std::array<double,3>;using Q=std::array<double,4>;
J Read(const std::filesystem::path& p){J j;std::ifstream(p)>>j;return j;}
std::vector<uint8_t> Decode(const std::string& s){std::vector<uint8_t> out;uint32_t value=0;unsigned bits=0;for(char c:s){if(c=='=')break;int d=c>='A'&&c<='Z'?c-'A':c>='a'&&c<='z'?c-'a'+26:c>='0'&&c<='9'?c-'0'+52:c=='+'?62:c=='/'?63:-1;if(d<0)throw std::runtime_error("invalid base64");value=(value<<6)|unsigned(d);bits+=6;if(bits>=8){bits-=8;out.push_back(uint8_t(value>>bits));value&=(1u<<bits)-1;}}return out;}
template<class T>T At(const std::vector<uint8_t>& b,size_t i){if(i+sizeof(T)>b.size())throw std::runtime_error("short key");T v;std::memcpy(&v,b.data()+i,sizeof(v));return v;}
Q Unit(Q q){double n=0;for(double x:q)n+=x*x;if(!std::isfinite(n)||n<1e-12)throw std::runtime_error("invalid quaternion");for(double& x:q)x/=std::sqrt(n);return q;}
Q Inv(Q q){return {-q[0],-q[1],-q[2],q[3]};}
Q Mul(Q a,Q b){return {a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1],a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0],a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3],a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]};}
V Sub(V a,V b){for(int i=0;i<3;++i)a[i]-=b[i];return a;}
double Norm(V a){return std::hypot(a[0],a[1],a[2]);}
V Rotate(Q q,V v){auto r=Mul(Mul(q,{v[0],v[1],v[2],0}),Inv(q));return {r[0],r[1],r[2]};}
double Angle(Q a,Q b){a=Unit(a);b=Unit(b);double dot=0;for(int i=0;i<4;++i)dot+=a[i]*b[i];return 2*std::acos(std::clamp(std::abs(dot),0.,1.))*180/3.14159265358979323846;}
struct Pose{V p{};Q q{0,0,0,1};};
Pose Relative(const J& input){auto wq=Unit(input.at("weapon").at("q").get<Q>()),bq=Unit(input.at("scopeBone").at("q").get<Q>());return {Rotate(Inv(wq),Sub(input.at("scopeBone").at("p").get<V>(),input.at("weapon").at("p").get<V>())),Mul(Inv(wq),bq)};}
int main(int argc,char** argv)try{
 if(argc!=5)throw std::runtime_error("rig.json anims.json live-capture-directory output.json");
 auto rig=Read(argv[1]).at("Data").at("RootChunk"),set=Read(argv[2]).at("Data").at("RootChunk");
 const auto& names=rig.at("boneNames");if(names.size()!=13||names[8].at("$value")!="scope_hinge"||rig.at("boneParentIndexes")[8]!=1||rig.at("boneParentIndexes")[1]!=0)throw std::runtime_error("unexpected Osprey hierarchy");
 std::array<Pose,13> bind,clip;for(int i=0;i<13;++i){const auto& p=rig.at("boneTransforms")[i];bind[i]={{p["Translation"]["X"],p["Translation"]["Y"],p["Translation"]["Z"]},{p["Rotation"]["i"],p["Rotation"]["j"],p["Rotation"]["k"],p["Rotation"]["r"]}};}clip=bind;
 J animation;for(const auto& a:set.at("animations")){const auto& d=a.at("Data").at("animation").at("Data");if(d.at("name").at("$value")=="idlepose_ironsight")animation=d;}
 if(animation.is_null())throw std::runtime_error("ADS pose absent");const auto& b=animation.at("animBuffer").at("Data");
 if(b.at("numJoints")!=13||b.at("numAnimKeys")!=0||b.at("numAnimKeysRaw")!=4||b.at("numConstAnimKeys")!=25||b.at("numTracks")!=0)throw std::runtime_error("unexpected compressed-key layout");
 const auto bytes=Decode(b.at("tempBuffer").at("Bytes"));if(bytes.size()!=464)throw std::runtime_error("unexpected ADS buffer length");
 auto apply=[&](uint16_t key,size_t offset){const unsigned bone=key&0x1fff,type=(key>>13)&3;if(bone>=13||type>1)throw std::runtime_error("unexpected key type");V values{At<float>(bytes,offset),At<float>(bytes,offset+4),At<float>(bytes,offset+8)};
  if(type==0)clip[bone].p=values;else{double squared=0;for(double x:values)squared+=x*x;const double w=std::sqrt(std::max(0.,1-squared));clip[bone].q=Unit({values[0],values[1],values[2],key&0x8000?-w:w});}};
 for(size_t i=0;i<64;i+=16)apply(At<uint16_t>(bytes,i+2),i+4);
 for(size_t i=64;i<bytes.size();i+=16)apply(At<uint16_t>(bytes,i),i+4);
 J out={{"scope","Native idlepose_ironsight local keys versus authored rig bind. Live comparison is bone-slot world/weapon-relative, not a direct VRRigBone local snapshot."},{"bones",J::array()}};
 for(int i=0;i<13;++i)out["bones"].push_back({{"index",i},{"name",names[i].at("$value")},{"translationDifferenceMeters",Norm(Sub(clip[i].p,bind[i].p))},{"rotationDifferenceDegrees",Angle(clip[i].q,bind[i].q)},{"bindPosition",bind[i].p},{"clipPosition",clip[i].p},{"bindRotation",bind[i].q},{"clipRotation",clip[i].q}});
 const std::filesystem::path live=argv[3];auto hip=Read(live/"osprey-hip-geometry-input.json"),ads=Read(live/"osprey-ads-geometry-input.json"),back=Read(live/"osprey-back-hip-geometry-input.json");auto h=Relative(hip),a=Relative(ads),z=Relative(back);
 out["live"]={{"weaponRelativePivotDifferenceMeters",Norm(Sub(a.p,h.p))},{"weaponRelativeHingeRotationDegrees",Angle(a.q,h.q)},{"restoredRelativeRotationDegrees",Angle(z.q,h.q)},
  {"worldPivotDifferenceMeters",Norm(Sub(ads["scopeBone"]["p"].get<V>(),hip["scopeBone"]["p"].get<V>()))},{"weaponRotationDifferenceDegrees",Angle(hip["weapon"]["q"].get<Q>(),ads["weapon"]["q"].get<Q>())}};
 std::ofstream(argv[4])<<out.dump(2)<<'\n';std::cout<<out["live"].dump(2)<<'\n';for(const auto& row:out["bones"])if(row["translationDifferenceMeters"].get<double>()>1e-5||row["rotationDifferenceDegrees"].get<double>()>.01)std::cout<<row.dump()<<'\n';return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
