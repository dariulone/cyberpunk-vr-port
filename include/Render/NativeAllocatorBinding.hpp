#pragma once
#include "Render/NativeDefinitionTree.hpp"
#include <set>

namespace cvr::stereo::allocator_binding {
using ReadMemory=definition_tree::ReadMemory;
inline constexpr uint32_t CommonRva=0x3BCDD80,LivePoolVtableRva=0x3572028;
inline constexpr uint32_t PoolOffset=0xC928,SmallBeginOffset=0xC900,SmallEndOffset=0xC908;
inline constexpr uint32_t MiddleBeginOffset=0xCDA8,MiddleEndOffset=0xCDB0;
inline constexpr uint32_t VectorAllocatorRva=0x30947E8,VectorFreeRva=0x120F6F0;
struct Lifecycle {
    uint64_t incarnation{},resetEpoch{};
    uint32_t inFlight{};
    bool closing{},coverageComplete{};
    bool operator==(const Lifecycle&)const=default;
};
using ObserveLifecycle=bool(*)(void*,Lifecycle&);
struct LayoutFacts {
    bool globalsAndComponentGuarded{},allocationRoutesGuarded{},exactExtentFactsAudited{};
};
enum class Region:uint32_t {Unsupported,Small,Middle};
enum class Kind:uint32_t {TopVector,Object,Control,Array,CString};
enum class ExtentKind:uint32_t {ParsedMinimum,ExactAllocation};
struct ExtentFact {Kind kind{};uint64_t address{},bytes{};};
struct Allocation {
    Kind kind{};Region region=Region::Unsupported;ExtentKind extent=ExtentKind::ParsedMinimum;
    uint64_t address{},bytes{},parsedMinimum{};
    bool operator==(const Allocation&)const=default;
};
struct Bounds {
    uint64_t smallBegin{},smallEnd{},middleBegin{},middleEnd{};
    bool operator==(const Bounds&)const=default;
};
enum class Failure:uint32_t {
    None,Arguments,Layout,Tracking,Read,Changed,Globals,Component,Bounds,Tree,Header,
    FreeRoute,ExtentFacts,UnsupportedPointer,ExtentOverflow,ExtentCrossesRegion,Overlap,Allocation
};
#if CVR_RUST_VRIK
// The receipt's rules: rust/crates/cvr-render/src/native/allocator_binding.rs through definition_tree_abi.rs
// (tools/single_pass_tests/allocator_binding_test runs its cases against them in its Rust variant).
struct RustIdentity {std::array<uint64_t,3> globals;uint64_t vptr;Bounds bounds;};
struct RustAllocations;
extern "C" {
bool cvr_ab_capture(uint64_t,const LayoutFacts*,const Lifecycle*,ReadMemory,ObserveLifecycle,void*,RustIdentity*,uint32_t*);
RustAllocations* cvr_ab_bind_saved(bool,uint64_t,const RustIdentity*,const LayoutFacts*,const definition_tree::NativeVector16*,
    bool,uint64_t,const definition_tree::RustView*,const ExtentFact*,size_t,uint32_t*);
const Allocation* cvr_ab_allocations(const RustAllocations*,size_t*);
void cvr_ab_free(RustAllocations*);
uint32_t cvr_ab_classify_pointer(const Bounds*,bool,uint64_t);
uint32_t cvr_ab_classify_span(const Bounds*,bool,uint64_t,uint64_t);
}
#endif

// Structural identity/range receipt only. Version guards and lifecycle samples
// come from the caller's allocator gate. Neither readable globals nor a live
// vptr prove birth, native liveness, allocation ownership, or native-free rights.
// Node/control sizes are parsed minimums unless an audited exact fact is supplied.
class Receipt {
public:
#if CVR_RUST_VRIK
    bool Capture(uint64_t imageBase,LayoutFacts facts,Lifecycle expected,ReadMemory read,
                 ObserveLifecycle observe,void* trackingContext)try{
        static_assert(sizeof(Identity)==sizeof(RustIdentity) && sizeof(Identity)==64 && sizeof(Lifecycle)==24 &&
            sizeof(Allocation)==40 && sizeof(ExtentFact)==24,"mirrored by allocator_binding.rs");
        *this=Receipt{};uint32_t failure=0;Identity identity;
        const bool captured=cvr_ab_capture(imageBase,&facts,&expected,read,observe,trackingContext,
            reinterpret_cast<RustIdentity*>(&identity),&failure);
        failure_=Failure(failure);if(!captured)return false;
        imageBase_=imageBase;identity_=identity;lifecycle_=expected;facts_=facts;captured_=true;return true;
    }catch(...){*this=Receipt{};failure_=Failure::Allocation;return false;}
#else
    bool Capture(uint64_t imageBase,LayoutFacts facts,Lifecycle expected,ReadMemory read,
                 ObserveLifecycle observe,void* trackingContext)try{
        *this=Receipt{};failure_=Failure::Arguments;
        if(!read || !observe || !Address(imageBase,CommonRva+MiddleEndOffset+8) || (imageBase&7))return false;
        failure_=Failure::Layout;if(!facts.globalsAndComponentGuarded || !facts.allocationRoutesGuarded)return false;
        failure_=Failure::Tracking;if(!Open(expected))return false;
        Lifecycle before,after;
        if(!observe(trackingContext,before) || before!=expected || !Open(before))return false;
        Identity first,second;
        if(!ReadIdentity(imageBase,read,first) || !ReadIdentity(imageBase,read,second))return false;
        failure_=Failure::Changed;if(first!=second)return false;
        failure_=Failure::Tracking;
        if(!observe(trackingContext,after) || after!=expected || !Open(after))return false;
        imageBase_=imageBase;identity_=first;lifecycle_=expected;facts_=facts;captured_=true;
        failure_=Failure::None;return true;
    }catch(...){*this=Receipt{};failure_=Failure::Allocation;return false;}
#endif

