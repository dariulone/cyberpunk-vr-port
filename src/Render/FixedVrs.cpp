#include "Render/FixedVrs.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/ViewInstancedPipeline.hpp"
#include "Stereo/EngineStereo.hpp"
#include <wrl/client.h>
#include <algorithm>
#include <cstring>
#include <memory>
#include <mutex>
#include <unordered_map>

#if !CVR_RUST_VRIK
extern "C" {
__declspec(dllexport) uint32_t CyberpunkVR_FixedVrsReportSeq=0;
__declspec(dllexport) char CyberpunkVR_FixedVrsReport[1024]="{}";
}
#endif

#if CVR_RUST_VRIK
// The GPU half: rust/crates/cvr-render/src/fixed_vrs.rs; its exports (the moved exports' modules) and the engine-stereo
// query are attached by export_attach.rs as the plugin loads. The pipeline stream reader stays here.
namespace cvr::vrs {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's pipeline hooks read the stream themselves (pso_hooks.rs)
void StreamCreated(ID3D12PipelineState* pso,const D3D12_PIPELINE_STATE_STREAM_DESC& source) {
    stereo::PipelineStreamInfo info;
    if(!pso || stereo::probe::internalCommands || !stereo::ReadPipelineStream(source,info))return;
    D3D12_GRAPHICS_PIPELINE_STATE_DESC d{};d.PS=info.ps;d.VS=info.vs;d.NumRenderTargets=info.targets.NumRenderTargets;
    d.DepthStencilState=info.depth;d.BlendState=info.blend;d.SampleDesc=info.samples;
    if(!info.haveDepth || !info.haveBlend || info.otherStages || info.viewInstancing || info.forcedSamples)return;
    GraphicsCreated(pso,d);
}
#endif
}
#else
namespace cvr::vrs {
using Microsoft::WRL::ComPtr;
std::atomic<bool> enabled{};
namespace {
constexpr GUID pipelineKey{0x5a4ac73c,0x80da,0x4983,{0xb5,0x12,0x1f,0x54,0xa9,0xa0,0xbe,0x47}};
struct Map;
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
    std::shared_ptr<Map> selected;
    ID3D12Resource* overrideImage{};
    RateFn rateFn{};
    ImageFn imageFn{};
};
struct Map {
    Settings settings;
    unsigned width{},height{};
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
Settings requested,applied;
uint64_t applyAfter{},fenceValue{},useSerial{};
uint64_t lastReport{};
Status status;
ComPtr<ID3D12Device> device;
ComPtr<ID3D12Fence> fence;
std::unordered_map<ID3D12GraphicsCommandList*,State> states;
std::vector<std::shared_ptr<Map>> maps;
std::vector<std::pair<unsigned,unsigned>> requests;

State* FindState(ID3D12GraphicsCommandList* list,bool create=false) {
    // Recording one command list is externally serialized by D3D12's contract.
    // Registry entries never move or disappear; only reset their recording data.
    thread_local ID3D12GraphicsCommandList* previous{};
    thread_local State* cached{};
    if(previous==list && cached)return cached;
    std::lock_guard lock(statesMutex);auto it=states.find(list);
    if(it==states.end() && create && states.size()<512)it=states.try_emplace(list).first;
    previous=list;cached=it==states.end()?nullptr:&it->second;return cached;
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
    const auto bytes=BuildRates(map.settings,map.width,map.height,status.tileSize,status.additionalRates);
    if(bytes.empty())return false;
    D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    d.Width=(map.width+status.tileSize-1)/status.tileSize;d.Height=(map.height+status.tileSize-1)/status.tileSize;
    d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Format=DXGI_FORMAT_R8_UINT;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&map.image))))return false;
    map.image->SetName(L"CVR_fixed_foveated_shading_rate");
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
void SetSettings(Settings s) {
    s=Normalize(s);std::lock_guard lock(mutex);
    // The rate map is laid out for one eye per target; engine stereo has both in one. The setting itself is
    // kept, so it is back when the mode is.
    const bool on=s.mode!=0 && !engine_stereo::Enabled();
    if(enabled.load(std::memory_order_relaxed)!=on)recordingEpoch.fetch_add(1,std::memory_order_release);
    enabled.store(on,std::memory_order_relaxed);
    if(on)everEnabled.store(true,std::memory_order_relaxed);
    if(s==requested)return;
    requested=s;applyAfter=GetTickCount64()+1000;
    if(!s.mode){applied=s;revision.fetch_add(1,std::memory_order_release);}
}
Status GetStatus(){std::lock_guard lock(mutex);auto value=status;for(int i=0;i<2;++i){value.draws[i]=draws[i].load();value.sceneDraws[i]=sceneDraws[i].load();value.eligibleDraws[i]=eligibleDraws[i].load();}return value;}
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
    s.width=s.height=0;
    if(n!=1 || !v || v->TopLeftX!=0 || v->TopLeftY!=0 || !(v->Width>=64 && v->Width<=16384 && v->Height>=64 && v->Height<=16384) ||
       float(UINT(v->Width))!=v->Width || float(UINT(v->Height))!=v->Height)return;
    s.width=UINT(v->Width);s.height=UINT(v->Height);
});}
void Targets(ID3D12GraphicsCommandList* list,UINT count,bool depth){Edit(list,[&](State& s){s.targets=count;s.depth=depth;});}
void ShadingRate(ID3D12GraphicsCommandList* list,D3D12_SHADING_RATE rate,const D3D12_SHADING_RATE_COMBINER* c){Edit(list,[&](State& s){
    s.rate=rate;s.combiners=c?std::array{c[0],c[1]}:std::array{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
});}
void ShadingImage(ID3D12GraphicsCommandList* list,ID3D12Resource* image){Edit(list,[&](State& s){s.image=image;});}

void PrepareDraw(ID3D12GraphicsCommandList* base,uint32_t node,int eye,RateFn setRate,ImageFn setImage) {
    if(stereo::probe::internalCommands || !everEnabled.load(std::memory_order_relaxed))return;
    auto* state=FindState(base);if(!state || !state->known)return;auto& s=*state;
    if(!enabled.load(std::memory_order_relaxed) || s.recordingEpoch!=recordingEpoch.load(std::memory_order_acquire) || node!=0x33DDA4 || eye<0 || eye>1 || !setRate || !setImage){RestoreState(base,s);return;}
    sceneDraws[eye].fetch_add(1,std::memory_order_relaxed);
    if(!s.eligible || !s.depth || s.targets!=s.expectedTargets || !s.width || !s.height){RestoreState(base,s);return;}
    eligibleDraws[eye].fetch_add(1,std::memory_order_relaxed);
    const auto version=revision.load(std::memory_order_acquire);
    if(s.revision!=version || s.mapWidth!=s.width || s.mapHeight!=s.height) {
        std::lock_guard lock(mutex);s.selected.reset();s.revision=version;s.mapWidth=s.width;s.mapHeight=s.height;
        if(applied.mode && status.supported && !status.failed) {
            const auto found=std::find_if(maps.begin(),maps.end(),[&](const auto& m){return m->settings==applied && m->width==s.width && m->height==s.height;});
            if(found!=maps.end()){if((*found)->ready){s.selected=*found;s.selected->lastUsed=++useSerial;}}
            else {const auto size=std::pair{s.width,s.height};if(requests.size()<8 && std::find(requests.begin(),requests.end(),size)==requests.end())requests.push_back(size);}
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
            "{\"requested\":%d,\"applied\":%d,\"checked\":%d,\"supported\":%d,\"additional\":%d,\"failed\":%d,\"tile\":%u,\"maps\":%u,\"draws\":[%llu,%llu],\"scene\":[%llu,%llu],\"eligible\":[%llu,%llu]}",
            requested.mode,applied.mode,int(status.checked),int(status.supported),int(status.additionalRates),int(status.failed),status.tileSize,status.readyMaps,
            draws[0].load(),draws[1].load(),sceneDraws[0].load(),sceneDraws[1].load(),eligibleDraws[0].load(),eligibleDraws[1].load());
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
    for(auto [w,h]:pending) {
        if(std::any_of(maps.begin(),maps.end(),[&](const auto& m){return m->width==w && m->height==h && m->settings==applied;}))continue;
        if(maps.size()>=16) {
            auto victim=maps.end();for(auto it=maps.begin();it!=maps.end();++it)if((*it)->ready && (victim==maps.end() || (*it)->lastUsed<(*victim)->lastUsed))victim=it;
            if(victim==maps.end())break;
            maps.erase(victim); // Recorded lists/submissions retain any in-flight image.
        }
        auto map=std::make_shared<Map>();map->settings=applied;map->width=w;map->height=h;maps.push_back(map);
        if(!CreateMap(*map,queue)){status.failed=true;break;}
    }
}
void Shutdown(){enabled.store(false,std::memory_order_relaxed);recordingEpoch.fetch_add(1,std::memory_order_release);std::lock_guard lock(mutex);requests.clear();
    // Submitted uploads remain retained until their fence completes, even on shutdown.
    const auto done=fence?fence->GetCompletedValue():0;std::erase_if(maps,[&](const auto& m){return m->completion && m->completion<=done;});
}
}
#endif
