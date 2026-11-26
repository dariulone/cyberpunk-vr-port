#include "Anim/OpticHinge.hpp"
#include <array>
#include <cstdlib>
#include <iostream>
using namespace cvr::anim::optic_hinge;
void Check(bool ok,const char* why){if(!ok){std::cerr<<why<<'\n';std::exit(1);}}
struct BonePose {std::array<float,4> translation,rotation,scale;};
static_assert(sizeof(BonePose)==48);
int main(){
 Identity owner{10,20,30,40,50,60,70,80,90,31,32};Target target{owner,100,true};
 Pass pass{owner,40,50,60,80,10,70,120,true,true,false};
 const std::array<uint64_t,4> names{1,2,BoneNameHash,3};
 const std::array<uint32_t,8> map{1,0,5,1,8,2,12,3};std::array<BonePose,4> bones;
 for(auto& bone:bones)bone={{1,2,3,1},{0,0,0,1},{1,2,3,1}};
 const auto native=bones;
 Check(Apply(target,pass,map,reinterpret_cast<uint8_t*>(bones.data()),4,names),"owned bone was not written");
 auto expected=native;expected[2].rotation=HipRotation;
 Check(std::memcmp(bones.data(),expected.data(),sizeof(bones))==0,"rotation-only write altered parents/reload translations/scales");
 auto reject=[&](Pass p,const char* why){bones=native;Check(!Apply(target,p,map,reinterpret_cast<uint8_t*>(bones.data()),4,names),why);Check(std::memcmp(bones.data(),native.data(),sizeof(bones))==0,"refused pass mutated storage");};
 auto other=pass;other.object=400;other.descriptor=500;other.track=800;
 reject(other,"second actor of identical rig was written");
 other=pass;other.current.weapon=200;reject(other,"changed entity binding was written");
 other=pass;other.current.component=300;reject(other,"changed Barrel component was written");
 other=pass;other.current.graphComponent=310;reject(other,"changed Receiver component was written");
 other=pass;other.current.controlBinding=320;reject(other,"changed exact control binding was written");
 other=pass;other.current.rig=600;reject(other,"rig replacement reused old target");
 other=pass;other.current.metaRigId=91;reject(other,"meta-rig rebuild reused old target");
 other=pass;other.metaRig=700;reject(other,"old in-flight graph meta-rig was accepted");
 other=pass;other.current.track=800;reject(other,"new track buffer reused old target");
 other=pass;other.weakAlive=false;reject(other,"expired weak lifetime was accepted");
 other=pass;other.now=300;reject(other,"stale request survived 200 ms");
 other=pass;other.now=99;reject(other,"future stamp was accepted");
 other=pass;other.player=11;reject(other,"player/device owner change was accepted");
 other=pass;other.context=false;reject(other,"disabled VR/device context was accepted");
 other=pass;other.legacyConflict=true;reject(other,"existing reload writer for hinge was overwritten");
 const std::array<uint32_t,4> omitted{1,0,5,1};bones=native;
 Check(!Apply(target,pass,omitted,reinterpret_cast<uint8_t*>(bones.data()),4,names)&&std::memcmp(bones.data(),native.data(),sizeof(bones))==0,"omitted LOD bone was pinned from a stale map");
 const std::array<uint32_t,4> duplicate{8,1,8,2};Check(!Apply(target,pass,duplicate,reinterpret_cast<uint8_t*>(bones.data()),4,names),"ambiguous remap was accepted");
 const std::array<uint32_t,2> outside{8,4};Check(!Apply(target,pass,outside,reinterpret_cast<uint8_t*>(bones.data()),4,names),"out-of-bounds remap was accepted");
 auto wrongNames=names;wrongNames[2]=99;Check(!Apply(target,pass,map,reinterpret_cast<uint8_t*>(bones.data()),4,wrongNames),"destination was not the hinge in merged MetaRig");
 std::array<BonePose,22> merged{};std::array<uint64_t,22> mergedNames{};mergedNames[17]=BoneNameHash;
 const std::array<uint32_t,2> mergedMap{8,17};
 Check(Apply(target,pass,mergedMap,reinterpret_cast<uint8_t*>(merged.data()),22,mergedNames)&&merged[17].rotation==HipRotation&&merged[8].rotation==std::array<float,4>{},"merged receiver rig was treated as source rig indexes");
 target.enabled=false;bones=native; // the original native memcpy has run again before this production site
 Check(!Apply(target,pass,map,reinterpret_cast<uint8_t*>(bones.data()),4,names)&&std::memcmp(bones.data(),native.data(),sizeof(bones))==0,"disable/holster rewrote fresh native pose");
 std::cout<<"PASS owner-qualified Osprey hinge: same-rig foreign actor, weak/freshness/context/rebind gates, current remap, rotation-only and native release\n";
}
