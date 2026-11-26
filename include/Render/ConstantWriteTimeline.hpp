#pragma once
#include <algorithm>
#include <array>
#include <cstdint>
#if CVR_RUST_VRIK
#include <cstddef>
#include <utility>
#endif
#include <map>
#include <span>
#include <tuple>
#include <vector>
#include "Render/GpuQueueOrder.hpp"

namespace cvr::stereo::constants {
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/constant_timeline.rs (tools/single_pass_tests/constant_timeline_test runs its cases
// against them in its Rust variant). The nested records cross by address, their layouts asserted in the class.
namespace timeline_detail {
struct RustTimeline;
extern "C" {
RustTimeline* cvr_cwt_new(const RustTimeline*);
void cvr_cwt_free(RustTimeline*);
void cvr_cwt_mark(RustTimeline*,uint64_t,void*);
uint32_t cvr_cwt_write(RustTimeline*,const void*,uint64_t,uint64_t,bool,uint64_t);
void cvr_cwt_alias(RustTimeline*,const void*,const void*);
void cvr_cwt_set(RustTimeline*,uint32_t,uint32_t,uint64_t,const void*);
void cvr_cwt_op(RustTimeline*,uint32_t,uint64_t,uint64_t,uint64_t);
void cvr_cwt_submit(RustTimeline*,uint64_t,uint64_t,const uint64_t*,size_t);
void cvr_cwt_resolve(const RustTimeline*,const void*,uint64_t,uint64_t,void*);
void cvr_cwt_stats(const RustTimeline*,std::array<uint32_t,5>*);
void cvr_cwt_describe(const RustTimeline*,const void*,void*);
size_t cvr_cwt_related(const RustTimeline*,const void*,uint64_t,uint64_t,void*,size_t);
bool cvr_cwt_could_alias(const void*,const void*);
}
}
#endif
// CPU recording order is deliberately absent from cross-list comparisons.
// The caller supplies actual queue submission order and witnessed payload IDs.
class ConstantWriteTimeline {
public:
    struct Position {uint32_t recording{};uint64_t command{};};
    enum class Status {Ready, Missing, Pending, UnknownWrite, CrossQueue, Abandoned, Overflow, Replayed};
    enum class Detail {None, UnknownWrite, PartialRange, AliasBarrier, UnsupportedDestination, UnmappedSource, UnstableSource, CopyTooLarge};
    struct Result {Status status=Status::Missing;uint64_t payload{},offset{};Detail detail{};};
    struct Context {uint64_t queue{},batch{};uint32_t index{},node{};};
    struct RelatedWrite {uint32_t id{},relation{};Position point;Context context;uint64_t payload{};};
    struct MemorySpan {uint64_t heap{},offset{},bytes{};};
#if CVR_RUST_VRIK
    static bool CouldAlias(MemorySpan a,MemorySpan b){return timeline_detail::cvr_cwt_could_alias(&a,&b);}
    static constexpr uint32_t Capacity=16384;
    ConstantWriteTimeline():rust_(timeline_detail::cvr_cwt_new(nullptr)){}
    ConstantWriteTimeline(const ConstantWriteTimeline& other):rust_(timeline_detail::cvr_cwt_new(other.rust_)){}
    ConstantWriteTimeline(ConstantWriteTimeline&& other)noexcept:rust_(std::exchange(other.rust_,timeline_detail::cvr_cwt_new(nullptr))){}
    ConstantWriteTimeline& operator=(ConstantWriteTimeline other)noexcept{std::swap(rust_,other.rust_);return *this;}
    ~ConstantWriteTimeline(){timeline_detail::cvr_cwt_free(rust_);}

    Position Mark(uint64_t list){Position point;timeline_detail::cvr_cwt_mark(rust_,list,&point);return point;}
    uint32_t Write(Position point,uint64_t address,uint64_t size,bool committed,uint64_t payload=0){
        return timeline_detail::cvr_cwt_write(rust_,&point,address,size,committed,payload);
    }
    void Alias(Position point,MemorySpan memory={}){timeline_detail::cvr_cwt_alias(rust_,&point,&memory);}
    void Payload(uint32_t write,uint64_t payload){timeline_detail::cvr_cwt_set(rust_,0,write,payload,nullptr);}
    void Explanation(uint32_t write,Detail detail){timeline_detail::cvr_cwt_set(rust_,1,write,uint64_t(uint32_t(detail)),nullptr);}
    void Memory(uint32_t write,MemorySpan memory){timeline_detail::cvr_cwt_set(rust_,2,write,0,&memory);}
    void Reset(uint64_t list){timeline_detail::cvr_cwt_op(rust_,0,list,0,0);}
    void Submit(uint64_t queue,uint64_t batch,std::span<const uint64_t> lists){
        timeline_detail::cvr_cwt_submit(rust_,queue,batch,lists.data(),lists.size());
    }
    Result Resolve(Position read,uint64_t address,uint64_t bytes) const {
        Result out;timeline_detail::cvr_cwt_resolve(rust_,&read,address,bytes,&out);return out;
    }
    std::array<uint32_t,5> Stats() const {std::array<uint32_t,5> out{};timeline_detail::cvr_cwt_stats(rust_,&out);return out;}
    Context Describe(Position position) const {Context out;timeline_detail::cvr_cwt_describe(rust_,&position,&out);return out;}
    std::vector<RelatedWrite> Related(Position read,uint64_t address,uint64_t bytes) const {
        std::vector<RelatedWrite> result(timeline_detail::cvr_cwt_related(rust_,&read,address,bytes,nullptr,0));
        if(!result.empty())timeline_detail::cvr_cwt_related(rust_,&read,address,bytes,result.data(),result.size());
        return result;
    }
    void Fence(uint64_t fence,uint64_t completed){timeline_detail::cvr_cwt_op(rust_,2,fence,completed,0);}
    void Signal(uint64_t queue,uint64_t fence,uint64_t value){timeline_detail::cvr_cwt_op(rust_,3,queue,fence,value);}
    void Wait(uint64_t queue,uint64_t fence,uint64_t value){timeline_detail::cvr_cwt_op(rust_,4,queue,fence,value);}
    void InvalidateFence(uint64_t fence){timeline_detail::cvr_cwt_op(rust_,5,fence,0,0);}
    void FenceProgress(uint64_t fence,uint64_t completed){timeline_detail::cvr_cwt_op(rust_,6,fence,completed,0);}
    void Clear(){timeline_detail::cvr_cwt_op(rust_,1,0,0,0);}
private:
    static_assert(sizeof(Position)==16 && sizeof(Result)==32 && sizeof(Context)==24 && sizeof(RelatedWrite)==56 &&
        sizeof(MemorySpan)==24 && offsetof(Result,detail)==24 && offsetof(RelatedWrite,payload)==48,"mirrored by constant_timeline.rs");
    timeline_detail::RustTimeline* rust_;
#else
    static bool CouldAlias(MemorySpan a,MemorySpan b) {
        if(!a.heap || !b.heap || !a.bytes || !b.bytes || a.offset>UINT64_MAX-a.bytes || b.offset>UINT64_MAX-b.bytes)return true;
        return a.heap==b.heap && a.offset<b.offset+b.bytes && b.offset<a.offset+a.bytes;
    }
    static constexpr uint32_t Capacity=16384;

    Position Mark(uint64_t list) {
        if(failed_ || !list)return {};
        auto found=current_.find(list);
        if(found==current_.end()) {
            if(recordings_.size()>=Capacity){failed_=true;return {};}
            found=current_.emplace(list,uint32_t(recordings_.size())).first;
            recordings_.push_back({});
        }
        auto& recording=recordings_[found->second];
        if(recording.submitted){failed_=true;return {};}
        return {found->second,++recording.commands};
    }
    uint32_t Write(Position point,uint64_t address,uint64_t size,bool committed,uint64_t payload=0) {
        if(failed_ || !Valid(point) || !address || !size || address>UINT64_MAX-size)return 0;
        if(writes_.size()>=Capacity){failed_=true;return 0;}
        writes_.push_back({point,address,size,payload,committed,false});
        return uint32_t(writes_.size());
    }
    void Alias(Position point,MemorySpan memory={}) {
        if(failed_ || !Valid(point))return;
        if(writes_.size()>=Capacity){failed_=true;return;}
        writes_.push_back({point,0,0,0,false,true,Detail::UnknownWrite,memory});
    }
    void Payload(uint32_t write,uint64_t payload) {
        if(write && write<=writes_.size())writes_[write-1].payload=payload;
    }
    void Explanation(uint32_t write,Detail detail){if(write && write<=writes_.size())writes_[write-1].detail=detail;}
    void Memory(uint32_t write,MemorySpan memory){if(write && write<=writes_.size())writes_[write-1].memory=memory;}
    void Reset(uint64_t list) {
        const auto found=current_.find(list);if(found==current_.end())return;
        auto& old=recordings_[found->second];
        if(!old.submitted)old.abandoned=true;
        current_.erase(found);
    }
    void Submit(uint64_t queue,uint64_t batch,std::span<const uint64_t> lists) {
        if(failed_ || !queue || !batch)return;
        ++submits_;queues_[queue]=true;
        const auto node=order_.Submit(queue);
        for(uint32_t i=0;i<lists.size();++i) {
            const auto found=current_.find(lists[i]);if(found==current_.end())continue;
            auto& value=recordings_[found->second];
            if(value.submitted){value.replayed=true;++replays_;continue;}
            value.queue=queue;value.batch=batch;value.index=i;value.submitted=true;value.node=node;
        }
    }
    Result Resolve(Position read,uint64_t address,uint64_t bytes) const {
        if(failed_ || order_.Failed())return {Status::Overflow};
        if(!Valid(read) || !address || !bytes || address>UINT64_MAX-bytes)return {};
        const auto& consumer=recordings_[read.recording];
        if(consumer.abandoned || consumer.replayed)return {Status::Abandoned};
        if(!consumer.submitted)return {Status::Pending};
        std::vector<const WriteEvent*> maximal;
        bool pending=false,crossQueue=false,replayed=false;
        for(const auto& write:writes_) {
            if(write.alias || !Overlap(write,address,bytes))continue;
            const auto relation=Before(write.point,read);
            if(relation==Relation::Pending){pending=true;continue;}
            if(relation==Relation::Unordered){crossQueue=true;continue;}
            if(relation==Relation::Replayed){replayed=true;continue;}
            if(relation!=Relation::Before)continue;
            // Old incomparable writes no longer matter after a complete write
            // proven later than both. Keep only maximal predecessors of read.
            const bool superseded=std::any_of(maximal.begin(),maximal.end(),[&](const auto* other){
                return Before(write.point,other->point)==Relation::Before;
            });
            if(superseded)continue;
            std::erase_if(maximal,[&](const auto* other){return Before(other->point,write.point)==Relation::Before;});
            maximal.push_back(&write);
        }
        for(size_t i=0;i<maximal.size();++i)for(size_t j=i+1;j<maximal.size();++j) {
            const auto relation=Before(maximal[i]->point,maximal[j]->point);
            pending|=relation==Relation::Pending;crossQueue|=relation!=Relation::Pending;
        }
        if(replayed)return {Status::Replayed};
        if(crossQueue)return {Status::CrossQueue};
        if(pending)return {Status::Pending};
        if(maximal.empty())return {};
        const auto* latest=maximal.front();
        // Partial writes are not reconstructed from unrelated historical bytes.
        if(!latest->payload)return {Status::UnknownWrite,0,0,latest->detail};
        if(address<latest->address || address-latest->address>latest->size ||
           bytes>latest->size-(address-latest->address))return {Status::UnknownWrite,0,0,Detail::PartialRange};
        if(!latest->committed)for(const auto& write:writes_)if(write.alias) {
            if(!CouldAlias(latest->memory,write.memory))continue;
            const auto relation=Before(write.point,read);
            if(relation==Relation::Pending)return {Status::Pending};
            if(relation==Relation::Unordered)return {Status::CrossQueue};
            if(relation==Relation::Replayed)return {Status::Replayed};
            if(relation==Relation::Before) {
                const auto newer=Before(latest->point,write.point);
                if(newer==Relation::Before)return {Status::UnknownWrite,0,0,Detail::AliasBarrier};
                if(newer==Relation::Pending)return {Status::Pending};
                if(newer!=Relation::After)return {Status::CrossQueue};
            }
        }
        return {Status::Ready,latest->payload,address-latest->address};
    }
    std::array<uint32_t,5> Stats() const {return {uint32_t(recordings_.size()-1),uint32_t(writes_.size()),submits_,replays_,uint32_t(queues_.size())};}
    Context Describe(Position position) const {
        if(!Valid(position))return {};const auto& r=recordings_[position.recording];return {r.queue,r.batch,r.index,r.node};
    }
    std::vector<RelatedWrite> Related(Position read,uint64_t address,uint64_t bytes) const {
        std::vector<RelatedWrite> result;if(!Valid(read) || address>UINT64_MAX-bytes)return result;
        for(uint32_t i=0;i<writes_.size();++i)if(!writes_[i].alias && Overlap(writes_[i],address,bytes)) {
            const auto& write=writes_[i];result.push_back({i+1,uint32_t(Before(write.point,read)),write.point,Describe(write.point),write.payload});
        }
        return result;
    }
    void Fence(uint64_t fence,uint64_t completed){order_.Fence(fence,completed);}
    void Signal(uint64_t queue,uint64_t fence,uint64_t value){order_.Signal(queue,fence,value);}
    void Wait(uint64_t queue,uint64_t fence,uint64_t value){order_.Wait(queue,fence,value);}
    void InvalidateFence(uint64_t fence){order_.Invalidate(fence);}
    void FenceProgress(uint64_t fence,uint64_t completed){order_.Progress(fence,completed);}
    void Clear(){current_.clear();recordings_.assign(1,{});writes_.clear();failed_=false;queues_.clear();submits_=replays_=0;order_.Clear();}
private:
#endif
#if !CVR_RUST_VRIK
    struct Recording {
        uint64_t commands{},queue{},batch{};uint32_t index{},node{};
        bool submitted{},abandoned{},replayed{};
    };
    struct WriteEvent {Position point;uint64_t address{},size{},payload{};bool committed{},alias{};Detail detail=Detail::UnknownWrite;MemorySpan memory;};
    enum class Relation {Before, After, Pending, Unordered, Abandoned, Replayed};
    std::map<uint64_t,uint32_t> current_;
    std::vector<Recording> recordings_{1};
    std::vector<WriteEvent> writes_;
    bool failed_{};
    std::map<uint64_t,bool> queues_;
    uint32_t submits_{},replays_{};
    GpuQueueOrder order_;
    bool Valid(Position point) const {
        return point.recording && point.recording<recordings_.size() && point.command &&
            point.command<=recordings_[point.recording].commands;
    }
    bool Less(Position a,Position b) const {
        const auto& x=recordings_[a.recording];const auto& y=recordings_[b.recording];
        return std::tuple{x.batch,x.index,a.command}<std::tuple{y.batch,y.index,b.command};
    }
    Relation Before(Position a,Position b) const {
        const auto& producer=recordings_[a.recording];const auto& consumer=recordings_[b.recording];
        if(producer.abandoned)return Relation::Abandoned;
        if(producer.replayed)return Relation::Replayed;
        if(!producer.submitted)return Relation::Pending;
        if(producer.queue!=consumer.queue) {
            const auto before=order_.Reaches(consumer.node,producer.node);
            if(before.before)return Relation::Before;
            const auto after=order_.Reaches(producer.node,consumer.node);
            if(after.before)return Relation::After;
            return before.pending || after.pending?Relation::Pending:Relation::Unordered;
        }
        return Less(a,b)?Relation::Before:Relation::After;
    }
    static bool Overlap(const WriteEvent& write,uint64_t address,uint64_t bytes) {
        return write.address<address+bytes && address<write.address+write.size;
    }
#endif
};
}
