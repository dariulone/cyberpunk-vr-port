#include "Render/NativeTargetHandoff.hpp"
#include "Render/NativeTextureRead.hpp"
#include <algorithm>
#include <mutex>
#include <utility>
#if CVR_RUST_VRIK
// The observer -- the two sources, the observation window, the capture epoch, their lock and every event -- is
// rust/crates/cvr-render/src/target_handoff_observer.rs (the detours call it directly). Here: the export rows its
// publication fills, and the API the C++ callers (the retained geometry, the target access) and the test variants use.
extern "C" {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
std::atomic<uint32_t> CyberpunkVR_TargetHandoffSeq{};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
uint64_t CyberpunkVR_TargetHandoffStats[16]{2};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
uint64_t CyberpunkVR_TargetHandoffSources[32]{};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
uint64_t CyberpunkVR_TargetHandoffSourceLocal[16]{};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
uint64_t CyberpunkVR_TargetHandoffOwners[16]{};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
uint64_t CyberpunkVR_TargetHandoffTargets[192]{};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
uint64_t CyberpunkVR_TargetHandoffStates[80]{};
#endif
}
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's handoff is target_handoff_observer.rs' and targets.rs'
extern "C" {
bool cvr_th2_begin(ID3D12GraphicsCommandList*,const cvr::stereo::native_recording::Lease*,uint32_t,uint32_t,
                   const cvr::stereo::scene_state::GeometryTargets*);
bool cvr_th2_snapshot(ID3D12CommandQueue*,uint32_t,cvr::stereo::target_handoff::SourceSnapshot*);
void cvr_th2_mark_exit(ID3D12GraphicsCommandList*,uint32_t,uint32_t);
void cvr_th2_reset(ID3D12GraphicsCommandList*);
void cvr_th2_finish_observation();
void cvr_th2_clear();
bool cvr_th2_observing();
void cvr_th2_barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void cvr_th2_written(ID3D12GraphicsCommandList*,ID3D12Resource*,uint32_t);
void cvr_th2_clear_view(ID3D12GraphicsCommandList*,size_t,bool);
void cvr_th2_targets(ID3D12GraphicsCommandList*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,bool,const D3D12_CPU_DESCRIPTOR_HANDLE*);
void cvr_th2_draw(ID3D12GraphicsCommandList*);
void cvr_th2_unknown(ID3D12GraphicsCommandList*,uint32_t);
void cvr_th2_close(ID3D12GraphicsCommandList*,HRESULT);
void cvr_th2_before_submit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void cvr_th2_submitted(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
}
namespace cvr::stereo::target_handoff {
static_assert(sizeof(scene_state::GeometryTargets)==328 && sizeof(TargetSnapshot)==448 && sizeof(SourceSnapshot)==1840,
    "mirrored by target_handoff_observer.rs");
bool Begin(ID3D12GraphicsCommandList* list,const native_recording::Lease& recording,uint32_t frame,uint32_t side,
           const scene_state::GeometryTargets& targets){return cvr_th2_begin(list,&recording,frame,side,&targets);}
void Reset(ID3D12GraphicsCommandList* list){cvr_th2_reset(list);}
// The snapshots take the observer's copies (their references handed over), then each identity its authenticated copy.
bool SnapshotSources(ID3D12CommandQueue* queue,uint32_t frame,std::array<SourceSnapshot,2>& output)try{
    output={};std::array<SourceSnapshot,2> next;if(!cvr_th2_snapshot(queue,frame,next.data()))return false;
    for(auto& source:next)for(auto& target:source.targets)texture_read::AuthenticateWritableIdentity(target.identity);
    output=std::move(next);return true;
}catch(...){output={};return false;}
void FinishObservation(){cvr_th2_finish_observation();}
void Clear(){cvr_th2_clear();}
bool Observing()noexcept{return cvr_th2_observing();}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers){cvr_th2_barriers(list,count,barriers);}
void Written(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,uint32_t kind){cvr_th2_written(list,destination,kind);}
void ClearView(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE handle,bool depth){cvr_th2_clear_view(list,handle.ptr,depth);}
void Targets(ID3D12GraphicsCommandList* list,UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* handles,BOOL contiguous,
             const D3D12_CPU_DESCRIPTOR_HANDLE* depth){cvr_th2_targets(list,count,handles,contiguous!=FALSE,depth);}
void Draw(ID3D12GraphicsCommandList* list){cvr_th2_draw(list);}
void Unknown(ID3D12GraphicsCommandList* list,uint32_t kind){cvr_th2_unknown(list,kind);}
void Close(ID3D12GraphicsCommandList* list,HRESULT result){cvr_th2_close(list,result);}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){cvr_th2_before_submit(queue,count,lists);}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){cvr_th2_submitted(queue,count,lists);}
}
#endif
#endif
#else

