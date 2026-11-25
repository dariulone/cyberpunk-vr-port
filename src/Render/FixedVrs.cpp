#include "Render/FixedVrs.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/ViewInstancedPipeline.hpp"
#include <wrl/client.h>
#include <algorithm>
#include <cstring>
#include <memory>
#include <mutex>
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
constexpr size_t kMaxSlotsPerPool=8;
class SlotLease final : public IUnknown {
    std::atomic<ULONG> refs{1};
public:
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid,void** ppv) override {
        if(!ppv)return E_POINTER;
        if(riid==IID_IUnknown){*ppv=static_cast<IUnknown*>(this);AddRef();return S_OK;}
        *ppv=nullptr;return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return refs.fetch_add(1,std::memory_order_relaxed)+1; }
    ULONG STDMETHODCALLTYPE Release() override {
        const ULONG n=refs.fetch_sub(1,std::memory_order_acq_rel)-1;
        if(!n)delete this;
        return n;
    }
    bool SoleOwner() const { return refs.load(std::memory_order_acquire)==1; }
};
struct Slot {
    ComPtr<SlotLease> lease;
    ComPtr<ID3D12Resource> image,upload;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{};
    UINT64 uploadBytes{};
    UINT texW{},texH{};
    StereoTileSignature sig{};
    uint64_t completion{};
    bool created{},ready{};
};
struct Pool {
    unsigned width{},height{};
    bool stereo{},dormant{};
    int eye{};
    uint64_t lastUsed{},lastSeenTick{};
    std::vector<std::shared_ptr<Slot>> slots;
    std::shared_ptr<Slot> active;
    std::shared_ptr<Slot> uploading;
};
struct State {
    bool known{},eligible{},depth{},blocked{},mapStereo{};
    UINT targets{},expectedTargets{},vpX{},width{},height{},targetWidth{},targetHeight{};
    D3D12_SHADING_RATE rate=D3D12_SHADING_RATE_1X1;
    std::array<D3D12_SHADING_RATE_COMBINER,2> combiners{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
    ID3D12Resource* image{};
    ID3D12Resource* retained{};
    ID3D12PipelineState* pipeline{};
    uint64_t revision{},recordingEpoch{};
    unsigned mapWidth{},mapHeight{};
    int mapEye{-1};
    ID3D12Resource* selectedImage{};
    ID3D12Resource* overrideImage{};
    RateFn rateFn{};
    ImageFn imageFn{};
};
struct MapRequest {
    unsigned width{},height{};
    bool stereo{};
    int eye{};
    bool operator==(const MapRequest&) const = default;
};
std::mutex mutex;
std::mutex statesMutex;
std::atomic<bool> everEnabled{};
std::atomic<bool> stateCapHit{};
std::atomic<uint64_t> revision{1};
std::atomic<uint64_t> recordingEpoch{1};
std::array<std::atomic<uint64_t>,2> draws{},sceneDraws{},eligibleDraws{};
Settings requested,applied;
DynamicEyeOffsets lastGaze{};
uint64_t applyAfter{},fenceValue{},useSerial{},tickSerial{};
uint64_t lastReport{};
Status status;
ComPtr<ID3D12Device> device;
ComPtr<ID3D12Fence> fence;
std::unordered_map<ID3D12GraphicsCommandList*,State> states;
std::vector<Pool> pools;
std::vector<MapRequest> requests;

State* FindState(ID3D12GraphicsCommandList* list,bool create=false) {
    // Recording one command list is externally serialized by D3D12's contract.
    // Registry entries never move or disappear; only reset their recording data.
    thread_local ID3D12GraphicsCommandList* previous{};
    thread_local State* cached{};
    if(previous==list && cached)return cached;
    std::lock_guard lock(statesMutex);auto it=states.find(list);
    if(it==states.end() && create){if(states.size()<2048)it=states.try_emplace(list).first;else stateCapHit.store(true,std::memory_order_relaxed);}
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
std::array<Settings,2> EffectiveEyes() {
    std::array<Settings,2> eyes{applied,applied};
    if(applied.mode!=0 && applied.eyeTracked && lastGaze.active) {
        for(int i=0;i<2;++i) {
            eyes[i].offsetX=lastGaze.offsetX[i];
            eyes[i].offsetY=lastGaze.offsetY[i];
            eyes[i]=Normalize(eyes[i]);
        }
    }
    return eyes;
}
void MarkFailed() {
    status.failed=true;status.readyMaps=0;
    for(auto& pool:pools)pool.active.reset();
    revision.fetch_add(1,std::memory_order_release);
}
bool UploadSlot(Pool& pool,const std::shared_ptr<Slot>& slotPtr,const std::vector<uint8_t>& bytes,const StereoTileSignature& sig,unsigned mapW,unsigned mapH,ID3D12CommandQueue* queue) {
    if(!slotPtr)return false;
    Slot& slot=*slotPtr;
    if(bytes.empty())return false;
    if(!slot.created) {
        if(!slot.lease)slot.lease.Attach(new SlotLease());
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        slot.texW=(mapW+status.tileSize-1)/status.tileSize;
        slot.texH=(mapH+status.tileSize-1)/status.tileSize;
        d.Width=slot.texW;d.Height=slot.texH;
        d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Format=DXGI_FORMAT_R8_UINT;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&slot.image))))return false;
        slot.image->SetName(L"CVR_fixed_foveated_shading_rate");
        device->GetCopyableFootprints(&d,0,1,0,&slot.fp,nullptr,nullptr,&slot.uploadBytes);
        D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=slot.uploadBytes;
        buffer.Height=1;buffer.DepthOrArraySize=1;buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        heap.Type=D3D12_HEAP_TYPE_UPLOAD;
        if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&slot.upload))))return false;
        if(FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&slot.allocator))) ||
           FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,slot.allocator.Get(),nullptr,IID_PPV_ARGS(&slot.list))))return false;
        slot.created=true;++status.allocations;
    } else {
        slot.ready=false;
        if(FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(),nullptr)))return false;
        gpu::ResetCommandResources(slot.list.Get());
        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={slot.image.Get(),0,D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE,D3D12_RESOURCE_STATE_COPY_DEST};
        slot.list->ResourceBarrier(1,&b);
    }
    void* data{};const D3D12_RANGE noRead{0,0};
    if(FAILED(slot.upload->Map(0,&noRead,&data))){slot.list->Close();return false;}
    std::memset(data,0,size_t(slot.uploadBytes));
    for(UINT y=0;y<slot.texH;++y)std::memcpy(static_cast<uint8_t*>(data)+slot.fp.Offset+y*slot.fp.Footprint.RowPitch,bytes.data()+size_t(y)*slot.texW,size_t(slot.texW));
    slot.upload->Unmap(0,nullptr);
    D3D12_TEXTURE_COPY_LOCATION from{},to{};
    from.pResource=slot.upload.Get();from.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;from.PlacedFootprint=slot.fp;
    to.pResource=slot.image.Get();
    slot.list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition={slot.image.Get(),0,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE};
    slot.list->ResourceBarrier(1,&b);
    if(FAILED(slot.list->Close()) || !gpu::KeepCommandResources(slot.list.Get(),{slot.lease.Get(),slot.image.Get(),slot.upload.Get(),slot.allocator.Get()}))return false;
    ID3D12CommandList* lists[]{slot.list.Get()};
    auto resources=gpu::PrepareCommandResources(1,lists);
    queue->ExecuteCommandLists(1,lists);
    gpu::SubmitCommandResources(queue,std::move(resources));
    slot.completion=++fenceValue;
    slot.sig=sig;slot.ready=false;pool.uploading=slotPtr;++status.uploads;
    if(FAILED(queue->Signal(fence.Get(),slot.completion)))return false;
    return true;
}
}
Settings GetSettings(){std::lock_guard lock(mutex);return requested;}
Settings GetAppliedSettings(){std::lock_guard lock(mutex);return applied;}
void SetSettings(Settings s) {
    s=Normalize(s);std::lock_guard lock(mutex);
    const bool on=s.mode!=0;
    if(enabled.load(std::memory_order_relaxed)!=on)recordingEpoch.fetch_add(1,std::memory_order_release);
    enabled.store(on,std::memory_order_relaxed);
    if(on)everEnabled.store(true,std::memory_order_relaxed);
    if(s==requested)return;
    if(!s.mode) {
        requested=applied=s;applyAfter=0;
        for(auto& pool:pools){pool.active.reset();pool.dormant=true;}
        status.readyMaps=0;
        revision.fetch_add(1,std::memory_order_release);
    } else if(applied.mode!=0 && s.SameAllocation(applied) && s.SameAllocation(requested)) {
        requested=applied=s;applyAfter=0;revision.fetch_add(1,std::memory_order_release);
    } else {
        requested=s;applyAfter=GetTickCount64()+1000;
    }
}
Status GetStatus(){std::lock_guard lock(mutex);auto value=status;value.appliedMode=applied.mode;value.stateCapHit=stateCapHit.load(std::memory_order_relaxed);for(int i=0;i<2;++i){value.draws[i]=draws[i].load();value.sceneDraws[i]=sceneDraws[i].load();value.eligibleDraws[i]=eligibleDraws[i].load();}return value;}
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
    s.width=s.height=s.vpX=0;
    if(n!=1 || !v || v->TopLeftY!=0 || !(v->Width>=64 && v->Width<=16384 && v->Height>=64 && v->Height<=16384) ||
       !(v->TopLeftX>=0 && v->TopLeftX<=16384) || float(UINT(v->Width))!=v->Width || float(UINT(v->Height))!=v->Height ||
       float(UINT(v->TopLeftX))!=v->TopLeftX)return;
    const UINT w=UINT(v->Width),x=UINT(v->TopLeftX);
    if(x!=0 && x!=w && x!=w+1)return;
    s.vpX=x;s.width=w;s.height=UINT(v->Height);
});}
void Targets(ID3D12GraphicsCommandList* list,UINT count,bool depth,unsigned targetWidth,unsigned targetHeight,bool blocked){Edit(list,[&](State& s){
    s.targets=count;s.depth=depth;s.targetWidth=targetWidth;s.targetHeight=targetHeight;s.blocked=blocked;
});}
void ShadingRate(ID3D12GraphicsCommandList* list,D3D12_SHADING_RATE rate,const D3D12_SHADING_RATE_COMBINER* c){Edit(list,[&](State& s){
    s.rate=rate;s.combiners=c?std::array{c[0],c[1]}:std::array{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
});}
void ShadingImage(ID3D12GraphicsCommandList* list,ID3D12Resource* image){Edit(list,[&](State& s){s.image=image;});}

void PrepareDraw(ID3D12GraphicsCommandList* base,uint32_t node,int eye,RateFn setRate,ImageFn setImage) {
    if(stereo::probe::internalCommands || !everEnabled.load(std::memory_order_relaxed))return;
    auto* state=FindState(base);if(!state || !state->known)return;auto& s=*state;
    if(!enabled.load(std::memory_order_relaxed) || s.recordingEpoch!=recordingEpoch.load(std::memory_order_acquire) || node!=kRenderElementsNode || eye<0 || eye>1 || !setRate || !setImage){RestoreState(base,s);return;}
    const bool singleEye=!s.blocked && s.vpX==0 && (s.targetWidth==0 || (s.targetWidth==s.width && s.targetHeight==s.height));
    const bool sideBySide=!s.blocked && s.width>0 && s.targetWidth>=128 && s.targetHeight==s.height && ((s.vpX==0 && (s.targetWidth==s.width*2 || s.targetWidth+1==s.width*2)) || (s.width==s.targetWidth/2 && s.vpX==s.targetWidth-s.width));
    const int drawEye=(s.vpX>0 && s.targetHeight==s.height && s.width==s.targetWidth/2 && s.vpX==s.targetWidth-s.width)?1:eye;
    sceneDraws[drawEye].fetch_add(1,std::memory_order_relaxed);
    if(!s.eligible || !s.depth || s.targets!=s.expectedTargets || !s.width || !s.height || !(singleEye || sideBySide)){RestoreState(base,s);return;}
    eligibleDraws[drawEye].fetch_add(1,std::memory_order_relaxed);
    const unsigned reqW=sideBySide?s.targetWidth:s.width,reqH=sideBySide?s.targetHeight:s.height;
    const auto version=revision.load(std::memory_order_acquire);
    const int lookupEye=sideBySide?0:drawEye;
    if((!s.selectedImage && s.revision!=version) || s.mapWidth!=reqW || s.mapHeight!=reqH || s.mapStereo!=sideBySide || s.mapEye!=lookupEye) {
        ComPtr<SlotLease> lease;
        ComPtr<ID3D12Resource> img;
        {
            std::lock_guard lock(mutex);
            s.revision=version;s.mapWidth=reqW;s.mapHeight=reqH;s.mapStereo=sideBySide;s.mapEye=lookupEye;
            if(applied.mode && status.supported && !status.failed) {
                const int poolEye=sideBySide?0:(applied.eyeTracked?drawEye:0);
                const auto found=std::find_if(pools.begin(),pools.end(),[&](const auto& p){
                    return p.width==reqW && p.height==reqH && p.stereo==sideBySide && p.eye==poolEye;
                });
                if(found!=pools.end()) {
                    found->dormant=false;
                    found->lastSeenTick=tickSerial;
                    if(found->active) {
                        lease=found->active->lease;
                        img=found->active->image;
                        found->lastUsed=++useSerial;
                    }
                } else {
                    const MapRequest req{reqW,reqH,sideBySide,poolEye};
                    if(requests.size()<8 && std::find(requests.begin(),requests.end(),req)==requests.end())requests.push_back(req);
                }
            }
        }
        if(img && s.retained!=img.Get()) {
            if(!gpu::KeepCommandResources(base,{lease.Get(),img.Get()})){s.selectedImage=nullptr;RestoreState(base,s);return;}
            s.retained=img.Get();
        }
        s.selectedImage=img.Get();
    }
    if(!s.selectedImage){RestoreState(base,s);return;}
    draws[drawEye].fetch_add(1,std::memory_order_relaxed);
    if(s.overrideImage==s.selectedImage)return;
    RestoreState(base,s);
    auto* list=static_cast<ID3D12GraphicsCommandList5*>(base);s.rateFn=setRate;s.imageFn=setImage;s.overrideImage=s.selectedImage;
    const D3D12_SHADING_RATE_COMBINER combine[]{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_OVERRIDE};
    setImage(list,s.selectedImage);setRate(list,D3D12_SHADING_RATE_1X1,combine);
}
void Restore(ID3D12GraphicsCommandList* list){if(stereo::probe::internalCommands || !everEnabled.load(std::memory_order_relaxed))return;if(auto* s=FindState(list))RestoreState(list,*s);}
void Tick(ID3D12Device* current,ID3D12CommandQueue* queue,const DynamicEyeOffsets* gaze) {
    if(!current || !queue)return;
    stereo::probe::Scope internal;
    gpu::CollectCommandResources();
    std::lock_guard lock(mutex);
    lastGaze=gaze?*gaze:DynamicEyeOffsets{};
    const auto now=GetTickCount64();
    if(now-lastReport>=250) {
        lastReport=now;InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_FixedVrsReportSeq));
        std::snprintf(CyberpunkVR_FixedVrsReport,sizeof(CyberpunkVR_FixedVrsReport),
            "{\"requested\":%d,\"applied\":%d,\"eyeTracked\":%d,\"checked\":%d,\"supported\":%d,\"additional\":%d,\"failed\":%d,\"tile\":%u,\"maps\":%u,\"uploads\":%llu,\"skipped\":%llu,\"draws\":[%llu,%llu],\"scene\":[%llu,%llu],\"eligible\":[%llu,%llu]}",
            requested.mode,applied.mode,int(applied.eyeTracked),int(status.checked),int(status.supported),int(status.additionalRates),int(status.failed),status.tileSize,status.readyMaps,
            status.uploads,status.skippedTicks,
            draws[0].load(),draws[1].load(),sceneDraws[0].load(),sceneDraws[1].load(),eligibleDraws[0].load(),eligibleDraws[1].load());
        InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_FixedVrsReportSeq));
    }
    if(device && device.Get()!=current){MarkFailed();return;}
    if(!status.checked) {
        if(!enabled.load(std::memory_order_relaxed))return;
        device=current;status.checked=true;D3D12_FEATURE_DATA_D3D12_OPTIONS6 options{};
        status.supported=SUCCEEDED(device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS6,&options,sizeof(options))) &&
            options.VariableShadingRateTier>=D3D12_VARIABLE_SHADING_RATE_TIER_2 && options.ShadingRateImageTileSize>0;
        status.tileSize=options.ShadingRateImageTileSize;status.additionalRates=options.AdditionalShadingRatesSupported!=FALSE;
        if(status.supported && FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))))status.failed=true;
        revision.fetch_add(1,std::memory_order_release);
    }
    if(!status.supported || status.failed)return;
    ++tickSerial;
    if(now>=applyAfter && !(applied==requested)) {
        if(!requested.mode) {
            for(auto& pool:pools){pool.active.reset();pool.dormant=true;}
        }
        applied=requested;requests.clear();revision.fetch_add(1,std::memory_order_release);
    }
    const auto done=fence->GetCompletedValue();if(done==UINT64_MAX){MarkFailed();return;}
    for(auto& pool:pools) {
        for(auto& slot:pool.slots) {
            if(slot->completion && slot->completion<=done && !slot->ready) {
                slot->ready=true;
                gpu::ResetCommandResources(slot->list.Get());
                if(pool.uploading==slot) {
                    if(!pool.dormant)pool.active=slot;
                    pool.uploading.reset();
                    revision.fetch_add(1,std::memory_order_release);
                }
            }
        }
    }
    if(!applied.mode) {
        for(auto& pool:pools){pool.active.reset();pool.dormant=true;}
        status.readyMaps=0;
        return;
    }
    const auto eyes=EffectiveEyes();
    const bool singleShared=!applied.eyeTracked;
    auto pending=std::move(requests);requests.clear();
    for(const auto& req:pending) {
        const auto existing=std::find_if(pools.begin(),pools.end(),[&](const auto& p){return p.width==req.width && p.height==req.height && p.stereo==req.stereo && p.eye==req.eye;});
        if(existing!=pools.end()){existing->dormant=false;existing->lastSeenTick=tickSerial;continue;}
        if(pools.size()>=16) {
            auto victim=pools.end();
            for(auto it=pools.begin();it!=pools.end();++it)if(!it->uploading && (victim==pools.end() || it->lastUsed<victim->lastUsed))victim=it;
            if(victim==pools.end())break;
            pools.erase(victim); // Active/recorded command lists retain any in-flight image via SlotLease.
        }
        Pool p;p.width=req.width;p.height=req.height;p.stereo=req.stereo;p.eye=req.eye;p.lastUsed=++useSerial;p.lastSeenTick=tickSerial;
        pools.push_back(std::move(p));
    }
    for(auto& pool:pools) {
        if(!pool.dormant && tickSerial>pool.lastSeenTick+30)pool.dormant=true;
        if(!pool.stereo && singleShared && pool.eye==1){pool.active.reset();continue;}
        if(pool.dormant)continue;
        const StereoTileSignature targetSig=pool.stereo
            ?ComputeStereoTileSignature(eyes[0],eyes[1],pool.width,pool.height,status.tileSize,status.additionalRates)
            :StereoTileSignature{ComputeTileSignature(eyes[pool.eye],pool.width,pool.height,status.tileSize,status.additionalRates),{}};
        const auto& latest=pool.uploading?pool.uploading:pool.active;
        if(latest && latest->sig==targetSig) {
            if(!pool.active && pool.uploading && pool.uploading->ready){pool.active=pool.uploading;pool.uploading.reset();revision.fetch_add(1,std::memory_order_release);}
            else if(!pool.active && !pool.uploading) {
                for(const auto& candidate:pool.slots)if(candidate->ready && candidate->sig==targetSig){pool.active=candidate;revision.fetch_add(1,std::memory_order_release);break;}
            }
            ++status.skippedTicks;
        } else if(!pool.uploading) {
            // If an existing ready slot already holds the exact target tile signature (e.g. waking from dormant), activate it directly!
            std::shared_ptr<Slot> matchingReady;
            for(const auto& candidate:pool.slots) {
                if(candidate->created && candidate->ready && candidate->completion<=done && candidate->sig==targetSig){matchingReady=candidate;break;}
            }
            if(matchingReady) {
                pool.active=matchingReady;
                revision.fetch_add(1,std::memory_order_release);
                ++status.skippedTicks;
                continue;
            }
            std::shared_ptr<Slot> slot;
            for(const auto& candidate:pool.slots) {
                if(candidate!=pool.active && (!candidate->created || (candidate->ready && candidate->completion<=done && candidate->lease->SoleOwner()))) {
                    slot=candidate;break;
                }
            }
            if(!slot && pool.slots.size()<kMaxSlotsPerPool) {
                slot=std::make_shared<Slot>();
                pool.slots.push_back(slot);
            }
            if(slot) {
                const auto bytes=pool.stereo
                    ?BuildStereoRates(eyes[0],eyes[1],pool.width,pool.height,status.tileSize,status.additionalRates)
                    :BuildRates(eyes[pool.eye],pool.width,pool.height,status.tileSize,status.additionalRates);
                if(!UploadSlot(pool,slot,bytes,targetSig,pool.width,pool.height,queue)){MarkFailed();return;}
            }
        }
    }
    const bool anyLive=std::any_of(pools.begin(),pools.end(),[&](const Pool& p){
        return !p.dormant && !(!p.stereo && singleShared && p.eye==1);
    });
    status.readyMaps=0;
    if(anyLive) {
        for(const auto& pool:pools) {
            if(pool.dormant || (!pool.stereo && singleShared && pool.eye==1))continue;
            if(pool.active)++status.readyMaps;
        }
    } else {
        uint64_t latestTick=0;
        for(const auto& pool:pools) {
            if(!pool.stereo && singleShared && pool.eye==1)continue;
            if(pool.active && pool.lastSeenTick>latestTick)latestTick=pool.lastSeenTick;
        }
        if(latestTick) {
            for(const auto& pool:pools) {
                if(!pool.stereo && singleShared && pool.eye==1)continue;
                if(pool.active && pool.lastSeenTick==latestTick)++status.readyMaps;
            }
        }
    }
}
void Shutdown() {
    enabled.store(false,std::memory_order_relaxed);
    recordingEpoch.fetch_add(1,std::memory_order_release);
    gpu::CollectCommandResources();
    std::lock_guard lock(mutex);
    requests.clear();
    status.readyMaps=0;
    applied={};
    revision.fetch_add(1,std::memory_order_release);
    // Submitted uploads remain retained until their fence completes, even on shutdown.
    const auto done=fence?fence->GetCompletedValue():0;
    for(auto& pool:pools) {
        pool.active.reset();
        pool.dormant=true;
        if(done!=UINT64_MAX) {
            for(auto& slot:pool.slots) {
                if(slot->created && slot->completion<=done) {
                    slot->ready=true;
                    if(pool.uploading==slot)pool.uploading.reset();
                    gpu::ResetCommandResources(slot->list.Get());
                }
            }
        }
    }
    if(done!=UINT64_MAX) {
        std::erase_if(pools,[&](const Pool& p){
            return std::all_of(p.slots.begin(),p.slots.end(),[&](const std::shared_ptr<Slot>& s){
                return !s->created || s->completion<=done;
            });
        });
    }
}
}
