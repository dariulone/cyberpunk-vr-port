#include "Framegen/RefreshCadence.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Framegen/Presenter.hpp"
#include "Framegen/FrameCapture.hpp"
#include "Utils/DebugGate.hpp"
#include <algorithm>
#include <cmath>
#include <psapi.h>
extern void Log(const char*,...);

namespace cvr::framegen {
namespace {
bool ExternalBridgeLoaded() {
    static uint64_t nextCheck=0;static bool loaded=false;
    const auto now=GetTickCount64();if(now<nextCheck)return loaded;nextCheck=now+2000;loaded=false;
    HMODULE modules[1024]{};DWORD bytes{};
    if(EnumProcessModules(GetCurrentProcess(),modules,sizeof(modules),&bytes)) {
        const auto count=std::min<size_t>(bytes/sizeof(HMODULE),std::size(modules));
        for(size_t i=0;i<count;++i) {
            wchar_t name[MAX_PATH]{};GetModuleBaseNameW(GetCurrentProcess(),modules[i],name,MAX_PATH);
            if(_wcsnicmp(name,L"XR_APILAYER_XRFrameBridge",23)==0) {loaded=true;break;}
        }
    }
    return loaded;
}
XrPosef Middle(const XrPosef& a,const XrPosef& b) {
    XrPosef out{};out.position={(a.position.x+b.position.x)*.5f,(a.position.y+b.position.y)*.5f,(a.position.z+b.position.z)*.5f};
    const auto& x=a.orientation;const auto& y=b.orientation;
    const float sign=x.x*y.x+x.y*y.y+x.z*y.z+x.w*y.w<0 ? -1.0f : 1.0f;
    out.orientation={x.x+sign*y.x,x.y+sign*y.y,x.z+sign*y.z,x.w+sign*y.w};
    auto& q=out.orientation;const float norm=std::sqrt(q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w);
    if(norm>0) {q.x/=norm;q.y/=norm;q.z/=norm;q.w/=norm;}else q={0,0,0,1};
    return out;
}
EyeCamera Camera(const XrPosef& pose,const XrFovf& fov) {
    EyeCamera c;const auto& q=pose.orientation;
    c.orientation[0]=q.x;c.orientation[1]=q.y;c.orientation[2]=q.z;c.orientation[3]=q.w;
    c.fov[0]=fov.angleLeft;c.fov[1]=fov.angleRight;c.fov[2]=fov.angleUp;c.fov[3]=fov.angleDown;
    return c;
}
bool SameProjection(const Frame& a,const Frame& b) {
    for(unsigned eye=0;eye<2;++eye) {
        const auto& x=a.fov[eye];const auto& y=b.fov[eye];
        if(std::abs(x.angleLeft-y.angleLeft)>.001f || std::abs(x.angleRight-y.angleRight)>.001f ||
           std::abs(x.angleUp-y.angleUp)>.001f || std::abs(x.angleDown-y.angleDown)>.001f)return false;
        const auto& q=a.pose[eye].orientation;const auto& r=b.pose[eye].orientation;
        if(std::abs(q.x*r.x+q.y*r.y+q.z*r.z+q.w*r.w)<.95f)return false;
    }
    return true;
}
}
void Presenter::Remember(const Frame& current){
    previous=current;previous.lease.reset();previous.inputs[0].reset();previous.inputs[1].reset();
    previous.color[0]=previous.color[1]=nullptr;previous.colorReady.Reset();previous.colorReadyValue=0;previous.captureTiming.reset();primed=true;
}
bool Presenter::Prepare(ID3D12Device* device,ID3D12CommandQueue* queue,Frame current,ID3D12CommandQueue* consumer){
    const auto settings=GetSettings();
    if(!refresh::Targeted() || !settings.enabled || !settings.autoPace || !UsesNvidiaOFA(settings.backend) ||
       !UsesNvidiaOFA(ActiveBackend()) || !consumer || consumer==queue || ExternalBridgeLoaded() ||
       !WantsPrepare(current.identity.serial) || !current.lease || !CanInterpolate(previous.identity,current.identity) ||
       !SameProjection(previous,current) || !generator.CanPrepare(device,queue,current.color,current.inputs,settings))return false;
    generator.Poll();
    EyePair cameras[2];for(unsigned eye=0;eye<2;++eye)
        cameras[eye]={Camera(previous.pose[eye],previous.fov[eye]),Camera(current.pose[eye],current.fov[eye])};
    bool generated=false;
    if(!generator.Generate(device,queue,current.color,current.inputs,cameras,settings,
        current.identity.capturedMs-previous.identity.capturedMs,false,++sdkFrame,&generated,
        current.colorReady.Get(),current.colorReadyValue,current.lease,consumer,current.captureTiming,true)){
        flight::Count(flight::Event::PrepareAheadFailed);primed=false;return false;
    }
    prepared={current,generated?Cadence::Next::Midpoint:Cadence::Next::Real,current.lease,generated};
    preparedEndpoint=current;preparedSettings=settings;preparedAt=NowMs();generatedSerial=current.identity.serial;
    if(generated){
        prepared.frame.identity.capturedMs=(previous.identity.capturedMs+current.identity.capturedMs)*.5;
        for(unsigned eye=0;eye<2;++eye){prepared.frame.color[eye]=generator.Output(eye);
            prepared.frame.pose[eye]=ActiveBackend()==Backend::NvidiaV557?current.pose[eye]:Middle(previous.pose[eye],current.pose[eye]);prepared.frame.identity.pose[eye]=0;}
        flight::Count(flight::Event::GeneratedQueued);
    }
    Remember(current);flight::Count(flight::Event::PrepareAheadQueued);return true;
}
Selection Presenter::Select(ID3D12Device* device,ID3D12CommandQueue* queue,Frame current,bool gameplay,ID3D12CommandQueue* consumer) {
    generator.Poll();
    const auto settings=GetSettings();
    if(prepared.frame.identity.serial && (!gameplay || !settings.enabled || !settings.autoPace ||
        !refresh::Targeted() || settings.backend!=preparedSettings.backend || settings.quality!=preparedSettings.quality ||
        settings.flowScale!=preparedSettings.flowScale || current.identity.origin!=preparedEndpoint.identity.origin ||
        current.identity.reset || !SameProjection(preparedEndpoint,current))){
        flight::Count(flight::Event::PrepareAheadCancelled);Reset();
    }
    const bool pairGap=primed && !cadence.Pending() && current.identity.inputs && !current.identity.reset &&
        current.identity.origin==previous.identity.origin && current.identity.serial>previous.identity.serial &&
        uint32_t(current.identity.nativeFrame-previous.identity.nativeFrame)>1 &&
        uint32_t(current.identity.nativeFrame-previous.identity.nativeFrame)<=4 &&
        current.identity.capturedMs-previous.identity.capturedMs<=100 && SameProjection(previous,current);
    if(pairGap)flight::Count(flight::Event::PairGap);
    OnPresentationCycle(pairGap);
    if(!gameplay || (pending.identity.serial && pending.identity.origin!=current.identity.origin))Reset();
    if(cadence.Pending())return {pending,Cadence::Next::PendingReal,{},false};
    if(prepared.frame.identity.serial){
        if(!generator.JoinOutput(consumer)){flight::Count(flight::Event::PrepareAheadFailed);Reset();}
        else {
            auto result=std::move(prepared);prepared={};
            if(result.synthetic)pending=std::move(preparedEndpoint);
            preparedEndpoint={};
            flight::Count(generator.OutputReady()?flight::Event::PrepareAheadReady:flight::Event::PrepareAheadPending);
            flight::Time(flight::Timing::PrepareAheadLead,NowMs()-preparedAt);
            flight::Count(flight::Event::PrepareAheadUsed);preparedAt=0;
            PacingReady(true);Status(ActiveBackend()==Backend::NvidiaV557?"Generating with V557 optical reconstruction":"Generating with engine motion vectors and depth",result.synthetic);return result;
        }
    }
    Selection result{current,cadence.Select(current.identity.serial,false,false),{},false};
    // Stereo captures for the benchmark (FrameCapture.hpp, tools/vr_bench) with frame generation off too: the frame's
    // colours and the depth inputs the depth layer uses are here either way. Costs a relaxed load while disarmed.
    // Depth is enough here: without frame generation the motion vectors are not captured (FrameCapture skips them).
    if(!settings.enabled && gameplay && current.identity.stereo && current.color[0] && current.color[1] &&
       current.inputs[0] && current.inputs[1] && current.inputs[0]->hasDepth && current.inputs[1]->hasDepth &&
       current.inputs[0]->frameId==current.inputs[1]->frameId) {
        EyePair cameras[2];
        for(unsigned eye=0;eye<2;++eye)cameras[eye]={Camera(current.pose[eye],current.fov[eye]),Camera(current.pose[eye],current.fov[eye])};
        auto* diagnosticQueue=consumer?consumer:queue;
        bool ready=true;
        if(diagnosticQueue!=queue) {
            ready=current.colorReady && current.colorReadyValue && SUCCEEDED(diagnosticQueue->Wait(current.colorReady.Get(),current.colorReadyValue));
            for(const auto& in:current.inputs)if(in->readyFence && in->fenceValue)
                ready=SUCCEEDED(diagnosticQueue->Wait(in->readyFence.Get(),in->fenceValue)) && ready;
        }
        if(ready)CaptureFrame(device,diagnosticQueue,current.color,current.inputs,cameras);
    }
    if(!settings.enabled || !gameplay) {
        if(settings.enabled)flight::Count(flight::Event::Paused);
        primed=false;previous={};generatedSerial=0;
        PacingReady(false);
        if(generator.Reset()) {if(!DepthCaptureEnabled())ReleaseInputs();if(MetricsEnabled())SetVram(InputVram());}
        Status(settings.enabled ? "Paused in menus or untracked views" : "Disabled");return result;
    }
    if(ExternalBridgeLoaded()) {
        Reset();Status("External OFXR is loaded: disarm its tray and restart before native generation");return result;
    }
    if(!current.identity.inputs || !current.identity.stereo || !current.lease) {
        flight::Count(flight::Event::WaitingInputs);
        primed=false;PacingReady(false);Status("Waiting for matching depth and motion vectors in both eyes");return result;
    }
    if(current.identity.serial==generatedSerial)return result;
    const bool interpolate=primed && CanInterpolate(previous.identity,current.identity) && SameProjection(previous,current);
    if(primed && !interpolate && cvr::RuntimeDiagnosticsEnabled()) {
        static uint64_t next{};const auto now=GetTickCount64();
        if(now>=next){next=now+1000;Log("[framegen-pair] prev=%llu/%u now=%llu/%u dt=%.3f reset=%d projection=%d\n",
            previous.identity.serial,previous.identity.nativeFrame,current.identity.serial,current.identity.nativeFrame,
            current.identity.capturedMs-previous.identity.capturedMs,current.identity.reset,SameProjection(previous,current));}
    }
    if(primed&&!interpolate)flight::Count(flight::Event::PairRejected);
    if(flight::Enabled()&&current.color[0]){auto d=current.color[0]->GetDesc();flight::Output(uint32_t(d.Width),d.Height);}
    const double milliseconds=interpolate ? current.identity.capturedMs-previous.identity.capturedMs : 1000.0/45;
    bool generated=false;
    EyePair cameras[2];
    for(unsigned eye=0;eye<2;++eye)cameras[eye]={Camera(previous.pose[eye],previous.fov[eye]),Camera(current.pose[eye],current.fov[eye])};
    if(!generator.Generate(device,queue,current.color,current.inputs,cameras,settings,milliseconds,!interpolate,++sdkFrame,&generated,current.colorReady.Get(),current.colorReadyValue,current.lease,consumer,current.captureTiming)) {
        flight::Count(flight::Event::GenerateFailed);primed=false;OnSkip();return result;
    }
    generatedSerial=current.identity.serial;
    PacingReady(true);
    if(interpolate && generated) {
        flight::Count(flight::Event::GeneratedQueued);
        result.kind=Cadence::Next::Midpoint;result.synthetic=true;result.generatedFrom=current.lease;
        result.frame.identity.capturedMs=(previous.identity.capturedMs+current.identity.capturedMs)*.5;
        for(unsigned eye=0;eye<2;++eye) {
            result.frame.color[eye]=generator.Output(eye);
            result.frame.pose[eye]=ActiveBackend()==Backend::NvidiaV557?current.pose[eye]:Middle(previous.pose[eye],current.pose[eye]);
            result.frame.identity.pose[eye]=0; // synthetic content is not the original real-frame receipt
        }
        pending=current;
        Status(ActiveBackend()==Backend::NvidiaV557?"Generating with V557 optical reconstruction":"Generating with engine motion vectors and depth",true);
    } else {
        result.generatedFrom=current.lease;
        flight::Count(flight::Event::Primed);
        Status("Priming frame history",false);
    }
    // The SDK owns the previous-color history. Keep metadata only, not another
    // reference/pin/copy of the port's old capture buffers.
    Remember(current);
    return result;
}
void Presenter::Submitted(const Selection& result,ID3D12Fence* fence,uint64_t value,bool accepted) {
    if(result.frame.lease)result.frame.lease->fence=std::max(result.frame.lease->fence,value);
    if(result.generatedFrom)result.generatedFrom->fence=std::max(result.generatedFrom->fence,value);
    generator.ConsumerFence(fence,value);
    if(!accepted) {flight::Count(flight::Event::SubmitRejected);Reset();return;}
    cadence.Accepted(result.kind,result.frame.identity.serial);
    if(MetricsEnabled()||flight::Enabled())OnSubmitted(result.frame.identity.serial,result.synthetic,result.kind==Cadence::Next::Repeat,NowMs());
    if(result.kind==Cadence::Next::PendingReal)pending={};
}
void Presenter::Shutdown() {Reset();generator.Shutdown();}
void Presenter::Reset() {
    PacingReady(false);
    cadence.Reset();pending={};prepared={};preparedEndpoint={};preparedAt=0;previous={};primed=false;generatedSerial=0;
    if(generator.Reset() && !DepthCaptureEnabled())ReleaseInputs();
}
}
