#include "Framegen/Presenter.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Framegen/RefreshCadence.hpp"
#include <iostream>
#include <cstdlib>
#include <cmath>
using namespace cvr::framegen;
extern "C" int CyberpunkVR_RuntimeDiagnostics=0;
void Log(const char*,...){}
static Settings settings;static bool targeted=true,compatible=true,outputReady=true,joinOk=true;
static unsigned dispatched{},joined{},deferred{};static double clockMs=100;
namespace cvr::framegen::refresh {bool Targeted(){return targeted;}}
namespace cvr::framegen::flight {void Count(Event){}void Time(Timing,double){}void Output(unsigned,unsigned){}}
namespace cvr::framegen {
struct Generator::Impl{};
Generator::Generator()=default;Generator::~Generator()=default;
bool Generator::Generate(ID3D12Device*,ID3D12CommandQueue*,ID3D12Resource*[2],const std::shared_ptr<const Inputs>(&)[2],const EyePair(&)[2],const Settings&,double,bool reset,uint64_t,bool* generated,ID3D12Fence*,uint64_t,std::shared_ptr<void>,ID3D12CommandQueue*,std::shared_ptr<const CaptureTiming>,bool defer){
 ++dispatched;if(defer)++deferred;else ++joined;if(generated)*generated=!reset;return true;
}
bool Generator::CanPrepare(ID3D12Device*,ID3D12CommandQueue*,ID3D12Resource* const[2],const std::shared_ptr<const Inputs>(&)[2],const Settings&)const{return compatible;}
bool Generator::JoinOutput(ID3D12CommandQueue*){++joined;return joinOk;}
bool Generator::OutputReady()const{return outputReady;}
ID3D12Resource* Generator::Output(unsigned i)const{return reinterpret_cast<ID3D12Resource*>(uintptr_t(100+i));}
void Generator::ConsumerFence(ID3D12Fence*,uint64_t){}void Generator::Poll(){}bool Generator::Reset(){return true;}void Generator::Shutdown(){}
Settings GetSettings(){return settings;}Backend ActiveBackend(){return settings.backend;}
bool MetricsEnabled(){return false;}bool DepthCaptureEnabled(){return false;}void ReleaseInputs(){}uint64_t InputVram(){return 0;}void SetVram(uint64_t){}
void PacingReady(bool){}void OnPresentationCycle(bool){}void Status(const char*,bool){}void OnSkip(){}
void OnSubmitted(uint64_t,bool,bool,double){}double NowMs(){return clockMs;}
void CaptureFrame(ID3D12Device*,ID3D12CommandQueue*,ID3D12Resource* const[2],const std::shared_ptr<const Inputs>(&)[2],const EyePair(&)[2]){}
}
void check(bool v,const char* why){if(!v){std::cerr<<why<<'\n';std::exit(1);}}
Frame frame(uint64_t n){
 Frame f;f.identity.serial=n;f.identity.origin=1;f.identity.nativeFrame=uint32_t(n);f.identity.capturedMs=n*22.22;
 f.identity.pose[0]=n*2;f.identity.pose[1]=n*2+1;f.identity.stereo=f.identity.inputs=true;
 f.lease=std::make_shared<ReadLease>();
 for(unsigned i=0;i<2;++i){f.color[i]=reinterpret_cast<ID3D12Resource*>(uintptr_t(1+i));f.pose[i].orientation.w=1;
  f.fov[i]={-.7f,.7f,.7f,-.7f};f.inputs[i]=std::make_shared<Inputs>();}
 return f;
}
int main(){
 settings.enabled=settings.autoPace=true;settings.backend=Backend::Nvidia;
 auto* device=reinterpret_cast<ID3D12Device*>(1);auto* queue=reinterpret_cast<ID3D12CommandQueue*>(2);auto* consumer=reinterpret_cast<ID3D12CommandQueue*>(3);
 Presenter p;auto a=frame(1);check(!p.Prepare(device,queue,a,consumer),"cannot prepare without primed history");
 auto real=p.Select(device,queue,a,true,consumer);check(!real.synthetic,"first real primes");p.Submitted(real,nullptr,0,true);
 auto b=frame(2);auto bLease=std::weak_ptr<ReadLease>(b.lease);const auto before=joined;
 check(p.Prepare(device,queue,b,consumer),"next capture prepared in idle time");b={};
 check(deferred==1 && joined==before,"preparation must not block current real presentation queue");
 check(!p.Prepare(device,queue,frame(3),consumer),"single prepared pair cannot be overwritten");
 clockMs+=9;auto mid=p.Select(device,queue,frame(3),true,consumer);
 check(mid.synthetic && mid.frame.identity.serial==2 && joined==before+1,"prepared midpoint takes priority over newer capture and joins only now");
 p.Submitted(mid,nullptr,0,true);mid={};check(!bLease.expired(),"real endpoint pinned after midpoint");
 real=p.Select(device,queue,frame(3),true,consumer);check(!real.synthetic && real.frame.identity.serial==2,"matching B endpoint follows midpoint");
 // Prepare C while B is being displayed. Pending-real acceptance must not clear C.
 check(p.Prepare(device,queue,frame(3),consumer),"prepare following pair during real endpoint");
 p.Submitted(real,nullptr,0,true);real={};check(bLease.expired(),"B lease released after matching endpoint");
 mid=p.Select(device,queue,frame(4),true,consumer);check(mid.synthetic && mid.frame.identity.serial==3,"C preparation survives B acceptance");
 p.Submitted(mid,nullptr,0,true);real=p.Select(device,queue,frame(4),true,consumer);p.Submitted(real,nullptr,0,true);
 targeted=false;check(!p.Prepare(device,queue,frame(4),consumer),"VDXR excluded");targeted=true;
 settings.autoPace=false;check(!p.Prepare(device,queue,frame(4),consumer),"disabled adaptive excluded");settings.autoPace=true;
 compatible=false;check(!p.Prepare(device,queue,frame(4),consumer),"reconfiguration cannot occur inside idle hold");compatible=true;
 auto reset=frame(4);reset.identity.reset=true;check(!p.Prepare(device,queue,reset,consumer),"reset capture not prefetched");
 check(!p.Prepare(device,queue,frame(5),consumer),"skipped neighbor not prefetched");
 check(p.Prepare(device,queue,frame(4),consumer),"prepare before cancellation");
 const auto n=joined;settings.enabled=false;real=p.Select(device,queue,frame(4),true,consumer);
 check(!real.synthetic && joined==n,"disabled FG cancels prepared image without output join");
 settings.enabled=true;real=p.Select(device,queue,frame(10),true,consumer);p.Submitted(real,nullptr,0,true);
 check(p.Prepare(device,queue,frame(11),consumer),"new history after toggle");
 auto changed=frame(11);changed.identity.origin=2;real=p.Select(device,queue,changed,true,consumer);
 check(!real.synthetic,"recenter cancels prepared pair");
 p.Shutdown();
 for(auto backend:{Backend::Nvidia,Backend::NvidiaV557})for(bool prepare:{false,true}){
  settings.backend=backend;Presenter cameraTest;
  auto first=frame(20),second=frame(21);
  for(unsigned e=0;e<2;++e){second.pose[e].position.x=0.2f;second.pose[e].orientation={0,.1f,0,.994987f};}
  auto initial=cameraTest.Select(device,queue,first,true,consumer);cameraTest.Submitted(initial,nullptr,0,true);
  if(prepare)check(cameraTest.Prepare(device,queue,second,consumer),"OFA backend supports idle preparation");
  auto generated=cameraTest.Select(device,queue,second,true,consumer);check(generated.synthetic,"camera test synthesized");
  for(unsigned e=0;e<2;++e){
   const auto& pose=generated.frame.pose[e];
   check(std::abs(pose.position.x-(backend==Backend::NvidiaV557?.2f:.1f))<.0001f,"reconstruction camera position contract");
   check(std::abs(pose.orientation.y-(backend==Backend::NvidiaV557?.1f:.050063f))<.0001f,"reconstruction camera orientation contract");
  }
  cameraTest.Shutdown();
 }
 std::cout<<"PASS: production Presenter prepares without consumer join, ordered midpoint/endpoint, lease lifetime, toggle/recenter and backend/config gating\n";
}
