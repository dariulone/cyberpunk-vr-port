#include "Render/OwnedGraphExecutionState.hpp"
#include <cstdio>
#include <initializer_list>
#include <map>
#include <stdexcept>
#include <type_traits>

using namespace cvr::stereo::owned_graph_execution;
namespace {
constexpr uint64_t Scope=7,Slot=0x10000,ExecutionGroup=0x20000,DeclarationGroup=0x30000;
constexpr AdmissionFacts Audited{true,true,true};
constexpr std::array<OwnerGenerationBinding,2> Bindings{{{0x40000,101,51,901},{0x50000,102,52,902}}};
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
Identity Id(uint64_t consumer=1){return {consumer,11,12,0x60000,0x70000,0x80000,0x90000};}
State& Admit(Ledger& ledger,uint64_t consumer=1){
    auto* state=ledger.Admit(Id(consumer),Bindings,Audited,Scope,Slot);
    Require(state,"Valid audited admission failed");return *state;
}
void Escape(State& state){const auto identity=state.FrozenIdentity();
    Require(state.RecordPublication(identity,Scope,true),"Publication rejected");
    Require(state.MarkConstructInvoked(identity,Scope),"Construct invocation rejected");
    Require(state.RecordConstruct(identity,true,true),"Construct identity rejected");
}
void Queue(State& state){const auto identity=state.FrozenIdentity();
    Require(state.RecordQueued(Path::Execution,identity,ExecutionGroup,true),"Execution enqueue rejected");
    Require(state.RecordQueued(Path::Declaration,identity,DeclarationGroup,true),"Declaration enqueue rejected");
}
void Finish(State& state,bool declarationFirst){const auto identity=state.FrozenIdentity();
    Require(state.RecordProducerClosed(identity,Scope),"Producer close rejected");
    const auto first=declarationFirst?Path::Declaration:Path::Execution;
    const auto second=declarationFirst?Path::Execution:Path::Declaration;
    for(const auto path:{first,second}){
        const auto group=state.TerminalGroup(path);
        Require(state.RecordOriginalReturned(path,identity,group),"Original callback return rejected");
        Require(!state.ObserveCompletion(path,identity,group,false) && !state.Quarantined(),"Pending work quarantined or completed");
        Require(state.ObserveCompletion(path,identity,group,true),"Terminal completion rejected");
        if(path==first)Require(!state.CanReleaseOwnedStorage(),"One reader retired the other reader");
    }
    Require(state.CanReleaseOwnedStorage(),"Complete consumer retained unnecessarily");
}
void SuccessfulLifetimes(){
    Ledger ledger;
    for(uint64_t cycle=1;cycle<=12544;++cycle){auto& state=Admit(ledger,cycle);const auto identity=state.FrozenIdentity();
        Require(state.OwnerBindings().size()==2 && state.OwnerBindings()[0]==Bindings[0],"Audited binding copied incorrectly");
        Require(!state.CanReleaseOwnedStorage() && state.ProducerOpen() && state.PreconstructGraphSlot(Scope)==Slot &&
            !state.PreconstructGraphSlot(Scope+1),"Initial lifetime/scope gate missing");
        Escape(state);Require(!state.PreconstructGraphSlot(Scope) && !state.CanRollback(identity,Scope),"Escaped graph exposed root stack");
        Queue(state);
        if(cycle&1){
            Require(state.RecordOriginalReturned(Path::Execution,identity,ExecutionGroup),"Early callback return rejected");
            Require(!state.ObserveCompletion(Path::Execution,identity,ExecutionGroup,true) && !state.CanReleaseOwnedStorage() &&
                !state.Quarantined(),"Zero before producer close became terminal");
            Require(state.RecordProducerClosed(identity,Scope),"Late producer close rejected");
            Require(state.ObserveCompletion(Path::Execution,identity,ExecutionGroup,true),"Closed producer completion rejected");
            Require(state.RecordOriginalReturned(Path::Declaration,identity,DeclarationGroup) &&
                state.ObserveCompletion(Path::Declaration,identity,DeclarationGroup,true),"Late declaration completion rejected");
        }else Finish(state,true);
        Require(state.CanReleaseOwnedStorage() && ledger.Retire(cycle) && !ledger.Size(),"Successful lifetime leaked state");
        Require(!ledger.Find(cycle) && ledger.LastConsumerId()==cycle,"Consumer ID or retired record lost");
    }
}
void PublicationAndRollback(){
    for(const bool verified:{false,true}){Ledger ledger;auto& state=Admit(ledger);const auto identity=state.FrozenIdentity();
        Require(state.RecordPublication(identity,Scope,verified)==verified,"Publication readback result ignored");
        Require(state.CanRollback(identity,Scope) && !state.CanReleaseOwnedStorage(),"Published pointer did not require rollback");
        ledger.CancelSelection();Require(state.CanRollback(identity,Scope) && !state.CanReleaseOwnedStorage(),"Selection cancellation released publication");
        Require(state.RecordRollback(identity,Scope,true) && state.CanReleaseOwnedStorage() && !state.PreconstructGraphSlot(Scope),"Verified rollback not terminal");
        Require(ledger.Retire(1),"Rolled back record could not retire");
    }
    {Ledger ledger;auto& state=Admit(ledger);const auto identity=state.FrozenIdentity();
        Require(state.RecordPublication(identity,Scope,true),"Rollback fixture publication failed");
        Require(!state.RecordRollback(identity,Scope,false) && state.Quarantined() && !state.CanRollback(identity,Scope) &&
            !state.CanReleaseOwnedStorage() && !ledger.Retire(1),"Uncertain rollback authorized free");
    }
    {Ledger ledger;auto& state=Admit(ledger);const auto identity=state.FrozenIdentity();auto foreign=identity;++foreign.executionGraph;
        Require(!state.RecordPublication(foreign,Scope,true) && state.CanRollback(identity,Scope) &&
            !state.CanReleaseOwnedStorage(),"Foreign publication lost possible stack write");
        Require(state.RecordRollback(identity,Scope,true),"Foreign publication could not roll back original scope");
    }
    {Ledger ledger;auto& state=Admit(ledger);state.ReportFailure(Failure::Admission);
        Require(state.CurrentPhase()==Phase::Cancelled && state.CanReleaseOwnedStorage() && ledger.Retire(1),"Unpublished failure could not retire");
    }
    {Ledger ledger;auto& state=Admit(ledger);const auto identity=state.FrozenIdentity();ledger.CancelSelection();
        Require(!state.RecordPublication(identity,Scope,true) && state.Quarantined() &&
            !state.CanReleaseOwnedStorage(),"Publication after cancellation released uncertain pointer");
    }
    {Ledger ledger;auto& state=Admit(ledger);const auto identity=state.FrozenIdentity();
        Require(state.RecordProducerClosed(identity,Scope) && state.CanReleaseOwnedStorage(),"Unpublished producer exit retained state");
    }
    {Ledger ledger;auto& state=Admit(ledger);const auto identity=state.FrozenIdentity();
        Require(state.RecordPublication(identity,Scope,true),"Producer expiry fixture failed");
        Require(!state.RecordProducerClosed(identity,Scope) && state.Quarantined() && !state.CanRollback(identity,Scope) &&
            !state.CanReleaseOwnedStorage(),"Expired producer slot permitted rollback/free");
    }
}
void EscapeFailures(){
    for(unsigned failure=0;failure<10;++failure){Ledger ledger;auto& state=Admit(ledger);const auto identity=state.FrozenIdentity();
        Require(state.RecordPublication(identity,Scope,true) && state.MarkConstructInvoked(identity,Scope),"Escape failure fixture failed");
        if(failure==0)Require(!state.RecordConstruct(identity,false,true),"Bad payload accepted");
        else if(failure==1)Require(!state.RecordConstruct(identity,true,false),"Unconsumed prerequisite accepted");
        else{
            Require(state.RecordConstruct(identity,true,true),"Construct failure fixture rejected");
            if(failure==2)state.ReportFailure(Failure::MissingProof);
            if(failure==3)Require(!state.RecordQueued(Path::Execution,identity,ExecutionGroup,false),"Unpinned terminal accepted");
            if(failure==4)Require(!state.RecordQueued(Path::Execution,identity,identity.prerequisiteGroup,true),"Prerequisite became terminal");
            if(failure==5)Require(!state.RecordProducerClosed(identity,Scope),"Missing declaration/execution proof accepted");
            if(failure>=6){Queue(state);
                if(failure==6)Require(!state.RecordQueued(Path::Execution,identity,ExecutionGroup,true),"Duplicate native reader accepted");
                if(failure==7){auto foreign=identity;++foreign.pairGeneration;
                    Require(!state.RecordOriginalReturned(Path::Execution,foreign,ExecutionGroup),"Foreign callback accepted");}
                if(failure==8)Require(!state.RecordOriginalReturned(Path::Declaration,identity,ExecutionGroup),"Declaration matched execution group");
                if(failure==9)Require(!state.RecordProducerClosed(identity,Scope+1),"Foreign producer scope accepted");
            }
        }
        Require(state.Escaped() && state.Quarantined() && !state.CanRollback(identity,Scope) && !state.PreconstructGraphSlot(Scope) &&
            !state.CanReleaseOwnedStorage() && !ledger.Retire(1),"Post-escape failure authorized rollback or release");
        const auto sticky=state.Rejection();state.ReportFailure(Failure::Allocation);ledger.CancelSelection();
        Require(state.Rejection()==sticky && state.Quarantined() && !state.CanReleaseOwnedStorage(),"Quarantine recovered without proof");
    }
    {Ledger ledger;auto& state=Admit(ledger);Escape(state);const auto identity=state.FrozenIdentity();
        Require(!state.RecordRollback(identity,Scope,true) && state.Quarantined() && !state.CanReleaseOwnedStorage(),"Stack rollback after originalConstruct accepted");
    }
}
void IncompleteEvidence(){
    {Ledger ledger;auto& state=Admit(ledger);Escape(state);const auto identity=state.FrozenIdentity();
        for(const auto path:{Path::Declaration,Path::Execution})Require(!state.ObserveCompletion(path,identity,ExecutionGroup,true) &&
            !state.Quarantined() && !state.CanReleaseOwnedStorage(),"Zero before enqueue accepted or poisoned state");
        Queue(state);Require(state.RecordProducerClosed(identity,Scope),"Incomplete fixture close failed");
        for(const auto path:{Path::Declaration,Path::Execution})Require(!state.ObserveCompletion(path,identity,state.TerminalGroup(path),true) &&
            !state.Quarantined(),"Terminal zero inferred original callback return");
        state.ReportFailure(Failure::MissingProof);Require(state.Quarantined() && !state.CanReleaseOwnedStorage(),"Missing original return did not quarantine");
    }
    {Ledger ledger;auto& state=Admit(ledger);Escape(state);Queue(state);const auto identity=state.FrozenIdentity();
        Require(!state.ObserveCompletion(Path::Execution,identity,DeclarationGroup,true) && state.Quarantined(),
            "Open producer concealed foreign terminal receipt");
    }
    {Ledger ledger;auto& state=Admit(ledger);Escape(state);Queue(state);const auto identity=state.FrozenIdentity();
        Require(state.RecordProducerClosed(identity,Scope),"Group failure fixture close failed");
        Require(state.RecordOriginalReturned(Path::Execution,identity,ExecutionGroup),"Group failure fixture return failed");
        Require(!state.ObserveCompletion(Path::Execution,identity,DeclarationGroup,true) && state.Quarantined(),"Foreign terminal completion accepted");
    }
    // A shared pinned terminal scope still needs both independent original returns.
    {Ledger ledger;auto& state=Admit(ledger);Escape(state);const auto identity=state.FrozenIdentity();
        Require(state.RecordQueued(Path::Execution,identity,ExecutionGroup,true) &&
            state.RecordQueued(Path::Declaration,identity,ExecutionGroup,true) && state.RecordProducerClosed(identity,Scope),"Shared scope rejected");
        Require(state.RecordOriginalReturned(Path::Execution,identity,ExecutionGroup) &&
            state.ObserveCompletion(Path::Execution,identity,ExecutionGroup,true) && !state.CanReleaseOwnedStorage(),"Shared group inferred declaration return");
        Require(state.RecordOriginalReturned(Path::Declaration,identity,ExecutionGroup) &&
            state.ObserveCompletion(Path::Declaration,identity,ExecutionGroup,true) && state.CanReleaseOwnedStorage(),"Shared scope did not complete");
    }
}
void CancelAndRearm(){
    Ledger ledger;auto& old=Admit(ledger);Escape(old);Queue(old);const auto identity=old.FrozenIdentity();
    ledger.CancelSelection();Require(!old.Selected() && old.Escaped() && !old.CanReleaseOwnedStorage() && !ledger.Retire(1),"Cancel released escaped reader");
    auto& replacement=Admit(ledger,2);Require(ledger.Size()==2 && old.FrozenIdentity()==identity && replacement.Selected(),"Rearm changed old identity");
    Finish(old,false);Require(ledger.Retire(1) && ledger.Size()==1 && ledger.SelectedId()==2,"Old terminal disturbed new selection");
    ledger.CancelSelection();Require(replacement.CanReleaseOwnedStorage() && ledger.Retire(2),"Unpublished replacement could not cancel");
    auto& published=Admit(ledger,3);const auto publishedId=published.FrozenIdentity();
    Require(published.RecordPublication(publishedId,Scope,true),"Rearm publication fixture failed");
    auto& next=Admit(ledger,4);Require(!published.Selected() && published.CanRollback(publishedId,Scope) &&
        !published.CanReleaseOwnedStorage() && next.Selected(),"Rearm freed published graph before rollback");
    Require(published.RecordRollback(publishedId,Scope,true) && ledger.Retire(3),"Old rearmed publication rollback failed");
}
void InvalidAdmissionAndBounds(){
    Ledger ledger;auto identity=Id();auto facts=Audited;
    facts.definitionProtectionAudited=false;
    Require(!ledger.Admit(identity,Bindings,facts,Scope,Slot) && ledger.LastConsumerId()==1 && !ledger.Size(),"Unaudited admission accepted");
    Require(!ledger.Admit(identity,Bindings,Audited,Scope,Slot) && ledger.Rejection()==Failure::Sequence,"Failed admission ID reused");
    for(unsigned invalid=0;invalid<11;++invalid){identity=Id(ledger.LastConsumerId()+1);uint64_t scope=Scope,slot=Slot;auto bindings=Bindings;facts=Audited;
        if(invalid==0)identity.epoch=0;if(invalid==1)identity.pairGeneration=0;if(invalid==2)identity.executionGraph=identity.sourceGraph;
        if(invalid==3)identity.sourceGraph=UINT64_MAX-7;if(invalid==4)identity.frameOwner=identity.prerequisiteGroup;
        if(invalid==5)scope=0;if(invalid==6)++slot;if(invalid==7)facts.exactReferenceCompiled=false;
        if(invalid==8)facts.sourceMappingAudited=false;if(invalid==9)bindings[1].ownerHeader=bindings[0].ownerHeader;
        if(invalid==10)bindings[1].protectionReceipt=0;
        Require(!ledger.Admit(identity,bindings,facts,scope,slot) && !ledger.Size(),"Invalid admission/binding accepted");
    }
    std::array<OwnerGenerationBinding,MaxOwnerBindings+1> excessive{};
    identity=Id(ledger.LastConsumerId()+1);Require(!ledger.Admit(identity,excessive,Audited,Scope,Slot),"Owner binding budget exceeded");
    for(size_t i=0;i<MaxConsumers;++i){auto& state=Admit(ledger,ledger.LastConsumerId()+1);Escape(state);state.ReportFailure(Failure::MissingProof);}
    identity=Id(ledger.LastConsumerId()+1);Require(!ledger.Admit(identity,Bindings,Audited,Scope,Slot) &&
        ledger.Rejection()==Failure::Capacity && ledger.Size()==MaxConsumers,"Quarantine budget exceeded or old state evicted");
    Require(ledger.Find(identity.consumerId-1)->Quarantined(),"Capacity failure discarded retained consumer");
    Ledger copied;auto bindings=Bindings;auto* bound=copied.Admit(Id(),bindings,Audited,Scope,Slot);
    Require(bound,"Binding immutability fixture admission failed");bindings[0]={};
    Require(bound->OwnerBindings()[0]==Bindings[0],"External binding mutation changed frozen provenance");
    Ledger last;identity=Id(UINT64_MAX);Require(last.Admit(identity,Bindings,Audited,Scope,Slot) &&
        !last.Admit(identity,Bindings,Audited,Scope,Slot) && !last.Admit(Id(),Bindings,Audited,Scope,Slot),"Monotonic ID wrapped/reused");
}
namespace expanded_fixture {
namespace scene=cvr::stereo::scene_sections;
namespace tree=cvr::stereo::definition_tree;
namespace stage=cvr::stereo::stage_definition;
namespace graph=cvr::stereo::graph_snapshot;
namespace borrowed=cvr::stereo::borrowed_scene_sections;
namespace expansion=cvr::stereo::scene_expansion;
constexpr uint64_t Base=0x140000000,Root=0xA10000,Handles=0xA20000,Children=0xA30000,Controls=0xA40000;
constexpr uint64_t Early=0xA50000,Late=0xA51000,ArrayAllocator=0xA62000;
std::map<uint64_t,uint8_t> memory;
template<class T>void Put(uint64_t address,const T& value){const auto* bytes=reinterpret_cast<const uint8_t*>(&value);
    for(size_t i=0;i<sizeof(value);++i)memory[address+i]=bytes[i];}
template<class T,size_t N>void Field(std::array<uint8_t,N>& bytes,size_t at,T value){std::memcpy(bytes.data()+at,&value,sizeof(value));}
bool Read(uint64_t address,void* output,size_t count){
    if(!address || address>UINT64_MAX-count)return false;auto* bytes=static_cast<uint8_t*>(output);
    for(size_t i=0;i<count;++i){const auto found=memory.find(address+i);if(found==memory.end())return false;bytes[i]=found->second;}return true;
}
struct Fixture {
    graph::Snapshot source;scene::Snapshot sections;borrowed::Sections views;expansion::Plan plan;borrowed::CallerLifetime lifetime;
};
// CPU metadata fixtures exercise real typed constructors, not native ownership.
Fixture Make(bool bind=true){
    memory.clear();Put(Base+tree::ObjectAllocatorRva,uint64_t(0xA60000));Put(Base+tree::ControlAllocatorRva,uint64_t(0xA61000));
    Put(ArrayAllocator+0x20,Base+0x985ABC);Put(Base+scene::VtableRva,Base+tree::SceneDriverDestructorRva);
    Put(Base+scene::VtableRva+8,Base+scene::WorkRva);
    scene::Header header{};Field(header,0,Base+scene::VtableRva);Field(header,24,scene::Name("GBuffer_Discard"));
    Field(header,32,uint32_t(1));Field(header,40,Handles);Field(header,48,scene::ChildCount);Field(header,52,scene::ChildCount);Put(Root,header);
    Put(Handles+16*scene::ChildCount,ArrayAllocator);
    for(uint32_t slot=0;slot<scene::ChildCount;++slot){const auto& profile=scene::detail::Pattern[slot];
        const uint64_t child=Children+uint64_t(slot)*0x100,control=Controls+uint64_t(slot)*0x100,vtable=Base+profile.vtable;
        Put(vtable,Base+profile.destructor);Put(vtable+8,Base+profile.work);
        if(slot==2 || slot==8){const bool late=slot==8;const uint64_t data=late?Late:Early;const uint32_t count=late?4:2;
            stage::Header render{};Field(render,0,vtable);Field(render,24,Base+(late?0x22749E0:0x2274AE0));
            Field(render,48,data);Field(render,56,count);Field(render,60,count);Put(child,render);
            for(uint32_t i=0;i<count;++i){stage::Stage row{};Field(row,8,i%2?17u:33u);row[12]=uint8_t(i%2?1:0);
                Field(row,16,late?(i<2?9u:8u):7u);row[20]=uint8_t(i%2?0:2);Field(row,36,UINT32_MAX);Put(data+48*i,row);}
            Put(data+48*count,ArrayAllocator);
        }else if(profile.bytes==24){std::array<uint8_t,24> row{};Field(row,0,vtable);Put(child,row);
        }else{std::array<uint8_t,32> row{};Field(row,0,vtable);Field(row,24,profile.parameter);Put(child,row);}
        Put(Handles+16*slot,tree::NativeHandle{child,control});Put(control,int32_t(1));Put(control+4,int32_t(1));
    }
    graph::Node node;node.handle=0x20000000;node.vtable=Base+scene::VtableRva;node.work=Base+scene::WorkRva;
    Field(node.bytes,8,uint32_t(4));Field(node.bytes,16,Root);Field(node.bytes,28,uint32_t(0xFACE0000));
    Require(Read(Root,node.definition.data(),node.definition.size()),"Expanded fixture source header missing");
    graph::Snapshot initial;initial.nodes.push_back(node);cvr::stereo::OwnedNativeGraph compiled;
    Require(compiled.Create(initial,{}) && compiled.CompileReference(),"Expanded fixture source compile failed");
    Fixture fixture;fixture.source=compiled.Snapshot();tree::Snapshot native;const uint64_t roots[]{Root};
    Require(native.Capture(roots,Base,Read,true) && fixture.sections.Capture(fixture.source.nodes[0],native,Read),"Expanded fixture section capture failed");
    fixture.lifetime={Id().consumerId,Id().epoch,Id().pairGeneration,Root,
        Bindings[0].ownerHeader,Bindings[0].generationId,Bindings[0].bornMutationEpoch,Bindings[0].protectionReceipt};
    Require(fixture.views.Prepare(fixture.sections) && fixture.plan.Build(fixture.source,Base,node.handle,fixture.views.Definitions()),
        "Expanded fixture prepare/plan failed");
    if(bind)Require(fixture.views.Bind(fixture.sections,fixture.lifetime),"Expanded fixture binding failed");
    return fixture;
}
constexpr ExpansionAdmissionFacts Facts{true,true,true,true};
State* Admit(Ledger& ledger,const Fixture& fixture){return ledger.AdmitExpanded(Id(),Bindings,fixture.plan,fixture.views,fixture.sections,
    fixture.lifetime,fixture.plan.Expanded(),Base,Facts,Scope,Slot);}
}
void ExpandedAdmission(){
    using namespace expanded_fixture;
    {auto fixture=Make();Ledger ledger;auto* state=expanded_fixture::Admit(ledger,fixture);
        Require(state && ledger.Size()==1 && state->OwnerBindings()[0]==Bindings[0],"Typed expanded admission failed");
        Escape(*state);Queue(*state);Finish(*state,true);Require(ledger.Retire(Id().consumerId),"Expanded complete lifecycle did not retire");}
    for(unsigned change=0;change<17;++change){auto fixture=Make();Ledger ledger;auto identity=Id();auto facts=Facts;
        auto lifetime=fixture.lifetime;auto bindings=Bindings;auto actual=fixture.plan.Expanded();uint64_t base=Base;
        if(change==0)facts.sourceMappingAudited=false;if(change==1)facts.definitionProtectionAudited=false;
        if(change==2)facts.nativeCodeGuarded=false;if(change==3)facts.nativeContextOrderCompiled=false;
        if(change==4)++identity.consumerId;if(change==5)++identity.epoch;if(change==6)++identity.pairGeneration;
        if(change==7)++lifetime.protectionReceipt;if(change==8)++bindings[0].generationId;
        if(change==9)++bindings[0].bornMutationEpoch;if(change==10)bindings[0].ownerHeader+=8;
        if(change==11)++bindings[0].protectionReceipt;if(change==12)bindings[1].protectionReceipt=bindings[0].protectionReceipt;
        if(change==13)actual.nodes.back().bytes[28]^=1;if(change==14)actual.nodes.pop_back();
        if(change==15)actual.edges.back().bytes[8]^=1;if(change==16)++base;
        Require(!ledger.AdmitExpanded(identity,bindings,fixture.plan,fixture.views,fixture.sections,lifetime,actual,base,facts,Scope,Slot) &&
            !ledger.Size() && ledger.LastConsumerId()==identity.consumerId,"Changed expansion proof admitted");
        Require(!ledger.Admit(identity,Bindings,Audited,Scope,Slot) && ledger.Rejection()==Failure::Sequence,"Failed expanded consumer ID reused");
    }
    {auto fixture=Make(false);Ledger ledger;Require(!expanded_fixture::Admit(ledger,fixture),"Prepared but unbound views admitted");}
    {auto fixture=Make();Ledger ledger;borrowed::Sections other;Require(other.Create(fixture.sections,fixture.lifetime),"Other view fixture failed");
        Require(!ledger.AdmitExpanded(Id(),Bindings,fixture.plan,other,fixture.sections,fixture.lifetime,fixture.plan.Expanded(),Base,Facts,Scope,Slot),
            "Equivalent source with foreign view addresses admitted");}
    {auto fixture=Make();Ledger ledger;Require(!ledger.AdmitExpanded(Id(),{},fixture.plan,fixture.views,fixture.sections,fixture.lifetime,
        fixture.plan.Expanded(),Base,Facts,Scope,Slot),"Missing actual reader owner binding admitted");}
    {auto fixture=Make();Ledger ledger;scene::Snapshot empty;Require(!ledger.AdmitExpanded(Id(),Bindings,fixture.plan,fixture.views,empty,
        fixture.lifetime,fixture.plan.Expanded(),Base,Facts,Scope,Slot),"Uncaptured source sections admitted");}
    {auto fixture=Make();Ledger ledger;expansion::Plan empty;Require(!ledger.AdmitExpanded(Id(),Bindings,empty,fixture.views,fixture.sections,
        fixture.lifetime,fixture.plan.Expanded(),Base,Facts,Scope,Slot),"Unproved expansion topology admitted");}
    // Expanded facts do not weaken or retrofit the original reference admission.
    {Ledger ledger;auto facts=Audited;facts.exactReferenceCompiled=false;
        Require(!ledger.Admit(Id(),Bindings,facts,Scope,Slot),"Reference path lost exact-reference requirement");
        auto* state=ledger.Admit(Id(2),Bindings,Audited,Scope,Slot);Require(state,"Reference path changed after expansion support");}
}
namespace both_expanded_fixture {
using namespace expanded_fixture;
struct Fixture {
    graph::Snapshot source;
    std::array<scene::Snapshot,2> sections;
    std::array<borrowed::Sections,2> views;
    std::array<borrowed::CallerLifetime,2> lifetimes;
    expansion::BothEyesPlan plan;
};
Fixture Make(bool sharedRoot=false,uint32_t bindMask=3){
    auto original=expanded_fixture::Make();
    constexpr uint64_t OtherRoot=Root+0x1000;
    Put(OtherRoot,original.sections.RawHeader());
    graph::Snapshot initial;
    for(uint32_t view=0;view<2;++view){auto node=original.source.nodes[0];
        node.handle=0x20000000u|view;node.order=view;Field(node.bytes,0,view);Field(node.bytes,24,view);
        Field(node.bytes,16,view && !sharedRoot?OtherRoot:Root);initial.nodes.push_back(node);
    }
    std::array<cvr::stereo::paired_frontier::Edges,2> edges;
    for(auto& phase:edges)phase.emplace(1,0);
    cvr::stereo::OwnedNativeGraph compiled;
    Require(compiled.Create(initial,edges) && compiled.CompileReference(),"Both-eye fixture source compile failed");
    Fixture fixture;fixture.source=compiled.Snapshot();
    for(uint32_t view=0;view<2;++view){tree::Snapshot native;
        const uint64_t root=graph::Field<uint64_t>(fixture.source.nodes[view].bytes,16),roots[]{root};
        Require(native.Capture(roots,Base,Read,true) && fixture.sections[view].Capture(fixture.source.nodes[view],native,Read),
            "Both-eye fixture section capture failed");
        const auto& binding=Bindings[sharedRoot?0:view];
        fixture.lifetimes[view]={Id().consumerId,Id().epoch,Id().pairGeneration,root,
            binding.ownerHeader,binding.generationId,binding.bornMutationEpoch,binding.protectionReceipt};
        Require(fixture.views[view].Prepare(fixture.sections[view]),"Both-eye view preparation failed");
        if(bindMask&(1u<<view))Require(fixture.views[view].Bind(fixture.sections[view],fixture.lifetimes[view]),
            "Both-eye view binding failed");
    }
    const std::array<std::array<expansion::Definition,2>,2> definitions{fixture.views[0].Definitions(),fixture.views[1].Definitions()};
    Require(fixture.plan.Build(fixture.source,Base,definitions),"Both-eye expansion plan failed");
    return fixture;
}
State* Admit(Ledger& ledger,const Fixture& fixture,std::span<const OwnerGenerationBinding> bindings=Bindings){
    return ledger.AdmitExpanded(Id(),bindings,fixture.plan,fixture.views,fixture.sections,fixture.lifetimes,
        fixture.plan.Expanded(),Base,Facts,Scope,Slot);
}
}
void BothExpandedAdmission(){
    using namespace both_expanded_fixture;
    {auto fixture=both_expanded_fixture::Make();Ledger ledger;auto* state=both_expanded_fixture::Admit(ledger,fixture);
        Require(state && ledger.Size()==1 && state->OwnerBindings().size()==2,"Two-owner both-eye admission failed");
        for(uint32_t view=0;view<2;++view)Require(fixture.plan.SelectedSource(view)==fixture.sections[view].RootIdentity(),
            "Both-eye selected source lost original graph identity");
        Escape(*state);Queue(*state);Finish(*state,false);Require(ledger.Retire(Id().consumerId),"Both-eye lifecycle failed");}
    {auto fixture=both_expanded_fixture::Make(true);Ledger ledger;
        auto* state=both_expanded_fixture::Admit(ledger,fixture,std::span(Bindings).first(1));
        Require(state && state->OwnerBindings().size()==1,"Shared source owner required a fake second reader");
        Escape(*state);Queue(*state);Finish(*state,true);Require(ledger.Retire(Id().consumerId),"Shared owner lifetime failed");}
    for(uint32_t view=0;view<2;++view)for(uint32_t change=0;change<11;++change){
        auto fixture=both_expanded_fixture::Make();Ledger ledger;auto lifetimes=fixture.lifetimes;auto bindings=Bindings;
        if(change==0)++lifetimes[view].consumerId;if(change==1)++lifetimes[view].epoch;if(change==2)++lifetimes[view].pairGeneration;
        if(change==3)lifetimes[view].originalRoot+=8;if(change==4)lifetimes[view].ownerHeader+=8;
        if(change==5)++lifetimes[view].ownerGeneration;if(change==6)++lifetimes[view].ownerBornEpoch;
        if(change==7)++lifetimes[view].protectionReceipt;if(change==8)++bindings[view].generationId;
        if(change==9)++bindings[view].bornMutationEpoch;if(change==10)++bindings[view].protectionReceipt;
        Require(!ledger.AdmitExpanded(Id(),bindings,fixture.plan,fixture.views,fixture.sections,lifetimes,
            fixture.plan.Expanded(),Base,Facts,Scope,Slot) && !ledger.Size() && ledger.LastConsumerId()==Id().consumerId,
            "One changed eye binding admitted");
    }
    for(uint32_t change=0;change<13;++change){auto fixture=both_expanded_fixture::Make();Ledger ledger;auto identity=Id();auto facts=Facts;
        auto actual=fixture.plan.Expanded();auto bindings=Bindings;uint64_t base=Base;
        if(change==0)facts.sourceMappingAudited=false;if(change==1)facts.definitionProtectionAudited=false;
        if(change==2)facts.nativeCodeGuarded=false;if(change==3)facts.nativeContextOrderCompiled=false;
        if(change==4)++identity.consumerId;if(change==5)++identity.epoch;if(change==6)++identity.pairGeneration;
        if(change==7)actual.nodes.back().bytes[28]^=1;if(change==8)actual.nodes.pop_back();
        if(change==9)actual.edges.back().bytes[8]^=1;if(change==10)++base;
        if(change==11)bindings[1].protectionReceipt=bindings[0].protectionReceipt;
        if(change==12)std::swap(fixture.sections[0],fixture.sections[1]);
        Require(!ledger.AdmitExpanded(identity,bindings,fixture.plan,fixture.views,fixture.sections,fixture.lifetimes,
            actual,base,facts,Scope,Slot) && !ledger.Size(),"Changed two-eye expansion proof admitted");
        Require(!ledger.Admit(identity,Bindings,Audited,Scope,Slot) && ledger.Rejection()==Failure::Sequence,
            "Failed both-eye consumer ID reused");
    }
    for(uint32_t view=0;view<2;++view){
        {auto fixture=both_expanded_fixture::Make(false,3u^(1u<<view));Ledger ledger;
            Require(!both_expanded_fixture::Admit(ledger,fixture),"Unbound eye admitted");}
        {auto fixture=both_expanded_fixture::Make();Ledger ledger;borrowed::Sections replacement;
            Require(replacement.Create(fixture.sections[view],fixture.lifetimes[view]),"Replacement eye fixture failed");
            fixture.views[view]=std::move(replacement);
            Require(!both_expanded_fixture::Admit(ledger,fixture),"Foreign clone address for one eye admitted");}
    }
    {auto fixture=both_expanded_fixture::Make();Ledger ledger;
        Require(!both_expanded_fixture::Admit(ledger,fixture,std::span(Bindings).first(1)),"Missing second owner's reader admitted");}
    {auto fixture=both_expanded_fixture::Make(true);Ledger ledger;
        const std::array<OwnerGenerationBinding,2> duplicate{Bindings[0],Bindings[0]};
        Require(!both_expanded_fixture::Admit(ledger,fixture,duplicate),"Duplicated shared receipt became two readers");}
    {auto fixture=both_expanded_fixture::Make(true);Ledger ledger;
        fixture.sections[1]=fixture.sections[0];
        Require(!both_expanded_fixture::Admit(ledger,fixture,std::span(Bindings).first(1)),
            "Shared native root concealed foreign native-view source identity");}
    {auto fixture=both_expanded_fixture::Make();Ledger ledger;expansion::BothEyesPlan empty;
        Require(!ledger.AdmitExpanded(Id(),Bindings,empty,fixture.views,fixture.sections,fixture.lifetimes,
            fixture.plan.Expanded(),Base,Facts,Scope,Slot),"Empty both-eye plan admitted");}
}
}
int main()try{
    static_assert(!std::is_copy_constructible_v<State> && !std::is_move_constructible_v<State>);
    static_assert(!std::is_copy_constructible_v<Ledger> && !std::is_move_constructible_v<Ledger>);
    SuccessfulLifetimes();PublicationAndRollback();EscapeFailures();IncompleteEvidence();CancelAndRearm();InvalidAdmissionAndBounds();ExpandedAdmission();BothExpandedAdmission();
    std::puts("PASS owned graph execution state: 12544 lifetimes, immutable identities, rollback, producer/declaration/aggregate gates and quarantine");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
