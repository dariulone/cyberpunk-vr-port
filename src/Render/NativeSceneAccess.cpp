#include "Render/NativeSceneAccess.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/NativeBreadcrumbWrite.hpp"
#include "Render/NativeTargetAccess.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/StereoSceneState.hpp"
#include "Render/RenderProbeScope.hpp"
#include <algorithm>
#include <mutex>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The trace and its observer front are Rust's (rust/crates/cvr-render/src/scene_access_observer.rs and
// scene_access_native.rs: the lock, the epochs, the attribution, the writes); this half lends the scene child's
// description and the bound targets.
namespace cvr::stereo::scene_access_trace {
struct CvrSaAt {scene_child::Identity child;uint64_t token,generation,context,list,registry;bool matched,internal;};
static_assert(sizeof(CvrSaAt)==184,"mirrored by At in scene_access_observer.rs");
extern "C" {
void cvr_sao_attach(const void*);
void cvr_sao_unavailable();
void cvr_sao_reset(ID3D12GraphicsCommandList*);
void cvr_sao_close(ID3D12GraphicsCommandList*,HRESULT);
void cvr_sao_operand(ID3D12GraphicsCommandList*,ID3D12Resource*,bool,uint32_t,UINT,uint64_t,uint64_t);
void cvr_sao_immediate(ID3D12GraphicsCommandList2*,UINT,uint64_t,uint64_t,uint64_t);
void cvr_sao_barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void cvr_sao_targets(ID3D12GraphicsCommandList*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,BOOL,const D3D12_CPU_DESCRIPTOR_HANDLE*);
void cvr_sao_clear_view(ID3D12GraphicsCommandList*,D3D12_CPU_DESCRIPTOR_HANDLE,bool);
void cvr_sao_shader(ID3D12GraphicsCommandList*,bool);
void cvr_sao_unknown(ID3D12GraphicsCommandList*,uint32_t);
}
namespace {
struct Host {
    bool (*at)(ID3D12GraphicsCommandList*,CvrSaAt*);
    bool (*bound)(UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,BOOL,const D3D12_CPU_DESCRIPTOR_HANDLE*,uint64_t*);
};
const Host kHost{
    [](ID3D12GraphicsCommandList* list,CvrSaAt* out){
        try{native_recording::Lease lease;const bool matched=scene_child::DescribeCurrentAtList(list,out->child,lease);
            out->generation=lease.generation;out->context=lease.context;out->list=lease.list;out->registry=lease.registry;
            out->token=reinterpret_cast<uintptr_t>(lease.token.Detach());return matched;}catch(...){return false;}},
    [](UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* colors,BOOL contiguous,const D3D12_CPU_DESCRIPTOR_HANDLE* depth,uint64_t* out){
        try{std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,9> held;
            const bool known=scene_state::CaptureBoundResources(count,colors,contiguous,depth,held);
            for(size_t i=0;i<held.size();++i)out[i]=reinterpret_cast<uintptr_t>(held[i].Detach());return known;}
        catch(...){for(size_t i=0;i<9;++i)out[i]=0;return false;}},
};
[[maybe_unused]] const bool kAttached=(cvr_sao_attach(&kHost),true);
}  // namespace
void ObserverUnavailable()noexcept{cvr_sao_unavailable();}
void Reset(ID3D12GraphicsCommandList* list)noexcept{cvr_sao_reset(list);}
void Close(ID3D12GraphicsCommandList* list,HRESULT result)noexcept{cvr_sao_close(list,result);}
void Operand(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,bool write,uint32_t tag,UINT subresource,uint64_t offset,uint64_t bytes)noexcept{
    cvr_sao_operand(list,resource,write,tag,subresource,offset,bytes);}
void ImmediateWrite(ID3D12GraphicsCommandList2* list,UINT count,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER* parameters,
                    const D3D12_WRITEBUFFERIMMEDIATE_MODE* modes,uintptr_t caller)noexcept{
    cvr_sao_immediate(list,count,reinterpret_cast<uintptr_t>(parameters),reinterpret_cast<uintptr_t>(modes),caller);}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers)noexcept{cvr_sao_barriers(list,count,barriers);}
void Targets(ID3D12GraphicsCommandList* list,UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* colors,BOOL contiguous,
             const D3D12_CPU_DESCRIPTOR_HANDLE* depth)noexcept{cvr_sao_targets(list,count,colors,contiguous,depth);}
