#pragma once
#include "Render/GpuMultilayerMembership.hpp"
#include <memory>

namespace cvr::stereo::prepared_pair {struct Source;}
namespace cvr::stereo::prepared_membership {
enum class Failure:UINT {None,Arguments,Family,Scope,B4,T5,Table,Coverage,Collision,Material,Capacity,Exception};
struct Summary {
    uint64_t ordinal{};UINT nativeSide{},sourcePassStorageExtent{},materialSlots{},coveredSlots{},immutableSlots{},nullSlots{},omittedSlots{};
    UINT maximumBit=UINT_MAX;
};
class Bank {
    struct Data;std::shared_ptr<const Data> data_;
    friend bool Build(const prepared_pair::Source&,Bank&,Failure&);
public:
    explicit operator bool()const{return bool(data_);}
    const MultilayerMembershipSource* Source()const;
    const Summary* Statistics()const;
};
// CPU-only descriptor/input bridge. Each set bit is both physically represented
// in the authenticated SourcePass storage and owned by this source's exact
// material/null receipt. No expected shader bytes or selector prediction is used.
// Source copies still require producer ordering/read state and SDK residency.
// This proves SourcePass coverage ONLY: any paired shared table must separately
// cover maximumBit; neither a successful Build nor a GPU membership match grants
// suppression, native resource access or a valid fallback on check failure.
bool Build(const prepared_pair::Source&,Bank&,Failure&);
namespace detail {
#if CVR_RUST_VRIK
// The rule: rust/crates/cvr-render/src/multilayer_selectors.rs (tools/single_pass_tests/prepared_membership_bank_test
// runs its cases against it in its Rust variant).
extern "C" bool cvr_ml_mark(std::array<UINT,1024>*,UINT,UINT,UINT*,UINT*);
inline bool Mark(std::array<UINT,1024>& bits,UINT offset,UINT copiedExtent,UINT& count,UINT& maximum){
    return cvr_ml_mark(&bits,offset,copiedExtent,&count,&maximum);
}
#else
inline bool Mark(std::array<UINT,1024>& bits,UINT offset,UINT copiedExtent,UINT& count,UINT& maximum){
    if(!copiedExtent || offset>=copiedExtent || offset>=multilayer_selectors::DeclaredExtent)return false;
    auto& word=bits[offset/32];const auto mask=UINT(1)<<(offset%32);
    if(!(word&mask)){word|=mask;++count;maximum=maximum==UINT_MAX?offset:std::max(maximum,offset);}return true;
}
#endif
}
}
