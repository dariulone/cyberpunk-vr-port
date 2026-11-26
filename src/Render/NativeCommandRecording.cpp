#include "Render/NativeCommandRecording.hpp"
#include "Render/NativeStereoProbe.hpp"
#include <windows.h>
#include <atomic>
#include <mutex>
#include <utility>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// THE CONTRACT IS RUST'S (rust/crates/cvr-render/src/native_recording.rs): the tokens, held by the lists as private
// data, the captures, the submissions and the failures; this face hands it the C++ half's arguments and leases.
extern "C" {
struct CvrRecordingLease { void* token; uint64_t generation, context, list, registry; };
bool cvr_native_recording_compatible();
bool cvr_native_recording_capture(ID3D12GraphicsCommandList* list, const cvr::stereo::camera_upload::Context* context,
                                  CvrRecordingLease* out);
bool cvr_native_recording_validate_recording(const CvrRecordingLease* lease, ID3D12GraphicsCommandList* list,
                                              const cvr::stereo::camera_upload::Context* context);
bool cvr_native_recording_validate_submission(const CvrRecordingLease* lease, ID3D12CommandQueue* queue);
uint32_t cvr_native_recording_last_failure();
}
namespace cvr::stereo::native_recording {
static_assert(sizeof(Lease) == sizeof(CvrRecordingLease) && offsetof(Lease, generation) == 8 && offsetof(Lease, registry) == 32 &&
                  sizeof(camera_upload::Context) == 24,
              "mirrored by Lease and Context in rust/crates/cvr-render/src/native_recording.rs");
namespace {
const CvrRecordingLease* Raw(const Lease& lease) { return reinterpret_cast<const CvrRecordingLease*>(&lease); }
}  // namespace
bool Compatible() { return cvr_native_recording_compatible(); }
uint32_t LastFailure() { return cvr_native_recording_last_failure(); }
bool Capture(ID3D12GraphicsCommandList* list, camera_upload::Context context, Lease& output) {
    output = {};
    CvrRecordingLease raw{};
    const bool captured = cvr_native_recording_capture(list, &context, &raw);
    output.token.Attach(static_cast<IUnknown*>(raw.token));
    output.generation = raw.generation;
    output.context = raw.context;
    output.list = raw.list;
    output.registry = raw.registry;
    return captured;
}
bool ValidateRecording(const Lease& lease, ID3D12GraphicsCommandList* list, camera_upload::Context context) {
    return cvr_native_recording_validate_recording(Raw(lease), list, &context);
}
bool ValidateSubmission(const Lease& lease, ID3D12CommandQueue* queue) {
    return cvr_native_recording_validate_submission(Raw(lease), queue);
}
}  // namespace cvr::stereo::native_recording
#endif
#else

