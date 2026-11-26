#pragma once
#include <algorithm>
#include <array>
#include <atomic>
#include <cstdint>
#include <map>
#include <span>
#include <unordered_map>
#include <vector>

namespace cvr::stereo::descriptors {
// Canonical API description. IDs refer to object-lifetime cookies, never raw
// resource addresses. Zero ID means unknown, not a valid null descriptor.
// SRV words 13..19 optionally describe the resource observed at view creation:
// dimension/format, width, height/array/mips, flags/layout, samples/quality,
// heap type/CPU page/memory pool (16-bit fields), and buffer GPU address.
// Flags at word 1: 1=resource, 2=implicit view, 4=resource description,
// 8=heap description. These are metadata, never a resource/content lease.
using Value=std::array<uint64_t,20>;
struct BorrowedDescriptorInventory {
    uint64_t slots{},unknown{},knownNull{},resourceSRV{},otherKinds{},uniqueCookies{},trackedCookies{},untrackedCookies{};
    uint64_t texture2D{},texture2DArray{},flagsRTDSUAV{},shapeUnknown{};
};
// Classification of canonical metadata only. The returned cookie is a weak
// numeric label, never a source pointer or resource/lifetime permission.
#if CVR_RUST_VRIK
// The rules and the store: rust/crates/cvr-render/src/descriptor_store.rs (tools/single_pass_tests/descriptor_store_test
// runs its cases against them in its Rust variant). The Store face owns the Rust store; BorrowedRangeTracking's atomics
// stay here and Rust operates on them.
struct RangeIdentity;struct BorrowedRootRange;struct BorrowedShaderUse;struct BorrowedUsedRange;class BorrowedRangeTracking;
namespace detail {
extern "C" {
uint64_t cvr_ds_inventory(const Value*,bool,BorrowedDescriptorInventory*);
uint32_t cvr_ds_mutation_mode(bool,bool,bool);
void cvr_ds_tracking_details(bool,bool,uint64_t,uint64_t,uint32_t,uint32_t,bool,uint32_t,uint32_t,bool,std::array<uint64_t,4>*);
bool cvr_ds_same_range(const RangeIdentity*,const RangeIdentity*);
bool cvr_ds_plan(uint32_t,bool,bool,const BorrowedRootRange*,size_t,const BorrowedShaderUse*,size_t,BorrowedUsedRange*,size_t*);
bool cvr_ds_tracking(const BorrowedRangeTracking*,uint32_t,uint64_t);
void* cvr_ds_new(bool);
void cvr_ds_free(void*);
void cvr_ds_set_retirement(void*,void(*)(const Value&));
uint64_t cvr_ds_query(const void*,uint32_t,uint64_t);
void cvr_ds_stats(const void*,std::array<uint64_t,6>*);
const Value* cvr_ds_get(const void*,uint32_t);
uint32_t cvr_ds_change(void*,uint32_t,uint64_t,uint64_t,uint64_t,uint32_t,uint32_t);
uint32_t cvr_ds_value(void*,uint32_t,const Value*,uint64_t,uint32_t);
void cvr_ds_range(const void*,uint64_t,uint32_t,RangeIdentity*);
}
}
inline uint64_t InventoryBorrowedDescriptor(const Value* value,bool canonicalSrvValid,BorrowedDescriptorInventory& out){
    return detail::cvr_ds_inventory(value,canonicalSrvValid,&out);
}
#else
inline uint64_t InventoryBorrowedDescriptor(const Value* value,bool canonicalSrvValid,BorrowedDescriptorInventory& out){
    ++out.slots;
    if(!value || !(*value)[0] || (*value)[0]>4){++out.unknown;return 0;}
    const auto& v=*value;
    if(v[0]!=2){++out.otherKinds;return 0;}
    if(!canonicalSrvValid || (v[1]&~uint64_t(15))){++out.unknown;return 0;}
    if(!(v[1]&1)){
        // An acceleration-structure SRV can have a null resource argument but
        // a nonnull GPU address; it is not a typed null texture descriptor.
        if(v[5]==11){++out.otherKinds;return 0;}
        if(v[1] || v[2] || !v[5] || v[5]>10){++out.unknown;return 0;}
        ++out.knownNull;return 0;
    }
    if(!v[2]){++out.unknown;return 0;}
    ++out.resourceSRV;
    const auto dimension=uint32_t(v[13]),height=uint32_t(v[15]);
    const auto layers=uint16_t(v[15]>>32),mips=uint16_t(v[15]>>48);
    const bool described=(v[1]&4) && dimension>=1 && dimension<=4 && v[14] && height && layers && mips && uint32_t(v[17]);
    if(!described)++out.shapeUnknown;
    else{
        if(dimension==3 && v[5]==4)++out.texture2D;
        if(dimension==3 && v[5]==5)++out.texture2DArray;
        if(uint32_t(v[16])&7)++out.flagsRTDSUAV;
    }
    return v[2];
}
#endif
struct RangeIdentity {uint64_t heap{},cpu{},gpu{},version{};uint32_t count{},stride{};bool valid{};};
enum class DescriptorMutationMode {Ignore,Canonical,InvalidateTracked};
#if CVR_RUST_VRIK
inline DescriptorMutationMode DescriptorMutationObservation(bool internal,bool enabled,bool borrowed){
    return DescriptorMutationMode(detail::cvr_ds_mutation_mode(internal,enabled,borrowed));
}
#else
inline DescriptorMutationMode DescriptorMutationObservation(bool internal,bool enabled,bool borrowed){
    if(internal)return borrowed?DescriptorMutationMode::InvalidateTracked:DescriptorMutationMode::Ignore;
    return enabled || borrowed?DescriptorMutationMode::Canonical:DescriptorMutationMode::Ignore;
}
#endif
enum class TrackedDescriptorWrite {Ignored,Invalidated,Rejected};
#if CVR_RUST_VRIK
class BorrowedRangeTracking {
    std::atomic<uint64_t> epoch_{1};
    std::atomic<uint32_t> pins_{},mutations_{};
    std::atomic<bool> ready_{},unavailable_{};
    bool Call(uint32_t which,uint64_t value=0)const{return detail::cvr_ds_tracking(this,which,value);}
public:
    static constexpr uint32_t PinLimit=4096;
    void Ready(bool value){Call(0,value);}
    bool Ready()const{return Call(1);}
    uint32_t Pins()const{return pins_.load(std::memory_order_acquire);}
    uint32_t Mutations()const{return mutations_.load(std::memory_order_acquire);}
    uint64_t Epoch()const{return epoch_.load(std::memory_order_acquire);}
    bool Pin(){return Call(2);}
    void Release(){Call(3);}
    bool MutationBegin(){return Call(4);}
    void MutationEnd(){Call(5);}
    bool Stable(uint64_t epoch)const{return Call(6,epoch);}
    bool VersionsRequired(bool liveMetadata)const{return Call(7,liveMetadata);}
    void Invalidate(){Call(8);}
};
static_assert(sizeof(BorrowedRangeTracking)==24,"mirrored by descriptor_store.rs");
#else
class BorrowedRangeTracking {
    std::atomic<uint64_t> epoch_{1};
    std::atomic<uint32_t> pins_{},mutations_{};
    std::atomic<bool> ready_{},unavailable_{};
public:
    static constexpr uint32_t PinLimit=4096;
    void Ready(bool value){if(value)ready_.store(true,std::memory_order_release);else unavailable_.store(true,std::memory_order_release);}
    bool Ready()const{return ready_.load(std::memory_order_acquire) && !unavailable_.load(std::memory_order_acquire);}
    uint32_t Pins()const{return pins_.load(std::memory_order_acquire);}
    uint32_t Mutations()const{return mutations_.load(std::memory_order_acquire);}
    uint64_t Epoch()const{return epoch_.load(std::memory_order_acquire);}
    bool Pin(){
        if(!Ready())return false;auto value=pins_.load(std::memory_order_relaxed);
        while(value<PinLimit){if(pins_.compare_exchange_weak(value,value+1,std::memory_order_acq_rel))return true;}
        return false;
    }
    void Release(){pins_.fetch_sub(1,std::memory_order_acq_rel);}
    bool MutationBegin(){
        if(!Ready())return false;
        auto value=mutations_.load(std::memory_order_relaxed);
        while(value<UINT32_MAX){if(mutations_.compare_exchange_weak(value,value+1,std::memory_order_acq_rel))return true;}
        Ready(false);return false;
    }
    void MutationEnd(){mutations_.fetch_sub(1,std::memory_order_acq_rel);}
    bool Stable(uint64_t epoch)const{return epoch && epoch==Epoch() && Ready() && !mutations_.load(std::memory_order_acquire);}
    bool VersionsRequired(bool liveMetadata)const{return !liveMetadata || Pins()!=0;}
    void Invalidate(){
        const auto value=epoch_.load(std::memory_order_relaxed);
        if(value==UINT64_MAX){Ready(false);return;}epoch_.store(value+1,std::memory_order_release);
    }
};
#endif
#if CVR_RUST_VRIK
inline std::array<uint64_t,4> BorrowedTrackingDetails(bool pinned,bool ready,uint64_t savedEpoch,uint64_t currentEpoch,
    uint32_t pins,uint32_t mutations,bool versions,uint32_t overflow,uint32_t failure,bool empty){
    std::array<uint64_t,4> details{};
    detail::cvr_ds_tracking_details(pinned,ready,savedEpoch,currentEpoch,pins,mutations,versions,overflow,failure,empty,&details);
    return details;
}
inline bool SameBorrowedRange(const RangeIdentity& a,const RangeIdentity& b){return detail::cvr_ds_same_range(&a,&b);}
#else
inline std::array<uint64_t,4> BorrowedTrackingDetails(bool pinned,bool ready,uint64_t savedEpoch,uint64_t currentEpoch,
    uint32_t pins,uint32_t mutations,bool versions,uint32_t overflow,uint32_t failure,bool empty){
    const uint64_t reasons=uint64_t(!pinned)|(uint64_t(!ready)<<1)|(uint64_t(savedEpoch!=currentEpoch)<<2)|
        (uint64_t(mutations!=0)<<3)|(uint64_t(!versions)<<4)|(uint64_t(overflow!=0)<<5)|(uint64_t(failure!=0)<<6)|(uint64_t(empty)<<7);
    return {UINT64_MAX,reasons,(uint64_t(pins)<<32)|mutations,(uint64_t(overflow)<<32)|failure};
}
inline bool SameBorrowedRange(const RangeIdentity& a,const RangeIdentity& b){
    return a.valid && b.valid && a.heap && a.cpu && a.gpu && a.count && a.stride &&
        a.heap==b.heap && a.cpu==b.cpu && a.gpu==b.gpu && a.version==b.version && a.count==b.count && a.stride==b.stride;
}
#endif
struct BorrowedRootRange {uint32_t parameter{},type{},space{},shaderRegister{},count{},offset{},visibility{};};
struct BorrowedShaderUse {uint32_t type{},visibility{},space{},shaderRegister{},count{};};
struct BorrowedUsedRange {
    uint32_t parameter{},type{},offset{},count{},shaderDeclaredCount{};
    bool physicalRemainder{};
    bool operator==(const BorrowedUsedRange&)const=default;
};
// Numeric planner only; the runtime obtains uses from an authenticated current
// PSO family, never from a caller's proposed access list. Unbounded-root uses
// retain the whole physical remainder even when shader reflection is finite.
#if CVR_RUST_VRIK
inline bool PlanBorrowedShaderRanges(uint32_t parameters,bool supported,bool directHeapIndexing,
    std::span<const BorrowedRootRange> declarations,std::span<const BorrowedShaderUse> uses,
    std::vector<BorrowedUsedRange>& output){
    output.clear();std::array<BorrowedUsedRange,256> planned{};size_t count{};
    if(!detail::cvr_ds_plan(parameters,supported,directHeapIndexing,declarations.data(),declarations.size(),
        uses.data(),uses.size(),planned.data(),&count))return false;
    output.assign(planned.begin(),planned.begin()+count);return true;
}
static_assert(sizeof(RangeIdentity)==48 && sizeof(BorrowedDescriptorInventory)==96 && sizeof(BorrowedRootRange)==28 &&
    sizeof(BorrowedShaderUse)==20 && sizeof(BorrowedUsedRange)==24,"mirrored by descriptor_store.rs");
#else
inline bool PlanBorrowedShaderRanges(uint32_t parameters,bool supported,bool directHeapIndexing,
    std::span<const BorrowedRootRange> declarations,std::span<const BorrowedShaderUse> uses,
    std::vector<BorrowedUsedRange>& output){
    output.clear();
    if(!supported || directHeapIndexing || !parameters || parameters>64 || declarations.empty() || declarations.size()>256 ||
        uses.empty() || uses.size()>4096)return false;
    const auto end=[](const BorrowedRootRange& range){return range.count==UINT32_MAX?uint64_t(UINT32_MAX)+1:uint64_t(range.shaderRegister)+range.count;};
    for(size_t i=0;i<declarations.size();++i){const auto& range=declarations[i];
        if(range.parameter>=parameters || range.type>3 || range.visibility>7 || !range.count ||
            (range.count!=UINT32_MAX && (uint64_t(range.offset)+range.count>uint64_t(UINT32_MAX)+1 ||
                uint64_t(range.shaderRegister)+range.count>uint64_t(UINT32_MAX)+1)))return false;
        for(size_t j=0;j<i;++j){const auto& prior=declarations[j];
            if(prior.parameter==range.parameter && ((prior.type==3)!=(range.type==3) || prior.count==UINT32_MAX))return false;
            const bool visible=!prior.visibility || !range.visibility || prior.visibility==range.visibility;
            if(visible && prior.type==range.type && prior.space==range.space &&
                prior.shaderRegister<end(range) && range.shaderRegister<end(prior))return false;
        }
    }
    std::vector<BorrowedUsedRange> result;result.reserve(std::min(uses.size(),size_t(256)));
    for(const auto& use:uses){
        if(use.type>3 || (use.visibility!=1 && use.visibility!=5) || !use.count ||
            uint64_t(use.shaderRegister)+use.count>uint64_t(UINT32_MAX)+1)return false;
        const BorrowedRootRange* match{};
        for(const auto& range:declarations){
            if(range.type!=use.type || range.space!=use.space || (range.visibility && range.visibility!=use.visibility) ||
                use.shaderRegister<range.shaderRegister)continue;
            const auto relative=uint64_t(use.shaderRegister)-range.shaderRegister;
            if(range.count!=UINT32_MAX && (relative>=range.count || use.count>range.count-relative))continue;
            if(match)return false;match=&range;
        }
        if(!match)return false;
        const bool unbounded=match->count==UINT32_MAX;
        const uint64_t first=unbounded?match->offset:uint64_t(match->offset)+use.shaderRegister-match->shaderRegister;
        if(first>UINT32_MAX)return false;
        result.push_back({match->parameter,use.type,uint32_t(first),unbounded?UINT32_MAX:use.count,use.count,unbounded});
    }
    std::sort(result.begin(),result.end(),[](const auto& a,const auto& b){
        if(a.parameter!=b.parameter)return a.parameter<b.parameter;
        if(a.offset!=b.offset)return a.offset<b.offset;return a.type<b.type;
    });
    std::vector<BorrowedUsedRange> merged;merged.reserve(std::min(result.size(),size_t(256)));
    for(const auto& range:result){
        if(!merged.empty() && merged.back().parameter==range.parameter){auto& prior=merged.back();
            const auto priorEnd=prior.physicalRemainder?UINT64_MAX:uint64_t(prior.offset)+prior.count;
            if(range.offset<priorEnd && range.type!=prior.type)return false;
            if(range.offset<=priorEnd && range.type==prior.type){
                prior.physicalRemainder=prior.physicalRemainder || range.physicalRemainder;
                prior.shaderDeclaredCount=std::max(prior.shaderDeclaredCount,range.shaderDeclaredCount);
                if(prior.physicalRemainder)prior.count=UINT32_MAX;
                else{const auto mergedEnd=std::max(priorEnd,uint64_t(range.offset)+range.count);
                    if(mergedEnd-prior.offset>UINT32_MAX)return false;prior.count=uint32_t(mergedEnd-prior.offset);}
                continue;
            }
        }
        if(merged.size()==256)return false;merged.push_back(range);
    }
    output=std::move(merged);return !output.empty();
}
#endif
#if CVR_RUST_VRIK
class Store {
public:
    explicit Store(bool dense=true):store_(detail::cvr_ds_new(dense)){}
    ~Store(){detail::cvr_ds_free(store_);}
    Store(const Store&)=delete;
    Store& operator=(const Store&)=delete;
    // Aggregate CPU metadata budget, not a per-shader-visible-heap size limit.
    // Keep descriptor values only once; the interning index stores hashes/IDs.
    static constexpr size_t SlotLimit=4*1048576,ValueLimit=1048576;
    // Called under the owner's metadata lock only when the last slot releases
    // a canonical value, including a streaming value overwritten in place.
    void SetValueRetirement(void(*callback)(const Value&)){detail::cvr_ds_set_retirement(store_,callback);}
    bool Overflow()const{return detail::cvr_ds_query(store_,0,0)!=0;}
    std::array<uint64_t,6> Stats()const{std::array<uint64_t,6> stats{};detail::cvr_ds_stats(store_,&stats);return stats;}
    bool Dense()const{return detail::cvr_ds_query(store_,1,0)!=0;}
    bool TrackVersions()const{return detail::cvr_ds_query(store_,2,0)!=0;}
    // A caller may suspend history only when no captured ranges are awaiting
    // submission. Current descriptor values and their ownership remain live.
    void TrackVersions(bool enabled){detail::cvr_ds_change(store_,0,enabled,0,0,0,0);}
    // Representation-only change under the caller's metadata lock. Descriptor
    // IDs, reference counts, heap cookies and every range version stay intact.
    void Dense(bool enabled){detail::cvr_ds_change(store_,1,enabled,0,0,0,0);}
    size_t Values()const{return size_t(detail::cvr_ds_query(store_,3,0));}
    uint64_t HeapSlots()const{return detail::cvr_ds_query(store_,4,0);}
    // Valid until the store's next change, as the C++ vector element was.
    const Value* Get(uint32_t id)const{return detail::cvr_ds_get(store_,id);}
    uint32_t Intern(const Value& value){return detail::cvr_ds_value(store_,0,&value,0,0);}
    bool HeapCreated(uint64_t cpu,uint64_t gpu,uint32_t count,uint32_t stride,uint64_t cookie){
        return detail::cvr_ds_change(store_,6,cpu,gpu,cookie,count,stride)!=0;
    }
    void HeapRemoved(uint64_t cookie){detail::cvr_ds_change(store_,2,cookie,0,0,0,0);}
    // Private heaps never enter this store. A private operation that writes an
    // already-tracked heap must still invalidate its exact destination range,
    // without reading untracked source descriptors or interning new metadata.
    TrackedDescriptorWrite InvalidateTracked(uint64_t cpu,uint32_t count,uint32_t stride){
        return TrackedDescriptorWrite(detail::cvr_ds_change(store_,5,cpu,0,0,count,stride));
    }
    void Write(uint64_t cpu,uint32_t stride,uint32_t id){detail::cvr_ds_change(store_,3,cpu,0,0,id,stride);}
    // Streaming CBVs rarely repeat, so avoid hashing/interning their full API
    // description. Copied descriptors still share immutable values; overwrite
    // in place only when this slot is their sole owner.
    void WriteValue(uint64_t cpu,uint32_t stride,const Value& value){detail::cvr_ds_value(store_,1,&value,cpu,stride);}
    uint32_t At(uint64_t cpu)const{return uint32_t(detail::cvr_ds_query(store_,5,cpu));}
    void Copy(uint64_t destination,uint64_t source,uint32_t count,uint32_t stride){
        detail::cvr_ds_change(store_,4,destination,source,0,count,stride);
    }
    RangeIdentity Range(uint64_t gpu,uint32_t count)const{RangeIdentity range;detail::cvr_ds_range(store_,gpu,count,&range);return range;}
    // The Rust store itself, for the owner's Rust half (descriptor audit).
    const void* Rust()const{return store_;}
private:
    void* store_;
};
#else
class Store {
    struct Heap {uint64_t cpu{},gpu{},cookie{};uint32_t count{},stride{};std::vector<uint32_t> slots;};
    static constexpr uint64_t PageBytes=4096;
    struct Interval {uint16_t begin{},end{};uint64_t version{};};
    struct VersionPage {
        std::vector<Interval> intervals;
        uint64_t latest{};
        void Stamp(uint16_t begin,uint16_t end,uint64_t version) {
            auto first=std::lower_bound(intervals.begin(),intervals.end(),begin,[](const auto& i,auto p){return i.end<=p;});
            latest=version;
            if(first!=intervals.end() && first->begin==begin && first->end==end){first->version=version;return;}
            auto last=std::lower_bound(first,intervals.end(),end,[](const auto& i,auto p){return i.begin<p;});
            std::array<Interval,3> replacement{};size_t count{};
            if(first!=last && first->begin<begin)replacement[count++]={first->begin,begin,first->version};
            replacement[count++]={begin,end,version};
            if(first!=last && std::prev(last)->end>end)replacement[count++]={end,std::prev(last)->end,std::prev(last)->version};
            const auto position=intervals.erase(first,last);
            intervals.insert(position,replacement.begin(),replacement.begin()+count);
        }
        uint64_t Version(uint16_t begin,uint16_t end)const {
            if(!begin && end==PageBytes)return latest;
            uint64_t result{};
            auto it=std::lower_bound(intervals.begin(),intervals.end(),begin,[](const auto& i,auto p){return i.end<=p;});
            for(;it!=intervals.end() && it->begin<end;++it)result=std::max(result,it->version);
            return result;
        }
        void Erase(uint16_t begin,uint16_t end) {
            auto first=std::lower_bound(intervals.begin(),intervals.end(),begin,[](const auto& i,auto p){return i.end<=p;});
            auto last=std::lower_bound(first,intervals.end(),end,[](const auto& i,auto p){return i.begin<p;});
            std::array<Interval,2> tail{};size_t count{};
            if(first!=last && first->begin<begin)tail[count++]={first->begin,begin,first->version};
            if(first!=last && std::prev(last)->end>end)tail[count++]={end,std::prev(last)->end,std::prev(last)->version};
            const auto position=intervals.erase(first,last);intervals.insert(position,tail.begin(),tail.begin()+count);
            latest=0;for(const auto& interval:intervals)latest=std::max(latest,interval.version);
        }
    };
    struct ValueHash {
        size_t operator()(const Value& value)const noexcept {
            uint64_t hash=0xcbf29ce484222325ull;
            for(auto word:value){hash^=word;hash*=0x100000001b3ull;hash^=hash>>32;}
            return size_t(hash);
        }
    };
    std::vector<Heap> heaps_;
    std::unordered_map<uint64_t,uint32_t> slots_;
    bool dense_=true;
    size_t denseSlots_{},denseAllocated_{};
    mutable std::array<size_t,4> heapHints_{SIZE_MAX,SIZE_MAX,SIZE_MAX,SIZE_MAX};
    // Small, contiguous interval tables avoid allocating and traversing a tree
    // node for every descriptor write. Coordinates remain byte-exact: opaque
    // handles need not have any assumed alignment or fixed device stride.
    std::map<uint64_t,VersionPage> changes_;
    std::unordered_multimap<size_t,uint32_t> ids_;
    std::vector<Value> values_{Value{}};
    std::vector<uint32_t> references_{0},freeIds_;
    std::vector<uint8_t> interned_{0};
    uint64_t version_{},versionFloor_{};
    bool trackVersions_=true;
    size_t intervals_{};
    uint64_t lastPageKey=UINT64_MAX;
    VersionPage* lastPage{};
    uint32_t overflow_{};
    mutable std::map<std::pair<uint64_t,uint32_t>,RangeIdentity> largeRanges_;
    void (*retireValue_)(const Value&){};
    void ClearHeapHints(){heapHints_.fill(SIZE_MAX);}
    size_t DenseHeap(uint64_t cpu)const {
        auto contains=[&](size_t i){return i<heaps_.size() && !heaps_[i].slots.empty() && cpu>=heaps_[i].cpu &&
            cpu-heaps_[i].cpu<uint64_t(heaps_[i].count)*heaps_[i].stride && (cpu-heaps_[i].cpu)%heaps_[i].stride==0;};
        for(auto i:heapHints_)if(contains(i))return i;
        for(size_t i=0;i<heaps_.size();++i)if(contains(i)){
            for(size_t j=heapHints_.size()-1;j;--j)heapHints_[j]=heapHints_[j-1];heapHints_[0]=i;return i;
        }
        return SIZE_MAX;
    }
    void Release(uint32_t id) {
        if(id && references_[id] && --references_[id]==0){
            if(retireValue_)retireValue_(values_[id]);
            if(interned_[id]){
                const auto [first,last]=ids_.equal_range(ValueHash{}(values_[id]));
                for(auto it=first;it!=last;++it)if(it->second==id){ids_.erase(it);break;}
            }
            values_[id]={};interned_[id]=0;freeIds_.push_back(id);
        }
    }
    void Set(uint64_t cpu,uint32_t id) {
        if(dense_){const auto index=DenseHeap(cpu);if(index!=SIZE_MAX){
            auto& heap=heaps_[index];auto& slot=heap.slots[size_t((cpu-heap.cpu)/heap.stride)];const auto old=slot;
            if(old==id)return;
            if(id){if(!old && denseSlots_+slots_.size()>=SlotLimit){overflow_|=4;return;}
                ++references_[id];if(!old)++denseSlots_;}
            else if(old)--denseSlots_;
            slot=id;Release(old);return;
        }}
        const auto found=slots_.find(cpu);const auto old=found==slots_.end()?0:found->second;
        if(old==id)return;
        if(id){if(found==slots_.end() && denseSlots_+slots_.size()>=SlotLimit){overflow_|=4;return;}
            slots_[cpu]=id;++references_[id];}
        else if(found!=slots_.end())slots_.erase(found);
        Release(old);
    }
    void EraseSlots(uint64_t begin,uint64_t end) {
        for(auto& heap:heaps_)if(!heap.slots.empty() && begin<heap.cpu+uint64_t(heap.count)*heap.stride && heap.cpu<end){
            auto index=[&](uint64_t address){if(address<=heap.cpu)return uint64_t(0);
                const auto delta=address-heap.cpu;return std::min(uint64_t(heap.count),delta/heap.stride+uint64_t(delta%heap.stride!=0));};
            const auto first=index(begin),last=index(end);
            for(auto i=first;i<last;++i)if(auto& slot=heap.slots[size_t(i)];slot){Release(slot);slot=0;--denseSlots_;}
        }
        for(auto it=slots_.begin();it!=slots_.end();)if(it->first>=begin && it->first<end){Release(it->second);it=slots_.erase(it);}else ++it;
    }
    static bool End(uint64_t begin,uint64_t count,uint64_t stride,uint64_t& end) {
        if(!begin || !count || !stride || count>(UINT64_MAX-begin)/stride)return false;
        end=begin+count*stride;return true;
    }
    void Stamp(uint64_t begin,uint64_t end) {
        if(!trackVersions_)return;
        for(auto it=largeRanges_.begin();it!=largeRanges_.end();) {
            const auto& r=it->second;
            if(begin<r.cpu+uint64_t(r.count)*r.stride && r.cpu<end)it=largeRanges_.erase(it);else ++it;
        }
        const auto version=++version_;
        while(begin<end){
            const auto key=begin/PageBytes;
            if(!lastPage || key!=lastPageKey){lastPage=&changes_[key];lastPageKey=key;}
            const auto first=uint16_t(begin%PageBytes);
            const auto count=std::min(end-begin,PageBytes-first);
            const auto previous=lastPage->intervals.size();lastPage->Stamp(first,uint16_t(first+count),version);
            intervals_=intervals_-previous+lastPage->intervals.size();begin+=count;
            if(intervals_>SlotLimit){overflow_|=8;return;}
        }
    }
    void Forget(uint64_t begin,uint64_t end) {
        for(auto it=largeRanges_.begin();it!=largeRanges_.end();){const auto& r=it->second;
            if(begin<r.cpu+uint64_t(r.count)*r.stride && r.cpu<end)it=largeRanges_.erase(it);else ++it;}
        for(auto it=changes_.lower_bound(begin/PageBytes);it!=changes_.end() && it->first<=(end-1)/PageBytes;){
            const auto base=it->first*PageBytes;
            const auto first=uint16_t(begin>base?begin-base:0),last=uint16_t(std::min(end-base,PageBytes));
            const auto before=it->second.intervals.size();it->second.Erase(first,last);
            intervals_=intervals_-before+it->second.intervals.size();
            if(it->second.intervals.empty()){
                if(lastPage==&it->second){lastPage=nullptr;lastPageKey=UINT64_MAX;}
                it=changes_.erase(it);
            }else ++it;
        }
    }
public:
    explicit Store(bool dense=true):dense_(dense){}
    Store(const Store&)=delete;
    Store& operator=(const Store&)=delete;
    // Aggregate CPU metadata budget, not a per-shader-visible-heap size limit.
    // Keep descriptor values only once; the interning index stores hashes/IDs.
    static constexpr size_t SlotLimit=4*1048576,ValueLimit=1048576;
    // Called under the owner's metadata lock only when the last slot releases
    // a canonical value, including a streaming value overwritten in place.
    void SetValueRetirement(void(*callback)(const Value&)){retireValue_=callback;}
    bool Overflow()const{return overflow_;}
    std::array<uint64_t,6> Stats()const{return {overflow_,heaps_.size(),slots_.size()+denseSlots_,ids_.size(),intervals_,largeRanges_.size()};}
    bool Dense()const{return dense_;}
    bool TrackVersions()const{return trackVersions_;}
    // A caller may suspend history only when no captured ranges are awaiting
    // submission. Current descriptor values and their ownership remain live.
    void TrackVersions(bool enabled) {
        if(enabled==trackVersions_)return;
        changes_.clear();largeRanges_.clear();intervals_=0;lastPage=nullptr;lastPageKey=UINT64_MAX;
        versionFloor_=++version_;trackVersions_=enabled;
    }
    // Representation-only change under the caller's metadata lock. Descriptor
    // IDs, reference counts, heap cookies and every range version stay intact.
    void Dense(bool enabled) {
        if(enabled==dense_)return;
        auto pending=slots_;
        std::vector<std::vector<uint32_t>> heaps(heaps_.size());size_t allocated{},occupied{};
        if(enabled){
            for(size_t i=0;i<heaps_.size();++i){const auto& h=heaps_[i];
                if(h.count>SlotLimit-allocated)continue;
                heaps[i].resize(h.count);allocated+=h.count;
                for(auto it=pending.begin();it!=pending.end();){const auto address=it->first;
                    if(address>=h.cpu && address-h.cpu<uint64_t(h.count)*h.stride && (address-h.cpu)%h.stride==0){
                        heaps[i][size_t((address-h.cpu)/h.stride)]=it->second;++occupied;it=pending.erase(it);
                    }else ++it;
                }
            }
        }else {
            pending.reserve(pending.size()+denseSlots_);
            for(const auto& h:heaps_)for(size_t i=0;i<h.slots.size();++i)if(h.slots[i])pending.emplace(h.cpu+uint64_t(i)*h.stride,h.slots[i]);
        }
        for(size_t i=0;i<heaps_.size();++i)heaps_[i].slots.swap(heaps[i]);
        slots_.swap(pending);denseAllocated_=allocated;denseSlots_=occupied;dense_=enabled;ClearHeapHints();
    }
    size_t Values()const{return values_.size();}
    uint64_t HeapSlots()const{uint64_t count{};for(const auto& heap:heaps_)count+=heap.count;return count;}
    const Value* Get(uint32_t id)const{return id && id<values_.size() && values_[id][0]?&values_[id]:nullptr;}
    uint32_t Intern(const Value& value) {
        if(!value[0] || overflow_)return 0;
        const auto hash=ValueHash{}(value);const auto [first,last]=ids_.equal_range(hash);
        for(auto it=first;it!=last;++it)if(values_[it->second]==value)return it->second;
        uint32_t id{};
        if(!freeIds_.empty()){id=freeIds_.back();freeIds_.pop_back();values_[id]=value;}
        else {if(values_.size()>=ValueLimit){overflow_|=1;return 0;}
            id=uint32_t(values_.size());values_.push_back(value);references_.push_back(0);interned_.push_back(0);}
        interned_[id]=1;ids_.emplace(hash,id);return id;
    }
    bool HeapCreated(uint64_t cpu,uint64_t gpu,uint32_t count,uint32_t stride,uint64_t cookie) {
        if(overflow_)return false;
        uint64_t end{},gpuEnd{};if(!cookie || !End(cpu,count,stride,end) || (gpu && !End(gpu,count,stride,gpuEnd)))return false;
        for(auto it=heaps_.begin();it!=heaps_.end();) {
            const auto oldEnd=it->gpu+uint64_t(it->count)*it->stride;
            const auto oldCpuEnd=it->cpu+uint64_t(it->count)*it->stride;
            if((gpu && it->gpu && gpu<oldEnd && it->gpu<gpuEnd)||(cpu<oldCpuEnd && it->cpu<end)) {
                if(it->cpu==cpu && it->gpu==gpu && it->cookie==cookie && it->count==count && it->stride==stride)return true;
                EraseSlots(it->cpu,oldCpuEnd);Stamp(it->cpu,oldCpuEnd);largeRanges_.clear();
                denseAllocated_-=it->slots.size();ClearHeapHints();
                it=heaps_.erase(it);
            }else ++it;
        }
        if(heaps_.size()>=1024){overflow_|=2;return false;}
        // A new heap at an old CPU address must not inherit any old descriptors.
        EraseSlots(cpu,end);Stamp(cpu,end);
        Heap heap{cpu,gpu,cookie,count,stride};
        if(dense_ && count<=SlotLimit-denseAllocated_)heap.slots.resize(count);
        const auto allocated=heap.slots.size();heaps_.push_back(std::move(heap));denseAllocated_+=allocated;ClearHeapHints();return true;
    }
    void HeapRemoved(uint64_t cookie) {
        for(auto it=heaps_.begin();it!=heaps_.end();++it)if(it->cookie==cookie){
            // Known native heap slots are exactly base + index * stride.
            for(uint32_t index=0;index<it->count;++index)Set(it->cpu+uint64_t(index)*it->stride,0);
            Forget(it->cpu,it->cpu+uint64_t(it->count)*it->stride);denseAllocated_-=it->slots.size();heaps_.erase(it);ClearHeapHints();return;
        }
    }
    // Private heaps never enter this store. A private operation that writes an
    // already-tracked heap must still invalidate its exact destination range,
    // without reading untracked source descriptors or interning new metadata.
    TrackedDescriptorWrite InvalidateTracked(uint64_t cpu,uint32_t count,uint32_t stride){
        if(overflow_)return TrackedDescriptorWrite::Rejected;
        if(!count)return TrackedDescriptorWrite::Ignored;
        uint64_t end{};
        if(!End(cpu,count,stride,end)){overflow_|=16;return TrackedDescriptorWrite::Rejected;}
        bool touched{},malformed{};
        for(const auto& heap:heaps_){const auto heapEnd=heap.cpu+uint64_t(heap.count)*heap.stride;
            if(cpu>=heapEnd || heap.cpu>=end)continue;touched=true;
            if(cpu<heap.cpu || end>heapEnd || stride!=heap.stride || (cpu-heap.cpu)%heap.stride || count>SlotLimit){
                EraseSlots(heap.cpu,heapEnd);Stamp(heap.cpu,heapEnd);malformed=true;
            }else{EraseSlots(cpu,end);Stamp(cpu,end);}
        }
        if(malformed){overflow_|=16;return TrackedDescriptorWrite::Rejected;}
        if(overflow_)return TrackedDescriptorWrite::Rejected;
        return touched?TrackedDescriptorWrite::Invalidated:TrackedDescriptorWrite::Ignored;
    }
    void Write(uint64_t cpu,uint32_t stride,uint32_t id) {
        if(overflow_)return;
        uint64_t end{};if(!End(cpu,1,stride,end)){overflow_|=16;return;}
        Set(cpu,Get(id)?id:0);
        Stamp(cpu,end);
    }
    // Streaming CBVs rarely repeat, so avoid hashing/interning their full API
    // description. Copied descriptors still share immutable values; overwrite
    // in place only when this slot is their sole owner.
    void WriteValue(uint64_t cpu,uint32_t stride,const Value& value) {
        // Origin-bearing values share one ID so its final retirement represents
        // every descriptor slot retaining that immutable creation receipt.
        if(value[0]!=1 || value[3]){Write(cpu,stride,Intern(value));return;}
        if(overflow_)return;
        uint64_t end{};if(!End(cpu,1,stride,end)){overflow_|=16;return;}
        const auto old=At(cpu);
        if(old && values_[old]==value){Stamp(cpu,end);return;}
        if(old && references_[old]==1 && !interned_[old]){
            if(retireValue_)retireValue_(values_[old]);values_[old]=value;Stamp(cpu,end);return;
        }
        uint32_t id{};
        if(!freeIds_.empty()){id=freeIds_.back();freeIds_.pop_back();values_[id]=value;}
        else {if(values_.size()>=ValueLimit){overflow_|=1;return;}
            id=uint32_t(values_.size());values_.push_back(value);references_.push_back(0);interned_.push_back(0);}
        Set(cpu,id);Stamp(cpu,end);
    }
    uint32_t At(uint64_t cpu)const {
        if(dense_){const auto index=DenseHeap(cpu);if(index!=SIZE_MAX){const auto& heap=heaps_[index];return heap.slots[size_t((cpu-heap.cpu)/heap.stride)];}}
        const auto it=slots_.find(cpu);return it==slots_.end()?0:it->second;
    }
    void Copy(uint64_t destination,uint64_t source,uint32_t count,uint32_t stride) {
        if(overflow_)return;
        uint64_t end{},sourceEnd{};if(!End(destination,count,stride,end)||!End(source,count,stride,sourceEnd)||count>SlotLimit){overflow_|=16;return;}
        // D3D12 forbids overlap. Treat it as unknown rather than guessing an
        // order; a metadata copy must never leave stale destination contents.
        if(destination<sourceEnd && source<end){EraseSlots(destination,end);Stamp(destination,end);return;}
        for(uint32_t i=0;i<count;++i){const auto dest=destination+uint64_t(i)*stride;const auto value=At(source+uint64_t(i)*stride);
            Set(dest,value);}
        Stamp(destination,end);
    }
    RangeIdentity Range(uint64_t gpu,uint32_t count)const {
        if(overflow_ || !count)return {};
        if(count>128){const auto cached=largeRanges_.find({gpu,count});if(cached!=largeRanges_.end())return cached->second;}
        for(const auto& heap:heaps_) {
            if(!heap.gpu)continue;
            const auto end=heap.gpu+uint64_t(heap.count)*heap.stride;
            if(gpu<heap.gpu || gpu>=end || (gpu-heap.gpu)%heap.stride)continue;
            const auto actual=count==UINT32_MAX?uint32_t((end-gpu)/heap.stride):count;
            if(uint64_t(actual)*heap.stride>end-gpu)continue;
            const auto cpu=heap.cpu+gpu-heap.gpu,last=cpu+uint64_t(actual)*heap.stride;
            uint64_t version=trackVersions_?versionFloor_:0;
            for(auto it=changes_.lower_bound(cpu/PageBytes);it!=changes_.end() && it->first<=(last-1)/PageBytes;++it){
                const auto base=it->first*PageBytes;
                const auto first=uint16_t(cpu>base?cpu-base:0),pageEnd=uint16_t(std::min(last-base,PageBytes));
                version=std::max(version,it->second.Version(first,pageEnd));
            }
            const RangeIdentity result{heap.cookie,cpu,gpu,version,actual,heap.stride,true};
            if(count>128 && largeRanges_.size()<128)largeRanges_[{gpu,count}]=result;
            return result;
        }
        return {};
    }
};
#endif
}
