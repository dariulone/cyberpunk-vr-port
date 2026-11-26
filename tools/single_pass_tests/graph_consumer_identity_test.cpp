#include "Render/NativeGraphConsumerIdentity.hpp"
#include <cstdio>
#include <initializer_list>
#include <map>
#include <stdexcept>
#include <type_traits>

using namespace cvr::stereo;
using namespace cvr::stereo::graph_jobs;
namespace {
constexpr uint64_t Base=0x140000000,OwnerAddress=0x10000,GraphAddress=0x10020,CompletionAddress=0x10040;
constexpr uint64_t Destination=0x11000,Payload=0x12000,Frame=0x18000,Group=0x19000,Graph=0x20000;
constexpr uint64_t Epoch=13,Generation=47,Id=99;
std::map<uint64_t,uint8_t> memory;
uint64_t reads{},failAt{},throwAt{},changeAt{},changeAddress{},changeValue{};
uint32_t ownerRetains{},ownerReleases{},groupRetains{},groupReleases{},completeCalls{};
int32_t ownerRefs{},groupRefs{},originalOwnerRefs{},originalGroupRefs{};
bool changeOnOwnerRetain{},changeOnGroupRetain{},forbidSourceReads{};
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
template<class T>void Put(uint64_t address,T value){
    const auto* bytes=reinterpret_cast<const uint8_t*>(&value);
    for(size_t i=0;i<sizeof(T);++i)memory[address+i]=bytes[i];
}
bool Read(uint64_t address,void* output,size_t count){
    Require(!forbidSourceReads || (address!=OwnerAddress && address!=GraphAddress && address!=CompletionAddress),"Expired root stack was read");
    Require(address!=Graph,"Ticket unexpectedly read the source graph");
    ++reads;if(reads==throwAt)throw std::runtime_error("Injected reader exception");if(reads==failAt)return false;
    if(reads==changeAt)Put(changeAddress,changeValue);
    if(address>UINT64_MAX-count)return false;
    auto* out=static_cast<uint8_t*>(output);
    for(size_t i=0;i<count;++i){const auto found=memory.find(address+i);if(found==memory.end())return false;out[i]=found->second;}
    return true;
}
void RetainOwner(void* value){
    Require(reinterpret_cast<uintptr_t>(value)==Frame && ownerRefs>0 && ownerRefs<INT32_MAX,"Invalid owner retain");
    ++ownerRefs;++ownerRetains;Put(Frame+8,ownerRefs);
    if(changeOnOwnerRetain)Put(OwnerAddress,Frame+8);
}
void ReleaseOwner(void* value){
    Require(reinterpret_cast<uintptr_t>(value)==Frame && ownerRefs>originalOwnerRefs && groupReleases==groupRetains,"Invalid owner release order");
    --ownerRefs;++ownerReleases;Put(Frame+8,ownerRefs);
}
void RetainGroup(void* value){
    Require(reinterpret_cast<uintptr_t>(value)==Group && groupRefs>0 && groupRefs<INT32_MAX,"Invalid group retain");
    ++groupRefs;++groupRetains;Put(Group+0x1c,groupRefs);
    if(changeOnGroupRetain)Put(CompletionAddress,Group+8);
}
void ReleaseGroup(void** value){
    Require(value && reinterpret_cast<uintptr_t>(*value)==Group && groupRefs>originalGroupRefs,"Invalid group release");
    --groupRefs;++groupReleases;Put(Group+0x1c,groupRefs);*value=nullptr;
}
bool Complete(void*){++completeCalls;return true;}
constexpr JobLeaseApi Jobs{RetainGroup,ReleaseGroup,Complete};
constexpr FrameOwnerApi Owners{RetainOwner,ReleaseOwner};
void Reset(){
    memory.clear();reads=failAt=throwAt=changeAt=changeAddress=changeValue=0;
    ownerRetains=ownerReleases=groupRetains=groupReleases=completeCalls=0;
    ownerRefs=groupRefs=originalOwnerRefs=originalGroupRefs=1;
    changeOnOwnerRetain=changeOnGroupRetain=forbidSourceReads=false;
    Put(OwnerAddress,Frame);Put(GraphAddress,Graph);Put(CompletionAddress,Group);
    Put(Frame+8,ownerRefs);Put(Group+0x1c,groupRefs);Put<int32_t>(Group+0x18,0);
}
bool Capture(NativeRootTicket& ticket,uint64_t epoch=Epoch,uint64_t generation=Generation){
    return ticket.CaptureBeforeConstruct(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn,Base,epoch,generation,Id,Read,Jobs,Owners);
}
void NativeConstruct(){
    Put(Destination,Frame);Put(Destination+8,Graph);Put(Destination+0x18,Group);Put<uint64_t>(CompletionAddress,0);
}
void NativeQueued(){
    Put(Payload,Frame);Put(Payload+8,Graph);Put(Payload+0x18,Group);Put<uint64_t>(Destination+0x18,0);
}
void Balanced(){
    Require(ownerRefs==originalOwnerRefs && groupRefs==originalGroupRefs && ownerRetains==ownerReleases &&
        groupRetains==groupReleases && completeCalls==0,"Unbalanced identity refs or prerequisite used as completion");
}
void Lifecycle(){
    Reset();
    {NativeRootTicket ticket;Require(!ticket.Captured() && !ticket.ConstructVerified(),"New ticket already captured");
        Require(Capture(ticket) && ticket.Captured() && !ticket.ConstructVerified(),"Valid constructor identity rejected");
        Require(ticket.Id()==Id && ticket.Epoch()==Epoch && ticket.Generation()==Generation && ticket.ImageBase()==Base &&
            ticket.FrameOwner()==Frame && ticket.SourceGraphAddress()==Graph && ticket.GraphAddress()==Graph && !ticket.Redirected() && ticket.Group()==Group,"Identity dereference or frozen labels differ");
        Require(ownerRefs==2 && groupRefs==2 && completeCalls==0,"Identity did not hold both distinct references");
        Require(!ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read),"Unverified construction matched queue");
        for(const uint32_t caller:{0x26E110u,0x2735E3u})
            Require(!ticket.MatchExecutionCallback(Payload,Graph,Base+caller,Read),"Unverified construction matched callback");
        NativeConstruct();Require(ticket.VerifyConstruct(Destination,Read) && ticket.ConstructVerified(),"Native ownership transfer rejected");
        Require(!ticket.VerifyConstruct(Destination,Read),"Construction checked more than once");
        NativeQueued();forbidSourceReads=true;
        for(uint64_t address:{OwnerAddress,GraphAddress,CompletionAddress})for(size_t i=0;i<8;++i)memory.erase(address+i);
        Require(ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read),"Late queue identity depends on expired root stack");
        for(const uint32_t caller:{0x26E110u,0x2735E3u})
            Require(ticket.MatchExecutionCallback(Payload,Graph,Base+caller,Read),"Execution callback depends on expired root stack");
        Require(!ticket.MatchExecutionCallback(Payload,Graph,Base+QueueReturn,Read),"Queue caller was accepted as scheduler callback");
        Require(!ticket.MatchExecutionCallback(Payload,Graph+8,Base+0x26E110,Read),"Different callback graph was accepted");
        uint64_t currentEpoch=Epoch+1,currentGeneration=Generation+1;
        Require(currentEpoch!=ticket.Epoch() && currentGeneration!=ticket.Generation() &&
            ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read),"Unrelated current epochs invalidated native consumer accounting");
        Require(!Capture(ticket) && ticket.Rejection()==IdentityFailure::AlreadyCaptured &&
            ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read),"Recapture replaced the held native identity");
        for(const int32_t pending:{0,7}){
            Put(Group+0x18,pending);Require(ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read),"Pending count changed identity matching");
        }
        Require(ownerRetains==1 && groupRetains==1 && !ownerReleases && !groupReleases,"Identity released before caller retired ticket");
    }
    Balanced();Reset();{NativeRootTicket ticket;Require(Capture(ticket,0,0) && ticket.Epoch()==0 && ticket.Generation()==0,
        "Unarmed native consumers could not be accounted");}Balanced();
}
void InvalidArguments(){
    Reset();
    const auto reject=[&](uint64_t owner,uint64_t graph,uint64_t completion,uint64_t caller,uint64_t base,uint64_t id,ReadMemory reader){
        NativeRootTicket ticket;Require(!ticket.CaptureBeforeConstruct(owner,graph,completion,caller,base,Epoch,Generation,id,reader,Jobs,Owners) &&
            !ticket.Captured(),"Invalid constructor arguments accepted");Balanced();
    };
    reject(0,GraphAddress,CompletionAddress,Base+ConstructReturn,Base,Id,Read);
    reject(OwnerAddress+1,GraphAddress,CompletionAddress,Base+ConstructReturn,Base,Id,Read);
    reject(OwnerAddress,GraphAddress+1,CompletionAddress,Base+ConstructReturn,Base,Id,Read);
    reject(OwnerAddress,Graph,CompletionAddress,Base+ConstructReturn,Base,Id,Read);
    reject(OwnerAddress,GraphAddress,UINT64_MAX-7,Base+ConstructReturn,Base,Id,Read);
    reject(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn+1,Base,Id,Read);
    reject(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn,0,Id,Read);
    reject(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn,UINT64_MAX,Id,Read);
    reject(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn,Base,0,Read);
    reject(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn,Base,Id,nullptr);
    for(const unsigned missing:{0u,1u,2u,3u}){
        auto jobs=Jobs;auto owners=Owners;
        if(missing==0)jobs.retain=nullptr;if(missing==1)jobs.release=nullptr;
        if(missing==2)owners.retain=nullptr;if(missing==3)owners.release=nullptr;
        NativeRootTicket ticket;Require(!ticket.CaptureBeforeConstruct(OwnerAddress,GraphAddress,CompletionAddress,
            Base+ConstructReturn,Base,Epoch,Generation,Id,Read,jobs,owners) && ticket.Rejection()==IdentityFailure::MissingApi,"Missing reference API accepted");
    }
    Balanced();
    // A complete callback is neither required nor called by identity-only tickets.
    {NativeRootTicket ticket;auto jobs=Jobs;jobs.complete=nullptr;
        Require(ticket.CaptureBeforeConstruct(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn,
            Base,Epoch,Generation,Id,Read,jobs,Owners),"Identity required a false completion contract");}Balanced();
    for(const uint64_t pointer:{uint64_t(0),uint64_t(1),UINT64_MAX-7})for(const uint64_t address:{OwnerAddress,GraphAddress,CompletionAddress}){
        Reset();Put(address,pointer);NativeRootTicket ticket;Require(!Capture(ticket),"Invalid identity pointer accepted");Balanced();
    }
    Reset();Put(OwnerAddress,Group);{NativeRootTicket ticket;Require(!Capture(ticket),"Owner/group reference domains were aliased");}Balanced();
    for(const int32_t refs:{0,-1,INT32_MAX})for(const uint64_t address:{Frame+8,Group+0x1c}){
        Reset();Put(address,refs);NativeRootTicket ticket;Require(!Capture(ticket) && ticket.Rejection()==IdentityFailure::References,
            "Dead or overflowing reference accepted");Balanced();
    }
    Reset();ownerRefs=groupRefs=originalOwnerRefs=originalGroupRefs=INT32_MAX-1;Put(Frame+8,ownerRefs);Put(Group+0x1c,groupRefs);
    {NativeRootTicket ticket;Require(Capture(ticket) && ownerRefs==INT32_MAX && groupRefs==INT32_MAX,"Last nonoverflowing retain rejected");}Balanced();
}
void RollbackAndTornReads(){
    Reset();uint64_t captureReads{};{NativeRootTicket ticket;Require(Capture(ticket),"Read baseline rejected");captureReads=reads;}Balanced();
    for(uint64_t at=1;at<=captureReads;++at)for(const bool throws:{false,true}){
        Reset();if(throws)throwAt=at;else failAt=at;
        {NativeRootTicket ticket;Require(!Capture(ticket) && !ticket.Captured(),"Incomplete/throwing capture accepted");}Balanced();
    }
    for(const bool onOwner:{false,true}){
        Reset();changeOnOwnerRetain=onOwner;changeOnGroupRetain=!onOwner;
        {NativeRootTicket ticket;Require(!Capture(ticket) && ticket.Rejection()==IdentityFailure::Changed,"Retain-time identity change accepted");}Balanced();
        Require(ownerRetains==1 && groupRetains==1,"Rollback test did not acquire both pins");
    }
    for(const uint64_t address:{OwnerAddress,GraphAddress,CompletionAddress}){
        Reset();changeAt=4;changeAddress=address;changeValue=address==OwnerAddress?Frame+8:address==GraphAddress?Graph+8:Group+8;
        {NativeRootTicket ticket;Require(!Capture(ticket) && ticket.Rejection()==IdentityFailure::Changed,"Identity changed between initial reads");}Balanced();
        Require(!ownerRetains && !groupRetains,"Torn initial capture acquired references");
    }
}
void ConstructAndQueueFailures(){
    for(const uint64_t field:{Destination,Destination+8,Destination+0x18,CompletionAddress}){
        Reset();{NativeRootTicket ticket;Require(Capture(ticket),"Construction fixture capture failed");NativeConstruct();Put(field,uint64_t(0x33330));
            Require(!ticket.VerifyConstruct(Destination,Read) && !ticket.ConstructVerified(),"Wrong constructor transfer accepted");
            NativeConstruct();Require(!ticket.VerifyConstruct(Destination,Read),"Rejected construction retried into a valid receipt");}Balanced();
    }
    for(uint64_t at=1;at<=8;++at){
        Reset();{NativeRootTicket ticket;Require(Capture(ticket),"Construction fixture capture failed");NativeConstruct();reads=0;failAt=at;
            Require(!ticket.VerifyConstruct(Destination,Read),"Partial constructed payload read accepted");}Balanced();
    }
    for(const uint64_t field:{Payload,Payload+8,Payload+0x18}){
        Reset();{NativeRootTicket ticket;Require(Capture(ticket),"Queue fixture capture failed");NativeConstruct();Require(ticket.VerifyConstruct(Destination,Read),"Construction verification failed");
            NativeQueued();Put(field,uint64_t(0x33330));Require(!ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read),"Foreign queued identity accepted");
            NativeQueued();reads=0;changeAt=4;changeAddress=field;changeValue=0x33330;
            Require(!ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read),"Queued identity changed between reads");}Balanced();
    }
    Reset();{NativeRootTicket ticket;Require(Capture(ticket),"Queue fixture capture failed");NativeConstruct();Require(ticket.VerifyConstruct(Destination,Read),"Construction verification failed");NativeQueued();
        Require(!ticket.MatchQueued(Payload,Graph+8,Base+QueueReturn,Read) && !ticket.MatchQueued(Payload,Graph,Base+QueueReturn+1,Read) &&
            !ticket.MatchQueued(UINT64_MAX-7,Graph,Base+QueueReturn,Read) && !ticket.MatchQueued(Payload,Graph,Base+QueueReturn,nullptr),"Queue caller/pointer validation missing");
        for(uint64_t at=1;at<=6;++at){reads=0;failAt=at;Require(!ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read),"Partial queued payload read accepted");}
    }Balanced();
    Reset();{NativeRootTicket ticket;Require(Capture(ticket),"Queue fixture capture failed");NativeConstruct();
        reads=0;changeAt=5;changeAddress=CompletionAddress;changeValue=Group;
        Require(!ticket.VerifyConstruct(Destination,Read) && ticket.Rejection()==IdentityFailure::SourceNotConsumed,"Consumed source handle changed between reads");}Balanced();
}
void ExecutionCallbackFailures(){
    for(const uint32_t caller:{0x26E110u,0x2735E3u}){
        Reset();{NativeRootTicket ticket;Require(Capture(ticket),"Callback fixture capture failed");NativeConstruct();
            Require(ticket.VerifyConstruct(Destination,Read),"Callback construction verification failed");NativeQueued();forbidSourceReads=true;
            Require(!ticket.MatchExecutionCallback(Payload,Graph,Base+caller-1,Read) &&
                !ticket.MatchExecutionCallback(Payload,Graph,Base+caller+1,Read) &&
                !ticket.MatchExecutionCallback(Payload,Graph+8,Base+caller,Read) &&
                !ticket.MatchExecutionCallback(UINT64_MAX-7,Graph,Base+caller,Read) &&
                !ticket.MatchExecutionCallback(Payload,Graph,Base+caller,nullptr),"Callback caller/pointer validation missing");
            for(const uint64_t field:{Payload,Payload+8,Payload+0x18}){
                Put(field,uint64_t(0x33330));
                Require(!ticket.MatchExecutionCallback(Payload,Graph,Base+caller,Read),"Foreign callback identity accepted");
                NativeQueued();reads=0;changeAt=4;changeAddress=field;changeValue=0x33330;
                Require(!ticket.MatchExecutionCallback(Payload,Graph,Base+caller,Read),"Callback identity changed between reads");
                changeAt=0;NativeQueued();
            }
            for(uint64_t at=1;at<=6;++at)for(const bool throws:{false,true}){
                reads=0;failAt=throws?0:at;throwAt=throws?at:0;
                Require(!ticket.MatchExecutionCallback(Payload,Graph,Base+caller,Read),"Partial/throwing callback payload read accepted");
            }
            failAt=throwAt=0;
            Require(ticket.MatchExecutionCallback(Payload,Graph,Base+caller,Read),"Valid callback identity lost after rejected observations");
        }Balanced();
    }
}
void SeparateSourceAndExecution(){
    constexpr uint64_t execution=0x30000;
    Reset();
    {NativeRootTicket ticket;
        Require(ticket.CaptureBeforeConstruct(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn,Base,
            Epoch,Generation,Id,Read,Jobs,Owners,execution),"Separate execution identity rejected");
        Require(ticket.SourceGraphAddress()==Graph && ticket.GraphAddress()==execution && ticket.Redirected() && ownerRetains==1 && groupRetains==1,
            "Source provenance or unique pins were replaced by execution identity");
        // Publication belongs to the transaction owner, not this read-only ticket.
        uint64_t original{};Require(Read(GraphAddress,&original,8) && original==Graph,"Ticket changed the native graph slot");
        NativeConstruct();Put(Destination+8,execution);
        Require(ticket.VerifyConstruct(Destination,Read),"Redirected constructor payload rejected");
        NativeQueued();Put(Payload+8,execution);forbidSourceReads=true;
        Require(ticket.MatchQueued(Payload,execution,Base+QueueReturn,Read) &&
            ticket.MatchExecutionCallback(Payload,execution,Base+0x2735E3,Read),"Owned execution lost immutable native identity");
        Require(!ticket.MatchQueued(Payload,Graph,Base+QueueReturn,Read) &&
            !ticket.MatchExecutionCallback(Payload,Graph,Base+0x26E110,Read),"Source graph masqueraded as redirected execution");
    }Balanced();
    Reset();{NativeRootTicket ticket;
        Require(ticket.CaptureBeforeConstruct(OwnerAddress,GraphAddress,CompletionAddress,Base+ConstructReturn,Base,
            Epoch,Generation,Id,Read,Jobs,Owners,execution),"Wrong-payload fixture failed");
        NativeConstruct();Require(!ticket.VerifyConstruct(Destination,Read),"Unpublished execution override was accepted");
    }Balanced();
    for(const uint64_t invalid:{uint64_t(1),UINT64_MAX-7,Frame,Group}){
        Reset();{NativeRootTicket ticket;Require(!ticket.CaptureBeforeConstruct(OwnerAddress,GraphAddress,CompletionAddress,
            Base+ConstructReturn,Base,Epoch,Generation,Id,Read,Jobs,Owners,invalid),"Invalid execution override accepted");}Balanced();
    }
    Reset();changeAt=4;changeAddress=GraphAddress;changeValue=Graph+8;
    {NativeRootTicket ticket;Require(!ticket.CaptureBeforeConstruct(OwnerAddress,GraphAddress,CompletionAddress,
        Base+ConstructReturn,Base,Epoch,Generation,Id,Read,Jobs,Owners,execution),"Torn source hidden by execution override");}Balanced();
}
}
int main()try{
    static_assert(!std::is_copy_constructible_v<NativeRootTicket> && !std::is_move_constructible_v<NativeRootTicket>);
    Lifecycle();InvalidArguments();RollbackAndTornReads();ConstructAndQueueFailures();ExecutionCallbackFailures();SeparateSourceAndExecution();
    std::puts("PASS native root identity ticket: group/owner pins, constructor transfer, late queue identity, rollback and no completion inference");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
