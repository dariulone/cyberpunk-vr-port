#pragma once
#include "Render/NativeIaBinding.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/StereoGeometryCommands.hpp"
#include <array>
#include <climits>

namespace cvr::stereo::indirect_copy {
enum class Failure:uint32_t {
    None,Arguments,Scope,Code,Parent,Binding,State,Backing,Allocation,Allocator,
    Residency,Capacity,Changed,Ownership,Registration,Exception
};
namespace detail {
struct ScopeStamp {
    uint64_t list{},generation{},position{},nonce{};
    uint32_t thread{};
    bool operator==(const ScopeStamp&)const=default;
};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/indirect_copy.rs (tools/single_pass_tests/indirect_copy_contract_test runs its
// cases against them in its Rust variant). The ledger's members stay here (thread_local in its users), mirrored there.
struct Observation;struct Allocation;struct AllocationBinding;class ScopeLedger;
extern "C" {
void cvr_icp_scope(ScopeLedger*,uint32_t,uint64_t,uint64_t,uint64_t,uint32_t,ScopeStamp*);
bool cvr_icp_scope_exit(ScopeLedger*,const ScopeStamp*);
bool cvr_icp_scope_matches(const ScopeLedger*,const ScopeStamp*,uint64_t,uint64_t,uint64_t,uint32_t);
bool cvr_icp_read_mask(const geometry::ReadState*,uint32_t,bool);
bool cvr_icp_parent(const preparation::GraphParentRecord*);
uint32_t cvr_icp_admit(const Observation*,const preparation::GraphParentRecord*);
bool cvr_icp_allocation_form(const AllocationBinding*,const Allocation*,uint64_t,uint64_t);
}
class ScopeLedger {
public:
    ScopeStamp Enter(uint64_t list,uint64_t generation,uint64_t position,uint32_t thread)noexcept {
        ScopeStamp stamp;cvr_icp_scope(this,0,list,generation,position,thread,&stamp);return stamp;
    }
    bool Exit(const ScopeStamp& stamp)noexcept{return cvr_icp_scope_exit(this,&stamp);}
    ScopeStamp Current()const noexcept{ScopeStamp stamp;cvr_icp_scope(const_cast<ScopeLedger*>(this),1,0,0,0,0,&stamp);return stamp;}
    bool Matches(const ScopeStamp& stamp,uint64_t list,uint64_t generation,uint64_t position,uint32_t thread)const noexcept {
        return cvr_icp_scope_matches(this,&stamp,list,generation,position,thread);
    }
private:
    std::array<ScopeStamp,16> frames_{};
    uint64_t next_{};size_t depth_{};bool broken_{};
};
static_assert(sizeof(ScopeStamp)==40 && sizeof(ScopeLedger)==664,"mirrored by indirect_copy.rs");
inline bool TrackedClass(uint32_t kind)noexcept{return cvr_icp_read_mask(nullptr,kind,false);}
inline bool ReadMask(geometry::ReadState state,uint32_t kind,bool copyReady=false)noexcept{return cvr_icp_read_mask(&state,kind,copyReady);}
inline bool Parent(const preparation::GraphParentRecord& parent)noexcept{return cvr_icp_parent(&parent);}
struct Observation {
    geometry::NativeBuffer native;
    uint64_t list{},generation{},position{},resource{},backingGpu{},backingBytes{},offset{},bytes{},recordAddress{};
    uint32_t caller{},maxCount{},signatureKind{},stride{},heap{},flags{},pending{},contextKind{},lowType{};
    geometry::ReadState state;
    bool fullBacking{},parentCurrent{},recordingCurrent{},nativeOrigin{},sameDevice{};
};
static_assert(sizeof(geometry::NativeBuffer)==48 && sizeof(geometry::ReadState)==12 && sizeof(Observation)==176,
    "mirrored by indirect_copy.rs");
inline Failure Admit(const Observation& value,const preparation::GraphParentRecord& parent)noexcept{
    return Failure(cvr_icp_admit(&value,&parent));
}
struct Allocation {
    uint64_t address{},vtable{},pimpl{},bytes{},resource{},heap{},block{},handle{};
    uint32_t kind{};
    bool operator==(const Allocation&)const=default;
};
struct AllocationBinding {
    uint64_t allocation{},payload{},control{},flags{},alignment{},bytes{};
    bool alias{};
    bool operator==(const AllocationBinding&)const=default;
};
static_assert(sizeof(Allocation)==72 && sizeof(AllocationBinding)==56,"mirrored by indirect_copy.rs");
inline bool AllocationForm(const AllocationBinding& binding,const Allocation& allocation,uint64_t source,uint64_t bytes)noexcept{
    return cvr_icp_allocation_form(&binding,&allocation,source,bytes);
}
#else
class ScopeLedger {
public:
    ScopeStamp Enter(uint64_t list,uint64_t generation,uint64_t position,uint32_t thread)noexcept {
        if(broken_ || !list || !generation || !position || !thread || depth_==frames_.size() || next_==UINT64_MAX)return {};
        ScopeStamp stamp{list,generation,position,++next_,thread};frames_[depth_++]=stamp;return stamp;
    }
    bool Exit(const ScopeStamp& stamp)noexcept {
        if(!depth_ || frames_[depth_-1]!=stamp){broken_=true;return false;}
        frames_[--depth_]={};return true;
    }
    ScopeStamp Current()const noexcept{return !broken_ && depth_?frames_[depth_-1]:ScopeStamp{};}
    bool Matches(const ScopeStamp& stamp,uint64_t list,uint64_t generation,uint64_t position,uint32_t thread)const noexcept {
        return stamp.nonce && stamp.nonce==next_ && Current()==stamp && stamp.list==list && stamp.generation==generation &&
            stamp.position==position && stamp.thread==thread;
    }
private:
    std::array<ScopeStamp,16> frames_{};
    uint64_t next_{};size_t depth_{};bool broken_{};
};
inline bool TrackedClass(uint32_t kind)noexcept{return kind==0 || kind==3;}
inline bool ReadMask(geometry::ReadState state,uint32_t kind,bool copyReady=false)noexcept {
    constexpr uint32_t reads=D3D12_RESOURCE_STATE_GENERIC_READ;
    return TrackedClass(kind) && state.kind==kind && state.status==2 &&
        (state.value&D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT) && !(state.value&~reads) &&
        (!copyReady || (state.value&D3D12_RESOURCE_STATE_COPY_SOURCE));
}
inline bool Parent(const preparation::GraphParentRecord& parent)noexcept {
    return !parent.status && parent.context && parent.manager && parent.fingerprint && parent.object && parent.work &&
        parent.handle<65536 && parent.ordinal<220 && parent.slot<220 && parent.viewIndex<2;
}
struct Observation {
    geometry::NativeBuffer native;
    uint64_t list{},generation{},position{},resource{},backingGpu{},backingBytes{},offset{},bytes{},recordAddress{};
    uint32_t caller{},maxCount{},signatureKind{},stride{},heap{},flags{},pending{},contextKind{},lowType{};
    geometry::ReadState state;
    bool fullBacking{},parentCurrent{},recordingCurrent{},nativeOrigin{},sameDevice{};
};
inline Failure Admit(const Observation& value,const preparation::GraphParentRecord& parent)noexcept {
    if(!value.list || !value.generation || !value.position || !value.native.resource || !value.native.registry ||
        !value.native.context || !value.native.handle || value.native.handle>0x100000 || value.native.resource!=value.resource || value.lowType!=0xB ||
        value.caller!=0x342B11 || value.maxCount!=1 || value.signatureKind!=1 || value.stride!=20 || value.bytes!=20 || value.offset%4)
        return Failure::Arguments;
    const auto relative=0x5C0AF8+uint64_t(value.native.handle-1)*0xB0;
    if(value.native.registry>UINT64_MAX-relative-0xB0 || value.recordAddress!=value.native.registry+relative)return Failure::Arguments;
    if(!Parent(parent) || !value.parentCurrent)return Failure::Parent;
    if(!value.recordingCurrent || !value.nativeOrigin || !value.sameDevice)return Failure::Binding;
    if(value.pending || (value.contextKind!=1 && value.contextKind!=2) || !ReadMask(value.state,value.native.kind))return Failure::State;
    if(value.heap!=D3D12_HEAP_TYPE_DEFAULT || value.flags!=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS || !value.fullBacking ||
        !value.backingGpu || !value.backingBytes || value.backingGpu>UINT64_MAX-value.backingBytes ||
        value.native.gpu!=value.backingGpu || value.native.bytes!=value.backingBytes ||
        value.offset>value.backingBytes || value.bytes>value.backingBytes-value.offset)return Failure::Backing;
    return Failure::None;
}
struct Allocation {
    uint64_t address{},vtable{},pimpl{},bytes{},resource{},heap{},block{},handle{};
    uint32_t kind{};
    bool operator==(const Allocation&)const=default;
};
struct AllocationBinding {
    uint64_t allocation{},payload{},control{},flags{},alignment{},bytes{};
    bool alias{};
    bool operator==(const AllocationBinding&)const=default;
};
inline bool AllocationForm(const AllocationBinding& binding,const Allocation& allocation,uint64_t source,uint64_t bytes)noexcept {
    if(!source || !bytes || !binding.allocation || binding.allocation!=allocation.address || !allocation.pimpl || !allocation.bytes || allocation.bytes<bytes)return false;
    if(!binding.alias)return !binding.payload && !binding.control &&
        ((allocation.kind==0 && !allocation.heap) || (allocation.kind==1 && allocation.heap)) && allocation.resource==source;
    return binding.payload && binding.control && binding.flags==0xC0 && binding.alignment==0x10000 && binding.bytes==allocation.bytes &&
        (allocation.kind==1 || allocation.kind==2) && !allocation.resource && allocation.heap;
}
#endif
}

// Caller places this only around preparation/capture inside the exact original
// native source callback. A nested source occurrence invalidates its outer one.
// This scope labels an occurrence; it does not establish GPU alias ordering.
class ActualCallScope {
public:
    ActualCallScope(ID3D12GraphicsCommandList*,uint64_t generation,uint64_t position)noexcept;
    ~ActualCallScope();
    ActualCallScope(const ActualCallScope&)=delete;
    ActualCallScope& operator=(const ActualCallScope&)=delete;
    bool Active()const noexcept;
private:
    detail::ScopeStamp stamp_;
};
bool Compatible();
// Caller certifies a valid, unchanged ORIGINAL native read epoch: copy the same
// 20 bytes immediately before the original ExecuteIndirect on this native list,
// after its producer/alias/split-readiness barriers, with unchanged queue edges.
// No source use may escape that epoch; only the private snapshot is used later.
// Full backing, parent identity and allocation retention do not prove this epoch.
// Native combined-read registration occurs only after all ownership is prepared.
bool PrepareAtActualCall(ID3D12GraphicsCommandList*,const geometry::IndirectBinding&,
    const preparation::GraphParentRecord&,uint64_t position,IndirectCopyProof&);
// Read-only, current occurrence only. Rechecks the new ready native read state;
// it never registers use, edits maps, emits barriers, recaptures or restores state.
bool Validate(ID3D12GraphicsCommandList*,const IndirectCopyProof&,UINT64 offset,UINT64 bytes);
// Queue validation is separate from current-callback Validate. It is called in
// the native BeforeSubmit window; caller retains the proof through the real fence.
bool ValidateSubmission(const IndirectCopyProof&,ID3D12CommandQueue*);
uint32_t LastFailure();
}
