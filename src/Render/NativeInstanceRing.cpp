#include "Render/NativeInstanceRing.hpp"
#include <algorithm>
#include <atomic>
#include <optional>
#include <vector>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// THE LEDGER IS RUST'S (rust/crates/cvr-render/src/instance_ring.rs): this face hands it the C++ half's records and holds
// the C++ ends -- the completed writers' presence (a shared_ptr the ledger owns through the callbacks below, so the
// weak_ptr expires when the ledger drops the writer) and the CPU candidates' payload references.
namespace cvr::stereo::instance_ring {
struct CvrInstanceRingQuery {const void* payload;CompletedWriter producer;size_t offset,bytes;Publication publication;};
}
extern "C" {
using cvr::stereo::instance_ring::CompletedWriter;
using cvr::stereo::instance_ring::CvrInstanceRingQuery;
using cvr::stereo::instance_ring::Limits;
using cvr::stereo::instance_ring::Publication;
using cvr::stereo::instance_ring::Range;
using cvr::stereo::instance_ring::ReadRequest;
using cvr::stereo::instance_ring::RingObservation;
using cvr::stereo::instance_ring::Scope;
using cvr::stereo::instance_ring::Status;
using cvr::stereo::instance_ring::WriterTicket;
void cvr_instance_ring_attach(void* (*make)(const WriterTicket*),void (*drop)(void*));
void* cvr_instance_ring_new(const Limits*);
void cvr_instance_ring_free(void*);
bool cvr_instance_ring_range(const RingObservation*,uint64_t,uint32_t,uint32_t,Range*);
Status cvr_instance_ring_observe(void*,const RingObservation*);
Status cvr_instance_ring_begin(void*,const RingObservation*,const Scope*,WriterTicket*);
Status cvr_instance_ring_qualify_write(void*,const RingObservation*,const WriterTicket*,uint64_t,uint32_t,uint32_t);
Status cvr_instance_ring_validate_write(void*,const WriterTicket*);
Status cvr_instance_ring_discard_completed(void*,const WriterTicket*);
Status cvr_instance_ring_invalidate_physical(void*,const RingObservation*,uint32_t,uint32_t);
Status cvr_instance_ring_finish_unobserved(void*,const RingObservation*,const WriterTicket*,uint32_t,uint32_t);
Status cvr_instance_ring_complete(void*,const RingObservation*,const CompletedWriter*,const uint8_t*,size_t);
Status cvr_instance_ring_publish(void*,const RingObservation*,const Publication*);
Status cvr_instance_ring_restart_observation(void*,const RingObservation*);
Status cvr_instance_ring_reuse(void*,const RingObservation*,uint64_t,uint32_t,const WriterTicket*);
void cvr_instance_ring_unknown(void*,uint64_t);
void cvr_instance_ring_destroy(void*,uint64_t);
void cvr_instance_ring_reset(void*);
Status cvr_instance_ring_query(void*,const RingObservation*,const ReadRequest*,CvrInstanceRingQuery*);
void* cvr_instance_ring_presence(void*,const WriterTicket*);
size_t cvr_instance_ring_live_cpu_bytes(void*);
void cvr_instance_ring_payload_retain(const void*);
void cvr_instance_ring_payload_release(const void*);
const uint8_t* cvr_instance_ring_payload_bytes(const void*,size_t*);
}
namespace cvr::stereo::instance_ring {
static_assert(sizeof(RingObservation)==120 && sizeof(Scope)==56 && sizeof(WriterTicket)==88 && sizeof(CompletedWriter)==104 &&
              sizeof(Publication)==64 && sizeof(ReadRequest)==160 && sizeof(Limits)==48 && sizeof(CvrInstanceRingQuery)==192,
              "mirrored in rust/crates/cvr-render/src/instance_ring.rs");
namespace {
using Presence=std::shared_ptr<const WriterTicket>;
void* MakePresence(const WriterTicket* ticket){return new Presence(std::make_shared<const WriterTicket>(*ticket));}
void DropPresence(void* presence){delete static_cast<Presence*>(presence);}
const bool attached=(cvr_instance_ring_attach(&MakePresence,&DropPresence),true);
}
bool InstanceRange(const RingObservation& a,uint64_t absolute,uint32_t first,uint32_t instances,Range& output)noexcept{
    return cvr_instance_ring_range(&a,absolute,first,instances,&output);}
CpuCandidate::CpuCandidate(const CpuCandidate& other)noexcept:data_(other.data_),producer_(other.producer_),source_(other.source_),
    publication_(other.publication_),offset_(other.offset_),bytes_(other.bytes_){cvr_instance_ring_payload_retain(data_);}
CpuCandidate& CpuCandidate::operator=(const CpuCandidate& other)noexcept{
    if(this!=&other){cvr_instance_ring_payload_retain(other.data_);cvr_instance_ring_payload_release(data_);data_=other.data_;
        producer_=other.producer_;source_=other.source_;publication_=other.publication_;offset_=other.offset_;bytes_=other.bytes_;}
    return *this;}
CpuCandidate::CpuCandidate(CpuCandidate&& other)noexcept:data_(std::exchange(other.data_,nullptr)),producer_(other.producer_),
    source_(other.source_),publication_(other.publication_),offset_(other.offset_),bytes_(other.bytes_){}
CpuCandidate& CpuCandidate::operator=(CpuCandidate&& other)noexcept{
    if(this!=&other){cvr_instance_ring_payload_release(data_);data_=std::exchange(other.data_,nullptr);producer_=other.producer_;
        source_=other.source_;publication_=other.publication_;offset_=other.offset_;bytes_=other.bytes_;}
    return *this;}
CpuCandidate::~CpuCandidate(){cvr_instance_ring_payload_release(data_);}
CpuCandidate::operator bool()const noexcept{return data_ && bytes_;}
std::span<const uint8_t> CpuCandidate::Bytes()const noexcept{
    size_t size{};const auto* bytes=cvr_instance_ring_payload_bytes(data_,&size);
    return bytes?std::span<const uint8_t>(bytes,size).subspan(offset_,bytes_):std::span<const uint8_t>{};}
const CompletedWriter& CpuCandidate::Producer()const noexcept{static const CompletedWriter empty;return data_?producer_:empty;}
struct Ledger::Data {void* rust;~Data(){cvr_instance_ring_free(rust);}};
Ledger::Ledger(Limits limits):data_(new Data{cvr_instance_ring_new(&limits)}){}
Ledger::~Ledger()=default;
Status Ledger::Observe(const RingObservation& a)noexcept{return cvr_instance_ring_observe(data_->rust,&a);}
Status Ledger::Begin(const RingObservation& a,const Scope& scope,WriterTicket& output)noexcept{
    return cvr_instance_ring_begin(data_->rust,&a,&scope,&output);}
Status Ledger::QualifyWrite(const RingObservation& a,const WriterTicket& ticket,uint64_t absolute,uint32_t first,uint32_t instances)noexcept{
    return cvr_instance_ring_qualify_write(data_->rust,&a,&ticket,absolute,first,instances);}
Status Ledger::ValidateWrite(const WriterTicket& ticket)const noexcept{return cvr_instance_ring_validate_write(data_->rust,&ticket);}
Status Ledger::DiscardCompleted(const WriterTicket& ticket)noexcept{return cvr_instance_ring_discard_completed(data_->rust,&ticket);}
Status Ledger::InvalidatePhysical(const RingObservation& a,uint32_t first,uint32_t instances)noexcept{
    return cvr_instance_ring_invalidate_physical(data_->rust,&a,first,instances);}
Status Ledger::FinishUnobserved(const RingObservation& a,const WriterTicket& ticket,uint32_t first,uint32_t instances)noexcept{
    return cvr_instance_ring_finish_unobserved(data_->rust,&a,&ticket,first,instances);}
Status Ledger::Complete(const RingObservation& a,const CompletedWriter& completed,std::span<const uint8_t> bytes)noexcept{
    return cvr_instance_ring_complete(data_->rust,&a,&completed,bytes.data(),bytes.size());}
Status Ledger::Publish(const RingObservation& a,const Publication& publication)noexcept{
    return cvr_instance_ring_publish(data_->rust,&a,&publication);}
Status Ledger::RestartObservation(const RingObservation& a)noexcept{return cvr_instance_ring_restart_observation(data_->rust,&a);}
Status Ledger::Reuse(const RingObservation& a,uint64_t absolute,uint32_t instances,const WriterTicket* active)noexcept{
    return cvr_instance_ring_reuse(data_->rust,&a,absolute,instances,active);}
void Ledger::Unknown(uint64_t ring)noexcept{cvr_instance_ring_unknown(data_->rust,ring);}
void Ledger::Destroy(uint64_t ring)noexcept{cvr_instance_ring_destroy(data_->rust,ring);}
void Ledger::Reset()noexcept{cvr_instance_ring_reset(data_->rust);}
Status Ledger::Query(const RingObservation& a,const ReadRequest& request,CpuCandidate& output)noexcept{
    output={};CvrInstanceRingQuery out{};const auto status=cvr_instance_ring_query(data_->rust,&a,&request,&out);
    if(out.payload){output.data_=out.payload;output.producer_=out.producer;output.source_=request;output.offset_=out.offset;
        output.bytes_=out.bytes;output.publication_=out.publication;}
    return status;}
size_t Ledger::LiveCpuBytes()const noexcept{return cvr_instance_ring_live_cpu_bytes(data_->rust);}
std::weak_ptr<const WriterTicket> Ledger::CompletedPresence(const WriterTicket& ticket)const noexcept{
    const auto* presence=static_cast<const Presence*>(cvr_instance_ring_presence(data_->rust,&ticket));
    return presence?std::weak_ptr<const WriterTicket>(*presence):std::weak_ptr<const WriterTicket>{};}
}
#endif
#else

