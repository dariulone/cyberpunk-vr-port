#pragma once
#include "Render/NativeGeometryPackets.hpp"
#include <cstddef>
#include <cstring>
#include <span>
#include <vector>
#include <algorithm>

namespace cvr::stereo::packets {
struct Inputs {
    uint32_t valid{},instances{},commandProgramField{},source{};
    std::array<uint8_t,64> draw{};
    std::array<uint8_t,8> shaders{}; // native entry is six bytes; final two are zero
    std::array<uint8_t,40> state{};
    std::array<uint8_t,24> object{};
    std::array<uint8_t,48> transform{};
    std::array<uint8_t,16> skin{};
    uint32_t instanceOffset{},instanceBytes{},instanceStride{},programOffset{},programBytes{},reserved{};
    uint64_t instanceGpuAddress{};
    uint64_t instanceResource{};
};
static_assert(sizeof(Inputs)==256);
struct MappedInstances {uint64_t cpu{},gpu{},bytes{},resource{};};
bool ResolveLive(uint64_t renderer,std::span<const Packet>,std::vector<Inputs>&,std::vector<uint8_t>&,bool geometryOnly=false);
bool CurrentDrawGeometry(uint64_t renderer,uint32_t instances,uint32_t stride,std::array<uint8_t,64>&);
constexpr size_t MaxInputBlobBytes=32*1024*1024;
using ReadInputs=bool(*)(void*,uint64_t,void*,size_t);
// Layout comes from the current EXE's packet consumer (0x334CC0),
// transform gather (0x335544) and direct draw path (0x3335D8).
// These are owned snapshots, not reusable native buffer handles. Nonempty
// command programs and multi-instance packets need additional resolution.
// 2.5 moved the shader table (0x2581D0 -> 0x2541D0) and the object table (0x274248 -> 0x270248) and shrank the
// state record from 40 to 32 bytes: the two resource pointers at +0/+8 became one u32 handle at +0, so the
// command program pointer moved +0x10 -> +0x08, its byte count +0x18 -> +0x10, the u32 +0x20 -> +0x18 and the
// three flag bytes +0x24..26 -> +0x1C..1E. state[] keeps its 40 bytes; the last 8 stay zero.
inline constexpr uint32_t StateBytes=32,StateProgram=0x08,StateProgramBytes=0x10;
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/packet_inputs.rs (tools/single_pass_tests/packet_inputs_test runs its cases against
// them in its Rust variant). The blob stays the caller's vector, grown and truncated through ResizeBlob.
namespace detail {
extern "C" {
void cvr_pkt_resolve_inputs(const Packet*,uint64_t,ReadInputs,void*,bool,Inputs*);
bool cvr_pkt_valid_program(const uint8_t*,size_t);
void cvr_pkt_resolve_details(const Packet*,uint64_t,const MappedInstances*,ReadInputs,void*,void*,size_t,uint8_t*,
    uint8_t*(*)(void*,size_t,bool),bool,Inputs*);
bool cvr_pkt_equal_geometry(const Inputs*,const uint8_t*,size_t,const Inputs*,const uint8_t*,size_t);
}
// Growing keeps the original amortized reservation: half again the capacity (4 KiB at first), at most the blob limit.
inline uint8_t* ResizeBlob(void* blob,size_t size,bool growing){
    auto& bytes=*static_cast<std::vector<uint8_t>*>(blob);
    if(growing && size>bytes.capacity())bytes.reserve(std::min(MaxInputBlobBytes,
        std::max(size,bytes.capacity()?bytes.capacity()+bytes.capacity()/2:size_t(4096))));
    bytes.resize(size);return bytes.data();
}
}
static_assert(sizeof(MappedInstances)==32,"mirrored by packet_inputs.rs");
inline Inputs ResolveInputs(Packet packet,uint64_t pool,ReadInputs read,void* context=nullptr,bool geometryOnly=false) {
    Inputs result;detail::cvr_pkt_resolve_inputs(&packet,pool,read,context,geometryOnly,&result);return result;
}
inline bool ValidNativeProgram(std::span<const uint8_t> program){return detail::cvr_pkt_valid_program(program.data(),program.size());}
// Copy programs and every instance while their native input is live. A matching
// program is not proof that all mutable objects/resources it reads also match.
inline Inputs ResolveDetails(Packet packet,uint64_t pool,MappedInstances uploaded,ReadInputs read,void* context,
    std::vector<uint8_t>& blob,bool geometryOnly=false) {
    Inputs result;
    detail::cvr_pkt_resolve_details(&packet,pool,&uploaded,read,context,&blob,blob.size(),blob.data(),detail::ResizeBlob,geometryOnly,&result);
    return result;
}
// Geometry membership does not consume shader/state/program data. Those mutable
// contracts are validated separately at each actual MAIN and peer draw.
inline bool EqualGeometry(const Inputs& a,std::span<const uint8_t> aBlob,const Inputs& b,std::span<const uint8_t> bBlob){
    return detail::cvr_pkt_equal_geometry(&a,aBlob.data(),aBlob.size(),&b,bBlob.data(),bBlob.size());
}
#else
inline Inputs ResolveInputs(Packet packet,uint64_t pool,ReadInputs read,void* context=nullptr,bool geometryOnly=false) {
    Inputs result{};result.instances=uint32_t((packet.second>>18)&0x7fff);
    if(!pool || !read)return result;
    auto copy=[&](uint64_t offset,uint64_t index,uint64_t stride,void* output,size_t bytes,uint32_t flag) {
        const auto relative=offset+index*stride;
        if(relative>UINT64_MAX-pool || pool+relative>UINT64_MAX-bytes)return;
        if(read(context,pool+relative,output,bytes))result.valid|=flag;
    };
    copy(0x50,(packet.first>>17)&0x7fff,64,result.draw.data(),64,1);
    const bool skin=(packet.second&(uint64_t(1)<<50))!=0;
    if(!geometryOnly){
        copy(0x2541D0,(packet.first>>32)&0x1fff,6,result.shaders.data(),6,2);
        copy(0x240110,(packet.first>>45)&0x7ff,StateBytes,result.state.data(),StateBytes,4);
    }
    if(!geometryOnly || skin)copy(0x270248,packet.second&0x3ffff,24,result.object.data(),24,8);
    if(result.valid&4)std::memcpy(&result.commandProgramField,result.state.data()+StateProgramBytes,4);
    const bool preuploaded=(packet.first&(uint64_t(1)<<59))!=0 && !skin;
    const bool ordinary=(result.valid&1) && result.draw[0]==0 && (packet.second&(uint64_t(1)<<51));
    result.source=ordinary?(preuploaded?2:1):0;
    if(!geometryOnly && result.instances==1 && result.source==1)copy(0x570280,(packet.second>>33)&0x1ffff,48,result.transform.data(),48,16);
    if(!(packet.second&(uint64_t(1)<<50)))result.valid|=32;
    else if(result.valid&8) {
        uint64_t metadata{};std::memcpy(&metadata,result.object.data()+8,8);
        if(!metadata){const uint32_t one=1;std::memcpy(result.skin.data()+12,&one,4);result.valid|=32;}
        else if(metadata<=UINT64_MAX-0x24 && read(context,metadata+0x14,result.skin.data(),16))result.valid|=32;
    }
    return result;
}
inline bool ValidNativeProgram(std::span<const uint8_t> program) {
    if(program.size()<8 || program.size()>4096)return false;
    uint32_t count{};std::memcpy(&count,program.data(),4);if(count>128)return false;
    size_t at=8;
    for(uint32_t i=0;i<count;++i){
        if(program.size()-at<8)return false;uint32_t bytes{};std::memcpy(&bytes,program.data()+at+4,4);at+=8;
        if(bytes>program.size()-at)return false;at+=bytes;
    }
    return at==program.size();
}
// Copy programs and every instance while their native input is live. A matching
// program is not proof that all mutable objects/resources it reads also match.
inline Inputs ResolveDetails(Packet packet,uint64_t pool,MappedInstances uploaded,ReadInputs read,void* context,
    std::vector<uint8_t>& blob,bool geometryOnly=false) {
    auto result=ResolveInputs(packet,pool,read,context,geometryOnly);
    auto reserve=[&](size_t bytes,uint32_t& offset) {
        if(bytes>MaxInputBlobBytes || blob.size()>MaxInputBlobBytes-bytes)return false;
        if(blob.size()+bytes>blob.capacity())blob.reserve(std::min(MaxInputBlobBytes,
            std::max(blob.size()+bytes,blob.capacity()?blob.capacity()+blob.capacity()/2:size_t(4096))));
        offset=static_cast<uint32_t>(blob.size());blob.resize(blob.size()+bytes);return true;
    };
    auto copy=[&](uint64_t address,size_t bytes,uint32_t& offset) {
        if(!address || address>UINT64_MAX-bytes || !reserve(bytes,offset))return false;
        if(read(context,address,blob.data()+offset,bytes))return true;
        blob.resize(offset);return false;
    };
    if(result.valid&4) {
        if(!result.commandProgramField)result.valid|=64;
        else if(result.commandProgramField<=4096) {
            uint64_t address{};std::memcpy(&address,result.state.data()+StateProgram,8);
            if(copy(address,result.commandProgramField,result.programOffset)) {
                const auto bytes=std::span<const uint8_t>(blob).subspan(result.programOffset,result.commandProgramField);
                if(ValidNativeProgram(bytes)){result.programBytes=result.commandProgramField;result.valid|=64;}
                else blob.resize(result.programOffset);
            }
        }
    }
    result.valid&=~16u; // the complete stream below is the authority, not its first matrix
    if(!result.instances || !result.source)return result;
    const bool skin=(packet.second&(uint64_t(1)<<50))!=0;
    const auto first=(packet.second>>33)&0x1ffff;
    result.instanceStride=skin?64:48;
    const auto bytes=size_t(result.instances)*result.instanceStride;
    if(result.source==2) {
        const auto offset=first*48;
        if(uploaded.gpu && offset<=uploaded.bytes && bytes<=uploaded.bytes-offset && uploaded.gpu<=UINT64_MAX-offset) {
            result.instanceGpuAddress=uploaded.gpu+offset;result.instanceResource=uploaded.resource;
            if(uploaded.cpu && uploaded.cpu<=UINT64_MAX-offset && copy(uploaded.cpu+offset,bytes,result.instanceOffset)) {
                result.instanceBytes=static_cast<uint32_t>(bytes);result.valid|=16;
            }
        }
    } else if(pool && pool<=UINT64_MAX-0x570280-first*48) {
        const auto source=pool+0x570280+first*48;
        if(!skin) {
            if(copy(source,bytes,result.instanceOffset)){result.instanceBytes=static_cast<uint32_t>(bytes);result.valid|=16;}
        } else if(result.valid&32) {
            std::vector<uint8_t> matrices(size_t(result.instances)*48);
            if(source<=UINT64_MAX-matrices.size() && read(context,source,matrices.data(),matrices.size()) && reserve(bytes,result.instanceOffset)) {
                for(uint32_t i=0;i<result.instances;++i){
                    auto* destination=blob.data()+result.instanceOffset+size_t(i)*64;
                    std::memcpy(destination,matrices.data()+size_t(i)*48,48);std::memcpy(destination+48,result.skin.data(),16);
                }
                result.instanceBytes=static_cast<uint32_t>(bytes);result.valid|=16;
            }
        }
    }
    if(result.valid&16)std::memcpy(result.transform.data(),blob.data()+result.instanceOffset,48);
    return result;
}
// Geometry membership does not consume shader/state/program data. Those mutable
// contracts are validated separately at each actual MAIN and peer draw.
inline bool EqualGeometry(const Inputs& a,std::span<const uint8_t> aBlob,const Inputs& b,std::span<const uint8_t> bBlob){
    if((a.valid&17)!=(b.valid&17) || a.instances!=b.instances || a.draw!=b.draw || a.source!=b.source ||
        a.instanceStride!=b.instanceStride || a.instanceBytes!=b.instanceBytes || a.instanceGpuAddress!=b.instanceGpuAddress ||
        a.instanceResource!=b.instanceResource)return false;
    if(!(a.valid&16))return true;
    if(a.instanceOffset>aBlob.size() || a.instanceBytes>aBlob.size()-a.instanceOffset ||
        b.instanceOffset>bBlob.size() || b.instanceBytes>bBlob.size()-b.instanceOffset)return false;
    return std::equal(aBlob.begin()+a.instanceOffset,aBlob.begin()+a.instanceOffset+a.instanceBytes,bBlob.begin()+b.instanceOffset);
}
#endif
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_GeometryPacketResolve;
__declspec(dllexport) extern uint32_t CyberpunkVR_GeometryPacketInputBytes;
__declspec(dllexport) extern uint64_t CyberpunkVR_GeometryPacketInputs;
__declspec(dllexport) extern uint64_t CyberpunkVR_GeometryPacketInputBlob;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_GeometryPacketInputBlobBytes;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_GeometryPacketInputCount,CyberpunkVR_GeometryPacketInputFailure;
}
