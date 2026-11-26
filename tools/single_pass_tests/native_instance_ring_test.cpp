#include "Render/NativeInstanceRing.hpp"
#include <algorithm>
#include <array>
#include <cstdio>
#include <stdexcept>
#include <vector>

namespace ring=cvr::stereo::instance_ring;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
static ring::RingObservation Native(uint64_t head=2,uint64_t tail=0,uint64_t generation=1){
    return {0x1000,generation,0x2000,0x10000000,0x20000000,0x3000,0x4000,0x5000,0x6000,
        0x600000,0x20000,head,tail,17,48,5};
}
static ring::Scope Scope(uint64_t occurrence=1){return {0x7000,9,occurrence,3,0x8000,0x3000,0x2401};}
static ring::WriterTicket Write(ring::Ledger& ledger,const ring::RingObservation& native,uint64_t absolute,
    uint32_t instances,uint64_t occurrence=1,uint8_t value=0x3A){
    ring::WriterTicket ticket;
    Require(ledger.Begin(native,Scope(occurrence),ticket)==ring::Status::Observed,"live writer begin");
    std::vector<uint8_t> bytes(size_t(instances)*native.stride,value);
    const ring::CompletedWriter completed{ticket,absolute,uint32_t(absolute%native.capacity),instances};
    Require(ledger.Complete(native,completed,bytes)==ring::Status::Completed,"completed mapped writer bytes");
    std::fill(bytes.begin(),bytes.end(),0xEE);return ticket;
}
static ring::ReadRequest Read(const ring::WriterTicket& writer,uint64_t absolute,uint32_t instances,uint32_t first){
    return {writer,Scope(99),absolute,first,instances};
}
int main()try{
    Require(ring::NativeCode[2].rva==0x337144 && ring::NativeCode[2].bytes==0xDB &&
        ring::NativeCode[2].fnv==0x5AE0D2CD64C9FC88ull,"actual native completed writer guard");
    Require(ring::NormalWriterReturn==0x335991 && ring::SkinWriterReturn==0x333CDF,"actual native normal/skin return sites");
    {
        ring::Ledger ledger;const auto native=Native();const auto writer=Write(ledger,native,0,2);
        ring::CpuCandidate candidate;const auto read=Read(writer,1,1,1);
        Require(ledger.Query(native,read,candidate)==ring::Status::Completed && candidate.Bytes().size()==48 &&
            std::all_of(candidate.Bytes().begin(),candidate.Bytes().end(),[](uint8_t value){return value==0x3A;}),"exact owned subrange after caller bytes changed");
        Require(candidate.Producer().writer==writer && candidate.Source().source.occurrence==99 &&
            !candidate.Published().scope.stamp,"unpublished CPU data does not invent submission");
        const ring::Publication publication{Scope(100),2};
        Require(ledger.Publish(native,publication)==ring::Status::Published && ledger.Query(native,read,candidate)==ring::Status::Published &&
            candidate.Published()==publication,"exact native stamp/head publication");
        auto separate=read;++separate.source.generation;separate.source.list+=0x100;separate.source.thread=7;
        Require(ledger.Query(native,separate,candidate)==ring::Status::Published && candidate.Source().source==separate.source,
            "numeric producer and source scopes stay separate; native joins remain caller proof");
        auto foreign=read;++foreign.source.registry;
        Require(ledger.Query(native,foreign,candidate)==ring::Status::InvalidScope && !candidate,"foreign source registry rejected");
        auto changed=read;changed.firstInstance=0;
        Require(ledger.Query(native,changed,candidate)==ring::Status::RangeMismatch && !candidate,"physical coordinates must equal absolute modulo capacity");
        changed=read;changed.instances=2;
        Require(ledger.Query(native,changed,candidate)==ring::Status::RangeMismatch,"read cannot escape writer range");
    }
    {
        ring::Ledger ledger;const auto native=Native();const auto first=Write(ledger,native,0,2);
        ring::CpuCandidate old;Require(ledger.Query(native,Read(first,0,2,0),old)==ring::Status::Completed,"old CPU version");
        Require(ledger.Publish(native,{Scope(100),2})==ring::Status::Published,"first publication");
        ring::WriterTicket pending;Require(ledger.Begin(native,Scope(2),pending)==ring::Status::Observed,"second live writer");
        ring::CpuCandidate current;Require(ledger.Query(native,Read(first,0,1,0),current)==ring::Status::Pending && !current,"incomplete writer prevents current query");
        const std::array<uint8_t,48> bytes{};
        Require(ledger.Complete(native,{pending,1,1,1},bytes)==ring::Status::Completed,"overwriting writer completes");
        Require(ledger.Query(native,Read(first,0,1,0),current)==ring::Status::UnknownWriter && !current,"intersecting write invalidates entire old version");
        Require(old.Bytes().size()==96 && old.Bytes()[0]==0x3A && ledger.LiveCpuBytes()==144,"exported prior bytes stay immutable and consume live budget");
        Require(ledger.Query(native,Read(pending,1,1,1),current)==ring::Status::Completed && !current.Published().scope.stamp,
            "earlier publication cannot certify a later completed writer");
        old={};Require(ledger.LiveCpuBytes()==48,"released prior CPU payload returns its budget");
    }
    {
        ring::Ledger ledger;auto native=Native();const auto writer=Write(ledger,native,0,2);
        ring::CpuCandidate candidate;Require(ledger.Query(native,Read(writer,0,2,0),candidate)==ring::Status::Completed,"before retirement");
        native.tail=2;Require(ledger.Observe(native)==ring::Status::Observed &&
            ledger.Query(native,Read(writer,0,1,0),candidate)==ring::Status::UnknownWriter && !candidate,"retired tail removes current version");
        native.head=0x20002;native.tail=0x20000;
        const auto next=Write(ledger,native,0x20000,2,2,0x71);
        Require(ledger.Query(native,Read(next,0x20000,2,0),candidate)==ring::Status::Completed && candidate.Bytes()[0]==0x71,"absolute wrap preserves new physical version");
        auto forged=Read(next,0,2,0);
        Require(ledger.Query(native,forged,candidate)==ring::Status::RangeMismatch && !candidate,"matching physical address does not revive earlier absolute range");
    }
    {
        ring::Ledger ledger;auto native=Native();const auto writer=Write(ledger,native,0,2);
        Require(ledger.Reuse(native,1,1)==ring::Status::Overlap,"actual overlapping reuse invalidates prior version");
        ring::CpuCandidate candidate;Require(ledger.Query(native,Read(writer,0,1,0),candidate)==ring::Status::UnknownWriter,"reuse rejected by fresh query");
        const auto next=Write(ledger,native,0,1,2);ledger.Unknown(native.ring);
        Require(ledger.Query(native,Read(next,0,1,0),candidate)==ring::Status::Changed,"unknown native write quarantines provenance");
        ++native.generation;Require(ledger.Observe(native)==ring::Status::Recreated,"new actual lifecycle starts empty");
        Require(ledger.Query(native,Read(next,0,1,0),candidate)==ring::Status::UnknownWriter,"recreation never inherits old CPU bytes");
        const auto fresh=Write(ledger,native,0,1,3);ledger.Destroy(native.ring);
        Require(ledger.Query(native,Read(fresh,0,1,0),candidate)==ring::Status::Changed,"destroyed lifecycle cannot query current bytes");
    }
    {
        ring::Ledger ledger;auto native=Native();ring::WriterTicket ticket;
        Require(ledger.Begin(native,Scope(),ticket)==ring::Status::Observed,"writer for torn completion");
        const std::array<uint8_t,47> truncated{};
        Require(ledger.Complete(native,{ticket,0,0,1},truncated)==ring::Status::RangeMismatch,"truncated mapped sample rejected");
        Require(ledger.Observe(native)==ring::Status::Changed,"bad completion cannot leave old provenance current");
    }
    {
        ring::Ledger ledger;auto native=Native();ring::WriterTicket ticket;
        Require(ledger.Begin(native,Scope(),ticket)==ring::Status::Observed,"writer for early publication");
        Require(ledger.Publish(native,{Scope(2),2})==ring::Status::Pending,"publication cannot close an unfinished producer");
        native.generation=2;Require(ledger.Observe(native)==ring::Status::Recreated,"replacement lifecycle");
        std::array<uint8_t,48> bytes{};
        Require(ledger.Complete(native,{ticket,0,0,1},bytes)==ring::Status::UnknownWriter,"old completion cannot attach to replacement lifecycle");
    }
    {
        ring::Ledger ledger;auto native=Native();const auto ticket=Write(ledger,native,0,1);
        auto changed=native;++changed.resource;
        Require(ledger.Observe(changed)==ring::Status::Changed,"same generation cannot change genuine backing identity");
        ring::CpuCandidate candidate;
        Require(ledger.Query(native,Read(ticket,0,1,0),candidate)==ring::Status::Changed,"numeric identity restoration cannot revive bytes");
        ++native.generation;Require(ledger.Observe(native)==ring::Status::Recreated,"new explicit generation clears quarantine");
        const auto fresh=Write(ledger,native,0,1,2);auto unknown=fresh;++unknown.id;
        const std::array<uint8_t,48> bytes{};
        Require(ledger.Complete(native,{unknown,1,1,1},bytes)==ring::Status::UnknownWriter &&
            ledger.Query(native,Read(fresh,0,1,0),candidate)==ring::Status::Changed,"unknown completed write invalidates current provenance");
    }
    {
        auto native=Native();ring::Range range;
        Require(!ring::InstanceRange(native,UINT64_MAX,0,1,range) && !range.bytes,"absolute overflow rejected");
        native.head=0x20001;native.tail=1;
        Require(!ring::InstanceRange(native,0x1FFFF,0x1FFFF,2,range),"one writer cannot cross the physical end");
        native=Native();native.width--;Require(!ring::InstanceRange(native,0,0,1,range),"size must match actual count times stride");
        native=Native();native.stride=64;native.capacity=0x8000;native.width=0x200000;
        Require(ring::InstanceRange(native,1,1,1,range) && range.byteOffset==64 && range.bytes==64,"actual 2MiB skin ring layout");
    }
    {
        ring::Limits limits;limits.liveCpuBytes=96;ring::Ledger ledger(limits);const auto native=Native();const auto writer=Write(ledger,native,0,2);
        ring::CpuCandidate retained;Require(ledger.Query(native,Read(writer,0,2,0),retained)==ring::Status::Completed,"bounded retained candidate");
        ledger.Reset();Require(ledger.LiveCpuBytes()==96,"reset cannot hide externally retained payload bytes");
        ring::WriterTicket ticket;Require(ledger.Begin(native,Scope(2),ticket)==ring::Status::Observed,"post-reset writer");
        const std::array<uint8_t,48> bytes{};
        Require(ledger.Complete(native,{ticket,0,0,1},bytes)==ring::Status::Capacity,"live exported bytes enforce global budget");
        retained={};Require(!ledger.LiveCpuBytes(),"exported payload eventually releases budget");
    }
    {
        ring::Limits limits;limits.rings=1;limits.pendingWriters=1;ring::Ledger ledger(limits);auto native=Native();ring::WriterTicket first,second;
        Require(ledger.Begin(native,Scope(),first)==ring::Status::Observed,"bounded pending writer");
        Require(ledger.Begin(native,Scope(2),second)==ring::Status::Capacity && !second.id,"pending writer limit invalidates output");
        ledger.Reset();Require(ledger.Observe(native)==ring::Status::Observed,"bounded ring");native.ring+=0x100;
        Require(ledger.Observe(native)==ring::Status::Capacity,"bounded ring inventory");
    }
    {
        ring::Limits limits;limits.publications=1;ring::Ledger ledger(limits);const auto native=Native();Write(ledger,native,0,1);
        Require(ledger.Publish(native,{Scope(2),2})==ring::Status::Published,"first bounded publication");
        auto next=Scope(3);++next.stamp;
        Require(ledger.Publish(native,{next,2})==ring::Status::Capacity,"publication storage has an explicit bound");
    }
    {
        ring::Limits limits;limits.ranges=1;ring::Ledger ledger(limits);const auto native=Native();const auto old=Write(ledger,native,0,1);
        const auto presence=ledger.CompletedPresence(old);ring::CpuCandidate retained,current;
        Require(!presence.expired() && ledger.Query(native,Read(old,0,1,0),retained)==ring::Status::Completed,"held bounded sample");
        ring::WriterTicket ticket;Require(ledger.Begin(native,Scope(2),ticket)==ring::Status::Observed,"second bounded range writer");
        const std::array<uint8_t,48> bytes{};
        Require(ledger.Complete(native,{ticket,1,1,1},bytes)==ring::Status::Completed,"full cache replaces only an older completed sample");
        Require(presence.expired() && !ledger.CompletedPresence(ticket).expired() &&
            ledger.Query(native,Read(old,0,1,0),current)==ring::Status::UnknownWriter && retained.Bytes()[0]==0x3A &&
            ledger.LiveCpuBytes()==96,"eviction revokes membership without mutating externally held bytes");
        Require(ledger.Query(native,Read(ticket,1,1,1),current)==ring::Status::Completed && current.Bytes()[0]==0,
            "cache saturation does not quarantine the ring");
    }
    {
        ring::Limits limits;limits.ranges=8;ring::Ledger ledger(limits);
        std::vector<std::weak_ptr<const ring::WriterTicket>> inventory;
        ring::WriterTicket oldest,last;ring::CpuCandidate candidate;
        for(uint32_t index=0;index<10000;++index){
            auto native=Native(index+1);last=Write(ledger,native,index,1,index+1,uint8_t(index));if(!index)oldest=last;
            std::erase_if(inventory,[](const auto& value){return value.expired();});
            inventory.push_back(ledger.CompletedPresence(last));
            Require(inventory.size()<=limits.ranges && ledger.LiveCpuBytes()<=limits.ranges*48,
                "observer inventory and CPU payload remain bounded beyond4096 completions");
            Require(ledger.Query(native,Read(last,index,1,index),candidate)==ring::Status::Completed && candidate.Bytes()[0]==uint8_t(index),
                "continuous completions remain queryable without a reset or tail advance");
            candidate={};
        }
        Require(ledger.Query(Native(10000),Read(oldest,0,1,0),candidate)==ring::Status::UnknownWriter,"old evicted epoch cannot return");
        auto native=Native(10000,10000);Require(ledger.Observe(native)==ring::Status::Observed,"retire remaining completed cache");
        Require(std::all_of(inventory.begin(),inventory.end(),[](const auto& value){return value.expired();}) && !ledger.LiveCpuBytes(),
            "tail retirement invalidates mirrored memberships");
    }
    {
        ring::Limits limits;limits.liveCpuBytes=96;ring::Ledger ledger(limits);const auto native=Native(4);
        const auto old=Write(ledger,native,0,2);ring::CpuCandidate held,current;
        Require(ledger.Query(native,Read(old,0,2,0),held)==ring::Status::Completed,"retain entire external byte budget");
        const auto presence=ledger.CompletedPresence(old);ring::WriterTicket returned;
        Require(ledger.Begin(native,Scope(2),returned)==ring::Status::Observed,"budget-limited returned writer begins");
        const std::array<uint8_t,48> bytes{};
        Require(ledger.Complete(native,{returned,2,2,1},bytes)==ring::Status::Capacity && presence.expired() &&
            ledger.Query(native,Read(returned,2,1,2),current)==ring::Status::UnknownWriter && held.Bytes()[0]==0x3A,
            "external budget pressure drops only the returned sample and keeps immutable bytes");
        held={};const auto fresh=Write(ledger,native,3,1,3);
        Require(ledger.Query(native,Read(fresh,3,1,3),current)==ring::Status::Completed,"external release resumes without reset");
        const auto freshPresence=ledger.CompletedPresence(fresh);ledger.Unknown(native.ring);
        Require(freshPresence.expired() && current.Bytes().size()==48,"quarantine also expires membership independently of payload ownership");
    }
    {
        ring::Limits limits;limits.ranges=1;ring::Ledger ledger(limits);const auto native=Native(4);
        Write(ledger,native,0,1);ring::WriterTicket pending,returned;
        Require(ledger.Begin(native,Scope(2),pending)==ring::Status::Observed &&
            ledger.QualifyWrite(native,pending,3,3,1)==ring::Status::Observed,"unrelated actual pending write");
        Require(ledger.Begin(native,Scope(3),returned)==ring::Status::Observed &&
            ledger.QualifyWrite(native,returned,2,2,1)==ring::Status::Observed,"independent completed replacement");
        const std::array<uint8_t,48> bytes{};ring::CpuCandidate candidate;
        Require(ledger.Complete(native,{returned,2,2,1},bytes)==ring::Status::Completed &&
            ledger.ValidateWrite(pending)==ring::Status::Observed && ledger.CompletedPresence(pending).expired(),
            "eviction preserves pending writer identity without inventing completed membership");
        Require(ledger.Query(native,Read(pending,0,1,0),candidate)==ring::Status::Pending && !candidate,
            "querying a pending writer's disjoint span never dereferences absent bytes");
        Require(ledger.Complete(native,{pending,3,3,1},bytes)==ring::Status::Completed,"pending completion survives another eviction");
        const auto presence=ledger.CompletedPresence(pending);ledger.Reset();Require(presence.expired(),"reset expires completed membership");
    }
    std::puts("PASS native instance ring numeric lifecycle, completed owned CPU bytes, publication ordering, absolute wrap/reuse, stale epochs and bounded retention; no GPU copy permission");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"native_instance_ring_test: %s\n",error.what());return 1;}