    // Caller supplies the exact retained owner header whose validated entry
    // slots produced this tree. Matching count is not an entry/ownership proof.
#if CVR_RUST_VRIK
    bool BindSaved(definition_tree::NativeVector16 header,const definition_tree::Snapshot& tree,
                   std::span<const ExtentFact> exactExtents={})try{
        savedBound_=false;savedHeader_={};allocations_.clear();uint32_t failure=0;
        const definition_tree::RustView view{tree.Roots().data(),tree.Roots().size(),tree.Nodes().data(),tree.Nodes().size(),
            tree.Handles().data(),tree.Handles().size(),tree.Arrays().data(),tree.Arrays().size(),
            tree.Strings().data(),tree.Strings().size(),tree.Allocators().data()};
        const auto bound=cvr_ab_bind_saved(captured_,imageBase_,reinterpret_cast<const RustIdentity*>(&identity_),&facts_,
            &header,tree.Ready(),tree.ImageBase(),&view,exactExtents.data(),exactExtents.size(),&failure);
        failure_=Failure(failure);if(!bound)return false;
        struct Release{RustAllocations* bound;~Release(){cvr_ab_free(bound);}}release{bound};
        size_t count=0;const auto allocations=cvr_ab_allocations(bound,&count);
        allocations_.assign(allocations,allocations+count);
        savedHeader_=header;savedBound_=true;failure_=Failure::None;return true;
    }catch(...){allocations_.clear();savedBound_=false;failure_=Failure::Allocation;return false;}
#else
    bool BindSaved(definition_tree::NativeVector16 header,const definition_tree::Snapshot& tree,
                   std::span<const ExtentFact> exactExtents={})try{
        savedBound_=false;savedHeader_={};allocations_.clear();failure_=Failure::Tree;
        if(!captured_ || !tree.Ready() || tree.ImageBase()!=imageBase_ || tree.Roots().size()!=header.count)return false;
        for(size_t i=0;i<2;++i){const auto& allocator=tree.Allocators()[i];
            const auto rva=i?definition_tree::ControlAllocatorRva:definition_tree::ObjectAllocatorRva;
            if(allocator.address!=imageBase_+rva || allocator.raw!=identity_.globals[i] || allocator.allocator!=Common())return false;
        }
        failure_=Failure::Header;
        if(header.count>header.capacity || !Address(header.data,1) || (header.data&7))return false;
        if(!header.capacity && header.data!=imageBase_+VectorAllocatorRva)return false;
        failure_=Failure::ExtentFacts;
        if(!exactExtents.empty() && !facts_.exactExtentFactsAudited)return false;
        std::map<std::pair<Kind,uint64_t>,ExtentFact> facts;
        for(const auto& fact:exactExtents){
            if((fact.kind!=Kind::Object && fact.kind!=Kind::Control) || !fact.bytes ||
                !facts.emplace(std::make_pair(fact.kind,fact.address),fact).second)return false;
        }
        std::vector<Allocation> next;
        if(header.capacity && !Add(next,Kind::TopVector,header.data,uint64_t(header.capacity)*8+8,ExtentKind::ExactAllocation))return false;
        std::set<uint64_t> controls;
        for(const auto& node:tree.Nodes()){
            const uint64_t minimum=node.kind==definition_tree::Kind::SceneDriver?0x38:
                node.kind==definition_tree::Kind::RenderElements || node.kind==definition_tree::Kind::CString?0x40:
                node.kind==definition_tree::Kind::DescriptionOwner?0x30:8;
            if(!AddNode(next,Kind::Object,node.object,minimum,facts))return false;
            if(node.controlBlock)controls.insert(node.controlBlock);
        }
        for(const auto& handle:tree.Handles())if(handle.controlBlock)controls.insert(handle.controlBlock);
        for(const auto control:controls)if(!AddNode(next,Kind::Control,control,8,facts))return false;
        for(const auto& array:tree.Arrays()){
            failure_=Failure::FreeRoute;
            if(array.allocator!=imageBase_+VectorAllocatorRva || array.freeCallback!=imageBase_+VectorFreeRva ||
                array.count>array.capacity || (array.stride!=16 && array.stride!=48))return false;
            if(!array.capacity){if(array.data!=array.allocator || array.trailerAddress)return false;continue;}
            const uint64_t payload=uint64_t(array.capacity)*array.stride;
            failure_=Failure::ExtentOverflow;if(payload>UINT64_MAX-15)return false;
            const uint64_t rounded=(payload+7)&~uint64_t(7);
            if(array.data>UINT64_MAX-rounded || array.trailerAddress!=array.data+rounded)return false;
            if(!Add(next,Kind::Array,array.data,rounded+8,ExtentKind::ExactAllocation))return false;
        }
        for(const auto& string:tree.Strings()){
            failure_=Failure::FreeRoute;
            if(string.mode!=1 || !string.capacity || string.allocatorValue || string.freeCallback ||
                string.fallbackGlobalAddress!=imageBase_+definition_tree::StringAllocatorRva ||
                string.fallbackRaw!=identity_.globals[2] || string.fallbackAllocator!=Common() ||
                string.fallbackFreeAdapter!=imageBase_+definition_tree::StringFreeAdapterRva)return false;
            const uint64_t bytes=(uint64_t(string.capacity)+8)&~uint64_t(7);
            if(string.allocationBytes!=bytes || !Add(next,Kind::CString,string.data,bytes,ExtentKind::ExactAllocation))return false;
        }
        failure_=Failure::ExtentFacts;if(!facts.empty())return false;
        savedHeader_=header;allocations_=std::move(next);savedBound_=true;failure_=Failure::None;return true;
    }catch(...){allocations_.clear();savedBound_=false;failure_=Failure::Allocation;return false;}
#endif

