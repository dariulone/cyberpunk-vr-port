#include "Render/NativeSourceSrvBinding.hpp"
#include "Render/RenderProbeScope.hpp"
#include <Windows.h>
#include <intrin.h>
#include <map>
#include <mutex>
#include <tuple>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The observer: rust/crates/cvr-render/src/source_srv_observe.rs (the setters' candidates, CvrSourceSrvBefore and the
// thunks' globals, the extended state, the capture at the source). The API forwards.
namespace cvr::stereo::source_srv {
static_assert(sizeof(descriptors::SrvBinding)==384 && sizeof(SourceSrvBacking)==1112 && offsetof(SourceSrvBacking,recording)==384 &&
    offsetof(SourceSrvBacking,slot)==512 && offsetof(SourceSrvBacking,allocation)==768 && offsetof(SourceSrvBacking,allocator)==960 &&
    offsetof(SourceSrvBacking,gpu)==1024 && offsetof(SourceSrvBacking,stage)==1080,"mirrored by source_srv_observe.rs' Backing");
extern "C" {
bool cvr_ssrvo_compatible();
uint32_t cvr_ssrvo_read_at_source(uint64_t,uint64_t,uint32_t,uint32_t,const descriptors::SrvBinding*,const native_recording::Lease*,
                                  SourceSrvBacking*);
}
bool Compatible()noexcept{return cvr_ssrvo_compatible();}
}
#endif
#else

