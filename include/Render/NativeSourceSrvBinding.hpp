#pragma once
#include "Render/NativeSourceCbvDetails.hpp"
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>

namespace cvr::stereo::source_srv {
inline constexpr uint32_t ShaderRegister=5,StageIndex=1,RootParameter=23,TableOffset=5;
inline constexpr bool TrackedRegister(uint32_t value){return value>=4 && value<=6;}
inline constexpr uint64_t MaxHandles=0x100000;
inline constexpr size_t MaxXstateBytes=16384;
enum class Route:uint32_t {Individual,Cached};
enum class Status:uint32_t {
    Ignored,Captured,UnsupportedExecutable,MissingContext,MissingObservation,
    InvalidBinding,DescriptorMismatch,Changed,Capacity,CoverageLost,Failure,Candidate
};
struct Code {uint32_t rva,bytes;uint64_t fnv;};
inline constexpr std::array<Code,9> Contract{{
    {0x338FC0,400,0x51B6659A1B7482CAull},{0x338ED4,224,0x7C30C7D822B3C01Dull},
    {0x339E8C,147,0x28D70ABB4E1F3623ull},{0x3393A0,103,0x892D112D7E8F25F9ull},
    {0x3364A8,791,0xA8C2FE51FFB12DA9ull},{0x337230,51,0x0396F3E29AEB7926ull},
    {0x337DA0,384,0xB36B2D868965D718ull},{0x338390,223,0x262EAAB2CEC6F5E0ull},
    {0x1EAF30,310,0xE80917A196B71137ull}
}};
struct Slot {
    uint64_t layout{},table{},entry{},cpu{};
    uint32_t group{},first{},count{},start{},index{},offset{},parameter{},cachedIndex{UINT32_MAX};
    std::array<uint8_t,16> raw{};
    bool operator==(const Slot&)const=default;
};
struct CandidateInput {
    Route route{};uint32_t stage{},shaderRegister{},handle{},bundle{},bundleIndex{};
    uint64_t handlePointer{},bundleEntry{};std::array<uint8_t,8> bundleRaw{};
    bool operator==(const CandidateInput&)const=default;
};
struct alignas(16) ThunkFrame {
    uint64_t rax{},rcx{},rdx{},rbx{},rbp{},rsi{},rdi{},r8{},r9{},r10{},r11{},r12{},r13{},r14{},r15{},flags{};
    uint64_t stack{},caller{},xstateAddress{},xstateMask{},xstateBytes{},xstateMode{};
    alignas(16) std::array<uint8_t,MaxXstateBytes+63> xstate{};
};
static_assert(offsetof(ThunkFrame,flags)==0x78 && offsetof(ThunkFrame,stack)==0x80 &&
    offsetof(ThunkFrame,xstate)==0xB0 && sizeof(ThunkFrame)==0x40F0);
namespace detail {
template<class T,size_t N>T Field(const std::array<uint8_t,N>& bytes,size_t offset)noexcept {
    T value{};std::memcpy(&value,bytes.data()+offset,sizeof(value));return value;
}
inline bool Read(geometry::MemoryReader read,void* user,uint64_t address,void* output,size_t bytes){
    return read && address && address<=UINT64_MAX-bytes && read(user,address,output,bytes);
}
template<size_t N>bool Stable(geometry::MemoryReader read,void* user,uint64_t address,std::array<uint8_t,N>& bytes){
    std::array<uint8_t,N> again{};
    return Read(read,user,address,bytes.data(),N) && Read(read,user,address,again.data(),N) && bytes==again;
}
#if CVR_RUST_VRIK
// The rules below: rust/crates/cvr-render/src/source_srv.rs (tools/single_pass_tests/source_srv_binding_test runs its
// cases against them in its Rust variant).
static_assert(sizeof(Slot)==80 && sizeof(CandidateInput)==48 && offsetof(ThunkFrame,r8)==56,"mirrored by source_srv.rs");
extern "C" {
bool cvr_srv_relevant(uint32_t,const ThunkFrame*);
bool cvr_srv_decode_candidate(uint32_t,const ThunkFrame*,uint64_t,geometry::MemoryReader,void*,CandidateInput*,uint32_t);
bool cvr_srv_read_slot(geometry::MemoryReader,void*,uint64_t,Slot*,uint32_t);
bool cvr_srv_matches_mode(const CandidateInput*,const Slot*);
bool cvr_srv_cached_group(geometry::MemoryReader,void*,const Slot*);
bool cvr_srv_read_record(geometry::MemoryReader,void*,uint64_t,uint64_t,uint32_t,std::array<uint8_t,0xB0>*,uint64_t*,geometry::NativeBuffer*);
bool cvr_srv_view_range(const geometry::NativeBuffer*,uint64_t,uint64_t,const D3D12_SHADER_RESOURCE_VIEW_DESC*,uint64_t*,uint64_t*,uint32_t);
bool cvr_srv_valid_xstate(uint64_t,uint64_t,uint64_t);
}
#endif
#if CVR_RUST_VRIK
inline bool Relevant(Route route,const ThunkFrame& frame)noexcept{return cvr_srv_relevant(uint32_t(route),&frame);}
#else
inline bool Relevant(Route route,const ThunkFrame& frame)noexcept {
    return route==Route::Individual?TrackedRegister(uint32_t(frame.rdx)) && uint8_t(frame.r8)==StageIndex:
        route==Route::Cached && uint8_t(frame.rdx)==StageIndex;
}
#endif
#if CVR_RUST_VRIK
inline bool DecodeCandidate(Route route,const ThunkFrame& frame,uint64_t registry,
    geometry::MemoryReader read,void* user,CandidateInput& output,uint32_t shaderRegister=ShaderRegister){
    return cvr_srv_decode_candidate(uint32_t(route),&frame,registry,read,user,&output,shaderRegister);
}
#else
inline bool DecodeCandidate(Route route,const ThunkFrame& frame,uint64_t registry,
    geometry::MemoryReader read,void* user,CandidateInput& output,uint32_t shaderRegister=ShaderRegister){
    output={};if(!Relevant(route,frame) || !TrackedRegister(shaderRegister) ||
        (route==Route::Individual && uint32_t(frame.rdx)!=shaderRegister))return false;
    CandidateInput next;next.route=route;next.stage=StageIndex;next.shaderRegister=shaderRegister;
    if(route==Route::Individual){
        std::array<uint8_t,4> bytes{};next.handlePointer=frame.rcx;
        if(!Stable(read,user,next.handlePointer,bytes))return false;
        next.handle=Field<uint32_t>(bytes,0);
    }else{
        next.bundle=uint32_t(frame.rcx);
        if(!registry || !next.bundle || next.bundle>MaxHandles)return false;
        const uint64_t offset=0xB50B00+uint64_t(next.bundle-1)*0x44+shaderRegister*8;
        if(registry>UINT64_MAX-offset-8)return false;
        next.bundleEntry=registry+offset;next.bundleIndex=(next.bundle-1)*8;
        if(!Stable(read,user,next.bundleEntry,next.bundleRaw) || next.bundleRaw[4]!=2)return false;
        next.handle=Field<uint32_t>(next.bundleRaw,0);
    }
    if(!next.handle || next.handle>MaxHandles)return false;output=next;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool ReadSlot(geometry::MemoryReader read,void* user,uint64_t state,Slot& output,uint32_t shaderRegister=ShaderRegister){
    return cvr_srv_read_slot(read,user,state,&output,shaderRegister);
}
#else
inline bool ReadSlot(geometry::MemoryReader read,void* user,uint64_t state,Slot& output,uint32_t shaderRegister=ShaderRegister){
    output={};if(!TrackedRegister(shaderRegister) || !state || state>UINT64_MAX-0x180)return false;
    Slot next;
    if(!Read(read,user,state+0x68,&next.layout,8) || !next.layout || next.layout>UINT64_MAX-0x5D0 ||
        !Read(read,user,state+0x28,&next.table,8))return false;
    uint8_t group{};
    if(!Read(read,user,next.layout+0x4C3+StageIndex*0x100+shaderRegister*2,&group,1) || group>=64)return false;
    next.group=group;next.entry=next.layout+0x38+uint64_t(group)*16;
    if(!Stable(read,user,next.entry,next.raw) || Field<uint32_t>(next.raw,0)!=0 || next.raw[13]==2)return false;
    next.first=Field<uint16_t>(next.raw,8);next.count=Field<uint16_t>(next.raw,10);next.start=Field<uint32_t>(next.raw,4);
    next.parameter=next.raw[14];
    if(next.first!=0 || next.count!=8 || next.parameter!=RootParameter || next.start>8192-next.count)return false;
    next.offset=shaderRegister-next.first;next.index=next.start+next.offset;
    if(!Read(read,user,state+0x80+uint64_t(group)*4,&next.cachedIndex,4))return false;
    if(next.cachedIndex==UINT32_MAX){
        if(!next.table || next.table>UINT64_MAX-uint64_t(next.index)*8-8 ||
            !Read(read,user,next.table+uint64_t(next.index)*8,&next.cpu,8) || !next.cpu)return false;
    }else if(next.cachedIndex>INT32_MAX)return false;
    output=next;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool MatchesMode(const CandidateInput& candidate,const Slot& slot)noexcept{return cvr_srv_matches_mode(&candidate,&slot);}
#else
inline bool MatchesMode(const CandidateInput& candidate,const Slot& slot)noexcept {
    return candidate.route==Route::Individual?slot.cachedIndex==UINT32_MAX:
        candidate.route==Route::Cached && slot.cachedIndex==candidate.bundleIndex;
}
#endif
#if CVR_RUST_VRIK
inline bool CachedGroup(geometry::MemoryReader read,void* user,const Slot& slot){return cvr_srv_cached_group(read,user,&slot);}
#else
inline bool CachedGroup(geometry::MemoryReader read,void* user,const Slot& slot){
    uint8_t first{};return Read(read,user,slot.layout+0x4C3+StageIndex*0x100,&first,1) && first==slot.group;
}
#endif
#if CVR_RUST_VRIK
inline bool ReadRecord(geometry::MemoryReader read,void* user,uint64_t registry,uint64_t context,
    uint32_t handle,std::array<uint8_t,0xB0>& raw,uint64_t& address,geometry::NativeBuffer& native){
    return cvr_srv_read_record(read,user,registry,context,handle,&raw,&address,&native);
}
#else
inline bool ReadRecord(geometry::MemoryReader read,void* user,uint64_t registry,uint64_t context,
    uint32_t handle,std::array<uint8_t,0xB0>& raw,uint64_t& address,geometry::NativeBuffer& native){
    raw={};address=0;native={};if(!registry || !context || !handle || handle>MaxHandles)return false;
    const uint64_t offset=0x5C0AF8+uint64_t(handle-1)*0xB0;
    if(registry>UINT64_MAX-offset-raw.size())return false;address=registry+offset;
    if(!Stable(read,user,address,raw))return false;
    native={Field<uint64_t>(raw,0x10),Field<uint64_t>(raw,8),Field<uint32_t>(raw,0),registry,context,handle,uint32_t(raw[6]>>4)};
    return native.kind!=4 && native.resource && native.gpu && native.bytes && native.gpu<=UINT64_MAX-native.bytes;
}
#endif
#if CVR_RUST_VRIK
inline bool ViewRange(const geometry::NativeBuffer& native,uint64_t gpu,uint64_t backingBytes,
    const D3D12_SHADER_RESOURCE_VIEW_DESC& view,uint64_t& offset,uint64_t& bytes,uint32_t shaderRegister=ShaderRegister)noexcept{
    return cvr_srv_view_range(&native,gpu,backingBytes,&view,&offset,&bytes,shaderRegister);
}
#else
inline bool ViewRange(const geometry::NativeBuffer& native,uint64_t gpu,uint64_t backingBytes,
    const D3D12_SHADER_RESOURCE_VIEW_DESC& view,uint64_t& offset,uint64_t& bytes,uint32_t shaderRegister=ShaderRegister)noexcept {
    offset=bytes=0;
    const uint64_t stride=shaderRegister==4?4:shaderRegister==5?128:64;
    if(!TrackedRegister(shaderRegister) || !gpu || !backingBytes || gpu>UINT64_MAX-backingBytes || view.ViewDimension!=D3D12_SRV_DIMENSION_BUFFER ||
        view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING || !view.Buffer.NumElements ||
        view.Buffer.FirstElement>UINT64_MAX/stride)return false;
    if(shaderRegister==4){if(view.Format!=DXGI_FORMAT_R32_TYPELESS || view.Buffer.Flags!=D3D12_BUFFER_SRV_FLAG_RAW || view.Buffer.StructureByteStride)return false;}
    else if(view.Format!=DXGI_FORMAT_UNKNOWN || view.Buffer.Flags || view.Buffer.StructureByteStride!=stride)return false;
    const auto begin=view.Buffer.FirstElement*stride,span=uint64_t(view.Buffer.NumElements)*stride;
    if(begin>backingBytes || span>backingBytes-begin || native.gpu<gpu || native.gpu-gpu>backingBytes ||
        native.bytes>backingBytes-(native.gpu-gpu) || gpu+begin<native.gpu || gpu+begin-native.gpu>native.bytes ||
        span>native.bytes-(gpu+begin-native.gpu))return false;
    offset=begin;bytes=span;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool ValidXstate(uint64_t enabled,uint64_t supported,uint64_t bytes)noexcept{return cvr_srv_valid_xstate(enabled,supported,bytes);}
#else
inline bool ValidXstate(uint64_t enabled,uint64_t supported,uint64_t bytes)noexcept {
    return (enabled&3)==3 && !(enabled&~supported) && bytes>=576 && bytes<=MaxXstateBytes;
}
#endif
}
struct SourceSrvBacking {
    descriptors::SrvBinding binding;
    native_recording::Lease recording;
    geometry::NativeBuffer native;
    geometry::ReadState state;
    camera_upload::Context context;
    Slot slot;
    std::array<uint8_t,0xB0> rawRecord{};
    source_cbv_details::Allocation allocation;
    source_cbv_details::Allocator allocator;
    uint64_t gpu{},backingBytes{},sourceOffset{},viewBytes{},recordAddress{},cpuDescriptor{},serial{};
    uint32_t stage{},shaderRegister{},route{},setterRva{},lowType{},heapType{},flags{};
    // Current binding/MA metadata only. No allocation pin or copy/state grant.
};
void PrepareCodeGuards(uintptr_t gameBase)noexcept;
bool Compatible()noexcept;
// Configure once before installing/enabling either native entry observer.
bool ConfigureXstate()noexcept;
Status ReadAtSource(ID3D12GraphicsCommandList*,uint64_t root,UINT parameter,UINT offset,
    const descriptors::SrvBinding&,const native_recording::Lease&,SourceSrvBacking&)noexcept;
// Revalidates exact source identity; returns fresh state separately in current.
// A caller must explicitly validate its own expected accumulated state mask.
Status RevalidateAtSource(ID3D12GraphicsCommandList*,uint64_t root,UINT parameter,UINT offset,
    const SourceSrvBacking&,SourceSrvBacking& current)noexcept;
void Reset(ID3D12GraphicsCommandList*)noexcept;
void Clear()noexcept;
uint32_t LastStatus()noexcept;
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SourceSrvBindingEnable;
extern void* CvrSourceSrvIndividualOriginal;
extern void* CvrSourceSrvCachedOriginal;
void CvrSourceSrvIndividualDetour();
void CvrSourceSrvCachedDetour();
}
