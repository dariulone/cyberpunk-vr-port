#pragma once
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace cvr::stereo::packets {
struct DrawEntryFrame {
    uint64_t home[6];
    uint64_t rax,rcx,rdx,r8,r9,r10,r11,flags;
    std::array<uint8_t,96> vectors;
    uint64_t rdi,r15,r12,rbx,r14,caller;
};
static_assert(sizeof(DrawEntryFrame)==0x100 && offsetof(DrawEntryFrame,rax)==0x30 &&
    offsetof(DrawEntryFrame,vectors)==0x70 && offsetof(DrawEntryFrame,rdi)==0xD0 && offsetof(DrawEntryFrame,caller)==0xF8);
struct DrawOrigin {
    uint64_t renderer{},draw{},batch{},flush{},packet{},instances{};
    bool valid{};
};
#if CVR_RUST_VRIK
using OriginRead=bool(*)(void*,uint64_t,void*,size_t);
// The rules: rust/crates/cvr-render/src/draw_origin.rs (tools/single_pass_tests/draw_origin_test runs its cases against
// them in its Rust variant; the stack's members stay here, mirrored there).
extern "C" {
void cvr_dor_origin(const DrawEntryFrame*,uint64_t,DrawOrigin*);
const DrawOrigin* cvr_dor_stack(void*,uint32_t,const DrawOrigin*);
bool cvr_dor_resolve(const DrawOrigin*,uint64_t,uint32_t,uint32_t,OriginRead,void*,std::array<uint8_t,64>*);
}
inline DrawOrigin OriginFromEntry(const DrawEntryFrame& f,uint64_t base){
    static_assert(sizeof(DrawOrigin)==56,"mirrored by draw_origin.rs");
    DrawOrigin origin;cvr_dor_origin(&f,base,&origin);return origin;
}
class DrawOriginStack {
public:
    void Enter(DrawOrigin value){cvr_dor_stack(this,0,&value);}
    void Leave(){cvr_dor_stack(this,1,nullptr);}
    const DrawOrigin* Current()const{return cvr_dor_stack(const_cast<DrawOriginStack*>(this),2,nullptr);}
private:
    std::array<DrawOrigin,32> entries{};
    uint32_t depth{};
};
inline bool ResolveOrigin(const DrawOrigin& origin,uint64_t renderer,uint32_t instances,uint32_t stride,
    OriginRead reader,void* context,std::array<uint8_t,64>& output){
    return cvr_dor_resolve(&origin,renderer,instances,stride,reader,context,&output);
}
#else
inline DrawOrigin OriginFromEntry(const DrawEntryFrame& f,uint64_t base){
    if(!base || base>UINT64_MAX-0x335496 || f.caller!=base+0x335496 || !f.rcx || !f.rdx || !f.r8 ||
        f.rdx!=f.r14 || f.r8!=f.rbx || !f.rdi || !f.r15 || !f.r12 || f.r12>UINT32_MAX)return {};
    return {f.rcx,f.rdx,f.r8,f.rdi,f.r15,f.r12,true};
}
class DrawOriginStack {
public:
    void Enter(DrawOrigin value){if(depth<entries.size())entries[depth]=value;++depth;}
    void Leave(){if(depth)--depth;}
    const DrawOrigin* Current()const{return depth && depth<=entries.size()?&entries[depth-1]:nullptr;}
private:
    std::array<DrawOrigin,32> entries{};
    uint32_t depth{};
};
using OriginRead=bool(*)(void*,uint64_t,void*,size_t);
inline bool ResolveOrigin(const DrawOrigin& origin,uint64_t renderer,uint32_t instances,uint32_t stride,
    OriginRead reader,void* context,std::array<uint8_t,64>& output){
    if(!origin.valid || !renderer || !instances || (stride!=48 && stride!=64) || origin.renderer!=renderer || origin.instances!=instances)return false;
    auto read=[&](uint64_t address,void* out,size_t bytes){return address && address<=UINT64_MAX-bytes && reader && reader(context,address,out,bytes);};
    std::array<uint64_t,6> flush{};uint64_t draw{};std::array<uint64_t,2> packet{};
    if(!read(origin.flush,flush.data(),sizeof(flush)) || flush[0]!=origin.batch || flush[3]!=renderer || flush[4]!=origin.packet ||
        origin.batch>UINT64_MAX-0x50 || !read(origin.batch+0x48,&draw,8) || draw!=origin.draw ||
        !read(origin.packet,packet.data(),sizeof(packet)) || !(packet[1]&(uint64_t(1)<<51)) ||
        bool(packet[1]&(uint64_t(1)<<50))!=(stride==64))return false;
    // The skin flush precedes the ordinary R12 count load. Its live batch count
    // at +0x1C remains authoritative until return 0x335512 clears it.
    if(stride==64){uint32_t count{};if(!read(origin.batch+0x1C,&count,4) || count!=instances)return false;}
    return read(draw,output.data(),output.size()) && output[0]==0;
}
#endif
void EnterDrawOrigin(DrawOrigin);
void LeaveDrawOrigin();
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_GeometryOriginMode,CyberpunkVR_GeometryOriginCompatible,CyberpunkVR_GeometryOriginVerified;
// Equal legacy comparisons, mismatches, scoped resolutions, legacy-only resolutions.
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_GeometryOriginStats[4];
}
