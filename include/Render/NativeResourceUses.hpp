#pragma once
#include "Render/NativeGraphSnapshot.hpp"
#include <array>
#include <cstdint>
#include <cstring>
#include <map>
#include <set>
#include <utility>
#include <vector>

namespace cvr::stereo::resource_uses {
using ReadMemory=bool(*)(void*,uint64_t,void*,size_t);
// 2.5 (sub_140334A34, sub_1403E0330, sub_1403EB51C; 2.31 sub_1401F0F80, sub_1401EDA84, sub_14024CAC8): a slot
// grew from 0x5E00 to 0x70C0 bytes (600 declarations instead of 500), the tables behind the 220 slots moved with it.
inline constexpr uint64_t SlotStride=0x70c0,Declarations=0x48,DeclarationCount=0x70c8;
inline constexpr uint64_t UseCount=0x60e540,UseIds=0x60e548,AuxCount=0x616548,AuxKeys=0x61654c;
inline constexpr uint64_t BoundaryCount=0x61664c,BoundaryIds=0x616650;
inline constexpr uint32_t MaxSlots=220,MaxDeclarations=600,MaxUses=4096,MaxAuxKeys=64,MaxBoundaries=24;
static_assert(SlotStride*MaxSlots==0x60e500 && Declarations+48*MaxDeclarations==DeclarationCount);
static_assert(UseIds+8*MaxUses==AuxCount && AuxKeys+4*MaxAuxKeys==BoundaryCount && BoundaryIds+8*MaxBoundaries==0x616710);
enum class Failure:uint32_t {None,Arguments,SourceGraph,Header,Limit,Ids,Slot,Index,Declaration,Changed,Allocation};
struct Declaration {
    uint32_t slot{},index{},key{},kind{},nodeHandle{},nodeOrder{};
    std::array<uint8_t,48> bytes{};
    bool operator==(const Declaration&)const=default;
};
static_assert(sizeof(Declaration)==72);
#if CVR_RUST_VRIK
// The capture: rust/crates/cvr-render/src/native/resource_uses.rs through resource_uses_abi.rs
// (tools/single_pass_tests/resource_uses_test runs its cases against it in its Rust variant).
struct RustUses;
struct RustUsesOut {
    uint64_t pool,contextRoot;std::array<uint8_t,4> poolHeader;uint32_t failure;std::array<uint32_t,14> kinds;bool opaque;
    const graph_snapshot::Node* sourceNodes;size_t sourceCount;const uint64_t* useIds;size_t useCount;
    const uint64_t* boundaryIds;size_t boundaryCount;const uint32_t* auxiliaryKeys;size_t auxiliaryCount;
    const Declaration* declarations;size_t declarationCount;
};
static_assert(sizeof(RustUsesOut)==168,"mirrored by resource_uses_abi.rs");
extern "C" {
RustUses* cvr_ru_capture(uint64_t,const graph_snapshot::Node*,size_t,uint32_t,ReadMemory,void*,uint64_t,RustUsesOut*);
void cvr_ru_free(RustUses*);
}
#endif
struct Snapshot {
    uint64_t pool{},contextRoot{};
    std::array<uint8_t,4> poolHeader{};
    std::vector<graph_snapshot::Node> sourceNodes;
    std::vector<uint64_t> useIds,boundaryIds;
    std::vector<uint32_t> auxiliaryKeys;
    std::vector<Declaration> declarations;
    std::array<uint32_t,14> kinds{};
    // These declarations describe native lifetimes, not GPU read/write access.
    // Kind7 relates keys; kind8 imports a native handle, not another key.
    // Neither these relationships nor key equality prove a reorder safe.
    bool opaqueStateUnsupported=true;
    Failure failure=Failure::Arguments;

