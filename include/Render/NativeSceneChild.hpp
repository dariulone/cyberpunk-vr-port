#pragma once
#include "Render/NativeCommandRecording.hpp"
#include <atomic>
#include <memory>
#include <span>
#include <type_traits>

namespace cvr::stereo::scene_child {
inline constexpr uint32_t MaxDepth=32;
struct Identity {
    uint64_t consumerId{},epoch{},pairGeneration{},frameOwner{},sourceGraph{},executionGraph{};
    uint64_t rootObject{},childObject{},childVtable{},childWork{},workContext{},invocation{},parentInvocation{};
    uint32_t rootHandle{},view{},section{},childSlot{},thread{},phase{},status{},reserved{};
    bool operator==(const Identity&)const=default;
};
static_assert(sizeof(Identity)==136 && std::is_trivially_copyable_v<Identity>);
namespace detail {
struct ChildKey {uint64_t object{},vtable{},work{};uint32_t slot{};};
// Section IDs match SceneGraphExpansion mappings. Selection is deliberately
// restricted to peer tail and main prefix, not every node of either eye.
#if CVR_RUST_VRIK
// The rule: rust/crates/cvr-render/src/scene_child.rs (tools/single_pass_tests/scene_child_test runs its cases
// against it in its Rust variant).
extern "C" bool cvr_sc_unique_child(const ChildKey*,size_t,uint32_t,uint32_t,const ChildKey*,uint32_t*);
inline bool UniqueChild(std::span<const ChildKey> children,uint32_t view,uint32_t section,
                        const ChildKey& observed,uint32_t& slot)noexcept{
    static_assert(sizeof(ChildKey)==32,"mirrored by scene_child.rs");
    return cvr_sc_unique_child(children.data(),children.size(),view,section,&observed,&slot);
}
#else
inline bool UniqueChild(std::span<const ChildKey> children,uint32_t view,uint32_t section,
                        const ChildKey& observed,uint32_t& slot)noexcept{
    slot=UINT32_MAX;
    if(children.size()!=11 || !observed.object || !observed.vtable || !observed.work ||
        !((view==1 && section==2) || (view==0 && section==1)))return false;
    for(size_t i=0;i<children.size();++i)
        if(children[i].slot!=i || !children[i].object || !children[i].vtable || !children[i].work)return false;
    const uint32_t begin=section==2?6u:0u,end=section==2?11u:6u;
    uint32_t matches{};
    for(uint32_t i=begin;i<end;++i){const auto& child=children[i];
        if(child.object==observed.object && child.vtable==observed.vtable && child.work==observed.work){slot=i;++matches;}
    }
    if(matches!=1){slot=UINT32_MAX;return false;}return true;
}
#endif
}
struct Receipt;
// Scope owns only C++ attribution and a live Consumer reference. It grants no
// permission to suppress commands, borrow resources or change native lifetime.
class Scope {
public:
    Scope(uint64_t node,uint64_t workContext,uint32_t phase=0)noexcept;
    ~Scope();
    Scope(const Scope&)=delete;
    Scope& operator=(const Scope&)=delete;
    Scope(Scope&&)=delete;
    Scope& operator=(Scope&&)=delete;
private:
    std::shared_ptr<const Receipt> previous_;
    uint32_t previousDepth_{};
};
bool DescribeCurrent(Identity&)noexcept;
// Acquire the actual native recording at the D3D call, not dispatcher entry.
// False leaves both outputs empty and never infers a list from a prior child.
bool DescribeCurrentAtList(ID3D12GraphicsCommandList*,Identity&,native_recording::Lease&)noexcept;
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SceneChildEnable;
}
