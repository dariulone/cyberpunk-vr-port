#include "Render/NativeSourceCbvBinding.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <intrin.h>
#include <atomic>
#include <map>
#include <mutex>
#include <tuple>

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The observer: rust/crates/cvr-render/src/source_cbv_observe.rs (the observations, the CBV origins, the upload
// writer's scopes and the read at the source; CyberpunkVR_SourceCbvBindingEnable is defined there). The API forwards.
namespace cvr::stereo::source_cbv {
namespace {
struct RustOrigin {uint64_t cookie,id;IUnknown* owner;};
struct RustAnnotation {uint64_t cpu;D3D12_CONSTANT_BUFFER_VIEW_DESC view;RustOrigin origin;};
static_assert(sizeof(RustAnnotation)==48 && sizeof(Annotation)==48 && offsetof(Annotation,origin)==24 &&
    sizeof(descriptors::CbvOrigin)==24,"mirrored by source_cbv_observe.rs' Annotation and RawOrigin");
static_assert(sizeof(SourceCbvBacking)==472 && offsetof(SourceCbvBacking,recording)==192 && offsetof(SourceCbvBacking,native)==232 &&
    offsetof(SourceCbvBacking,descriptorBytes)==336 && offsetof(SourceCbvBacking,creationRecording)==368 &&
    offsetof(SourceCbvBacking,writerBytes)==424 && offsetof(SourceCbvBacking,writerRecording)==432,
    "mirrored by source_cbv_observe.rs' Backing");
}
extern "C" {
uint32_t cvr_scbo_bind(uint32_t,uint32_t,uint32_t,uint32_t,RustAnnotation*);
void cvr_scbo_last_diagnostic(std::array<uint64_t,16>*);
void cvr_scbo_writer_enter(uint32_t,uint32_t,detail::UploadWriterStamp*);
void cvr_scbo_writer_exit(const detail::UploadWriterStamp*);
uint32_t cvr_scbo_read_at_source(uint64_t,uint64_t,uint32_t,uint32_t,const descriptors::DescriptorStamp*,IUnknown*,SourceCbvBacking*);
}
namespace {
descriptors::CbvOrigin Adopt(const RustOrigin& raw){
    descriptors::CbvOrigin result;result.cookie=raw.cookie;result.id=raw.id;result.owner.Attach(raw.owner);return result;
}
std::vector<Annotation> Annotations(uint32_t which,uint32_t reg,uint32_t handle,uint32_t stages){
    RustAnnotation raw[2]{};const auto count=cvr_scbo_bind(which,reg,handle,stages,raw);
    std::vector<Annotation> result;
    for(uint32_t i=0;i<count;++i){Annotation value;value.cpu=raw[i].cpu;value.view=raw[i].view;value.origin=Adopt(raw[i].origin);
        result.push_back(std::move(value));}
    return result;
}
}
UploadWriterScope::UploadWriterScope(uint32_t handle,uint32_t bytes)noexcept{cvr_scbo_writer_enter(handle,bytes,&stamp_);}
UploadWriterScope::~UploadWriterScope(){cvr_scbo_writer_exit(&stamp_);}
std::vector<Annotation> ObserveBindMask(uint32_t reg,uint32_t handle,uint32_t stages){return Annotations(0,reg,handle,stages);}
std::vector<Annotation> ObserveBindStage(uint32_t reg,uint32_t handle,uint32_t stage){return Annotations(1,reg,handle,stage);}
}
#endif
#else
extern "C" {__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SourceCbvBindingEnable{};}
namespace cvr::stereo::source_cbv {
using Microsoft::WRL::ComPtr;
namespace {
constexpr GUID originType{0x178a32ad,0x2113,0x4264,{0x9e,0xc1,0xb9,0x34,0x66,0x80,0x21,0x0f}};
std::mutex mutex,codeMutex;
uint64_t verifiedBase{};bool codeAttempted{},codeVerified{};
std::atomic<uint64_t> nextId{};
thread_local Status last{};
thread_local Diagnostic diagnostic;
void At(Phase phase){diagnostic.values[0]=uint32_t(phase);}
bool Read(void*,uint64_t address,void* value,size_t bytes){SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),value,bytes,&copied) && copied==bytes;}
bool Active(){return CyberpunkVR_SourceCbvBindingEnable.load(std::memory_order_relaxed) && !probe::internalCommands && native_probe::ObserveBindings();}
uint64_t Base(){return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
camera_upload::Context Current(){
    uint64_t thread{},context{},state{},list{};uint8_t initialized{};
    if(!Read(nullptr,__readgsqword(0x58),&thread,8) || !thread || thread>UINT64_MAX-0x1A0 ||
        !Read(nullptr,thread+0x14,&initialized,1) || !initialized || !Read(nullptr,thread+0x198,&context,8) ||
        !context || context>UINT64_MAX-0x68 || !Read(nullptr,context+0x60,&state,8) || !Read(nullptr,context+0x30,&list,8))return {};
    return {context,state,list};
}
bool Device(ID3D12Device* expected,ID3D12DeviceChild* object){ComPtr<ID3D12Device> actual;
    return object && SUCCEEDED(object->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==expected;}
enum class WriterStage:uint32_t {Open,Completed,Cancelled};
struct WriterReceipt final:IUnknown {
    std::atomic<ULONG> refs{1};std::atomic<WriterStage> stage{WriterStage::Open};
    native_recording::Lease recording;detail::UploadWriterJoin joined;detail::UploadWriterCompletion completion;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
thread_local detail::UploadWriterLedger uploadWriters;
thread_local std::array<ComPtr<WriterReceipt>,detail::MaxUploadWriterDepth> writerOwners;
detail::UploadWriterContext WriterContext(const native_recording::Lease& recording){
    return {GetCurrentThreadId(),reinterpret_cast<uintptr_t>(recording.token.Get()),recording.list,
        recording.generation,recording.context,recording.registry};
}
struct Origin final:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Resource> resource;
    uint64_t id{},cookie{},gpu{},bytes{},offset{},cpu{};uint32_t viewBytes{},caller{};
    uint32_t logicalBytes{},reservedBytes{},completedProducerRva{};
    native_recording::Lease creationRecording;
    ComPtr<WriterReceipt> writer;
    geometry::NativeBuffer native;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown) && id!=originType)return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
ComPtr<Origin> GetOrigin(IUnknown* value){ComPtr<Origin> result;
    if(value)value->QueryInterface(originType,reinterpret_cast<void**>(result.GetAddressOf()));return result;}
descriptors::CbvOrigin MakeOrigin(ID3D12Resource* resource,const D3D12_CONSTANT_BUFFER_VIEW_DESC& view,uint64_t cpu,uint32_t caller,
    const geometry::NativeBuffer& native={},const detail::Creation* creation=nullptr,const native_recording::Lease* recording=nullptr){
    descriptors::CbvOrigin result;if(!resource)return result;
    const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !detail::Descriptor(gpu,desc.Width,view.BufferLocation,view.SizeInBytes))return result;
    if(caller==detail::DirectCreatorRva && (!creation || !detail::DirectProducer(*creation,view.SizeInBytes) ||
        creation->resource!=reinterpret_cast<uintptr_t>(resource) || creation->offset!=view.BufferLocation-gpu || creation->cpu!=cpu))return result;
    const auto cookie=descriptors::SourceResourceCookieAtCreation(resource);if(!cookie)return result;
    auto id=nextId.load(std::memory_order_relaxed);
    do{if(id==UINT64_MAX)return result;}while(!nextId.compare_exchange_weak(id,id+1,std::memory_order_relaxed));
    ComPtr<Origin> owner;owner.Attach(new Origin);owner->resource=resource;owner->id=id+1;owner->cookie=cookie;
    owner->gpu=gpu;owner->bytes=desc.Width;owner->offset=view.BufferLocation-gpu;owner->viewBytes=view.SizeInBytes;owner->cpu=cpu;owner->caller=caller;
    owner->native=native;
    if(creation && caller==detail::DirectCreatorRva){
        owner->logicalBytes=creation->logicalBytes;owner->reservedBytes=creation->reservedBytes;
        owner->completedProducerRva=creation->completedProducerRva;
    }
    if(recording)owner->creationRecording=*recording;
    return {cookie,id+1,owner};
}
struct Observation {
    camera_upload::Context context;native_recording::Lease recording;Slot slot;
    geometry::NativeBuffer native;ComPtr<ID3D12Resource> resource;
    uint64_t serial{},address{};uint32_t viewBytes{},logicalBytes{},kind{},stage{},reg{};
};
using Key=std::tuple<uint64_t,uint64_t,uint32_t,uint32_t>;
std::map<Key,Observation> observations;
uint64_t nextSerial{};constexpr size_t Capacity=8192;
struct AddressHint {ComPtr<ID3D12Resource> resource;uint64_t gpu{};camera_upload::Context context;};
thread_local AddressHint addressHint;
bool CpuDescriptor(camera_upload::Context context,uint32_t handle,uint32_t kind,uint64_t record,uint64_t& descriptor){
    descriptor=0;if(kind!=4)return Read(nullptr,record+0x48,&descriptor,8) && descriptor;
    if(context.context>UINT64_MAX-0x518)return false;
    struct Map {uint64_t buckets;uint32_t count,bucketCount;uint64_t data;uint32_t capacity,stride;};Map map{};
    if(!Read(nullptr,context.context+0x4F8,&map,sizeof(map)) || !map.count || map.count>map.capacity || map.capacity>1048576 ||
        !map.bucketCount || map.bucketCount>1048576 || map.stride!=64 || !map.buckets || !map.data)return false;
    const uint64_t offset=uint64_t(handle%map.bucketCount)*4;uint32_t at{};
    if(map.buckets>UINT64_MAX-offset-4 || !Read(nullptr,map.buckets+offset,&at,4))return false;
    for(UINT n=0;n<map.count && n<64 && at!=UINT_MAX;++n){
        if(at>=map.capacity || map.data>UINT64_MAX-uint64_t(at)*64-64)return false;
        std::array<uint8_t,64> entry{};if(!Read(nullptr,map.data+uint64_t(at)*64,entry.data(),entry.size()))return false;
        if(detail::Field<uint32_t>(entry,4)==handle && detail::Field<uint32_t>(entry,8)==handle){descriptor=detail::Field<uint64_t>(entry,0x38);return descriptor!=0;}
        at=detail::Field<uint32_t>(entry,0);
    }return false;
}
bool Native(camera_upload::Context context,uint32_t handle,uint32_t stage,uint32_t reg,Observation& out,bool requireCurrent=true){
    if(!context || !handle || handle>0x100000 || stage>1 || reg>13)return false;
    uint64_t registry{};if(!Read(nullptr,Base()+0x3BCDCA8,&registry,8) || !registry)return false;
    const uint64_t relative=0x5C0AF8+uint64_t(handle-1)*0xB0;
    if(registry>UINT64_MAX-relative-0xB0)return false;
    std::array<uint8_t,0xB0> record{};const auto address=registry+relative;
    if(!Read(nullptr,address,record.data(),record.size()) || (record[6]&15)!=6)return false;
    Observation value;value.context=context;value.logicalBytes=detail::Field<uint32_t>(record,0);value.kind=record[6]>>4;value.stage=stage;value.reg=reg;
    uint64_t descriptor{};
    if(!value.logicalBytes || !CpuDescriptor(context,handle,value.kind,address,descriptor) ||
        !detail::SelectSlot(Read,nullptr,context.state,stage,reg,descriptor,requireCurrent,value.slot))return false;
    if(value.kind!=4){
        value.native={detail::Field<uint64_t>(record,0x10),detail::Field<uint64_t>(record,8),value.logicalBytes,registry,context.context,handle,value.kind};
        if(!value.native.resource || !value.native.gpu || value.native.gpu>UINT64_MAX-value.native.bytes)return false;
    }else{value.native.bytes=value.logicalBytes;value.native.registry=registry;value.native.context=context.context;value.native.handle=handle;value.native.kind=4;}
    out=std::move(value);return true;
}
bool SameNative(const Observation& a,const Observation& b){
    return a.context==b.context && a.slot==b.slot && a.kind==b.kind && a.logicalBytes==b.logicalBytes &&
        a.stage==b.stage && a.reg==b.reg && detail::SameBuffer(a.native,b.native);
}
bool SameSourceNative(const Observation& a,const Observation& b){
    return a.context==b.context && detail::SameSourceSlot(a.slot,b.slot) && a.kind==b.kind && a.logicalBytes==b.logicalBytes &&
        a.stage==b.stage && a.reg==b.reg && detail::SameBuffer(a.native,b.native);
}
bool Frame(uint64_t game,uintptr_t caller,detail::CreationFrame& output){
    __try{
        CONTEXT c{};RtlCaptureContext(&c);ULONG_PTR low{},high{};GetCurrentThreadStackLimits(&low,&high);
        for(UINT n=0;n<32 && high>=8 && c.Rsp>=low && c.Rsp<=high-8;++n){
            if(c.Rip>=game && c.Rip-game<0x2FF3000){
                const auto rva=c.Rip-game;
                if(rva!=0x333C1A && rva!=0x3346B1 && rva!=0x3362A0)return false;
                if(caller>=game && caller-game<0x2FF3000 && caller!=c.Rip)return false;
                output={rva,c.Rsp,c.Rbx,c.Rbp,c.Rdi,c.Rsi,c.R12,c.R13,c.R14,c.R15};return true;
            }
            DWORD64 image{};const auto entry=RtlLookupFunctionEntry(c.Rip,&image,nullptr);
            if(entry){PVOID handler{};DWORD64 establisher{};RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,c.Rip,entry,&c,&handler,&establisher,nullptr);}
            else{if(!Read(nullptr,c.Rsp,&c.Rip,8))return false;c.Rsp+=8;}
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){}return false;
}
bool OriginalUploadWriterPath(uint64_t game){
    __try{
        CONTEXT context{};RtlCaptureContext(&context);ULONG_PTR low{},high{};GetCurrentThreadStackLimits(&low,&high);
        std::array<uint64_t,3> native{};UINT count{};
        for(UINT n=0;n<32 && high>=8 && context.Rsp>=low && context.Rsp<=high-8;++n){
            if(context.Rip>=game && context.Rip-game<0x2FF3000){
                if(count>=native.size())return false;native[count++]=context.Rip-game;
                if(count==native.size())return detail::UploadWriterPath(native);
            }
            DWORD64 image{};const auto entry=RtlLookupFunctionEntry(context.Rip,&image,nullptr);
            if(entry){PVOID handler{};DWORD64 establisher{};RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,context.Rip,entry,&context,&handler,&establisher,nullptr);}
            else{if(!Read(nullptr,context.Rsp,&context.Rip,8))return false;context.Rsp+=8;}
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){}return false;
}
std::vector<Annotation> Bind(uint32_t reg,uint32_t handle,uint32_t stages){
    std::vector<Annotation> result;last=Status::Ignored;if(!Active())return result;
    last=Status::UnsupportedExecutable;if(!Compatible())return result;
    uint32_t tracked{};last=Status::InvalidBinding;if(reg>13 || !detail::TrackStageMask(stages,tracked))return result;
    const auto context=Current();last=Status::MissingContext;if(!context)return result;
    auto* list=reinterpret_cast<ID3D12GraphicsCommandList*>(context.list);native_recording::Lease recording;
    if(!native_recording::Capture(list,context,recording))return result;
    for(UINT stage=0;stage<2;++stage)if(tracked&(1u<<stage)){
        const Key key{context.list,recording.generation,stage,reg};
        decltype(observations)::node_type retired;
        {std::lock_guard lock(mutex);retired=observations.extract(key);}
        Observation value,again;descriptors::CbvBinding cpu;
        last=Status::InvalidBinding;
        if(!Native(context,handle,stage,reg,value,false))continue;
        value.recording=recording;
        Annotation annotation;annotation.cpu=value.slot.descriptor;
        if(value.kind!=4){
            last=Status::UnsupportedResource;
            if(!descriptors::ResolveCpuCbvAddress(value.slot.descriptor,cpu))continue;
            value.address=cpu.address;value.viewBytes=cpu.bytes;annotation.view={cpu.address,cpu.bytes};
            if(value.native.gpu!=cpu.address || value.logicalBytes>cpu.bytes)continue;
            auto lease=geometry::AcquireBuffer(value.native,cpu.address,value.logicalBytes);
            if(!lease.resource || !detail::Descriptor(lease.gpu,lease.bytes,cpu.address,cpu.bytes))continue;
            ComPtr<ID3D12Device> device;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || !Device(device.Get(),lease.resource.Get()))continue;
            value.resource=lease.resource;annotation.origin=MakeOrigin(value.resource.Get(),annotation.view,annotation.cpu,0);
            if(!annotation.origin.owner)continue;
        }
        last=Status::Changed;
        if(Current()!=context || !Native(context,handle,stage,reg,again,false) || !SameNative(value,again) ||
            !native_recording::ValidateRecording(recording,list,context))continue;
        {std::lock_guard lock(mutex);last=Status::Capacity;
            if(observations.size()>=Capacity || nextSerial==UINT64_MAX)continue;value.serial=++nextSerial;
            if(!observations.try_emplace(key,value).second){last=Status::Changed;continue;}}
        if(annotation.origin.owner)result.push_back(std::move(annotation));last=value.kind==4?Status::Transient:Status::Registered;
    }return result;
}
}
void PrepareCodeGuards(uintptr_t base){
    std::lock_guard lock(codeMutex);if(codeAttempted)return;codeAttempted=true;
    struct Code{uint32_t rva,bytes;uint64_t hash;};
    constexpr Code functions[]{
        {0x336B54,487,0x664356FF35FEE308ull},{0x337968,314,0xB6933C2DB6992BFBull},
        {0x336D3C,241,0x6B6B9044CD5F18EAull},{0x337230,51,0x96D1A06A9C2CBA7Full},
        {0x333B88,173,0x76EEBB3582ED71F9ull},{0x333E0C,108,0xDE2A0793240ED287ull},
        {0x3345BC,611,0x4C5BA16F0EB0E34Cull},{0x336100,934,0xD8D2033BD559B499ull},
        {0x334328,75,0xF9FA501E795819C9ull},{0x334374,336,0xBA3063A4D82D7932ull},{0x3344CC,243,0x4C6F33AC0618E59Eull},
        {0x335C3C,1244,0x20371793713C127Full},{0x3E3A0C,423,0xA658FD27E52F148Full},
        {0x5209FC,296,0x79F8D9910A55837Dull},{0xD41788,137,0xFA610B5219D7D92Eull}
    };
    const auto reader=[](uint64_t at,void* out,size_t bytes){return Read(nullptr,at,out,bytes);};
    bool valid=base && base==Base();std::array<uint8_t,2048> bytes{};
    for(const auto& f:functions){if(f.bytes>bytes.size() || base>UINT64_MAX-f.rva || !reader(base+f.rva,bytes.data(),f.bytes)){valid=false;break;}
        uint64_t h=14695981039346656037ull;for(UINT n=0;n<f.bytes;++n)h=(h^bytes[n])*1099511628211ull;if(h!=f.hash){valid=false;break;}}
    verifiedBase=base;codeVerified=valid;
}
bool Compatible(){std::lock_guard lock(codeMutex);return codeVerified && verifiedBase==Base();}
UploadWriterScope::UploadWriterScope(uint32_t handle,uint32_t bytes)noexcept {
    if(uploadWriters.Depth())uploadWriters.Invalidate();
    try{
        if(!Active() || !Compatible())return;const auto context=Current();native_recording::Lease recording;
        if(!context || !native_recording::Capture(reinterpret_cast<ID3D12GraphicsCommandList*>(context.list),context,recording))return;
        stamp_=uploadWriters.Enter(WriterContext(recording),handle,bytes);if(!stamp_.nonce)return;
        ComPtr<WriterReceipt> owner;owner.Attach(new WriterReceipt);owner->recording=recording;
        writerOwners[stamp_.depth-1]=std::move(owner);
    }catch(...){if(stamp_.nonce){uploadWriters.Cancel(stamp_);writerOwners[stamp_.depth-1].Reset();stamp_={};}}
}
UploadWriterScope::~UploadWriterScope(){
    if(!stamp_.nonce)return;
    if(stamp_.depth<=writerOwners.size()){const auto owner=writerOwners[stamp_.depth-1];
        if(owner)owner->stage.store(WriterStage::Cancelled,std::memory_order_release);writerOwners[stamp_.depth-1].Reset();}
    if(!uploadWriters.Cancel(stamp_))uploadWriters.Invalidate();
}
void UploadWriterScope::CompleteNormalReturn()noexcept {
    if(!stamp_.nonce)return;
    try{
        const auto owner=writerOwners[stamp_.depth-1];const auto context=Current();native_recording::Lease recording;
        detail::UploadWriterCompletion completion;
        const bool current=owner && context && Active() && Compatible() &&
            native_recording::Capture(reinterpret_cast<ID3D12GraphicsCommandList*>(context.list),context,recording);
        const bool closed=uploadWriters.Finish(stamp_,current?WriterContext(recording):detail::UploadWriterContext{},completion);
        if(owner){if(closed){owner->completion=completion;owner->stage.store(WriterStage::Completed,std::memory_order_release);}
            else owner->stage.store(WriterStage::Cancelled,std::memory_order_release);}
        writerOwners[stamp_.depth-1].Reset();stamp_={};
    }catch(...){const auto owner=writerOwners[stamp_.depth-1];if(owner)owner->stage.store(WriterStage::Cancelled,std::memory_order_release);
        uploadWriters.Cancel(stamp_);writerOwners[stamp_.depth-1].Reset();stamp_={};}
}
uint32_t LastStatus(){return uint32_t(last);}
Diagnostic LastDiagnostic(){auto result=diagnostic;result.values[1]=uint32_t(last);return result;}
std::vector<Annotation> ObserveBindMask(uint32_t reg,uint32_t handle,uint32_t stages)try{return Bind(reg,handle,stages);}catch(...){last=Status::Failure;return {};}
std::vector<Annotation> ObserveBindStage(uint32_t reg,uint32_t handle,uint32_t stage)try{if(stage>1){last=Status::InvalidBinding;return {};}return Bind(reg,handle,1u<<stage);}catch(...){last=Status::Failure;return {};}
void ObserveCreationAddress(ID3D12Resource* resource,uint64_t address,uintptr_t caller){
    if(caller!=Base()+0x33468F)return;
    addressHint={};if(!Active() || !Compatible() || !resource || !address)return;
    const auto desc=resource->GetDesc();if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !desc.Width || address>UINT64_MAX-desc.Width)return;
    addressHint={resource,address,Current()};
}
descriptors::CbvOrigin ObserveCreatedCbv(ID3D12Device* device,const D3D12_CONSTANT_BUFFER_VIEW_DESC* view,D3D12_CPU_DESCRIPTOR_HANDLE cpu,uintptr_t caller)try{
    last=Status::Ignored;if(!Active())return {};
    last=Status::UnsupportedExecutable;if(!Compatible())return {};
    last=Status::UnknownCreator;if(!device || !view || !cpu.ptr)return {};
    detail::CreationFrame frame;detail::Creation a,b;
    if(!Frame(Base(),caller,frame) || !detail::DecodeCreation(frame,Read,nullptr,a) ||
        !detail::DecodeCreation(frame,Read,nullptr,b) || a!=b || a.cpu!=cpu.ptr)return {};
    const auto context=Current();last=Status::MissingContext;
    if(!context || (a.context && a.context!=context.context))return {};
    ComPtr<ID3D12Resource> source;
    geometry::NativeBuffer native;
    if(a.caller==0x3346B1){
        last=Status::UnknownOrigin;
        Observation first,second;
        if(!Native(context,a.handle,0,0,first,false) || !Native(context,a.handle,0,0,second,false) ||
            !SameNative(first,second) || first.kind!=4 || first.slot.descriptor!=a.cpu)return {};
        native=first.native;
        if(!addressHint.resource || addressHint.context!=context || reinterpret_cast<uintptr_t>(addressHint.resource.Get())!=a.resource ||
            addressHint.gpu>UINT64_MAX-a.offset || addressHint.gpu+a.offset!=view->BufferLocation){addressHint={};return {};}
        source=addressHint.resource;addressHint={};
    }else source=reinterpret_cast<ID3D12Resource*>(a.resource);
    last=Status::UnsupportedResource;D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    const auto gpu=source->GetGPUVirtualAddress();
    if(!Device(device,source.Get()) || gpu>UINT64_MAX-a.offset || gpu+a.offset!=view->BufferLocation ||
        FAILED(source->GetHeapProperties(&heap,&flags)) || (heap.Type!=D3D12_HEAP_TYPE_UPLOAD && heap.Type!=D3D12_HEAP_TYPE_GPU_UPLOAD))return {};
    native_recording::Lease recording;
    // Retain a creation occurrence when native tracking can supply it. The
    // absence of that lease does not discard the established creation facts.
    native_recording::Capture(reinterpret_cast<ID3D12GraphicsCommandList*>(context.list),context,recording);
    auto result=MakeOrigin(source.Get(),*view,cpu.ptr,a.caller,native,&a,&recording);
    if(a.caller==0x3346B1 && uploadWriters.Depth()){
        const auto stamp=uploadWriters.Current();const auto writer=writerOwners[stamp.depth-1];const auto origin=GetOrigin(result.owner.Get());
        const detail::UploadWriterJoin joined{origin?origin->id:0,frame.r15,a.resource,a.offset,a.handle,UINT(native.bytes),view->SizeInBytes};
        if(writer && origin && native.bytes<=65536 && OriginalUploadWriterPath(Base()) &&
            uploadWriters.Join(stamp,WriterContext(recording),joined)){writer->joined=joined;origin->writer=writer;}
        else uploadWriters.Invalidate();
    }
    last=result.owner?Status::Created:Status::Failure;return result;
}catch(...){last=Status::Failure;return {};}
void Reset(ID3D12GraphicsCommandList* list){
    std::map<Key,Observation> retired;
    {std::lock_guard lock(mutex);for(auto it=observations.begin();it!=observations.end();)if(std::get<0>(it->first)==reinterpret_cast<uintptr_t>(list)){
        auto node=observations.extract(it++);retired.insert(std::move(node));}else ++it;}
    if(addressHint.context.list==reinterpret_cast<uintptr_t>(list))addressHint={};
}
void Clear(){std::map<Key,Observation> retired;{std::lock_guard lock(mutex);retired.swap(observations);}addressHint={};}
Status ReadAtSource(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,
    const descriptors::DescriptorStamp& stamp,IUnknown* originOwner,SourceCbvBacking& out)try{
    out={};diagnostic={};last=Status::Ignored;if(!Active())return last;
    At(Phase::Code);last=Status::UnsupportedExecutable;if(!Compatible())return last;
    const auto context=Current();native_recording::Lease recording;last=Status::MissingContext;
    At(Phase::Context);diagnostic.values[5]=context.context;
    if(!list || !root || context.list!=reinterpret_cast<uintptr_t>(list) || !native_recording::Capture(list,context,recording))return last;
    diagnostic.values[4]=recording.generation;diagnostic.values[7]=stamp.value[2];diagnostic.values[8]=stamp.value[3];
    Slot slot,again;UINT stage{},reg{},matches{};
    At(Phase::Slot);
    for(UINT s=0;s<2;++s)for(UINT r=0;r<14;++r){Slot candidate;
        if(detail::ReadSlot(Read,nullptr,context.state,s,r,candidate) && candidate.parameter==parameter && candidate.offset==offset){slot=candidate;stage=s;reg=r;++matches;}}
    last=Status::DescriptorMismatch;if(matches!=1)return last;
    diagnostic.values[2]=stage;diagnostic.values[3]=reg;diagnostic.values[6]=slot.descriptor;
    At(Phase::Stamp);if(!descriptors::MatchesDescriptor(stamp))return last;
    descriptors::CbvBinding actual,cpu;
    At(Phase::Binding);
    if(!descriptors::ResolveCbv(list,root,parameter,offset,actual) || actual.descriptor!=stamp.gpu || actual.heap!=stamp.heap)return last;
    At(Phase::CpuView);
    if(!descriptors::ResolveCpuCbvAddress(slot.descriptor,cpu) || cpu.address!=actual.address || cpu.bytes!=actual.bytes ||
        !detail::SameView(stamp,actual.address,actual.bytes))return last;
    const Key key{context.list,recording.generation,stage,reg};Observation saved;bool observed{};
    At(Phase::Observation);
    {std::lock_guard lock(mutex);const auto found=observations.find(key);if(found!=observations.end()){saved=found->second;observed=true;}}
    diagnostic.values[11]=saved.native.handle;diagnostic.values[12]=saved.kind;diagnostic.values[13]=saved.serial;
    diagnostic.values[14]=saved.address;diagnostic.values[15]=saved.viewBytes;
    auto owner=GetOrigin(originOwner);ComPtr<ID3D12Resource> resource;uint64_t cookie{},id{},logical{};UINT creator{};
    At(Phase::Origin);
    last=Status::UnknownOrigin;
    if((originOwner && !owner) || (!owner && (stamp.value[2] || stamp.value[3])))return last;
    if(owner){
        diagnostic.values[9]=owner->caller;diagnostic.values[10]=owner->cpu;
        last=Status::UnknownOrigin;
        if(!detail::SameOrigin(stamp,owner->cookie,owner->id) ||
            !detail::SameView(stamp,owner->gpu+owner->offset,owner->viewBytes))return last;
        resource=owner->resource;cookie=owner->cookie;id=owner->id;creator=owner->caller;
    }
    // This exact creator writes VS b5 directly, superseding a prior setter.
    const bool direct=owner && detail::DirectCreation(stage,reg,owner->caller);
    if(direct){
        const detail::Creation creation{reinterpret_cast<uintptr_t>(owner->resource.Get()),owner->offset,owner->cpu,0,
            owner->caller,0,owner->logicalBytes,owner->reservedBytes,owner->completedProducerRva};
        if(!detail::DirectProducer(creation,actual.bytes))return last;
        logical=owner->logicalBytes;observed=false;saved={};
    }
    const bool transient=owner && owner->caller==0x3346B1 && owner->native.kind==4;
    bool nativeReceipt=observed;
    At(Phase::Native);
    if(transient){
        Observation current;last=Status::Changed;
        if(owner->native.context!=context.context || owner->cpu!=slot.descriptor ||
            !Native(context,owner->native.handle,stage,reg,current) || current.kind!=4 ||
            !detail::SameBuffer(owner->native,current.native))return last;
        // Upload updates a previously bound descriptor; no setter follows it.
        saved=std::move(current);observed=false;nativeReceipt=true;logical=saved.logicalBytes;
        diagnostic.values[11]=saved.native.handle;diagnostic.values[12]=saved.kind;
        diagnostic.values[13]=0;diagnostic.values[14]=0;diagnostic.values[15]=0;
    }else if(observed){
        Observation current;last=Status::Changed;
        if(saved.context!=context || !Native(context,saved.native.handle,stage,reg,current) || !SameSourceNative(saved,current))return last;
        logical=saved.logicalBytes;
        if(saved.kind!=4){
            if(!saved.resource || reinterpret_cast<uintptr_t>(saved.resource.Get())!=current.native.resource ||
                (resource && resource!=saved.resource) || saved.address!=actual.address || saved.viewBytes!=actual.bytes)return last;
            resource=saved.resource;if(!cookie)cookie=descriptors::SourceResourceCookieAtCreation(resource.Get());
        }else{last=Status::UnknownOrigin;return last;}
    }else if(!direct){
        last=Status::MissingObservation;return last;
    }
    At(Phase::Resource);last=Status::UnsupportedResource;ComPtr<ID3D12Device> device;
    if(!resource || !cookie || FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || !Device(device.Get(),resource.Get()))return last;
    const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !detail::Descriptor(gpu,desc.Width,actual.address,actual.bytes))return last;
    At(Phase::Revalidate);last=Status::Changed;
    if(Current()!=context || !detail::ReadSlot(Read,nullptr,context.state,stage,reg,again) || again!=slot ||
        !native_recording::ValidateRecording(recording,list,context) || !descriptors::MatchesDescriptor(stamp))return last;
    if(nativeReceipt){Observation current;
        if(!Native(context,saved.native.handle,stage,reg,current) || !SameSourceNative(saved,current))return last;}
    if(observed){At(Phase::Serial);std::lock_guard lock(mutex);const auto found=observations.find(key);if(found==observations.end() || found->second.serial!=saved.serial)return last;}
    out.resource=std::move(resource);out.originOwner=owner;out.stamp=stamp;out.recording=std::move(recording);out.native=saved.native;
    out.cookie=cookie;out.origin=id;out.gpu=gpu;out.backingBytes=desc.Width;out.sourceOffset=actual.address-gpu;out.serial=saved.serial;
    out.cpuDescriptor=slot.descriptor;out.descriptorBytes=actual.bytes;out.logicalBytes=UINT(logical);out.stage=stage;out.shaderRegister=reg;out.creatorRva=creator;
    if(owner)out.creationRecording=owner->creationRecording;
    if(direct){out.reservedBytes=owner->reservedBytes;out.completedProducerRva=owner->completedProducerRva;}
    if(transient && owner->writer && owner->writer->stage.load(std::memory_order_acquire)==WriterStage::Completed){
        const auto& writer=*owner->writer.Get();const auto& completed=writer.completion;const auto& joined=writer.joined;
        if(completed.nonce && completed.origin==owner->id && completed.destination==joined.destination && completed.producerRva==detail::UploadWriterMemcpyReturnRva &&
            joined.resource==reinterpret_cast<uintptr_t>(out.resource.Get()) && joined.offset==owner->offset && joined.handle==saved.native.handle &&
            joined.logicalBytes==saved.logicalBytes && joined.reservedBytes==actual.bytes && completed.bytes && completed.bytes<=saved.logicalBytes){
            out.writerNonce=completed.nonce;out.writerBytes=completed.bytes;out.writerDestination=completed.destination;out.writerRecording=writer.recording;
            out.reservedBytes=actual.bytes;out.completedProducerRva=completed.producerRva;
        }
    }
    At(Phase::Complete);last=observed && saved.kind!=4?Status::Registered:Status::Created;return last;
}catch(...){out={};last=Status::Failure;return last;}
}
#endif