void ClearView(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE view,bool depth)noexcept{cvr_sao_clear_view(list,view,depth);}
void Shader(ID3D12GraphicsCommandList* list,bool graphics)noexcept{cvr_sao_shader(list,graphics);}
void Unknown(ID3D12GraphicsCommandList* list,uint32_t tag)noexcept{cvr_sao_unknown(list,tag);}
}  // namespace cvr::stereo::scene_access_trace
#endif
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SceneAccessEnable{},CyberpunkVR_SceneAccessSeq{};
__declspec(dllexport) uint64_t CyberpunkVR_SceneAccessStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_SceneAccessRecordings{},CyberpunkVR_SceneAccessResources{};
__declspec(dllexport) uint64_t CyberpunkVR_SceneAccessEvents{},CyberpunkVR_SceneAccessChildren{};
}
namespace cvr::stereo::scene_access_trace {
namespace {
using Microsoft::WRL::ComPtr;
struct Recording {
    ComPtr<ID3D12GraphicsCommandList> list;
    native_recording::Lease native;
    uint64_t generation{};bool whole{},closed{};HRESULT close=E_PENDING;
    uint32_t child=UINT32_MAX,failure{};
    std::vector<Event> events;
    std::array<uint32_t,9> om{};
    bool omKnown=true;
    Recording(){om.fill(NoResource);}
};
struct Resource {ComPtr<ID3D12Resource> resource;constants::ResourceCreationInfo creation;std::array<uint64_t,16> row{};};
std::mutex mutex;
std::atomic<bool> active{},unavailable{};
std::vector<Recording> recordings;
std::vector<Resource> resources;
std::vector<scene_child::Identity> children;
std::vector<std::array<uint64_t,16>> recordingRows,resourceRows;
std::vector<Event> eventRows;
uint64_t epoch{},generation{},sequence{},eventCount{},shaderUnknown{},unattributed{},missingMetadata{},resets{},overflow{};
uint32_t failure{};bool finished{};
struct Write {
    Write(){CyberpunkVR_SceneAccessSeq.fetch_add(1,std::memory_order_acq_rel);}
    ~Write(){
        const auto selected=std::count_if(recordings.begin(),recordings.end(),[](const auto& value){return bool(value.native.token);});
        const uint64_t stats[]{1,active.load(),finished,epoch,recordings.size(),resources.size(),eventCount,children.size(),uint64_t(selected),
            shaderUnknown,unattributed,missingMetadata,resets,overflow,failure,0};
        std::copy_n(stats,16,CyberpunkVR_SceneAccessStats);
        CyberpunkVR_SceneAccessSeq.fetch_add(1,std::memory_order_release);
    }
};
void Fail(uint32_t reason){if(!failure)failure=reason;}
Recording* Find(ID3D12GraphicsCommandList* list){
    Recording* result{};
    for(auto& value:recordings)if(value.list.Get()==list && (!result || value.generation>result->generation))result=&value;
    return result;
}
Recording* New(ID3D12GraphicsCommandList* list,bool whole){
    if(!list || generation==UINT64_MAX){Fail(1);return nullptr;}
    Recording next;next.list=list;next.whole=whole;next.omKnown=whole;next.generation=++generation;
    if(auto* old=Find(list);old && !old->native.token){eventCount-=old->events.size();*old=std::move(next);return old;}
    if(recordings.size()>=RecordingLimit){++overflow;Fail(2);return nullptr;}
    recordings.push_back(std::move(next));return &recordings.back();
}
Recording* Get(ID3D12GraphicsCommandList* list){auto* result=Find(list);return result?result:New(list,false);}
uint32_t Hold(ID3D12Resource* resource){
    if(!resource)return NoResource;
    for(uint32_t i=0;i<resources.size();++i)if(resources[i].resource.Get()==resource)return i;
    if(resources.size()>=ResourceLimit){++overflow;Fail(2);return NoResource;}
    Resource next;next.resource=resource;
    const auto desc=resource->GetDesc();uint64_t cookie{};
    const bool identity=descriptors::GetHeldResourceCookie(next.resource,cookie);
    const bool placed=constants::QueryResourceCreation(next.resource,next.creation) || texture_read::QueryObservedAllocation(next.resource,next.creation);
    next.row={uint64_t(resource),cookie,next.creation.kind,uint64_t(next.creation.heap.Get()),next.creation.heapOffset,
        next.creation.allocationBytes,uint64_t(desc.Dimension),desc.Width,desc.Height,desc.DepthOrArraySize,desc.MipLevels,
        uint64_t(desc.Format),uint64_t(desc.Flags),identity,placed,0};
    if(!identity || !placed)++missingMetadata;
    resources.push_back(std::move(next));return uint32_t(resources.size()-1);
}
uint32_t HoldBreadcrumb(const breadcrumb_write::Snapshot& snapshot){
    const auto index=Hold(snapshot.creation.resource.Get());if(index==NoResource)return index;
    auto& cached=resources[index];const auto& creation=snapshot.creation;
    const target_access::detail::Resource observed{uint64_t(creation.resource.Get()),
        {uint64_t(creation.heap.Get()),creation.heapOffset,creation.allocationBytes,creation.kind},uint32_t(snapshot.resourceDesc.Flags)};
    if(!texture_read::detail::KnownAllocation(observed.allocation))return index;
    const target_access::detail::Resource prior{uint64_t(cached.resource.Get()),
        {uint64_t(cached.creation.heap.Get()),cached.creation.heapOffset,cached.creation.allocationBytes,cached.creation.kind},UINT(cached.row[12])};
    const auto match=target_access::detail::CompareAuthenticatedCreation(prior,observed);
    if(match==target_access::detail::CreationMatch::Conflict)return NoResource;
    if(match==target_access::detail::CreationMatch::Upgrade || !cached.row[14]){
        const bool missing=!cached.row[13] || !cached.row[14];cached.creation=creation;
        cached.row[2]=creation.kind;cached.row[3]=uint64_t(creation.heap.Get());cached.row[4]=creation.heapOffset;
        cached.row[5]=creation.allocationBytes;cached.row[14]=1;
        if(missing && cached.row[13] && missingMetadata)--missingMetadata;
    }
    return index;
}
struct Attribution {scene_child::Identity child;native_recording::Lease native;bool matched{};};
Attribution At(ID3D12GraphicsCommandList* list){
    Attribution out;
    if(!probe::internalCommands)out.matched=scene_child::DescribeCurrentAtList(list,out.child,out.native);
    return out;
}
bool SameParent(const scene_child::Identity& a,const scene_child::Identity& b){
    return a.consumerId==b.consumerId && a.epoch==b.epoch && a.pairGeneration==b.pairGeneration &&
        a.frameOwner==b.frameOwner && a.sourceGraph==b.sourceGraph && a.executionGraph==b.executionGraph &&
        a.rootObject==b.rootObject && a.workContext==b.workContext && a.rootHandle==b.rootHandle &&
        a.view==b.view && a.section==b.section;
}
uint32_t Child(Recording& record,const Attribution& at){
    if(!at.matched)return UINT32_MAX;
    if(!at.native.token || !at.native.generation || !at.native.context || !at.native.registry ||
        at.native.list!=uint64_t(record.list.Get()) ||
        (record.native.token && (record.native.token!=at.native.token || record.native.generation!=at.native.generation ||
            record.native.context!=at.native.context || record.native.registry!=at.native.registry)) ||
        (record.child!=UINT32_MAX && (record.child>=children.size() || !SameParent(children[record.child],at.child)))){
        record.failure=3;Fail(3);return UINT32_MAX;
    }
    record.native=at.native;
    for(uint32_t i=0;i<children.size();++i)if(children[i]==at.child){if(record.child==UINT32_MAX)record.child=i;return i;}
    if(children.size()>=ChildLimit){++overflow;Fail(2);return UINT32_MAX;}
    const auto index=uint32_t(children.size());children.push_back(at.child);if(record.child==UINT32_MAX)record.child=index;return index;
}
void Append(Recording& record,const Attribution& at,Event event){
    if(record.closed){record.failure=4;Fail(4);return;}
    if(eventCount>=EventLimit || sequence==UINT64_MAX){++overflow;Fail(2);return;}
    event.sequence=++sequence;event.recording=uint32_t(&record-recordings.data());event.child=Child(record,at);
    event.internal=probe::internalCommands!=0;
    if(event.child==UINT32_MAX && !event.internal)++unattributed;
    record.events.push_back(event);++eventCount;
}
uint64_t CaptureEpoch(){
    std::lock_guard lock(mutex);return active.load()?epoch:0;
}
template<class F>void Observe(ID3D12GraphicsCommandList* list,F&& function,uint64_t expectedEpoch=0)noexcept{
    if(!active.load(std::memory_order_acquire))return;
    uint64_t capturedEpoch=expectedEpoch;
    try{
        if(!capturedEpoch)capturedEpoch=CaptureEpoch();if(!capturedEpoch)return;
        const auto attribution=At(list);
        std::lock_guard lock(mutex);if(!active.load() || epoch!=capturedEpoch)return;Write write;
        if(auto* record=Get(list))function(*record,attribution);
    }catch(...){std::lock_guard lock(mutex);if(!active.load() || epoch!=capturedEpoch)return;Write write;Fail(5);}
}
}
bool Active()noexcept{return active.load(std::memory_order_acquire);}
void Arm()noexcept try{
    std::lock_guard lock(mutex);Write write;active=false;
    if(epoch==UINT64_MAX){++overflow;Fail(2);return;}
    recordings.clear();resources.clear();children.clear();recordingRows.clear();resourceRows.clear();eventRows.clear();
    CyberpunkVR_SceneAccessRecordings=CyberpunkVR_SceneAccessResources=CyberpunkVR_SceneAccessEvents=CyberpunkVR_SceneAccessChildren=0;
    ++epoch;generation=sequence=eventCount=shaderUnknown=unattributed=missingMetadata=resets=overflow=0;failure=0;finished=false;
    if(unavailable.load()){Fail(6);return;}active=true;
}catch(...){active=false;}
void Finish(bool aborted)noexcept try{
    std::lock_guard lock(mutex);if(finished)return;Write write;active=false;finished=true;
    if(aborted)Fail(8);
    const auto selected=std::count_if(recordings.begin(),recordings.end(),[](const auto& record){return bool(record.native.token);});
    if(selected!=2)Fail(9);
    for(const auto& record:recordings)if(record.native.token && (!record.closed || FAILED(record.close)))Fail(9);
    recordingRows.clear();resourceRows.clear();eventRows.clear();
    for(const auto& resource:resources)resourceRows.push_back(resource.row);
    for(const auto& record:recordings){
        const auto first=eventRows.size();eventRows.insert(eventRows.end(),record.events.begin(),record.events.end());
        const scene_child::Identity child=record.child<children.size()?children[record.child]:scene_child::Identity{};
        recordingRows.push_back({uint64_t(record.list.Get()),record.generation,record.whole,record.closed,uint32_t(record.close),
            record.native.generation,uint64_t(record.native.token.Get()),record.native.context,record.native.registry,
            child.rootHandle,child.view,child.consumerId,child.epoch,child.pairGeneration,first,record.events.size()});
    }
    CyberpunkVR_SceneAccessRecordings=uint64_t(recordingRows.data());CyberpunkVR_SceneAccessResources=uint64_t(resourceRows.data());
    CyberpunkVR_SceneAccessEvents=uint64_t(eventRows.data());CyberpunkVR_SceneAccessChildren=uint64_t(children.data());
}catch(...){active=false;std::lock_guard lock(mutex);Write write;Fail(5);}
void ObserverUnavailable()noexcept{unavailable=true;if(Active()){std::lock_guard lock(mutex);Write write;Fail(6);}}
void Reset(ID3D12GraphicsCommandList* list)noexcept{
    if(!Active())return;
    try{std::lock_guard lock(mutex);if(!active.load())return;Write write;++resets;New(list,true);}catch(...){ObserverUnavailable();}
}
void Close(ID3D12GraphicsCommandList* list,HRESULT result)noexcept{
    Observe(list,[&](Recording& record,const Attribution&){record.closed=true;record.close=result;if(FAILED(result)){record.failure=7;Fail(7);}});
}
void Operand(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,bool write,uint32_t tag,UINT subresource,uint64_t offset,uint64_t bytes)noexcept{
    Observe(list,[&](Recording& record,const Attribution& at){Event event;event.kind=uint32_t(write?Kind::Write:Kind::Read);
        event.resource=Hold(resource);event.offset=offset;event.bytes=bytes;event.subresource=subresource;event.tag=tag;
        if(event.resource==NoResource)event.kind=uint32_t(Kind::Unknown);Append(record,at,event);});
}
void ImmediateWrite(ID3D12GraphicsCommandList2* list,UINT count,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER* parameters,
                    const D3D12_WRITEBUFFERIMMEDIATE_MODE* modes,uintptr_t caller)noexcept{
    if(!count || !Active())return;
    uint64_t capturedEpoch{};try{capturedEpoch=CaptureEpoch();}catch(...){ObserverUnavailable();return;}if(!capturedEpoch)return;
    breadcrumb_write::Lease lease;breadcrumb_write::Snapshot snapshot;uint64_t offset{};bool resolved{};
    try{
        if(breadcrumb_write::Capture(list,count,parameters,modes,caller,lease) && breadcrumb_write::Describe(lease,snapshot) &&
            snapshot.creation.resource && snapshot.resourceDesc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER){
            const auto& resource=snapshot.creation.resource;
            const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
            if(count==1 && desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && desc.Width==snapshot.resourceDesc.Width &&
                desc.Width>=4 && gpu && gpu<=UINT64_MAX-desc.Width && snapshot.destination>=gpu &&
                !(snapshot.destination&3) && snapshot.destination-gpu<=desc.Width-4){
                offset=snapshot.destination-gpu;resolved=true;
            }
        }
    }catch(...){resolved=false;}
    Observe(list,[&](Recording& record,const Attribution& at){Event event;event.kind=uint32_t(Kind::Unknown);event.tag=13;
        if(resolved){event.resource=HoldBreadcrumb(snapshot);
            if(event.resource!=NoResource){event.kind=uint32_t(Kind::Write);event.offset=offset;event.bytes=4;event.subresource=0;}}
        Append(record,at,event);
    },capturedEpoch);
}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers)noexcept{
    Observe(list,[&](Recording& record,const Attribution& at){
        if(count && !barriers){Event event;event.kind=uint32_t(Kind::Unknown);event.tag=5;Append(record,at,event);return;}
        for(UINT i=0;i<count;++i){const auto& b=barriers[i];Event event;event.flags=uint32_t(b.Flags);
            if(b.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION){event.kind=uint32_t(Kind::Transition);event.resource=Hold(b.Transition.pResource);
                event.subresource=b.Transition.Subresource;event.before=uint32_t(b.Transition.StateBefore);event.after=uint32_t(b.Transition.StateAfter);}
            else if(b.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING){event.kind=uint32_t(Kind::Alias);event.resource=Hold(b.Aliasing.pResourceBefore);event.other=Hold(b.Aliasing.pResourceAfter);}
            else if(b.Type==D3D12_RESOURCE_BARRIER_TYPE_UAV){event.kind=uint32_t(Kind::UavBarrier);event.resource=Hold(b.UAV.pResource);}
            else event.kind=uint32_t(Kind::Unknown);
            Append(record,at,event);
        }
    });
}
void Targets(ID3D12GraphicsCommandList* list,UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* colors,BOOL contiguous,const D3D12_CPU_DESCRIPTOR_HANDLE* depth)noexcept{
    if(!Active())return;
    uint64_t capturedEpoch{};try{capturedEpoch=CaptureEpoch();}catch(...){ObserverUnavailable();return;}if(!capturedEpoch)return;
    std::array<ComPtr<ID3D12Resource>,9> held;
    const bool known=scene_state::CaptureBoundResources(count,colors,contiguous,depth,held);
    Observe(list,[&](Recording& record,const Attribution& at){record.om.fill(NoResource);record.omKnown=known;
        if(!known){Event event;event.kind=uint32_t(Kind::Unknown);event.tag=8;Append(record,at,event);return;}
        for(uint32_t i=0;i<held.size();++i){record.om[i]=Hold(held[i].Get());if(record.om[i]==NoResource)continue;
            Event event;event.kind=uint32_t(Kind::Targets);event.resource=record.om[i];event.tag=i;Append(record,at,event);}
    },capturedEpoch);
}
void ClearView(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE view,bool depth)noexcept{
    if(!Active())return;
    uint64_t capturedEpoch{};try{capturedEpoch=CaptureEpoch();}catch(...){ObserverUnavailable();return;}if(!capturedEpoch)return;
    std::array<ComPtr<ID3D12Resource>,9> held;
    const bool known=scene_state::CaptureBoundResources(depth?0:1,depth?nullptr:&view,FALSE,depth?&view:nullptr,held);
    Observe(list,[&](Recording& record,const Attribution& at){
        if(!known){Event event;event.kind=uint32_t(Kind::Unknown);event.tag=6;Append(record,at,event);return;}
        for(const auto& resource:held)if(resource){Event event;event.kind=uint32_t(Kind::Write);event.resource=Hold(resource.Get());event.tag=6;
            if(event.resource==NoResource)event.kind=uint32_t(Kind::Unknown);Append(record,at,event);}
    },capturedEpoch);
}
void Shader(ID3D12GraphicsCommandList* list,bool graphics)noexcept{
    Observe(list,[&](Recording& record,const Attribution& at){Event marker;marker.kind=uint32_t(Kind::Shader);marker.flags=graphics;
        Append(record,at,marker);++shaderUnknown;
        if(graphics){if(!record.omKnown){Event event;event.kind=uint32_t(Kind::Unknown);event.tag=8;Append(record,at,event);}
            for(const auto resource:record.om)if(resource!=NoResource){Event event;event.kind=uint32_t(Kind::Write);event.resource=resource;event.tag=1;Append(record,at,event);}}
    });
}
void Unknown(ID3D12GraphicsCommandList* list,uint32_t tag)noexcept{
    Observe(list,[&](Recording& record,const Attribution& at){Event event;event.kind=uint32_t(Kind::Unknown);event.tag=tag;Append(record,at,event);});
}
}
#endif