namespace cvr::stereo::descriptors {
bool MatchesCpuSourceSrv(uint64_t,const DescriptorStamp&);
}
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SourceSrvBindingEnable{0};
void* CvrSourceSrvIndividualOriginal{};
void* CvrSourceSrvCachedOriginal{};
uint64_t CvrSourceSrvXstateMask{};
uint32_t CvrSourceSrvXstateBytes{512},CvrSourceSrvXstateMode{},CvrSourceSrvXstateReady{};
}
namespace cvr::stereo::source_srv {
namespace {
std::mutex codeMutex,mutex;
uintptr_t verifiedBase{};bool codeAttempted{},codeVerified{};
std::atomic<bool> lost{false};std::atomic<uint64_t> epoch{1};
thread_local Status last{};
constexpr size_t Capacity=8192;
struct Candidate {
    CandidateInput input;camera_upload::Context context;native_recording::Lease recording;
    uint64_t registry{},serial{},epoch{};uint32_t thread{};
};
using Key=std::tuple<uint64_t,uint64_t,uint32_t>;
std::map<Key,Candidate> candidates;uint64_t nextSerial{};
uintptr_t Base()noexcept{return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
bool Read(void*,uint64_t address,void* output,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),output,bytes,&copied) && copied==bytes;
}
camera_upload::Context Current(){
    uint64_t thread{},context{},state{},list{};uint8_t initialized{};
    if(!Read(nullptr,__readgsqword(0x58),&thread,8) || !thread || thread>UINT64_MAX-0x1A0 ||
        !Read(nullptr,thread+0x14,&initialized,1) || !initialized || !Read(nullptr,thread+0x198,&context,8) ||
        !context || context>UINT64_MAX-0x68 || !Read(nullptr,context+0x60,&state,8) || !Read(nullptr,context+0x30,&list,8))return {};
    return {context,state,list};
}
bool Registry(uint64_t& registry){
    registry=0;const auto base=Base();return base && base<=UINT64_MAX-0x3BCDCB0 && Read(nullptr,base+0x3BCDCA8,&registry,8) && registry;
}
bool SameRecording(const native_recording::Lease& a,const native_recording::Lease& b){
    return a.token && a.token==b.token && a.list==b.list && a.context==b.context && a.registry==b.registry && a.generation==b.generation;
}
bool SameDevice(ID3D12GraphicsCommandList* list,ID3D12Resource* resource){
    Microsoft::WRL::ComPtr<ID3D12Device> a,b;
    return list && resource && SUCCEEDED(list->GetDevice(IID_PPV_ARGS(&a))) && SUCCEEDED(resource->GetDevice(IID_PPV_ARGS(&b))) && a==b;
}
bool SameSrv(const descriptors::SrvBinding& a,const descriptors::SrvBinding& b){
    const auto& x=a.view;const auto& y=b.view;
    return a.resource==b.resource && a.description==b.description && a.description==a.stamp.value &&
        a.stamp.gpu==b.stamp.gpu && a.stamp.heap==b.stamp.heap && a.stamp.value==b.stamp.value &&
        x.Format==y.Format && x.ViewDimension==y.ViewDimension && x.Shader4ComponentMapping==y.Shader4ComponentMapping &&
        x.Buffer.FirstElement==y.Buffer.FirstElement && x.Buffer.NumElements==y.Buffer.NumElements &&
        x.Buffer.StructureByteStride==y.Buffer.StructureByteStride && x.Buffer.Flags==y.Buffer.Flags;
}
bool SameAllocation(const source_cbv_details::Allocation& a,const source_cbv_details::Allocation& b){
    return a.observation==b.observation && a.address==b.address && a.vtable==b.vtable && a.pimpl==b.pimpl && a.bytes==b.bytes &&
        a.resource==b.resource && a.heap==b.heap && a.block==b.block && a.handle==b.handle && a.kind==b.kind &&
        a.associationKnown==b.associationKnown && a.resourceMatches==b.resourceMatches && a.extentCovers==b.extentCovers;
}
bool SameAllocator(const source_cbv_details::Allocator& a,const source_cbv_details::Allocator& b){
    return a.observation==b.observation && a.address==b.address && a.vtable==b.vtable && a.pimpl==b.pimpl &&
        a.associationKnown==b.associationKnown && a.pimplMatches==b.pimplMatches;
}
bool SameIdentity(const SourceSrvBacking& a,const SourceSrvBacking& b){
    return SameSrv(a.binding,b.binding) &&
        SameRecording(a.recording,b.recording) && source_cbv::detail::SameBuffer(a.native,b.native) && a.context==b.context &&
        a.slot==b.slot && a.rawRecord==b.rawRecord && a.recordAddress==b.recordAddress && a.cpuDescriptor==b.cpuDescriptor &&
        a.serial==b.serial && a.gpu==b.gpu && a.backingBytes==b.backingBytes && a.sourceOffset==b.sourceOffset && a.viewBytes==b.viewBytes &&
        a.route==b.route && a.setterRva==b.setterRva && a.stage==b.stage && a.shaderRegister==b.shaderRegister && a.lowType==b.lowType &&
        a.heapType==b.heapType && a.flags==b.flags && SameAllocation(a.allocation,b.allocation) && SameAllocator(a.allocator,b.allocator);
}
bool Mode(const Candidate& candidate,const Slot& slot,uint64_t cpu){
    if(!detail::MatchesMode(candidate.input,slot))return false;
    if(candidate.input.route==Route::Individual)return slot.cpu==cpu;
    std::array<uint8_t,8> entry{};
    return detail::CachedGroup(Read,nullptr,slot) && detail::Stable(Read,nullptr,candidate.input.bundleEntry,entry) && entry==candidate.input.bundleRaw;
}
void ObserveMa(SourceSrvBacking& output){
    using O=source_cbv_details::Observation;
    const auto direct=detail::Field<uint64_t>(output.rawRecord,0x18);
    const auto payload=detail::Field<uint64_t>(output.rawRecord,0x20),control=detail::Field<uint64_t>(output.rawRecord,0x28);
    if(direct && !payload && !control){
        output.allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,Base(),direct);
        if(output.allocation.observation==O::Known){
            output.allocation.associationKnown=true;
            output.allocation.resourceMatches=output.allocation.resource==reinterpret_cast<uintptr_t>(output.binding.resource.Get());
            output.allocation.extentCovers=output.allocation.bytes>=output.backingBytes;
        }
    }else output.allocation.observation=(!direct && !payload && !control)?O::Absent:O::Unsupported;
    output.allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,Base(),output.native.registry,output.allocation);
}
Status Capture(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,
    const descriptors::SrvBinding& held,const native_recording::Lease& recording,SourceSrvBacking& output){
    output={};if(!CyberpunkVR_SourceSrvBindingEnable.load(std::memory_order_relaxed))return Status::Ignored;
    if(!Compatible())return Status::UnsupportedExecutable;
    if(lost.load(std::memory_order_acquire))return Status::CoverageLost;
    if(!list || !root || parameter!=RootParameter || !TrackedRegister(offset) || !held.resource || !recording.token || !recording.generation ||
        held.view.ViewDimension!=D3D12_SRV_DIMENSION_BUFFER)
        return Status::InvalidBinding;
    const auto context=Current();
    if(!context || context.list!=reinterpret_cast<uintptr_t>(list) || context.context!=recording.context ||
        !native_recording::ValidateRecording(recording,list,context))return Status::MissingContext;
    Candidate candidate;
    {
        std::lock_guard lock(mutex);const auto at=candidates.find({context.list,recording.generation,offset});
        if(at==candidates.end())return Status::MissingObservation;candidate=at->second;
    }
    if(!candidate.serial || candidate.epoch!=epoch.load(std::memory_order_acquire) || candidate.thread!=GetCurrentThreadId() ||
        candidate.context!=context || !SameRecording(candidate.recording,recording))return Status::Changed;
    uint64_t registry{};if(!Registry(registry) || registry!=candidate.registry || registry!=recording.registry)return Status::Changed;
    const auto nativeContext=source_cbv_details::detail::ReadContext(Read,nullptr,context.context,context.list);
    if(nativeContext.observation!=source_cbv_details::Observation::Known || nativeContext.state!=context.state ||
        nativeContext.pending || nativeContext.submitted)return Status::MissingContext;
    UINT actualParameter{},actualOffset{};
    if(!descriptors::FindBinding(root,D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,offset,D3D12_SHADER_VISIBILITY_PIXEL,actualParameter,actualOffset) ||
        actualParameter!=parameter || actualOffset!=offset || !descriptors::MatchesDescriptor(held.stamp))return Status::DescriptorMismatch;
    descriptors::SrvBinding actualSrv;
    if(!descriptors::ResolveSrvAtDraw(list,root,parameter,offset,actualSrv) || !SameSrv(held,actualSrv))return Status::DescriptorMismatch;
    SourceSrvBacking next;next.binding=held;next.recording=recording;next.context=context;
    if(!detail::ReadSlot(Read,nullptr,context.state,next.slot,offset) || !detail::ReadRecord(Read,nullptr,registry,context.context,candidate.input.handle,
        next.rawRecord,next.recordAddress,next.native))return Status::InvalidBinding;
    next.cpuDescriptor=detail::Field<uint64_t>(next.rawRecord,0x48);
    if(next.native.resource!=reinterpret_cast<uintptr_t>(held.resource.Get()) || !next.cpuDescriptor ||
        !Mode(candidate,next.slot,next.cpuDescriptor) || !descriptors::MatchesCpuSourceSrv(next.cpuDescriptor,held.stamp))return Status::DescriptorMismatch;
    if(!SameDevice(list,held.resource.Get()))return Status::InvalidBinding;
    const auto desc=held.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS heapFlags{};
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || FAILED(held.resource->GetHeapProperties(&heap,&heapFlags)))return Status::InvalidBinding;
    next.gpu=held.resource->GetGPUVirtualAddress();next.backingBytes=desc.Width;next.heapType=UINT(heap.Type);next.flags=UINT(desc.Flags);
    if(heap.Type!=D3D12_HEAP_TYPE_DEFAULT || !detail::ViewRange(next.native,next.gpu,next.backingBytes,held.view,next.sourceOffset,next.viewBytes,offset))
        return Status::InvalidBinding;
    next.lowType=next.rawRecord[6]&15;next.stage=StageIndex;next.shaderRegister=offset;
    next.route=uint32_t(candidate.input.route);next.setterRva=candidate.input.route==Route::Individual?0x338FC0:0x338ED4;next.serial=candidate.serial;
    next.state=geometry::ReadNativeState(next.native,context.list,Read,nullptr);
    ObserveMa(next);
    Slot slotAgain;std::array<uint8_t,0xB0> rawAgain{};geometry::NativeBuffer nativeAgain;uint64_t recordAgain{},registryAgain{};
    if(Current()!=context || !Registry(registryAgain) || registryAgain!=registry ||
        !native_recording::ValidateRecording(recording,list,context) ||
        !detail::ReadSlot(Read,nullptr,context.state,slotAgain,offset) || slotAgain!=next.slot ||
        !detail::ReadRecord(Read,nullptr,registry,context.context,candidate.input.handle,rawAgain,recordAgain,nativeAgain) ||
        rawAgain!=next.rawRecord || recordAgain!=next.recordAddress || !source_cbv::detail::SameBuffer(nativeAgain,next.native) ||
        !Mode(candidate,slotAgain,next.cpuDescriptor) || !descriptors::MatchesCpuSourceSrv(next.cpuDescriptor,held.stamp) ||
        !descriptors::MatchesDescriptor(held.stamp))return Status::Changed;
    const auto contextAgain=source_cbv_details::detail::ReadContext(Read,nullptr,context.context,context.list);
    const auto stateAgain=geometry::ReadNativeState(next.native,context.list,Read,nullptr);
    descriptors::SrvBinding srvAgain;
    if(!descriptors::ResolveSrvAtDraw(list,root,parameter,offset,srvAgain) || !SameSrv(held,srvAgain))return Status::Changed;
    if(contextAgain!=nativeContext || stateAgain.value!=next.state.value || stateAgain.status!=next.state.status || stateAgain.kind!=next.state.kind)
        return Status::Changed;
    {
        std::lock_guard lock(mutex);const auto at=candidates.find({context.list,recording.generation,offset});
        if(lost.load(std::memory_order_acquire) || candidate.epoch!=epoch.load(std::memory_order_acquire))return Status::CoverageLost;
        if(at==candidates.end() || at->second.serial!=candidate.serial || at->second.input!=candidate.input)return Status::Changed;
    }
    output=std::move(next);return Status::Captured;
}
}
void PrepareCodeGuards(uintptr_t base)noexcept {
    try{
        std::lock_guard lock(codeMutex);if(codeAttempted)return;codeAttempted=true;verifiedBase=base;
        bool valid=base && base==Base();std::array<uint8_t,1024> bytes{};
        for(const auto& code:Contract){
            if(!valid || code.bytes>bytes.size() || base>UINT64_MAX-code.rva || !Read(nullptr,base+code.rva,bytes.data(),code.bytes)){valid=false;break;}
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=code.fnv){valid=false;break;}
        }
        codeVerified=valid;
    }catch(...){codeVerified=false;}
}
bool Compatible()noexcept {try{std::lock_guard lock(codeMutex);return codeVerified && verifiedBase==Base();}catch(...){return false;}}
bool ConfigureXstate()noexcept {
    static const bool configured=[] {
    CvrSourceSrvXstateReady=0;CvrSourceSrvXstateMask=0;CvrSourceSrvXstateBytes=512;CvrSourceSrvXstateMode=0;
    int cpu[4]{};__cpuid(cpu,0);const auto maximum=cpu[0];__cpuid(cpu,1);
    if(uint32_t(cpu[2])&(1u<<27)){
        if(!(uint32_t(cpu[2])&(1u<<26)) || maximum<0xD)return false;
        const auto enabled=_xgetbv(0);__cpuidex(cpu,0xD,0);
        const auto supported=uint64_t(uint32_t(cpu[0]))|(uint64_t(uint32_t(cpu[3]))<<32);
        if(!detail::ValidXstate(enabled,supported,uint32_t(cpu[1])))return false;
        CvrSourceSrvXstateMask=enabled;CvrSourceSrvXstateBytes=uint32_t(cpu[1]);CvrSourceSrvXstateMode=1;
    }
    CvrSourceSrvXstateReady=1;return true;
    }();return configured;
}
Status ReadAtSource(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,
    const descriptors::SrvBinding& held,const native_recording::Lease& recording,SourceSrvBacking& output)noexcept {
    try{return last=Capture(list,root,parameter,offset,held,recording,output);}catch(...){output={};return last=Status::Failure;}
}
Status RevalidateAtSource(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,
    const SourceSrvBacking& saved,SourceSrvBacking& current)noexcept {
    const auto status=ReadAtSource(list,root,parameter,offset,saved.binding,saved.recording,current);
    if(status!=Status::Captured)return status;
    if(!SameIdentity(saved,current)){current={};return last=Status::Changed;}return last=Status::Captured;
}
void Reset(ID3D12GraphicsCommandList* list)noexcept {
    try{std::lock_guard lock(mutex);for(auto at=candidates.begin();at!=candidates.end();)
        if(std::get<0>(at->first)==reinterpret_cast<uintptr_t>(list))at=candidates.erase(at);else ++at;}catch(...){lost.store(true,std::memory_order_release);}
}
void Clear()noexcept {
    try{std::lock_guard lock(mutex);candidates.clear();const auto old=epoch.load();
        if(old==UINT64_MAX || nextSerial==UINT64_MAX){lost.store(true,std::memory_order_release);return;}
        epoch.store(old+1,std::memory_order_release);lost.store(false,std::memory_order_release);
    }catch(...){lost.store(true,std::memory_order_release);}
}
uint32_t LastStatus()noexcept{return uint32_t(last);}
}
extern "C" void CvrSourceSrvBefore(uint32_t route,const cvr::stereo::source_srv::ThunkFrame* frame)noexcept {
    using namespace cvr::stereo::source_srv;
    try{
        last=Status::Ignored;if(!frame || route>uint32_t(Route::Cached) || !CyberpunkVR_SourceSrvBindingEnable.load(std::memory_order_relaxed) ||
            !detail::Relevant(Route(route),*frame) || cvr::stereo::probe::internalCommands)return;
        last=Status::UnsupportedExecutable;if(!Compatible())return;
        if(lost.load(std::memory_order_acquire)){last=Status::CoverageLost;return;}
        const auto observedEpoch=epoch.load(std::memory_order_acquire);const auto context=Current();last=Status::MissingContext;
        if(!context){lost.store(true,std::memory_order_release);return;}
        auto* list=reinterpret_cast<ID3D12GraphicsCommandList*>(context.list);cvr::stereo::native_recording::Lease recording;
        if(!cvr::stereo::native_recording::Capture(list,context,recording)){Reset(list);return;}
        uint64_t registry{};if(!Registry(registry) || registry!=recording.registry){Reset(list);last=Status::InvalidBinding;return;}
        const auto first=Route(route)==Route::Individual?uint32_t(frame->rdx):4u;
        const auto end=Route(route)==Route::Individual?first+1:7u;
        std::array<CandidateInput,3> inputs{};std::array<bool,3> valid{};
        for(UINT reg=first;reg<end;++reg)valid[reg-4]=detail::DecodeCandidate(Route(route),*frame,registry,Read,nullptr,inputs[reg-4],reg);
        std::lock_guard lock(mutex);
        if(observedEpoch!=epoch.load(std::memory_order_acquire) || !CyberpunkVR_SourceSrvBindingEnable.load(std::memory_order_relaxed))return;
        for(UINT reg=first;reg<end;++reg){const Key key{context.list,recording.generation,reg};
            if(!valid[reg-4]){candidates.erase(key);last=Status::InvalidBinding;continue;}
            if(nextSerial==UINT64_MAX || (!candidates.contains(key) && candidates.size()>=Capacity)){
                lost.store(true,std::memory_order_release);last=Status::Capacity;return;
            }
            Candidate candidate{inputs[reg-4],context,recording,registry,++nextSerial,observedEpoch,GetCurrentThreadId()};
            candidates.insert_or_assign(key,std::move(candidate));last=Status::Candidate;
        }
    }catch(...){lost.store(true,std::memory_order_release);last=Status::Failure;}
}
#endif