namespace cvr::stereo::instance_ring {
namespace detail {
struct Budget {std::atomic<size_t> bytes{};};
struct Payload {
    CompletedWriter producer;
    std::vector<uint8_t> bytes;
    std::shared_ptr<Budget> budget;
    ~Payload(){if(budget)budget->bytes.fetch_sub(bytes.size(),std::memory_order_relaxed);}
};
}
namespace {
bool Valid(const RingObservation& a)noexcept {
    return a.ring && !(a.ring&7) && a.ring<=UINT64_MAX-0x60 && a.generation && a.resource && a.gpu && a.mapped && a.registry &&
        a.allocation && a.allocator && a.heap && a.handle && a.handle<=0x100000 && a.kind==5 &&
        ((a.capacity==0x20000 && a.stride==48) || (a.capacity==0x8000 && a.stride==64)) &&
        a.width==a.capacity*a.stride && a.gpu<=UINT64_MAX-a.width && a.mapped<=UINT64_MAX-a.width &&
        a.tail<=a.head && a.head-a.tail<=a.capacity;
}
bool Identity(const RingObservation& a,const RingObservation& b)noexcept {
    auto left=a,right=b;left.head=left.tail=right.head=right.tail=0;return left==right;
}
bool Valid(const Scope& a)noexcept {return a.list && a.generation && a.occurrence && a.thread && a.context && a.registry && a.stamp;}
bool Success(Status status)noexcept {return status==Status::Observed || status==Status::Recreated;}
bool Intersects(uint64_t begin,uint64_t bytes,uint64_t other,uint64_t size)noexcept {
    return begin<other+size && other<begin+bytes;
}
}
bool InstanceRange(const RingObservation& a,uint64_t absolute,uint32_t first,uint32_t instances,Range& output)noexcept {
    output={};if(!Valid(a) || !instances || first>=a.capacity || absolute<a.tail || absolute>a.head ||
        instances>a.head-absolute || absolute%a.capacity!=first || instances>a.capacity-first)return false;
    output={absolute+instances,uint64_t(first)*a.stride,uint64_t(instances)*a.stride};return true;
}
struct Ledger::Data {
    struct Ring {RingObservation observed;bool current=true;std::vector<Publication> publications;};
    struct Writer {WriterTicket ticket;std::shared_ptr<detail::Payload> data;Publication publication;std::optional<CompletedWriter> pending;bool conflicted{};
        std::shared_ptr<const WriterTicket> presence;};
    Limits limits;uint64_t epoch=1,next{};bool exhausted{};
    std::vector<Ring> rings;std::vector<Writer> writers;
    std::shared_ptr<detail::Budget> budget=std::make_shared<detail::Budget>();
    explicit Data(Limits value):limits(value){}
    Ring* Find(uint64_t address)noexcept {const auto at=std::find_if(rings.begin(),rings.end(),[&](const auto& a){return a.observed.ring==address;});return at==rings.end()?nullptr:&*at;}
    Writer* Find(const WriterTicket& ticket)noexcept {const auto at=std::find_if(writers.begin(),writers.end(),[&](const auto& a){return a.ticket==ticket;});return at==writers.end()?nullptr:&*at;}
    void Erase(uint64_t ring)noexcept {std::erase_if(writers,[&](const auto& a){return a.ticket.ring==ring;});}
    void Reject(uint64_t ring)noexcept {if(auto* a=Find(ring)){a->current=false;a->publications.clear();}Erase(ring);}
    void RejectAll()noexcept {exhausted=true;for(auto& a:rings){a.current=false;a.publications.clear();}writers.clear();}
    bool Pending(uint64_t ring)const noexcept {return std::any_of(writers.begin(),writers.end(),[&](const auto& a){return a.ticket.ring==ring && !a.data;});}
    bool PendingRange(uint64_t ring,const Range& range,uint32_t stride,uint64_t skip=0)const noexcept {
        return std::any_of(writers.begin(),writers.end(),[&](const auto& value){if(value.ticket.ring!=ring || value.ticket.id==skip || value.data)return false;
            return !value.pending || Intersects(range.byteOffset,range.bytes,uint64_t(value.pending->firstInstance)*stride,uint64_t(value.pending->instances)*stride);});
    }
    bool KnownPendingOverlap(uint64_t ring,const Range& range,uint32_t stride,uint64_t skip)const noexcept {
        return std::any_of(writers.begin(),writers.end(),[&](const auto& value){return value.ticket.ring==ring && value.ticket.id!=skip &&
            !value.data && value.pending && Intersects(range.byteOffset,range.bytes,uint64_t(value.pending->firstInstance)*stride,uint64_t(value.pending->instances)*stride);});
    }
    bool Overwrite(const RingObservation& a,const Range& range,uint64_t skip=0)noexcept {
        bool overlap{};
        for(auto& writer:writers)if(writer.ticket.ring==a.ring && writer.ticket.id!=skip && !writer.data && writer.pending &&
            Intersects(range.byteOffset,range.bytes,uint64_t(writer.pending->firstInstance)*a.stride,uint64_t(writer.pending->instances)*a.stride))writer.conflicted=true;
        std::erase_if(writers,[&](const auto& writer){
            if(writer.ticket.ring!=a.ring || writer.ticket.id==skip || !writer.data)return false;
            const auto& producer=writer.data->producer;const auto offset=uint64_t(producer.firstInstance)*a.stride;
            const auto bytes=uint64_t(producer.instances)*a.stride;
            const bool hit=Intersects(range.byteOffset,range.bytes,offset,bytes);overlap|=hit;return hit;
        });return overlap;
    }
};
CpuCandidate::operator bool()const noexcept{return data_ && bytes_;}
std::span<const uint8_t> CpuCandidate::Bytes()const noexcept{return data_?std::span<const uint8_t>(data_->bytes).subspan(offset_,bytes_):std::span<const uint8_t>{};}
const CompletedWriter& CpuCandidate::Producer()const noexcept {static const CompletedWriter empty;return data_?data_->producer:empty;}
Ledger::Ledger(Limits limits):data_(std::make_unique<Data>(limits)){}
Ledger::~Ledger()=default;
Status Ledger::Observe(const RingObservation& a)noexcept try {
    auto& d=*data_;if(d.exhausted)return Status::Capacity;
    if(!Valid(a)){d.Reject(a.ring);return Status::InvalidObservation;}
    auto* ring=d.Find(a.ring);
    if(!ring){if(d.rings.size()>=d.limits.rings){d.RejectAll();return Status::Capacity;}d.rings.push_back({a});return Status::Observed;}
    if(a.generation>ring->observed.generation){d.Erase(a.ring);ring->observed=a;ring->current=true;ring->publications.clear();return Status::Recreated;}
    if(!ring->current || !Identity(a,ring->observed) || a.head<ring->observed.head || a.tail<ring->observed.tail){d.Reject(a.ring);return Status::Changed;}
    ring->observed=a;
    std::erase_if(d.writers,[&](const auto& writer){return writer.ticket.ring==a.ring && writer.data && writer.data->producer.absoluteBegin<a.tail;});
    std::erase_if(ring->publications,[&](const auto& publication){return publication.head<=a.tail;});return Status::Observed;
}catch(...){data_->RejectAll();return Status::Exception;}
Status Ledger::Begin(const RingObservation& a,const Scope& scope,WriterTicket& output)noexcept try {
    output={};const auto observed=Observe(a);if(!Success(observed))return observed;auto& d=*data_;
    if(!Valid(scope) || scope.registry!=a.registry){d.Reject(a.ring);return Status::InvalidScope;}
    if(std::any_of(d.writers.begin(),d.writers.end(),[&](const auto& writer){return !writer.data && writer.ticket.scope==scope;})){
        d.Reject(a.ring);return Status::InvalidScope;}
    const auto count=std::count_if(d.writers.begin(),d.writers.end(),[](const auto& writer){return !writer.data;});
    if(size_t(count)>=d.limits.pendingWriters || d.next==UINT64_MAX){d.RejectAll();return Status::Capacity;}
    output={++d.next,d.epoch,a.ring,a.generation,scope};d.writers.push_back({output});return Status::Observed;
}catch(...){output={};data_->RejectAll();return Status::Exception;}
Status Ledger::Complete(const RingObservation& a,const CompletedWriter& completed,std::span<const uint8_t> bytes)noexcept try {
    const auto observed=Observe(a);if(!Success(observed))return observed;auto& d=*data_;auto* writer=d.Find(completed.writer);
    if(!writer || completed.writer.ring!=a.ring || completed.writer.ringGeneration!=a.generation){d.Reject(a.ring);return Status::UnknownWriter;}
    if(writer->data){d.Reject(a.ring);return Status::Changed;}
    Range range;if(!InstanceRange(a,completed.absoluteBegin,completed.firstInstance,completed.instances,range) || bytes.size()!=range.bytes){
        d.Reject(a.ring);return Status::RangeMismatch;}
    if(writer->pending && (writer->pending->absoluteBegin!=completed.absoluteBegin || writer->pending->firstInstance!=completed.firstInstance ||
        writer->pending->instances!=completed.instances))return Status::RangeMismatch;
    if(writer->conflicted || d.PendingRange(a.ring,range,a.stride,completed.writer.id))return Status::Pending;
    // This writer has returned. Its actual overwritten bytes stay invalid even
    // when the bounded CPU cache cannot retain the new sample.
    d.Overwrite(a,range,completed.writer.id);
    const auto drop=[&]{std::erase_if(d.writers,[&](const auto& value){return value.ticket==completed.writer;});return Status::Capacity;};
    if(!d.limits.ranges || bytes.size()>d.limits.writerBytes || bytes.size()>d.limits.liveCpuBytes)return drop();
    auto count=std::count_if(d.writers.begin(),d.writers.end(),[](const auto& value){return bool(value.data);});
    while(size_t(count)>=d.limits.ranges || d.budget->bytes.load(std::memory_order_relaxed)>d.limits.liveCpuBytes-bytes.size()){
        const auto oldest=std::find_if(d.writers.begin(),d.writers.end(),[](const auto& value){return bool(value.data);});
        if(oldest==d.writers.end())return drop();
        d.writers.erase(oldest);--count;
    }
    auto payload=std::make_shared<detail::Payload>();payload->producer=completed;payload->bytes.assign(bytes.begin(),bytes.end());
    auto presence=std::make_shared<const WriterTicket>(completed.writer);
    // Attach the budget only after the immutable byte allocation succeeds.
    d.budget->bytes.fetch_add(bytes.size(),std::memory_order_relaxed);payload->budget=d.budget;
    writer=d.Find(completed.writer);writer->data=std::move(payload);writer->presence=std::move(presence);return Status::Completed;
}catch(...){data_->RejectAll();return Status::Exception;}
Status Ledger::QualifyWrite(const RingObservation& a,const WriterTicket& ticket,uint64_t absolute,uint32_t first,uint32_t instances)noexcept {
    const auto observed=Observe(a);if(!Success(observed))return observed;auto& d=*data_;auto* writer=d.Find(ticket);
    if(!writer || writer->data)return Status::UnknownWriter;Range range;
    if(!InstanceRange(a,absolute,first,instances,range))return Status::RangeMismatch;
    const CompletedWriter pending{ticket,absolute,first,instances};
    if(writer->pending && (writer->pending->absoluteBegin!=absolute || writer->pending->firstInstance!=first || writer->pending->instances!=instances))return Status::Changed;
    const bool overlap=d.KnownPendingOverlap(a.ring,range,a.stride,ticket.id);d.Overwrite(a,range,ticket.id);writer=d.Find(ticket);
    writer->pending=pending;writer->conflicted|=overlap;return writer->conflicted?Status::Overlap:Status::Observed;
}
Status Ledger::ValidateWrite(const WriterTicket& ticket)const noexcept {
    const auto& d=*data_;const auto at=std::find_if(d.writers.begin(),d.writers.end(),[&](const auto& value){return value.ticket==ticket;});
    if(at==d.writers.end() || at->data || !at->pending)return Status::UnknownWriter;
    if(at->conflicted)return Status::Overlap;
    const auto* ring=const_cast<Data&>(d).Find(ticket.ring);if(!ring || !ring->current)return Status::Changed;
    Range range;if(!InstanceRange(ring->observed,at->pending->absoluteBegin,at->pending->firstInstance,at->pending->instances,range))return Status::RangeMismatch;
    return d.PendingRange(ticket.ring,range,ring->observed.stride,ticket.id)?Status::Pending:Status::Observed;
}
Status Ledger::Publish(const RingObservation& a,const Publication& publication)noexcept try {
    const auto observed=Observe(a);if(!Success(observed))return observed;auto& d=*data_;auto* ring=d.Find(a.ring);
    if(!Valid(publication.scope) || publication.scope.registry!=a.registry || publication.head!=a.head){d.Reject(a.ring);return Status::InvalidScope;}
    if(d.Pending(a.ring))return Status::Pending;
    if(std::any_of(ring->publications.begin(),ring->publications.end(),[&](const auto& value){return value.scope.stamp==publication.scope.stamp;})){
        d.Reject(a.ring);return Status::Changed;}
    size_t count{};for(const auto& value:d.rings)count+=value.publications.size();
    if(count>=d.limits.publications){d.RejectAll();return Status::Capacity;}
    ring->publications.push_back(publication);
    for(auto& writer:d.writers)if(writer.ticket.ring==a.ring && writer.data &&
        writer.data->producer.absoluteBegin+writer.data->producer.instances<=publication.head)writer.publication=publication;
    return Status::Published;
}catch(...){data_->RejectAll();return Status::Exception;}
Status Ledger::Reuse(const RingObservation& a,uint64_t absolute,uint32_t instances,const WriterTicket* active)noexcept {
    const auto observed=Observe(a);if(!Success(observed))return observed;auto& d=*data_;
    if(active){const auto* writer=d.Find(*active);if(!writer || writer->data || active->ring!=a.ring){d.Reject(a.ring);return Status::UnknownWriter;}}
    Range range;if(!InstanceRange(a,absolute,uint32_t(absolute%a.capacity),instances,range)){
        d.Reject(a.ring);return Status::RangeMismatch;}
    return d.Overwrite(a,range)?Status::Overlap:Status::Observed;
}
Status Ledger::RestartObservation(const RingObservation& a)noexcept {
    auto& d=*data_;if(d.exhausted)return Status::Capacity;auto* ring=d.Find(a.ring);
    if(!ring)return Status::UnknownRing;
    if(!Valid(a) || !Identity(a,ring->observed) || a.head<ring->observed.head || a.tail<ring->observed.tail){d.Reject(a.ring);return Status::Changed;}
    d.Erase(a.ring);ring->observed=a;ring->current=true;ring->publications.clear();return Status::Observed;
}
Status Ledger::DiscardCompleted(const WriterTicket& ticket)noexcept {
    auto& d=*data_;const auto at=std::find_if(d.writers.begin(),d.writers.end(),[&](const auto& value){return value.ticket==ticket;});
    if(at==d.writers.end())return Status::UnknownWriter;
    if(at->data || !at->pending)return Status::Pending;
    d.writers.erase(at);return Status::Observed;
}
Status Ledger::InvalidatePhysical(const RingObservation& a,uint32_t first,uint32_t instances)noexcept {
    const auto observed=Observe(a);if(!Success(observed))return observed;
    if(!instances || first>=a.capacity || instances>a.capacity-first)return Status::RangeMismatch;
    const Range range{0,uint64_t(first)*a.stride,uint64_t(instances)*a.stride};
    return data_->Overwrite(a,range)?Status::Overlap:Status::Observed;
}
Status Ledger::FinishUnobserved(const RingObservation& a,const WriterTicket& ticket,uint32_t first,uint32_t instances)noexcept {
    const auto* writer=data_->Find(ticket);
    if(!writer || writer->data || writer->pending || ticket.ring!=a.ring)return Status::UnknownWriter;
    const auto status=InvalidatePhysical(a,first,instances);
    if(status!=Status::Observed && status!=Status::Overlap)return status;
    std::erase_if(data_->writers,[&](const auto& value){return value.ticket==ticket;});return status;
}
void Ledger::Unknown(uint64_t ring)noexcept {if(ring)data_->Reject(ring);else data_->RejectAll();}
void Ledger::Destroy(uint64_t ring)noexcept {data_->Reject(ring);}
void Ledger::Reset()noexcept {auto& d=*data_;d.writers.clear();d.rings.clear();if(d.epoch==UINT64_MAX)d.exhausted=true;else{++d.epoch;d.exhausted=false;}}
Status Ledger::Query(const RingObservation& a,const ReadRequest& request,CpuCandidate& output)noexcept {
    output={};const auto observed=Observe(a);if(!Success(observed))return observed;auto& d=*data_;const auto* writer=d.Find(request.writer);
    if(!writer || request.writer.ring!=a.ring || request.writer.ringGeneration!=a.generation)return Status::UnknownWriter;
    if(!writer->data)return Status::Pending;
    if(!Valid(request.source) || request.source.registry!=a.registry)return Status::InvalidScope;
    Range range;if(!InstanceRange(a,request.absoluteBegin,request.firstInstance,request.instances,range))return Status::RangeMismatch;
    if(d.PendingRange(a.ring,range,a.stride))return Status::Pending;
    const auto& producer=writer->data->producer;
    if(request.absoluteBegin<producer.absoluteBegin || request.absoluteBegin-producer.absoluteBegin>producer.instances ||
        request.instances>producer.instances-(request.absoluteBegin-producer.absoluteBegin))return Status::RangeMismatch;
    output.data_=writer->data;output.source_=request;output.offset_=size_t(request.absoluteBegin-producer.absoluteBegin)*a.stride;output.bytes_=size_t(range.bytes);
    if(writer->publication.scope.stamp && range.absoluteEnd<=writer->publication.head){
        output.publication_=writer->publication;return Status::Published;}return Status::Completed;
}
size_t Ledger::LiveCpuBytes()const noexcept{return data_->budget->bytes.load(std::memory_order_relaxed);}
std::weak_ptr<const WriterTicket> Ledger::CompletedPresence(const WriterTicket& ticket)const noexcept {
    const auto* writer=data_->Find(ticket);return writer?writer->presence:std::weak_ptr<const WriterTicket>{};
}
}
#endif
