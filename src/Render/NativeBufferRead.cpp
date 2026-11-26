#include "Render/NativeBufferRead.hpp"
#include "Render/NativeCopyWindows.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <map>
#include <mutex>
#include <vector>

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// THE PROOFS ARE RUST'S (rust/crates/cvr-render/src/buffer_read.rs): this face calls the copy windows' observers first,
// as the former code did, and hands Rust the engine's current upload context and the copy windows' unavailability.
namespace cvr::stereo::buffer_read::rust_face {
struct Lease {ID3D12Resource* resource;uint64_t gpu,bytes;uint32_t heapType,flags;geometry::NativeBuffer native;};
static_assert(sizeof(Lease)==80 && sizeof(Proof)==96 && sizeof(geometry::NativeBuffer)==48 && sizeof(camera_upload::Context)==24,
    "mirrored by rust/crates/cvr-render/src/buffer_read.rs");
inline Lease Raw(const geometry::BufferLease& s){return {s.resource.Get(),s.gpu,s.bytes,s.heapType,s.flags,s.native};}
struct Attach {void (*currentContext)(uint64_t,camera_upload::Context*);void (*copyUnavailable)();};
}
extern "C" {
void cvr_buffer_read_attach(const cvr::stereo::buffer_read::rust_face::Attach*);
}
namespace cvr::stereo::buffer_read {
namespace {
void CurrentContextForRust(uint64_t game,camera_upload::Context* out){*out=camera_upload::CurrentContext(uintptr_t(game));}
void CopyUnavailableForRust(){copy_windows::Unavailable();}
const rust_face::Attach kAttach{&CurrentContextForRust,&CopyUnavailableForRust};
[[maybe_unused]] const bool kAttached=(cvr_buffer_read_attach(&kAttach),true);
}
}
#endif
#else
namespace cvr::stereo::buffer_read {
namespace {
thread_local uint32_t failure{},poolFailure{};
thread_local std::array<uint64_t,4> failureDetails{};
constexpr GUID receiptKey{0xcf2d37a1,0x14b7,0x42d0,{0x95,0x1a,0x41,0xb3,0xd8,0x6e,0x7c,0x10}};
constexpr GUID receiptIdKey{0xcf2d37a1,0x14b7,0x42d0,{0x95,0x1a,0x41,0xb3,0xd8,0x6e,0x7c,0x11}};
constexpr GUID recordingKey{0xcf2d37a1,0x14b7,0x42d0,{0x95,0x1a,0x41,0xb3,0xd8,0x6e,0x7c,0x12}};
constexpr GUID recordingIdKey{0xcf2d37a1,0x14b7,0x42d0,{0x95,0x1a,0x41,0xb3,0xd8,0x6e,0x7c,0x13}};
struct Receipt;
struct Recording;
std::atomic<Receipt*> retiredReceipts{};
std::atomic<Recording*> retiredRecordings{};
struct Receipt final:IUnknown {
    std::atomic<ULONG> refs{1};uint64_t id{},registry{},uploadQueue{},gpu{},bytes{};
    ID3D12Resource* resource{};detail::History history;Receipt* next{};
    uint32_t firstReason{};std::array<uint64_t,4> firstDetails{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid,void** out)override{
        if(!out)return E_POINTER;*out=nullptr;if(iid!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{
        const auto count=--refs;if(!count){next=retiredReceipts.load(std::memory_order_relaxed);
            while(!retiredReceipts.compare_exchange_weak(next,this,std::memory_order_release,std::memory_order_relaxed)){}
        }return count;
    }
};
struct Recording final:IUnknown {
    std::atomic<ULONG> refs{1};uint64_t id{},generation{};ID3D12CommandList* list{};
    detail::RecordingState state;std::vector<uint64_t> writes;Recording* next{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid,void** out)override{
        if(!out)return E_POINTER;*out=nullptr;if(iid!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{
        const auto count=--refs;if(!count){next=retiredRecordings.load(std::memory_order_relaxed);
            while(!retiredRecordings.compare_exchange_weak(next,this,std::memory_order_release,std::memory_order_relaxed)){}
        }return count;
    }
};
std::mutex mutex;
std::map<uint64_t,Receipt*> receipts;
std::map<uint64_t,Recording*> recordings;
uint64_t nextReceipt{},nextRecording{},nextGeneration{};
bool unavailable{};
constexpr size_t ReceiptLimit=16384,RecordingLimit=4096,WriteLimit=4096;
bool Enabled(){return native_probe::ObserveBindings();}
bool Read(void*,uint64_t address,void* out,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),out,bytes,&copied) && copied==bytes;
}
uint64_t HistoryBits(const detail::History& history){
    return uint64_t(history.valid)|uint64_t(history.uploaded)<<1|uint64_t(history.pending)<<2;
}
struct HistoryDiagnostic {uint64_t producer{},bits{};bool valid{};};
HistoryDiagnostic Diagnostic(const detail::History& history){return {history.producer,HistoryBits(history),history.valid};}
uint64_t RecordingBits(const detail::RecordingState& state){
    return uint64_t(state.valid)|uint64_t(state.pending)<<1|uint64_t(state.submitted)<<2;
}
void FirstCause(Receipt& receipt,Invalidation reason,std::array<uint64_t,4> details){
    if(receipt.firstReason)return;
    receipt.firstReason=uint32_t(reason);receipt.firstDetails=details;
}
void Invalidate(Receipt& receipt,Invalidation reason,std::array<uint64_t,4> details){
    if(receipt.history.valid)FirstCause(receipt,reason,details);
    receipt.history.Invalidate();
}
void Abandon(const Recording& recording){
    for(const auto id:recording.writes){const auto found=receipts.find(id);
        if(found!=receipts.end()){
            auto& receipt=*found->second;const auto was=Diagnostic(receipt.history);
            receipt.history.Abandon(recording.generation);
            if(was.valid && !receipt.history.valid)FirstCause(receipt,Invalidation::Abandon,
                {reinterpret_cast<uintptr_t>(recording.list),recording.generation,was.producer,was.bits});
        }
    }
}
void DrainRetired(){
    auto* receipt=retiredReceipts.exchange(nullptr,std::memory_order_acquire);
    while(receipt){auto* next=receipt->next;const auto found=receipts.find(receipt->id);
        if(found!=receipts.end() && found->second==receipt)receipts.erase(found);
        delete receipt;receipt=next;
    }
    auto* recording=retiredRecordings.exchange(nullptr,std::memory_order_acquire);
    while(recording){auto* next=recording->next;const auto found=recordings.find(recording->id);
        if(found!=recordings.end() && found->second==recording){Abandon(*recording);recordings.erase(found);}
        delete recording;recording=next;
    }
}
Receipt* ReceiptOf(ID3D12Resource* resource){
    uint64_t id{};UINT bytes=sizeof(id);
    if(!resource || FAILED(resource->GetPrivateData(receiptIdKey,&bytes,&id)) || bytes!=sizeof(id))return nullptr;
    const auto found=receipts.find(id);
    return found!=receipts.end() && found->second->resource==resource?found->second:nullptr;
}
Recording* RecordingOf(ID3D12CommandList* list){
    uint64_t id{};UINT bytes=sizeof(id);
    if(!list || FAILED(list->GetPrivateData(recordingIdKey,&bytes,&id)) || bytes!=sizeof(id))return nullptr;
    const auto found=recordings.find(id);
    return found!=recordings.end() && found->second->list==list?found->second:nullptr;
}
bool Compatible(){
    static const bool compatible=[] {
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        struct Code {uint32_t rva,bytes;uint64_t hash;};
        constexpr Code expected[]{
            {0x688F14,0x2F1,0xD50BD7002FEF17E1ull},{0x3DF71C,0x6B,0xF224A3F5039E06C6ull},
            {0x3445AC,0x5A2,0x612F7F49D216A154ull},{0x344B8C,0xC4,0xE3617AB0F86D80D7ull},
            {0x3490F4,0x62,0xAF5DD974E8F5ED23ull},{0x349254,0x282,0x172367073F0F6C1Cull},
            {0x3495D8,0x226,0xCA9BB65B64144A83ull},{0x34A688,0x3EB,0xF2AA1E0EF7125B55ull},
            {0x22F2D00,0x384,0x09BF1A44FA19923Full},  // 2.5 rewrite of 2.31 sub_14021E238, not re-audited
           {0x3E5D54,1695,0x0FB56D2DE5F3E34Aull},
            {0x3E8A9C,48,0x356A8175F9F055FAull},{0x3E7B90,183,0x64042E928C7E9B3Cull},
            {0x5B3DA8,640,0xE030F844D980B8EEull},{0x5B4110,77,0xFA1D4E49FE87F2EEull},
            {0x22EF830,1493,0x9117D2376320C0B7ull},{0x3E1020,682,0x898F20179715110Full},
            {0x3E47E8,382,0xD8695C867CC19844ull},{0x3E12CC,148,0xA99EAC2C327A1A01ull},
            {0x337DA0,0x180,0xB36B2D868965D718ull},{0x3E3BB4,0x34,0x1A0F0B6F177A422Cull}
        };
        std::array<uint8_t,2048> bytes{};
        for(const auto code:expected){
            if(!game || game>UINT64_MAX-code.rva || code.bytes>bytes.size() || !Read(nullptr,game+code.rva,bytes.data(),code.bytes))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=code.hash)return false;
        }return true;
    }();return compatible;
}
bool QueueAddress(uint64_t registry,bool upload,uint64_t& result){
    result=0;const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));uint64_t current{};
    const uint64_t offset=upload?0x1410630:0x14104D0;
    return registry && registry<=UINT64_MAX-offset-8 && Read(nullptr,game+0x3BCDCA8,&current,8) && current==registry &&
        Read(nullptr,registry+offset,&result,8) && result;
}
bool NativeQueue(uint64_t registry,uint64_t expected,ID3D12CommandQueue* queue,bool upload){
    uint64_t current{};return queue && expected==reinterpret_cast<uintptr_t>(queue) &&
        QueueAddress(registry,upload,current) && current==expected &&
        queue->GetDesc().Type==(upload?D3D12_COMMAND_LIST_TYPE_COPY:D3D12_COMMAND_LIST_TYPE_DIRECT);
}
bool Uploader(ID3D12GraphicsCommandList* list,uint64_t& registry,uint64_t& queue){
    registry=queue=0;if(!list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_COPY || !Compatible())return false;
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));uint64_t context{},actual{};uint32_t kind{};
    return Read(nullptr,game+0x3BCDCA8,&registry,8) && registry && registry<=UINT64_MAX-0x1410630 &&
        Read(nullptr,registry+0x1410628,&context,8) && context && context<=UINT64_MAX-0x6C &&
        Read(nullptr,context+0x30,&actual,8) && actual==reinterpret_cast<uintptr_t>(list) &&
        Read(nullptr,context+0x68,&kind,4) && kind==3 && QueueAddress(registry,true,queue);
}
bool Context(const camera_upload::Context& context,const geometry::NativeBuffer& source){
    uint64_t actual{};uint32_t kind{},pending{};
    return context && context.context==source.context && context.context<=UINT64_MAX-0x638 &&
        Read(nullptr,context.context+0x30,&actual,8) && actual==context.list &&
        Read(nullptr,context.context+0x68,&kind,4) && (kind==1 || kind==2) &&
        Read(nullptr,context.context+0x534,&pending,4) && !pending;
}
bool PoolOfRange(const geometry::BufferLease& source,uint64_t comOffset,uint64_t copyBytes,Pool& pool){
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));Pool again;
    return detail::ReadPoolRange(Read,nullptr,game,source.native,source.gpu,source.bytes,comOffset,copyBytes,pool,&poolFailure) &&
        detail::ReadPoolRange(Read,nullptr,game,source.native,source.gpu,source.bytes,comOffset,copyBytes,again,&poolFailure) && pool==again;
}
bool BufferShape(const geometry::BufferLease& source){
    if(!source.resource)return false;
    const auto desc=source.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && desc.Width==source.bytes && desc.Flags==D3D12_RESOURCE_FLAG_NONE &&
        source.resource->GetGPUVirtualAddress()==source.gpu && SUCCEEDED(source.resource->GetHeapProperties(&heap,&flags)) &&
        heap.Type==D3D12_HEAP_TYPE_DEFAULT;
}
bool Residency(const geometry::NativeBuffer& source){
    const uint64_t offset=0x5C0AF8+uint64_t(source.handle-1)*0xB0;uint64_t owner{},tracker{};uint8_t failed{};
    if(source.registry>UINT64_MAX-offset-0x70 || !Read(nullptr,source.registry+offset+0x68,&owner,8))return false;
    if(!owner)return true;
    return Read(nullptr,source.context+0x630,&tracker,8) && tracker && tracker<=UINT64_MAX-38 &&
        Read(nullptr,tracker+37,&failed,1) && !failed;
}
}
void ObserverUnavailable(){copy_windows::Unavailable();std::lock_guard lock(mutex);unavailable=true;}
void ResourceCreated(ID3D12Resource* resource,D3D12_RESOURCE_STATES initial,bool placed){
    if(!Enabled() || probe::internalCommands || !resource || placed || !detail::InitialState(initial))return;
    try{
        const auto desc=resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Flags!=D3D12_RESOURCE_FLAG_NONE ||
            FAILED(resource->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_DEFAULT)return;
        std::lock_guard lock(mutex);DrainRetired();if(unavailable || ReceiptOf(resource))return;
        if(receipts.size()>=ReceiptLimit || nextReceipt==UINT64_MAX){unavailable=true;return;}
        Microsoft::WRL::ComPtr<Receipt> receipt;receipt.Attach(new Receipt);
        receipt->id=++nextReceipt;receipt->resource=resource;
        receipt->gpu=resource->GetGPUVirtualAddress();receipt->bytes=desc.Width;
        if(!receipt->gpu || !receipt->bytes || receipt->gpu>UINT64_MAX-receipt->bytes){unavailable=true;return;}
        if(FAILED(resource->SetPrivateDataInterface(receiptKey,receipt.Get())) ||
            FAILED(resource->SetPrivateData(receiptIdKey,sizeof(receipt->id),&receipt->id))){unavailable=true;return;}
        receipts.emplace(receipt->id,receipt.Get());
    }catch(...){ObserverUnavailable();}
}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers){
    copy_windows::Barriers(count,barriers);
    if(!Enabled() || !count)return;
    if(!barriers){ObserverUnavailable();return;}
    try{
        std::lock_guard lock(mutex);DrainRetired();
        for(UINT i=0;i<count;++i){const auto& barrier=barriers[i];
            if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION){
                if(!barrier.Transition.pResource){unavailable=true;return;}
                if(auto* receipt=ReceiptOf(barrier.Transition.pResource))Invalidate(*receipt,Invalidation::Transition,
                    {reinterpret_cast<uintptr_t>(list),UINT(barrier.Transition.StateBefore),UINT(barrier.Transition.StateAfter),
                        uint64_t(barrier.Flags)<<32|barrier.Transition.Subresource});
            }else if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING){
                // Every eligible receipt is committed, so distinct allocations
                // cannot physically overlap. Explicit references still reject.
                const std::array<uint64_t,4> details{reinterpret_cast<uintptr_t>(list),
                    reinterpret_cast<uintptr_t>(barrier.Aliasing.pResourceBefore),reinterpret_cast<uintptr_t>(barrier.Aliasing.pResourceAfter),0};
                if(auto* receipt=ReceiptOf(barrier.Aliasing.pResourceBefore))Invalidate(*receipt,Invalidation::Alias,details);
                if(auto* receipt=ReceiptOf(barrier.Aliasing.pResourceAfter))Invalidate(*receipt,Invalidation::Alias,details);
            }
        }
    }catch(...){ObserverUnavailable();}
}
void Written(ID3D12GraphicsCommandList* list,ID3D12Resource* destination){
    copy_windows::Write(destination);
    if(!Enabled())return;
    try{
        std::lock_guard lock(mutex);DrainRetired();
        if(!destination){unavailable=true;return;}
        auto* receipt=ReceiptOf(destination);if(!receipt || !receipt->history.valid)return;
        auto* recording=RecordingOf(list);uint64_t registry{},queue{};
        const auto address=reinterpret_cast<uintptr_t>(list);
        if(!recording){Invalidate(*receipt,Invalidation::MissingRecording,{address,0,0,0});return;}
        if(!recording->state.valid || recording->state.pending || recording->state.submitted){
            Invalidate(*receipt,Invalidation::Recording,{address,recording->generation,RecordingBits(recording->state),0});return;
        }
        if(!Uploader(list,registry,queue)){
            Invalidate(*receipt,Invalidation::Uploader,{address,UINT(list->GetType()),registry,queue});return;
        }
        if(receipt->registry && (receipt->registry!=registry || receipt->uploadQueue!=queue)){
            Invalidate(*receipt,Invalidation::Origin,{address,recording->generation,receipt->registry,receipt->uploadQueue});return;
        }
        if(std::find(recording->writes.begin(),recording->writes.end(),receipt->id)==recording->writes.end()){
            if(recording->writes.size()>=WriteLimit){unavailable=true;return;}
            recording->writes.push_back(receipt->id);
        }
        receipt->registry=registry;receipt->uploadQueue=queue;const auto was=Diagnostic(receipt->history);
        receipt->history.Written(recording->generation);
        if(was.valid && !receipt->history.valid)FirstCause(*receipt,Invalidation::WriteHistory,
            {address,recording->generation,was.producer,was.bits});
    }catch(...){ObserverUnavailable();}
}
void WrittenAddress(uint64_t address,uint64_t bytes){
    copy_windows::Address(address,bytes);
    if(!Enabled() || !bytes)return;
    try{
        std::lock_guard lock(mutex);DrainRetired();
        if(!address || address>UINT64_MAX-bytes){unavailable=true;return;}
        for(auto& [id,receipt]:receipts)if(address<receipt->gpu+receipt->bytes && receipt->gpu<address+bytes)
            Invalidate(*receipt,Invalidation::AddressWrite,{address,bytes,receipt->gpu,receipt->bytes});
    }catch(...){ObserverUnavailable();}
}
void Reset(ID3D12GraphicsCommandList* list){
    copy_windows::Reset(list);
    if(!Enabled() || !list)return;
    const auto type=list->GetType();if(type!=D3D12_COMMAND_LIST_TYPE_DIRECT && type!=D3D12_COMMAND_LIST_TYPE_COPY)return;
    try{
        std::lock_guard lock(mutex);DrainRetired();if(unavailable)return;
        if(nextGeneration==UINT64_MAX){unavailable=true;return;}
        if(auto* recording=RecordingOf(list)){
            Abandon(*recording);recording->generation=++nextGeneration;recording->state={};recording->writes.clear();return;
        }
        if(recordings.size()>=RecordingLimit || nextRecording==UINT64_MAX){unavailable=true;return;}
        Microsoft::WRL::ComPtr<Recording> recording;recording.Attach(new Recording);
        recording->id=++nextRecording;recording->generation=++nextGeneration;recording->list=list;
        if(FAILED(list->SetPrivateDataInterface(recordingKey,recording.Get())) ||
            FAILED(list->SetPrivateData(recordingIdKey,sizeof(recording->id),&recording->id))){unavailable=true;return;}
        recordings.emplace(recording->id,recording.Get());
    }catch(...){ObserverUnavailable();}
}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!Enabled() || !count)return;
    if(!queue || !lists){ObserverUnavailable();return;}
    try{
        std::lock_guard lock(mutex);DrainRetired();
        for(UINT i=0;i<count;++i){auto* recording=RecordingOf(lists[i]);if(!recording)continue;
            const bool fresh=recording->state.BeforeSubmit();
            for(const auto id:recording->writes){const auto found=receipts.find(id);if(found==receipts.end())continue;
                auto& receipt=*found->second;
                const auto was=Diagnostic(receipt.history);const bool matches=fresh && NativeQueue(receipt.registry,receipt.uploadQueue,queue,true);
                receipt.history.BeforeSubmit(recording->generation,matches);
                if(was.valid && !receipt.history.valid){
                    const auto reason=!fresh?Invalidation::BeforeRecording:!matches?Invalidation::BeforeQueue:Invalidation::BeforeHistory;
                    FirstCause(receipt,reason,{reinterpret_cast<uintptr_t>(recording->list),recording->generation,
                        reason==Invalidation::BeforeHistory?was.producer:reinterpret_cast<uintptr_t>(queue),
                        reason==Invalidation::BeforeHistory?was.bits:receipt.uploadQueue});
                }
            }
        }
    }catch(...){ObserverUnavailable();}
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!Enabled() || !count)return;
    if(!queue || !lists){ObserverUnavailable();return;}
    try{
        std::lock_guard lock(mutex);DrainRetired();
        for(UINT i=0;i<count;++i){auto* recording=RecordingOf(lists[i]);if(!recording)continue;
            const bool fresh=recording->state.Submitted();
            for(const auto id:recording->writes){const auto found=receipts.find(id);if(found==receipts.end())continue;
                auto& receipt=*found->second;
                const auto was=Diagnostic(receipt.history);const bool matches=fresh && NativeQueue(receipt.registry,receipt.uploadQueue,queue,true);
                receipt.history.Submitted(recording->generation,matches);
                if(was.valid && !receipt.history.valid){
                    const auto reason=!fresh?Invalidation::AfterRecording:!matches?Invalidation::AfterQueue:Invalidation::AfterHistory;
                    FirstCause(receipt,reason,{reinterpret_cast<uintptr_t>(recording->list),recording->generation,
                        reason==Invalidation::AfterHistory?was.producer:reinterpret_cast<uintptr_t>(queue),
                        reason==Invalidation::AfterHistory?was.bits:receipt.uploadQueue});
                }
            }
        }
    }catch(...){ObserverUnavailable();}
}
namespace {
bool ValidateObservation(const geometry::BufferLease& source,const Proof& proof,uint64_t comOffset,uint64_t copyBytes,ID3D12CommandQueue* queue){
    if(!Enabled() || !Compatible() || !source.resource || source.heapType!=D3D12_HEAP_TYPE_DEFAULT || source.flags ||
        source.native.resource!=reinterpret_cast<uintptr_t>(source.resource.Get()) || proof.registry!=source.native.registry ||
        !proof.receipt || !proof.recording || !proof.generation || !proof.directQueue || proof.comOffset!=comOffset || proof.copyBytes!=copyBytes ||
        !detail::ValidRange(source.native,source.gpu,source.bytes,comOffset,copyBytes) || !BufferShape(source))return false;
    uint64_t direct{};Pool pool;
    if(!QueueAddress(proof.registry,false,direct) || direct!=proof.directQueue || !PoolOfRange(source,comOffset,copyBytes,pool) || pool!=proof.pool ||
        (queue && !NativeQueue(proof.registry,proof.directQueue,queue,false)))return false;
    std::lock_guard lock(mutex);DrainRetired();const auto* receipt=ReceiptOf(source.resource.Get());
    const auto found=recordings.find(proof.recording);
    return !unavailable && receipt && receipt->id==proof.receipt && receipt->registry==proof.registry &&
        receipt->history.Matches(proof.epoch) && found!=recordings.end() &&
        found->second->generation==proof.generation && found->second->state.valid && !found->second->state.submitted &&
        (!queue || found->second->state.pending);
}
}
bool ValidateRange(const geometry::BufferLease& source,const Proof& proof,uint64_t comOffset,uint64_t copyBytes,ID3D12CommandQueue* queue)try{
    return detail::RegisteredRange(proof,comOffset,copyBytes) && ValidateObservation(source,proof,comOffset,copyBytes,queue);
}catch(...){return false;}
bool Validate(const geometry::BufferLease& source,const Proof& proof,ID3D12CommandQueue* queue){
    return source.native.bytes>=292 && source.native.gpu>=source.gpu &&
        ValidateRange(source,proof,source.native.gpu-source.gpu,292,queue);
}
uint32_t LastFailure(){return failure;}
std::array<uint64_t,4> LastFailureDetails(){return failureDetails;}
bool PreflightRange(ID3D12GraphicsCommandList* list,const geometry::BufferLease& source,uint64_t comOffset,uint64_t copyBytes,Proof& out)try{
    out={};failureDetails={};failure=1;if(!Enabled() || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT ||
        !source.resource || source.heapType!=D3D12_HEAP_TYPE_DEFAULT || source.flags || source.native.kind!=1 ||
        source.native.resource!=reinterpret_cast<uintptr_t>(source.resource.Get()) ||
        !detail::ValidRange(source.native,source.gpu,source.bytes,comOffset,copyBytes) || !BufferShape(source))return false;
    failure=2;if(!Compatible())return false;
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(game);
    failure=3;if(context.list!=reinterpret_cast<uintptr_t>(list) || !Context(context,source.native))return false;
    Proof next;next.registry=source.native.registry;next.comOffset=comOffset;next.copyBytes=copyBytes;
    if(!PoolOfRange(source,comOffset,copyBytes,next.pool)){failure=20+poolFailure;return false;}
    failure=4;if(!QueueAddress(next.registry,false,next.directQueue))return false;
    {
        std::lock_guard lock(mutex);DrainRetired();const auto* receipt=ReceiptOf(source.resource.Get());
        const auto* recording=RecordingOf(list);
        failure=5;if(unavailable)return false;
        failure=6;if(!receipt)return false;
        failureDetails=receipt->firstDetails;
        if(!receipt->history.valid && receipt->firstReason){failure=200+receipt->firstReason;return false;}
        failure=7;if(receipt->registry!=next.registry)return false;
        if(!receipt->history.Matches(receipt->history.epoch)){
            failure=100+UINT(HistoryBits(receipt->history));return false;
        }
        failure=8;if(!recording)return false;
        failure=9;if(!recording->state.valid || recording->state.pending || recording->state.submitted)return false;
        next.receipt=receipt->id;next.epoch=receipt->history.epoch;next.recording=recording->id;next.generation=recording->generation;
    }
    failure=10;if(!ValidateObservation(source,next,comOffset,copyBytes,nullptr) || camera_upload::CurrentContext(game)!=context ||
        !Context(context,source.native) || !Residency(source.native))return false;
    out=next;failure=0;return true;
}catch(...){out={};failure=12;return false;}
bool CaptureRange(ID3D12GraphicsCommandList* list,const geometry::BufferLease& source,uint64_t comOffset,uint64_t copyBytes,Proof& out)try{
    const auto expected=out;out={};Proof next;
    if(!PreflightRange(list,source,comOffset,copyBytes,next))return false;
    if(expected!=Proof{} && !detail::SameRangeObservation(expected,next)){failure=13;return false;}
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(game);
    failure=10;if(context.list!=reinterpret_cast<uintptr_t>(list) || !Context(context,source.native) ||
        !ValidateObservation(source,next,comOffset,copyBytes,nullptr))return false;
    using Register=void(*)(void*,uint32_t,uint32_t);
    const auto registerRead=reinterpret_cast<Register>(game+0x337DA0);
    // Class 1 registers allocation use but skips the native transition maps.
    // Both the source copy and following CBV draw use cumulative read promotion.
    {
        probe::Scope internal;
        registerRead(reinterpret_cast<void*>(context.context),source.native.handle,
            D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER|D3D12_RESOURCE_STATE_COPY_SOURCE);
    }
    next.readRegistered=true;
    failure=11;if(camera_upload::CurrentContext(game)!=context || !Context(context,source.native) ||
        !Residency(source.native) || !ValidateRange(source,next,comOffset,copyBytes))return false;
    out=next;failure=0;return true;
}catch(...){out={};failure=12;return false;}
bool Capture(ID3D12GraphicsCommandList* list,const geometry::BufferLease& source,Proof& out){
    out={};failureDetails={};failure=1;
    if(source.native.bytes<292 || source.native.gpu<source.gpu)return false;
    return CaptureRange(list,source,source.native.gpu-source.gpu,292,out);
}
}
#endif