    // Call at native1EDA84(mode2) entry before the original compiler, while
    // pool+2 is1 and native declaration jobs have completed. sourceGraph is
    // the caller-owned exact graph used by those jobs, not a frame-context ptr.
    // No native pointer in a captured declaration is dereferenced or retained.
#if CVR_RUST_VRIK
    bool CapturePool(uint64_t address,const graph_snapshot::Snapshot& graph,ReadMemory read,
                     void* user=nullptr,uint64_t root=0)try{
        *this={};RustUsesOut out{};
        const auto captured=cvr_ru_capture(address,graph.nodes.data(),graph.nodes.size(),graph.failure,read,user,root,&out);
        struct Release{RustUses* uses;~Release(){cvr_ru_free(uses);}}release{captured};
        // A refusal leaves the reset snapshot with its reason, as by the C++ rules.
        pool=out.pool;contextRoot=out.contextRoot;poolHeader=out.poolHeader;kinds=out.kinds;opaqueStateUnsupported=out.opaque;
        sourceNodes.assign(out.sourceNodes,out.sourceNodes+out.sourceCount);useIds.assign(out.useIds,out.useIds+out.useCount);
        boundaryIds.assign(out.boundaryIds,out.boundaryIds+out.boundaryCount);
        auxiliaryKeys.assign(out.auxiliaryKeys,out.auxiliaryKeys+out.auxiliaryCount);
        declarations.assign(out.declarations,out.declarations+out.declarationCount);
        failure=Failure(out.failure);return failure==Failure::None;
    }catch(...){*this={};failure=Failure::Allocation;return false;}
#else
    bool CapturePool(uint64_t address,const graph_snapshot::Snapshot& graph,ReadMemory read,
                     void* user=nullptr,uint64_t root=0)try{
        *this={};
        auto reject=[&](Failure reason){*this={};failure=reason;return false;};
        auto memory=[&](uint64_t offset,void* out,size_t size){
            return address && read && offset<=UINT64_MAX-address && size<=UINT64_MAX-address-offset &&
                read(user,address+offset,out,size);
        };
        if(!address || !read || address>UINT64_MAX-(BoundaryIds+uint64_t(MaxBoundaries)*8))return reject(Failure::Arguments);
        if(graph.failure || graph.nodes.empty() || graph.nodes.size()>MaxSlots)return reject(Failure::SourceGraph);
        std::array<const graph_snapshot::Node*,MaxSlots> nodes{};std::set<uint32_t> handles;
        for(size_t i=0;i<graph.nodes.size();++i){const auto& node=graph.nodes[i];
            const auto id=node.handle&0xdfffffffu,slot=uint32_t(node.bytes[0x1d]);
            if(id>=65536 || (node.handle!=id && node.handle!=(id|0x20000000u)) || node.order!=i ||
                graph_snapshot::Field<uint32_t>(node.bytes,0)!=i || !handles.insert(id).second || slot>=MaxSlots || nodes[slot])
                return reject(Failure::SourceGraph);
            nodes[slot]=&node;
        }
        Snapshot next;next.pool=address;next.contextRoot=root;next.sourceNodes=graph.nodes;
        uint32_t uses{},auxiliary{},boundaries{};
        if(!memory(0,next.poolHeader.data(),4) || next.poolHeader[2]!=1 ||
            !memory(UseCount,&uses,4) || !memory(AuxCount,&auxiliary,4) || !memory(BoundaryCount,&boundaries,4))
            return reject(Failure::Header);
        if(uses>MaxUses || auxiliary>MaxAuxKeys || boundaries>MaxBoundaries)return reject(Failure::Limit);
        next.useIds.resize(uses);next.auxiliaryKeys.resize(auxiliary);next.boundaryIds.resize(boundaries);
        if((uses && !memory(UseIds,next.useIds.data(),uses*8ull)) ||
            (auxiliary && !memory(AuxKeys,next.auxiliaryKeys.data(),auxiliary*4ull)) ||
            (boundaries && !memory(BoundaryIds,next.boundaryIds.data(),boundaries*8ull)))return reject(Failure::Ids);
        std::map<uint32_t,uint32_t> counts;std::map<uint32_t,size_t> rows;
        for(const auto value:next.useIds){
            const auto slot=uint32_t((value>>16)&65535),index=uint32_t(value&65535),key=uint32_t(value>>32);
            if(slot>=MaxSlots || !nodes[slot])return reject(Failure::Slot);
            if(!counts.contains(slot)){
                uint32_t count{};if(!memory(SlotStride*slot+DeclarationCount,&count,4))return reject(Failure::Declaration);
                if(count>MaxDeclarations)return reject(Failure::Limit);
                counts.emplace(slot,count);
            }
            if(index>=counts.at(slot) || index>=MaxDeclarations)return reject(Failure::Index);
            const auto ordinal=(slot<<16)|index;
            const auto found=rows.find(ordinal);
            if(found!=rows.end()){
                if(next.declarations[found->second].key!=key)return reject(Failure::Declaration);
                continue;
            }
            Declaration row;row.slot=slot;row.index=index;row.key=key;
            row.nodeHandle=nodes[slot]->handle;row.nodeOrder=nodes[slot]->order;
            if(!memory(SlotStride*slot+Declarations+uint64_t(index)*48,row.bytes.data(),row.bytes.size()) ||
                row.bytes[1]!=slot || graph_snapshot::Field<uint32_t>(row.bytes,4)!=key)return reject(Failure::Declaration);
            row.kind=row.bytes[0];if(row.kind<next.kinds.size())++next.kinds[row.kind];
            rows.emplace(ordinal,next.declarations.size());next.declarations.push_back(row);
        }
        for(const auto value:next.boundaryIds){
            if(value>>32 || !rows.contains(uint32_t(value)))return reject(Failure::Ids);
            const auto& row=next.declarations[rows.at(uint32_t(value))];
            const auto subtype=graph_snapshot::Field<uint32_t>(row.bytes,8);
            if(row.kind!=11 || (subtype!=1 && subtype!=2))return reject(Failure::Declaration);
        }
        for(const auto& row:next.declarations){std::array<uint8_t,48> again{};
            if(!memory(SlotStride*row.slot+Declarations+uint64_t(row.index)*48,again.data(),again.size()) || again!=row.bytes)
                return reject(Failure::Changed);
        }
        for(const auto& [slot,count]:counts){uint32_t again{};
            if(!memory(SlotStride*slot+DeclarationCount,&again,4) || again!=count)return reject(Failure::Changed);
        }
        std::vector<uint64_t> idsAgain(uses),boundsAgain(boundaries);std::vector<uint32_t> auxAgain(auxiliary);
        if((uses && (!memory(UseIds,idsAgain.data(),uses*8ull) || idsAgain!=next.useIds)) ||
            (auxiliary && (!memory(AuxKeys,auxAgain.data(),auxiliary*4ull) || auxAgain!=next.auxiliaryKeys)) ||
            (boundaries && (!memory(BoundaryIds,boundsAgain.data(),boundaries*8ull) || boundsAgain!=next.boundaryIds)))
            return reject(Failure::Changed);
        std::array<uint8_t,4> headerAgain{};uint32_t usesAgain{},auxiliaryAgain{},boundariesAgain{};
        if(!memory(0,headerAgain.data(),4) || headerAgain!=next.poolHeader ||
            !memory(UseCount,&usesAgain,4) || usesAgain!=uses || !memory(AuxCount,&auxiliaryAgain,4) || auxiliaryAgain!=auxiliary ||
            !memory(BoundaryCount,&boundariesAgain,4) || boundariesAgain!=boundaries)return reject(Failure::Changed);
        next.failure=Failure::None;*this=std::move(next);return true;
    }catch(...){*this={};failure=Failure::Allocation;return false;}
#endif
};
inline bool CapturePool(uint64_t pool,const graph_snapshot::Snapshot& graph,ReadMemory read,Snapshot& out,
                        void* user=nullptr,uint64_t contextRoot=0){return out.CapturePool(pool,graph,read,user,contextRoot);}
}
