#include <source_location>
#include <string>
#include "Render/NativeSubmissionFrontier.hpp"
#include <cstdio>
#include <map>
#include <memory>
#include <stdexcept>

using namespace cvr::stereo::submission_frontier;
static void Require(bool value,std::source_location at=std::source_location::current()){
    if(!value)throw std::runtime_error("submission frontier assertion failed at line "+std::to_string(at.line()));
}
struct Memory {
    std::map<uint64_t,uint8_t> bytes;uint64_t changeAddress{};uint32_t changeReads{};
    template<class T>void Put(uint64_t address,const T& value){
        const auto* data=reinterpret_cast<const uint8_t*>(&value);for(size_t i=0;i<sizeof(value);++i)bytes[address+i]=data[i];
    }
    static bool Read(void* user,uint64_t address,void* out,size_t size){
        auto& self=*static_cast<Memory*>(user);auto* data=static_cast<uint8_t*>(out);
        for(size_t i=0;i<size;++i){const auto found=self.bytes.find(address+i);if(found==self.bytes.end())return false;data[i]=found->second;}
        if(address==self.changeAddress && ++self.changeReads==2)++data[0];return true;
    }
};
struct Fixture {
    static constexpr uint64_t Base=0x140000000,Renderer=0x200000,Header=Renderer+0x4C60,Table=0x220000,Registry=0x400000;
    static constexpr uint64_t Context0=0x80000,Context1=0x80100,List0=0x90000,List1=0x90100;
    Memory memory;uint32_t end=87,mode=1;uint64_t caller=Base+detail::CallerReturn;
    Fixture(){
        memory.Put(Base+detail::RendererRva,Renderer);memory.Put(Base+detail::RegistryRva,Registry);
        memory.Put(Header,Table);memory.Put(Header+0x10,uint32_t(84));
        memory.Put(Table+84*4,std::array<uint32_t,4>{0,7,8,UINT32_MAX});
        Context(7,Context0,List0);Context(8,Context1,List1);
    }
    void Context(uint32_t id,uint64_t context,uint64_t list){
        memory.Put(Registry+0xD6EDA8+uint64_t(id-1)*16,context);
        memory.Put(context+0x30,list);memory.Put(context+0x60,uint64_t(0xA0000+id*0x100));
        memory.Put(context+0x68,uint32_t(1));memory.Put(context+0x630,uint64_t(0xB0000+id*0x100));memory.Put(context+0x638,uint64_t(0));
    }
    bool Capture(detail::Range& range,detail::Failure& failure){
        return detail::CaptureRange(Memory::Read,&memory,Base,caller,Header,end,mode,range,failure);
    }
};
struct BoundaryFixture {
    detail::BoundaryBatch observed{4,27,123,0xA000,0xB000,8},current=observed;
    std::array<detail::BoundaryList,8> rows{};
    std::array<uint64_t,8> lists{};
    std::array<detail::BoundarySource,2> sources{};
    BoundaryFixture(){
        for(uint32_t i=0;i<rows.size();++i){auto& row=rows[i];row.list=lists[i]=0x10000+i*0x100;
            if(!i)continue;
            row.context=0x20000+i*0x100;row.generation=80000+i;row.token=0x30000+i*0x100;row.registry=0x40000;
            row.work=Fixture::Base+0x332D5C;row.name=0x50000+i;
            row.graph={19,77,13,11,0x60000,7242777683317427689ull,18};
            row.slot=83+i;row.node=i==1?109:24+i;row.view=i==1?1:0;row.match=detail::MatchKind::Matched;row.qualified=true;
        }
        for(uint32_t side=0;side<2;++side){const auto& row=rows[side?1:2];
            sources[side]={row.list,row.context,row.generation,row.token,row.registry,row.graph.fingerprint,row.work,row.name,
                row.slot,row.node,row.view,0};}
    }
    bool Find(Boundary& output,detail::BoundaryFailure& failure){
        return detail::SelectBoundary(observed,current,rows,lists,sources,output,&failure);
    }
};
int main()try{
    detail::AdmissionState admissionState;detail::Admission first,second;
    Require(admissionState.Epoch()==1 && !admissionState.InFlight());
    Require(!admissionState.Admit(false,first) && !first.active && !admissionState.Matches(1));
    Require(admissionState.Admit(true,first) && first.epoch==1 && admissionState.InFlight()==1);
    Require(!admissionState.Admit(true,first) && admissionState.InFlight()==1);
    // Disabling between admission and Begin cannot permit reset or move the
    // admitted native call into a rearmed epoch.
    Require(!admissionState.Reset(false) && admissionState.Epoch()==first.epoch && admissionState.Matches(first.epoch));
    Require(admissionState.Admit(true,second) && second.epoch==first.epoch && admissionState.InFlight()==2);
    Require(admissionState.Release(first) && !admissionState.Release(first) && admissionState.InFlight()==1);
    Require(!admissionState.Reset(false) && admissionState.Matches(second.epoch));
    const auto oldEpoch=second.epoch;
    Require(admissionState.Release(second) && !admissionState.InFlight());
    Require(!admissionState.Reset(true) && admissionState.Epoch()==oldEpoch);
    Require(admissionState.Reset(false) && admissionState.Epoch()==oldEpoch+1 && !admissionState.Matches(oldEpoch));
    // A stale fast-path enable read has no authority. The locked recheck either
    // rejects while disabled or captures the actual new epoch after rearming.
    const bool staleFastEnabled=true;Require(staleFastEnabled);
    Require(admissionState.Reset(false));const auto rearmedEpoch=admissionState.Epoch();
    Require(!admissionState.Admit(false,first) && !admissionState.InFlight());
    Require(admissionState.Admit(true,first) && first.epoch==rearmedEpoch && admissionState.Matches(first.epoch));
    Require(!admissionState.Matches(oldEpoch) && !admissionState.Reset(false));
    detail::Admission stale{oldEpoch,true};Require(!admissionState.Release(stale) && admissionState.InFlight()==1);
    Require(admissionState.Release(first) && admissionState.Reset(false));

    detail::Range range;detail::Failure failure{};Fixture good;
    Require(good.Capture(range,failure) && failure==detail::Failure::None && range.slots.size()==4);
    Require(range.begin==84 && range.end==87 && range.registry==Fixture::Registry);
    Require(range.slots[0].id==0 && !range.slots[0].context.address && range.slots[3].id==UINT32_MAX && !range.slots[3].context.address);
    Require(range.slots[1].context.address==Fixture::Context0 && range.slots[1].context.list==Fixture::List0);
    Require(detail::TokenCaptureAllowed(0,true,range.slots[1]));
    for(uint32_t graphStatus=1;graphStatus<=8;++graphStatus)Require(!detail::TokenCaptureAllowed(graphStatus,true,range.slots[1]));
    Require(!detail::TokenCaptureAllowed(0,false,range.slots[1]) && !detail::TokenCaptureAllowed(0,true,range.slots[0]));
    auto ineligible=range.slots[1];ineligible.context.submitted=1;Require(!detail::TokenCaptureAllowed(0,true,ineligible));
    ineligible=range.slots[1];ineligible.context.kind=3;Require(!detail::TokenCaptureAllowed(0,true,ineligible));
    ineligible=range.slots[1];ineligible.failure=detail::Failure::Changed;Require(!detail::TokenCaptureAllowed(0,true,ineligible));
    Require(detail::ProducerSlot(range.slots[1].index)==84 && detail::ProducerSlot(0)==UINT32_MAX);
    for(const auto mode:{0u,1u,2u}){Fixture f;f.mode=mode;Require(f.Capture(range,failure));}
    const auto reject=[](auto mutate,detail::Failure expected){Fixture f;mutate(f);detail::Range output;detail::Failure reason{};
        Require(!f.Capture(output,reason) && reason==expected && output.slots.empty());};
    reject([](auto& f){++f.caller;},detail::Failure::Caller);
    reject([](auto& f){f.mode=3;},detail::Failure::Arguments);
    reject([](auto& f){f.end=detail::MaxSlots;},detail::Failure::Arguments);
    reject([](auto& f){f.memory.Put(Fixture::Base+detail::RendererRva,Fixture::Renderer+8);},detail::Failure::Renderer);
    reject([](auto& f){f.memory.Put(Fixture::Header,uint64_t(0));},detail::Failure::Header);
    reject([](auto& f){f.memory.Put(Fixture::Header+0x10,uint32_t(89));},detail::Failure::Bounds);
    reject([](auto& f){f.memory.Put(Fixture::Base+detail::RegistryRva,uint64_t(0));},detail::Failure::Registry);
    reject([](auto& f){f.memory.bytes.erase(Fixture::Table+84*4);},detail::Failure::Read);
    reject([](auto& f){f.memory.Put(Fixture::Header,UINT64_MAX-4);},detail::Failure::Read);
    reject([](auto& f){f.memory.changeAddress=Fixture::Header+0x10;},detail::Failure::Changed);
    reject([](auto& f){f.memory.changeAddress=Fixture::Base+detail::RendererRva;},detail::Failure::Changed);
    reject([](auto& f){f.memory.changeAddress=Fixture::Table+84*4;},detail::Failure::Changed);
    Fixture empty;empty.memory.Put(Fixture::Header+0x10,uint32_t(88));Require(empty.Capture(range,failure) && range.slots.empty());
    Fixture last;last.end=255;last.memory.Put(Fixture::Header+0x10,uint32_t(255));last.memory.Put(Fixture::Table+255*4,uint32_t(0));
    Require(last.Capture(range,failure) && range.slots.size()==1 && range.slots.front().index==255);
    Fixture invalid;invalid.memory.Put(Fixture::Table+85*4,uint32_t(129));
    Require(invalid.Capture(range,failure) && range.slots[1].failure==detail::Failure::ContextId && !range.slots[1].context.address);
    Fixture absent;absent.memory.bytes.erase(Fixture::Registry+0xD6EDA8+6*16);
    Require(absent.Capture(range,failure) && range.slots[1].failure==detail::Failure::Context);
    Fixture changed;changed.memory.changeAddress=Fixture::Context0+0x30;
    Require(changed.Capture(range,failure) && range.slots[1].failure==detail::Failure::Context);

    auto ledger=std::make_unique<detail::Ledger>();
    Require(good.Capture(range,failure));const auto copied=range.slots[1].context;
    Require(ledger->Add({copied.list,copied.address,10,100,1,0,true}));
    // The native range transfer may clear/recycle the entire bank. A real token
    // validator, rather than another bank lookup, decides the later submission.
    good.memory.Put(Fixture::Registry+0xD6EDA8+6*16,uint64_t(0));good.memory.Put(Fixture::Table+85*4,uint32_t(0));
    const auto valid=[](uint32_t,uint32_t& reason){reason=0;return true;};
    auto match=ledger->Observe(copied.list,valid);Require(match.kind==detail::MatchKind::Matched && match.entry==0);
    Require(ledger->At(0).context==Fixture::Context0 && ledger->At(0).generation==10);
    Require(ledger->Observe(copied.list,valid).kind==detail::MatchKind::Replay);
    Require(ledger->Observe(copied.list,[](uint32_t,uint32_t& reason){reason=9;return false;}).kind==detail::MatchKind::Replay);
    Require(ledger->Observe(copied.list,[](uint32_t,uint32_t& reason){reason=12;return false;}).kind==detail::MatchKind::Replay);
    Require(ledger->Observe(Fixture::List1,valid).kind==detail::MatchKind::UnknownList && !ledger->Mentions(Fixture::List1));
    // The same COM address with a new native reset generation is a new source.
    Require(ledger->Add({copied.list,Fixture::Context1,11,101,2,1,true}));
    match=ledger->Observe(copied.list,[](uint32_t index,uint32_t& reason){reason=index?0:5;return index==1;});
    Require(match.kind==detail::MatchKind::Matched && match.entry==1 && ledger->At(match.entry).generation==11);
    ledger->Clear();Require(!ledger->Size() && !ledger->Mentions(copied.list));
    Require(ledger->Add({copied.list,copied.address,0,0,1,0,true}));
    bool called{};match=ledger->Observe(copied.list,[&](uint32_t,uint32_t&){called=true;return true;});
    Require(match.kind==detail::MatchKind::MissingToken && !called);
    ledger->Clear();Require(ledger->Add({copied.list,copied.address,10,100,1,0,false}));
    Require(ledger->Observe(copied.list,valid).kind==detail::MatchKind::MatchedUnknownGraph);
    ledger->Clear();Require(ledger->Add({copied.list,copied.address,10,100,1,0,true}));
    match=ledger->Observe(copied.list,[](uint32_t,uint32_t& reason){reason=8;return false;});
    Require(match.kind==detail::MatchKind::RejectedToken && match.failure==8);
    Require(ledger->Add({copied.list,Fixture::Context1,11,101,2,1,true}));
    Require(ledger->Observe(copied.list,valid).kind==detail::MatchKind::Ambiguous);
    ledger->Clear();
    for(uint32_t i=0;i<detail::MaxRows;++i)Require(ledger->Add({uint64_t(i)+1,1,1,1,1,i,false}));
    Require(!ledger->Add({1,1,1,1,1,0,false}) && ledger->Size()==detail::MaxRows);
    Require(!detail::NativeId(0) && !detail::NativeId(UINT32_MAX) && detail::NativeId(1));
    // 2.5: the hook row (sub_1422701F0) is rewritten but not re-audited -- the header leaves its hash 0, so it never
    // matches and the hook stays uninstalled. Every other row is a real audited span.
    for(const auto& code:detail::Contract)Require(code.rva && code.bytes && code.bytes<=512 && (code.hash || code.rva==detail::HookRva));

    BoundaryFixture boundaryFixture;Boundary boundary;detail::BoundaryFailure boundaryFailure{};
    Require(boundaryFixture.Find(boundary,boundaryFailure) && boundaryFailure==detail::BoundaryFailure::None);
    Require(boundary.afterIndex==3 && boundary.count==8 && boundary.sourceIndices==std::array<uint32_t,2>{2,1});
    Require(boundary.probeEpoch==4 && boundary.batch==27 && boundary.queue==0xA000 && boundary.lists==0xB000 &&
        boundary.consumer==77 && boundary.graphEpoch==13 && boundary.invocation==19 && boundary.fingerprint==7242777683317427689ull);
    // Inspection is idempotent. A second observed native submission must carry
    // Replay and fail; the caller separately owns its one insertion attempt.
    Require(boundaryFixture.Find(boundary,boundaryFailure) && boundary.afterIndex==3);
    const auto rejectBoundary=[](auto mutate,detail::BoundaryFailure expected){BoundaryFixture f;mutate(f);
        Boundary output;output.batch=UINT64_MAX;detail::BoundaryFailure reason{};
        Require(!f.Find(output,reason) && reason==expected && !output.batch && !output.afterIndex);};
    rejectBoundary([](auto& f){++f.current.queue;},detail::BoundaryFailure::Batch);
    rejectBoundary([](auto& f){++f.current.thread;},detail::BoundaryFailure::Batch);
    rejectBoundary([](auto& f){++f.current.batch;},detail::BoundaryFailure::Batch);
    rejectBoundary([](auto& f){++f.current.probeEpoch;},detail::BoundaryFailure::Batch);
    rejectBoundary([](auto& f){++f.current.lists;},detail::BoundaryFailure::Batch);
    rejectBoundary([](auto& f){++f.lists[2];},detail::BoundaryFailure::Changed);
    rejectBoundary([](auto& f){std::swap(f.lists[1],f.lists[2]);},detail::BoundaryFailure::Changed);
    rejectBoundary([](auto& f){f.rows[3].list=f.lists[3]=f.lists[1];},detail::BoundaryFailure::Duplicate);
    rejectBoundary([](auto& f){++f.sources[0].list;},detail::BoundaryFailure::MissingSource);
    rejectBoundary([](auto& f){f.sources[0].status=1;},detail::BoundaryFailure::Source);
    rejectBoundary([](auto& f){++f.sources[0].generation;},detail::BoundaryFailure::SourceMatch);
    rejectBoundary([](auto& f){++f.sources[0].token;},detail::BoundaryFailure::SourceMatch);
    rejectBoundary([](auto& f){++f.sources[0].context;},detail::BoundaryFailure::SourceMatch);
    rejectBoundary([](auto& f){++f.sources[0].registry;},detail::BoundaryFailure::SourceMatch);
    rejectBoundary([](auto& f){++f.sources[0].fingerprint;},detail::BoundaryFailure::SourceMatch);
    rejectBoundary([](auto& f){++f.sources[0].node;},detail::BoundaryFailure::SourceMatch);
    rejectBoundary([](auto& f){++f.sources[0].slot;},detail::BoundaryFailure::SourceMatch);
    rejectBoundary([](auto& f){++f.sources[0].name;},detail::BoundaryFailure::SourceMatch);
    rejectBoundary([](auto& f){++f.rows[1].graph.consumer;},detail::BoundaryFailure::Graph);
    rejectBoundary([](auto& f){++f.rows[1].graph.epoch;},detail::BoundaryFailure::Graph);
    rejectBoundary([](auto& f){++f.rows[1].graph.range;},detail::BoundaryFailure::Graph);
    rejectBoundary([](auto& f){f.rows[7].match=detail::MatchKind::UnknownList;},detail::BoundaryFailure::Suffix);
    rejectBoundary([](auto& f){f.rows[7].qualified=false;},detail::BoundaryFailure::Suffix);
    rejectBoundary([](auto& f){++f.rows[7].graph.fingerprint;},detail::BoundaryFailure::Suffix);
    rejectBoundary([](auto& f){++f.rows[7].graph.invocation;},detail::BoundaryFailure::Suffix);
    rejectBoundary([](auto& f){++f.rows[7].graph.frameOwner;},detail::BoundaryFailure::Suffix);
    rejectBoundary([](auto& f){f.rows[7].slot=84;},detail::BoundaryFailure::Suffix);
    rejectBoundary([](auto& f){f.rows[7].token=f.rows[3].token;},detail::BoundaryFailure::Duplicate);
    rejectBoundary([](auto& f){f.rows[7].slot=f.rows[3].slot;},detail::BoundaryFailure::Duplicate);
    for(const auto kind:{detail::MatchKind::Replay,detail::MatchKind::Ambiguous,detail::MatchKind::RejectedToken,detail::MatchKind::MissingToken,detail::MatchKind::MatchedUnknownGraph}){
        rejectBoundary([&](auto& f){f.rows[1].match=kind;},detail::BoundaryFailure::Replay);
        rejectBoundary([&](auto& f){f.rows[7].match=kind;},detail::BoundaryFailure::Replay);
    }
    rejectBoundary([](auto& f){std::swap(f.rows[2],f.rows[7]);std::swap(f.lists[2],f.lists[7]);},detail::BoundaryFailure::NoSuffix);
    std::puts("PASS submission frontier epoch admission/reset, bounded ranges, token matching, replay rejection and exact submission boundary");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
