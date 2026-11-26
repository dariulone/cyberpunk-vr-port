#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

namespace cvr::stereo {
struct NativeCullCandidate {
    uint64_t id{};
    std::array<float,6> bounds{};
    uint16_t includeMask{},excludeMask{};
    uint32_t reserved{};
};
static_assert(sizeof(NativeCullCandidate)==40);
struct NativeVisibleIds {
    uint64_t* data{};
    uint32_t capacity{},count{};
};
static_assert(sizeof(NativeVisibleIds)==16);
using NativeFineCull=char(*)(void*,int64_t*,char,void*);
inline constexpr size_t MaxFineCandidates=65536;
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/fine_visibility.rs (tools/single_pass_tests/cpu_collector_test runs its cases
// against them in its Rust variant).
extern "C" {
bool cvr_fv_count(uintptr_t,uintptr_t,size_t*);
bool cvr_fv_run(NativeFineCull,void*,const NativeCullCandidate*,size_t,char,uint64_t*,size_t,uint32_t*);
}
#endif

#if CVR_RUST_VRIK
inline bool FineCandidateCount(uintptr_t begin,uintptr_t end,size_t& count) {
    return cvr_fv_count(begin,end,&count);
}

// The native fine test appends at most one ID per candidate. Preallocate the
// whole output so the native container never grows or owns this allocation.
inline bool RunNativeFine(NativeFineCull fine,void* query,
    std::span<const NativeCullCandidate> candidates,char partial,std::vector<uint64_t>& ids) {
    ids.clear();
    if(fine && query && candidates.size()<=MaxFineCandidates)ids.resize(candidates.size());
    uint32_t count{};
    if(!cvr_fv_run(fine,query,candidates.data(),candidates.size(),partial,ids.data(),ids.size(),&count)){ids.clear();return false;}
    ids.resize(count);return true;
}
#else
inline bool FineCandidateCount(uintptr_t begin,uintptr_t end,size_t& count) {
    count=0;
    if(!begin || end<begin || (end-begin)%sizeof(NativeCullCandidate))return false;
    count=(end-begin)/sizeof(NativeCullCandidate);
    return count<=MaxFineCandidates;
}

// The native fine test appends at most one ID per candidate. Preallocate the
// whole output so the native container never grows or owns this allocation.
inline bool RunNativeFine(NativeFineCull fine,void* query,
    std::span<const NativeCullCandidate> candidates,char partial,std::vector<uint64_t>& ids) {
    ids.clear();
    if(!fine || !query || candidates.size()>MaxFineCandidates)return false;
    if(candidates.empty())return true;
    ids.resize(candidates.size());
    NativeVisibleIds output{ids.data(),static_cast<uint32_t>(ids.size()),0};
    int64_t range[]{static_cast<int64_t>(reinterpret_cast<uintptr_t>(candidates.data())),
        static_cast<int64_t>(reinterpret_cast<uintptr_t>(candidates.data()+candidates.size()))};
    fine(query,range,partial,&output);
    if(output.data!=ids.data() || output.capacity!=ids.size() || output.count>output.capacity){ids.clear();return false;}
    ids.resize(output.count);return true;
}
#endif
}