extern "C" {
std::atomic<uint32_t> CyberpunkVR_TargetHandoffSeq{};
uint64_t CyberpunkVR_TargetHandoffStats[16]{2};
uint64_t CyberpunkVR_TargetHandoffSources[32]{};
uint64_t CyberpunkVR_TargetHandoffSourceLocal[16]{};
uint64_t CyberpunkVR_TargetHandoffOwners[16]{};
uint64_t CyberpunkVR_TargetHandoffTargets[192]{};
uint64_t CyberpunkVR_TargetHandoffStates[80]{};
}
namespace cvr::stereo::target_handoff {
namespace {
struct Target {
    texture_read::WritableTargetIdentity identity;
    scene_state::TargetInfo view{};
    D3D12_CPU_DESCRIPTOR_HANDLE handle{};
    detail::TargetState state;
    UINT slot{},planes{},stateFirst{};
};
struct Source {
    detail::RecordingState state;
    native_recording::Lease recording;
    std::array<Target,TargetsPerSource> targets;
    uint32_t frame{},side{},targetCount{},stateCount{};
    bool occupied{},capturing{},captureTouched{},retiredRecording{};
};
std::mutex mutex;
std::array<Source,SourceLimit> sources;
std::atomic<bool> observing{};
detail::ObservationWindow window;
uint64_t captureEpoch{};
struct Publication {
    Publication(){CyberpunkVR_TargetHandoffSeq.fetch_add(1,std::memory_order_acq_rel);}
    ~Publication(){CyberpunkVR_TargetHandoffSeq.fetch_add(1,std::memory_order_release);}
};
constexpr UINT BindingLimit=64;
struct BoundTargets {
    ID3D12GraphicsCommandList* list{};
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,9> resources;
    bool known{};
};
std::array<BoundTargets,BindingLimit> bindings;
bool bindingOverflow{};
uint64_t Address(const void* value){return reinterpret_cast<uintptr_t>(value);}
void Count(UINT index){auto& value=CyberpunkVR_TargetHandoffStats[index];if(value!=UINT64_MAX)++value;}
bool Event(ID3D12GraphicsCommandList* list){
    if(!window.Event())return false;
    for(auto& source:sources)if(source.capturing && source.state.list==Address(list))source.captureTouched=true;
    return true;
}
bool SameRecording(const native_recording::Lease& a,const native_recording::Lease& b){
    return a.token && a.token==b.token && a.generation==b.generation && a.context==b.context && a.list==b.list && a.registry==b.registry;
}
void Reject(Source& source,Failure failure){
    source.state.Reject(failure);
    for(UINT i=0;i<source.targetCount;++i)source.targets[i].state.Reject(failure);
}
void Publish(){
    auto& stats=CyberpunkVR_TargetHandoffStats;stats[1]=stats[2]=stats[3]=stats[14]=0;
    std::fill_n(CyberpunkVR_TargetHandoffSources,32,0);
    std::fill_n(CyberpunkVR_TargetHandoffSourceLocal,16,0);
    std::fill_n(CyberpunkVR_TargetHandoffOwners,16,0);
    std::fill_n(CyberpunkVR_TargetHandoffTargets,192,0);
    std::fill_n(CyberpunkVR_TargetHandoffStates,80,0);
    for(UINT side=0;side<SourceLimit;++side){const auto& source=sources[side];if(!source.occupied)continue;
        ++stats[1];stats[2]+=source.targetCount;stats[3]+=source.stateCount;
        const auto& state=source.state;auto* row=CyberpunkVR_TargetHandoffSources+side*16;
        if(state.AggregateFlags()&PendingValidated)++stats[14];
        const std::array<uint64_t,16> values{source.frame,side,state.list,state.generation,source.recording.context,
            source.recording.registry,side*TargetsPerSource,source.targetCount,side*StatesPerSource,source.stateCount,
            state.AggregateFlags(),state.exitReason,state.exitKind,uint32_t(state.closeResult),state.queue,uint32_t(state.AggregateFailure())};
        std::copy(values.begin(),values.end(),row);
        const std::array<uint64_t,8> local{state.flags,uint32_t(state.failure),uint32_t(state.closeResult),uint32_t(state.foreignFailure),
            state.foreignList,state.foreignKind,state.foreignEvents,(state.flags&SubmittedObserved)?1ull:0ull};
        std::copy(local.begin(),local.end(),CyberpunkVR_TargetHandoffSourceLocal+side*8);
        for(UINT index=0;index<source.targetCount;++index){const auto& target=source.targets[index];
            const auto targetIndex=side*TargetsPerSource+index;const auto& identity=target.identity;
            const auto& creation=identity.creation;const auto& desc=identity.resourceDesc;
            auto* output=CyberpunkVR_TargetHandoffTargets+targetIndex*24;
            const std::array<uint64_t,24> fields{side,target.slot,Address(creation.resource.Get()),target.view.identity,
                identity.native.registry,identity.native.handle,creation.kind,Address(creation.heap.Get()),creation.heapOffset,
                creation.allocationBytes,desc.Width,desc.Height,UINT(desc.Format),target.view.viewFormat,UINT(desc.Flags),
                target.view.viewFlags,desc.MipLevels,desc.DepthOrArraySize,target.planes,target.stateFirst,target.state.count,
                uint64_t(Captured)|(target.state.failure!=Failure::None?uint64_t(Refused):0),uint32_t(target.state.failure),
                UINT(identity.expectedWritableState)};
            std::copy(fields.begin(),fields.end(),output);
            uint32_t ownerTag{};std::memcpy(&ownerTag,identity.nativeRecord.data()+8,sizeof(ownerTag));
            CyberpunkVR_TargetHandoffOwners[targetIndex*2]=ownerTag;
            CyberpunkVR_TargetHandoffOwners[targetIndex*2+1]=identity.allocatorOwnerObservation;
            for(UINT subresource=0;subresource<target.state.count;++subresource){const auto& current=target.state.states[subresource];
                auto* subrow=CyberpunkVR_TargetHandoffStates+(target.stateFirst+subresource)*8;
                const std::array<uint64_t,8> fields{side,targetIndex,subresource,current.seed,current.current,current.frozen,current.flags,current.transitions};
                std::copy(fields.begin(),fields.end(),subrow);
            }
        }
    }
}
bool TargetShape(ID3D12Device* device,Target& target,UINT slot,const scene_state::GeometryTargets& targets){
    const auto& desc=target.identity.resourceDesc;const auto& view=targets.views[slot];
    if(!device || !view.identity || view.resource!=Address(target.identity.creation.resource.Get()) || !targets.handles[slot].ptr ||
        view.viewFlags || desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || !desc.Width || !desc.Height ||
        desc.MipLevels!=1 || desc.DepthOrArraySize!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality ||
        desc.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN || view.width!=desc.Width || view.height!=desc.Height ||
        view.format!=UINT(desc.Format) || view.resourceFlags!=UINT(desc.Flags))return false;
    D3D12_FEATURE_DATA_FORMAT_INFO format{desc.Format,0};
    if(FAILED(device->CheckFeatureSupport(D3D12_FEATURE_FORMAT_INFO,&format,sizeof(format))) ||
        format.PlaneCount!=(slot==4?2u:1u))return false;
    if(slot==4){
        if((desc.Format!=DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format!=DXGI_FORMAT_D32_FLOAT_S8X24_UINT) ||
            view.viewFormat!=DXGI_FORMAT_D32_FLOAT_S8X24_UINT || !(desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL))return false;
    }else{
        constexpr std::array<DXGI_FORMAT,3> formats{DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R8G8B8A8_UNORM};
        if(!(desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET) || view.viewFormat!=UINT(desc.Format) || desc.Format!=formats[slot])return false;
    }
    target.slot=slot;target.planes=format.PlaneCount;target.view=view;target.handle=targets.handles[slot];
    return target.state.Seed(format.PlaneCount,UINT(target.identity.expectedWritableState));
}
bool Affects(const Target& target,ID3D12Resource* before,ID3D12Resource* after=nullptr){
    return texture_read::AliasAffects(target.identity.creation.resource.Get(),before,after);
}
bool ForeignRecording(const Source& source,ID3D12GraphicsCommandList* list){
    return source.retiredRecording || source.state.list!=Address(list);
}
void UnknownSource(Source& source,ID3D12GraphicsCommandList* list,uint32_t kind,Failure failure=Failure::UnknownCommand){
    if(ForeignRecording(source,list))source.state.Foreign(Address(list),kind);
    else Reject(source,failure);
}
void UnknownLocked(ID3D12GraphicsCommandList* list,uint32_t kind){
    for(auto& source:sources)if(source.occupied && (source.state.flags&Captured) &&
        (source.state.Tracking() || source.retiredRecording || source.state.list!=Address(list)))
        UnknownSource(source,list,kind);
}
void Touch(Source& source,ID3D12GraphicsCommandList* list,bool write,bool alias,uint32_t kind){
    if(ForeignRecording(source,list))source.state.Foreign(Address(list),kind);
    else source.state.Touch(Address(list),write,alias,kind);
}
BoundTargets* Binding(ID3D12GraphicsCommandList* list,bool create){
    if(!list)return nullptr;
    BoundTargets* empty{};
    for(auto& value:bindings){if(value.list==list)return &value;if(!value.list && !empty)empty=&value;}
    if(!create)return nullptr;
    if(!empty){bindingOverflow=true;return nullptr;}
    empty->list=list;return empty;
}
void WriteLocked(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,uint32_t kind){
    if(!destination){UnknownLocked(list,kind);return;}
    for(auto& source:sources)if(source.occupied && (source.state.flags&Captured)){
        if(source.capturing){
            if(ForeignRecording(source,list))source.state.Foreign(Address(list),kind);
            continue;
        }
        if(!source.state.Tracking() && !source.retiredRecording && source.state.list==Address(list))continue;
        for(UINT i=0;i<source.targetCount;++i){auto& target=source.targets[i];if(!Affects(target,destination))continue;
            const bool aliases=target.identity.creation.resource.Get()!=destination;
            Touch(source,list,true,aliases,kind);
            if(ForeignRecording(source,list))continue;
            if(aliases)target.state.Reject(Failure::Aliasing);
            if(source.state.flags&Refused)Reject(source,source.state.failure);
        }
    }
}
void ShaderDrawLocked(ID3D12GraphicsCommandList* list){
    for(auto& source:sources)if(source.occupied){
        if(source.capturing){
            if(ForeignRecording(source,list))source.state.Foreign(Address(list),uint32_t(ExitKind::Other));
            continue;
        }
        for(UINT i=0;i<source.targetCount;++i)source.state.ShaderDraw(Address(list),source.targets[i].state,source.retiredRecording);
        if(source.state.flags&Refused)Reject(source,source.state.failure);
    }
}
}
bool Begin(ID3D12GraphicsCommandList* list,const native_recording::Lease& recording,uint32_t frame,uint32_t side,
           const scene_state::GeometryTargets& targets){
    uint64_t epoch{};bool admitted{};
    try{
    {
        std::lock_guard lock(mutex);if(window.finished)return false;Publication publication;Count(4);
        if(!list || !frame || side>=SourceLimit || targets.colors!=3 || !recording.token || recording.list!=Address(list)){
            Count(5);return false;
        }
        auto& source=sources[side];
        if(source.occupied){Reject(source,Failure::DuplicateBegin);Count(5);Publish();return false;}
        source.occupied=source.capturing=true;source.frame=frame;source.side=side;source.recording=recording;
        window.Begin();observing.store(true,std::memory_order_release);
        source.state.Begin(Address(list),recording.generation);epoch=captureEpoch;admitted=true;
        if(auto* binding=Binding(list,true)){
            binding->resources={};for(UINT slot=0;slot<3;++slot)binding->resources[slot]=targets.resources[slot];
            binding->resources[8]=targets.resources[4];binding->known=true;
        }
    }
    // External identity APIs acquire their own locks. No observer lock is held
    // during these read-only captures. Concurrent events on this recording
    // refuse the seed; foreign observations remain separate uncertainty.
    Source captured;captured.occupied=true;captured.frame=frame;captured.side=side;captured.recording=recording;
    captured.state.Begin(Address(list),recording.generation);
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))Reject(captured,Failure::Shape);
    for(const UINT slot:{0u,1u,2u,4u}){
        if(captured.state.flags&Refused)break;
        auto& target=captured.targets[captured.targetCount];
        if(!texture_read::CaptureWritableIdentity(list,targets.resources[slot].Get(),
            slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,target.identity)){
            Reject(captured,Failure::Identity);break;
        }
        if(!SameRecording(recording,target.identity.recording)){Reject(captured,Failure::Recording);break;}
        if(!TargetShape(device.Get(),target,slot,targets)){Reject(captured,Failure::Shape);break;}
        if(captured.targetCount && (target.identity.resourceDesc.Width!=captured.targets[0].identity.resourceDesc.Width ||
            target.identity.resourceDesc.Height!=captured.targets[0].identity.resourceDesc.Height)){Reject(captured,Failure::Shape);break;}
        target.stateFirst=side*StatesPerSource+captured.stateCount;
        captured.stateCount+=target.state.count;++captured.targetCount;
        for(UINT prior=0;prior+1<captured.targetCount;++prior)if(Affects(captured.targets[prior],target.identity.creation.resource.Get())){
            Reject(captured,Failure::Aliasing);break;
        }
    }
    std::lock_guard lock(mutex);if(epoch!=captureEpoch || !window.open)return false;Publication publication;
    if(sources[side].captureTouched)Reject(captured,Failure::CaptureRace);
    if(bindingOverflow)Reject(captured,Failure::UnknownCommand);
    if(sources[side].state.flags&Refused)Reject(captured,sources[side].state.failure);
    captured.state.foreignFailure=sources[side].state.foreignFailure;captured.state.foreignList=sources[side].state.foreignList;
    captured.state.foreignKind=sources[side].state.foreignKind;captured.state.foreignEvents=sources[side].state.foreignEvents;
    if(captured.targetCount!=TargetsPerSource || captured.stateCount!=StatesPerSource)Reject(captured,Failure::Shape);
    sources[side]=std::move(captured);const bool valid=!(sources[side].state.AggregateFlags()&Refused);
    if(!valid)Count(5);Publish();return valid;
}catch(...){
    std::lock_guard lock(mutex);if(!admitted || epoch!=captureEpoch || !window.open)return false;Publication publication;
    if(side<SourceLimit && sources[side].occupied)Reject(sources[side],Failure::Identity);
    Count(5);Publish();return false;
}
}
bool Observing()noexcept{return observing.load(std::memory_order_acquire);}
void MarkExit(ID3D12GraphicsCommandList* list,uint32_t reason,ExitKind kind){
    if(!observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!Event(list))return;Publication publication;
    for(auto& source:sources)if(source.occupied && source.state.list==Address(list) && source.state.Tracking() && !(source.state.flags&Exited)){
        source.state.Exit(reason,uint32_t(kind));Count(10);
    }
    Publish();
}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers){
    if(!count || !observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!Event(list))return;Publication publication;Count(6);
    if(!list || !barriers){UnknownLocked(list,uint32_t(ExitKind::Unknown));Publish();return;}
    for(UINT index=0;index<count;++index){const auto& barrier=barriers[index];
        const auto kind=barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION?ExitKind::Transition:
            barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING?ExitKind::Aliasing:
            barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_UAV?ExitKind::Uav:ExitKind::Unknown;
        if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING)Count(8);
        for(auto& source:sources)if(source.occupied && (source.state.flags&Captured)){
            if(source.capturing){
                if(ForeignRecording(source,list))source.state.Foreign(Address(list),uint32_t(kind));
                continue;
            }
            if(!source.state.Tracking() && !source.retiredRecording && source.state.list==Address(list))continue;
            for(UINT i=0;i<source.targetCount;++i){auto& target=source.targets[i];
                auto* resource=target.identity.creation.resource.Get();bool affects{};
                if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION){
                    if(!barrier.Transition.pResource){UnknownSource(source,list,uint32_t(ExitKind::Transition),Failure::UnknownBarrier);continue;}
                    affects=barrier.Transition.pResource==resource;
                    if(!affects){
                        if(Affects(target,barrier.Transition.pResource)){
                            Touch(source,list,false,true,uint32_t(ExitKind::Transition));
                            if(!ForeignRecording(source,list))Reject(source,source.state.failure);
                        }
                        continue;
                    }
                    Touch(source,list,false,false,uint32_t(ExitKind::Transition));
                    if(ForeignRecording(source,list))continue;
                    if(source.state.flags&Refused){Reject(source,source.state.failure);continue;}
                    if(target.state.Transition(barrier.Transition.Subresource,UINT(barrier.Transition.StateBefore),UINT(barrier.Transition.StateAfter),barrier.Flags))Count(7);
                    else Reject(source,target.state.failure);
                }else if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING){
                    affects=Affects(target,barrier.Aliasing.pResourceBefore,barrier.Aliasing.pResourceAfter);
                    if(affects){
                        Touch(source,list,false,true,uint32_t(ExitKind::Aliasing));
                        if(!ForeignRecording(source,list)){target.state.Alias(true);Reject(source,source.state.failure);}
                    }
                }else if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_UAV){
                    affects=!barrier.UAV.pResource || Affects(target,barrier.UAV.pResource);
                    if(affects){
                        Touch(source,list,true,false,uint32_t(ExitKind::Uav));
                        if(!ForeignRecording(source,list))Reject(source,Failure::UnknownBarrier);
                    }
                }else UnknownSource(source,list,uint32_t(ExitKind::Unknown),Failure::UnknownBarrier);
            }
        }
    }
    Publish();
}
void Written(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,uint32_t kind){
    if(!observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!Event(list))return;Publication publication;Count(9);WriteLocked(list,destination,kind);Publish();
}
void ClearView(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE handle,bool depth){
    if(!Observing())return;
    uint64_t epoch{};
    {std::lock_guard lock(mutex);if(!window.open)return;epoch=captureEpoch;}
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,9> resources;
    const bool known=depth?scene_state::CaptureBoundResources(0,nullptr,FALSE,&handle,resources):
        scene_state::CaptureBoundResources(1,&handle,FALSE,nullptr,resources);
    std::lock_guard lock(mutex);if(epoch!=captureEpoch || !Event(list))return;Publication publication;Count(9);
    if(!known){UnknownLocked(list,uint32_t(ExitKind::Clear));Publish();return;}
    bool any{};
    for(const auto& resource:resources)if(resource){any=true;WriteLocked(list,resource.Get(),uint32_t(ExitKind::Clear));}
    if(!any)UnknownLocked(list,uint32_t(ExitKind::Clear));
    Publish();
}
void DrawTargets(ID3D12GraphicsCommandList* list,const scene_state::GeometryTargets& targets){
    if(!observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!Event(list))return;Publication publication;Count(9);
    ShaderDrawLocked(list);
    if(targets.colors>4){UnknownLocked(list,uint32_t(ExitKind::Other));Publish();return;}
    bool any{};
    for(UINT slot=0;slot<5;++slot)if(slot<targets.colors || (slot==4 && targets.resources[4])){
        any=true;WriteLocked(list,targets.resources[slot].Get(),uint32_t(ExitKind::Other));
    }
    if(!any)UnknownLocked(list,uint32_t(ExitKind::Other));
    Publish();
}
void Targets(ID3D12GraphicsCommandList* list,UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* handles,BOOL contiguous,
             const D3D12_CPU_DESCRIPTOR_HANDLE* depth){
    if(!observing.load(std::memory_order_acquire))return;
    uint64_t epoch{};
    {
        std::lock_guard lock(mutex);
        if(!window.open || std::none_of(sources.begin(),sources.end(),[](const auto& source){return source.occupied;}))return;
        epoch=captureEpoch;
    }
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,9> resources;
    const bool known=scene_state::CaptureBoundResources(count,handles,contiguous,depth,resources);
    std::lock_guard lock(mutex);if(epoch!=captureEpoch || !Event(list))return;Publication publication;
    if(auto* binding=Binding(list,true)){binding->resources=std::move(resources);binding->known=known;}
    else UnknownLocked(list,uint32_t(ExitKind::Other));
    Publish();
}
void Draw(ID3D12GraphicsCommandList* list){
    if(!observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!Event(list))return;Publication publication;Count(9);
    ShaderDrawLocked(list);
    const auto* binding=Binding(list,false);
    if(!binding || !binding->known || bindingOverflow)UnknownLocked(list,uint32_t(ExitKind::Other));
    else for(const auto& resource:binding->resources)if(resource)WriteLocked(list,resource.Get(),uint32_t(ExitKind::Other));
    Publish();
}
void Unknown(ID3D12GraphicsCommandList* list,uint32_t kind){
    if(!observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!Event(list))return;Publication publication;UnknownLocked(list,kind);Publish();
}
void Close(ID3D12GraphicsCommandList* list,HRESULT result){
    if(!observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!Event(list))return;Publication publication;
    for(auto& source:sources)if(source.occupied && source.state.list==Address(list) && source.state.Tracking()){
        Count(SUCCEEDED(result)?11:12);
        if(source.state.Close(result))for(UINT i=0;i<source.targetCount;++i)source.targets[i].state.Freeze();
        else if(source.state.flags&Refused)Reject(source,source.state.failure);
    }
    Publish();
}
void Reset(ID3D12GraphicsCommandList* list){
    if(!observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!Event(list))return;Publication publication;
    if(auto* binding=Binding(list,false))*binding={};
    for(auto& source:sources)if(source.occupied && source.state.list==Address(list)){
        if(source.state.flags&SubmittedObserved)source.retiredRecording=true;
        else if(source.state.Tracking()){source.state.Reset();Reject(source,Failure::ResetBeforeSubmit);Count(13);source.retiredRecording=true;}
    }
    Publish();
}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!queue || !count || !lists || !observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!window.Event())return;Publication publication;
    for(auto& source:sources)if(source.capturing)for(UINT i=0;i<count;++i)if(Address(lists[i])==source.state.list)source.captureTouched=true;
    for(auto& source:sources)if(source.occupied && !source.capturing){
        UINT occurrences{};for(UINT i=0;i<count;++i)occurrences+=Address(lists[i])==source.state.list;
        if(!occurrences)continue;
        if(source.retiredRecording){source.state.Foreign(source.state.list,uint32_t(ExitKind::Other));Count(15);continue;}
        bool valid=native_recording::ValidateSubmission(source.recording,queue);
        for(UINT i=0;i<source.targetCount;++i){const auto& target=source.targets[i];
            valid=texture_read::ValidateWritableIdentity(target.identity,queue) && valid;
            valid=target.state.failure==Failure::None && valid;
            for(UINT subresource=0;subresource<target.state.count;++subresource)
                valid=(target.state.states[subresource].flags&FrozenKnown) && valid;
        }
        const bool localValid=source.state.Pending(Address(queue),occurrences,valid);
        if(!localValid || source.state.foreignEvents){if(!localValid)Reject(source,source.state.failure);Count(15);}
    }
    Publish();
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!queue || !count || !lists || !observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!window.Event())return;Publication publication;
    for(auto& source:sources)if(source.capturing)for(UINT i=0;i<count;++i)if(Address(lists[i])==source.state.list)source.captureTouched=true;
    for(auto& source:sources)if(source.occupied && !source.retiredRecording)for(UINT i=0;i<count;++i)if(Address(lists[i])==source.state.list){
        source.state.Submitted(Address(queue));if(source.state.flags&SubmittedObserved)source.retiredRecording=true;break;
    }
    Publish();
}
bool SnapshotSources(ID3D12CommandQueue* queue,uint32_t frame,std::array<SourceSnapshot,2>& output)try{
    output={};if(!queue || !frame || !Observing())return false;
    std::array<SourceSnapshot,2> next;uint64_t epoch{};
    {
        std::lock_guard lock(mutex);if(!window.open)return false;epoch=captureEpoch;
        constexpr uint64_t required=Captured|Exited|CloseSucceeded|PendingValidated;
        for(UINT side=0;side<SourceLimit;++side){const auto& source=sources[side];const auto& state=source.state;
            if(!source.occupied || source.capturing || source.retiredRecording || source.frame!=frame || source.side!=side ||
                state.failure!=Failure::None || (state.flags&required)!=required || (state.flags&(Refused|ResetBeforeSubmit|SubmittedObserved)) ||
                state.queue!=Address(queue) || source.targetCount!=TargetsPerSource || source.stateCount!=StatesPerSource ||
                source.recording.list!=state.list || source.recording.generation!=state.generation)return false;
            auto& saved=next[side];saved.recording=source.recording;saved.frame=frame;saved.side=side;
            for(UINT i=0;i<TargetsPerSource;++i){const auto& target=source.targets[i];auto& copy=saved.targets[i];
                if(target.state.failure!=Failure::None || target.planes!=target.state.count || !target.planes || target.planes>2)return false;
                copy.identity=target.identity;copy.view=target.view;copy.slot=target.slot;copy.planes=target.planes;
                if(!SameRecording(saved.recording,copy.identity.recording))return false;
                for(UINT plane=0;plane<target.planes;++plane){const auto& value=target.state.states[plane];
                    if((value.flags&(CurrentKnown|FrozenKnown|StateRefused))!=(CurrentKnown|FrozenKnown))return false;
                    copy.finalStates[plane]=D3D12_RESOURCE_STATES(value.frozen);
                }
            }
        }
    }
    for(const auto& source:next){
        if(!native_recording::ValidateSubmission(source.recording,queue))return false;
        for(const auto& target:source.targets)if(!texture_read::ValidateWritableIdentity(target.identity,queue))return false;
    }
    {
        std::lock_guard lock(mutex);if(epoch!=captureEpoch || !window.open)return false;
        for(UINT side=0;side<SourceLimit;++side)if(!SameRecording(sources[side].recording,next[side].recording) ||
            sources[side].state.failure!=Failure::None || sources[side].retiredRecording)return false;
    }
    output=std::move(next);return true;
}catch(...){output={};return false;}
void FinishObservation(){
    std::lock_guard lock(mutex);if(window.finished)return;Publication publication;
    window.Finish();++captureEpoch;observing.store(false,std::memory_order_release);
    bindings={};bindingOverflow=false;Publish();
}
void Clear(){
    std::lock_guard lock(mutex);Publication publication;sources={};bindings={};bindingOverflow=false;++captureEpoch;window.Clear();
    observing.store(false,std::memory_order_release);
    std::fill_n(CyberpunkVR_TargetHandoffStats,16,0);CyberpunkVR_TargetHandoffStats[0]=2;Publish();
}
}
#endif
