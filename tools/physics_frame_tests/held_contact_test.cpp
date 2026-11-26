#include "Natives/HeldGripContact.hpp"
#include <cstdlib>
#include <iostream>
#include <limits>
using namespace cvr::held_grip_contact;
using namespace cvr::held_grip;
static void Check(bool ok,const char* why){if(!ok){std::cerr<<why<<'\n';std::exit(1);}}
int main(){
 const auto* sceneA=reinterpret_cast<void*>(uintptr_t(0x1000));
 const auto* sceneB=reinterpret_cast<void*>(uintptr_t(0x2000));
 StepGate gate;
 gate.Begin(sceneA,sceneA);gate.Prepared(sceneA);const auto first=gate.Take(sceneA);
 Check(first.valid,"prepared scene step rejected");
 Check(!gate.Take(sceneA).valid,"duplicate fetch refreshed old target");
 gate.Begin(sceneA,sceneA); // Origin/Prove failure: Prepared never runs.
 Check(!gate.Take(sceneA).valid,"failed preparation reused old target");
 gate.Begin(sceneA,sceneA);gate.Prepared(sceneA);
 gate.Begin(sceneB,sceneA);Check(!gate.Take(sceneB).valid,"foreign scene consumed world contact");
 Check(gate.Take(sceneA).valid,"interleaved non-world scene lost world target");
 gate.Begin(sceneA,sceneA);gate.Prepared(sceneA);
 Check(!gate.Take(sceneB).valid,"new scene reused old-scene target");
 gate.Begin(sceneB,sceneB);gate.Prepared(sceneB);const auto replacement=gate.Take(sceneB);
 Check(replacement.valid && replacement.generation!=first.generation,"new scene did not get a distinct prepared generation");
 gate.Begin(sceneB,sceneB);gate.Prepared(sceneB);gate.Take(sceneB); // fetch fails later in Origin/Prove.
 Check(!gate.Take(sceneB).valid,"failed fetch was reused with a new timestamp");
 const float identity[4]{0,0,0,1},origin[3]{-2048,1536,0};
 const float actualGrip[3]{.025f,.04f,-.02f},target[3]{-2280.75f,1704.5f,19.1f};
 float actor[3];for(int k=0;k<3;++k)actor[k]=target[k]-origin[k]-actualGrip[k];
 Check(ErrorMicrometres(actor,identity,actualGrip,origin,target,true)<500,"initial actual-body contact not aligned");
 // Ref body7 can articulate far away while native body18 continues holding its
 // own local point. Rotation of body18 about that point is not grip loss either.
 for(float degrees:{-170.f,-60.f,0.f,50.f,170.f}){
  const float h=degrees*.00872664626f,q[4]{0,0,std::sin(h),std::cos(h)};float offset[3];Rotate(q,actualGrip,offset);
  for(int k=0;k<3;++k)actor[k]=target[k]-origin[k]-offset[k];
  Check(ErrorMicrometres(actor,q,actualGrip,origin,target,true)<500,"rotation about the actual anchor looked like contact loss");
  const float unrelatedRefPoint[3]{target[0]+1,target[1]-.7f,target[2]+.3f};
  Check(ErrorMicrometres(actor,q,actualGrip,origin,unrelatedRefPoint,true)>1000000,"fixture did not separate reference-joint motion");
 }
 const float rebased[3]{-2304,1792,128};
 for(int k=0;k<3;++k)actor[k]=target[k]-rebased[k]-actualGrip[k];
 Check(ErrorMicrometres(actor,identity,actualGrip,rebased,target,true)<500,"origin rebase changed world contact");
 actor[2]+=.02f;const auto error=ErrorMicrometres(actor,identity,actualGrip,rebased,target,true);
 Check(error>19500&&error<20500,"real 2cm contact error was not measured");
 // Two holders of one actor have separate body-local anchors and targets.
 const float aGrip[3]{-.1f,0,0},bGrip[3]{.1f,0,0},zero[3]{},aTarget[3]{-.1f,0,0},bTarget[3]{.15f,0,0};
 Check(ErrorMicrometres(zero,identity,aGrip,zero,aTarget,true)==0,"first hand inherited the other hand's contact error");
 Check(ErrorMicrometres(zero,identity,bGrip,zero,bTarget,true)>=49999,"second hand's real contact error was hidden by midpoint");
 Check(ErrorMicrometres(zero,identity,aGrip,zero,aTarget,false)==kInvalid,"missing target published zero success");
 const float bad[4]{0,0,std::numeric_limits<float>::quiet_NaN(),1};
 Check(ErrorMicrometres(zero,bad,aGrip,zero,aTarget,true)==kInvalid,"invalid actor pose published zero success");
 const float noTarget[3]{std::numeric_limits<float>::infinity(),0,0};
 Check(ErrorMicrometres(zero,identity,aGrip,zero,noTarget,true)==kInvalid,"invalid target published contact success");
 std::cout<<"native contact: selected-body anchor, independent articulation, pivot rotation, world origin rebase, two holders and invalid sentinel PASS\n";
}
