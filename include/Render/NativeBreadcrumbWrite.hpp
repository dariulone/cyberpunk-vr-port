#pragma once
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/NativeCommandRecording.hpp"
#include <array>
#include <memory>

namespace cvr::stereo::breadcrumb_write {
namespace detail {
using ReadMemory=bool(*)(void*,uint64_t,void*,size_t);
#if CVR_RUST_VRIK
// The rules below: rust/crates/cvr-render/src/breadcrumb.rs (tools/single_pass_tests/breadcrumb_write_test runs its cases
// against them in its Rust variant).
struct Observation;struct Frame;struct FrameLayout;
extern "C" {
uint32_t cvr_bc_caller_mode(uint64_t,uint64_t);
uint32_t cvr_bc_nearest_caller(uint64_t,uint64_t,const uint64_t*,size_t);
bool cvr_bc_parameters(uint32_t,UINT,uint64_t,uint32_t,uint32_t);
bool cvr_bc_stack_span(const Frame*,uint64_t,uint64_t);
bool cvr_bc_layout(uint64_t,const Frame*,FrameLayout*);
bool cvr_bc_frame_from_registers(uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,Frame*);
bool cvr_bc_observe(ReadMemory,void*,uint64_t,const Frame*,uint64_t,uint64_t,uint32_t,uint32_t,Observation*,uint32_t*);
bool cvr_bc_shape(const Observation*,const D3D12_RESOURCE_DESC*,const D3D12_HEAP_DESC*,const D3D12_RESOURCE_ALLOCATION_INFO*,uint64_t);
bool cvr_bc_observed_creation_matches(const constants::ResourceCreationInfo*,const Observation*,uint64_t);
}
#endif
inline constexpr uint32_t MarkerInReturn=0x3E529E,MarkerOutReturn=0x3E5A59;
// 2.5 inlines the out-of-line chunk 2.31 kept at 0x1EDF598 into sub_14051FF90 (its row covers it).
inline constexpr std::array<native_recording::detail::Code,16> Contract{{
    {0x3E47E8,382,0xD8695C867CC19844ull},{0x3E5174,349,0x260466FD7E09BA99ull},
    {0x246A0B6,138,0x1471986E0476A136ull},{0x3E59DC,156,0x7261A9D40E98E5FDull},
    {0x246A1E6,87,0x9F25F05CFB672FC7ull},{0x688D5C,260,0x2EB88470C17A84D9ull},
    {0x68A1D0,978,0x0E75B87A8C6851B5ull},{0x51FDD8,439,0x15A56074B6B4CFBEull},
    {0x51FF90,338,0xDE807FDE92738233ull},
    {0x520284,123,0x77AA09F4EE843A10ull},{0x51FD40,149,0x12D9E1DA51EB8090ull},
    {0x520228,58,0x5FC34FAD173BDCE7ull},{0x1919E98,93,0x187A5BC1963CF585ull},
    {0x14F4AD0,35,0x104BC2F46DA77B2Full},{0x2FAEE24,41,0x18A456DC7BE471B1ull},
    {0x2427F00,12,0xA453EAFC10D16B8Cull}
}};
inline bool Read(ReadMemory read,void* user,uint64_t address,void* output,size_t bytes){
    return read && address && bytes && address<=UINT64_MAX-bytes && read(user,address,output,bytes);
}
template<class T> bool Field(ReadMemory read,void* user,uint64_t address,uint64_t offset,T& output){
    return address && address<=UINT64_MAX-offset && Read(read,user,address+offset,&output,sizeof(output));
}
#if CVR_RUST_VRIK
inline uint32_t CallerMode(uint64_t base,uint64_t caller){return cvr_bc_caller_mode(base,caller);}
#else
inline uint32_t CallerMode(uint64_t base,uint64_t caller){
    if(!base || caller<base)return 0;
    if(caller-base==MarkerInReturn)return 1;
    if(caller-base==MarkerOutReturn)return 2;
    return 0;
}
#endif
// Frames are OS-unwound return addresses, never a scan for plausible values.
#if CVR_RUST_VRIK
inline uint32_t NearestCaller(uint64_t base,uint64_t imageBytes,std::span<const uint64_t> frames){
    return cvr_bc_nearest_caller(base,imageBytes,frames.data(),frames.size());
}
#else
inline uint32_t NearestCaller(uint64_t base,uint64_t imageBytes,std::span<const uint64_t> frames){
    if(!base || !imageBytes || base>UINT64_MAX-imageBytes || frames.size()>64)return 0;
    for(const auto rip:frames)if(rip>=base && rip-base<imageBytes)return CallerMode(base,rip);
    return 0;
}
#endif
#if CVR_RUST_VRIK
inline bool Parameters(uint32_t callerMode,UINT count,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER& value,uint32_t mode){
    return cvr_bc_parameters(callerMode,count,value.Dest,value.Value,mode);
}
#else
inline bool Parameters(uint32_t callerMode,UINT count,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER& value,uint32_t mode){
    return count==1 && (callerMode==1 || callerMode==2) && mode==callerMode && value.Value==callerMode &&
        value.Dest && !(value.Dest&3) && value.Dest<=UINT64_MAX-4;
}
#endif
struct Observation {
    uint64_t base{},registry{},device{},holder{},list{},marker{},entry{},heap{},resource{},gpu{},slotGpu{},bitmap{};
    uint32_t kind{},slot{},capacity{},stride{},slots{},words{};
    bool operator==(const Observation&)const=default;
};
struct Frame {
    uint64_t rip{},rsp{},rbp{},holder{},stackLow{},stackHigh{};
    bool operator==(const Frame&)const=default;
};
struct FrameLayout {uint64_t parameters{},modes{};uint32_t mode{};};
#if CVR_RUST_VRIK
static_assert(sizeof(Observation)==120 && sizeof(Frame)==48 && sizeof(FrameLayout)==24,"mirrored by breadcrumb.rs");
#endif
#if CVR_RUST_VRIK
inline bool StackSpan(const Frame& frame,uint64_t address,uint64_t bytes){return cvr_bc_stack_span(&frame,address,bytes);}
#else
inline bool StackSpan(const Frame& frame,uint64_t address,uint64_t bytes){
    return frame.stackLow && frame.stackLow<frame.stackHigh && frame.stackHigh>=bytes &&
        address>=frame.stackLow && address<=frame.stackHigh-bytes;
}
#endif
#if CVR_RUST_VRIK
inline bool Layout(uint64_t base,const Frame& frame,FrameLayout& output){return cvr_bc_layout(base,&frame,&output);}
#else
inline bool Layout(uint64_t base,const Frame& frame,FrameLayout& output){
    output={};const auto mode=CallerMode(base,frame.rip);
    if(!mode || !frame.holder || frame.holder>UINT64_MAX-8 || frame.rsp%16 ||
        frame.rsp>UINT64_MAX-0x54 || !StackSpan(frame,frame.rsp,8))return false;
    if(mode==1 && frame.rbp!=frame.rsp+0x30)return false;
    FrameLayout value{frame.rsp+0x20,frame.rsp+(mode==1?0x50:0x40),mode};
    if(!StackSpan(frame,value.parameters,sizeof(D3D12_WRITEBUFFERIMMEDIATE_PARAMETER)) ||
        !StackSpan(frame,value.modes,sizeof(D3D12_WRITEBUFFERIMMEDIATE_MODE)))return false;
    output=value;return true;
}
#endif
// Only the audited nonvolatile argument holder is recovered: RDI in marker-in,
// RBX in marker-out. RBP is a frame pointer only in marker-in, not marker-out.
#if CVR_RUST_VRIK
inline bool FrameFromRegisters(uint64_t base,uint64_t rip,uint64_t rsp,uint64_t rbp,uint64_t rdi,uint64_t rbx,
                               uint64_t low,uint64_t high,Frame& output){
    return cvr_bc_frame_from_registers(base,rip,rsp,rbp,rdi,rbx,low,high,&output);
}
#else
inline bool FrameFromRegisters(uint64_t base,uint64_t rip,uint64_t rsp,uint64_t rbp,uint64_t rdi,uint64_t rbx,
                               uint64_t low,uint64_t high,Frame& output){
    output={};const auto mode=CallerMode(base,rip);if(!mode)return false;
    Frame value{rip,rsp,mode==1?rbp:0,mode==1?rdi:rbx,low,high};FrameLayout layout;
    if(!Layout(base,value,layout))return false;output=value;return true;
}
#endif
// Reader failures: 1 arguments/frame, 2 globals/holder, 3 marker association,
// 4 factory shape, 5 heap/buffer, 6 bitmap/lock, 7 slot range, 8 destination.
// 9 native stack arguments differ. Padding bytes are not compared.
#if CVR_RUST_VRIK
inline bool Observe(ReadMemory read,void* user,uint64_t base,const Frame& frame,uint64_t actualList,
                    const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER& parameter,uint32_t mode,
                    Observation& output,uint32_t* failure=nullptr){
    return cvr_bc_observe(read,user,base,&frame,actualList,parameter.Dest,parameter.Value,mode,&output,failure);
}
#else
inline bool Observe(ReadMemory read,void* user,uint64_t base,const Frame& frame,uint64_t actualList,
                    const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER& parameter,uint32_t mode,
                    Observation& output,uint32_t* failure=nullptr){
    output={};if(failure)*failure=0;Observation value;value.base=base;value.holder=frame.holder;value.list=actualList;
    const auto fail=[&](uint32_t code){if(failure)*failure=code;output=value;return false;};
    FrameLayout layout;
    if(!base || base>UINT64_MAX-0x3C29078 || !actualList || !Layout(base,frame,layout) ||
        !Parameters(layout.mode,1,parameter,mode))return fail(1);
    const auto destination=parameter.Dest;D3D12_WRITEBUFFERIMMEDIATE_PARAMETER native{};uint32_t nativeMode{};
    if(!Read(read,user,layout.parameters,&native,sizeof(native)) || !Read(read,user,layout.modes,&nativeMode,4) ||
        native.Dest!=parameter.Dest || native.Value!=parameter.Value || nativeMode!=mode)return fail(9);
    uint8_t stopped{};
    if(!Field(read,user,base,0x3BCDCA8,value.registry) || !value.registry ||
        !Field(read,user,value.registry,1,stopped) || stopped ||
        !Field(read,user,base,0x3C28E30,value.device) || !value.device ||
        !Field(read,user,frame.holder,0,value.marker) || !value.marker)return fail(2);
    uint64_t markerList{};
    if(!Field(read,user,value.marker,0,markerList) || markerList!=actualList ||
        !Field(read,user,value.marker,8,value.kind) || !value.kind || value.kind>4 ||
        !Field(read,user,value.marker,0x10,value.capacity) ||
        !Field(read,user,value.marker,0x60,value.slotGpu) || !value.slotGpu ||
        !Field(read,user,value.marker,0x68,value.slot) || !value.slot)return fail(3);
    value.entry=base+0x3C28E38+uint64_t(value.kind)*0x70;
    const uint32_t expectedSlots=value.kind==1?1024:value.kind==4?64:8;
    const uint32_t expectedStride=value.kind==1?4096:value.kind==4?2048:512;
    if(!Field(read,user,value.entry,0x20,value.stride) || value.stride!=expectedStride || value.capacity!=expectedStride/4 ||
        !Field(read,user,value.entry,0x60,value.slots) || value.slots!=expectedSlots || value.slot>value.slots ||
        !Field(read,user,value.entry,0x5C,value.words) || value.words!=(value.slots+63)/64)return fail(4);
    if(!Field(read,user,value.entry,0x30,value.heap) || !value.heap ||
        !Field(read,user,value.entry,0x38,value.resource) || !value.resource ||
        !Field(read,user,value.entry,0x40,value.gpu) || !value.gpu)return fail(5);
    uint8_t lock{};uint64_t word{};
    if(!Field(read,user,value.entry,0x48,lock) || lock || !Field(read,user,value.entry,0x50,value.bitmap) || !value.bitmap ||
        !Field(read,user,value.bitmap,uint64_t((value.slot-1)/64)*8,word) || !(word&(uint64_t(1)<<((value.slot-1)%64))) ||
        !Field(read,user,value.entry,0x48,lock) || lock)return fail(6);
    const uint64_t total=uint64_t(value.stride)*value.slots,offset=uint64_t(value.slot-1)*value.stride;
    if(value.gpu>UINT64_MAX-total || value.slotGpu!=value.gpu+offset || value.slotGpu>UINT64_MAX-value.stride)return fail(7);
    if(destination<value.slotGpu || destination-value.slotGpu>value.stride-4)return fail(8);
    output=value;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool Shape(const Observation& observation,const D3D12_RESOURCE_DESC& desc,const D3D12_HEAP_DESC& heap,
                  const D3D12_RESOURCE_ALLOCATION_INFO& allocation,uint64_t gpu){
    return cvr_bc_shape(&observation,&desc,&heap,&allocation,gpu);
}
#else
inline bool Shape(const Observation& observation,const D3D12_RESOURCE_DESC& desc,const D3D12_HEAP_DESC& heap,
                  const D3D12_RESOURCE_ALLOCATION_INFO& allocation,uint64_t gpu){
    const auto flags=D3D12_RESOURCE_FLAGS(D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE|
        ((heap.Flags&D3D12_HEAP_FLAG_SHARED_CROSS_ADAPTER)?D3D12_RESOURCE_FLAG_ALLOW_CROSS_ADAPTER:0));
    return gpu==observation.gpu && desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER &&
        desc.Width==uint64_t(observation.stride)*observation.slots && desc.Height==1 && desc.DepthOrArraySize==1 && desc.MipLevels==1 &&
        desc.Format==DXGI_FORMAT_UNKNOWN && desc.SampleDesc.Count==1 && !desc.SampleDesc.Quality &&
        desc.Layout==D3D12_TEXTURE_LAYOUT_ROW_MAJOR && desc.Flags==flags &&
        allocation.SizeInBytes && allocation.SizeInBytes!=UINT64_MAX && allocation.SizeInBytes>=desc.Width &&
        allocation.Alignment && allocation.SizeInBytes<=heap.SizeInBytes &&
        !(allocation.SizeInBytes%allocation.Alignment) && (!heap.Alignment || heap.Alignment==allocation.Alignment);
}
#endif
#if CVR_RUST_VRIK
inline bool ObservedCreationMatches(const constants::ResourceCreationInfo& value,const Observation& native,
                                   uint64_t allocationBytes){
    return cvr_bc_observed_creation_matches(&value,&native,allocationBytes);
}
#else
inline bool ObservedCreationMatches(const constants::ResourceCreationInfo& value,const Observation& native,
                                   uint64_t allocationBytes){
    return value.kind==2 && reinterpret_cast<uintptr_t>(value.resource.Get())==native.resource &&
        reinterpret_cast<uintptr_t>(value.heap.Get())==native.heap && !value.heapOffset && value.allocationBytes==allocationBytes &&
        (!value.initialStateKnown || value.initialState==D3D12_RESOURCE_STATE_COPY_DEST);
}
#endif
}
enum class Provenance:uint32_t {ObservedCreation=1,NativeFactory=2};
struct Snapshot {
    constants::ResourceCreationInfo creation;
    D3D12_RESOURCE_DESC resourceDesc{};
    D3D12_HEAP_DESC heapDesc{};
    uint64_t destination{};uint32_t value{},mode{};
    Provenance provenance{};
};
class Lease {
    struct Data;std::shared_ptr<const Data> data_;
    friend bool Capture(ID3D12GraphicsCommandList2*,UINT,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER*,
                        const D3D12_WRITEBUFFERIMMEDIATE_MODE*,uintptr_t,Lease&);
    friend bool Describe(const Lease&,Snapshot&);
public:explicit operator bool()const{return bool(data_);}
};
// Only the actual audited native breadcrumb call. This records the buffer's
// physical identity for access classification, never grants later GPU access.
bool Capture(ID3D12GraphicsCommandList2*,UINT,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER*,
             const D3D12_WRITEBUFFERIMMEDIATE_MODE*,uintptr_t caller,Lease&);
// Returns held, authenticated metadata. NativeFactory is not an API creation
// receipt and is never registered as one; COM retention is not allocation pinning.
bool Describe(const Lease&,Snapshot&);
uint32_t LastFailure();
// stage,detail,caller,markerHolder,marker,entry,resource,heap,gpu,slotGpu,slot,kind.
// Stages:1arguments,2contract(detail=RVA),3native frame,4parameters,
// 6native reader(detail=Observe rejection),7COM shape,8creation,9changed,10exception.
// Native-frame details101..110: image,caller,stack,nearest game frame,function
// extent,frame layout,unwind metadata,unwind progress,exception,frame depth.
std::array<uint64_t,12> LastDetails();
}
