#pragma once
#include "Render/NativeGraphSnapshot.hpp"
#include <set>
#include <span>

namespace cvr::stereo::graph_parent {
struct Code {uint32_t rva{},bytes{};uint64_t hash{};};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/native/graph_parent.rs through graph_parent_abi.rs
// (tools/single_pass_tests/graph_parent_test runs its cases against them in its Rust variant).
struct Context;struct Receipt;
extern "C" {
bool cvr_gp_certify(uint64_t,graph_snapshot::Read,const Code*,size_t);
bool cvr_gp_read_context(graph_snapshot::Read,uint64_t,Context*);
bool cvr_gp_resolve(const graph_snapshot::Node*,size_t,uint32_t,const Context*,Receipt*);
}
#endif
// One-shot original-image certificate, captured before installing our detours.
// A patched entry cannot be compared against the original whole-function hash.
// Caller serializes capture and preserves the identified image's lifetime.
class CodeReceipt {
public:
#if CVR_RUST_VRIK
    bool Capture(uint64_t base,graph_snapshot::Read read,std::span<const Code> functions){
        static_assert(sizeof(Code)==16,"mirrored by graph_parent.rs");
        if(attempted_)return false;attempted_=true;
        if(!cvr_gp_certify(base,read,functions.data(),functions.size()))return false;
        base_=base;verified_=true;return true;
    }
#else
    bool Capture(uint64_t base,graph_snapshot::Read read,std::span<const Code> functions){
        if(attempted_)return false;attempted_=true;
        if(!base || !read || functions.empty() || functions.size()>64)return false;
        std::array<uint8_t,512> bytes{};
        for(const auto& code:functions){
            if(!code.bytes || code.bytes>bytes.size() || base>UINT64_MAX-code.rva ||
                base+code.rva>UINT64_MAX-code.bytes || !read(base+code.rva,bytes.data(),code.bytes))return false;
            uint64_t hash=14695981039346656037ull;
            for(uint32_t i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=code.hash)return false;
        }
        base_=base;verified_=true;return true;
    }
#endif
    bool Attempted()const{return attempted_;}
    bool VerifiedFor(uint64_t base)const{return verified_ && base==base_;}
private:
    uint64_t base_{};bool attempted_{},verified_{};
};
struct Context {
    uint64_t address{},view{},manager{};
    uint32_t viewIndex{},slot{};
    bool operator==(const Context&)const=default;
};
struct Receipt {
    uint64_t object{},work{},name{};
    uint32_t handle{},ordinal{},viewIndex{},slot{};
};
// Native1ECF2C copies node metadata+5 to context+38 and resolves node metadata
// viewIndex through context.manager+48. Native1EC7EC copies both values into
// asynchronous child contexts. These are order slots, not inferred stage IDs.
#if CVR_RUST_VRIK
inline bool ReadContext(graph_snapshot::Read read,uint64_t address,Context& output) {
    static_assert(sizeof(Context)==32 && sizeof(Receipt)==40,"mirrored by graph_parent.rs");
    return cvr_gp_read_context(read,address,&output);
}
#else
inline bool ReadContext(graph_snapshot::Read read,uint64_t address,Context& output) {
    output={};
    const auto once=[&](Context& out){
        if(!read || !address || address>UINT64_MAX-0x40)return false;
        out.address=address;uint8_t slot{};uint64_t views{};uint32_t count{};
        if(!read(address+0x18,&out.view,8) || !read(address+0x20,&out.manager,8) || !read(address+0x38,&slot,1) ||
           !out.view || !out.manager || out.manager>UINT64_MAX-0x58 ||
           !read(out.manager+0x48,&views,8) || !read(out.manager+0x54,&count,4) || !views || !count || count>64 ||
           views>UINT64_MAX-uint64_t(count)*8)return false;
        uint32_t matches{};
        for(uint32_t i=0;i<count;++i){uint64_t view{};if(!read(views+uint64_t(i)*8,&view,8))return false;
            if(view==out.view){out.viewIndex=i;++matches;}}
        out.slot=slot;return matches==1;
    };
    Context a,b;if(!once(a) || !once(b) || a!=b)return false;output=a;return true;
}
#endif
// A source-bound classification only. It authorizes neither changed scheduling
// nor reuse of target allocations. Every native phase-1 slot must be unique.
#if CVR_RUST_VRIK
inline bool Resolve(const graph_snapshot::Snapshot& source,const Context& context,Receipt& output) {
    return cvr_gp_resolve(source.nodes.data(),source.nodes.size(),source.failure,&context,&output);
}
#else
inline bool Resolve(const graph_snapshot::Snapshot& source,const Context& context,Receipt& output) {
    output={};
    if(source.failure || source.nodes.empty() || source.nodes.size()>255 || !context.address || !context.view || !context.manager)return false;
    std::set<uint32_t> slots,ids;const graph_snapshot::Node* found{};
    for(const auto& node:source.nodes){
        const auto id=node.handle&0xDFFFFFFFu;
        const auto order=graph_snapshot::Field<uint32_t>(node.bytes,28);
        const auto slot=(order>>8)&255u;
        if(node.order>=source.nodes.size() || graph_snapshot::Field<uint32_t>(node.bytes,0)!=node.order ||
           id>=65536 || !ids.insert(id).second || slot>=source.nodes.size() || !slots.insert(slot).second)return false;
        if(slot==context.slot){if(graph_snapshot::Field<uint32_t>(node.bytes,24)!=context.viewIndex)return false;found=&node;}
    }
    if(!found || !found->work || !found->vtable || !graph_snapshot::Field<uint64_t>(found->bytes,16) ||
       graph_snapshot::Field<uint64_t>(found->definition,0)!=found->vtable)return false;
    output={graph_snapshot::Field<uint64_t>(found->bytes,16),found->work,graph_snapshot::Field<uint64_t>(found->definition,24),
            found->handle,found->order,context.viewIndex,context.slot};return true;
}
#endif
}
