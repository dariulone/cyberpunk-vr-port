#include "Render/NativeIaBinding.hpp"
#include "Render/NativeIndirectPolicy.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <intrin.h>
#include <utility>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The game-code contracts: rust/crates/cvr-render/src/code_tables.rs (through ReadProcessMemory).
// The image's size: rust/crates/cvr-render/src/image_size.rs.
// The frames, reads and leases: rust/crates/cvr-render/src/ia_binding.rs (the engine's memory through ReadProcessMemory,
// the walks through cvr-engine's unwinder, the thread's TLS array from GS).
extern "C" {
bool cvr_iab_native_compatible();
void cvr_iab_acquire(const cvr::stereo::geometry::NativeBuffer*,uint64_t,uint64_t,cvr::stereo::geometry::BufferLease*);
void cvr_iab_read_state(ID3D12GraphicsCommandList*,const cvr::stereo::geometry::BufferLease*,cvr::stereo::geometry::ReadState*);
void cvr_iab_bound_index(const D3D12_INDEX_BUFFER_VIEW*,cvr::stereo::geometry::BufferLease*);
void cvr_iab_bound_vertices(UINT,UINT,const D3D12_VERTEX_BUFFER_VIEW*,cvr::stereo::geometry::BufferLease*);
// The indirect binding (BoundIndirect, RevalidateIndirect) and its failure, the code contract.
uint32_t cvr_iab_last_failure();
bool cvr_iab_bound_indirect(ID3D12GraphicsCommandList*,ID3D12CommandSignature*,UINT,ID3D12Resource*,UINT64,ID3D12Resource*,UINT64,
    const cvr::stereo::native_recording::Lease*,bool,cvr::stereo::geometry::IndirectBinding*);
bool cvr_iab_revalidate_indirect(ID3D12GraphicsCommandList*,const cvr::stereo::native_recording::Lease*,const cvr::stereo::geometry::IndirectBinding*,bool);
}
static_assert(sizeof(cvr::stereo::geometry::IndirectBinding)==392 && offsetof(cvr::stereo::geometry::IndirectBinding,context)==88 &&
    offsetof(cvr::stereo::geometry::IndirectBinding,recording)==112 && offsetof(cvr::stereo::geometry::IndirectBinding,rawRecord)==164 &&
    offsetof(cvr::stereo::geometry::IndirectBinding,recordAddress)==344 && offsetof(cvr::stereo::geometry::IndirectBinding,callerRva)==368 &&
    offsetof(cvr::stereo::geometry::IndirectBinding,fullBacking)==384,"mirrored by ia_binding.rs");
static_assert(sizeof(cvr::stereo::geometry::BufferLease)==80 && sizeof(cvr::stereo::geometry::NativeBuffer)==48 &&
    sizeof(cvr::stereo::geometry::IndirectFrame)==24 && sizeof(cvr::stereo::camera_upload::Context)==24,"mirrored by ia_binding.rs");
#endif
#endif

