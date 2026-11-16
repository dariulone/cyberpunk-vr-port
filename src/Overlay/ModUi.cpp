#include "Overlay/ModUi.hpp"
#include "Overlay/ModUiInput.hpp"
#include <array>
#include <atomic>
#include <mutex>
#include <string>

namespace cvr::modui {
namespace {
using Microsoft::WRL::ComPtr;
using cvr::vrui::Canvas;
struct Source {
    uint64_t id{},stateAt{},activation{},lease{},readyAt{};
    uint32_t flags{};bool retired{};
    std::string name;
    std::array<std::shared_ptr<Canvas>,3> pool;
    std::shared_ptr<Canvas> writing,latest,ready;
};
std::mutex mutex;
std::array<Source,8> sources;
ComPtr<ID3D12Device> runtimeDevice;
uint64_t nextId{},nextLease{},serial{},activation{},runtimeAt{},owner{};
bool runtimeAvailable{},capture{},releasePending{},focused{};
Input input;
bool Available(){return runtimeAvailable && runtimeDevice && GetTickCount64()-runtimeAt<1000;}
Source* Find(uint64_t id){if(id)for(auto& s:sources)if(s.id==id)return &s;return nullptr;}
bool Done(const std::shared_ptr<Canvas>& c){return !c || !c->fence || c->fence->GetCompletedValue()>=c->value;}
bool DeviceMatches(ID3D12Device* a,ID3D12Device* b){
    if(!a || !b)return false;ComPtr<IUnknown> x,y;
    return SUCCEEDED(a->QueryInterface(IID_PPV_ARGS(&x))) && SUCCEEDED(b->QueryInterface(IID_PPV_ARGS(&y))) && x.Get()==y.Get();
}
void Collect(){for(auto& s:sources)if(s.retired && !s.writing){
    bool free=true;for(const auto& c:s.pool)if(c && (c.use_count()!=1 || !Done(c)))free=false;
    if(free)s={};
}}
CvrModUiHandle Register(const char* name){try {
    if(!name || !*name || strnlen_s(name,128)>=128)return 0;
    std::lock_guard lock(mutex);Collect();
    for(auto& s:sources)if(!s.id){s.id=++nextId;s.name=name;return s.id;}return 0;
}catch(...){return 0;}}
void Unregister(CvrModUiHandle id){std::lock_guard lock(mutex);if(auto* s=Find(id)){s->retired=true;s->flags=0;s->latest.reset();s->ready.reset();}Collect();}
int Viewport(CvrModUiHandle id,CvrModUiViewport* out){
    if(!out || out->size!=sizeof(*out))return 0;std::lock_guard lock(mutex);
    auto* s=Find(id);if(!s || s->retired || !Available())return 0;
    *out={sizeof(*out),cvr::vrui::CanvasWidth,cvr::vrui::CanvasHeight,0};return 1;
}
void State(CvrModUiHandle id,uint32_t flags){std::lock_guard lock(mutex);if(auto* s=Find(id);s && !s->retired){
    flags&=CVR_MOD_UI_VISIBLE|CVR_MOD_UI_INTERACTIVE;
    if((flags&CVR_MOD_UI_VISIBLE) && (!(s->flags&CVR_MOD_UI_VISIBLE) || GetTickCount64()-s->stateAt>=1000))s->activation=++activation;
    s->flags=flags;s->stateAt=GetTickCount64();if(!(flags&CVR_MOD_UI_VISIBLE)){s->latest.reset();s->ready.reset();}
}}
uint32_t Poll(CvrModUiHandle id,CvrModUiInput* out,uint32_t cap){
    if(!out || !cap || cap>64)return 0;std::lock_guard lock(mutex);
    if(!Find(id))return 0;
    if(id!=owner || !capture || !Available()){out[0]={-1,-1,0,0,0,0};return 1;}
    const auto count=input.Poll(out,cap);for(uint32_t i=0;i<count;++i)out[i].flags|=CVR_MOD_UI_ACTIVE;return count;
}
int Begin(CvrModUiHandle id,void* rawDevice,uint32_t rawFormat,CvrModUiTarget* out){try {
    if(!out || out->size!=sizeof(*out))return 0;
    const auto format=static_cast<DXGI_FORMAT>(rawFormat);
    if(format!=DXGI_FORMAT_R8G8B8A8_UNORM && format!=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB &&
       format!=DXGI_FORMAT_R10G10B10A2_UNORM && format!=DXGI_FORMAT_R16G16B16A16_FLOAT)return 0;
    std::lock_guard lock(mutex);auto* s=Find(id);auto* device=static_cast<ID3D12Device*>(rawDevice);
    if(!s || s->retired || s->writing || !Available() || !DeviceMatches(device,runtimeDevice.Get()))return 0;
    for(auto& c:s->pool){
        if(c && (c.use_count()!=1 || !Done(c)))continue;
        if(c){ComPtr<ID3D12Device> previous;c->texture->GetDevice(IID_PPV_ARGS(&previous));
            if(c->format!=format || !DeviceMatches(previous.Get(),device))c.reset();}
        if(!c)c=cvr::vrui::MakeCanvas(device,format);if(!c)return 0;
        s->writing=c;s->lease=++nextLease;
        *out={sizeof(*out),cvr::vrui::CanvasWidth,cvr::vrui::CanvasHeight,rawFormat,s->lease,c->texture.Get(),c->rtv->GetCPUDescriptorHandleForHeapStart().ptr};return 1;
    }return 0;
}catch(...){return 0;}}
int Submit(CvrModUiHandle id,uint64_t lease,void* rawFence,uint64_t value){
    if(!rawFence || !value)return 0;std::lock_guard lock(mutex);auto* s=Find(id);
    if(!s || !s->writing || s->lease!=lease)return 0;
    auto* fence=static_cast<ID3D12Fence*>(rawFence);ComPtr<ID3D12Device> fd,td;
    if(FAILED(fence->GetDevice(IID_PPV_ARGS(&fd))) || FAILED(s->writing->texture->GetDevice(IID_PPV_ARGS(&td))) || !DeviceMatches(fd.Get(),td.Get()))return 0;
    s->writing->fence=fence;s->writing->value=value;s->writing->serial=++serial;
    if(!s->retired)s->latest=s->writing;s->writing.reset();s->lease=0;return 1;
}
void Cancel(CvrModUiHandle id,uint64_t lease){std::lock_guard lock(mutex);if(auto* s=Find(id);s && s->lease==lease){s->writing.reset();s->lease=0;}}
const CvrModUiApi api{sizeof(CvrModUiApi),CVR_MOD_UI_VERSION,Register,Unregister,Viewport,State,Poll,Begin,Submit,Cancel};
}
const CvrModUiApi* Api(){return &api;}
void Tracking(const cvr::vrui::Tracking& t,ID3D12Device* device,bool hasFocus,bool available){
    std::lock_guard lock(mutex);runtimeDevice=device;runtimeAt=GetTickCount64();runtimeAvailable=available;focused=hasFocus;Collect();
    Source* chosen=nullptr;
    if(Available())for(auto& s:sources){
        if(s.latest && s.latest->fence && s.latest->fence->GetCompletedValue()!=UINT64_MAX && Done(s.latest) &&
           (!s.ready || s.ready->serial!=s.latest->serial)){s.ready=s.latest;s.readyAt=runtimeAt;}
        const bool interactive=(s.flags&CVR_MOD_UI_INTERACTIVE)!=0;
        const bool chosenInteractive=chosen && (chosen->flags&CVR_MOD_UI_INTERACTIVE);
        // A late toast/HUD producer must not evict an interactive window.
        const bool preferred=!chosen || (interactive && !chosenInteractive) ||
            (interactive==chosenInteractive && s.activation>chosen->activation);
        if(s.id && !s.retired && (s.flags&CVR_MOD_UI_VISIBLE) && runtimeAt-s.stateAt<1000 && s.ready && runtimeAt-s.readyAt<1000 && preferred)chosen=&s;
    }
    const auto next=chosen?chosen->id:0;
    if(next!=owner){input.Reset();owner=next;}
    const bool was=capture;capture=chosen && (chosen->flags&CVR_MOD_UI_INTERACTIVE) && focused && t.valid;
    releasePending|=was && !capture;
    bool neutral=!t.menuLeft && !t.menuRight;
    for(const auto& h:t.hands)neutral&=h.trigger<.3f && h.grip<.5f && std::abs(h.stickX)<.35f && std::abs(h.stickY)<.35f;
    if(neutral)releasePending=false;
    input.Step(t,chosen!=nullptr,capture,runtimeAt,cvr::vrui::GetSettings());
}
void Suspend(){std::lock_guard lock(mutex);runtimeAvailable=capture=releasePending=false;owner=0;input.Reset();runtimeDevice.Reset();}
bool CapturesInput(){std::lock_guard lock(mutex);return Available() && (capture || releasePending);}
bool Visible(){std::lock_guard lock(mutex);return Available() && owner && input.View().valid;}
cvr::vrui::View GetView(){std::lock_guard lock(mutex);return Available()?input.View():cvr::vrui::View{};}
std::shared_ptr<Canvas> Latest(){std::lock_guard lock(mutex);auto* s=Find(owner);return Available() && s?s->ready:nullptr;}
}
extern "C" __declspec(dllexport) const CvrModUiApi* CyberpunkVR_GetModUiApi(uint32_t version,uint32_t size){
    return version==CVR_MOD_UI_VERSION && size==sizeof(CvrModUiApi)?cvr::modui::Api():nullptr;
}
