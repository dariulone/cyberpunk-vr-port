#pragma once
#include "Render/GpuQueueOrder.hpp"
#include <algorithm>
#include <cstdint>
#if CVR_RUST_VRIK
#include <cstddef>
#include <utility>
#endif
#include <map>
#include <span>
#include <tuple>
#include <vector>

namespace cvr::stereo::geometry {
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/geometry_write_receipt.rs (tools/single_pass_tests/geometry_write_receipt_test runs
// its cases against them in its Rust variant). The nested records cross by address, their layouts asserted in the class.
namespace write_receipt_detail {
struct RustReceipt;
extern "C" {
RustReceipt* cvr_gwr_new(const RustReceipt*);
void cvr_gwr_free(RustReceipt*);
void cvr_gwr_op(RustReceipt*,uint32_t,uint64_t,uint64_t,uint64_t);
void cvr_gwr_mark(RustReceipt*,uint64_t,void*);
bool cvr_gwr_add(RustReceipt*,uint32_t,const void*,const void*,const void*);
void cvr_gwr_capture(const RustReceipt*,const void*,const void*,void*);
bool cvr_gwr_submit(RustReceipt*,uint64_t,const uint64_t*,size_t);
uint32_t cvr_gwr_check(const RustReceipt*,const void*,const void*);
}
}
#endif
// Caller serializes recording and queue evidence, supplies stable lifetime IDs,
// and retains the resources/heaps/fences. No GPU objects or payloads are owned.
// Feed every possible writer from an observed Reset; unknown submissions remain
// opaque. Check after provisional peer Submit, under the queue-order lock and
// before admission freezes. The actual peer binding must also match the range.
class GeometryWriteReceipt {
public:
    static constexpr uint32_t Capacity=16384;
    enum class Status {Unchanged, MissingProvenance, Pending, Abandoned, Replayed,
                       Incomparable, IntersectingWrite, Aliased, Overflow};
    struct Position {uint64_t epoch{};uint32_t recording{};uint64_t command{};};
    struct MemorySpan {uint64_t heap{},offset{},bytes{};};
    struct Range {
        uint64_t resource{},offset{},bytes{};
        // For placed buffers, the allocation begins at resource byte zero.
        MemorySpan allocation;
        bool committed{};
    };
    struct Receipt {Position main;Range range;bool valid{};};

#if CVR_RUST_VRIK
    GeometryWriteReceipt():rust_(write_receipt_detail::cvr_gwr_new(nullptr)){}
    GeometryWriteReceipt(const GeometryWriteReceipt& other):rust_(write_receipt_detail::cvr_gwr_new(other.rust_)){}
    GeometryWriteReceipt(GeometryWriteReceipt&& other)noexcept:rust_(std::exchange(other.rust_,write_receipt_detail::cvr_gwr_new(nullptr))){}
    GeometryWriteReceipt& operator=(GeometryWriteReceipt other)noexcept{std::swap(rust_,other.rust_);return *this;}
    ~GeometryWriteReceipt(){write_receipt_detail::cvr_gwr_free(rust_);}
    void Reset(uint64_t list) noexcept {Op(0,list);}
    Position Mark(uint64_t list) noexcept {Position result;write_receipt_detail::cvr_gwr_mark(rust_,list,&result);return result;}
    bool Write(Position point,Range range) noexcept {return write_receipt_detail::cvr_gwr_add(rust_,0,&point,&range,nullptr);}
    // An empty or incomplete alias span cannot exclude any watched allocation.
    bool Alias(Position point,MemorySpan memory={}) noexcept {return write_receipt_detail::cvr_gwr_add(rust_,1,&point,nullptr,&memory);}
    bool UnknownWrite(Position point) noexcept {return write_receipt_detail::cvr_gwr_add(rust_,2,&point,nullptr,nullptr);}
    Receipt Capture(Position main,Range range) const noexcept {
        Receipt result;write_receipt_detail::cvr_gwr_capture(rust_,&main,&range,&result);return result;
    }
    bool Submit(uint64_t queue,std::span<const uint64_t> lists) noexcept {
        return write_receipt_detail::cvr_gwr_submit(rust_,queue,lists.data(),lists.size());
    }
    void Fence(uint64_t fence,uint64_t completed) noexcept {Op(1,fence,completed);}
    void Signal(uint64_t queue,uint64_t fence,uint64_t value) noexcept {Op(2,queue,fence,value);}
    void Wait(uint64_t queue,uint64_t fence,uint64_t value) noexcept {Op(3,queue,fence,value);}
    void Progress(uint64_t fence,uint64_t completed) noexcept {Op(4,fence,completed);}
    void InvalidateFence(uint64_t fence) noexcept {Op(5,fence);}
    Status Check(const Receipt& receipt,Position peer) const noexcept {
        return Status(write_receipt_detail::cvr_gwr_check(rust_,&receipt,&peer));
    }
    // Invalidates all receipts. Resume only after complete recording/queue
    // observation has been established again; this is not a frame-boundary reset.
    void Clear() noexcept {Op(6);}
private:
    static_assert(sizeof(Position)==24 && sizeof(MemorySpan)==24 && sizeof(Range)==56 && sizeof(Receipt)==88 &&
        offsetof(Range,committed)==48 && offsetof(Receipt,valid)==80,"mirrored by geometry_write_receipt.rs");
    void Op(uint32_t which,uint64_t a=0,uint64_t b=0,uint64_t c=0){write_receipt_detail::cvr_gwr_op(rust_,which,a,b,c);}
    write_receipt_detail::RustReceipt* rust_;
};
#else
    void Reset(uint64_t list) noexcept {
        Mutate([&]{
            if(!list){fault_=Status::MissingProvenance;return;}
            const auto old=current_.find(list);
            if(old!=current_.end() && !recordings_[old->second].submitted)recordings_[old->second].abandoned=true;
            NewRecording(list,true);
        });
    }
    Position Mark(uint64_t list) noexcept {
        Position result;
        Mutate([&]{
            const auto found=current_.find(list);if(found==current_.end())return;
            auto& r=recordings_[found->second];
            if(!r.known || r.submitted || r.abandoned)return;
            if(r.commands==UINT64_MAX){fault_=Status::Overflow;return;}
            result={epoch_,found->second,++r.commands};
        });
        return result;
    }
    bool Write(Position point,Range range) noexcept {
        if(fault_!=Status::Unchanged)return false;
        if(!ValidRange(range,false)){fault_=Status::MissingProvenance;return false;}
        return Add({point,range,{},Kind::Write});
    }
    // An empty or incomplete alias span cannot exclude any watched allocation.
    bool Alias(Position point,MemorySpan memory={}) noexcept {return Add({point,{},memory,Kind::Alias});}
    bool UnknownWrite(Position point) noexcept {return Add({point,{},{},Kind::Unknown});}
    Receipt Capture(Position main,Range range) const noexcept {
        return fault_==Status::Unchanged && Valid(main) && ValidRange(range,true)?Receipt{main,range,true}:Receipt{};
    }
    bool Submit(uint64_t queue,std::span<const uint64_t> lists) noexcept {
        bool result{};
        Mutate([&]{
            if(!queue || lists.empty()){fault_=Status::MissingProvenance;return;}
            if(lists.size()>Capacity){fault_=Status::Overflow;return;}
            if(batch_==UINT64_MAX){fault_=Status::Overflow;return;}
            const auto batch=++batch_;const auto node=Append(queue),orderNode=order_.Submit(queue);
            if(!node || !orderNode)return;
            for(size_t i=0;i<lists.size();++i){
                if(!lists[i] || i>UINT32_MAX){fault_=Status::MissingProvenance;return;}
                auto found=current_.find(lists[i]);
                if(found==current_.end()){
                    const auto id=NewRecording(lists[i],false);if(!id)return;
                    recordings_[id].commands=1;
                    if(!Add({{epoch_,id,1},{},{},Kind::Unknown}))return;
                    found=current_.find(lists[i]);
                }
                auto& r=recordings_[found->second];
                if(r.submitted){r.replayed=true;continue;}
                r.queue=queue;r.batch=batch;r.index=uint32_t(i);r.node=node;r.orderNode=orderNode;r.submitted=true;
            }
            result=true;
        });
        return result && fault_==Status::Unchanged;
    }
    void Fence(uint64_t fence,uint64_t completed) noexcept {
        Mutate([&]{
            if(!fence || completed==UINT64_MAX){fault_=Status::MissingProvenance;return;}
            if(fences_.contains(fence))return;
            if(fences_.size()>=Capacity){fault_=Status::Overflow;return;}
            fences_.emplace(fence,FenceState{completed,completed,0,completed});order_.Fence(fence,completed);
        });
    }
    void Signal(uint64_t queue,uint64_t fence,uint64_t value) noexcept {
        Mutate([&]{
            const auto found=fences_.find(fence);if(found==fences_.end()){fault_=Status::MissingProvenance;return;}
            const auto node=Append(queue);if(!node)return;
            auto& f=found->second;
            if(value<=f.highest || (f.owner && f.owner!=queue))f.invalid=true;
            else {f.owner=queue;f.highest=value;f.signals.emplace(value,node);}
            order_.Signal(queue,fence,value);
        });
    }
    void Wait(uint64_t queue,uint64_t fence,uint64_t value) noexcept {
        Mutate([&]{
            if(!fences_.contains(fence)){fault_=Status::MissingProvenance;return;}
            if(Append(queue,fence,value))order_.Wait(queue,fence,value);
        });
    }
    void Progress(uint64_t fence,uint64_t completed) noexcept {
        Mutate([&]{
            const auto found=fences_.find(fence);if(found==fences_.end()){fault_=Status::MissingProvenance;return;}
            auto& f=found->second;
            if(completed==UINT64_MAX || completed<f.completed){f.invalid=true;order_.Invalidate(fence);return;}
            f.completed=completed;order_.Progress(fence,completed);
        });
    }
    void InvalidateFence(uint64_t fence) noexcept {
        Mutate([&]{
            const auto found=fences_.find(fence);if(found==fences_.end()){fault_=Status::MissingProvenance;return;}
            found->second.invalid=true;order_.Invalidate(fence);
        });
    }
    Status Check(const Receipt& receipt,Position peer) const noexcept try {
        if(fault_!=Status::Unchanged)return fault_;
        if(order_.Failed())return Status::Overflow;
        if(!receipt.valid || !Valid(receipt.main) || !Valid(peer) || !ValidRange(receipt.range,true))return Status::MissingProvenance;
        for(const auto point:{receipt.main,peer}){
            const auto& r=recordings_[point.recording];
            if(!r.known)return Status::MissingProvenance;
            if(r.abandoned)return Status::Abandoned;
            if(r.replayed)return Status::Replayed;
            if(!r.submitted)return Status::Pending;
            const auto closed=Closed(r.node);if(closed!=Status::Unchanged)return closed;
        }
        const auto reads=Before(receipt.main,peer);
        if(reads!=Relation::Before)return reads==Relation::Pending?Status::Pending:Status::Incomparable;
        for(const auto& event:events_){
            const auto conflict=Overlap(receipt.range,event);if(conflict==Status::Unchanged)continue;
            const auto beforeMain=Before(event.point,receipt.main);
            if(beforeMain==Relation::Before || beforeMain==Relation::Abandoned)continue;
            if(beforeMain==Relation::Replayed)return Status::Replayed;
            const auto beforePeer=Before(event.point,peer);
            if(beforePeer==Relation::After)continue;
            if(beforeMain==Relation::Pending || beforePeer==Relation::Pending)return Status::Pending;
            if(beforeMain==Relation::Incomparable || beforePeer==Relation::Incomparable)return Status::Incomparable;
            return conflict;
        }
        return Status::Unchanged;
    }catch(...){return Status::Overflow;}
    // Invalidates all receipts. Resume only after complete recording/queue
    // observation has been established again; this is not a frame-boundary reset.
    void Clear() noexcept {
        if(epoch_==UINT64_MAX){fault_=Status::Overflow;return;}
        try {
            ++epoch_;batch_=0;current_.clear();recordings_.assign(1,{});events_.clear();
            queues_.clear();nodes_.assign(1,{});fences_.clear();closed_.clear();order_.Clear();fault_=Status::Unchanged;
        }catch(...){fault_=Status::Overflow;}
    }
private:
    enum class Kind {Write, Alias, Unknown};
    enum class Relation {Before, After, Equal, Pending, Abandoned, Replayed, Incomparable};
    struct Recording {
        uint64_t commands{},queue{},batch{};uint32_t index{},node{},orderNode{};
        bool known{},submitted{},abandoned{},replayed{};
    };
    struct Event {Position point;Range range;MemorySpan memory;Kind kind;};
    struct QueueNode {uint32_t previous{};uint64_t fence{},value{};};
    struct FenceState {
        uint64_t baseline{},completed{},owner{},highest{};bool invalid{};
        std::map<uint64_t,uint32_t> signals;
    };
    uint64_t epoch_=1,batch_{};
    Status fault_=Status::Unchanged;
    std::map<uint64_t,uint32_t> current_,queues_;
    std::vector<Recording> recordings_{1};
    std::vector<Event> events_;
    std::vector<QueueNode> nodes_{1};
    std::map<uint64_t,FenceState> fences_;
    mutable std::map<uint32_t,Status> closed_;
    constants::GpuQueueOrder order_;
    template<class F> void Mutate(F action) noexcept {
        if(fault_!=Status::Unchanged)return;
        try {closed_.clear();action();if(order_.Failed())fault_=Status::Overflow;}catch(...){fault_=Status::Overflow;}
    }
    uint32_t NewRecording(uint64_t list,bool known){
        if(recordings_.size()>Capacity){fault_=Status::Overflow;return 0;}
        const auto id=uint32_t(recordings_.size());Recording r;r.known=known;
        recordings_.push_back(r);current_[list]=id;return id;
    }
    bool Valid(Position p) const {
        return p.epoch==epoch_ && p.recording && p.recording<recordings_.size() && p.command && p.command<=recordings_[p.recording].commands;
    }
    static bool ValidMemory(MemorySpan m){return m.heap && m.bytes && m.offset<=UINT64_MAX-m.bytes;}
    static bool EmptyMemory(MemorySpan m){return !m.heap && !m.offset && !m.bytes;}
    static bool ValidRange(const Range& r,bool requireAllocation){
        if(!r.resource || !r.bytes || r.offset>UINT64_MAX-r.bytes)return false;
        if(r.committed)return EmptyMemory(r.allocation);
        if(EmptyMemory(r.allocation))return !requireAllocation;
        return ValidMemory(r.allocation) && r.offset<=r.allocation.bytes && r.bytes<=r.allocation.bytes-r.offset;
    }
    bool Add(Event event) noexcept {
        bool result{};Mutate([&]{
            if(!Valid(event.point) || recordings_[event.point.recording].submitted || recordings_[event.point.recording].abandoned){fault_=Status::MissingProvenance;return;}
            if(events_.size()>=Capacity){fault_=Status::Overflow;return;}
            events_.push_back(event);result=true;
        });return result;
    }
    uint32_t Append(uint64_t queue,uint64_t fence=0,uint64_t value=0){
        if(!queue){fault_=Status::MissingProvenance;return 0;}
        if(nodes_.size()>Capacity){fault_=Status::Overflow;return 0;}
        const auto id=uint32_t(nodes_.size());nodes_.push_back({queues_[queue],fence,value});queues_[queue]=id;return id;
    }
    Relation Before(Position a,Position b) const {
        const auto& x=recordings_[a.recording];const auto& y=recordings_[b.recording];
        if(x.abandoned)return Relation::Abandoned;
        if(x.replayed || y.replayed)return Relation::Replayed;
        if(!x.submitted || !y.submitted)return Relation::Pending;
        if(x.queue==y.queue){
            // One ExecuteCommandLists call can merge/overlap different lists.
            // Without barrier evidence, their array indices prove no ordering.
            if(x.batch==y.batch && a.recording!=b.recording)return Relation::Incomparable;
            const auto left=std::tuple{x.batch,x.index,a.command},right=std::tuple{y.batch,y.index,b.command};
            return left<right?Relation::Before:left>right?Relation::After:Relation::Equal;
        }
        const auto before=order_.Reaches(y.orderNode,x.orderNode),after=order_.Reaches(x.orderNode,y.orderNode);
        if(before.before && after.before)return Relation::Incomparable;
        if(before.before)return Relation::Before;if(after.before)return Relation::After;
        return before.pending || after.pending?Relation::Pending:Relation::Incomparable;
    }
    static bool Intersects(uint64_t a,uint64_t na,uint64_t b,uint64_t nb){return a<b+nb && b<a+na;}
    static Status Overlap(const Range& read,const Event& event){
        if(event.kind==Kind::Unknown)return Status::MissingProvenance;
        if(event.kind==Kind::Alias){
            if(!ValidMemory(event.memory))return Status::Aliased;
            if(read.committed || read.allocation.heap!=event.memory.heap)return Status::Unchanged;
            return Intersects(read.allocation.offset+read.offset,read.bytes,event.memory.offset,event.memory.bytes)?Status::Aliased:Status::Unchanged;
        }
        const auto& write=event.range;
        if(read.resource==write.resource)return Intersects(read.offset,read.bytes,write.offset,write.bytes)?Status::IntersectingWrite:Status::Unchanged;
        if(read.committed || write.committed)return Status::Unchanged;
        if(!ValidMemory(write.allocation))return Status::MissingProvenance;
        if(read.allocation.heap!=write.allocation.heap)return Status::Unchanged;
        return Intersects(read.allocation.offset+read.offset,read.bytes,write.allocation.offset+write.offset,write.bytes)?Status::Aliased:Status::Unchanged;
    }
    // A known MAIN predecessor does not close another pending wait branch.
    // Walk every ancestor, rejecting future signals and dependency cycles.
    Status Closed(uint32_t node) const {
        if(const auto found=closed_.find(node);found!=closed_.end())return found->second;
        const auto result=WalkClosed(node);if(closed_.size()<Capacity)closed_.emplace(node,result);return result;
    }
    Status WalkClosed(uint32_t node) const {
        std::vector<uint8_t> visited(nodes_.size());std::vector<std::pair<uint32_t,bool>> stack{{node,false}};
        while(!stack.empty()){
            const auto [id,leaving]=stack.back();stack.pop_back();if(!id)continue;
            if(leaving){visited[id]=2;continue;}
            if(visited[id]==2)continue;if(visited[id]==1)return Status::Incomparable;
            visited[id]=1;stack.push_back({id,true});const auto& n=nodes_[id];
            if(n.previous)stack.push_back({n.previous,false});
            if(!n.fence)continue;
            const auto found=fences_.find(n.fence);if(found==fences_.end() || found->second.invalid)return Status::MissingProvenance;
            const auto& fence=found->second;
            if(n.value<=fence.baseline || n.value<=fence.completed)continue;
            const auto signal=fence.signals.find(n.value);if(signal==fence.signals.end())return Status::Pending;
            stack.push_back({signal->second,false});
        }
        return Status::Unchanged;
    }
};
#endif
}