    bool MatchesBefore(ReadMemory read,ObserveLifecycle observe,void* trackingContext)const{
        if(!captured_ || failure_!=Failure::None)return false;
        Receipt current;
        return current.Capture(imageBase_,facts_,lifecycle_,read,observe,trackingContext) &&
            current.identity_==identity_ && current.lifecycle_==lifecycle_;
    }
    bool MatchesFresh(const Receipt& current)const noexcept{
        if(!savedBound_ || failure_!=Failure::None || !current.captured_ || current.failure_!=Failure::None ||
            imageBase_!=current.imageBase_ || lifecycle_!=current.lifecycle_ || identity_!=current.identity_)return false;
        for(const auto& allocation:allocations_)if(current.ClassifySpan(allocation.address,allocation.bytes)!=allocation.region)return false;
        return true;
    }
    // Mirrors 14455C's free-pointer route; not a statement about object extents.
#if CVR_RUST_VRIK
    Region ClassifyPointer(uint64_t address)const noexcept{
        return Region(cvr_ab_classify_pointer(&identity_.bounds,captured_,address));
    }
#else
    Region ClassifyPointer(uint64_t address)const noexcept{
        if(!captured_ || !address)return Region::Unsupported;
        const auto& bounds=identity_.bounds;
        if(address>=bounds.smallBegin && address<bounds.smallEnd)return Region::Small;
        if(address>=bounds.middleBegin && address<bounds.middleEnd)return Region::Middle;
        return Region::Unsupported;
    }
#endif
#if CVR_RUST_VRIK
    Region ClassifySpan(uint64_t address,uint64_t bytes)const noexcept{
        return Region(cvr_ab_classify_span(&identity_.bounds,captured_,address,bytes));
    }
#else
    Region ClassifySpan(uint64_t address,uint64_t bytes)const noexcept{
        if(!bytes || address>UINT64_MAX-bytes)return Region::Unsupported;
        const auto region=ClassifyPointer(address);const auto end=address+bytes;
        if(region==Region::Small && end<=identity_.bounds.smallEnd)return region;
        if(region==Region::Middle && end<=identity_.bounds.middleEnd)return region;
        return Region::Unsupported;
    }
#endif
    bool Captured()const noexcept{return captured_;}
    bool SavedBound()const noexcept{return savedBound_;}
    bool AllExactExtents()const noexcept{
        return savedBound_ && !allocations_.empty() && std::all_of(allocations_.begin(),allocations_.end(),
            [](const Allocation& allocation){return allocation.extent==ExtentKind::ExactAllocation;});
    }
    uint64_t ImageBase()const noexcept{return imageBase_;}
    uint64_t Common()const noexcept{return captured_?imageBase_+CommonRva:0;}
    uint64_t ComponentVptr()const noexcept{return identity_.vptr;}
    const std::array<uint64_t,3>& Globals()const noexcept{return identity_.globals;}
    const Bounds& Ranges()const noexcept{return identity_.bounds;}
    const Lifecycle& Tracking()const noexcept{return lifecycle_;}
    const definition_tree::NativeVector16& SavedHeader()const noexcept{return savedHeader_;}
    const std::vector<Allocation>& Allocations()const noexcept{return allocations_;}
    Failure Rejection()const noexcept{return failure_;}
private:
    struct Identity {
        std::array<uint64_t,3> globals{};uint64_t vptr{};Bounds bounds;
        bool operator==(const Identity&)const=default;
    };
#if !CVR_RUST_VRIK
    static bool Address(uint64_t address,uint64_t bytes)noexcept{
        return address && address<=UINT64_MAX-bytes && address<=std::numeric_limits<uintptr_t>::max();
    }
    static bool Open(const Lifecycle& value)noexcept{
        return value.incarnation && !value.inFlight && !value.closing && value.coverageComplete;
    }
    bool ReadIdentity(uint64_t base,ReadMemory read,Identity& out){
        const uint64_t common=base+CommonRva;
        constexpr uint32_t globals[]{definition_tree::ObjectAllocatorRva,definition_tree::ControlAllocatorRva,definition_tree::StringAllocatorRva};
        for(size_t i=0;i<3;++i){failure_=Failure::Read;if(!read(base+globals[i],&out.globals[i],8))return false;
            failure_=Failure::Globals;if((out.globals[i]&~uint64_t(7))!=common)return false;
        }
        failure_=Failure::Read;if(!read(common+PoolOffset,&out.vptr,8))return false;
        failure_=Failure::Component;if(out.vptr!=base+LivePoolVtableRva)return false;
        failure_=Failure::Read;
        if(!read(common+SmallBeginOffset,&out.bounds.smallBegin,8) || !read(common+SmallEndOffset,&out.bounds.smallEnd,8) ||
            !read(common+MiddleBeginOffset,&out.bounds.middleBegin,8) || !read(common+MiddleEndOffset,&out.bounds.middleEnd,8))return false;
        const auto& bounds=out.bounds;failure_=Failure::Bounds;
        if(!Address(bounds.smallBegin,1) || !Address(bounds.middleBegin,1) || bounds.smallBegin>=bounds.smallEnd ||
            bounds.middleBegin>=bounds.middleEnd || bounds.smallEnd>std::numeric_limits<uintptr_t>::max() ||
            bounds.middleEnd>std::numeric_limits<uintptr_t>::max() ||
            !(bounds.smallEnd<=bounds.middleBegin || bounds.middleEnd<=bounds.smallBegin))return false;
        return true;
    }
    bool AddNode(std::vector<Allocation>& next,Kind kind,uint64_t address,uint64_t minimum,
                 std::map<std::pair<Kind,uint64_t>,ExtentFact>& facts){
        const auto found=facts.find({kind,address});
        if(found==facts.end())return Add(next,kind,address,minimum,ExtentKind::ParsedMinimum,minimum);
        failure_=Failure::ExtentFacts;if(found->second.bytes<minimum)return false;
        const auto bytes=found->second.bytes;facts.erase(found);
        return Add(next,kind,address,bytes,ExtentKind::ExactAllocation,minimum);
    }
    bool Add(std::vector<Allocation>& next,Kind kind,uint64_t address,uint64_t bytes,ExtentKind extent,uint64_t minimum=0){
        failure_=Failure::ExtentOverflow;if(!Address(address,bytes) || !bytes)return false;
        failure_=Failure::UnsupportedPointer;const auto region=ClassifyPointer(address);if(region==Region::Unsupported)return false;
        failure_=Failure::ExtentCrossesRegion;if(ClassifySpan(address,bytes)!=region)return false;
        failure_=Failure::Overlap;
        for(const auto& allocation:next)if(address<allocation.address+allocation.bytes && allocation.address<address+bytes)return false;
        next.push_back({kind,region,extent,address,bytes,minimum});return true;
    }
#endif
    uint64_t imageBase_{};Identity identity_;Lifecycle lifecycle_;LayoutFacts facts_;
    definition_tree::NativeVector16 savedHeader_;
    std::vector<Allocation> allocations_;
    bool captured_{},savedBound_{};Failure failure_=Failure::None;
};
}
