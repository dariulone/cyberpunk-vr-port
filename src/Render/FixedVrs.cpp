#include "Render/FixedVrs.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/ViewInstancedPipeline.hpp"
#include "Stereo/EngineStereo.hpp"
#include <wrl/client.h>
#include <algorithm>
#include <cstring>
#include <cmath>
#include <memory>
#include <mutex>
#include <new>
#include <unordered_map>

extern "C" {
__declspec(dllexport) uint32_t CyberpunkVR_FixedVrsReportSeq=0;
__declspec(dllexport) char CyberpunkVR_FixedVrsReport[1024]="{}";
}

namespace cvr::vrs {
using Microsoft::WRL::ComPtr;
std::atomic<bool> enabled{};
namespace {
constexpr GUID pipelineKey{0x5a4ac73c,0x80da,0x4983,{0xb5,0x12,0x1f,0x54,0xa9,0xa0,0xbe,0x47}};
constexpr GUID stateKey{0xf6e31653,0xe00f,0x4ee1,{0x8e,0xb4,0xc8,0xaf,0x84,0x93,0x36,0x71}};
struct Map;
struct Layout {
    unsigned width{},height{};
    bool stereo{};
    std::array<Settings,2> regions{};
    bool operator==(const Layout&) const = default;
};
struct State {
    bool known{},eligible{},depth{};
    UINT targets{},expectedTargets{},width{},height{};
    D3D12_SHADING_RATE rate=D3D12_SHADING_RATE_1X1;
    std::array<D3D12_SHADING_RATE_COMBINER,2> combiners{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
    ID3D12Resource* image{};
    ID3D12Resource* retained{};
    ID3D12PipelineState* pipeline{};
    uint64_t revision{},recordingEpoch{};
    unsigned mapWidth{},mapHeight{};
    unsigned targetWidth{},targetHeight{},viewportCount{};
    std::array<D3D12_VIEWPORT,2> viewports{};
    int mapEye=-1;
    std::shared_ptr<Map> selected;
    ID3D12Resource* overrideImage{};
    RateFn rateFn{};
    ImageFn imageFn{};
};
std::atomic<unsigned> trackedLists{},stateFailures{};
// The list owns its state; weak registry/cache references expire when it dies.
// Recycled addresses therefore start fresh, without a lifetime registration cap.
class StateOwner final : public IUnknown {
    std::atomic<ULONG> refs{1};
public:
    std::shared_ptr<State> state;
    explicit StateOwner(std::shared_ptr<State> value):state(std::move(value)){++trackedLists;}
    ~StateOwner(){--trackedLists;}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out) override {
        if(!out)return E_POINTER;
        *out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=static_cast<IUnknown*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release() override{const auto n=--refs;if(!n)delete this;return n;}
};
struct Map {
    Settings settings;
    Layout layout;
    ComPtr<ID3D12Resource> image,upload;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    uint64_t completion{},lastUsed{};
    bool ready{};
};
std::mutex mutex;
std::mutex statesMutex;
std::atomic<bool> everEnabled{};
std::atomic<uint64_t> revision{1};
std::atomic<uint64_t> recordingEpoch{1};
std::array<std::atomic<uint64_t>,2> draws{},sceneDraws{},eligibleDraws{};
// First rejection after the scene/gaze gate: pipeline, depth, target count,
// viewport, native stereo frame, target dimensions, stereo viewports, legacy viewports.
std::array<std::atomic<uint64_t>,8> rejectedDraws{};
Settings requested,applied;
uint64_t applyAfter{},fenceValue{},useSerial{};
uint64_t lastReport{};
Status status;
ComPtr<ID3D12Device> device;
ComPtr<ID3D12Fence> fence;
std::unordered_map<ID3D12GraphicsCommandList*,std::weak_ptr<State>> states;
unsigned registrationsSincePrune{};
std::vector<std::shared_ptr<Map>> maps;
std::vector<Layout> requests;
GazeCenters gaze{};
std::atomic<uint64_t> gazeExpires{};
std::atomic<bool> gazeRequired{};

State* FindState(ID3D12GraphicsCommandList* list,bool create=false) {
    // Recording one command list is externally serialized by D3D12's contract.
    // Its private-data owner keeps the returned state alive throughout the call.
    thread_local ID3D12GraphicsCommandList* previous{};
    thread_local std::weak_ptr<State> cached;
    if(previous==list)if(auto state=cached.lock())return state.get();
    std::lock_guard lock(statesMutex);
    auto it=states.find(list);
    auto state=it==states.end()?std::shared_ptr<State>{}:it->second.lock();
    if(!state && create)try {
        if(++registrationsSincePrune>=128) {
            std::erase_if(states,[](const auto& entry){return entry.second.expired();});
            registrationsSincePrune=0;
        }
        auto entry=states.try_emplace(list).first;
        state=std::make_shared<State>();
        ComPtr<StateOwner> owner;owner.Attach(new StateOwner(state));
        if(SUCCEEDED(list->SetPrivateDataInterface(stateKey,owner.Get())))entry->second=state;
        else {++stateFailures;state.reset();}
    } catch(const std::bad_alloc&) {++stateFailures;state.reset();}
    previous=list;cached=state;return state.get();
}
void RestoreState(ID3D12GraphicsCommandList* list,State& s) {
    if(!s.overrideImage)return;
    auto* l5=static_cast<ID3D12GraphicsCommandList5*>(list);
    s.imageFn(l5,s.image);s.rateFn(l5,s.rate,s.combiners.data());s.overrideImage=nullptr;
}
template<class F> void Edit(ID3D12GraphicsCommandList* list,F edit) {
    if(stereo::probe::internalCommands || !enabled.load(std::memory_order_relaxed))return;
    if(auto* s=FindState(list);s && s->known && s->recordingEpoch==recordingEpoch.load(std::memory_order_acquire))edit(*s);
}
void SetPipeline(State& s,ID3D12PipelineState* pso) {
    if(s.pipeline==pso)return;
    s.pipeline=pso;
    UINT count=0,size=sizeof(count);
    s.eligible=pso && SUCCEEDED(pso->GetPrivateData(pipelineKey,&size,&count)) && size==sizeof(count) && count>=3 && count<=4;
    s.expectedTargets=s.eligible?count:0;
}
bool CreateMap(Map& map,ID3D12CommandQueue* queue) {
    const auto& layout=map.layout;
    const auto bytes=layout.stereo?BuildStereoRates(layout.regions,layout.width,layout.height,status.tileSize,status.additionalRates):
        BuildRates(layout.regions[0],layout.width,layout.height,status.tileSize,status.additionalRates);
    if(bytes.empty())return false;
    D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    d.Width=(layout.width+status.tileSize-1)/status.tileSize;d.Height=(layout.height+status.tileSize-1)/status.tileSize;
    d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Format=DXGI_FORMAT_R8_UINT;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&map.image))))return false;
    map.image->SetName(L"CVR_foveated_shading_rate");
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{};UINT64 size{};device->GetCopyableFootprints(&d,0,1,0,&fp,nullptr,nullptr,&size);
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=size;
    buffer.Height=1;buffer.DepthOrArraySize=1;buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    heap.Type=D3D12_HEAP_TYPE_UPLOAD;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&map.upload))))return false;
    void* data{};const D3D12_RANGE noRead{0,0};if(FAILED(map.upload->Map(0,&noRead,&data)))return false;
    std::memset(data,0,size);
    for(UINT y=0;y<d.Height;++y)std::memcpy(static_cast<uint8_t*>(data)+fp.Offset+y*fp.Footprint.RowPitch,bytes.data()+y*d.Width,size_t(d.Width));
    map.upload->Unmap(0,nullptr);
    if(FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&map.allocator))) ||
       FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,map.allocator.Get(),nullptr,IID_PPV_ARGS(&map.list))))return false;
    D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=map.upload.Get();from.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;from.PlacedFootprint=fp;to.pResource=map.image.Get();
    map.list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={map.image.Get(),0,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE};
    map.list->ResourceBarrier(1,&b);
    if(FAILED(map.list->Close()) || !gpu::KeepCommandResources(map.list.Get(),{map.image.Get(),map.upload.Get(),map.allocator.Get()}))return false;
    ID3D12CommandList* lists[]{map.list.Get()};auto resources=gpu::PrepareCommandResources(1,lists);
    queue->ExecuteCommandLists(1,lists);gpu::SubmitCommandResources(queue,std::move(resources));
    map.completion=++fenceValue;
    return SUCCEEDED(queue->Signal(fence.Get(),map.completion));
}
}
Settings GetSettings(){std::lock_guard lock(mutex);return requested;}
void UpdateGaze(GazeCenters centers,uint64_t stamp) {
    for(const auto& eye:centers)for(float v:eye)if(!std::isfinite(v) || v<0 || v>1)stamp=0;
    std::lock_guard lock(mutex);
    if(!stamp && !gazeExpires.load())return;
    gaze=centers;gazeExpires.store(stamp?stamp+100:0,std::memory_order_release);
    revision.fetch_add(1,std::memory_order_release);
}
void SetSettings(Settings s) {
    s=Normalize(s);std::lock_guard lock(mutex);
    // Fixed VRS retains its legacy behavior; gaze VRS also handles a verified SBS target layout.
    const bool on=s.mode!=0 && (!engine_stereo::Enabled() || s.eyeTracked);
    gazeRequired.store(s.eyeTracked,std::memory_order_release);
    if(enabled.load(std::memory_order_relaxed)!=on)recordingEpoch.fetch_add(1,std::memory_order_release);
    enabled.store(on,std::memory_order_relaxed);
    if(on)everEnabled.store(true,std::memory_order_relaxed);
    if(s==requested)return;
    const bool gazeModeChanged=s.eyeTracked!=requested.eyeTracked;
    requested=s;applyAfter=GetTickCount64()+1000;
    // Switching gaze mode must not spend the settings debounce drawing an old fixed map.
    if(!s.mode || gazeModeChanged){applied=s;requests.clear();revision.fetch_add(1,std::memory_order_release);}
}
Status GetStatus(){std::lock_guard lock(mutex);auto value=status;value.trackedLists=trackedLists.load();value.stateFailures=stateFailures.load();value.gazeFresh=gazeExpires.load()>GetTickCount64();for(int i=0;i<2;++i){value.draws[i]=draws[i].load();value.sceneDraws[i]=sceneDraws[i].load();value.eligibleDraws[i]=eligibleDraws[i].load();}return value;}
void GraphicsCreated(ID3D12PipelineState* pso,const D3D12_GRAPHICS_PIPELINE_STATE_DESC& d) {
    if(!pso || stereo::probe::internalCommands)return;
    const UINT count=EligiblePipeline(d)?d.NumRenderTargets:0;pso->SetPrivateData(pipelineKey,sizeof(count),&count);
}
void StreamCreated(ID3D12PipelineState* pso,const D3D12_PIPELINE_STATE_STREAM_DESC& source) {
    stereo::PipelineStreamInfo info;
    if(!pso || stereo::probe::internalCommands || !stereo::ReadPipelineStream(source,info))return;
    D3D12_GRAPHICS_PIPELINE_STATE_DESC d{};d.PS=info.ps;d.VS=info.vs;d.NumRenderTargets=info.targets.NumRenderTargets;
    d.DepthStencilState=info.depth;d.BlendState=info.blend;d.SampleDesc=info.samples;
    if(!info.haveDepth || !info.haveBlend || info.otherStages || info.viewInstancing || info.forcedSamples)return;
    GraphicsCreated(pso,d);
}
void Reset(ID3D12GraphicsCommandList* list,bool known,ID3D12PipelineState* pso) {
    if(stereo::probe::internalCommands)return;
    if(!everEnabled.load(std::memory_order_relaxed))return;
    const bool active=enabled.load(std::memory_order_relaxed) && known && list && list->GetType()==D3D12_COMMAND_LIST_TYPE_DIRECT;
    if(auto* s=FindState(list,active)){*s={};s->known=active;s->recordingEpoch=recordingEpoch.load(std::memory_order_acquire);if(active)SetPipeline(*s,pso);}
}
void Invalidate(ID3D12GraphicsCommandList* list){if(stereo::probe::internalCommands || !everEnabled.load(std::memory_order_relaxed))return;if(auto* s=FindState(list))*s={};}
void Pipeline(ID3D12GraphicsCommandList* list,ID3D12PipelineState* pso){Edit(list,[&](State& s){SetPipeline(s,pso);});}
void Viewports(ID3D12GraphicsCommandList* list,UINT n,const D3D12_VIEWPORT* v){Edit(list,[&](State& s){
    s.width=s.height=s.viewportCount=0;
    if(!n || n>2 || !v)return;
    for(UINT i=0;i<n;++i)if(v[i].TopLeftY!=0 || !(v[i].TopLeftX>=0 && v[i].TopLeftX<=16384 && v[i].Width>=64 && v[i].Width<=16384 && v[i].Height>=64 && v[i].Height<=16384) ||
       float(UINT(v[i].Width))!=v[i].Width || float(UINT(v[i].Height))!=v[i].Height)return;
    s.viewportCount=n;std::copy_n(v,n,s.viewports.begin());
    s.width=UINT(v->Width);s.height=UINT(v->Height);
});}
void Targets(ID3D12GraphicsCommandList* list,UINT count,bool depth,unsigned width,unsigned height){Edit(list,[&](State& s){s.targets=count;s.depth=depth;s.targetWidth=width;s.targetHeight=height;});}
void ShadingRate(ID3D12GraphicsCommandList* list,D3D12_SHADING_RATE rate,const D3D12_SHADING_RATE_COMBINER* c){Edit(list,[&](State& s){
    s.rate=rate;s.combiners=c?std::array{c[0],c[1]}:std::array{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
});}
void ShadingImage(ID3D12GraphicsCommandList* list,ID3D12Resource* image){Edit(list,[&](State& s){s.image=image;});}

void PrepareDraw(ID3D12GraphicsCommandList* base,uint32_t node,int eye,RateFn setRate,ImageFn setImage) {
    if(stereo::probe::internalCommands || !everEnabled.load(std::memory_order_relaxed))return;
    auto* state=FindState(base);if(!state || !state->known)return;auto& s=*state;
    if(!enabled.load(std::memory_order_relaxed) || s.recordingEpoch!=recordingEpoch.load(std::memory_order_acquire) || node!=0x33DDA4 || eye<0 || eye>1 || !setRate || !setImage){RestoreState(base,s);return;}
    if(gazeRequired.load(std::memory_order_acquire) && gazeExpires.load(std::memory_order_acquire)<=GetTickCount64()){RestoreState(base,s);return;}
    sceneDraws[eye].fetch_add(1,std::memory_order_relaxed);
    auto reject=[&](unsigned reason){rejectedDraws[reason].fetch_add(1,std::memory_order_relaxed);RestoreState(base,s);};
    if(!s.eligible){reject(0);return;}
    if(!s.depth){reject(1);return;}
    if(s.targets!=s.expectedTargets){reject(2);return;}
    if(!s.width || !s.height){reject(3);return;}
    const bool stereo=engine_stereo::Enabled();
    unsigned width=s.width,height=s.height;
    if(stereo) {
        // Require the current frame's native stereo, the actual bound target size,
        // and either full-width or half-width eye viewports. Never guess from window size.
        if(!engine_stereo::Rendering()){reject(4);return;}
        if(!s.targetWidth || s.targetWidth%2 || s.targetWidth>16384 ||
           !s.targetHeight || s.targetHeight>16384){reject(5);return;}
        width=s.targetWidth;height=s.targetHeight;
        for(unsigned i=0;i<s.viewportCount;++i) {
            const auto& v=s.viewports[i];
            if(v.Height!=height || !((s.viewportCount==1 && v.TopLeftX==0 && v.Width==width) ||
               (v.Width==width/2 && (v.TopLeftX==0 || v.TopLeftX==width/2)))){reject(6);return;}
        }
    } else if(s.viewportCount!=1 || s.viewports[0].TopLeftX!=0){reject(7);return;}
    eligibleDraws[eye].fetch_add(1,std::memory_order_relaxed);
    const auto version=revision.load(std::memory_order_acquire);
    if(s.revision!=version || s.mapWidth!=width || s.mapHeight!=height || s.mapEye!=eye) {
        std::lock_guard lock(mutex);s.selected.reset();s.revision=version;s.mapWidth=width;s.mapHeight=height;s.mapEye=eye;
        if(applied.mode && status.supported && !status.failed) {
            if(stereo && !applied.eyeTracked){RestoreState(base,s);return;}
            Layout layout{width,height,stereo,{applied,applied}};
            if(applied.eyeTracked) {
                if(gazeExpires.load()<=GetTickCount64()){RestoreState(base,s);return;}
                for(int i=0;i<2;++i)layout.regions[i]=GazeRegion(applied,gaze[stereo?i:eye][0],gaze[stereo?i:eye][1]);
            }
            const auto found=std::find_if(maps.begin(),maps.end(),[&](const auto& m){return m->settings==applied && m->layout==layout;});
            if(found!=maps.end()){if((*found)->ready){s.selected=*found;s.selected->lastUsed=++useSerial;}}
            else if(requests.size()<8 && std::find(requests.begin(),requests.end(),layout)==requests.end())requests.push_back(layout);
        }
    }
    if(!s.selected){RestoreState(base,s);return;}
    const auto& map=*s.selected;
    if(s.retained!=map.image.Get()) {
        if(!gpu::KeepCommandResources(base,{map.image.Get()})){RestoreState(base,s);return;}
        s.retained=map.image.Get();
    }
    draws[eye].fetch_add(1,std::memory_order_relaxed);
    if(s.overrideImage==map.image.Get())return;
    RestoreState(base,s);
    auto* list=static_cast<ID3D12GraphicsCommandList5*>(base);s.rateFn=setRate;s.imageFn=setImage;s.overrideImage=map.image.Get();
    const D3D12_SHADING_RATE_COMBINER combine[]{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_OVERRIDE};
    setImage(list,map.image.Get());setRate(list,D3D12_SHADING_RATE_1X1,combine);
}
void Restore(ID3D12GraphicsCommandList* list){if(stereo::probe::internalCommands || !everEnabled.load(std::memory_order_relaxed))return;if(auto* s=FindState(list))RestoreState(list,*s);}
void Tick(ID3D12Device* current,ID3D12CommandQueue* queue) {
    if(!current || !queue)return;
    stereo::probe::Scope internal;
    std::lock_guard lock(mutex);
    const auto now=GetTickCount64();
    if(now-lastReport>=250) {
        lastReport=now;InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_FixedVrsReportSeq));
        std::snprintf(CyberpunkVR_FixedVrsReport,sizeof(CyberpunkVR_FixedVrsReport),
            "{\"requested\":%d,\"applied\":%d,\"checked\":%d,\"supported\":%d,\"additional\":%d,\"failed\":%d,\"tile\":%u,\"maps\":%u,\"draws\":[%llu,%llu],\"scene\":[%llu,%llu],\"eligible\":[%llu,%llu],\"eye_tracked\":%d,\"gaze_fresh\":%d,\"tracked_lists\":%u,\"state_failures\":%u,\"rejected\":[%llu,%llu,%llu,%llu,%llu,%llu,%llu,%llu]}",
            requested.mode,applied.mode,int(status.checked),int(status.supported),int(status.additionalRates),int(status.failed),status.tileSize,status.readyMaps,
            draws[0].load(),draws[1].load(),sceneDraws[0].load(),sceneDraws[1].load(),eligibleDraws[0].load(),eligibleDraws[1].load(),
            int(requested.eyeTracked),int(gazeExpires.load()>now),trackedLists.load(),stateFailures.load(),
            rejectedDraws[0].load(),rejectedDraws[1].load(),rejectedDraws[2].load(),rejectedDraws[3].load(),
            rejectedDraws[4].load(),rejectedDraws[5].load(),rejectedDraws[6].load(),rejectedDraws[7].load());
        InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_FixedVrsReportSeq));
    }
    if(!enabled.load(std::memory_order_relaxed))return;
    if(device && device.Get()!=current){status.failed=true;return;}
    if(!status.checked) {
        device=current;status.checked=true;D3D12_FEATURE_DATA_D3D12_OPTIONS6 options{};
        status.supported=SUCCEEDED(device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS6,&options,sizeof(options))) &&
            options.VariableShadingRateTier>=D3D12_VARIABLE_SHADING_RATE_TIER_2 && options.ShadingRateImageTileSize>0;
        status.tileSize=options.ShadingRateImageTileSize;status.additionalRates=options.AdditionalShadingRatesSupported!=FALSE;
        if(status.supported && FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))))status.failed=true;
        revision.fetch_add(1,std::memory_order_release);
    }
    if(!status.supported || status.failed)return;
    if(GetTickCount64()>=applyAfter && !(applied==requested)) {applied=requested;requests.clear();revision.fetch_add(1,std::memory_order_release);}
    const auto done=fence->GetCompletedValue();if(done==UINT64_MAX){status.failed=true;return;}
    status.readyMaps=0;
    for(auto& map:maps)if(map->completion && map->completion<=done) {
        if(!map->ready)revision.fetch_add(1,std::memory_order_release);
        map->ready=true;map->upload.Reset();map->allocator.Reset();map->list.Reset();
        if(map->settings==applied)++status.readyMaps;
    }
    std::erase_if(maps,[&](const auto& m){return m->ready && !(m->settings==applied);});
    if(!applied.mode)return;
    auto pending=std::move(requests);requests.clear();
    for(const auto& layout:pending) {
        if(std::any_of(maps.begin(),maps.end(),[&](const auto& m){return m->layout==layout && m->settings==applied;}))continue;
        if(maps.size()>=(applied.eyeTracked?128u:16u)) {
            auto victim=maps.end();for(auto it=maps.begin();it!=maps.end();++it)if((*it)->ready && (victim==maps.end() || (*it)->lastUsed<(*victim)->lastUsed))victim=it;
            if(victim==maps.end())break;
            maps.erase(victim); // Recorded lists/submissions retain any in-flight image.
        }
        auto map=std::make_shared<Map>();map->settings=applied;map->layout=layout;maps.push_back(map);
        if(!CreateMap(*map,queue)){status.failed=true;break;}
    }
}
void Shutdown(){enabled.store(false,std::memory_order_relaxed);recordingEpoch.fetch_add(1,std::memory_order_release);std::lock_guard lock(mutex);requests.clear();
    // Submitted uploads remain retained until their fence completes, even on shutdown.
    const auto done=fence?fence->GetCompletedValue():0;std::erase_if(maps,[&](const auto& m){return m->completion && m->completion<=done;});
}
}