namespace cvr::stereo::geometry {
namespace {
#if !CVR_RUST_VRIK
thread_local IndirectBindingFailure indirectFailure{};
constexpr GUID initialStateKey{0xb7b50d38,0xf497,0x4fc3,{0xa3,0xc1,0x62,0x40,0x21,0x37,0x63,0x20}};
#endif
#if CVR_RUST_VRIK
}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool NativeIaCompatible(){return cvr_iab_native_compatible();}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
BufferLease AcquireBuffer(const NativeBuffer& native,uint64_t address,uint64_t bytes) {
    BufferLease result;cvr_iab_acquire(&native,address,bytes,&result);return result;
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
ReadState ReadStateAtDraw(ID3D12GraphicsCommandList* list,const BufferLease& source) {
    ReadState state;cvr_iab_read_state(list,&source,&state);return state;
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
BufferLease BoundIndex(const D3D12_INDEX_BUFFER_VIEW& view){BufferLease result;cvr_iab_bound_index(&view,&result);return result;}
std::array<BufferLease,16> BoundVertices(UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW* views) {
    std::array<BufferLease,16> result{};cvr_iab_bound_vertices(start,count,views,result.data());return result;
}
#endif
#else
bool Read(void*,uint64_t address,void* output,size_t bytes) {
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
bool Hash(uint64_t rva,size_t bytes,uint64_t expected) {
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
#if CVR_RUST_VRIK
    return cvr_code_hash(base,rva,bytes,expected,2048);
#else
    std::array<uint8_t,2048> data{};if(!base || base>UINT64_MAX-rva || bytes>data.size() || !Read(nullptr,base+rva,data.data(),bytes))return false;
    uint64_t value=14695981039346656037ull;for(size_t i=0;i<bytes;++i)value=(value^data[i])*1099511628211ull;return value==expected;
#endif
}
bool Native(bool index,UINT start,UINT count,std::array<NativeBuffer,16>& output) {
    if(!NativeIaCompatible())return false;
    __try {
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));CONTEXT context{};RtlCaptureContext(&context);
        ULONG_PTR low{},high{};GetCurrentThreadStackLimits(&low,&high);
        for(UINT depth=0;depth<32 && context.Rsp>=low && context.Rsp<=high-8;++depth) {
            if(context.Rip>=game && context.Rip-game<=UINT32_MAX) {
                IaFrame frame{context.Rip-game,context.Rsp,context.Rbx,context.Rbp,context.Rdi,context.Rsi,context.R12,context.R14,context.R15};
                if(DecodeIa(frame,index,start,count,Read,nullptr,output))return true;
            }
            DWORD64 image{};const auto entry=RtlLookupFunctionEntry(context.Rip,&image,nullptr);
            if(entry){PVOID handler{};DWORD64 establisher{};RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,context.Rip,entry,&context,&handler,&establisher,nullptr);}
            else {if(!Read(nullptr,context.Rsp,&context.Rip,8))return false;context.Rsp+=8;}
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){}
    return false;
}
bool CurrentIndirectContext(camera_upload::Context& out){
    uint64_t tls{},thread{},context{},state{},list{};uint8_t initialized{};
    tls=__readgsqword(0x58);
    if(!Read(nullptr,tls,&thread,8) || !thread || thread>UINT64_MAX-0x1A0 || !Read(nullptr,thread+0x14,&initialized,1) || !initialized ||
        !Read(nullptr,thread+0x198,&context,8) || !context || context>UINT64_MAX-0x68 ||
        !Read(nullptr,context+0x60,&state,8) || !Read(nullptr,context+0x30,&list,8))return false;
    out={context,state,list};return bool(out);
}
uint64_t NativeImageBytes(){
#if CVR_RUST_VRIK
    static const uint64_t bytes=cvr_image_bytes_above(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)),0x413AE0);return bytes;
#else
    static const uint64_t bytes=[]()->uint64_t{
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));IMAGE_DOS_HEADER dos{};IMAGE_NT_HEADERS64 nt{};
        if(!Read(nullptr,base,&dos,sizeof(dos)) || dos.e_magic!=IMAGE_DOS_SIGNATURE || dos.e_lfanew<=0 || dos.e_lfanew>0x100000 ||
            base>UINT64_MAX-UINT(dos.e_lfanew) || !Read(nullptr,base+UINT(dos.e_lfanew),&nt,sizeof(nt)) ||
            nt.Signature!=IMAGE_NT_SIGNATURE || nt.FileHeader.Machine!=IMAGE_FILE_MACHINE_AMD64 ||
            nt.OptionalHeader.Magic!=IMAGE_NT_OPTIONAL_HDR64_MAGIC || nt.OptionalHeader.SizeOfImage<=0x413AE0 ||
            nt.OptionalHeader.SizeOfImage>UINT64_MAX-base)return 0;
        return nt.OptionalHeader.SizeOfImage;
    }();return bytes;
