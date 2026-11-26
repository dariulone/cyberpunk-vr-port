#pragma once
#include "Render/NativeIaBinding.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <vector>

namespace cvr::stereo::source_cbv {
enum class Status:uint32_t {
    Ignored,Registered,Transient,Created,UnsupportedExecutable,MissingContext,
    InvalidBinding,MissingObservation,UnknownCreator,UnknownOrigin,Changed,
    DescriptorMismatch,UnsupportedResource,Capacity,Failure
};
struct Slot {
    uint64_t layout{},table{},address{},descriptor{};
    uint32_t group{},parameter{},index{},offset{},first{},count{};
    bool operator==(const Slot&)const=default;
};
namespace detail {
template<class T,size_t N>T Field(const std::array<uint8_t,N>& bytes,size_t offset){T value{};std::memcpy(&value,bytes.data()+offset,sizeof(value));return value;}
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/source_cbv_binding.rs (tools/single_pass_tests/source_cbv_binding_test runs its
// cases against them in its Rust variant). UploadWriterLedger's members stay here, mirrored there.
extern "C" {
bool cvr_scb_track_stage_mask(uint32_t,uint32_t*);
bool cvr_scb_slot(uint32_t,geometry::MemoryReader,void*,uint64_t,uint32_t,uint32_t,uint64_t,bool,Slot*);
bool cvr_scb_descriptor(uint64_t,uint64_t,uint64_t,uint32_t);
bool cvr_scb_decode_creation(const void*,geometry::MemoryReader,void*,void*);
bool cvr_scb_check(uint32_t,const void*,const void*,uint32_t,uint64_t,uint64_t);
bool cvr_scb_ledger(void*,uint32_t,const void*,const void*,const void*,uint32_t,uint32_t,void*,void*);
}
inline bool TrackStageMask(uint32_t stages,uint32_t& tracked)noexcept{return cvr_scb_track_stage_mask(stages,&tracked);}
inline bool ReadSlot(geometry::MemoryReader read,void* user,uint64_t state,uint32_t stage,uint32_t reg,Slot& out){
    return cvr_scb_slot(0,read,user,state,stage,reg,0,false,&out);
}
inline bool Descriptor(uint64_t gpu,uint64_t width,uint64_t address,uint32_t bytes)noexcept{return cvr_scb_descriptor(gpu,width,address,bytes);}
#else
inline bool TrackStageMask(uint32_t stages,uint32_t& tracked)noexcept {
    tracked=0;if(stages!=1 && stages!=2 && stages!=3 && stages!=7)return false;
    tracked=stages&3;return true;
}
inline bool ReadSlot(geometry::MemoryReader read,void* user,uint64_t state,uint32_t stage,uint32_t reg,Slot& out){
    out={};auto copy=[&](uint64_t at,void* value,size_t bytes){return read && at && at<=UINT64_MAX-bytes && read(user,at,value,bytes);};
    if(!state || state>UINT64_MAX-0x70 || stage>1 || reg>13)return false;
    Slot next;
    if(!copy(state+0x68,&next.layout,8) || !copy(state+0x28,&next.table,8) || !next.layout || !next.table || next.layout>UINT64_MAX-0x48A)return false;
    uint8_t group{};if(!copy(next.layout+0x453+uint64_t(stage)*0x1C+uint64_t(reg)*2,&group,1) || group>=64)return false;
    next.group=group;const auto entry=next.layout+0x38+uint64_t(group)*16;
    std::array<uint8_t,16> bytes{};if(!copy(entry,bytes.data(),bytes.size()) || Field<uint32_t>(bytes,0)!=2 || bytes[13]==2 || bytes[14]>=64)return false;
    next.first=Field<uint16_t>(bytes,8);next.count=Field<uint16_t>(bytes,10);next.parameter=bytes[14];next.offset=reg-next.first;
    const auto start=Field<uint32_t>(bytes,4);
    if(reg<next.first || !next.count || next.offset>=next.count || start>=8192 || next.count>8192-start)return false;
    next.index=start+next.offset;
    if(next.table>UINT64_MAX-uint64_t(next.index)*8-8)return false;
    next.address=next.table+uint64_t(next.index)*8;
    if(!copy(next.address,&next.descriptor,8) || !next.descriptor)return false;out=next;return true;
}
inline bool Descriptor(uint64_t gpu,uint64_t width,uint64_t address,uint32_t bytes)noexcept {
    return gpu && width && gpu<=UINT64_MAX-width && address && !(address&255) && bytes && bytes<=65536 && !(bytes&255) &&
        address>=gpu && address-gpu<=width && bytes<=width-(address-gpu);
}
#endif
struct CreationFrame {uint64_t rva{},rsp{},rbx{},rbp{},rdi{},rsi{},r12{},r13{},r14{},r15{};};
inline constexpr uint32_t DirectCreatorRva=0x3362A0,DirectMemcpyReturnRva=0x33626F;
struct Creation {
    uint64_t resource{},offset{},cpu{},context{};
    uint32_t caller{},handle{},logicalBytes{},reservedBytes{},completedProducerRva{};
    bool operator==(const Creation&)const=default;
};
#if CVR_RUST_VRIK
inline bool DirectProducer(const Creation& creation,uint32_t viewBytes)noexcept{return cvr_scb_check(0,&creation,nullptr,viewBytes,0,0);}
inline bool DecodeCreation(const CreationFrame& frame,geometry::MemoryReader read,void* user,Creation& output){
    return cvr_scb_decode_creation(&frame,read,user,&output);
}
#else
inline bool DirectProducer(const Creation& creation,uint32_t viewBytes)noexcept {
    return creation.caller==DirectCreatorRva && creation.completedProducerRva==DirectMemcpyReturnRva &&
        creation.logicalBytes && creation.logicalBytes<=65536 && creation.reservedBytes && creation.reservedBytes<=65536 &&
        !(creation.reservedBytes&255) && creation.reservedBytes==((creation.logicalBytes+255u)&~255u) &&
        viewBytes==creation.reservedBytes;
}
inline bool DecodeCreation(const CreationFrame& frame,geometry::MemoryReader read,void* user,Creation& output){
    output={};auto copy=[&](uint64_t at,void* value,size_t bytes){return read && at && at<=UINT64_MAX-bytes && read(user,at,value,bytes);};
    Creation next;next.caller=uint32_t(frame.rva);
    if(frame.rva==0x333C1A){
        if(frame.rsp>UINT64_MAX-0x40 || !copy(frame.rsp+0x30,&next.resource,8) || !copy(frame.rsp+0x38,&next.offset,8))return false;
        next.cpu=frame.rbx;
    }else if(frame.rva==DirectCreatorRva){
        if(!frame.r13 || frame.r13>65536 || !frame.r12 || frame.r12>65536 || (frame.r12&255) ||
            frame.r12!=((frame.r13+255)&~uint64_t(255)))return false;
        next.resource=frame.rbx;next.offset=frame.r15;next.context=frame.r14;
        next.logicalBytes=uint32_t(frame.r13);next.reservedBytes=uint32_t(frame.r12);next.completedProducerRva=DirectMemcpyReturnRva;
        if(next.context>UINT64_MAX-0x80 || !copy(next.context+0x78,&next.cpu,8))return false;
    }else if(frame.rva==0x3346B1){
        next.offset=frame.r12;next.context=frame.r13;
        uint32_t key{};
        if(!frame.r14 || frame.r14>UINT64_MAX-0x14F2248 || frame.rdi<0xC || frame.rdi>UINT64_MAX-0x30 ||
            !copy(frame.r14+0x14F2240,&next.resource,8) || !copy(frame.rdi+0x28,&next.cpu,8) ||
            !copy(frame.rdi-0xC,&key,4) || !copy(frame.rdi-8,&next.handle,4) || key!=next.handle || !key || key>0x100000)return false;
    }else return false;
    if(!next.resource || !next.cpu)return false;output=next;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool SameView(const descriptors::DescriptorStamp& stamp,uint64_t address,uint32_t bytes)noexcept{return cvr_scb_check(1,&stamp,nullptr,bytes,address,0);}
inline bool SameOrigin(const descriptors::DescriptorStamp& stamp,uint64_t cookie,uint64_t id)noexcept{return cvr_scb_check(2,&stamp,nullptr,0,cookie,id);}
inline bool DirectCreation(uint32_t stage,uint32_t reg,uint32_t caller)noexcept{return cvr_scb_check(3,nullptr,nullptr,stage,reg,caller);}
inline bool SelectSlot(geometry::MemoryReader read,void* user,uint64_t state,uint32_t stage,uint32_t reg,
    uint64_t descriptor,bool requireCurrent,Slot& out){
    return cvr_scb_slot(1,read,user,state,stage,reg,descriptor,requireCurrent,&out);
}
inline bool SameSourceSlot(const Slot& a,const Slot& b)noexcept{return cvr_scb_check(4,&a,&b,0,0,0);}
inline bool SameBuffer(const geometry::NativeBuffer& x,const geometry::NativeBuffer& y)noexcept{return cvr_scb_check(5,&x,&y,0,0,0);}
#else
inline bool SameView(const descriptors::DescriptorStamp& stamp,uint64_t address,uint32_t bytes)noexcept {
    return stamp.gpu && stamp.heap && stamp.value[0]==1 && stamp.value[4]==address && stamp.value[5]==bytes;
}
inline bool SameOrigin(const descriptors::DescriptorStamp& stamp,uint64_t cookie,uint64_t id)noexcept {
    return cookie && id && stamp.value[2]==cookie && stamp.value[3]==id;
}
inline bool DirectCreation(uint32_t stage,uint32_t reg,uint32_t caller)noexcept {
    return stage==0 && reg==5 && caller==DirectCreatorRva;
}
inline bool SelectSlot(geometry::MemoryReader read,void* user,uint64_t state,uint32_t stage,uint32_t reg,
    uint64_t descriptor,bool requireCurrent,Slot& out){
    out={};if(stage>1 || reg>13 || !descriptor)return false;Slot current;
    if(ReadSlot(read,user,state,stage,reg,current) && current.descriptor==descriptor){out=current;return true;}
    if(requireCurrent)return false;out.descriptor=descriptor;return true;
}
inline bool SameSourceSlot(const Slot& a,const Slot& b)noexcept {return a.descriptor && a.descriptor==b.descriptor;}
inline bool SameBuffer(const geometry::NativeBuffer& x,const geometry::NativeBuffer& y)noexcept {
    return x.resource==y.resource && x.gpu==y.gpu && x.bytes==y.bytes && x.registry==y.registry &&
        x.context==y.context && x.handle==y.handle && x.kind==y.kind;
}
#endif
inline constexpr UINT MaxUploadWriterDepth=8;
inline constexpr uint32_t UploadWriterRva=0x334328,UploadWriterMemcpyReturnRva=0x33435E;
struct UploadWriterContext {
    uint64_t thread{},token{},list{},generation{},context{},registry{};
    bool operator==(const UploadWriterContext&)const=default;
#if CVR_RUST_VRIK
    bool Valid()const noexcept;
#else
    bool Valid()const noexcept{return thread && token && list && generation && context && registry;}
#endif
};
struct UploadWriterStamp {
    uint64_t nonce{};UINT depth{};
    bool operator==(const UploadWriterStamp&)const=default;
};
struct UploadWriterJoin {
    uint64_t origin{},destination{},resource{},offset{};
    uint32_t handle{},logicalBytes{},reservedBytes{};
};
struct UploadWriterCompletion {uint64_t nonce{},origin{},destination{};uint32_t bytes{},producerRva{};};
// Exact nearest native return chain from the OS unwinder, never caller labels.
#if CVR_RUST_VRIK
inline bool UploadWriterPath(std::span<const uint64_t> returns)noexcept{return cvr_scb_check(6,returns.data(),nullptr,0,returns.size(),0);}
#else
inline bool UploadWriterPath(std::span<const uint64_t> returns)noexcept {
    return returns.size()==3 && returns[0]==0x3346B1 && returns[1]==0x334469 && returns[2]==0x33434B;
}
#endif
#if CVR_RUST_VRIK
class UploadWriterLedger {
    struct Entry {UploadWriterStamp stamp;UploadWriterContext context;UploadWriterJoin joined;uint32_t handle{},bytes{};bool valid{};};
    std::array<Entry,MaxUploadWriterDepth> entries_{};UINT depth_{};uint64_t nonce_{};
    bool Call(uint32_t which,const UploadWriterStamp* stamp,const UploadWriterContext* context,const UploadWriterJoin* joined,
        uint32_t a,uint32_t b,UploadWriterStamp* stampOut,UploadWriterCompletion* completion)noexcept{
        return cvr_scb_ledger(this,which,stamp,context,joined,a,b,stampOut,completion);
    }
public:
    UploadWriterStamp Current()const noexcept{
        UploadWriterStamp stamp;cvr_scb_ledger(const_cast<UploadWriterLedger*>(this),0,nullptr,nullptr,nullptr,0,0,&stamp,nullptr);return stamp;
    }
    UINT Depth()const noexcept{return depth_;}
    UploadWriterStamp Enter(const UploadWriterContext& context,uint32_t handle,uint32_t bytes)noexcept {
        UploadWriterStamp stamp;Call(1,nullptr,&context,nullptr,handle,bytes,&stamp,nullptr);return stamp;
    }
    bool Join(const UploadWriterStamp& stamp,const UploadWriterContext& context,const UploadWriterJoin& joined)noexcept {
        return Call(2,&stamp,&context,&joined,0,0,nullptr,nullptr);
    }
    bool Finish(const UploadWriterStamp& stamp,const UploadWriterContext& context,UploadWriterCompletion& output)noexcept {
        return Call(3,&stamp,&context,nullptr,0,0,nullptr,&output);
    }
    bool Cancel(const UploadWriterStamp& stamp)noexcept{return Call(4,&stamp,nullptr,nullptr,0,0,nullptr,nullptr);}
    void Invalidate()noexcept{Call(5,nullptr,nullptr,nullptr,0,0,nullptr,nullptr);}
};
inline bool UploadWriterContext::Valid()const noexcept{return cvr_scb_check(7,this,nullptr,0,0,0);}
static_assert(sizeof(Slot)==56 && sizeof(CreationFrame)==80 && sizeof(Creation)==56 && sizeof(descriptors::DescriptorStamp)==176 &&
    sizeof(UploadWriterContext)==48 && sizeof(UploadWriterStamp)==16 && sizeof(UploadWriterJoin)==48 &&
    sizeof(UploadWriterCompletion)==32 && sizeof(UploadWriterLedger)==1040,"mirrored by source_cbv_binding.rs");
#else
class UploadWriterLedger {
    struct Entry {UploadWriterStamp stamp;UploadWriterContext context;UploadWriterJoin joined;uint32_t handle{},bytes{};bool valid{};};
    std::array<Entry,MaxUploadWriterDepth> entries_{};UINT depth_{};uint64_t nonce_{};
public:
    UploadWriterStamp Current()const noexcept{return depth_?entries_[depth_-1].stamp:UploadWriterStamp{};}
    UINT Depth()const noexcept{return depth_;}
    UploadWriterStamp Enter(const UploadWriterContext& context,uint32_t handle,uint32_t bytes)noexcept {
        for(UINT n=0;n<depth_;++n)entries_[n].valid=false;
        if(!context.Valid() || !handle || handle>0x100000 || !bytes || bytes>65536)return {};
        if(depth_>=MaxUploadWriterDepth || nonce_==UINT64_MAX)return {};
        const bool outermost=!depth_;const UploadWriterStamp stamp{++nonce_,depth_+1};entries_[depth_++]={stamp,context,{},handle,bytes,outermost};return stamp;
    }
    bool Join(const UploadWriterStamp& stamp,const UploadWriterContext& context,const UploadWriterJoin& joined)noexcept {
        if(!depth_ || !stamp.nonce || Current()!=stamp)return false;auto& entry=entries_[depth_-1];
        if(!entry.valid || entry.joined.origin || entry.context!=context || entry.handle!=joined.handle ||
            !joined.origin || !joined.destination || !joined.resource || !joined.logicalBytes || joined.logicalBytes>65536 ||
            !joined.reservedBytes || joined.reservedBytes>65536 || joined.reservedBytes!=((joined.logicalBytes+255u)&~255u) ||
            entry.bytes>joined.logicalBytes || joined.destination>UINT64_MAX-entry.bytes){entry.valid=false;return false;}
        entry.joined=joined;return true;
    }
    bool Finish(const UploadWriterStamp& stamp,const UploadWriterContext& context,UploadWriterCompletion& output)noexcept {
        output={};if(!depth_ || !stamp.nonce || Current()!=stamp){Invalidate();return false;}
        const auto entry=entries_[--depth_];entries_[depth_]={};
        if(!entry.valid || !entry.joined.origin || entry.context!=context)return false;
        output={stamp.nonce,entry.joined.origin,entry.joined.destination,entry.bytes,UploadWriterMemcpyReturnRva};return true;
    }
    bool Cancel(const UploadWriterStamp& stamp)noexcept {
        if(!depth_ || !stamp.nonce || Current()!=stamp)return false;entries_[--depth_]={};return true;
    }
    void Invalidate()noexcept{for(UINT n=0;n<depth_;++n)entries_[n].valid=false;}
};
#endif
}
class UploadWriterScope {
public:
    UploadWriterScope(uint32_t handle,uint32_t bytes)noexcept;
    ~UploadWriterScope();
    UploadWriterScope(const UploadWriterScope&)=delete;UploadWriterScope& operator=(const UploadWriterScope&)=delete;
    // Call only after the original guarded 1F088C invocation returns normally.
    // Destructor cancellation, nesting and ambiguous creation never close it.
    void CompleteNormalReturn()noexcept;
private:detail::UploadWriterStamp stamp_;
};
struct Annotation {
    uint64_t cpu{};D3D12_CONSTANT_BUFFER_VIEW_DESC view{};descriptors::CbvOrigin origin;
};
struct SourceCbvBacking {
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    Microsoft::WRL::ComPtr<IUnknown> originOwner;
    descriptors::DescriptorStamp stamp;
    native_recording::Lease recording;
    geometry::NativeBuffer native;
    uint64_t cookie{},origin{},gpu{},backingBytes{},sourceOffset{},serial{},cpuDescriptor{};
    uint32_t descriptorBytes{},logicalBytes{},stage{},shaderRegister{},creatorRva{};
    // Only guarded completed producers supply these exact write facts.
    // They do not establish an original GPU read epoch or upload permission.
    uint32_t reservedBytes{},completedProducerRva{};
    // Optional actual creation occurrence. Missing/older leases do not erase
    // the immutable producer facts; consumers validate their required epoch.
    native_recording::Lease creationRecording;
    uint64_t writerNonce{},writerDestination{};uint32_t writerBytes{};
    native_recording::Lease writerRecording;
    // Strong current backing ownership only. No byte/state/allocation/queue grant.
};
struct Diagnostic {
    // phase,status,stage,register,recordingGeneration,context,cpuDescriptor,
    // canonicalCookie,canonicalOrigin,creator,originCpu,handle,kind,serial,
    // setterAddress,setterViewBytes. Source-read TLS snapshot only.
    std::array<uint64_t,16> values{};
};
enum class Phase:uint32_t {Idle,Code,Context,Slot,Stamp,Binding,CpuView,Observation,Origin,Native,Resource,Revalidate,Serial,Complete};
// Capture original whole bodies BEFORE any MH_CreateHook/MH_EnableHook.
void PrepareCodeGuards(uintptr_t gameBase);
bool Compatible();
// Called after the original stage-MASK 1F4700 / stage-INDEX 1F3790 setter. Root
// applies returned annotations with DescriptorAudit::Cbv without touching D3D.
std::vector<Annotation> ObserveBindMask(uint32_t shaderRegister,uint32_t handle,uint32_t stages);
std::vector<Annotation> ObserveBindStage(uint32_t shaderRegister,uint32_t handle,uint32_t stageIndex);
// After original native CreateCBV, before descriptor metadata is finalized.
// Caller is _ReturnAddress from the device hook; frames are OS-unwound exactly.
descriptors::CbvOrigin ObserveCreatedCbv(ID3D12Device*,const D3D12_CONSTANT_BUFFER_VIEW_DESC*,D3D12_CPU_DESCRIPTOR_HANDLE,uintptr_t caller);
// Required pairing for transient 1F0BF7: actual GetGPUVirtualAddress API
// argument/result at native return 1F0BD5. Never a historical address registry.
void ObserveCreationAddress(ID3D12Resource*,uint64_t address,uintptr_t caller);
// Actual source callback only. Root supplies a held propagated origin owner
// OUTSIDE its metadata lock; registered pre-created descriptors can use null.
Status ReadAtSource(ID3D12GraphicsCommandList*,uint64_t root,UINT parameter,UINT offset,
    const descriptors::DescriptorStamp&,IUnknown* originOwner,SourceCbvBacking&);
void Reset(ID3D12GraphicsCommandList*);
void Clear();
uint32_t LastStatus();
Diagnostic LastDiagnostic();
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SourceCbvBindingEnable;
}
