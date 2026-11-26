#pragma once
#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <iterator>
#include <limits>
#include <map>
#include <span>
#include <type_traits>
#include <utility>
#include <vector>

namespace cvr::stereo::definition_tree {
using ReadMemory=bool(*)(uint64_t,void*,size_t);
inline constexpr uint32_t MaxNodes=4096,MaxHandles=8192,MaxRoots=8192,MaxDepth=64;
inline constexpr uint32_t MaxArrayCapacity=8192,NoIndex=UINT32_MAX;
inline constexpr uint64_t MaxOwnedBytes=16ull*1024*1024;
inline constexpr uint32_t SceneDriverDestructorRva=0x8E913C,RenderElementsDestructorRva=0x8EA4E0;
inline constexpr uint32_t DescriptionOwnerDestructorRva=0x8EAB98,CStringDestructorRva=0x8EA51C;
inline constexpr uint32_t ObjectAllocatorRva=0x391C2B0,ControlAllocatorRva=0x3913850;
inline constexpr uint32_t StringAllocatorRva=0x39138C8,StringFreeAdapterRva=0x275070;

struct NativeVector16 {
    uint64_t data{};
    uint32_t capacity{},count{};
    bool operator==(const NativeVector16&)const=default;
};
struct NativeHandle {
    uint64_t object{},controlBlock{};
    bool operator==(const NativeHandle&)const=default;
};
enum class Kind:uint32_t {Unknown=0,SceneDriver=1,RenderElements=2,DescriptionOwner=3,CString=4};
enum class AllocatorKind:uint32_t {Object,ControlBlock,CString};
enum class Failure:uint32_t {
    None,Arguments,Layout,Root,Node,Header,Bounds,Handles,Control,References,
    Pairing,Allocator,Overlap,NodeBudget,HandleBudget,StorageBudget,Depth,Cycle,Changed,Allocation
};

struct Root {
    uint64_t object{};
    uint32_t slot{},node=NoIndex;
    bool operator==(const Root&)const=default;
};
struct Node {
    uint64_t object{},vtable{},destructor{},work{},controlBlock{};
    Kind kind=Kind::Unknown;
    uint32_t array=NoIndex,firstHandle=NoIndex,handleCount{},maxDepth{},reserved{};
    bool operator==(const Node&)const=default;
};
struct Handle {
    uint64_t address{},object{},controlBlock{};
    uint32_t parent=NoIndex,child=NoIndex,slot{},reserved{};
    int32_t strong{},weak{};
    bool operator==(const Handle&)const=default;
};
struct Array {
    uint64_t headerAddress{},data{},trailerAddress{},allocator{},freeCallback{};
    uint32_t capacity{},count{},stride{},node=NoIndex;
    bool operator==(const Array&)const=default;
};
struct GlobalAllocator {
    uint64_t address{},raw{},allocator{};
    AllocatorKind kind=AllocatorKind::Object;
    uint32_t reserved{};
    bool operator==(const GlobalAllocator&)const=default;
};
// freeCallback is populated only for active custom dispatch, whose native RCX
// is allocatorSlotAddress. fallbackFreeAdapter is a separate direct-call ABI.
struct String {
    uint64_t storageAddress{},data{},allocationBytes{},allocatorSlotAddress{},allocatorValue{},freeCallback{};
    uint64_t fallbackGlobalAddress{},fallbackRaw{},fallbackAllocator{},fallbackFreeAdapter{};
    std::array<uint8_t,32> storage{};
    uint32_t node=NoIndex,mode{},length{},capacity{};
    bool operator==(const String&)const=default;
};
static_assert(sizeof(NativeVector16)==16 && offsetof(NativeVector16,capacity)==8 && offsetof(NativeVector16,count)==12);
static_assert(sizeof(NativeHandle)==16 && sizeof(Root)==16 && sizeof(Node)==64 && sizeof(Handle)==48);
static_assert(sizeof(Array)==56 && sizeof(GlobalAllocator)==32);
static_assert(sizeof(String)==128 && offsetof(String,storage)==80 && offsetof(String,node)==112);
static_assert(std::is_trivially_copyable_v<Node> && std::is_standard_layout_v<Node>);
static_assert(std::is_trivially_copyable_v<Handle> && std::is_standard_layout_v<Handle>);
static_assert(std::is_trivially_copyable_v<Array> && std::is_standard_layout_v<Array>);
static_assert(std::is_trivially_copyable_v<String> && std::is_standard_layout_v<String>);
#if CVR_RUST_VRIK
// The snapshot: rust/crates/cvr-render/src/native/definition_tree.rs through definition_tree_abi.rs
// (tools/single_pass_tests/definition_tree_test, allocator_binding_test and native_stage_definition_test run their
// cases against it in their Rust variants).
struct RustData;
struct RustView {
    const Root* roots;size_t rootCount;const Node* nodes;size_t nodeCount;const Handle* handles;size_t handleCount;
    const Array* arrays;size_t arrayCount;const String* strings;size_t stringCount;const GlobalAllocator* allocators;
};
extern "C" {
RustData* cvr_dt_capture(const uint64_t*,size_t,uint64_t,ReadMemory,bool,uint32_t*);
void cvr_dt_view(const RustData*,RustView*);
void cvr_dt_free(RustData*);
bool cvr_dt_matches_before(const RustView*,uint64_t,ReadMemory,bool);
}
#endif

// Metadata observation only. The caller supplies root identities and guards the
// exact native destructor/layout bodies before enabling known-layout reads.
// Equal snapshots cannot exclude ABA, establish ownership, or certify any
// destructor, allocator lifetime, graph completion, or borrowed work input.
class Snapshot {
public:
#if CVR_RUST_VRIK
    bool Capture(std::span<const uint64_t> roots,uint64_t imageBase,ReadMemory read,
                 bool nativeLayoutGuarded)try{
        *this=Snapshot{};uint32_t failure=0;
        const auto captured=cvr_dt_capture(roots.data(),roots.size(),imageBase,read,nativeLayoutGuarded,&failure);
        failure_=Failure(failure);
        if(failure_==Failure::Allocation){*this=Snapshot{};failure_=Failure::Allocation;return false;}
        if(failure_!=Failure::Arguments){imageBase_=imageBase;layoutGuarded_=nativeLayoutGuarded;}
        if(!captured)return false;
        struct Release{RustData* data;~Release(){cvr_dt_free(data);}}release{captured};
        RustView view{};cvr_dt_view(captured,&view);
        data_.roots.assign(view.roots,view.roots+view.rootCount);
        data_.nodes.assign(view.nodes,view.nodes+view.nodeCount);
        data_.handles.assign(view.handles,view.handles+view.handleCount);
        data_.arrays.assign(view.arrays,view.arrays+view.arrayCount);
        data_.strings.assign(view.strings,view.strings+view.stringCount);
        std::copy_n(view.allocators,data_.allocators.size(),data_.allocators.begin());
        ready_=true;failure_=Failure::None;return true;
    }catch(...){*this=Snapshot{};failure_=Failure::Allocation;return false;}
    bool MatchesBefore(ReadMemory read)const try{
        if(!ready_)return false;
        const RustView view{data_.roots.data(),data_.roots.size(),data_.nodes.data(),data_.nodes.size(),
            data_.handles.data(),data_.handles.size(),data_.arrays.data(),data_.arrays.size(),
            data_.strings.data(),data_.strings.size(),data_.allocators.data()};
        return cvr_dt_matches_before(&view,imageBase_,read,layoutGuarded_);
    }catch(...){return false;}
    // The records and layout flag the Rust stage definition revalidates this tree with.
    RustView RustRecords()const{
        return {data_.roots.data(),data_.roots.size(),data_.nodes.data(),data_.nodes.size(),data_.handles.data(),
            data_.handles.size(),data_.arrays.data(),data_.arrays.size(),data_.strings.data(),data_.strings.size(),
            data_.allocators.data()};
    }
    bool LayoutGuarded()const{return layoutGuarded_;}
#else
    bool Capture(std::span<const uint64_t> roots,uint64_t imageBase,ReadMemory read,
                 bool nativeLayoutGuarded)try{
        *this=Snapshot{};failure_=Failure::Arguments;
        if(!read || !Pointer(imageBase,8) || imageBase>UINT64_MAX-ObjectAllocatorRva-8 || roots.size()>MaxRoots)return false;
        imageBase_=imageBase;layoutGuarded_=nativeLayoutGuarded;
        State first,second;
        if(!ReadOnce(roots,read,first) || !ReadOnce(roots,read,second))return false;
        failure_=Failure::Changed;if(!StructuralEqual(first.data,second.data))return false;
        data_=std::move(second.data);ready_=true;failure_=Failure::None;return true;
    }catch(...){*this=Snapshot{};failure_=Failure::Allocation;return false;}
    bool MatchesBefore(ReadMemory read)const try{
        if(!ready_)return false;
        std::vector<uint64_t> roots;roots.reserve(data_.roots.size());
        for(const auto& root:data_.roots)roots.push_back(root.object);
        Snapshot current;
        return current.Capture(roots,imageBase_,read,layoutGuarded_) && StructuralEqual(current.data_,data_);
    }catch(...){return false;}
#endif
    bool Ready()const{return ready_;}
    Failure Rejection()const{return failure_;}
    uint64_t ImageBase()const{return imageBase_;}
    bool NativeOwnershipProved()const{return false;}
    bool DestructorSafetyProved()const{return false;}
    const std::vector<Root>& Roots()const{return data_.roots;}
    const std::vector<Node>& Nodes()const{return data_.nodes;}
    const std::vector<Handle>& Handles()const{return data_.handles;}
    const std::vector<Array>& Arrays()const{return data_.arrays;}
    const std::vector<String>& Strings()const{return data_.strings;}
    const std::array<GlobalAllocator,2>& Allocators()const{return data_.allocators;}
private:
    struct Data {
        std::vector<Root> roots;
        std::vector<Node> nodes;
        std::vector<Handle> handles;
        std::vector<Array> arrays;
        std::vector<String> strings;
        std::array<GlobalAllocator,2> allocators{};
        bool operator==(const Data&)const=default;
    };
#if !CVR_RUST_VRIK
    enum class RangeKind:uint32_t {Object,Control,Array,String,Vtable,AllocatorIdentity,AllocatorMethod,Global};
    struct Range {uint64_t end{};RangeKind kind{};};
    struct Control {
        uint64_t object{};
        int32_t strong{},weak{};
        uint32_t handles{};
    };
    struct State {
        Data data;
        std::map<uint64_t,uint32_t> objects;
        std::map<uint64_t,Control> controls;
        std::map<uint64_t,Range> ranges;
        GlobalAllocator stringAllocator;
        uint64_t ownedBytes{},childCapacity{};
        bool stringAllocatorCaptured{};
    };
    static bool StructuralEqual(const Data& a,const Data& b){
        if(a.roots!=b.roots || a.nodes!=b.nodes || a.arrays!=b.arrays || a.strings!=b.strings || a.allocators!=b.allocators ||
            a.handles.size()!=b.handles.size())return false;
        for(size_t i=0;i<a.handles.size();++i){
            auto first=a.handles[i],second=b.handles[i];
            // Other shared owners may adjust both counts. Each complete pass
            // independently validates positive counts and captured-slot cover.
            first.strong=first.weak=second.strong=second.weak=0;
            if(first!=second)return false;
        }
        return true;
    }
    static bool Address(uint64_t address,uint64_t bytes){
        return address && address<=UINT64_MAX-bytes &&
            address<=std::numeric_limits<uintptr_t>::max();
    }
    static bool Pointer(uint64_t address,uint64_t bytes){return Address(address,bytes) && !(address&7);}
    static bool Code(uint64_t address){return address && address<=std::numeric_limits<uintptr_t>::max();}
    bool Reject(Failure reason){failure_=reason;return false;}
    bool AddRange(State& state,uint64_t begin,uint64_t bytes,RangeKind kind){
        if(!Address(begin,bytes) || (kind!=RangeKind::String && (begin&7)))return Reject(Failure::Bounds);
        const auto end=begin+bytes;
        const auto next=state.ranges.lower_bound(begin);
        if(next!=state.ranges.end() && next->first==begin && next->second.end==end && next->second.kind==kind &&
            (kind==RangeKind::Vtable || kind==RangeKind::AllocatorIdentity || kind==RangeKind::AllocatorMethod ||
             kind==RangeKind::Global))return true;
        if(next!=state.ranges.end() && next->first<end)return Reject(Failure::Overlap);
        if(next!=state.ranges.begin() && std::prev(next)->second.end>begin)return Reject(Failure::Overlap);
        // Every range belongs to a bounded node, control, array, or global.
        if(state.ranges.size()>=size_t(MaxNodes)*5+MaxHandles+6)return Reject(Failure::StorageBudget);
        state.ranges.emplace(begin,Range{end,kind});return true;
    }
    bool AddNode(State& state,uint64_t object,uint64_t control,ReadMemory read,uint32_t& index){
        if(!Pointer(object,8))return Reject(Failure::Node);
        const auto found=state.objects.find(object);
        if(found!=state.objects.end()){
            index=found->second;
            if(state.data.nodes[index].controlBlock!=control)return Reject(Failure::Pairing);
            return true;
        }
        if(state.data.nodes.size()>=MaxNodes)return Reject(Failure::NodeBudget);
        Node node;node.object=object;node.controlBlock=control;
        failure_=Failure::Node;
        if(!read(object,&node.vtable,8) || !Pointer(node.vtable,16) ||
            !read(node.vtable,&node.destructor,8) || !read(node.vtable+8,&node.work,8) ||
            !Code(node.destructor) || !Code(node.work))return false;
        if(node.destructor==imageBase_+SceneDriverDestructorRva)node.kind=Kind::SceneDriver;
        else if(node.destructor==imageBase_+RenderElementsDestructorRva)node.kind=Kind::RenderElements;
        else if(node.destructor==imageBase_+DescriptionOwnerDestructorRva)node.kind=Kind::DescriptionOwner;
        else if(node.destructor==imageBase_+CStringDestructorRva)node.kind=Kind::CString;
        if(node.kind!=Kind::Unknown && !layoutGuarded_)return Reject(Failure::Layout);
        const uint64_t knownBytes=node.kind==Kind::SceneDriver?0x38:
            node.kind==Kind::RenderElements || node.kind==Kind::CString?0x40:node.kind==Kind::DescriptionOwner?0x30:8;
        if(!AddRange(state,object,knownBytes,RangeKind::Object) || !AddRange(state,node.vtable,16,RangeKind::Vtable))return false;
        index=uint32_t(state.data.nodes.size());state.objects.emplace(object,index);
        state.data.nodes.push_back(node);return true;
    }
    bool ReadControl(State& state,Handle& handle,ReadMemory read){
        if(!handle.controlBlock){
            // 1444C4 skips an empty control; a live object with no control is
            // not evidence of the shared ownership this capture is auditing.
            return !handle.object || Reject(Failure::Pairing);
        }
        if(!Pointer(handle.controlBlock,8))return Reject(Failure::Control);
        auto found=state.controls.find(handle.controlBlock);
        if(found==state.controls.end()){
            if(!AddRange(state,handle.controlBlock,8,RangeKind::Control))return false;
            Control control;control.object=handle.object;
            std::array<int32_t,2> counts{};failure_=Failure::Control;
            if(!read(handle.controlBlock,counts.data(),sizeof(counts)))return false;
            control.strong=counts[0];control.weak=counts[1];
            if(control.strong<=0 || control.weak<=0)return Reject(Failure::References);
            found=state.controls.emplace(handle.controlBlock,control).first;
        }
        if(found->second.object!=handle.object)return Reject(Failure::Pairing);
        auto& control=found->second;
        if(++control.handles>uint32_t(control.strong))return Reject(Failure::References);
        handle.strong=control.strong;handle.weak=control.weak;return true;
    }
    bool ReadArray(State& state,uint32_t nodeIndex,ReadMemory read){
        const auto kind=state.data.nodes[nodeIndex].kind;
        const uint64_t offset=kind==Kind::SceneDriver?0x28:kind==Kind::DescriptionOwner?0x20:0x30;
        Array array;array.node=nodeIndex;array.stride=kind==Kind::SceneDriver?16:0x30;
        array.headerAddress=state.data.nodes[nodeIndex].object+offset;
        NativeVector16 header;failure_=Failure::Header;
        if(!read(array.headerAddress,&header,sizeof(header)))return false;
        array.data=header.data;array.capacity=header.capacity;array.count=header.count;
        if(!Pointer(array.data,8) || array.capacity>MaxArrayCapacity || array.count>array.capacity)return Reject(Failure::Bounds);
        if(kind==Kind::SceneDriver){
            if(array.count>MaxHandles-state.data.handles.size())return Reject(Failure::HandleBudget);
            if(array.capacity>MaxHandles-state.childCapacity)return Reject(Failure::StorageBudget);
            state.childCapacity+=array.capacity;
        }
        if(array.capacity){
            const auto bytes=(uint64_t(array.capacity)*array.stride+7)&~uint64_t(7);
            if(bytes>UINT32_MAX || bytes+8>MaxOwnedBytes-state.ownedBytes)return Reject(Failure::StorageBudget);
            if(!AddRange(state,array.data,bytes+8,RangeKind::Array))return false;
            state.ownedBytes+=bytes+8;array.trailerAddress=array.data+bytes;
            failure_=Failure::Allocator;if(!read(array.trailerAddress,&array.allocator,8))return false;
        }else array.allocator=array.data;
        if(!Pointer(array.allocator,0x28))return Reject(Failure::Allocator);
        if(!AddRange(state,array.allocator,8,RangeKind::AllocatorIdentity) ||
            !AddRange(state,array.allocator+0x20,8,RangeKind::AllocatorMethod))return false;
        failure_=Failure::Allocator;
        if(!read(array.allocator+0x20,&array.freeCallback,8) || !Code(array.freeCallback))return false;
        state.data.nodes[nodeIndex].array=uint32_t(state.data.arrays.size());
        state.data.arrays.push_back(array);
        if(kind!=Kind::SceneDriver)return true;
        auto& parent=state.data.nodes[nodeIndex];
        parent.firstHandle=uint32_t(state.data.handles.size());parent.handleCount=array.count;
        std::vector<NativeHandle> handles(array.count);failure_=Failure::Handles;
        if(array.count && !read(array.data,handles.data(),size_t(array.count)*sizeof(NativeHandle)))return false;
        for(uint32_t slot=0;slot<array.count;++slot){
            Handle handle;handle.address=array.data+uint64_t(slot)*16;handle.object=handles[slot].object;
            handle.controlBlock=handles[slot].controlBlock;handle.parent=nodeIndex;handle.slot=slot;
            if(!ReadControl(state,handle,read))return false;
            if(handle.object && !AddNode(state,handle.object,handle.controlBlock,read,handle.child))return false;
            state.data.handles.push_back(handle);
        }
        return true;
    }
    template<class T>static T StringField(const String& string,size_t offset){
        T value{};std::memcpy(&value,string.storage.data()+offset,sizeof(value));return value;
    }
    bool ReadStringAllocator(State& state,ReadMemory read){
        if(state.stringAllocatorCaptured)return true;
        auto& allocator=state.stringAllocator;allocator.address=imageBase_+StringAllocatorRva;
        allocator.kind=AllocatorKind::CString;
        if(!AddRange(state,allocator.address,8,RangeKind::Global))return false;
        failure_=Failure::Allocator;if(!read(allocator.address,&allocator.raw,8))return false;
        allocator.allocator=allocator.raw&~uint64_t(7);
        if(!Pointer(allocator.allocator,8))return Reject(Failure::Allocator);
        if(!AddRange(state,allocator.allocator,8,RangeKind::AllocatorIdentity))return false;
        state.stringAllocatorCaptured=true;return true;
    }
    bool ReadString(State& state,uint32_t nodeIndex,ReadMemory read){
        if(state.data.strings.size()>=MaxNodes)return Reject(Failure::NodeBudget);
        String string;string.node=nodeIndex;string.storageAddress=state.data.nodes[nodeIndex].object+0x20;
        string.allocatorSlotAddress=string.storageAddress+0x18;
        failure_=Failure::Header;if(!read(string.storageAddress,string.storage.data(),string.storage.size()))return false;
        const auto flags=StringField<uint32_t>(string,0x14);
        string.mode=flags>>30;string.length=flags&0x3FFFFFFF;
        string.allocatorValue=StringField<uint64_t>(string,0x18);
        if(string.mode==3)return Reject(Failure::Bounds);
        if(!string.mode){
            string.data=string.storageAddress;string.capacity=19;
            if(string.length>string.capacity)return Reject(Failure::Bounds);
        }else{
            string.data=StringField<uint64_t>(string,0);string.capacity=StringField<uint32_t>(string,0x10);
            if(string.mode==1){
                if(string.length>string.capacity || string.capacity>UINT32_MAX-8)return Reject(Failure::Bounds);
                if(string.capacity){
                    string.allocationBytes=(uint64_t(string.capacity)+8)&~uint64_t(7);
                    if(string.allocationBytes>MaxOwnedBytes-state.ownedBytes)return Reject(Failure::StorageBudget);
                    if(!AddRange(state,string.data,string.allocationBytes,RangeKind::String))return false;
                    state.ownedBytes+=string.allocationBytes;
                    if(string.allocatorValue){
                        if(!Pointer(string.allocatorValue,0x28))return Reject(Failure::Allocator);
                        if(!AddRange(state,string.allocatorValue,8,RangeKind::AllocatorIdentity) ||
                            !AddRange(state,string.allocatorValue+0x20,8,RangeKind::AllocatorMethod))return false;
                        failure_=Failure::Allocator;
                        if(!read(string.allocatorValue+0x20,&string.freeCallback,8) || !Code(string.freeCallback))return false;
                    }else{
                        if(!ReadStringAllocator(state,read))return false;
                        string.fallbackGlobalAddress=state.stringAllocator.address;string.fallbackRaw=state.stringAllocator.raw;
                        string.fallbackAllocator=state.stringAllocator.allocator;
                        // Direct 1444B0 takes &block in RCX. Custom dispatch
                        // instead calls freeCallback with RCX=&S.allocator.
                        string.fallbackFreeAdapter=imageBase_+StringFreeAdapterRva;
                    }
                }
            }
            // Borrowed mode2 does not dereference its data or allocator and
            // implies neither an allocation range nor an eventual free route.
        }
        state.data.strings.push_back(string);return true;
    }
    bool ValidatePaths(State& state){
        std::vector<uint32_t> incoming(state.data.nodes.size()),queue;
        queue.reserve(state.data.nodes.size());
        for(const auto& handle:state.data.handles)if(handle.child!=NoIndex)++incoming[handle.child];
        for(uint32_t i=0;i<incoming.size();++i)if(!incoming[i])queue.push_back(i);
        for(size_t cursor=0;cursor<queue.size();++cursor){
            const auto index=queue[cursor];const auto& parent=state.data.nodes[index];
            for(uint32_t slot=0;slot<parent.handleCount;++slot){
                const auto child=state.data.handles[parent.firstHandle+slot].child;if(child==NoIndex)continue;
                auto& node=state.data.nodes[child];node.maxDepth=std::max(node.maxDepth,parent.maxDepth+1);
                if(node.maxDepth>MaxDepth)return Reject(Failure::Depth);
                if(!--incoming[child])queue.push_back(child);
            }
        }
        return queue.size()==state.data.nodes.size() || Reject(Failure::Cycle);
    }
    bool ReadOnce(std::span<const uint64_t> roots,ReadMemory read,State& state){
        state.data.roots.reserve(roots.size());state.data.nodes.reserve(MaxNodes);
        state.data.handles.reserve(MaxHandles);state.data.arrays.reserve(MaxNodes);
        state.data.strings.reserve(MaxNodes);
        const std::array<uint32_t,2> globalRvas{ObjectAllocatorRva,ControlAllocatorRva};
        for(uint32_t i=0;i<globalRvas.size();++i){
            auto& allocator=state.data.allocators[i];allocator.address=imageBase_+globalRvas[i];
            allocator.kind=i?AllocatorKind::ControlBlock:AllocatorKind::Object;
            if(!AddRange(state,allocator.address,8,RangeKind::Global))return false;
            failure_=Failure::Allocator;if(!read(allocator.address,&allocator.raw,8))return false;
            allocator.allocator=allocator.raw&~uint64_t(7);
            if(!Pointer(allocator.allocator,8))return Reject(Failure::Allocator);
            if(!AddRange(state,allocator.allocator,8,RangeKind::AllocatorIdentity))return false;
        }
        for(uint32_t slot=0;slot<roots.size();++slot){
            Root root;root.object=roots[slot];root.slot=slot;
            if(root.object){
                if(state.objects.contains(root.object))return Reject(Failure::Root);
                if(!AddNode(state,root.object,0,read,root.node))return false;
            }
            state.data.roots.push_back(root);
        }
        for(uint32_t i=0;i<state.data.nodes.size();++i){
            const auto kind=state.data.nodes[i].kind;
            if(kind==Kind::CString){if(!ReadString(state,i,read))return false;}
            else if(kind!=Kind::Unknown && !ReadArray(state,i,read))return false;
        }
        return ValidatePaths(state);
    }
#endif
    Data data_;
    uint64_t imageBase_{};
    bool layoutGuarded_{},ready_{};
    Failure failure_=Failure::None;
};
}