namespace cvr::stereo::native_recording {
using Microsoft::WRL::ComPtr;
namespace {
thread_local uint32_t failure{};
thread_local std::array<uint64_t,8> details{};
std::mutex mutex;uint64_t nextGeneration{};
constexpr GUID ownerKey{0x39721d23,0x9ccf,0x4793,{0x88,0x43,0x18,0x66,0x7b,0x0a,0x6e,0x10}};
constexpr GUID tokenType{0x39721d23,0x9ccf,0x4793,{0x88,0x43,0x18,0x66,0x7b,0x0a,0x6e,0x11}};
struct Token final:IUnknown {
    std::atomic<ULONG> refs{1};std::atomic<bool> active{true};
    uint64_t generation{},context{},list{},registry{};bool nativeReset{};
    detail::SubmissionState submission;ID3D12CommandQueue* queue{};
    std::array<uint64_t,8> observed{};uint32_t rejection{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{
        if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown) && id!=tokenType)return E_NOINTERFACE;
        *out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto left=--refs;if(!left)delete this;return left;}
};
struct Owner final:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<Token> token;
    explicit Owner(ComPtr<Token> value):token(std::move(value)){}
    ~Owner(){token->active.store(false,std::memory_order_release);}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{
        if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown) && id!=ownerKey)return E_NOINTERFACE;
        *out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto left=--refs;if(!left)delete this;return left;}
};
bool Enabled(){static const bool enabled=native_probe::ObserveBindings();return enabled;}
uint64_t Base(){return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
bool Read(uint64_t address,void* out,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),out,bytes,&copied) && copied==bytes;
}
uint64_t ImageBytes(){
    static const uint64_t bytes=[]()->uint64_t {
        const auto base=Base();IMAGE_DOS_HEADER dos{};IMAGE_NT_HEADERS64 nt{};
        if(!Read(base,&dos,sizeof(dos)) || dos.e_magic!=IMAGE_DOS_SIGNATURE || dos.e_lfanew<=0 || dos.e_lfanew>0x100000 ||
            base>UINT64_MAX-UINT(dos.e_lfanew) || !Read(base+UINT(dos.e_lfanew),&nt,sizeof(nt)) ||
            nt.Signature!=IMAGE_NT_SIGNATURE || nt.FileHeader.Machine!=IMAGE_FILE_MACHINE_AMD64 ||
            nt.OptionalHeader.Magic!=IMAGE_NT_OPTIONAL_HDR64_MAGIC || nt.OptionalHeader.SizeOfImage<=detail::ExecuteReturn ||
            nt.OptionalHeader.SizeOfImage>UINT64_MAX-base)return 0;
        return nt.OptionalHeader.SizeOfImage;
    }();return bytes;
}
bool UnwoundExecuteOrigin(uint64_t base,uintptr_t caller){
    const auto bytes=ImageBytes();
    if(!bytes || (caller>=base && uint64_t(caller)-base<bytes))return false;
    std::array<void*,detail::MaxUnwoundFrames> addresses{};
    const auto count=CaptureStackBackTrace(0,ULONG(addresses.size()),addresses.data(),nullptr);
    std::array<uint64_t,detail::MaxUnwoundFrames> frames{};
    for(USHORT i=0;i<count;++i)frames[i]=reinterpret_cast<uintptr_t>(addresses[i]);
    return detail::ExecuteOrigin(base,bytes,caller,{frames.data(),count});
}
uint64_t Registry(){uint64_t value{};const auto base=Base();return base && base<=UINT64_MAX-0x3BCDCB0 && Read(base+0x3BCDCA8,&value,8)?value:0;}
ComPtr<Owner> Recording(ID3D12CommandList* list){
    ComPtr<IUnknown> unknown;ComPtr<Owner> owner;UINT bytes=sizeof(IUnknown*);
    if(list && SUCCEEDED(list->GetPrivateData(ownerKey,&bytes,unknown.GetAddressOf())) && bytes==sizeof(IUnknown*) && unknown)
        unknown->QueryInterface(ownerKey,reinterpret_cast<void**>(owner.GetAddressOf()));
    return owner;
}
ComPtr<Token> LeaseToken(const Lease& lease){
    ComPtr<Token> token;if(lease.token)lease.token->QueryInterface(tokenType,reinterpret_cast<void**>(token.GetAddressOf()));return token;
}
bool SameLease(const Token& token,const Lease& lease){
    return token.active.load(std::memory_order_acquire) && token.nativeReset && token.generation==lease.generation &&
        token.context==lease.context && token.list==lease.list && token.registry==lease.registry;
}
bool NativeQueue(uint64_t registry,ID3D12CommandQueue* queue,uint64_t* output=nullptr){
    uint64_t expected{};
    const bool read=registry && registry<=UINT64_MAX-0x14104D8 && Read(registry+0x14104D0,&expected,8);
    if(output)*output=expected;
    return read && queue && registry==Registry() && expected==reinterpret_cast<uintptr_t>(queue) &&
        queue->GetDesc().Type==D3D12_COMMAND_LIST_TYPE_DIRECT && registry==Registry();
}
uint64_t SubmissionBits(const detail::SubmissionState& state){
    return uint64_t(state.pending)|(uint64_t(state.submitted)<<32)|(uint64_t(state.rejected)<<33);
}
bool Current(camera_upload::Context context,uint64_t list){
    uint64_t actualList{},state{},submitted{};UINT kind{};
    return context.context && context.context<=UINT64_MAX-0x640 &&
        Read(context.context+0x30,&actualList,8) && Read(context.context+0x60,&state,8) &&
        Read(context.context+0x68,&kind,4) && Read(context.context+0x638,&submitted,8) &&
        detail::ContextMatches(context,list,actualList,state,kind,submitted);
}
bool FirstOccurrence(UINT index,ID3D12CommandList* const* lists){
    for(UINT i=0;i<index;++i)if(lists[i]==lists[index])return false;return true;
}
}
bool Compatible(){
    static const bool value=[] {
        const auto base=Base();std::array<uint8_t,2048> bytes{};
        for(const auto& code:detail::Contract){
            if(!base || base>UINT64_MAX-code.rva || code.bytes>bytes.size() || !Read(base+code.rva,bytes.data(),code.bytes) ||
                detail::Fingerprint({bytes.data(),code.bytes})!=code.hash)return false;
        }
        return true;
    }();return value;
}
uint32_t LastFailure(){return failure;}
std::array<uint64_t,8> LastDetails(){return details;}
void Reset(ID3D12GraphicsCommandList* list,uintptr_t caller){
    if(!Enabled() || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return;
    try{
        std::lock_guard lock(mutex);failure=10;details={};
        if(auto previous=Recording(list))previous->token->active.store(false,std::memory_order_release);
        if(nextGeneration==UINT64_MAX)return;
        ComPtr<Token> token;token.Attach(new Token);token->generation=++nextGeneration;
        token->list=reinterpret_cast<uintptr_t>(list);token->registry=Registry();
        token->nativeReset=Compatible() && detail::Caller(Base(),caller,detail::ResetReturn);
        token->observed[6]=caller;token->observed[7]=token->generation;
        ComPtr<Owner> owner;owner.Attach(new Owner(std::move(token)));
        if(FAILED(list->SetPrivateDataInterface(ownerKey,owner.Get())))return;
        failure=0;
    }catch(...){failure=11;}
}
bool Capture(ID3D12GraphicsCommandList* list,camera_upload::Context context,Lease& output){
    output={};failure=1;details={};
    if(!Enabled() || !list || !context || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    try{
        failure=2;if(!Compatible())return false;
        std::lock_guard lock(mutex);failure=3;const auto owner=Recording(list);if(!owner)return false;
        auto& token=*owner->token.Get();details=token.observed;details[5]=SubmissionBits(token.submission);
        failure=4;if(!token.nativeReset)return false;
        failure=5;if(!token.active.load(std::memory_order_acquire) || !token.submission.Fresh())return false;
        failure=6;if((token.context && token.context!=context.context) || !Current(context,token.list))return false;
        failure=7;if(!token.registry || token.registry!=Registry())return false;
        failure=6;if(!Current(context,token.list))return false;
        token.context=context.context;output.token=owner->token;
        output.generation=token.generation;output.context=token.context;output.list=token.list;output.registry=token.registry;
        failure=0;return true;
    }catch(...){failure=11;return false;}
}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists,uintptr_t caller){
    if(!Enabled())return;failure=1;details={};if(!queue || !lists || !count)return;
    try{
        std::lock_guard lock(mutex);const auto base=Base();const bool compatible=Compatible();
        bool native=compatible && detail::Caller(base,caller,detail::ExecuteReturn),unwound=false;failure=0;
        for(UINT i=0;i<count;++i){
            if(!FirstOccurrence(i,lists))continue;const auto owner=Recording(lists[i]);if(!owner)continue;
            auto& token=*owner->token.Get();UINT occurrences{};for(UINT j=0;j<count;++j)occurrences+=lists[j]==lists[i];
            uint64_t expectedQueue{};const bool queueValid=NativeQueue(token.registry,queue,&expectedQueue);
            if(!native && !unwound && compatible && token.context && token.nativeReset && token.submission.Fresh() &&
                token.active.load(std::memory_order_acquire) && occurrences==1 && queueValid){
                unwound=true;native=UnwoundExecuteOrigin(base,caller);
            }
            const bool allowed=native && token.nativeReset && token.active.load(std::memory_order_acquire) && queueValid;
            const bool first=token.submission.Fresh();
            token.queue=queue;
            const bool accepted=token.submission.BeforeSubmit(occurrences,allowed);
            token.observed={caller,base && base<=UINT64_MAX-detail::ExecuteReturn?base+detail::ExecuteReturn:0,
                reinterpret_cast<uintptr_t>(queue),expectedQueue,occurrences,SubmissionBits(token.submission),token.observed[6],token.generation};
            if(!accepted){token.rejection=!first || occurrences!=1?9u:8u;
                if(token.context){failure=token.rejection;details=token.observed;}}
        }
    }catch(...){failure=11;}
}
bool ValidateRecording(const Lease& lease,ID3D12GraphicsCommandList* list,camera_upload::Context context){
    failure=1;details={};
    if(!Enabled() || !list || !context || !lease.token || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT ||
        reinterpret_cast<uintptr_t>(list)!=lease.list)return false;
    try{
        failure=2;if(!Compatible())return false;
        std::lock_guard lock(mutex);failure=3;const auto token=LeaseToken(lease);if(!token)return false;
        details=token->observed;details[5]=SubmissionBits(token->submission);
        failure=5;if(!SameLease(*token.Get(),lease) || !token->submission.Fresh())return false;
        failure=6;if(context.context!=lease.context || !Current(context,lease.list))return false;
        failure=7;if(!lease.registry || lease.registry!=Registry())return false;
        failure=6;if(!Current(context,lease.list))return false;
        failure=0;return true;
    }catch(...){failure=11;return false;}
}
bool ValidateSubmission(const Lease& lease,ID3D12CommandQueue* queue){
    failure=1;details={};if(!Enabled() || !lease.token || !queue)return false;
    try{
        std::lock_guard lock(mutex);failure=3;const auto token=LeaseToken(lease);if(!token)return false;
        details=token->observed;details[5]=SubmissionBits(token->submission);
        failure=5;if(!SameLease(*token.Get(),lease))return false;
        if(token->submission.rejected){failure=token->rejection?token->rejection:9;return false;}
        failure=8;if(token->queue && token->queue!=queue)return false;
        failure=12;if(!token->submission.Validating())return false;
        failure=8;if(!NativeQueue(lease.registry,queue))return false;
        failure=0;return true;
    }catch(...){failure=11;return false;}
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!Enabled() || !queue || !lists || !count)return;
    try{
        std::lock_guard lock(mutex);
        for(UINT i=0;i<count;++i){
            if(!FirstOccurrence(i,lists))continue;const auto owner=Recording(lists[i]);if(!owner)continue;
            auto& token=*owner->token.Get();
            // A concurrent successful Reset may already own a new generation.
            if(token.queue==queue && token.submission.pending)token.submission.Submitted();
        }
    }catch(...){failure=11;}
}
}
#endif