#endif
}
bool IndirectOrigin(uint64_t game,IndirectFrame& output){
    const auto imageBytes=NativeImageBytes();if(!game || !imageBytes)return false;
    __try{
        CONTEXT context{};RtlCaptureContext(&context);ULONG_PTR low{},high{};GetCurrentThreadStackLimits(&low,&high);
        for(UINT depth=0;depth<32 && high>=8 && context.Rsp>=low && context.Rsp<=high-8;++depth){
            if(context.Rip>=game && context.Rip-game<imageBytes){
                const auto rva=context.Rip-game;
                if(rva!=0x342B11 && rva!=0xD543BB && rva!=0x413AE0)return false;
                output={rva,context.Rbx,context.Rdi};return true;
            }
            DWORD64 image{};const auto entry=RtlLookupFunctionEntry(context.Rip,&image,nullptr);
            if(entry){PVOID handler{};DWORD64 establisher{};RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,context.Rip,entry,&context,&handler,&establisher,nullptr);}
            else {if(!Read(nullptr,context.Rsp,&context.Rip,8))return false;context.Rsp+=8;}
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){}
    return false;
}
bool IndirectState(ReadState state){
    return (state.status==5 && !state.value) || ((state.status==1 || state.status==2 || state.status==8 || state.status==9) &&
        (state.value&D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT) && !(state.value&~UINT(D3D12_RESOURCE_STATE_GENERIC_READ)));
}
bool SameRecording(const native_recording::Lease& a,const native_recording::Lease& b){
    return a.token && a.token.Get()==b.token.Get() && a.generation && a.generation==b.generation &&
        a.context==b.context && a.list==b.list && a.registry==b.registry;
}
bool SourceDevice(ID3D12GraphicsCommandList* list,ID3D12DeviceChild* object){
    if(!list || !object)return false;Microsoft::WRL::ComPtr<ID3D12Device> expected,actual;
    return SUCCEEDED(list->GetDevice(IID_PPV_ARGS(&expected))) && SUCCEEDED(object->GetDevice(IID_PPV_ARGS(&actual))) && expected==actual;
}
bool DescribeIndirectResource(ID3D12GraphicsCommandList* list,const IndirectNativeRecord& native,ID3D12Resource* actual,
    UINT64 offset,UINT64 bytes,BufferLease& lease,bool& fullBacking){
    if(!actual || native.arguments.resource!=reinterpret_cast<uintptr_t>(actual) || !SourceDevice(list,actual))return false;
    BufferLease pending;pending.resource=actual;const auto desc=actual->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || FAILED(actual->GetHeapProperties(&heap,&flags)))return false;
    pending.gpu=actual->GetGPUVirtualAddress();pending.bytes=desc.Width;pending.heapType=UINT(heap.Type);pending.flags=UINT(desc.Flags);pending.native=native.arguments;
    if(!indirect_binding_detail::SourceRange(native.arguments,pending.gpu,pending.bytes,offset,bytes,fullBacking))return false;
    lease=std::move(pending);return true;
}
}
bool NativeIaCompatible() {
    static const bool result=Hash(0x339150,0x24D,0x16AF288F10E7FCCFull) &&
        Hash(0x342554,0x52D,0x0E7580FE6E751E01ull) && Hash(0x339DB0,0xDC,0xB9D1AE5DAB6AFEA5ull);
    return result;
}
void BufferCreated(ID3D12Resource* resource,D3D12_RESOURCE_STATES state) {
    if(!resource)return;const auto d=resource->GetDesc();
    if(d.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || (d.Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS))return;
    const uint32_t value=UINT(state);resource->SetPrivateData(initialStateKey,sizeof(value),&value);
}
BufferLease AcquireBuffer(const NativeBuffer& native,uint64_t address,uint64_t bytes) {
    BufferLease result;
    if(!native.resource || !native.gpu || !bytes || address<native.gpu || address-native.gpu>native.bytes || bytes>native.bytes-(address-native.gpu))return result;
    Microsoft::WRL::ComPtr<ID3D12Resource> resource=reinterpret_cast<ID3D12Resource*>(native.resource);
    const auto d=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(d.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || address<gpu || address-gpu>d.Width || bytes>d.Width-(address-gpu) ||
       FAILED(resource->GetHeapProperties(&heap,&flags)))return result;
    result.resource=std::move(resource);result.gpu=gpu;result.bytes=d.Width;result.heapType=UINT(heap.Type);result.flags=UINT(d.Flags);result.native=native;return result;
}
ReadState ReadStateAtDraw(ID3D12GraphicsCommandList* list,const BufferLease& source) {
    if(!list || !source.resource)return {0,3,source.native.kind};
    if(source.heapType==D3D12_HEAP_TYPE_UPLOAD)return {D3D12_RESOURCE_STATE_GENERIC_READ,1,source.native.kind};
    if(source.heapType!=D3D12_HEAP_TYPE_DEFAULT)return {0,6,source.native.kind};
    static const bool compatible=NativeIaCompatible() && Hash(0x337DA0,0x180,0xB36B2D868965D718ull) &&
        Hash(0x338390,0xDF,0x262EAAB2CEC6F5E0ull) && Hash(0x1EAF30,0x136,0xE80917A196B71137ull) &&
        Hash(0x3445AC,0x5A2,0x612F7F49D216A154ull);
    if(!compatible)return {0,7,source.native.kind};
    auto state=ReadNativeState(source.native,reinterpret_cast<uintptr_t>(list),Read,nullptr);
    // Native class 1 bypasses both state-map update paths. Direct construction
    // chooses COMMON/GENERIC_READ; both fixed-read pool constructors use GENERIC_READ.
    // Early pools predate our API hooks, so their fallback also checks the full
    // constructor bodies, in addition to the live native binding identity above.
    if(state.status==5 && source.native.kind==1 && !(source.flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)) {
        uint32_t initial=UINT_MAX;UINT bytes=sizeof(initial);
        const auto receipt=source.resource->GetPrivateData(initialStateKey,&bytes,&initial);
        if(SUCCEEDED(receipt)) {
            if(bytes==sizeof(initial) && (initial==D3D12_RESOURCE_STATE_COMMON || initial==D3D12_RESOURCE_STATE_GENERIC_READ))return {D3D12_RESOURCE_STATE_GENERIC_READ,8,1};
        } else if(receipt==DXGI_ERROR_NOT_FOUND) {
            static const bool pools=Hash(0x342F48,0x483,0x232E9A0D5DBE1C20ull) && Hash(0x22F2D00,0x384,0x09BF1A44FA19923Full);
            if(pools)return {D3D12_RESOURCE_STATE_GENERIC_READ,9,1};
        }
    }
    return state;
}
BufferLease BoundIndex(const D3D12_INDEX_BUFFER_VIEW& view) {
    std::array<NativeBuffer,16> sources;
    if(!view.BufferLocation || (view.Format!=DXGI_FORMAT_R16_UINT && view.Format!=DXGI_FORMAT_R32_UINT) || !Native(true,0,1,sources))return {};
    return AcquireBuffer(sources[0],view.BufferLocation,view.SizeInBytes);
}
std::array<BufferLease,16> BoundVertices(UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW* views) {
    std::array<BufferLease,16> result{};std::array<NativeBuffer,16> sources;
    if(!views || !count || start>16 || count>16-start || !Native(false,start,count,sources))return result;
    for(UINT i=0;i<count;++i)result[i]=AcquireBuffer(sources[i],views[i].BufferLocation,views[i].SizeInBytes);
    return result;
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
IndirectBindingFailure LastIndirectBindingFailure(){return IndirectBindingFailure(cvr_iab_last_failure());}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool BoundIndirect(ID3D12GraphicsCommandList* list,ID3D12CommandSignature* signature,UINT maximum,ID3D12Resource* arguments,UINT64 offset,
    ID3D12Resource* countBuffer,UINT64 countOffset,const native_recording::Lease& recording,IndirectBinding& output) {
    IndirectBinding pending;
    if(!cvr_iab_bound_indirect(list,signature,maximum,arguments,offset,countBuffer,countOffset,&recording,probe::internalCommands,&pending))return false;
    output=std::move(pending);return true;
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool RevalidateIndirect(ID3D12GraphicsCommandList* list,const native_recording::Lease& recording,const IndirectBinding& saved) {
    return cvr_iab_revalidate_indirect(list,&recording,&saved,probe::internalCommands);
}
#endif
#else
bool NativeIndirectCompatible(){
    static const bool result=[](){for(const auto& code:indirect_binding_detail::Contract)if(!Hash(code.rva,code.bytes,code.hash))return false;return true;}();
    return result;
}
IndirectBindingFailure LastIndirectBindingFailure(){return indirectFailure;}
bool BoundIndirect(ID3D12GraphicsCommandList* list,ID3D12CommandSignature* signature,UINT maximum,ID3D12Resource* arguments,UINT64 offset,
    ID3D12Resource* countBuffer,UINT64 countOffset,const native_recording::Lease& recording,IndirectBinding& output) try {
    indirectFailure=IndirectBindingFailure::Arguments;
    if(probe::internalCommands || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !signature || !arguments ||
        maximum!=1 || countBuffer || countOffset || offset%4)return false;
    indirectFailure=IndirectBindingFailure::Fingerprints;if(!NativeIndirectCompatible())return false;
    indirectFailure=IndirectBindingFailure::Device;if(!SourceDevice(list,signature))return false;
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));IndirectFrame frame;camera_upload::Context current;
    indirectFailure=IndirectBindingFailure::Origin;if(!IndirectOrigin(game,frame))return false;
    indirectFailure=IndirectBindingFailure::CurrentContext;if(!CurrentIndirectContext(current))return false;
    indirectFailure=IndirectBindingFailure::Recording;if(!native_recording::ValidateRecording(recording,list,current))return false;
    const IndirectCall call{reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(signature),reinterpret_cast<uintptr_t>(arguments),
        offset,reinterpret_cast<uintptr_t>(countBuffer),countOffset,maximum};
    IndirectNativeRecord native;
    indirectFailure=IndirectBindingFailure::Decode;
    if(!indirect_binding_detail::Decode(game,frame,current,call,Read,nullptr,native) || native.arguments.registry!=recording.registry)return false;
    const auto receipt=indirect_policy::Describe(signature);
    indirectFailure=IndirectBindingFailure::Signature;
    if(indirect_policy::detail::DrawKind(receipt)!=native.signatureKind || indirect_policy::detail::PureDrawStride(receipt)!=native.signatureStride)return false;
    IndirectBinding pending;pending.signature=signature;pending.context=current;pending.recording=recording;
    pending.argumentOffset=offset;pending.argumentBytes=UINT64(maximum)*native.signatureStride;
    indirectFailure=IndirectBindingFailure::ResourceRange;
    if(!DescribeIndirectResource(list,native,arguments,offset,pending.argumentBytes,pending.arguments,pending.fullBacking))return false;
    indirectFailure=IndirectBindingFailure::State;
    pending.state=ReadStateAtDraw(list,pending.arguments);if(!IndirectState(pending.state))return false;
    pending.rawRecord=native.rawRecord;pending.recordAddress=native.recordAddress;pending.callerRva=native.callerRva;
    pending.maxCount=maximum;pending.signatureKind=native.signatureKind;pending.signatureStride=native.signatureStride;
    if(!RevalidateIndirect(list,recording,pending))return false;output=std::move(pending);indirectFailure=IndirectBindingFailure::None;return true;
}catch(...){indirectFailure=IndirectBindingFailure::Exception;return false;}
bool RevalidateIndirect(ID3D12GraphicsCommandList* list,const native_recording::Lease& recording,const IndirectBinding& saved) try {
    indirectFailure=IndirectBindingFailure::Arguments;
    if(probe::internalCommands || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !saved.arguments.resource || !saved.signature)return false;
    indirectFailure=IndirectBindingFailure::Fingerprints;if(!NativeIndirectCompatible())return false;
    indirectFailure=IndirectBindingFailure::Recording;if(!SameRecording(recording,saved.recording))return false;
    indirectFailure=IndirectBindingFailure::Device;if(!SourceDevice(list,saved.signature.Get()))return false;
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));IndirectFrame frame;camera_upload::Context current;
    indirectFailure=IndirectBindingFailure::Origin;if(!IndirectOrigin(game,frame))return false;
    indirectFailure=IndirectBindingFailure::CurrentContext;if(!CurrentIndirectContext(current) || current!=saved.context)return false;
    indirectFailure=IndirectBindingFailure::Origin;if(!indirect_binding_detail::MatchesOrigin(frame,saved.callerRva,current,recording.registry))return false;
    indirectFailure=IndirectBindingFailure::Recording;if(!native_recording::ValidateRecording(recording,list,current))return false;
    const IndirectCall call{reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(saved.signature.Get()),reinterpret_cast<uintptr_t>(saved.arguments.resource.Get()),
        saved.argumentOffset,0,0,saved.maxCount};
    IndirectNativeRecord fresh,expected;
    expected.arguments=saved.arguments.native;expected.context=saved.context;expected.rawRecord=saved.rawRecord;expected.recordAddress=saved.recordAddress;
    expected.signature=call.signature;expected.callerRva=saved.callerRva;expected.signatureKind=saved.signatureKind;expected.signatureStride=saved.signatureStride;
    indirectFailure=IndirectBindingFailure::Decode;
    if(!indirect_binding_detail::Decode(game,frame,current,call,Read,nullptr,fresh) || !indirect_binding_detail::SameRecord(fresh,expected) || fresh.arguments.registry!=recording.registry ||
        saved.argumentBytes!=UINT64(saved.maxCount)*saved.signatureStride)return false;
    const auto receipt=indirect_policy::Describe(saved.signature.Get());
    indirectFailure=IndirectBindingFailure::Signature;
    if(indirect_policy::detail::DrawKind(receipt)!=saved.signatureKind || indirect_policy::detail::PureDrawStride(receipt)!=saved.signatureStride)return false;
    BufferLease resource;bool fullBacking{};
    indirectFailure=IndirectBindingFailure::ResourceRange;
    if(!DescribeIndirectResource(list,fresh,saved.arguments.resource.Get(),saved.argumentOffset,saved.argumentBytes,resource,fullBacking) ||
        resource.gpu!=saved.arguments.gpu || resource.bytes!=saved.arguments.bytes || resource.heapType!=saved.arguments.heapType || resource.flags!=saved.arguments.flags || fullBacking!=saved.fullBacking)return false;
    const auto state=ReadStateAtDraw(list,resource);
    indirectFailure=IndirectBindingFailure::State;
    if(!IndirectState(state) || state.value!=saved.state.value || state.status!=saved.state.status || state.kind!=saved.state.kind)return false;
    camera_upload::Context finalContext;IndirectNativeRecord finalRecord;IndirectFrame finalFrame;
    indirectFailure=IndirectBindingFailure::Revalidation;
    const bool valid=IndirectOrigin(game,finalFrame) && CurrentIndirectContext(finalContext) && finalContext==current &&
        indirect_binding_detail::MatchesOrigin(finalFrame,saved.callerRva,finalContext,recording.registry) &&
        native_recording::ValidateRecording(recording,list,finalContext) &&
        indirect_binding_detail::Decode(game,finalFrame,finalContext,call,Read,nullptr,finalRecord) && indirect_binding_detail::SameRecord(finalRecord,fresh);
    if(valid)indirectFailure=IndirectBindingFailure::None;return valid;
}catch(...){indirectFailure=IndirectBindingFailure::Exception;return false;}
#endif
}
