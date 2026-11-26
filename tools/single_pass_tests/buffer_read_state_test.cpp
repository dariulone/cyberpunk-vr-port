#include "Render/NativeBufferRead.hpp"
#include <cstdio>
#include <initializer_list>
#include <map>
#include <source_location>
#include <stdexcept>
#include <string>
#include <vector>

using namespace cvr::stereo;
using buffer_read::detail::History;
using buffer_read::detail::RecordingState;
namespace {
void Require(bool good,const std::source_location at=std::source_location::current()){
    if(!good)throw std::runtime_error("buffer read state assertion at line "+std::to_string(at.line()));
}
void Submit(History& history,uint64_t recording){
    history.BeforeSubmit(recording,true);Require(history.valid && history.pending && !history.Matches(history.epoch));
    history.Submitted(recording,true);Require(history.Matches(history.epoch));
}
History Uploaded(){History history;history.Written(101);Submit(history,101);return history;}
void UploadEpochs(){
    History history;Require(history.valid && history.epoch && !history.Matches(history.epoch));
    const auto initial=history.epoch;
    history.Written(101);Require(history.valid && history.epoch==initial+1 && !history.Matches(history.epoch));
    history.Written(101);Require(history.valid && history.epoch==initial+2);
    Submit(history,101);const auto first=history.epoch;
    Require(!history.Matches(0) && !history.Matches(first-1) && !history.Matches(first+1));
    for(const auto recording:{202ull,303ull,404ull}){
        const auto previous=history.epoch;history.Written(recording);
        Require(history.valid && history.epoch==previous+1 && !history.Matches(previous));
        history.Written(recording);Require(history.valid && history.epoch==previous+2);
        Submit(history,recording);Require(!history.Matches(previous) && !history.Matches(first));
    }
    const auto saved=history.epoch;history.Invalidate();history.Written(505);
    history.BeforeSubmit(505,true);history.Submitted(505,true);
    Require(!history.valid && !history.Matches(saved) && history.epoch==saved);
}
void InvalidUploadHistories(){
    {
        History history;history.Written(0);history.Written(101);Require(!history.valid);
    }
    {
        History history;history.Written(101);history.BeforeSubmit(101,true);history.Written(101);
        Require(!history.valid && !history.Matches(history.epoch));
    }
    {
        History history;history.epoch=UINT64_MAX;history.Written(101);
        Require(!history.valid && history.epoch==UINT64_MAX);
    }
    {
        History history;history.BeforeSubmit(101,true);Require(!history.valid);
    }
    for(const auto recording:{0ull,202ull}){
        History before;before.Written(101);before.BeforeSubmit(recording,true);Require(!before.valid);
        History after;after.Written(101);after.BeforeSubmit(101,true);after.Submitted(recording,true);
        Require(!after.valid && !after.Matches(after.epoch));
    }
    {
        History history;history.Written(101);history.BeforeSubmit(101,false);
        history.Submitted(101,true);Require(!history.valid);
    }
    {
        History history;history.Written(101);history.BeforeSubmit(101,true);
        history.Submitted(101,false);Require(!history.valid);
    }
    {
        History history;history.Written(101);history.Submitted(101,true);Require(!history.valid);
    }
    {
        History history;history.Written(101);history.BeforeSubmit(101,true);history.BeforeSubmit(101,true);
        history.Submitted(101,true);Require(!history.valid);
    }
    {
        auto history=Uploaded();history.Submitted(101,true);Require(!history.valid);
    }
    {
        auto history=Uploaded();history.BeforeSubmit(101,true);Require(!history.valid);
    }
    {
        auto history=Uploaded();history.Written(101);Require(!history.valid);
    }
    for(const bool pending:{false,true}){
        History history;history.Written(101);if(pending)history.BeforeSubmit(101,true);
        history.Abandon(202);Require(history.valid);
        history.Abandon(101);history.Submitted(101,true);Require(!history.valid);
    }
    {
        auto history=Uploaded();const auto saved=history.epoch;
        history.Abandon(0);history.Abandon(202);history.Abandon(101);Require(history.Matches(saved));
    }
}
void ProducerOrders(){
    const std::array<std::array<uint64_t,3>,6> orders{{
        {101,202,303},{101,303,202},{202,101,303},
        {202,303,101},{303,101,202},{303,202,101}}};
    for(const auto& order:orders){
        for(const bool batch:{false,true}){
            History history;const auto initial=history.epoch;
            for(const auto recording:{101ull,202ull,303ull})history.Written(recording);
            Require(history.valid && history.epoch==initial+3 && !history.uploaded);
            const auto saved=history.epoch;Require(!history.Matches(saved));
            if(batch){
                for(const auto recording:{101ull,202ull,303ull}){
                    history.BeforeSubmit(recording,true);Require(history.valid && history.pending && !history.uploaded);
                }
            }
            for(size_t i=0;i<order.size();++i){
                if(!batch)history.BeforeSubmit(order[i],true);
                Require(history.valid && history.pending && !history.Matches(saved));
                history.Submitted(order[i],true);
                const bool complete=i+1==order.size();
                Require(history.valid && history.uploaded==complete && history.Matches(saved)==complete);
                Require(history.pending==(batch && !complete));
            }
        }
    }
    History growing;growing.Written(101);growing.BeforeSubmit(101,true);
    auto previous=growing.epoch;growing.Written(202);
    Require(growing.valid && growing.pending && growing.epoch==previous+1 && !growing.Matches(previous));
    previous=growing.epoch;growing.Written(202);
    Require(growing.valid && growing.pending && growing.epoch==previous+1);
    growing.BeforeSubmit(202,true);previous=growing.epoch;growing.Written(303);
    Require(growing.valid && growing.pending && growing.epoch==previous+1);
    growing.Submitted(202,true);growing.Abandon(202);
    Require(growing.valid && growing.pending && !growing.uploaded);
    growing.Submitted(101,true);growing.Abandon(101);
    Require(growing.valid && !growing.pending && !growing.uploaded && !growing.Matches(growing.epoch));
    previous=growing.epoch;growing.Written(303);
    Require(growing.valid && growing.epoch==previous+1);Submit(growing,303);
}
void InvalidProducerCohorts(){
    const auto active=[] {
        History history;history.Written(101);history.Written(202);history.Written(303);return history;
    };
    for(const auto recording:{0ull,404ull}){
        auto before=active();before.BeforeSubmit(recording,true);Require(!before.valid);
        auto after=active();after.BeforeSubmit(101,true);after.Submitted(recording,true);Require(!after.valid);
    }
    for(const auto recording:{101ull,202ull,303ull}){
        auto before=active();before.BeforeSubmit(recording,false);Require(!before.valid);
        auto after=active();after.BeforeSubmit(recording,true);after.Submitted(recording,false);Require(!after.valid);
        auto missing=active();missing.Submitted(recording,true);Require(!missing.valid);
        auto duplicate=active();duplicate.BeforeSubmit(recording,true);duplicate.BeforeSubmit(recording,true);
        Require(!duplicate.valid);
        auto pendingWrite=active();pendingWrite.BeforeSubmit(recording,true);pendingWrite.Written(recording);
        Require(!pendingWrite.valid);
        auto completed=active();completed.BeforeSubmit(recording,true);completed.Submitted(recording,true);
        Require(completed.valid && !completed.uploaded);
        auto replay=completed;replay.BeforeSubmit(recording,true);Require(!replay.valid);
        auto duplicateAfter=completed;duplicateAfter.Submitted(recording,true);Require(!duplicateAfter.valid);
        completed.Written(recording);Require(!completed.valid);
    }
    for(const bool pending:{false,true}){
        auto history=active();history.BeforeSubmit(101,true);history.Submitted(101,true);
        if(pending)history.BeforeSubmit(202,true);
        history.Abandon(101);history.Abandon(404);Require(history.valid);
        history.Abandon(202);Require(!history.valid && !history.Matches(history.epoch));
    }
}
void ProducerCapacityAndReuse(){
    History full;const auto initial=full.epoch;
    for(uint64_t recording=1;recording<=128;++recording)full.Written(recording);
    Require(full.valid && full.epoch==initial+128 && !full.uploaded);
    full.Written(64);Require(full.valid && full.epoch==initial+129);
    auto overflow=full;overflow.Written(129);Require(!overflow.valid && !overflow.Matches(overflow.epoch));
    full.BeforeSubmit(1,true);full.Submitted(1,true);
    Require(full.valid && !full.uploaded);full.Abandon(1);Require(full.valid);
    auto partial=full;partial.Written(129);Require(!partial.valid);
    for(uint64_t recording=128;recording>1;--recording){
        full.BeforeSubmit(recording,true);full.Submitted(recording,true);
        Require(full.valid && full.uploaded==(recording==2));
    }
    const auto saved=full.epoch;Require(full.Matches(saved));
    auto replay=full;replay.Written(128);Require(!replay.valid);
    for(uint64_t recording=1001;recording<=1128;++recording){
        full.Written(recording);Require(full.valid && !full.uploaded && !full.Matches(saved));
    }
    Require(full.epoch==saved+128);
    auto oldBefore=full;oldBefore.BeforeSubmit(1,true);Require(!oldBefore.valid);
    auto oldAfter=full;oldAfter.BeforeSubmit(1001,true);oldAfter.Submitted(1,true);Require(!oldAfter.valid);
    full.Abandon(1);Require(full.valid);
    for(uint64_t recording=1001;recording<=1128;++recording)full.BeforeSubmit(recording,true);
    for(uint64_t recording=1128;recording>=1001;--recording){
        full.Submitted(recording,true);Require(full.valid && full.uploaded==(recording==1001));
    }
    Require(full.Matches(full.epoch));
    full.Written(2001);Submit(full,2001);
}
void RecordingOwnership(){
    RecordingState state;Require(state.valid && !state.pending && !state.submitted);
    Require(state.BeforeSubmit() && state.pending && !state.submitted);
    Require(state.Submitted() && !state.pending && state.submitted);
    Require(!state.BeforeSubmit() && !state.valid && !state.Submitted());
    RecordingState duplicate;Require(duplicate.BeforeSubmit());
    Require(!duplicate.BeforeSubmit() && !duplicate.valid && !duplicate.Submitted());
    RecordingState unsubmitted;Require(!unsubmitted.Submitted() && !unsubmitted.valid && !unsubmitted.BeforeSubmit());
    RecordingState completed;Require(completed.BeforeSubmit() && completed.Submitted());
    Require(!completed.Submitted() && !completed.valid);
    RecordingState invalid;invalid.valid=false;Require(!invalid.BeforeSubmit() && !invalid.Submitted());

    // Reset abandons an unsubmitted producer; a new recording cannot repair its history.
    for(const bool pending:{false,true}){
        History history;history.Written(101);RecordingState old;
        if(pending){Require(old.BeforeSubmit());history.BeforeSubmit(101,true);}
        history.Abandon(101);RecordingState replacement;
        Require(replacement.BeforeSubmit() && replacement.Submitted());
        history.Written(202);history.BeforeSubmit(202,true);history.Submitted(202,true);
        Require(!history.valid && !history.Matches(history.epoch));
    }
}
struct Memory {
    std::map<uint64_t,std::vector<uint8_t>> blocks;
    size_t calls{},failCall{};
};
template<class T> void Put(Memory& memory,uint64_t address,const T& value){
    auto& bytes=memory.blocks[address];bytes.resize(sizeof(value));std::memcpy(bytes.data(),&value,sizeof(value));
}
bool Read(void* user,uint64_t address,void* out,size_t bytes){
    auto& memory=*static_cast<Memory*>(user);
    if(++memory.calls==memory.failCall)return false;
    auto it=memory.blocks.upper_bound(address);if(it==memory.blocks.begin())return false;--it;
    const auto offset=address-it->first;
    if(offset>it->second.size() || bytes>it->second.size()-offset)return false;
    std::memcpy(out,it->second.data()+offset,bytes);return true;
}
struct Fixture {
    uint64_t game=0x140000000,registry=0x10000000,resourceGpu=0xa0000000,resourceBytes;
    geometry::NativeBuffer source;
    struct Group {uint64_t data;uint32_t capacity,count;} group;
    std::array<uint8_t,0x50> record{};
    std::array<uint8_t,144> entry{};
    uint64_t recordAddress,entryAddress,owner=0x77770000;
    uint32_t allocation;
    Memory memory;
    Fixture(uint32_t slot=65,uint32_t index=3,uint64_t bytes=512*128,uint32_t handle=2):
        resourceBytes(bytes),source{0xf000000,resourceGpu+uint64_t(slot)*512,292,registry,0,handle,1},
        group{0x800000,index+1,index+1},recordAddress(registry+0x5c0af8+uint64_t(handle-1)*0xb0),
        entryAddress(group.data+uint64_t(index)*144),allocation((slot<<16)|(index<<3)){
        std::memcpy(record.data(),&source.bytes,4);record[6]=0x16;
        std::memcpy(record.data()+8,&source.gpu,8);std::memcpy(record.data()+16,&source.resource,8);
        std::memcpy(record.data()+0x40,&allocation,4);
        std::memcpy(entry.data(),&source.resource,8);std::memcpy(entry.data()+8,&owner,8);
        const uint64_t occupied=uint64_t(1)<<(slot%64);
        std::memcpy(entry.data()+16+uint64_t(slot/64)*8,&occupied,8);
        Put(memory,game+0x3bcdca8,registry);Put(memory,game+0x36f44c8,uint32_t(512));
        Put(memory,registry+0x14105c0,uint8_t(1));Put(memory,registry+0x1410548,group);
        Put(memory,recordAddress,record);Put(memory,entryAddress,entry);
    }
    template<class T> void Record(size_t offset,const T& value){
        std::memcpy(record.data()+offset,&value,sizeof(value));Put(memory,recordAddress,record);
    }
    template<class T> void Entry(size_t offset,const T& value){
        std::memcpy(entry.data()+offset,&value,sizeof(value));Put(memory,entryAddress,entry);
    }
    bool ReadPool(buffer_read::Pool& pool){
        return buffer_read::detail::ReadPool(Read,&memory,game,source,resourceGpu,resourceBytes,pool);
    }
    buffer_read::Pool Accepted(){
        buffer_read::Pool pool{};Require(ReadPool(pool));
        Require(pool.entry==entryAddress && pool.owner==owner && pool.allocation==allocation);return pool;
    }
    void Rejected(){
        buffer_read::Pool pool{1,2,3};Require(!ReadPool(pool) && pool==buffer_read::Pool{});
    }
};
template<class Mutate> void Reject(Mutate mutate){Fixture fixture;mutate(fixture);fixture.Rejected();}
void ValidPoolsAndInitialStates(){
    Require(buffer_read::detail::InitialState(D3D12_RESOURCE_STATE_COMMON));
    Require(buffer_read::detail::InitialState(D3D12_RESOURCE_STATE_GENERIC_READ));
    for(const auto state:{uint32_t(D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER),
        uint32_t(D3D12_RESOURCE_STATE_COPY_SOURCE),uint32_t(D3D12_RESOURCE_STATE_COPY_DEST),
        uint32_t(D3D12_RESOURCE_STATE_UNORDERED_ACCESS),uint32_t(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE),
        uint32_t(D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER|D3D12_RESOURCE_STATE_COPY_SOURCE),UINT32_MAX})
        Require(!buffer_read::detail::InitialState(state));
    for(const uint32_t slot:{0u,63u,64u,65u,1023u}){
        Fixture fixture(slot,3,512*1024);fixture.Accepted();
    }
    Fixture minimum(0,0,512,1);minimum.Accepted();
    Fixture maximum(1023,8191,512*1024,0x100000);maximum.Accepted();
    for(const uint32_t bytes:{292u,293u,304u,320u,512u}){
        Fixture padded;padded.source.bytes=bytes;padded.Record(0,bytes);padded.Accepted();
    }
    Fixture end;end.resourceGpu=UINT64_MAX-end.resourceBytes;
    end.source.gpu=end.resourceGpu+uint64_t(end.allocation>>16)*512;
    end.Record(8,end.source.gpu);end.Accepted();
}
void PoolIdentityAndLayout(){
    Reject([](auto& f){Put(f.memory,f.game+0x3bcdca8,f.registry+1);});
    Reject([](auto& f){Put(f.memory,f.registry+0x14105c0,uint8_t(0));});
    for(const uint32_t unit:{0u,256u,511u,513u,1024u})
        Reject([=](auto& f){Put(f.memory,f.game+0x36f44c8,unit);});
    for(const uint32_t kind:{0u,2u,3u})Reject([=](auto& f){f.source.kind=kind;});
    for(const uint8_t kind:{uint8_t(0),uint8_t(0x06),uint8_t(0x15),uint8_t(0x26),uint8_t(0x96)})
        Reject([=](auto& f){f.Record(6,kind);});
    for(const uint32_t bytes:{0u,291u,293u,512u,513u}){
        Reject([=](auto& f){f.source.bytes=bytes;});
        Reject([=](auto& f){f.Record(0,bytes);});
    }
    for(const uint32_t bytes:{0u,291u,513u})Reject([=](auto& f){f.source.bytes=bytes;f.Record(0,bytes);});
    Reject([](auto& f){f.Record(8,f.source.gpu+1);});
    Reject([](auto& f){f.Record(16,f.source.resource+1);});
    Reject([](auto& f){++f.source.gpu;});
    Reject([](auto& f){++f.source.resource;});
    for(uint32_t flag=1;flag<8;++flag)Reject([=](auto& f){f.Record(0x40,f.allocation|flag);});
    Reject([](auto& f){f.Record(0x40,f.allocation+(1u<<16));});
    Reject([](auto& f){f.Record(0x40,(128u<<16)|(f.allocation&0xffff));});
    Reject([](auto& f){f.Record(0x40,(65535u<<16)|(f.allocation&0xffff));});
    Reject([](auto& f){f.Entry(0,f.source.resource+1);});
    Reject([](auto& f){f.Entry(8,uint64_t(0));});
    Reject([](auto& f){f.Entry(24,uint64_t(0));});
    Reject([](auto& f){f.Entry(24,uint64_t(1));});
    Reject([](auto& f){f.group.data=0;Put(f.memory,f.registry+0x1410548,f.group);});
    Reject([](auto& f){f.group.count=0;Put(f.memory,f.registry+0x1410548,f.group);});
    Reject([](auto& f){f.group.capacity=0;Put(f.memory,f.registry+0x1410548,f.group);});
    Reject([](auto& f){f.group.count=f.group.capacity+1;Put(f.memory,f.registry+0x1410548,f.group);});
    Reject([](auto& f){f.group.capacity=8193;Put(f.memory,f.registry+0x1410548,f.group);});
    Reject([](auto& f){f.group.count=3;Put(f.memory,f.registry+0x1410548,f.group);});
}
void UnreadableAndChangedPools(){
    Fixture complete;complete.Accepted();const auto reads=complete.memory.calls;Require(reads==8);
    for(size_t failure=1;failure<=reads;++failure){
        Fixture fixture;fixture.memory.failCall=failure;fixture.Rejected();
    }
    Reject([](auto& f){f.memory.blocks[f.recordAddress].resize(0x4f);});
    Reject([](auto& f){f.memory.blocks[f.entryAddress].resize(31);});
    Fixture absent;buffer_read::Pool output{1,2,3};
    Require(!buffer_read::detail::ReadPool(nullptr,&absent.memory,absent.game,absent.source,
        absent.resourceGpu,absent.resourceBytes,output) && output==buffer_read::Pool{});

    // ReadPool is one pass. The caller rejects changed identities between two successful passes.
    Fixture changed;const auto before=changed.Accepted();
    changed.owner+=1;changed.Entry(8,changed.owner);const auto after=changed.Accepted();Require(before!=after);
    Fixture moved;const auto old=moved.Accepted();
    moved.group.data+=0x1000;moved.entryAddress+=0x1000;
    Put(moved.memory,moved.registry+0x1410548,moved.group);Put(moved.memory,moved.entryAddress,moved.entry);
    Require(old!=moved.Accepted());
    const buffer_read::Pool identity{10,20,30};
    Require(identity!=buffer_read::Pool{10,20,31} && identity!=buffer_read::Pool{10,21,30} &&
        identity!=buffer_read::Pool{11,20,30});
}
void Bounds(){
    Reject([](auto& f){f.game=0;});
    Reject([](auto& f){f.game=UINT64_MAX-0x3bcdcb0+1;});
    Reject([](auto& f){f.source.registry=0;});
    Reject([](auto& f){f.source.registry=UINT64_MAX-0x14105c1+1;
        Put(f.memory,f.game+0x3bcdca8,f.source.registry);});
    Reject([](auto& f){f.source.resource=0;});
    for(const uint32_t handle:{0u,0x100001u,UINT32_MAX})Reject([=](auto& f){f.source.handle=handle;});
    for(const uint64_t bytes:{0ull,292ull,511ull,513ull,512ull*1024+512,UINT64_MAX})
        Reject([=](auto& f){f.resourceBytes=bytes;});
    Reject([](auto& f){f.resourceGpu=0;});
    Reject([](auto& f){f.resourceGpu=UINT64_MAX-f.resourceBytes+1;});
    Reject([](auto& f){f.group.data=UINT64_MAX-3*144-143;Put(f.memory,f.registry+0x1410548,f.group);});
    // The largest handle uses a record offset larger than the fixed registry fields.
    Fixture high(0,0,512,0x100000);
    high.source.registry=UINT64_MAX-0x14105c1;
    Put(high.memory,high.game+0x3bcdca8,high.source.registry);
    Put(high.memory,high.source.registry+0x14105c0,uint8_t(1));high.Rejected();
}
void GeneralLogicalRanges(){
    for(const uint32_t logical:{1u,16u,32u,112u,256u,292u,304u,368u,512u}){
        Fixture fixture(65,3,262144);fixture.source.bytes=logical;fixture.Record(0,logical);
        const auto offset=fixture.source.gpu-fixture.resourceGpu;
        buffer_read::Pool pool{};
        Require(buffer_read::detail::ValidRange(fixture.source,fixture.resourceGpu,fixture.resourceBytes,offset,logical));
        Require(buffer_read::detail::ReadPoolRange(Read,&fixture.memory,fixture.game,fixture.source,
            fixture.resourceGpu,fixture.resourceBytes,offset,logical,pool));
        Require(pool==buffer_read::Pool{fixture.entryAddress,fixture.owner,fixture.allocation});
        Require(fixture.ReadPool(pool)==(logical>=292));
        if(logical>1)Require(buffer_read::detail::ReadPoolRange(Read,&fixture.memory,fixture.game,fixture.source,
            fixture.resourceGpu,fixture.resourceBytes,offset+1,logical-1,pool));
        for(const auto range:{std::array<uint64_t,2>{offset,0},std::array<uint64_t,2>{offset-1,1},
            std::array<uint64_t,2>{offset+logical,1},std::array<uint64_t,2>{offset,logical+1},
            std::array<uint64_t,2>{offset+1,logical},std::array<uint64_t,2>{UINT64_MAX,1},
            std::array<uint64_t,2>{offset,UINT64_MAX}}){
            pool={1,2,3};Require(!buffer_read::detail::ReadPoolRange(Read,&fixture.memory,fixture.game,fixture.source,
                fixture.resourceGpu,fixture.resourceBytes,range[0],range[1],pool) && pool==buffer_read::Pool{});
        }
        // Padded256/512-byte CBVs never enlarge smaller native logical records.
        for(const uint64_t padded:{256ull,512ull})if(padded>logical)
            Require(!buffer_read::detail::ValidRange(fixture.source,fixture.resourceGpu,fixture.resourceBytes,offset,padded));
    }
    Fixture changed;changed.source.bytes=16;changed.Record(0,uint32_t(16));
    const auto offset=changed.source.gpu-changed.resourceGpu;buffer_read::Pool pool;
    changed.Entry(24,uint64_t(0));
    Require(!buffer_read::detail::ReadPoolRange(Read,&changed.memory,changed.game,changed.source,
        changed.resourceGpu,changed.resourceBytes,offset,16,pool));
    Fixture mismatch;mismatch.source.bytes=16; // Native B0 still says292.
    Require(!buffer_read::detail::ReadPoolRange(Read,&mismatch.memory,mismatch.game,mismatch.source,
        mismatch.resourceGpu,mismatch.resourceBytes,mismatch.source.gpu-mismatch.resourceGpu,16,pool));
    Fixture invalid;invalid.source.bytes=0;invalid.Record(0,uint32_t(0));
    Require(!buffer_read::detail::ValidRange(invalid.source,invalid.resourceGpu,invalid.resourceBytes,offset,1));
    invalid.source.bytes=513;invalid.Record(0,uint32_t(513));
    Require(!buffer_read::detail::ValidRange(invalid.source,invalid.resourceGpu,invalid.resourceBytes,offset,1));
    Fixture last(511,0,262144,1);last.source.bytes=368;last.Record(0,uint32_t(368));
    const auto lastOffset=last.source.gpu-last.resourceGpu;
    Require(buffer_read::detail::ReadPoolRange(Read,&last.memory,last.game,last.source,
        last.resourceGpu,last.resourceBytes,lastOffset,368,pool));
    Require(!buffer_read::detail::ValidRange(last.source,last.resourceGpu,last.resourceBytes,lastOffset,512));
}
void ExactRangeProofs(){
    buffer_read::Proof preflight;preflight.receipt=1;preflight.epoch=2;preflight.recording=3;preflight.generation=4;
    preflight.registry=5;preflight.directQueue=6;preflight.pool={7,8,9};preflight.comOffset=65*512;preflight.copyBytes=16;
    Require(buffer_read::detail::SameRangeObservation(preflight,preflight));
    Require(!buffer_read::detail::RegisteredRange(preflight,preflight.comOffset,preflight.copyBytes));
    auto captured=preflight;captured.readRegistered=true;
    Require(buffer_read::detail::SameRangeObservation(preflight,captured));
    Require(buffer_read::detail::RegisteredRange(captured,captured.comOffset,16));
    Require(!buffer_read::detail::RegisteredRange(captured,captured.comOffset,0) &&
        !buffer_read::detail::RegisteredRange(captured,captured.comOffset,256) &&
        !buffer_read::detail::RegisteredRange(captured,captured.comOffset+1,16));
    for(UINT field=0;field<11;++field){auto changed=preflight;
        switch(field){case 0:++changed.receipt;break;case 1:++changed.epoch;break;case 2:++changed.recording;break;
            case 3:++changed.generation;break;case 4:++changed.registry;break;case 5:++changed.directQueue;break;
            case 6:++changed.pool.entry;break;case 7:++changed.pool.owner;break;case 8:++changed.pool.allocation;break;
            case 9:++changed.comOffset;break;case 10:++changed.copyBytes;break;}
        Require(!buffer_read::detail::SameRangeObservation(preflight,changed));
    }
    Require(!buffer_read::detail::SameRangeObservation(buffer_read::Proof{},buffer_read::Proof{}));
    auto history=Uploaded();const auto epoch=history.epoch;history.Written(202);
    Require(!history.Matches(epoch));Submit(history,202);Require(!history.Matches(epoch));
}
}
int main()try{
    UploadEpochs();InvalidUploadHistories();ProducerOrders();InvalidProducerCohorts();ProducerCapacityAndReuse();
    RecordingOwnership();ValidPoolsAndInitialStates();
    PoolIdentityAndLayout();UnreadableAndChangedPools();Bounds();GeneralLogicalRanges();ExactRangeProofs();
    std::puts("PASS native buffer read epochs, uploader ownership, exact292 wrappers, bounded logical pooled ranges and preflight/registered proof separation");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
